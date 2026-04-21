#[path = "../build/mod.rs"]
mod build;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "installrs",
    about = "Build self-contained installer executables"
)]
struct Cli {
    /// Directory containing the installer source crate
    #[arg(long, default_value = ".")]
    target: PathBuf,

    /// Output installer file path
    #[arg(long, short, default_value = "./installer")]
    output: PathBuf,

    /// Compression method: lzma, gzip, bzip2, or none
    #[arg(long, default_value = "lzma")]
    compression: String,

    /// Comma-separated glob patterns to ignore when including directories
    #[arg(long, default_value = ".git,.svn,node_modules")]
    ignore: String,

    /// Rust target triple for cross-compilation (e.g. x86_64-pc-windows-gnu)
    #[arg(long)]
    target_triple: Option<String>,

    /// Enable debug output (-v) or trace output (-vv)
    #[arg(long, short, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Suppress non-error output
    #[arg(long, short)]
    quiet: bool,

    /// Suppress all output
    #[arg(long, short)]
    silent: bool,
}

fn main() {
    let cli = Cli::parse();

    let log_level = if cli.silent {
        "off"
    } else if cli.quiet {
        "error"
    } else {
        match cli.verbose {
            0 => "info",
            1 => "debug",
            _ => "trace",
        }
    };

    env_logger::Builder::new()
        .filter_level(log_level.parse().unwrap())
        .format_timestamp(None)
        .format_target(false)
        .init();

    if let Err(e) = run(cli) {
        log::error!("{e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let target = cli
        .target
        .canonicalize()
        .unwrap_or_else(|_| cli.target.clone());

    let build_dir = target.join("build");

    let ignore_patterns: Vec<String> = cli
        .ignore
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let mut output_file = cli.output;
    if let Some(ref triple) = cli.target_triple {
        if triple.contains("windows") && output_file.extension().is_none_or(|e| e != "exe") {
            output_file.set_extension("exe");
        }
    }

    // Always read the [package.metadata.installrs] config — the builder
    // decides per-target whether to emit Windows resources vs. embed the
    // icon as PNG for the GTK backend.
    let (installer_win_resource, uninstaller_win_resource) =
        build::builder::read_win_resource_config(&target)?;

    let gui_enabled = build::builder::read_gui_config(&target)?;
    if gui_enabled {
        log::info!("GUI support enabled");
    }

    let params = build::builder::BuildParams {
        target_dir: target,
        build_dir,
        output_file,
        compression: cli.compression,
        ignore_patterns,
        target_triple: cli.target_triple,
        verbosity: cli.verbose,
        installer_win_resource,
        uninstaller_win_resource,
        gui_enabled,
    };

    build::builder::build(params)
}

#[cfg(test)]
mod tests {
    use super::build::compress;
    use super::build::scanner;
    use std::io::Read;

    // ── compress::validate_method ─────────────────────────────────────────────

    #[test]
    fn validate_accepts_lzma() {
        compress::validate_method("lzma").unwrap();
    }

    #[test]
    fn validate_accepts_gzip() {
        compress::validate_method("gzip").unwrap();
    }

    #[test]
    fn validate_accepts_bzip2() {
        compress::validate_method("bzip2").unwrap();
    }

    #[test]
    fn validate_accepts_none() {
        compress::validate_method("none").unwrap();
    }

    #[test]
    fn validate_rejects_unknown() {
        assert!(compress::validate_method("zstd")
            .unwrap_err()
            .to_string()
            .contains("unsupported"));
    }

    #[test]
    fn validate_rejects_empty_string() {
        // "" is not in the accepted set; only the explicit string "none" is.
        assert!(compress::validate_method("").is_err());
    }

    // ── compress round-trips ──────────────────────────────────────────────────

    const SAMPLE: &[u8] = b"round-trip test data for compression algorithms";

    #[test]
    fn compress_none_is_passthrough() {
        assert_eq!(compress::compress(SAMPLE, "none").unwrap(), SAMPLE);
    }

    #[test]
    fn compress_empty_is_passthrough() {
        assert_eq!(compress::compress(SAMPLE, "").unwrap(), SAMPLE);
    }

    #[test]
    fn compress_lzma_roundtrip() {
        let compressed = compress::compress(SAMPLE, "lzma").unwrap();
        let mut out = Vec::new();
        lzma_rs::lzma_decompress(&mut std::io::Cursor::new(&compressed), &mut out).unwrap();
        assert_eq!(out, SAMPLE);
    }

    #[test]
    fn compress_gzip_roundtrip() {
        let compressed = compress::compress(SAMPLE, "gzip").unwrap();
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(compressed.as_slice())
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, SAMPLE);
    }

    #[test]
    fn compress_bzip2_roundtrip() {
        let compressed = compress::compress(SAMPLE, "bzip2").unwrap();
        let mut out = Vec::new();
        bzip2::read::BzDecoder::new(compressed.as_slice())
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, SAMPLE);
    }

    #[test]
    fn compress_unknown_errors() {
        assert!(compress::compress(SAMPLE, "zstd").is_err());
    }

    // ── scanner helper ────────────────────────────────────────────────────────

    fn scan_str(source: &str) -> scanner::ScanResult {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("lib.rs"), source).unwrap();
        scanner::scan_source_dir(tmp.path()).unwrap()
    }

    // ── scanner: function detection ───────────────────────────────────────────

    #[test]
    fn scanner_detects_install_fn() {
        let r = scan_str("pub fn install(i: &mut T) -> R { Ok(()) }");
        assert!(r.has_install_fn);
        assert!(!r.has_uninstall_fn);
    }

    #[test]
    fn scanner_detects_uninstall_fn() {
        let r = scan_str("pub fn uninstall(i: &mut T) -> R { Ok(()) }");
        assert!(!r.has_install_fn);
        assert!(r.has_uninstall_fn);
    }

    #[test]
    fn scanner_detects_both_fns() {
        let r = scan_str("pub fn install() {} pub fn uninstall() {}");
        assert!(r.has_install_fn);
        assert!(r.has_uninstall_fn);
    }

    #[test]
    fn scanner_detects_neither_fn() {
        let r = scan_str("fn helper() {}");
        assert!(!r.has_install_fn);
        assert!(!r.has_uninstall_fn);
    }

    // ── scanner: source! macro detection ─────────────────────────────────────

    #[test]
    fn scanner_detects_source_macro() {
        let r = scan_str(
            r#"fn install(i: &mut T) { i.file(installrs::source!("cfg.toml"), "dst").install().unwrap(); }"#,
        );
        assert!(r.install_sources.contains(&"cfg.toml".to_string()));
    }

    #[test]
    fn scanner_detects_unqualified_source_macro() {
        let r = scan_str(r#"fn install(i: &mut T) { i.file(source!("data.txt"), "dst"); }"#);
        assert!(r.install_sources.contains(&"data.txt".to_string()));
    }

    #[test]
    fn scanner_no_duplicate_sources() {
        let r = scan_str(
            r#"fn install(i: &mut T) {
                i.file(source!("x.txt"), "a");
                i.file(source!("x.txt"), "b");
            }"#,
        );
        assert_eq!(
            r.install_sources
                .iter()
                .filter(|p| p.as_str() == "x.txt")
                .count(),
            1
        );
    }

    #[test]
    fn scanner_source_nested_path() {
        let r =
            scan_str(r#"fn install(i: &mut T) { i.file(source!("vendor/lib.so"), "lib.so"); }"#);
        assert!(
            r.install_sources.contains(&"vendor/lib.so".to_string()),
            "got: {:?}",
            r.install_sources
        );
    }

    #[test]
    fn scanner_source_in_dir_call() {
        // Builder determines file-vs-dir from filesystem; scanner just collects paths.
        let r = scan_str(r#"fn install(i: &mut T) { i.dir(source!("assets/icons"), "icons"); }"#);
        assert!(
            r.install_sources.contains(&"assets/icons".to_string()),
            "got: {:?}",
            r.install_sources
        );
    }

    // ── scanner: function-scoped macro detection ─────────────────────────────

    #[test]
    fn scanner_source_in_uninstall_goes_to_uninstall() {
        let r = scan_str(r#"fn uninstall(i: &mut T) { i.file(source!("cleanup.sh"), "dst"); }"#);
        assert!(r.uninstall_sources.contains(&"cleanup.sh".to_string()));
        assert!(r.install_sources.is_empty());
    }

    #[test]
    fn scanner_source_outside_install_uninstall_goes_to_both() {
        let r = scan_str(r#"fn helper(i: &mut T) { i.file(source!("shared.dat"), "dst"); }"#);
        assert!(r.install_sources.contains(&"shared.dat".to_string()));
        assert!(r.uninstall_sources.contains(&"shared.dat".to_string()));
    }

    #[test]
    fn scanner_ignores_non_source_macros() {
        let r = scan_str(
            r#"fn install(i: &mut T) {
                println!("{}", "nope.txt");
                format!("also-ignored.txt");
                i.file(source!("real.txt"), "dst");
            }"#,
        );
        assert_eq!(r.install_sources, vec!["real.txt".to_string()]);
    }
}
