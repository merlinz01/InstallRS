#[cfg(target_os = "windows")]
#[path = "self_destruct_windows.rs"]
mod self_destruct;

#[cfg(not(target_os = "windows"))]
mod self_destruct;

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

/// FNV-1a 64-bit hash of a (normalized) path string.
/// Must stay identical to the copy in installrs-build.
pub fn path_hash(path: &str) -> u64 {
    let normalized = path.replace('\\', "/");
    let mut h: u64 = 14695981039346656037;
    for b in normalized.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

/// An embedded file entry baked into the installer binary at compile time.
/// Source paths are stored only as hashes so build-time paths are not
/// visible as strings in the output binary.
pub struct EmbeddedFileEntry {
    /// FNV-1a hash of the full source path
    pub path_hash: u64,
    /// FNV-1a hash of the parent directory's source path
    pub parent_hash: u64,
    /// Bare file or directory name (last path component only)
    pub name: &'static str,
    /// Raw (possibly compressed) file data
    pub data: &'static [u8],
    /// Compression used: "lzma", "gzip", "bzip2", or ""
    pub compression: &'static str,
    /// True if this entry represents a directory rather than a file
    pub is_dir: bool,
}

pub struct Installer {
    pub headless: bool,
    files: &'static [EmbeddedFileEntry],
    in_dir: PathBuf,
    out_dir: Option<PathBuf>,
    uninstaller_data: &'static [u8],
    uninstaller_compression: &'static str,
}

impl Installer {
    pub fn new(
        files: &'static [EmbeddedFileEntry],
        uninstaller_data: &'static [u8],
        uninstaller_compression: &'static str,
    ) -> Self {
        Installer {
            headless: false,
            files,
            in_dir: PathBuf::new(),
            out_dir: None,
            uninstaller_data,
            uninstaller_compression,
        }
    }

    /// Set the base output directory for relative destination paths.
    pub fn set_out_dir(&mut self, dir: &str) {
        self.out_dir = Some(PathBuf::from(dir));
    }

    /// Set the base input directory for relative source paths.
    /// Also detected by the build tool to resolve which files to embed.
    pub fn set_in_dir(&mut self, dir: &str) {
        self.in_dir = PathBuf::from(dir);
    }

    /// Hint the build tool to embed a specific file. No-op at runtime.
    pub fn include_file(&self, _source_path: &str) {}

    /// Hint the build tool to embed a directory. No-op at runtime.
    pub fn include_dir(&self, _source_path: &str) {}

    fn resolve_in_path(&self, source_path: &str) -> PathBuf {
        let p = Path::new(source_path);
        if p.is_absolute() || self.in_dir.as_os_str().is_empty() {
            p.to_path_buf()
        } else {
            self.in_dir.join(p)
        }
    }

    fn resolve_out_path(&self, dest_path: &str) -> Result<PathBuf> {
        let p = Path::new(dest_path);
        if p.is_absolute() {
            return Ok(p.to_path_buf());
        }
        let out = self
            .out_dir
            .as_ref()
            .ok_or_else(|| anyhow!("output directory not set; call set_out_dir() first"))?;
        Ok(out.join(p))
    }

    fn lookup_hash(&self, h: u64) -> Option<&'static EmbeddedFileEntry> {
        self.files.iter().find(|e| e.path_hash == h)
    }

    fn decompress(data: &[u8], compression: &str) -> Result<Vec<u8>> {
        use std::io::Read;
        match compression {
            "" | "none" => Ok(data.to_vec()),
            "lzma" => {
                let mut out = Vec::new();
                lzma_rs::lzma_decompress(&mut std::io::Cursor::new(data), &mut out)
                    .context("LZMA decompression failed")?;
                Ok(out)
            }
            "gzip" => {
                let mut decoder = flate2::read::GzDecoder::new(data);
                let mut out = Vec::new();
                decoder.read_to_end(&mut out).context("gzip decompression failed")?;
                Ok(out)
            }
            "bzip2" => {
                let mut decoder = bzip2::read::BzDecoder::new(data);
                let mut out = Vec::new();
                decoder.read_to_end(&mut out).context("bzip2 decompression failed")?;
                Ok(out)
            }
            other => Err(anyhow!("unsupported compression: {other}")),
        }
    }

    /// Install a single embedded file to the destination path.
    pub fn file(&self, source_path: &str, dest_path: &str) -> Result<()> {
        let source = self.resolve_in_path(source_path);
        let h = path_hash(&source.to_string_lossy());
        let dest = self.resolve_out_path(dest_path)?;

        let entry = self
            .lookup_hash(h)
            .ok_or_else(|| anyhow!("file not embedded in installer: {}", source.display()))?;

        if entry.is_dir {
            return Err(anyhow!(
                "expected a file but path is a directory: {}",
                source.display()
            ));
        }

        let data = Self::decompress(entry.data, entry.compression)?;
        write_file(&dest, &data)
    }

    /// Install an embedded directory tree to the destination path.
    pub fn dir(&self, source_path: &str, dest_path: &str) -> Result<()> {
        let source = self.resolve_in_path(source_path);
        let base_hash = path_hash(&source.to_string_lossy());
        let dest = self.resolve_out_path(dest_path)?;

        std::fs::create_dir_all(&dest)
            .with_context(|| format!("failed to create directory: {}", dest.display()))?;

        self.install_children(base_hash, &dest)
    }

    fn install_children(&self, parent_hash: u64, dest: &Path) -> Result<()> {
        for entry in self.files {
            if entry.parent_hash != parent_hash {
                continue;
            }
            let target = dest.join(entry.name);
            if entry.is_dir {
                std::fs::create_dir_all(&target)
                    .with_context(|| format!("failed to create dir: {}", target.display()))?;
                self.install_children(entry.path_hash, &target)?;
            } else {
                let data = Self::decompress(entry.data, entry.compression)?;
                write_file(&target, &data)?;
            }
        }
        Ok(())
    }

    /// Create a directory (and all parents) on the target system.
    pub fn mkdir(&self, dir: &str) -> Result<()> {
        let path = self.resolve_out_path(dir)?;
        std::fs::create_dir_all(&path)
            .with_context(|| format!("failed to create directory: {}", path.display()))
    }

    /// Write the embedded uninstaller executable to the destination path.
    pub fn uninstaller(&self, dest_path: &str) -> Result<()> {
        let dest = self.resolve_out_path(dest_path)?;
        let data = Self::decompress(self.uninstaller_data, self.uninstaller_compression)?;
        write_file(&dest, &data)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
                .with_context(|| format!("failed to set permissions on: {}", dest.display()))?;
        }
        Ok(())
    }

    /// Remove a file or directory from the target system.
    pub fn remove(&self, path: &str) -> Result<()> {
        let p = self.resolve_out_path(path)?;
        if !p.exists() {
            return Ok(());
        }
        if p.is_dir() {
            std::fs::remove_dir_all(&p)
                .with_context(|| format!("failed to remove directory: {}", p.display()))
        } else {
            std::fs::remove_file(&p)
                .with_context(|| format!("failed to remove file: {}", p.display()))
        }
    }

    /// Check whether a path exists on the target system.
    pub fn exists(&self, path: &str) -> Result<bool> {
        let p = self.resolve_out_path(path)?;
        Ok(p.exists())
    }

    /// Run a shell command via the system shell and wait for it to complete.
    pub fn exec_shell(&self, command: &str) -> Result<()> {
        let status = if cfg!(target_os = "windows") {
            std::process::Command::new("cmd")
                .args(["/C", command])
                .status()
        } else {
            std::process::Command::new("sh")
                .args(["-c", command])
                .status()
        };
        let status = status.with_context(|| format!("failed to spawn: {command}"))?;
        if !status.success() {
            return Err(anyhow!("command exited with {status}: {command}"));
        }
        Ok(())
    }

    /// Entry point for installer binaries. Call this from `main()`.
    pub fn install_main(&mut self, install_fn: impl Fn(&mut Installer) -> Result<()>) {
        self.headless = std::env::args().any(|a| a == "--headless");
        if let Err(e) = install_fn(self) {
            eprintln!("Error: {e:#}");
            std::process::exit(1);
        }
    }

    /// Entry point for uninstaller binaries. Call this from `main()`.
    pub fn uninstall_main(&mut self, uninstall_fn: impl Fn(&mut Installer) -> Result<()>) {
        self.headless = std::env::args().any(|a| a == "--headless");
        self_destruct::prepare();
        if let Err(e) = uninstall_fn(self) {
            eprintln!("Error: {e:#}");
            std::process::exit(1);
        }
        self_destruct::destruct();
        std::process::exit(0);
    }
}

fn write_file(dest: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir for: {}", dest.display()))?;
    }
    std::fs::write(dest, data)
        .with_context(|| format!("failed to write: {}", dest.display()))
}
