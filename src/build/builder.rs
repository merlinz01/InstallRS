use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};

use super::compress;
use super::scanner;

// Embedded at compile time so the build tool is self-contained.
const INSTALLRS_CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

/// FNV-1a 64-bit hash — must stay identical to the copy in installrs/src/lib.rs.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}


pub struct BuildParams {
    pub target_dir: PathBuf,
    pub build_dir: PathBuf,
    pub output_file: PathBuf,
    pub compression: String,
    pub ignore_patterns: Vec<String>,
    pub target_triple: Option<String>,
}

struct GatheredFile {
    /// Path relative to target_dir, forward-slash separated
    source_path: String,
    /// File name inside files_dir (hash-compression, empty for dirs)
    storage_name: String,
    compression: String,
    is_dir: bool,
}

pub fn build(params: &BuildParams) -> Result<()> {
    log::info!("Starting build...");

    compress::validate_method(&params.compression)?;

    // ── Prepare directories ──────────────────────────────────────────────────
    std::fs::create_dir_all(&params.build_dir)
        .context("failed to create build directory")?;
    std::fs::write(params.build_dir.join(".gitignore"), "*\n")
        .context("failed to write .gitignore")?;

    let installer_dir = params.build_dir.join("installer");
    let uninstaller_dir = params.build_dir.join("uninstaller");
    let files_dir = installer_dir.join("files");
    let uninstaller_bin = params.build_dir.join("uninstaller-bin");

    std::fs::create_dir_all(&files_dir).context("failed to create files directory")?;
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
    let src_dir = abs_lib
        .parent()
        .unwrap_or(&params.target_dir)
        .to_path_buf();
    log::info!("Scanning source files in {}", src_dir.display());
    let scan = scanner::scan_source_dir(&src_dir)?;

    if !scan.has_install_fn {
        return Err(anyhow!("source must define a public `install` function"));
    }
    if !scan.has_uninstall_fn {
        return Err(anyhow!("source must define a public `uninstall` function"));
    }

    log::info!("Included files ({}):", scan.included_files.len());
    for f in &scan.included_files {
        log::info!("  {f}");
    }
    log::info!("Included directories ({}):", scan.included_dirs.len());
    for d in &scan.included_dirs {
        log::info!("  {d}");
    }

    // ── Gather and compress files ────────────────────────────────────────────
    let mut gathered: Vec<GatheredFile> = Vec::new();
    let mut hash_cache: HashMap<String, String> = HashMap::new(); // hash -> storage_name

    for file_path in &scan.included_files {
        let abs = params.target_dir.join(file_path);
        gather_file(
            file_path,
            &abs,
            &files_dir,
            &params.compression,
            &params.ignore_patterns,
            &mut gathered,
            &mut hash_cache,
        )?;
    }
    for dir_path in &scan.included_dirs {
        let abs = params.target_dir.join(dir_path);
        gather_dir(
            dir_path,
            &abs,
            &files_dir,
            &params.compression,
            &params.ignore_patterns,
            &mut gathered,
            &mut hash_cache,
        )?;
    }
    log::info!("Total entries gathered: {}", gathered.len());

    // ── Compile uninstaller ──────────────────────────────────────────────────
    write_uninstaller_sources(&uninstaller_dir, &user_crate_name, &params.target_dir, "none")?;
    compile_cargo_project(&uninstaller_dir, params.target_triple.as_deref())?;

    // Copy compiled uninstaller to known path
    let compiled = uninstaller_dir
        .join("target")
        .join(if let Some(t) = &params.target_triple {
            format!("{}/release", t)
        } else {
            "release".to_string()
        })
        .join(if cfg!(target_os = "windows") {
            "uninstaller.exe"
        } else {
            "uninstaller"
        });
    let uninstaller_raw = std::fs::read(&compiled)
        .with_context(|| format!("failed to read uninstaller from {}", compiled.display()))?;
    let uninstaller_compressed = compress::compress(&uninstaller_raw, &params.compression)
        .context("failed to compress uninstaller binary")?;
    std::fs::write(&uninstaller_bin, &uninstaller_compressed)
        .with_context(|| format!("failed to write compressed uninstaller to {}", uninstaller_bin.display()))?;
    log::info!("Uninstaller binary ready: {} (compression: {})", uninstaller_bin.display(), params.compression);

    // ── Prune stale cached files ─────────────────────────────────────────────
    prune_files_dir(&files_dir, &gathered)?;

    // ── Write installer sources and compile ──────────────────────────────────
    write_installer_sources(
        &installer_dir,
        &user_crate_name,
        &params.target_dir,
        &gathered,
        &params.compression,
    )?;
    compile_cargo_project(&installer_dir, params.target_triple.as_deref())?;

    // Copy final binary to output path
    let compiled_installer = installer_dir
        .join("target")
        .join(if let Some(t) = &params.target_triple {
            format!("{}/release", t)
        } else {
            "release".to_string()
        })
        .join(if cfg!(target_os = "windows") {
            "installer-generated.exe"
        } else {
            "installer-generated"
        });
    std::fs::copy(&compiled_installer, &params.output_file)
        .with_context(|| format!("failed to copy installer to {}", params.output_file.display()))?;

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
        return Err(anyhow!("expected a file but got a directory: {source_path}"));
    }

    let data = std::fs::read(abs_path)
        .with_context(|| format!("failed to read: {}", abs_path.display()))?;

    let hash = hex::encode(Sha256::digest(&data));

    let storage_name = format!("{hash}-{compression}");
    let storage_path = files_dir.join(&storage_name);

    if !hash_cache.contains_key(&storage_name) && !storage_path.exists() {
        let compressed = compress::compress(&data, compression)
            .with_context(|| format!("failed to compress: {source_path}"))?;
        std::fs::write(&storage_path, &compressed)
            .with_context(|| format!("failed to write cache: {}", storage_path.display()))?;
        log::debug!("Compressed {source_path} → {storage_name}");
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
    if gathered.iter().any(|f| f.source_path == source_path && f.is_dir) {
        return Ok(());
    }

    let stat = std::fs::metadata(abs_path)
        .with_context(|| format!("failed to stat: {}", abs_path.display()))?;
    if !stat.is_dir() {
        return Err(anyhow!("expected a directory but got a file: {source_path}"));
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

fn write_uninstaller_sources(
    uninstaller_dir: &Path,
    user_crate_name: &str,
    user_crate_path: &Path,
    compression: &str,
) -> Result<()> {
    log::debug!("Writing uninstaller sources");

    let features_str = match compression_feature(compression) {
        Some(f) => format!(", default-features = false, features = [{f:?}]"),
        None => ", default-features = false".to_string(),
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

    let main_rs = format!(
        r#"// Code generated by installrs; DO NOT EDIT.
fn main() {{
    let mut i = installrs::Installer::new(&[], &[], "none");
    i.uninstall_main({user_crate_name}::uninstall);
}}
"#
    );

    std::fs::write(uninstaller_dir.join("Cargo.toml"), &cargo_toml)
        .context("failed to write uninstaller Cargo.toml")?;
    std::fs::write(uninstaller_dir.join("src").join("main.rs"), &main_rs)
        .context("failed to write uninstaller main.rs")?;

    Ok(())
}

fn write_installer_sources(
    installer_dir: &Path,
    user_crate_name: &str,
    user_crate_path: &Path,
    gathered: &[GatheredFile],
    compression: &str,
) -> Result<()> {
    log::debug!("Writing installer sources");

    let features_str = match compression_feature(compression) {
        Some(f) => format!(", default-features = false, features = [{f:?}]"),
        None => ", default-features = false".to_string(),
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

    // One named static per unique storage file so that files with identical
    // content share a single &[u8] reference in the binary (content-addressed
    // deduplication).
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

    // Identify root entries: root files are those not under any included dir,
    // root dirs are the top-level included directories.
    let dir_prefixes: Vec<&str> = gathered
        .iter()
        .filter(|f| f.is_dir)
        .filter(|f| {
            // A root dir has no parent that is also a gathered dir
            !gathered.iter().any(|other| {
                other.is_dir
                    && other.source_path != f.source_path
                    && f.source_path.starts_with(&format!("{}/", other.source_path))
            })
        })
        .map(|f| f.source_path.as_str())
        .collect();

    let root_files: Vec<&GatheredFile> = gathered
        .iter()
        .filter(|f| !f.is_dir)
        .filter(|f| {
            // Not under any root dir
            !dir_prefixes
                .iter()
                .any(|dp| f.source_path.starts_with(&format!("{dp}/")))
        })
        .collect();

    // Check for path hash collisions among root entries only
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

    // Build the ENTRIES array with tree-structured entries
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

    let main_rs = format!(
        r#"// Code generated by installrs; DO NOT EDIT.
{statics_code}
static ENTRIES: &[installrs::EmbeddedEntry] = &[
{entries_code}];
static UNINSTALLER_DATA: &[u8] = include_bytes!("../../uninstaller-bin");

fn main() {{
    let mut i = installrs::Installer::new(ENTRIES, UNINSTALLER_DATA, {compression:?});
    i.install_main({user_crate_name}::install);
}}
"#
    );

    std::fs::write(installer_dir.join("Cargo.toml"), &cargo_toml)
        .context("failed to write installer Cargo.toml")?;
    std::fs::write(installer_dir.join("src").join("main.rs"), &main_rs)
        .context("failed to write installer main.rs")?;

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

fn compile_cargo_project(project_dir: &Path, target_triple: Option<&str>) -> Result<()> {
    log::info!("Compiling {}", project_dir.display());

    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("build").arg("--release");
    if let Some(triple) = target_triple {
        cmd.args(["--target", triple]);
    }
    cmd.current_dir(project_dir);

    let status = cmd
        .status()
        .with_context(|| format!("failed to run cargo in {}", project_dir.display()))?;

    if !status.success() {
        return Err(anyhow!(
            "cargo build failed in {}",
            project_dir.display()
        ));
    }

    Ok(())
}
