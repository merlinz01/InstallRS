use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};

use super::compress;
use super::ico_convert;
use super::scanner;

// Embedded at compile time so the build tool is self-contained.
const INSTALLRS_CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

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

    // ── Prepare directories ──────────────────────────────────────────────────
    log::trace!("Creating build directory: {}", params.build_dir.display());
    std::fs::create_dir_all(&params.build_dir).context("failed to create build directory")?;

    // ── Convert PNG icons to ICO if needed ─────────────────────────────────
    for cfg in [
        &mut params.installer_win_resource,
        &mut params.uninstaller_win_resource,
    ]
    .into_iter()
    .flatten()
    {
        if let Some(ref icon_path) = cfg.icon {
            if icon_path.extension().and_then(|e| e.to_str()) == Some("png") {
                let ico_path =
                    ico_convert::png_to_ico(icon_path, &params.build_dir, &cfg.icon_sizes)?;
                cfg.icon = Some(ico_path);
            }
        }
    }
    std::fs::write(params.build_dir.join(".gitignore"), "*\n")
        .context("failed to write .gitignore")?;

    let installer_dir = params.build_dir.join("installer");
    let uninstaller_dir = params.build_dir.join("uninstaller");
    let install_files_dir = installer_dir.join("files");
    let uninstall_files_dir = uninstaller_dir.join("files");
    let uninstaller_bin = params.build_dir.join("uninstaller-bin");

    std::fs::create_dir_all(&install_files_dir)
        .context("failed to create installer files directory")?;
    std::fs::create_dir_all(&uninstall_files_dir)
        .context("failed to create uninstaller files directory")?;
    std::fs::create_dir_all(uninstaller_dir.join("src"))
        .context("failed to create uninstaller src directory")?;
    std::fs::create_dir_all(installer_dir.join("src"))
        .context("failed to create installer src directory")?;

    // ── Read user's package name and lib path ────────────────────────────────
    let (user_package_name, user_crate_name, lib_path) = read_package_info(&params.target_dir)?;
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

    log::info!("Install files ({}):", scan.install_files.len());
    for f in &scan.install_files {
        log::info!("  {f}");
    }
    log::info!("Install directories ({}):", scan.install_dirs.len());
    for d in &scan.install_dirs {
        log::info!("  {d}");
    }
    log::info!("Uninstall files ({}):", scan.uninstall_files.len());
    for f in &scan.uninstall_files {
        log::info!("  {f}");
    }
    log::info!("Uninstall directories ({}):", scan.uninstall_dirs.len());
    for d in &scan.uninstall_dirs {
        log::info!("  {d}");
    }

    // ── Gather and compress files for installer ──────────────────────────────
    let mut install_gathered: Vec<GatheredFile> = Vec::new();
    let mut hash_cache: HashMap<String, String> = HashMap::new();

    for file_path in &scan.install_files {
        let abs = params.target_dir.join(file_path);
        gather_file(
            file_path,
            &abs,
            &install_files_dir,
            &params.compression,
            &params.ignore_patterns,
            &mut install_gathered,
            &mut hash_cache,
        )?;
    }
    for dir_path in &scan.install_dirs {
        let abs = params.target_dir.join(dir_path);
        gather_dir(
            dir_path,
            &abs,
            &install_files_dir,
            &params.compression,
            &params.ignore_patterns,
            &mut install_gathered,
            &mut hash_cache,
        )?;
    }
    log::info!("Total install entries gathered: {}", install_gathered.len());

    // ── Gather and compress files for uninstaller ────────────────────────────
    let mut uninstall_gathered: Vec<GatheredFile> = Vec::new();

    for file_path in &scan.uninstall_files {
        let abs = params.target_dir.join(file_path);
        gather_file(
            file_path,
            &abs,
            &uninstall_files_dir,
            &params.compression,
            &params.ignore_patterns,
            &mut uninstall_gathered,
            &mut hash_cache,
        )?;
    }
    for dir_path in &scan.uninstall_dirs {
        let abs = params.target_dir.join(dir_path);
        gather_dir(
            dir_path,
            &abs,
            &uninstall_files_dir,
            &params.compression,
            &params.ignore_patterns,
            &mut uninstall_gathered,
            &mut hash_cache,
        )?;
    }
    log::info!(
        "Total uninstall entries gathered: {}",
        uninstall_gathered.len()
    );

    // ── Compile uninstaller ──────────────────────────────────────────────────
    let target_is_windows = params
        .target_triple
        .as_deref()
        .is_some_and(|t| t.contains("windows"))
        || cfg!(target_os = "windows");

    let uninstall_compression = if uninstall_gathered.is_empty() {
        "none"
    } else {
        &params.compression
    };
    write_uninstaller_sources(
        &uninstaller_dir,
        &user_crate_name,
        &params.target_dir,
        uninstall_compression,
        &uninstall_gathered,
        params.uninstaller_win_resource.as_ref(),
        params.gui_enabled,
        target_is_windows,
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
    prune_files_dir(&install_files_dir, &install_gathered)?;
    prune_files_dir(&uninstall_files_dir, &uninstall_gathered)?;

    // ── Write installer sources and compile ──────────────────────────────────

    // Resolve "auto" subsystem: "windows" when GUI is enabled, "console" otherwise.
    let auto_resolved = if params.gui_enabled {
        "windows"
    } else {
        "console"
    };
    for cfg in [
        &mut params.installer_win_resource,
        &mut params.uninstaller_win_resource,
    ] {
        if let Some(cfg) = cfg.as_mut() {
            if cfg.windows_subsystem == "auto" {
                log::debug!("Resolved subsystem \"auto\" → {auto_resolved:?}");
                cfg.windows_subsystem = auto_resolved.to_string();
            }
        }
    }

    write_installer_sources(
        &installer_dir,
        &user_crate_name,
        &params.target_dir,
        &install_gathered,
        &params.compression,
        params.installer_win_resource.as_ref(),
        params.gui_enabled,
        target_is_windows,
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

/// Returns (package_name, lib_crate_name, lib_path) where lib_path is relative to target_dir.
/// lib_crate_name is [lib].name if set, otherwise package_name with hyphens → underscores.
fn read_package_info(target_dir: &Path) -> Result<(String, String, PathBuf)> {
    let cargo_toml_path = target_dir.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml_path)
        .with_context(|| format!("failed to read {}", cargo_toml_path.display()))?;
    let value: toml::Value = content.parse().context("failed to parse Cargo.toml")?;
    let package_name = value
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| anyhow!("could not find [package].name in Cargo.toml"))?
        .to_string();
    let lib = value.get("lib");
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

/// Returns (installer_config, uninstaller_config).
///
/// Base keys in `[package.metadata.installrs]` apply to both. Keys in
/// `[package.metadata.installrs.installer]` or `…uninstaller` override the base.
pub fn read_win_resource_config(
    target_dir: &Path,
) -> Result<(Option<WinResourceConfig>, Option<WinResourceConfig>)> {
    let cargo_toml_path = target_dir.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml_path)
        .with_context(|| format!("failed to read {}", cargo_toml_path.display()))?;
    let value: toml::Value = content.parse().context("failed to parse Cargo.toml")?;

    let meta = match value
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("installrs"))
    {
        Some(v) => v,
        None => return Ok((None, None)),
    };

    let base = parse_win_resource_table(meta, target_dir)?;

    let installer = if let Some(sub) = meta.get("installer") {
        let overrides = parse_win_resource_table(sub, target_dir)?;
        merge_win_resource_config(&base, &overrides)
    } else {
        base.clone()
    };

    let uninstaller = if let Some(sub) = meta.get("uninstaller") {
        let overrides = parse_win_resource_table(sub, target_dir)?;
        merge_win_resource_config(&base, &overrides)
    } else {
        base
    };

    Ok((Some(installer), Some(uninstaller)))
}

/// Read `gui = true` from `[package.metadata.installrs]`.
pub fn read_gui_config(target_dir: &Path) -> Result<bool> {
    let cargo_toml_path = target_dir.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml_path)
        .with_context(|| format!("failed to read {}", cargo_toml_path.display()))?;
    let value: toml::Value = content.parse().context("failed to parse Cargo.toml")?;

    let gui = value
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("installrs"))
        .and_then(|i| i.get("gui"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(gui)
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

fn prune_files_dir(files_dir: &Path, gathered: &[GatheredFile]) -> Result<()> {
    let used: std::collections::HashSet<&str> = gathered
        .iter()
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
/// Returns (statics_code, entries_code).
fn generate_embedded_code(gathered: &[GatheredFile]) -> Result<(String, String)> {
    // One named static per unique storage file
    let mut seen_statics: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut statics_code = String::new();
    for f in gathered.iter().filter(|f| !f.is_dir) {
        if seen_statics.insert(f.storage_name.clone()) {
            let ident = format!("D_{}", f.storage_name.replace('-', "_").to_uppercase());
            statics_code.push_str(&format!(
                "static {ident}: &[u8] = include_bytes!(\"../files/{}\");\n",
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
            "    installrs::EmbeddedEntry::File {{ source_path_hash: {ph}u64, data: {ident}, compression: {:?} }},\n",
            f.compression,
        ));
    }
    for dp in &dir_prefixes {
        let ph = fnv1a(dp);
        let children_code = emit_dir_children(gathered, dp, 2);
        entries_code.push_str(&format!(
            "    installrs::EmbeddedEntry::Dir {{ source_path_hash: {ph}u64, children: &[\n{children_code}    ] }},\n"
        ));
    }

    Ok((statics_code, entries_code))
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

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="InstallRS.Installer" version="1.0.0.0" processorArchitecture="*" />
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
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

fn write_build_rs(dir: &Path, config: &WinResourceConfig) -> Result<()> {
    let mut code =
        String::from("fn main() {\n    let mut res = winresource::WindowsResource::new();\n");

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
    }

    code.push_str("    res.compile().unwrap();\n}\n");

    write_if_changed(&dir.join("build.rs"), &code)?;
    Ok(())
}

fn write_uninstaller_sources(
    uninstaller_dir: &Path,
    user_crate_name: &str,
    user_crate_path: &Path,
    compression: &str,
    gathered: &[GatheredFile],
    win_resource: Option<&WinResourceConfig>,
    gui_enabled: bool,
    target_is_windows: bool,
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

    let build_deps = if win_resource.is_some() {
        "\n[build-dependencies]\nwinresource = \"0.1\"\n"
    } else {
        ""
    };

    let cargo_toml = format!(
        r#"[package]
name = "uninstaller"
version = "0.1.0"
edition = "2021"

[workspace]

[dependencies]
installrs = {{ path = {installrs_path:?}{features_str} }}
{user_crate_name} = {{ path = {user_path:?}, package = "{user_package_name}" }}
{build_deps}
[profile.release]
opt-level = "z"
strip = true
lto = true
codegen-units = 1
"#,
        installrs_path = INSTALLRS_CRATE_PATH,
        user_path = user_crate_path,
        user_package_name = user_crate_name.replace('_', "-"),
    );

    let subsystem_attr = match win_resource {
        Some(cfg) if cfg.windows_subsystem == "windows" => "#![windows_subsystem = \"windows\"]\n",
        _ => "",
    };

    let main_rs = if gathered.is_empty() {
        format!(
            r#"// Code generated by installrs; DO NOT EDIT.
{subsystem_attr}fn main() {{
    let mut i = installrs::Installer::new(&[], &[], "none");
    i.uninstall_main({user_crate_name}::uninstall);
}}
"#
        )
    } else {
        let (statics_code, entries_code) = generate_embedded_code(gathered)?;
        format!(
            r#"// Code generated by installrs; DO NOT EDIT.
{subsystem_attr}{statics_code}
static ENTRIES: &[installrs::EmbeddedEntry] = &[
{entries_code}];

fn main() {{
    let mut i = installrs::Installer::new(ENTRIES, &[], {compression:?});
    i.uninstall_main({user_crate_name}::uninstall);
}}
"#
        )
    };

    write_if_changed(&uninstaller_dir.join("Cargo.toml"), &cargo_toml)?;
    write_if_changed(&uninstaller_dir.join("src").join("main.rs"), &main_rs)?;

    if let Some(cfg) = win_resource {
        write_build_rs(uninstaller_dir, cfg)?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_installer_sources(
    installer_dir: &Path,
    user_crate_name: &str,
    user_crate_path: &Path,
    gathered: &[GatheredFile],
    compression: &str,
    win_resource: Option<&WinResourceConfig>,
    gui_enabled: bool,
    target_is_windows: bool,
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

    let build_deps = if win_resource.is_some() {
        "\n[build-dependencies]\nwinresource = \"0.1\"\n"
    } else {
        ""
    };

    let cargo_toml = format!(
        r#"[package]
name = "installer-generated"
version = "0.1.0"
edition = "2021"

[workspace]

[dependencies]
installrs = {{ path = {installrs_path:?}{features_str} }}
{user_crate_name} = {{ path = {user_path:?}, package = "{user_package_name}" }}
{build_deps}
[profile.release]
opt-level = "z"
strip = true
lto = true
codegen-units = 1
"#,
        installrs_path = INSTALLRS_CRATE_PATH,
        user_path = user_crate_path,
        user_package_name = user_crate_name.replace('_', "-"),
    );

    let (statics_code, entries_code) = generate_embedded_code(gathered)?;

    let subsystem_attr = match win_resource {
        Some(cfg) if cfg.windows_subsystem == "windows" => "#![windows_subsystem = \"windows\"]\n",
        _ => "",
    };

    let main_rs = format!(
        r#"// Code generated by installrs; DO NOT EDIT.
{subsystem_attr}{statics_code}
static ENTRIES: &[installrs::EmbeddedEntry] = &[
{entries_code}];
static UNINSTALLER_DATA: &[u8] = include_bytes!("../../uninstaller-bin");

fn main() {{
    let mut i = installrs::Installer::new(ENTRIES, UNINSTALLER_DATA, {compression:?});
    i.install_main({user_crate_name}::install);
}}
"#
    );

    write_if_changed(&installer_dir.join("Cargo.toml"), &cargo_toml)?;
    write_if_changed(&installer_dir.join("src").join("main.rs"), &main_rs)?;

    if let Some(cfg) = win_resource {
        write_build_rs(installer_dir, cfg)?;
    }

    Ok(())
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
                "{pad}installrs::DirChild {{ name: {name:?}, kind: installrs::DirChildKind::Dir {{ children: &[\n{children_code}{pad}] }} }},\n"
            ));
        } else {
            let ident = format!("D_{}", f.storage_name.replace('-', "_").to_uppercase());
            out.push_str(&format!(
                "{pad}installrs::DirChild {{ name: {name:?}, kind: installrs::DirChildKind::File {{ data: {ident}, compression: {:?} }} }},\n",
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
