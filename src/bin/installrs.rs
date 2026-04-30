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

    /// Enable a user-library cargo feature in the generated installer.
    /// Activates `source!(..., features = [...])` entries gated on the
    /// same name, and enables the matching feature on the user-crate
    /// dependency so `#[cfg(feature = "...")]` code in `install()` /
    /// `uninstall()` is compiled in. Repeatable.
    #[arg(long = "feature", value_name = "NAME", action = clap::ArgAction::Append)]
    features: Vec<String>,

    /// Override a `[package.metadata.installrs]` key. Repeatable. Format:
    /// `key=value`. Use a dotted path (`installer.file-version=1.2.3`,
    /// `uninstaller.file-description=...`) to target the installer or
    /// uninstaller subtable. Values parse as TOML (bool / int /
    /// otherwise string), so `-m gui=true` and `-m language=0x0409`
    /// work. Useful for stamping CI-provided version strings without
    /// touching `Cargo.toml`.
    #[arg(
        long = "metadata",
        short = 'm',
        value_name = "KEY=VALUE",
        action = clap::ArgAction::Append,
    )]
    metadata: Vec<String>,

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
        if triple.contains("windows") && output_file.extension().map(|e| e != "exe").unwrap_or(true)
        {
            output_file.set_extension("exe");
        }
    }

    // Always read the [package.metadata.installrs] config — the builder
    // decides per-target whether to emit Windows resources vs. embed the
    // icon as PNG for the GTK backend.
    let metadata_overrides = parse_metadata_overrides(&cli.metadata)?;

    let (installer_win_resource, uninstaller_win_resource) =
        build::builder::read_win_resource_config(&target, &cli.features, &metadata_overrides)?;

    let gui_enabled = build::builder::read_gui_config(&target, &cli.features, &metadata_overrides)?;
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
        features: cli.features,
    };

    build::builder::build(params)
}

fn parse_override_value(value: &str) -> toml::Value {
    if let Ok(b) = value.parse::<bool>() {
        return toml::Value::Boolean(b);
    }
    let hex = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"));
    if let Some(stripped) = hex {
        if let Ok(n) = i64::from_str_radix(stripped, 16) {
            return toml::Value::Integer(n);
        }
    }
    if let Ok(n) = value.parse::<i64>() {
        return toml::Value::Integer(n);
    }
    toml::Value::String(value.to_string())
}

/// Parse each `--metadata KEY=VALUE` entry into a (path, value) pair.
/// `KEY` is split on `.` for nested overrides; `VALUE` parses as TOML
/// (bool / integer first, falling back to string) so booleans and
/// numeric metadata keys work without quoting.
fn parse_metadata_overrides(raw: &[String]) -> Result<Vec<(Vec<String>, toml::Value)>> {
    let mut out = Vec::with_capacity(raw.len());
    for entry in raw {
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--metadata expects KEY=VALUE, got {entry:?}"))?;
        if key.is_empty() {
            return Err(anyhow::anyhow!("--metadata key is empty in {entry:?}"));
        }
        let path: Vec<String> = key.split('.').map(|s| s.to_string()).collect();
        if path.iter().any(|s| s.is_empty()) {
            return Err(anyhow::anyhow!(
                "--metadata key {key:?} has an empty path segment"
            ));
        }
        let parsed = parse_override_value(value);
        out.push((path, parsed));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::build::builder::CargoManifest;
    use super::build::compress;
    use super::build::scanner;
    use std::io::Read;

    // ── per-feature metadata overlays ────────────────────────────────────────

    fn write_manifest(dir: &std::path::Path, body: &str) {
        let header = "[package]\nname = \"t\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n";
        std::fs::write(dir.join("Cargo.toml"), format!("{header}{body}")).unwrap();
    }

    fn write_manifest_with_version(dir: &std::path::Path, version: &str, body: &str) {
        let header =
            format!("[package]\nname = \"t\"\nversion = \"{version}\"\nedition = \"2021\"\n\n");
        std::fs::write(dir.join("Cargo.toml"), format!("{header}{body}")).unwrap();
    }

    #[test]
    fn file_version_defaults_to_package_version() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest_with_version(
            tmp.path(),
            "1.2.3",
            r#"
[package.metadata.installrs]
product-name = "App"
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        let (installer, uninstaller) = m.win_resource_config(&[], &[]).unwrap();
        let i_v: std::collections::HashMap<_, _> =
            installer.unwrap().version_info.into_iter().collect();
        let u_v: std::collections::HashMap<_, _> =
            uninstaller.unwrap().version_info.into_iter().collect();
        assert_eq!(i_v.get("FileVersion").unwrap(), "1.2.3");
        assert_eq!(i_v.get("ProductVersion").unwrap(), "1.2.3");
        assert_eq!(u_v.get("FileVersion").unwrap(), "1.2.3");
    }

    #[test]
    fn explicit_file_version_overrides_package_version_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest_with_version(
            tmp.path(),
            "1.2.3",
            r#"
[package.metadata.installrs]
file-version = "9.9.9.9"
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        let (installer, _) = m.win_resource_config(&[], &[]).unwrap();
        let v: std::collections::HashMap<_, _> =
            installer.unwrap().version_info.into_iter().collect();
        assert_eq!(v.get("FileVersion").unwrap(), "9.9.9.9");
        // ProductVersion still falls back to package version.
        assert_eq!(v.get("ProductVersion").unwrap(), "1.2.3");
    }

    #[test]
    fn cli_metadata_override_wins_over_base_and_package_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest_with_version(
            tmp.path(),
            "1.2.3",
            r#"
[package.metadata.installrs]
file-version = "1.0.0.0"
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        let overrides: Vec<(Vec<String>, toml::Value)> = vec![(
            vec!["file-version".to_string()],
            toml::Value::String("9.9.9.9".to_string()),
        )];
        let (installer, _) = m.win_resource_config(&[], &overrides).unwrap();
        let v: std::collections::HashMap<_, _> =
            installer.unwrap().version_info.into_iter().collect();
        assert_eq!(v.get("FileVersion").unwrap(), "9.9.9.9");
        // ProductVersion still falls back to package version.
        assert_eq!(v.get("ProductVersion").unwrap(), "1.2.3");
    }

    #[test]
    fn cli_metadata_dotted_path_targets_installer_subtable() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            r#"
[package.metadata.installrs]
file-version = "1.0.0.0"
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        let overrides: Vec<(Vec<String>, toml::Value)> = vec![(
            vec!["installer".to_string(), "file-version".to_string()],
            toml::Value::String("7.7.7.7".to_string()),
        )];
        let (installer, uninstaller) = m.win_resource_config(&[], &overrides).unwrap();
        let i_v: std::collections::HashMap<_, _> =
            installer.unwrap().version_info.into_iter().collect();
        let u_v: std::collections::HashMap<_, _> =
            uninstaller.unwrap().version_info.into_iter().collect();
        // Installer side overridden via dotted path.
        assert_eq!(i_v.get("FileVersion").unwrap(), "7.7.7.7");
        // Uninstaller still inherits the base value.
        assert_eq!(u_v.get("FileVersion").unwrap(), "1.0.0.0");
    }

    #[test]
    fn pack_version_u64_handles_common_shapes() {
        use super::build::builder::pack_version_u64;
        assert_eq!(
            pack_version_u64("1.2.3.4"),
            (1u64 << 48) | (2u64 << 32) | (3u64 << 16) | 4u64
        );
        // Missing parts default to 0.
        assert_eq!(
            pack_version_u64("1.2.3"),
            (1u64 << 48) | (2u64 << 32) | (3u64 << 16)
        );
        assert_eq!(pack_version_u64("1.2"), (1u64 << 48) | (2u64 << 32));
        // Pre-release / build suffixes are stripped before parsing.
        assert_eq!(
            pack_version_u64("1.2.3-rc4"),
            (1u64 << 48) | (2u64 << 32) | (3u64 << 16)
        );
        assert_eq!(
            pack_version_u64("0.1.0-rc12+sha.abc"),
            (0u64 << 48) | (1u64 << 32)
        );
        // Non-numeric segments → 0; leading digits within a segment are kept.
        assert_eq!(pack_version_u64("1.beta.3"), (1u64 << 48) | (3u64 << 16));
        assert_eq!(pack_version_u64(""), 0);
    }

    #[test]
    fn pack_version_u64_handles_calver_with_commit_sha() {
        use super::build::builder::pack_version_u64;
        // CalVer like 2026.04.30 with a git-sha build suffix — the
        // suffix should be stripped, leaving the date components in
        // the FIXEDFILEINFO numeric block.
        assert_eq!(
            pack_version_u64("2026.04.30-abc123def"),
            (2026u64 << 48) | (4u64 << 32) | (30u64 << 16)
        );
        // Year-only CalVer with a 4-digit year still fits in u16.
        assert_eq!(
            pack_version_u64("2026.04.30.7-rc1+sha.deadbeef"),
            (2026u64 << 48) | (4u64 << 32) | (30u64 << 16) | 7u64
        );
    }

    #[test]
    fn cli_metadata_can_toggle_gui() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            r#"
[package.metadata.installrs]
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        assert!(!m.gui_enabled(&[], &[]));
        let overrides: Vec<(Vec<String>, toml::Value)> =
            vec![(vec!["gui".to_string()], toml::Value::Boolean(true))];
        assert!(m.gui_enabled(&[], &overrides));
    }

    #[test]
    fn parse_override_value_distinguishes_types() {
        use super::parse_override_value;
        assert_eq!(parse_override_value("true"), toml::Value::Boolean(true));
        assert_eq!(parse_override_value("false"), toml::Value::Boolean(false));
        assert_eq!(parse_override_value("42"), toml::Value::Integer(42));
        assert_eq!(parse_override_value("0x0409"), toml::Value::Integer(0x0409));
        assert_eq!(
            parse_override_value("1.2.3"),
            toml::Value::String("1.2.3".to_string())
        );
        assert_eq!(
            parse_override_value("My App"),
            toml::Value::String("My App".to_string())
        );
    }

    #[test]
    fn installer_subtable_override_wins_over_package_version_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest_with_version(
            tmp.path(),
            "1.2.3",
            r#"
[package.metadata.installrs.installer]
file-version = "7.0.0.0"
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        let (installer, uninstaller) = m.win_resource_config(&[], &[]).unwrap();
        let i_v: std::collections::HashMap<_, _> =
            installer.unwrap().version_info.into_iter().collect();
        let u_v: std::collections::HashMap<_, _> =
            uninstaller.unwrap().version_info.into_iter().collect();
        assert_eq!(i_v.get("FileVersion").unwrap(), "7.0.0.0");
        // Uninstaller has no override, so it gets the package fallback.
        assert_eq!(u_v.get("FileVersion").unwrap(), "1.2.3");
    }

    #[test]
    fn feature_overlay_overrides_base_scalars() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            r#"
[package.metadata.installrs]
product-name = "Base"
file-version = "1.0.0.0"

[package.metadata.installrs.feature.pro]
product-name = "Pro"
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        let (installer, _) = m.win_resource_config(&["pro".to_string()], &[]).unwrap();
        let v = installer.unwrap().version_info;
        let by_key: std::collections::HashMap<_, _> = v.into_iter().collect();
        assert_eq!(by_key.get("ProductName").unwrap(), "Pro");
        assert_eq!(by_key.get("FileVersion").unwrap(), "1.0.0.0");
    }

    #[test]
    fn feature_overlay_inactive_uses_base() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            r#"
[package.metadata.installrs]
product-name = "Base"

[package.metadata.installrs.feature.pro]
product-name = "Pro"
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        let (installer, _) = m.win_resource_config(&[], &[]).unwrap();
        let v: std::collections::HashMap<_, _> =
            installer.unwrap().version_info.into_iter().collect();
        assert_eq!(v.get("ProductName").unwrap(), "Base");
    }

    #[test]
    fn feature_overlay_last_wins_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            r#"
[package.metadata.installrs]
product-name = "Base"

[package.metadata.installrs.feature.a]
product-name = "A"

[package.metadata.installrs.feature.b]
product-name = "B"
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        let (installer, _) = m
            .win_resource_config(&["a".to_string(), "b".to_string()], &[])
            .unwrap();
        let v: std::collections::HashMap<_, _> =
            installer.unwrap().version_info.into_iter().collect();
        assert_eq!(v.get("ProductName").unwrap(), "B");
    }

    #[test]
    fn feature_overlay_merges_installer_subtable_keywise() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            r#"
[package.metadata.installrs]
product-name = "App"

[package.metadata.installrs.installer]
file-description = "App Installer"
internal-name = "app-installer"

[package.metadata.installrs.feature.pro.installer]
file-description = "App Pro Installer"
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        let (installer, _) = m.win_resource_config(&["pro".to_string()], &[]).unwrap();
        let v: std::collections::HashMap<_, _> =
            installer.unwrap().version_info.into_iter().collect();
        // Overridden by the feature overlay.
        assert_eq!(v.get("FileDescription").unwrap(), "App Pro Installer");
        // Inherited from the base installer subtable — not wiped.
        assert_eq!(v.get("InternalName").unwrap(), "app-installer");
    }

    #[test]
    fn feature_overlay_installer_works_without_base_installer_subtable() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            r#"
[package.metadata.installrs]
product-name = "App"

[package.metadata.installrs.feature.pro.installer]
file-description = "App Pro Installer"
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        let (installer, _) = m.win_resource_config(&["pro".to_string()], &[]).unwrap();
        let v: std::collections::HashMap<_, _> =
            installer.unwrap().version_info.into_iter().collect();
        assert_eq!(v.get("ProductName").unwrap(), "App");
        assert_eq!(v.get("FileDescription").unwrap(), "App Pro Installer");
    }

    #[test]
    fn feature_overlay_uninstaller_subtable_isolated_from_installer() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            r#"
[package.metadata.installrs]
product-name = "App"

[package.metadata.installrs.feature.pro.uninstaller]
file-description = "App Pro Uninstaller"
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        let (installer, uninstaller) = m.win_resource_config(&["pro".to_string()], &[]).unwrap();
        let i_v: std::collections::HashMap<_, _> =
            installer.unwrap().version_info.into_iter().collect();
        let u_v: std::collections::HashMap<_, _> =
            uninstaller.unwrap().version_info.into_iter().collect();
        // Installer keeps the base ProductName and gets no FileDescription.
        assert_eq!(i_v.get("ProductName").unwrap(), "App");
        assert!(!i_v.contains_key("FileDescription"));
        // Uninstaller picks up the feature override.
        assert_eq!(u_v.get("FileDescription").unwrap(), "App Pro Uninstaller");
    }

    #[test]
    fn feature_overlay_can_enable_gui() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            r#"
[package.metadata.installrs]

[package.metadata.installrs.feature.gui]
gui = true
"#,
        );
        let m = CargoManifest::load(tmp.path()).unwrap();
        assert!(!m.gui_enabled(&[], &[]));
        assert!(m.gui_enabled(&["gui".to_string()], &[]));
    }

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

    fn has_path(list: &[scanner::SourceRef], path: &str) -> bool {
        list.iter().any(|r| r.path == path)
    }

    #[test]
    fn scanner_detects_source_macro() {
        let r = scan_str(
            r#"fn install(i: &mut T) { i.file(installrs::source!("cfg.toml"), "dst").install().unwrap(); }"#,
        );
        assert!(has_path(&r.install_sources, "cfg.toml"));
    }

    #[test]
    fn scanner_detects_unqualified_source_macro() {
        let r = scan_str(r#"fn install(i: &mut T) { i.file(source!("data.txt"), "dst"); }"#);
        assert!(has_path(&r.install_sources, "data.txt"));
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
                .filter(|p| p.path == "x.txt")
                .count(),
            1
        );
    }

    #[test]
    fn scanner_source_nested_path() {
        let r =
            scan_str(r#"fn install(i: &mut T) { i.file(source!("vendor/lib.so"), "lib.so"); }"#);
        assert!(
            has_path(&r.install_sources, "vendor/lib.so"),
            "got: {:?}",
            r.install_sources
        );
    }

    #[test]
    fn scanner_source_in_dir_call() {
        // Builder determines file-vs-dir from filesystem; scanner just collects paths.
        let r = scan_str(r#"fn install(i: &mut T) { i.dir(source!("assets/icons"), "icons"); }"#);
        assert!(
            has_path(&r.install_sources, "assets/icons"),
            "got: {:?}",
            r.install_sources
        );
    }

    // ── scanner: function-scoped macro detection ─────────────────────────────

    #[test]
    fn scanner_source_in_uninstall_goes_to_uninstall() {
        let r = scan_str(r#"fn uninstall(i: &mut T) { i.file(source!("cleanup.sh"), "dst"); }"#);
        assert!(has_path(&r.uninstall_sources, "cleanup.sh"));
        assert!(r.install_sources.is_empty());
    }

    #[test]
    fn scanner_source_outside_install_uninstall_goes_to_both() {
        let r = scan_str(r#"fn helper(i: &mut T) { i.file(source!("shared.dat"), "dst"); }"#);
        assert!(has_path(&r.install_sources, "shared.dat"));
        assert!(has_path(&r.uninstall_sources, "shared.dat"));
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
        assert_eq!(r.install_sources.len(), 1);
        assert_eq!(r.install_sources[0].path, "real.txt");
    }

    // ── scanner: source! options ─────────────────────────────────────────────

    #[test]
    fn scanner_parses_ignore_option() {
        let r = scan_str(
            r#"fn install(i: &mut T) { i.dir(source!("assets", ignore = ["*.bak", "scratch"]), "dst"); }"#,
        );
        let s = r
            .install_sources
            .iter()
            .find(|s| s.path == "assets")
            .expect("missing assets source");
        assert_eq!(s.ignore, vec!["*.bak".to_string(), "scratch".to_string()]);
    }

    #[test]
    fn scanner_parses_features_option() {
        let r = scan_str(
            r#"fn install(i: &mut T) { i.file(source!("docs.tar", features = ["docs", "full"]), "dst"); }"#,
        );
        let s = r
            .install_sources
            .iter()
            .find(|s| s.path == "docs.tar")
            .expect("missing docs.tar source");
        assert_eq!(s.features, vec!["docs".to_string(), "full".to_string()]);
    }

    #[test]
    fn scanner_empty_features_wins_over_gated_repeat() {
        let r = scan_str(
            r#"fn install(i: &mut T) {
                i.file(source!("x.dat", features = ["pro"]), "a");
                i.file(source!("x.dat"), "b");
            }"#,
        );
        let s = r
            .install_sources
            .iter()
            .find(|s| s.path == "x.dat")
            .unwrap();
        assert!(
            s.features.is_empty(),
            "unconditional reference should clear feature gates, got {:?}",
            s.features
        );
    }

    #[test]
    fn scanner_unions_features_across_invocations() {
        let r = scan_str(
            r#"fn install(i: &mut T) {
                i.file(source!("x.dat", features = ["a"]), "1");
                i.file(source!("x.dat", features = ["b"]), "2");
            }"#,
        );
        let s = r
            .install_sources
            .iter()
            .find(|s| s.path == "x.dat")
            .unwrap();
        assert!(s.features.contains(&"a".to_string()));
        assert!(s.features.contains(&"b".to_string()));
    }

    #[test]
    fn scanner_merges_ignore_across_invocations() {
        let r = scan_str(
            r#"fn install(i: &mut T) {
                i.dir(source!("assets", ignore = ["*.bak"]), "a");
                i.dir(source!("assets", ignore = ["scratch"]), "b");
            }"#,
        );
        let s = r
            .install_sources
            .iter()
            .find(|s| s.path == "assets")
            .unwrap();
        assert!(s.ignore.contains(&"*.bak".to_string()));
        assert!(s.ignore.contains(&"scratch".to_string()));
    }
}
