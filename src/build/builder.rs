use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};

use super::compress;
use super::ico_convert;
use super::scanner;

/// The crates.io version this CLI was built from. Generated installer /
/// uninstaller crates pin to this exact version so binaries built by a
/// given `installrs` release always compile against a matching runtime.
const INSTALLRS_CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Schema version of the `ENTRIES` / `DirChild` table the CLI emits.
/// Comes from the runtime's `__private::ENTRIES_VERSION`, so the CLI
/// can never drift from its own runtime; the generated code asserts
/// this matches the runtime it links against, catching cross-version
/// mismatches at compile time of the generated crate.
const GENERATED_ENTRIES_VERSION: u32 = installrs::__private::ENTRIES_VERSION;

/// Render the `installrs` dependency spec for the generated `Cargo.toml`.
///
/// `local_path = Some(p)` emits `installrs = { path = "<p>" }` so the
/// generated crate picks up a local checkout instead of crates.io —
/// used by InstallRS-on-InstallRS development (the in-repo CI,
/// integration tests, and the release script). The path is supplied
/// via the `--installrs-path` CLI flag. `None` (the default) emits
/// `installrs = "=<CLI version>"` from crates.io, the exact-version
/// pin guaranteeing the runtime matches the CLI that built the
/// installer.
/// Compare the `installrs` version req declared in the user's installer
/// crate's `Cargo.toml` against this CLI's version. Errors if they're
/// semver-incompatible — catches the "user's crate says `0.3`, CLI is
/// `0.4`" mismatch here, at `installrs --target ...` time, instead of
/// letting it surface as a cryptic `expected Installer, found Installer`
/// type error deep in cargo's downstream compile.
///
/// Silent pass-through cases:
/// - No `installrs` dep in the user's `Cargo.toml` (rare — they'd have
///   nothing to call, but we don't force a dependency).
/// - Dep is `{ path = ... }` or `{ git = ... }` without a version req — the
///   user is pointing at a specific source on disk or a ref, and the usual
///   crates.io version check doesn't apply.
fn check_installrs_version_compat(manifest: &CargoManifest) -> Result<()> {
    let user_req_str = match manifest.installrs_dep_version_req() {
        Some(s) => s,
        None => return Ok(()),
    };

    let user_req: semver::VersionReq = user_req_str.parse().with_context(|| {
        format!("invalid `installrs` version requirement in Cargo.toml: {user_req_str:?}")
    })?;
    let cli_version: semver::Version = INSTALLRS_CRATE_VERSION
        .parse()
        .context("failed to parse CLI version (compile-time bug)")?;

    if !user_req.matches(&cli_version) {
        return Err(anyhow!(
            "`installrs` version mismatch: your installer crate's Cargo.toml declares \
             `installrs = \"{user_req_str}\"`, but this CLI is {cli_version}. Update the \
             version requirement in your Cargo.toml to include {cli_version}, or install \
             a matching CLI with `cargo install installrs@{user_req_str}`."
        ));
    }
    Ok(())
}

/// Parsed-once view of the user's installer crate `Cargo.toml`. All the
/// builder-side code that needs to read fields from it (package info,
/// `[package.metadata.installrs]` config, `installrs` version req) goes
/// through methods on this type so the file is only parsed once per
/// build invocation.
pub struct CargoManifest {
    raw: toml::Value,
    target_dir: PathBuf,
}

impl CargoManifest {
    pub fn load(target_dir: &Path) -> Result<Self> {
        let cargo_toml_path = target_dir.join("Cargo.toml");
        let content = std::fs::read_to_string(&cargo_toml_path)
            .with_context(|| format!("failed to read {}", cargo_toml_path.display()))?;
        let raw: toml::Value = content.parse().context("failed to parse Cargo.toml")?;
        Ok(Self {
            raw,
            target_dir: target_dir.to_path_buf(),
        })
    }

    /// The `installrs` dep's version requirement string, if declared and
    /// a version key is present. Returns `None` for path/git-only deps
    /// or when the dep is absent — callers treat those as silent passes.
    fn installrs_dep_version_req(&self) -> Option<String> {
        self.raw
            .get("dependencies")
            .and_then(|d| d.get("installrs"))
            .and_then(|dep| match dep {
                toml::Value::String(s) => Some(s.clone()),
                toml::Value::Table(t) => t
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                _ => None,
            })
    }

    /// `[package].version`, if declared. Used as the default value for
    /// `file-version` / `product-version` when those keys aren't set
    /// explicitly in `[package.metadata.installrs]`.
    fn package_version(&self) -> Option<String> {
        self.raw
            .get("package")
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Returns (package_name, lib_crate_name, lib_path).
    fn package_info(&self) -> Result<(String, String, PathBuf)> {
        let package_name = self
            .raw
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .ok_or_else(|| anyhow!("could not find [package].name in Cargo.toml"))?
            .to_string();
        let lib = self.raw.get("lib");
        let lib_crate_name = lib
            .and_then(|l| l.get("name"))
            .and_then(|n| n.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| package_name.replace('-', "_"));
        let lib_path = lib
            .and_then(|l| l.get("path"))
            .and_then(|p| p.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("src/lib.rs"));
        Ok((package_name, lib_crate_name, lib_path))
    }

    /// `[package.metadata.installrs]` subtree, if present.
    fn installrs_meta(&self) -> Option<&toml::Value> {
        self.raw
            .get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("installrs"))
    }

    /// `[package.metadata.installrs]` with `[…feature.<name>]` overlays
    /// for each active feature merged on top, in CLI argument order
    /// (later wins), then any CLI `--metadata KEY=VALUE` overrides
    /// applied last. Top-level keys are replaced wholesale, except for
    /// the `installer` and `uninstaller` subtables — those are merged
    /// key-by-key, so `[…feature.pro.installer]` overrides individual
    /// installer fields without forcing the user to restate the whole
    /// subtable. The `feature` subtable itself is stripped from the
    /// returned value so downstream parsers don't see it.
    fn installrs_meta_merged(
        &self,
        features: &[String],
        cli_overrides: &[(Vec<String>, toml::Value)],
    ) -> Option<toml::Value> {
        let base = self.installrs_meta()?;
        let mut merged = base.clone();
        let merged_table = match &mut merged {
            toml::Value::Table(t) => t,
            _ => return Some(merged),
        };
        for f in features {
            if let Some(toml::Value::Table(overlay)) = base.get("feature").and_then(|fs| fs.get(f))
            {
                for (k, v) in overlay {
                    if k == "installer" || k == "uninstaller" {
                        if let toml::Value::Table(overlay_sub) = v {
                            if let Some(toml::Value::Table(existing)) = merged_table.get_mut(k) {
                                for (k2, v2) in overlay_sub {
                                    existing.insert(k2.clone(), v2.clone());
                                }
                                continue;
                            }
                        }
                    }
                    merged_table.insert(k.clone(), v.clone());
                }
            }
        }
        merged_table.remove("feature");

        // Apply CLI --metadata overrides last so they always win.
        for (path, value) in cli_overrides {
            insert_at_path(merged_table, path, value.clone());
        }

        Some(merged)
    }

    /// Read `gui = true` from `[package.metadata.installrs]`, with
    /// `[…feature.<name>]` overlays applied for each active feature.
    pub fn gui_enabled(
        &self,
        features: &[String],
        cli_overrides: &[(Vec<String>, toml::Value)],
    ) -> bool {
        self.installrs_meta_merged(features, cli_overrides)
            .as_ref()
            .and_then(|i| i.get("gui"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Parse installer and uninstaller Windows resource configs.
    ///
    /// Base keys in `[package.metadata.installrs]` apply to both. Keys in
    /// `[package.metadata.installrs.installer]` or `…uninstaller` override
    /// the base. `[…feature.<name>]` subtables are merged onto the base
    /// (in `features` order) before installer/uninstaller overrides.
    pub fn win_resource_config(
        &self,
        features: &[String],
        cli_overrides: &[(Vec<String>, toml::Value)],
    ) -> Result<(Option<WinResourceConfig>, Option<WinResourceConfig>)> {
        let meta = match self.installrs_meta_merged(features, cli_overrides) {
            Some(v) => v,
            None => return Ok((None, None)),
        };

        let mut base = parse_win_resource_table(&meta, &self.target_dir)?;
        // Default file-version / product-version to [package].version so
        // bumping the user crate's Cargo.toml automatically restamps the
        // installer without an explicit metadata entry. Only fills keys
        // that aren't already set; installer/uninstaller subtable
        // overrides applied below still win.
        if let Some(pkg_ver) = self.package_version() {
            for win_key in ["FileVersion", "ProductVersion"] {
                if !base.version_info.iter().any(|(k, _)| k == win_key) {
                    base.version_info
                        .push((win_key.to_string(), pkg_ver.clone()));
                }
            }
        }

        let installer = if let Some(sub) = meta.get("installer") {
            let overrides = parse_win_resource_table(sub, &self.target_dir)?;
            merge_win_resource_config(&base, &overrides)
        } else {
            base.clone()
        };

        let uninstaller = if let Some(sub) = meta.get("uninstaller") {
            let overrides = parse_win_resource_table(sub, &self.target_dir)?;
            merge_win_resource_config(&base, &overrides)
        } else {
            base
        };

        Ok((Some(installer), Some(uninstaller)))
    }
}

fn installrs_dep_spec(features_suffix: &str, local_path: Option<&Path>) -> String {
    if let Some(p) = local_path {
        format!(
            "installrs = {{ path = {path:?}{features_suffix} }}",
            path = p.display().to_string(),
        )
    } else {
        // Exact-version pin (`=X.Y.Z`) — the generated crate compiles against
        // precisely the runtime the CLI was built from, not just any
        // semver-compatible release. Removes the "latest 0.3.x at build time"
        // variance in exchange for requiring a CLI reinstall to pick up
        // runtime bug fixes.
        format!(
            "installrs = {{ version = \"={version}\"{features_suffix} }}",
            version = INSTALLRS_CRATE_VERSION,
        )
    }
}

/// Pack a "1.2.3.4"-style version string into the u64 layout that
/// `winresource::VersionInfo::{FILEVERSION, PRODUCTVERSION}` expects:
/// `(major << 48) | (minor << 32) | (patch << 16) | build`. Tolerant
/// of fewer than four parts (missing components default to 0), of
/// pre-release suffixes (`1.2.3-rc4` → `1.2.3.0`), and of non-numeric
/// segments (treated as 0). The string field is set separately and is
/// unaffected by this normalization.
pub fn pack_version_u64(version: &str) -> u64 {
    // Strip pre-release / build metadata so "1.2.3-rc4+sha.abc" → "1.2.3".
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let mut parts: [u64; 4] = [0; 4];
    for (i, segment) in core.split('.').take(4).enumerate() {
        // Take leading digits only — handles "1rc" or "1_dev" gracefully.
        let digits: String = segment.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u64>() {
            parts[i] = n.min(0xFFFF);
        }
    }
    (parts[0] << 48) | (parts[1] << 32) | (parts[2] << 16) | parts[3]
}

/// Insert `value` at the given dotted-key `path` inside `table`,
/// creating intermediate sub-tables as needed. Used to apply CLI
/// `--metadata KEY.SUBKEY=VALUE` overrides on top of the merged
/// `[package.metadata.installrs]` table. Non-table intermediates are
/// replaced with fresh tables so the path can be walked.
fn insert_at_path(
    table: &mut toml::map::Map<String, toml::Value>,
    path: &[String],
    value: toml::Value,
) {
    let Some((last, rest)) = path.split_last() else {
        return;
    };
    let mut cursor = table;
    for segment in rest {
        let entry = cursor
            .entry(segment.clone())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if !matches!(entry, toml::Value::Table(_)) {
            *entry = toml::Value::Table(toml::map::Map::new());
        }
        cursor = match entry {
            toml::Value::Table(t) => t,
            _ => unreachable!(),
        };
    }
    cursor.insert(last.clone(), value);
}

/// Format a ", features = [\"a\", \"b\"]" suffix for the user-crate dep,
/// or "" when empty.
fn format_user_features(features: &[String]) -> String {
    if features.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = features.iter().map(|f| format!("{f:?}")).collect();
        format!(", features = [{}]", list.join(", "))
    }
}

/// Write a file only if its content has changed, preserving mtime for build caching.
fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    if path.exists() {
        if let Ok(existing) = std::fs::read_to_string(path) {
            if existing == content {
                log::trace!("Unchanged, skipping write: {}", path.display());
                return Ok(());
            }
        }
    }
    log::debug!("Writing: {}", path.display());
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

/// FNV-1a 64-bit hash — must stay identical to the copy in installrs/src/lib.rs.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

#[derive(Clone)]
pub enum ManifestConfig {
    /// Path to an external .manifest file
    File(PathBuf),
    /// Raw XML string
    Raw(String),
    /// Structured config — XML is generated automatically
    Generated {
        execution_level: String,
        dpi_aware: Option<String>,
        long_path_aware: Option<bool>,
        supported_os: Vec<String>,
    },
}

#[derive(Clone)]
pub struct WinResourceConfig {
    pub icon: Option<PathBuf>,
    pub icon_sizes: Vec<u32>,
    pub manifest: Option<ManifestConfig>,
    pub language: Option<u16>,
    /// `"console"`, `"windows"`, or `"auto"` (resolved at build time).
    pub windows_subsystem: String,
    pub version_info: Vec<(String, String)>,
}

pub struct BuildParams {
    pub target_dir: PathBuf,
    pub build_dir: PathBuf,
    pub output_file: PathBuf,
    pub compression: String,
    pub ignore_patterns: Vec<String>,
    pub target_triple: Option<String>,
    /// 0 = normal (quiet cargo), 1 = debug (cargo default), 2+ = trace (cargo -vv)
    pub verbosity: u8,
    pub installer_win_resource: Option<WinResourceConfig>,
    pub uninstaller_win_resource: Option<WinResourceConfig>,
    pub gui_enabled: bool,
    /// User-library cargo features to enable. Gates `source!(..., features
    /// = [...])` entries and is passed through as the user-crate
    /// dependency's `features = [...]` list in the generated crates.
    pub features: Vec<String>,
    /// If `Some`, generated `Cargo.toml`s depend on `installrs` via a
    /// `path = "<this>"` entry instead of the crates.io version pin.
    /// Used by InstallRS-on-InstallRS development (CI, integration
    /// tests, the release script). Set via `--installrs-path`.
    pub installrs_local_path: Option<PathBuf>,
}

/// A source is active if it has no feature gates, or if any of its listed
/// features is in the builder's active-feature set.
fn source_is_active(src: &scanner::SourceRef, active: &[String]) -> bool {
    src.features.is_empty() || src.features.iter().any(|f| active.contains(f))
}

struct GatheredFile {
    /// Path relative to target_dir, forward-slash separated
    source_path: String,
    /// File name inside files_dir (hash-compression, empty for dirs)
    storage_name: String,
    compression: String,
    is_dir: bool,
}

pub fn build(mut params: BuildParams) -> Result<()> {
    log::info!("Starting build...");
    log::debug!("Target: {}", params.target_dir.display());
    log::debug!("Build dir: {}", params.build_dir.display());
    log::debug!("Output: {}", params.output_file.display());
    log::debug!("Compression: {}", params.compression);
    if let Some(ref triple) = params.target_triple {
        log::debug!("Target triple: {triple}");
    }

    compress::validate_method(&params.compression)?;

    // Parse the user's Cargo.toml once and reuse it for every subsequent
    // read (version check, package info, and — in the CLI path — gui /
    // win-resource configs that were already read to populate
    // BuildParams).
    let manifest = CargoManifest::load(&params.target_dir)?;

    // Preflight: user's installer crate must declare an `installrs` version
    // compatible with this CLI. Fails fast with a clear message instead of
    // letting cargo discover the mismatch later.
    check_installrs_version_compat(&manifest)?;

    // ── Prepare directories ──────────────────────────────────────────────────
    log::trace!("Creating build directory: {}", params.build_dir.display());
    std::fs::create_dir_all(&params.build_dir).context("failed to create build directory")?;

    std::fs::write(params.build_dir.join(".gitignore"), "*\n")
        .context("failed to write .gitignore")?;

    let installer_dir = params.build_dir.join("installer");
    let uninstaller_dir = params.build_dir.join("uninstaller");
    // Single shared blob cache for both generated crates. Each crate's
    // generated `main.rs` references entries via `include_bytes!` with
    // a `../../files/<hash>-<compression>` relative path, so identical
    // file contents referenced from both `install` and `uninstall`
    // share one on-disk blob (and one cache validation pass per build).
    let files_dir = params.build_dir.join("files");
    let uninstaller_bin = params.build_dir.join("uninstaller-bin");

    std::fs::create_dir_all(&files_dir).context("failed to create shared files directory")?;
    std::fs::create_dir_all(uninstaller_dir.join("src"))
        .context("failed to create uninstaller src directory")?;
    std::fs::create_dir_all(installer_dir.join("src"))
        .context("failed to create installer src directory")?;

    // ── Read user's package name and lib path ────────────────────────────────
    let (user_package_name, user_crate_name, lib_path) = manifest.package_info()?;
    log::debug!("User package: {user_package_name} (crate name: {user_crate_name})");

    // ── Scan user source ─────────────────────────────────────────────────────
    // Scan the directory containing the lib entry point (parent of lib_path).
    let abs_lib = params.target_dir.join(&lib_path);
    let src_dir = abs_lib.parent().unwrap_or(&params.target_dir).to_path_buf();
    log::info!("Scanning source files in {}", src_dir.display());
    let scan = scanner::scan_source_dir(&src_dir)?;

    if !scan.has_install_fn {
        return Err(anyhow!("source must define a public `install` function"));
    }
    if !scan.has_uninstall_fn {
        return Err(anyhow!("source must define a public `uninstall` function"));
    }

    if !params.features.is_empty() {
        log::info!("Active features: {}", params.features.join(", "));
    }

    log_source_list("Install sources", &scan.install_sources, &params.features);
    log_source_list(
        "Uninstall sources",
        &scan.uninstall_sources,
        &params.features,
    );

    // ── Gather and compress files for installer + uninstaller ────────────────
    let mut hash_cache: HashMap<String, String> = HashMap::new();
    let install_gathered = gather_for_phase(
        "install",
        &scan.install_sources,
        &files_dir,
        &params,
        &mut hash_cache,
    )?;
    let uninstall_gathered = gather_for_phase(
        "uninstall",
        &scan.uninstall_sources,
        &files_dir,
        &params,
        &mut hash_cache,
    )?;

    // ── Compile uninstaller ──────────────────────────────────────────────────
    let target_is_windows = params
        .target_triple
        .as_deref()
        .is_some_and(|t| t.contains("windows"))
        || (params.target_triple.is_none() && cfg!(target_os = "windows"));
    let target_is_linux = params
        .target_triple
        .as_deref()
        .is_some_and(|t| t.contains("linux"))
        || (params.target_triple.is_none() && cfg!(target_os = "linux"));

    let auto_resolved = if params.gui_enabled {
        "windows"
    } else {
        "console"
    };
    for slot in [
        &mut params.installer_win_resource,
        &mut params.uninstaller_win_resource,
    ] {
        let Some(cfg) = slot.as_mut() else { continue };
        // Convert PNG icons to ICO only when targeting Windows — Linux
        // uses the original PNG (embedded via include_bytes! in main.rs).
        if target_is_windows {
            if let Some(ref icon_path) = cfg.icon {
                if icon_path.extension().and_then(|e| e.to_str()) == Some("png") {
                    let ico_path =
                        ico_convert::png_to_ico(icon_path, &params.build_dir, &cfg.icon_sizes)?;
                    cfg.icon = Some(ico_path);
                }
            }
        }
        if cfg.windows_subsystem == "auto" {
            log::debug!("Resolved subsystem \"auto\" → {auto_resolved:?}");
            cfg.windows_subsystem = auto_resolved.to_string();
        }
    }

    let uninstall_compression = if uninstall_gathered.is_empty() {
        "none"
    } else {
        &params.compression
    };
    write_uninstaller_sources(
        &uninstaller_dir,
        &user_crate_name,
        &user_package_name,
        &params.target_dir,
        uninstall_compression,
        &uninstall_gathered,
        &files_dir,
        params.uninstaller_win_resource.as_ref(),
        params.gui_enabled,
        target_is_windows,
        target_is_linux,
        &params.features,
        params.installrs_local_path.as_deref(),
    )?;
    compile_cargo_project(
        &uninstaller_dir,
        params.target_triple.as_deref(),
        params.verbosity,
    )?;

    // Copy compiled uninstaller to known path
    let compiled = uninstaller_dir
        .join("target")
        .join(if let Some(t) = &params.target_triple {
            format!("{}/release", t)
        } else {
            "release".to_string()
        })
        .join(
            if params
                .target_triple
                .as_deref()
                .is_some_and(|t| t.contains("windows"))
                || cfg!(target_os = "windows")
            {
                "uninstaller.exe"
            } else {
                "uninstaller"
            },
        );
    let uninstaller_raw = std::fs::read(&compiled)
        .with_context(|| format!("failed to read uninstaller from {}", compiled.display()))?;
    let uninstaller_compressed = compress::compress(&uninstaller_raw, &params.compression)
        .context("failed to compress uninstaller binary")?;
    std::fs::write(&uninstaller_bin, &uninstaller_compressed).with_context(|| {
        format!(
            "failed to write compressed uninstaller to {}",
            uninstaller_bin.display()
        )
    })?;
    log::debug!(
        "Uninstaller binary ready: {} (compression: {})",
        uninstaller_bin.display(),
        params.compression
    );

    // ── Prune stale cached files ─────────────────────────────────────────────
    // Single shared `files_dir`: keep blobs referenced by either phase.
    prune_files_dir(&files_dir, &[&install_gathered, &uninstall_gathered])?;

    // ── Write installer sources and compile ──────────────────────────────────

    write_installer_sources(
        &installer_dir,
        &user_crate_name,
        &user_package_name,
        &params.target_dir,
        &install_gathered,
        &files_dir,
        &uninstaller_compressed,
        &params.compression,
        params.installer_win_resource.as_ref(),
        params.gui_enabled,
        target_is_windows,
        target_is_linux,
        &params.features,
        params.installrs_local_path.as_deref(),
    )?;
    compile_cargo_project(
        &installer_dir,
        params.target_triple.as_deref(),
        params.verbosity,
    )?;

    // Copy final binary to output path
    let compiled_installer = installer_dir
        .join("target")
        .join(if let Some(t) = &params.target_triple {
            format!("{}/release", t)
        } else {
            "release".to_string()
        })
        .join(
            if params
                .target_triple
                .as_deref()
                .is_some_and(|t| t.contains("windows"))
                || cfg!(target_os = "windows")
            {
                "installer-generated.exe"
            } else {
                "installer-generated"
            },
        );
    std::fs::copy(&compiled_installer, &params.output_file).with_context(|| {
        format!(
            "failed to copy installer to {}",
            params.output_file.display()
        )
    })?;

    log::info!("Build complete: {}", params.output_file.display());
    Ok(())
}

// The parse-and-extract helpers live as methods on [`CargoManifest`]
// above. `read_win_resource_config` / `read_gui_config` remain as thin
// wrappers for callers that don't already hold a manifest handle.

/// Returns (installer_config, uninstaller_config). Parses `Cargo.toml`
/// on every call — prefer [`CargoManifest::win_resource_config`] when
/// you already have a manifest.
pub fn read_win_resource_config(
    target_dir: &Path,
    features: &[String],
    cli_overrides: &[(Vec<String>, toml::Value)],
) -> Result<(Option<WinResourceConfig>, Option<WinResourceConfig>)> {
    CargoManifest::load(target_dir)?.win_resource_config(features, cli_overrides)
}

/// Read `gui = true` from `[package.metadata.installrs]`. Parses on
/// every call — prefer [`CargoManifest::gui_enabled`] when you already
/// have a manifest.
pub fn read_gui_config(
    target_dir: &Path,
    features: &[String],
    cli_overrides: &[(Vec<String>, toml::Value)],
) -> Result<bool> {
    Ok(CargoManifest::load(target_dir)?.gui_enabled(features, cli_overrides))
}

const VERSION_INFO_KEYS: &[(&str, &str)] = &[
    ("product-name", "ProductName"),
    ("file-description", "FileDescription"),
    ("file-version", "FileVersion"),
    ("product-version", "ProductVersion"),
    ("original-filename", "OriginalFilename"),
    ("legal-copyright", "LegalCopyright"),
    ("legal-trademarks", "LegalTrademarks"),
    ("company-name", "CompanyName"),
    ("internal-name", "InternalName"),
    ("comments", "Comments"),
];

fn parse_win_resource_table(meta: &toml::Value, target_dir: &Path) -> Result<WinResourceConfig> {
    let icon = meta
        .get("icon")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| target_dir.join(s));

    if let Some(ref icon_path) = icon {
        if !icon_path.exists() {
            return Err(anyhow!("icon file not found: {}", icon_path.display()));
        }
        match icon_path.extension().and_then(|e| e.to_str()) {
            Some("png") | Some("ico") => {}
            _ => {
                return Err(anyhow!(
                    "icon must be a .png or .ico file, got: {}",
                    icon_path.display()
                ))
            }
        }
    }

    let icon_sizes: Vec<u32> = if let Some(arr) = meta.get("icon-sizes").and_then(|v| v.as_array())
    {
        let mut sizes = Vec::new();
        for v in arr {
            let size = v
                .as_integer()
                .ok_or_else(|| anyhow!("`icon-sizes` entries must be integers"))?
                as u32;
            if size == 0 || size > 256 {
                return Err(anyhow!("icon-sizes values must be 1..=256, got {size}"));
            }
            sizes.push(size);
        }
        sizes
    } else {
        Vec::new()
    };

    let has_manifest_file = meta.get("manifest-file").is_some();
    let has_manifest_raw = meta.get("manifest-raw").is_some();
    let has_execution_level = meta.get("execution-level").is_some();
    let has_dpi_aware = meta.get("dpi-aware").is_some();
    let has_long_path_aware = meta.get("long-path-aware").is_some();
    let has_supported_os = meta.get("supported-os").is_some();
    let has_generated =
        has_execution_level || has_dpi_aware || has_long_path_aware || has_supported_os;

    if (has_manifest_file as u8 + has_manifest_raw as u8 + has_generated as u8) > 1 {
        return Err(anyhow!(
            "only one of `manifest-file`, `manifest-raw`, or generated manifest keys (execution-level, dpi-aware, long-path-aware) may be used"
        ));
    }

    let manifest = if has_manifest_file {
        let path = meta
            .get("manifest-file")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("`manifest-file` must be a string"))?;
        Some(ManifestConfig::File(target_dir.join(path)))
    } else if has_manifest_raw {
        let xml = meta
            .get("manifest-raw")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("`manifest-raw` must be a string"))?;
        Some(ManifestConfig::Raw(xml.to_string()))
    } else if has_generated {
        let execution_level = meta
            .get("execution-level")
            .and_then(|v| v.as_str())
            .unwrap_or("asInvoker")
            .to_string();
        if !matches!(
            execution_level.as_str(),
            "asInvoker" | "requireAdministrator" | "highestAvailable"
        ) {
            return Err(anyhow!(
                "invalid execution-level {:?}, expected \"asInvoker\", \"requireAdministrator\", or \"highestAvailable\"",
                execution_level
            ));
        }

        let dpi_aware = meta
            .get("dpi-aware")
            .map(|v| match v {
                toml::Value::Boolean(b) => Ok(b.to_string()),
                toml::Value::String(s) => {
                    if matches!(s.as_str(), "true" | "false" | "system" | "permonitor" | "permonitorv2") {
                        Ok(s.clone())
                    } else {
                        Err(anyhow!(
                            "invalid dpi-aware value {:?}, expected true, false, \"system\", \"permonitor\", or \"permonitorv2\"",
                            s
                        ))
                    }
                }
                _ => Err(anyhow!("`dpi-aware` must be a boolean or string")),
            })
            .transpose()?;

        let long_path_aware = meta
            .get("long-path-aware")
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| anyhow!("`long-path-aware` must be a boolean"))
            })
            .transpose()?;

        let supported_os = parse_supported_os(meta)?;

        Some(ManifestConfig::Generated {
            execution_level,
            dpi_aware,
            long_path_aware,
            supported_os,
        })
    } else {
        None
    };

    let windows_subsystem = meta
        .get("subsystem")
        .and_then(|v| v.as_str())
        .unwrap_or("auto")
        .to_string();

    if !matches!(windows_subsystem.as_str(), "console" | "windows" | "auto") {
        return Err(anyhow!(
            "invalid subsystem value {:?}, expected \"console\", \"windows\", or \"auto\"",
            windows_subsystem
        ));
    }

    let language = meta
        .get("language")
        .map(|v| {
            v.as_integer()
                .ok_or_else(|| {
                    anyhow!("`language` must be an integer (Windows LANGID, e.g. 0x0409 for en-US)")
                })
                .and_then(|n| {
                    u16::try_from(n)
                        .map_err(|_| anyhow!("`language` must be a valid u16 LANGID, got {n}"))
                })
        })
        .transpose()?;

    let mut version_info = Vec::new();
    for (toml_key, win_key) in VERSION_INFO_KEYS {
        if let Some(s) = meta.get(*toml_key).and_then(|v| v.as_str()) {
            version_info.push((win_key.to_string(), s.to_string()));
        }
    }

    Ok(WinResourceConfig {
        icon,
        icon_sizes,
        manifest,
        language,
        windows_subsystem,
        version_info,
    })
}

/// Merge an override config on top of a base. Override fields take precedence
/// when present; empty/None fields in the override fall through to base.
fn merge_win_resource_config(
    base: &WinResourceConfig,
    over: &WinResourceConfig,
) -> WinResourceConfig {
    WinResourceConfig {
        icon: over.icon.clone().or_else(|| base.icon.clone()),
        icon_sizes: if over.icon_sizes.is_empty() {
            base.icon_sizes.clone()
        } else {
            over.icon_sizes.clone()
        },
        manifest: over.manifest.clone().or_else(|| base.manifest.clone()),
        language: over.language.or(base.language),
        windows_subsystem: if over.windows_subsystem != "auto" {
            over.windows_subsystem.clone()
        } else {
            base.windows_subsystem.clone()
        },
        version_info: {
            let mut merged = base.version_info.clone();
            for (key, val) in &over.version_info {
                if let Some(entry) = merged.iter_mut().find(|(k, _)| k == key) {
                    entry.1 = val.clone();
                } else {
                    merged.push((key.clone(), val.clone()));
                }
            }
            merged
        },
    }
}

fn log_source_list(label: &str, sources: &[scanner::SourceRef], active_features: &[String]) {
    let active: Vec<&scanner::SourceRef> = sources
        .iter()
        .filter(|s| source_is_active(s, active_features))
        .collect();
    let skipped_count = sources.len() - active.len();
    if skipped_count > 0 {
        log::info!(
            "{} ({}, {} feature-gated skipped):",
            label,
            active.len(),
            skipped_count
        );
    } else {
        log::info!("{} ({}):", label, active.len());
    }
    for s in &active {
        log_source_ref(s);
    }
    // Skipped sources appear only in verbose (debug) mode.
    for s in sources {
        if !source_is_active(s, active_features) {
            log::debug!(
                "  [skipped] {} (features: {})",
                s.path,
                s.features.join(", ")
            );
        }
    }
}

fn log_source_ref(s: &scanner::SourceRef) {
    let mut annotations: Vec<String> = Vec::new();
    if !s.ignore.is_empty() {
        annotations.push(format!("ignore: {}", s.ignore.join(", ")));
    }
    if !s.features.is_empty() {
        annotations.push(format!("features: {}", s.features.join(", ")));
    }
    if annotations.is_empty() {
        log::info!("  {}", s.path);
    } else {
        log::info!("  {} ({})", s.path, annotations.join("; "));
    }
}

fn merge_ignore(global: &[String], per_source: &[String]) -> Vec<String> {
    let mut out: Vec<String> = global.to_vec();
    for p in per_source {
        if !out.contains(p) {
            out.push(p.clone());
        }
    }
    out
}

/// Walk one phase's source list (install or uninstall), filter out
/// feature-gated entries, merge per-source ignore globs with the
/// global ones, and gather + compress each source into `files_dir`.
/// `hash_cache` is shared across phases so identical content is
/// content-addressed (and written) once.
fn gather_for_phase(
    phase: &str,
    sources: &[scanner::SourceRef],
    files_dir: &Path,
    params: &BuildParams,
    hash_cache: &mut HashMap<String, String>,
) -> Result<Vec<GatheredFile>> {
    let mut gathered: Vec<GatheredFile> = Vec::new();
    for src in sources {
        if !source_is_active(src, &params.features) {
            log::debug!(
                "Skipping feature-gated source {:?} (features {:?})",
                src.path,
                src.features
            );
            continue;
        }
        let merged = merge_ignore(&params.ignore_patterns, &src.ignore);
        gather_source(
            &src.path,
            &params.target_dir,
            files_dir,
            &params.compression,
            &merged,
            &mut gathered,
            hash_cache,
        )?;
    }
    log::info!("Total {phase} entries gathered: {}", gathered.len());
    Ok(gathered)
}

/// Gather a single `source!()` path — dispatches to `gather_file` or
/// `gather_dir` based on filesystem metadata.
fn gather_source(
    source_path: &str,
    target_dir: &Path,
    files_dir: &Path,
    compression: &str,
    ignore: &[String],
    gathered: &mut Vec<GatheredFile>,
    hash_cache: &mut HashMap<String, String>,
) -> Result<()> {
    let abs = target_dir.join(source_path);
    let stat =
        std::fs::metadata(&abs).with_context(|| format!("failed to stat: {}", abs.display()))?;
    if stat.is_dir() {
        gather_dir(
            source_path,
            &abs,
            files_dir,
            compression,
            ignore,
            gathered,
            hash_cache,
        )
    } else {
        gather_file(
            source_path,
            &abs,
            files_dir,
            compression,
            ignore,
            gathered,
            hash_cache,
        )
    }
}

fn gather_file(
    source_path: &str,
    abs_path: &Path,
    files_dir: &Path,
    compression: &str,
    _ignore: &[String],
    gathered: &mut Vec<GatheredFile>,
    hash_cache: &mut HashMap<String, String>,
) -> Result<()> {
    if gathered.iter().any(|f| f.source_path == source_path) {
        return Ok(());
    }

    let stat = std::fs::metadata(abs_path)
        .with_context(|| format!("failed to stat: {}", abs_path.display()))?;
    if stat.is_dir() {
        return Err(anyhow!(
            "expected a file but got a directory: {source_path}"
        ));
    }

    let data = std::fs::read(abs_path)
        .with_context(|| format!("failed to read: {}", abs_path.display()))?;

    let hash = hex::encode(Sha256::digest(&data));

    let storage_name = format!("{hash}-{compression}");
    let storage_path = files_dir.join(&storage_name);

    if hash_cache.contains_key(&storage_name) {
        log::trace!("Already verified this run: {storage_name}");
    } else {
        let needs_write = if storage_path.exists() {
            log::trace!("Verifying cached file: {storage_name}");
            match std::fs::read(&storage_path) {
                Ok(cached) => match compress::decompress(&cached, compression) {
                    Ok(decompressed) => {
                        let cached_hash = hex::encode(Sha256::digest(&decompressed));
                        if cached_hash != hash {
                            log::warn!("Corrupt cache entry {storage_name}, recompressing");
                            true
                        } else {
                            log::debug!("Cache hit: {storage_name}");
                            false
                        }
                    }
                    Err(_) => {
                        log::warn!("Corrupt cache entry {storage_name} (decompression failed), recompressing");
                        true
                    }
                },
                Err(e) => {
                    log::warn!("Failed to read cache entry {storage_name}: {e}, recompressing");
                    true
                }
            }
        } else {
            log::trace!("No cached file for {storage_name}");
            true
        };

        if needs_write {
            let compressed = compress::compress(&data, compression)
                .with_context(|| format!("failed to compress: {source_path}"))?;
            std::fs::write(&storage_path, &compressed)
                .with_context(|| format!("failed to write cache: {}", storage_path.display()))?;
            log::debug!("Compressed {source_path} → {storage_name}");
        }
    }
    hash_cache.insert(storage_name.clone(), storage_name.clone());

    // Normalize to forward slashes
    let source_path = source_path.replace('\\', "/");
    gathered.push(GatheredFile {
        source_path,
        storage_name,
        compression: compression.to_string(),
        is_dir: false,
    });
    Ok(())
}

fn gather_dir(
    source_path: &str,
    abs_path: &Path,
    files_dir: &Path,
    compression: &str,
    ignore: &[String],
    gathered: &mut Vec<GatheredFile>,
    hash_cache: &mut HashMap<String, String>,
) -> Result<()> {
    if gathered
        .iter()
        .any(|f| f.source_path == source_path && f.is_dir)
    {
        return Ok(());
    }

    let stat = std::fs::metadata(abs_path)
        .with_context(|| format!("failed to stat: {}", abs_path.display()))?;
    if !stat.is_dir() {
        return Err(anyhow!(
            "expected a directory but got a file: {source_path}"
        ));
    }

    // Add the directory entry itself
    let source_path_norm = source_path.replace('\\', "/");
    gathered.push(GatheredFile {
        source_path: source_path_norm.clone(),
        storage_name: String::new(),
        compression: String::new(),
        is_dir: true,
    });

    for entry in std::fs::read_dir(abs_path)
        .with_context(|| format!("failed to read dir: {}", abs_path.display()))?
    {
        let entry = entry.context("failed to read directory entry")?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if matches_ignore(name_str.as_ref(), ignore) {
            log::debug!("Ignoring: {name_str}");
            continue;
        }

        let child_path = format!("{source_path_norm}/{name_str}");
        let child_abs = abs_path.join(&*name_str);

        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            gather_dir(
                &child_path,
                &child_abs,
                files_dir,
                compression,
                ignore,
                gathered,
                hash_cache,
            )?;
        } else {
            gather_file(
                &child_path,
                &child_abs,
                files_dir,
                compression,
                ignore,
                gathered,
                hash_cache,
            )?;
        }
    }

    Ok(())
}

fn matches_ignore(name: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| {
        glob::Pattern::new(p)
            .map(|pat: glob::Pattern| pat.matches(name))
            .unwrap_or(false)
    })
}

fn prune_files_dir(files_dir: &Path, gathered_groups: &[&[GatheredFile]]) -> Result<()> {
    let used: std::collections::HashSet<&str> = gathered_groups
        .iter()
        .flat_map(|g| g.iter())
        .filter(|f| !f.is_dir)
        .map(|f| f.storage_name.as_str())
        .collect();

    for entry in std::fs::read_dir(files_dir).context("failed to read files dir")? {
        let entry = entry.context("failed to read files dir entry")?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !used.contains(name_str.as_ref()) {
            std::fs::remove_file(entry.path())
                .with_context(|| format!("failed to remove stale file: {name_str}"))?;
            log::debug!("Pruned stale file: {name_str}");
        }
    }

    Ok(())
}

fn compression_feature(method: &str) -> Option<&str> {
    match method {
        "lzma" => Some("lzma"),
        "gzip" => Some("gzip"),
        "bzip2" => Some("bzip2"),
        _ => None,
    }
}

/// Generate the statics and ENTRIES code for a set of gathered files.
/// Returns (statics_code, entries_code, unique_storage_names_in_order).
fn generate_embedded_code(gathered: &[GatheredFile]) -> Result<(String, String, Vec<String>)> {
    // One named static per unique storage file
    let mut seen_statics: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut unique_order: Vec<String> = Vec::new();
    let mut statics_code = String::new();
    for f in gathered.iter().filter(|f| !f.is_dir) {
        if seen_statics.insert(f.storage_name.clone()) {
            unique_order.push(f.storage_name.clone());
            let ident = format!("D_{}", f.storage_name.replace('-', "_").to_uppercase());
            statics_code.push_str(&format!(
                "static {ident}: &[u8] = include_bytes!(\"../../files/{}\");\n",
                f.storage_name
            ));
        }
    }

    // Identify root entries
    let dir_prefixes: Vec<&str> = gathered
        .iter()
        .filter(|f| f.is_dir)
        .filter(|f| {
            !gathered.iter().any(|other| {
                other.is_dir
                    && other.source_path != f.source_path
                    && f.source_path
                        .starts_with(&format!("{}/", other.source_path))
            })
        })
        .map(|f| f.source_path.as_str())
        .collect();

    let root_files: Vec<&GatheredFile> = gathered
        .iter()
        .filter(|f| !f.is_dir)
        .filter(|f| {
            !dir_prefixes
                .iter()
                .any(|dp| f.source_path.starts_with(&format!("{dp}/")))
        })
        .collect();

    // Check for path hash collisions
    let mut hash_to_path: HashMap<u64, &str> = HashMap::new();
    for f in root_files.iter() {
        let ph = fnv1a(&f.source_path);
        if let Some(existing) = hash_to_path.get(&ph) {
            if *existing != f.source_path {
                return Err(anyhow!(
                    "path hash collision: {:?} and {:?} both hash to {:#018x}",
                    existing,
                    f.source_path,
                    ph
                ));
            }
        } else {
            hash_to_path.insert(ph, &f.source_path);
        }
    }
    for dp in &dir_prefixes {
        let ph = fnv1a(dp);
        if let Some(existing) = hash_to_path.get(&ph) {
            if *existing != *dp {
                return Err(anyhow!(
                    "path hash collision: {:?} and {:?} both hash to {:#018x}",
                    existing,
                    dp,
                    ph
                ));
            }
        } else {
            hash_to_path.insert(ph, dp);
        }
    }

    // Build the ENTRIES array
    let mut entries_code = String::new();
    for f in &root_files {
        let ph = fnv1a(&f.source_path);
        let ident = format!("D_{}", f.storage_name.replace('-', "_").to_uppercase());
        entries_code.push_str(&format!(
            "    EmbeddedEntry::File {{ source_path_hash: {ph}u64, data: {ident}, compression: {:?} }},\n",
            f.compression,
        ));
    }
    for dp in &dir_prefixes {
        let ph = fnv1a(dp);
        let children_code = emit_dir_children(gathered, dp, 2);
        entries_code.push_str(&format!(
            "    EmbeddedEntry::Dir {{ source_path_hash: {ph}u64, children: &[\n{children_code}    ] }},\n"
        ));
    }

    Ok((statics_code, entries_code, unique_order))
}

const SUPPORTED_OS_MAP: &[(&str, &str, &str)] = &[
    (
        "vista",
        "e2011457-1546-43c5-a5fe-008deee3d3f0",
        "Windows Vista",
    ),
    ("7", "35138b9a-5d96-4fbd-8e2d-a2440225f93a", "Windows 7"),
    ("8", "4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38", "Windows 8"),
    ("8.1", "1f676c76-80e1-4239-95bb-83d0f6d0da78", "Windows 8.1"),
    (
        "10",
        "8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a",
        "Windows 10 / 11",
    ),
];

const DEFAULT_SUPPORTED_OS: &[&str] = &["vista", "7", "8", "8.1", "10"];

fn parse_supported_os(meta: &toml::Value) -> Result<Vec<String>> {
    match meta.get("supported-os").and_then(|v| v.as_array()) {
        Some(arr) => {
            let mut os_list = Vec::new();
            for v in arr {
                let s = v
                    .as_str()
                    .ok_or_else(|| anyhow!("`supported-os` entries must be strings"))?;
                if !SUPPORTED_OS_MAP.iter().any(|(name, _, _)| *name == s) {
                    let valid: Vec<&str> = SUPPORTED_OS_MAP.iter().map(|(n, _, _)| *n).collect();
                    return Err(anyhow!(
                        "unknown supported-os value {:?}, expected one of: {}",
                        s,
                        valid.join(", ")
                    ));
                }
                os_list.push(s.to_string());
            }
            Ok(os_list)
        }
        None => Ok(Vec::new()),
    }
}

fn generate_manifest_xml(
    execution_level: &str,
    dpi_aware: Option<&str>,
    long_path_aware: Option<bool>,
    supported_os: &[String],
    gui_enabled: bool,
) -> String {
    let mut settings = String::new();
    if let Some(dpi) = dpi_aware {
        let (aware_val, awareness_val) = match dpi {
            "true" => ("true", "system"),
            "false" => ("false", "unaware"),
            "system" => ("true", "system"),
            "permonitor" => ("true/pm", "permonitor"),
            "permonitorv2" => ("true/pm", "permonitorv2"),
            _ => ("true", "system"),
        };
        settings.push_str(&format!(
            "        <dpiAware xmlns=\"http://schemas.microsoft.com/SMI/2005/WindowsSettings\">{aware_val}</dpiAware>\n\
             \x20       <dpiAwareness xmlns=\"http://schemas.microsoft.com/SMI/2016/WindowsSettings\">{awareness_val}</dpiAwareness>\n"
        ));
    }
    if let Some(true) = long_path_aware {
        settings.push_str(
            "        <longPathAware xmlns=\"http://schemas.microsoft.com/SMI/2016/WindowsSettings\">true</longPathAware>\n"
        );
    }

    let ws_block = if settings.is_empty() {
        String::new()
    } else {
        format!(
            "  <asmv3:application>\n\
             \x20   <asmv3:windowsSettings>\n\
             {settings}\
             \x20   </asmv3:windowsSettings>\n\
             \x20 </asmv3:application>\n"
        )
    };

    let os_names: &[&str] = if supported_os.is_empty() {
        DEFAULT_SUPPORTED_OS
    } else {
        // Safe: we only use this slice within this function call
        &supported_os.iter().map(|s| s.as_str()).collect::<Vec<_>>()
    };

    let mut compat_entries = String::new();
    for name in os_names {
        if let Some((_, guid, label)) = SUPPORTED_OS_MAP.iter().find(|(n, _, _)| n == name) {
            compat_entries.push_str(&format!(
                "      <!-- {label} -->\n      <supportedOS Id=\"{{{guid}}}\" />\n"
            ));
        }
    }

    let comctl_block = if gui_enabled {
        "  <dependency>\n\
         \x20   <dependentAssembly>\n\
         \x20     <assemblyIdentity type=\"win32\" name=\"Microsoft.Windows.Common-Controls\" version=\"6.0.0.0\" processorArchitecture=\"*\" publicKeyToken=\"6595b64144ccf1df\" language=\"*\" />\n\
         \x20   </dependentAssembly>\n\
         \x20 </dependency>\n"
    } else {
        ""
    };

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="InstallRS.Installer" version="1.0.0.0" processorArchitecture="*" />
{comctl_block}  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="{execution_level}" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
{compat_entries}    </application>
  </compatibility>
{ws_block}</assembly>
"#
    )
}

fn write_build_rs(dir: &Path, config: &WinResourceConfig, gui_enabled: bool) -> Result<()> {
    let mut code =
        String::from("fn main() {\n    #[allow(unused_mut)]\n    let mut res = winresource::WindowsResource::new();\n");

    if let Some(icon) = &config.icon {
        let icon_str = icon.display().to_string().replace('\\', "/");
        code.push_str(&format!("    res.set_icon(r\"{icon_str}\");\n"));
    }

    match &config.manifest {
        Some(ManifestConfig::File(path)) => {
            let path_str = path.display().to_string().replace('\\', "/");
            code.push_str(&format!("    res.set_manifest_file(r\"{path_str}\");\n"));
        }
        Some(ManifestConfig::Raw(xml)) => {
            code.push_str(&format!("    res.set_manifest(r#\"{}\"#);\n", xml));
        }
        Some(ManifestConfig::Generated {
            execution_level,
            dpi_aware,
            long_path_aware,
            supported_os,
        }) => {
            let xml = generate_manifest_xml(
                execution_level,
                dpi_aware.as_deref(),
                *long_path_aware,
                supported_os,
                gui_enabled,
            );
            code.push_str(&format!("    res.set_manifest(r#\"{}\"#);\n", xml));
        }
        None => {}
    }

    if let Some(lang) = config.language {
        code.push_str(&format!("    res.set_language({lang:#06x});\n"));
    }

    for (key, val) in &config.version_info {
        code.push_str(&format!("    res.set({key:?}, {val:?});\n"));
        // Also populate the FIXEDFILEINFO numeric version block so the
        // 4-part version shown by `Get-Item .exe | % VersionInfo` and
        // by Explorer's Properties → Details panel matches the string
        // field, instead of staying at 0.0.0.0.
        let fixed_kind = match key.as_str() {
            "FileVersion" => Some("FILEVERSION"),
            "ProductVersion" => Some("PRODUCTVERSION"),
            _ => None,
        };
        if let Some(kind) = fixed_kind {
            let packed = pack_version_u64(val);
            code.push_str(&format!(
                "    res.set_version_info(winresource::VersionInfo::{kind}, {packed:#x});\n",
            ));
        }
    }

    code.push_str("    res.compile().unwrap();\n}\n");

    write_if_changed(&dir.join("build.rs"), &code)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_uninstaller_sources(
    uninstaller_dir: &Path,
    user_crate_name: &str,
    user_package_name: &str,
    user_crate_path: &Path,
    compression: &str,
    gathered: &[GatheredFile],
    files_dir: &Path,
    win_resource: Option<&WinResourceConfig>,
    gui_enabled: bool,
    target_is_windows: bool,
    target_is_linux: bool,
    user_features: &[String],
    installrs_local_path: Option<&Path>,
) -> Result<()> {
    log::debug!("Writing uninstaller sources");

    let mut features: Vec<&str> = Vec::new();
    if let Some(f) = compression_feature(compression) {
        features.push(f);
    }
    if gui_enabled {
        features.push("gui");
        if target_is_windows {
            features.push("gui-win32");
        } else if target_is_linux {
            features.push("gui-gtk");
        }
    }
    let features_str = if features.is_empty() {
        ", default-features = false".to_string()
    } else {
        let feat_list: Vec<String> = features.iter().map(|f| format!("{f:?}")).collect();
        format!(
            ", default-features = false, features = [{}]",
            feat_list.join(", ")
        )
    };

    let emit_win_resource = target_is_windows && win_resource.is_some();
    let build_deps = if emit_win_resource {
        "\n[build-dependencies]\nwinresource = \"0.1\"\n"
    } else {
        ""
    };

    let user_features_str = format_user_features(user_features);

    let cargo_toml = format!(
        r#"[package]
name = "uninstaller"
version = "0.1.0"
edition = "2021"

[workspace]

[dependencies]
{installrs_dep}
{user_crate_name} = {{ path = {user_path:?}, package = "{user_package_name}"{user_features_str} }}
{build_deps}
[profile.release]
opt-level = "z"
strip = true
lto = true
codegen-units = 1
"#,
        installrs_dep = installrs_dep_spec(&features_str, installrs_local_path),
        user_path = user_crate_path,
    );

    let subsystem_attr = match win_resource {
        Some(cfg) if target_is_windows && cfg.windows_subsystem == "windows" => {
            "#![windows_subsystem = \"windows\"]\n"
        }
        _ => "",
    };

    let icon_init = gui_enabled
        .then(|| linux_icon_init(target_is_linux, win_resource))
        .flatten()
        .unwrap_or_default();
    let gui_use = if icon_init.is_empty() {
        ""
    } else {
        "use installrs::gui::__private::set_window_icon_png;\n"
    };

    let main_rs = if gathered.is_empty() {
        format!(
            r#"// Code generated by installrs; DO NOT EDIT.
{subsystem_attr}{gui_use}fn main() {{
{icon_init}    let mut i = installrs::Installer::new(&[], &[], "none");
    i.install_ctrlc_handler();
    i.uninstall_main({user_crate_name}::uninstall);
}}
"#
        )
    } else {
        let (statics_code, entries_code, unique_order) = generate_embedded_code(gathered)?;
        let payload_hash = compute_payload_hash(&unique_order, files_dir, None)?;
        let hash_literal = format_hash_array(&payload_hash);
        let blobs_literal = format_blobs_array(&unique_order);
        format!(
            r#"// Code generated by installrs; DO NOT EDIT.
{subsystem_attr}use installrs::__private::{{verify_payload, DirChild, DirChildKind, EmbeddedEntry}};
const _: () = installrs::__private::assert_entries_version({entries_version});
{gui_use}{statics_code}
static ENTRIES: &[EmbeddedEntry] = &[
{entries_code}];
static PAYLOAD_BLOBS: &[&[u8]] = &{blobs_literal};
static PAYLOAD_HASH: [u8; 32] = {hash_literal};

fn main() {{
{icon_init}    if let Err(e) = verify_payload(PAYLOAD_BLOBS, &[], &PAYLOAD_HASH) {{
        eprintln!("{{e}}");
        std::process::exit(1);
    }}
    let mut i = installrs::Installer::new(ENTRIES, &[], {compression:?});
    i.install_ctrlc_handler();
    i.uninstall_main({user_crate_name}::uninstall);
}}
"#,
            entries_version = GENERATED_ENTRIES_VERSION,
        )
    };

    write_if_changed(&uninstaller_dir.join("Cargo.toml"), &cargo_toml)?;
    write_if_changed(&uninstaller_dir.join("src").join("main.rs"), &main_rs)?;

    if emit_win_resource {
        if let Some(cfg) = win_resource {
            write_build_rs(uninstaller_dir, cfg, gui_enabled)?;
        }
    } else {
        let build_rs = uninstaller_dir.join("build.rs");
        if build_rs.exists() {
            std::fs::remove_file(&build_rs).ok();
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_installer_sources(
    installer_dir: &Path,
    user_crate_name: &str,
    user_package_name: &str,
    user_crate_path: &Path,
    gathered: &[GatheredFile],
    files_dir: &Path,
    uninstaller_bytes: &[u8],
    compression: &str,
    win_resource: Option<&WinResourceConfig>,
    gui_enabled: bool,
    target_is_windows: bool,
    target_is_linux: bool,
    user_features: &[String],
    installrs_local_path: Option<&Path>,
) -> Result<()> {
    log::debug!("Writing installer sources");

    let mut features: Vec<&str> = Vec::new();
    if let Some(f) = compression_feature(compression) {
        features.push(f);
    }
    if gui_enabled {
        features.push("gui");
        if target_is_windows {
            features.push("gui-win32");
        } else if target_is_linux {
            features.push("gui-gtk");
        }
    }
    let features_str = if features.is_empty() {
        ", default-features = false".to_string()
    } else {
        let feat_list: Vec<String> = features.iter().map(|f| format!("{f:?}")).collect();
        format!(
            ", default-features = false, features = [{}]",
            feat_list.join(", ")
        )
    };

    let emit_win_resource = target_is_windows && win_resource.is_some();
    let build_deps = if emit_win_resource {
        "\n[build-dependencies]\nwinresource = \"0.1\"\n"
    } else {
        ""
    };

    let user_features_str = format_user_features(user_features);

    let cargo_toml = format!(
        r#"[package]
name = "installer-generated"
version = "0.1.0"
edition = "2021"

[workspace]

[dependencies]
{installrs_dep}
{user_crate_name} = {{ path = {user_path:?}, package = "{user_package_name}"{user_features_str} }}
{build_deps}
[profile.release]
opt-level = "z"
strip = true
lto = true
codegen-units = 1
"#,
        installrs_dep = installrs_dep_spec(&features_str, installrs_local_path),
        user_path = user_crate_path,
    );

    let (statics_code, entries_code, unique_order) = generate_embedded_code(gathered)?;

    let subsystem_attr = match win_resource {
        Some(cfg) if target_is_windows && cfg.windows_subsystem == "windows" => {
            "#![windows_subsystem = \"windows\"]\n"
        }
        _ => "",
    };

    // On Linux with a PNG icon, embed the bytes and install them as the GTK
    // default window icon so the wizard + all dialogs get the right icon.
    let icon_init = gui_enabled
        .then(|| linux_icon_init(target_is_linux, win_resource))
        .flatten()
        .unwrap_or_default();
    let gui_use = if icon_init.is_empty() {
        ""
    } else {
        "use installrs::gui::__private::set_window_icon_png;\n"
    };

    let payload_hash = compute_payload_hash(&unique_order, files_dir, Some(uninstaller_bytes))?;
    let hash_literal = format_hash_array(&payload_hash);
    let blobs_literal = format_blobs_array(&unique_order);

    let main_rs = format!(
        r#"// Code generated by installrs; DO NOT EDIT.
{subsystem_attr}use installrs::__private::{{verify_payload, DirChild, DirChildKind, EmbeddedEntry}};
const _: () = installrs::__private::assert_entries_version({entries_version});
{gui_use}{statics_code}
static ENTRIES: &[EmbeddedEntry] = &[
{entries_code}];
static UNINSTALLER_DATA: &[u8] = include_bytes!("../../uninstaller-bin");
static PAYLOAD_BLOBS: &[&[u8]] = &{blobs_literal};
static PAYLOAD_HASH: [u8; 32] = {hash_literal};

fn main() {{
{icon_init}    if let Err(e) = verify_payload(PAYLOAD_BLOBS, UNINSTALLER_DATA, &PAYLOAD_HASH) {{
        eprintln!("{{e}}");
        std::process::exit(1);
    }}
    let mut i = installrs::Installer::new(ENTRIES, UNINSTALLER_DATA, {compression:?});
    i.install_ctrlc_handler();
    i.install_main({user_crate_name}::install);
}}
"#,
        entries_version = GENERATED_ENTRIES_VERSION,
    );

    write_if_changed(&installer_dir.join("Cargo.toml"), &cargo_toml)?;
    write_if_changed(&installer_dir.join("src").join("main.rs"), &main_rs)?;

    if emit_win_resource {
        if let Some(cfg) = win_resource {
            write_build_rs(installer_dir, cfg, gui_enabled)?;
        }
    } else {
        let build_rs = installer_dir.join("build.rs");
        if build_rs.exists() {
            std::fs::remove_file(&build_rs).ok();
        }
    }

    Ok(())
}

/// SHA-256 each unique compressed blob once (in the order its `D_*` static is
/// declared), then the optional uninstaller bytes. Mirrors the runtime
/// `installrs::__private::verify_payload`, which hashes the same `PAYLOAD_BLOBS` slice.
fn compute_payload_hash(
    unique_storage_names: &[String],
    files_dir: &Path,
    uninstaller: Option<&[u8]>,
) -> Result<[u8; 32]> {
    let mut h = Sha256::new();
    for name in unique_storage_names {
        let data = std::fs::read(files_dir.join(name))
            .with_context(|| format!("failed to read {name} for payload hash"))?;
        h.update(&data);
    }
    if let Some(u) = uninstaller {
        h.update(u);
    }
    Ok(h.finalize().into())
}

fn format_blobs_array(unique_storage_names: &[String]) -> String {
    let mut out = String::from("[\n");
    for name in unique_storage_names {
        let ident = format!("D_{}", name.replace('-', "_").to_uppercase());
        out.push_str(&format!("    {ident},\n"));
    }
    out.push(']');
    out
}

fn format_hash_array(hash: &[u8; 32]) -> String {
    let parts: Vec<String> = hash.iter().map(|b| format!("0x{b:02x}")).collect();
    format!("[{}]", parts.join(", "))
}

/// If the build is targeting Linux and the user configured a PNG icon,
/// emit a `set_window_icon_png(include_bytes!("..."));`
/// snippet to drop at the top of `main()`. Skips ICO icons (GTK can't
/// load them), non-Linux targets, and builds without an icon configured.
fn linux_icon_init(
    target_is_linux: bool,
    win_resource: Option<&WinResourceConfig>,
) -> Option<String> {
    if !target_is_linux {
        return None;
    }
    let icon_path = win_resource.as_ref().and_then(|c| c.icon.as_ref())?;
    if icon_path.extension().and_then(|e| e.to_str()) != Some("png") {
        return None;
    }
    // Absolute path so include_bytes! resolves regardless of where the
    // generated crate lives relative to the user's project.
    let abs = icon_path
        .canonicalize()
        .unwrap_or_else(|_| icon_path.clone());
    let path_str = abs.display().to_string().replace('\\', "/");
    Some(format!(
        "    set_window_icon_png(include_bytes!({path_str:?}));\n"
    ))
}

/// Emit nested Rust code for DirChild entries under `parent_path`.
fn emit_dir_children(gathered: &[GatheredFile], parent_path: &str, indent: usize) -> String {
    let pad = "    ".repeat(indent);
    let mut out = String::new();

    // Collect direct children (one level deep under parent_path)
    let prefix = format!("{parent_path}/");
    for f in gathered {
        if !f.source_path.starts_with(&prefix) {
            continue;
        }
        let rest = &f.source_path[prefix.len()..];
        // Direct child has no further '/'
        if rest.contains('/') {
            continue;
        }
        let name = rest;
        if f.is_dir {
            let children_code = emit_dir_children(gathered, &f.source_path, indent + 1);
            out.push_str(&format!(
                "{pad}DirChild {{ name: {name:?}, kind: DirChildKind::Dir {{ children: &[\n{children_code}{pad}] }} }},\n"
            ));
        } else {
            let ident = format!("D_{}", f.storage_name.replace('-', "_").to_uppercase());
            out.push_str(&format!(
                "{pad}DirChild {{ name: {name:?}, kind: DirChildKind::File {{ data: {ident}, compression: {:?} }} }},\n",
                f.compression,
            ));
        }
    }

    out
}

fn compile_cargo_project(
    project_dir: &Path,
    target_triple: Option<&str>,
    verbosity: u8,
) -> Result<()> {
    log::info!("Compiling {}", project_dir.display());

    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("build").arg("--release");
    if let Some(triple) = target_triple {
        cmd.args(["--target", triple]);
    }
    match verbosity {
        0 => {
            cmd.arg("--quiet");
        }
        2.. => {
            cmd.arg("-vv");
        }
        _ => {}
    }
    cmd.current_dir(project_dir);

    log::trace!(
        "Running: cargo build --release{}",
        target_triple
            .map(|t| format!(" --target {t}"))
            .unwrap_or_default()
    );

    let status = cmd
        .status()
        .with_context(|| format!("failed to run cargo in {}", project_dir.display()))?;

    if !status.success() {
        return Err(anyhow!("cargo build failed in {}", project_dir.display()));
    }

    log::debug!("Compiled successfully: {}", project_dir.display());
    Ok(())
}
