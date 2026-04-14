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

    /// Enable debug output
    #[arg(long, short)]
    verbose: bool,

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
    } else if cli.verbose {
        "debug"
    } else {
        "info"
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

    let params = build::builder::BuildParams {
        target_dir: target,
        build_dir,
        output_file: cli.output,
        compression: cli.compression,
        ignore_patterns,
        target_triple: cli.target_triple,
    };

    build::builder::build(&params)
}

#[cfg(test)]
mod tests {
    use super::build::compress;
    use super::build::scanner;
    use std::io::Read;

    // ── compress::validate_method ─────────────────────────────────────────────

    #[test]
    fn validate_accepts_lzma() { compress::validate_method("lzma").unwrap(); }

    #[test]
    fn validate_accepts_gzip() { compress::validate_method("gzip").unwrap(); }

    #[test]
    fn validate_accepts_bzip2() { compress::validate_method("bzip2").unwrap(); }

    #[test]
    fn validate_accepts_none() { compress::validate_method("none").unwrap(); }

    #[test]
    fn validate_rejects_unknown() {
        assert!(compress::validate_method("zstd").unwrap_err()
            .to_string().contains("unsupported"));
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
        flate2::read::GzDecoder::new(compressed.as_slice()).read_to_end(&mut out).unwrap();
        assert_eq!(out, SAMPLE);
    }

    #[test]
    fn compress_bzip2_roundtrip() {
        let compressed = compress::compress(SAMPLE, "bzip2").unwrap();
        let mut out = Vec::new();
        bzip2::read::BzDecoder::new(compressed.as_slice()).read_to_end(&mut out).unwrap();
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

    // ── scanner: file!/dir! macro detection ──────────────────────────────────

    #[test]
    fn scanner_detects_file_macro() {
        let r = scan_str(r#"fn f(i: &mut T) { installrs::file!(i, "cfg.toml", "dst").unwrap(); }"#);
        assert!(r.included_files.contains(&"cfg.toml".to_string()));
        assert!(r.included_dirs.is_empty());
    }

    #[test]
    fn scanner_detects_dir_macro() {
        let r = scan_str(r#"fn f(i: &mut T) { installrs::dir!(i, "assets", "out").unwrap(); }"#);
        assert!(r.included_dirs.contains(&"assets".to_string()));
        assert!(r.included_files.is_empty());
    }

    #[test]
    fn scanner_detects_unqualified_file_macro() {
        let r = scan_str(r#"fn f(i: &mut T) { file!(i, "data.txt", "dst")?; }"#);
        assert!(r.included_files.contains(&"data.txt".to_string()));
    }

    #[test]
    fn scanner_detects_unqualified_dir_macro() {
        let r = scan_str(r#"fn f(i: &mut T) { dir!(i, "res", "dst")?; }"#);
        assert!(r.included_dirs.contains(&"res".to_string()));
    }

    #[test]
    fn scanner_no_duplicate_files() {
        let r = scan_str(r#"fn f(i: &mut T) { file!(i, "x.txt", "a")?; file!(i, "x.txt", "b")?; }"#);
        assert_eq!(r.included_files.iter().filter(|p| p.as_str() == "x.txt").count(), 1);
    }

    #[test]
    fn scanner_no_duplicate_dirs() {
        let r = scan_str(r#"fn f(i: &mut T) { dir!(i, "d", "a")?; dir!(i, "d", "b")?; }"#);
        assert_eq!(r.included_dirs.iter().filter(|p| p.as_str() == "d").count(), 1);
    }

    #[test]
    fn scanner_file_macro_nested_path() {
        let r = scan_str(r#"fn f(i: &mut T) { file!(i, "vendor/lib.so", "lib.so")?; }"#);
        assert!(r.included_files.contains(&"vendor/lib.so".to_string()),
            "got: {:?}", r.included_files);
    }

    #[test]
    fn scanner_dir_macro_nested_path() {
        let r = scan_str(r#"fn f(i: &mut T) { dir!(i, "assets/icons", "icons")?; }"#);
        assert!(r.included_dirs.contains(&"assets/icons".to_string()),
            "got: {:?}", r.included_dirs);
    }
}
