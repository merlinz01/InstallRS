#[cfg(feature = "gui")]
pub mod gui;

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

/// Compile-time FNV-1a 64-bit hash of a path string (backslashes normalized to forward slashes).
///
/// Used internally by the [`file!`] and [`dir!`] macros.
pub const fn source_path_hash_const(path: &str) -> u64 {
    let bytes = path.as_bytes();
    let mut h: u64 = 14695981039346656037;
    let mut i = 0;
    while i < bytes.len() {
        let b = if bytes[i] == b'\\' { b'/' } else { bytes[i] };
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
        i += 1;
    }
    h
}

/// Install a single embedded file to a destination path.
///
/// The source path is hashed at **compile time** — it never appears as a string
/// in the final binary. Paths are relative to the project root (as seen by the
/// build tool).
///
/// ```rust,ignore
/// installrs::file!(i, "assets/config.toml", "etc/myapp/config.toml")?;
/// ```
#[macro_export]
macro_rules! file {
    ($installer:expr, $src:literal, $dst:expr) => {{
        const H: u64 = $crate::source_path_hash_const($src);
        $installer.file_hashed(H, $dst)
    }};
}

/// Install an embedded directory tree to a destination path.
///
/// The source path is hashed at **compile time** — it never appears as a string
/// in the final binary. Paths are relative to the project root (as seen by the
/// build tool).
///
/// ```rust,ignore
/// installrs::dir!(i, "assets/icons", "share/myapp/icons")?;
/// ```
#[macro_export]
macro_rules! dir {
    ($installer:expr, $src:literal, $dst:expr) => {{
        const H: u64 = $crate::source_path_hash_const($src);
        $installer.dir_hashed(H, $dst)
    }};
}

/// A top-level embedded entry baked into the installer binary at compile time.
///
/// `File` entries store a single file keyed by path hash — no filename is
/// retained. `Dir` entries store a recursive tree of children with names
/// for directory traversal.
pub enum EmbeddedEntry {
    File {
        source_path_hash: u64,
        data: &'static [u8],
        compression: &'static str,
    },
    Dir {
        source_path_hash: u64,
        children: &'static [DirChild],
    },
}

/// A named child inside an [`EmbeddedEntry::Dir`] tree.
pub struct DirChild {
    pub name: &'static str,
    pub kind: DirChildKind,
}

/// The payload of a [`DirChild`] — either file data or a nested directory.
pub enum DirChildKind {
    File {
        data: &'static [u8],
        compression: &'static str,
    },
    Dir {
        children: &'static [DirChild],
    },
}

pub struct Installer {
    pub headless: bool,
    entries: &'static [EmbeddedEntry],
    out_dir: Option<PathBuf>,
    uninstaller_data: &'static [u8],
    uninstaller_compression: &'static str,
    #[cfg(target_os = "windows")]
    self_delete: bool,
}

impl Installer {
    pub fn new(
        entries: &'static [EmbeddedEntry],
        uninstaller_data: &'static [u8],
        uninstaller_compression: &'static str,
    ) -> Self {
        Installer {
            headless: false,
            entries,
            out_dir: None,
            uninstaller_data,
            uninstaller_compression,
            #[cfg(target_os = "windows")]
            self_delete: false,
        }
    }

    /// Set the base output directory for relative destination paths.
    pub fn set_out_dir(&mut self, dir: &str) {
        self.out_dir = Some(PathBuf::from(dir));
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

    fn decompress(data: &[u8], compression: &str) -> Result<Vec<u8>> {
        #[allow(unused_imports)]
        use std::io::Read;
        match compression {
            "" | "none" => Ok(data.to_vec()),
            #[cfg(feature = "lzma")]
            "lzma" => {
                let mut out = Vec::new();
                lzma_rs::lzma_decompress(&mut std::io::Cursor::new(data), &mut out)
                    .context("LZMA decompression failed")?;
                Ok(out)
            }
            #[cfg(feature = "gzip")]
            "gzip" => {
                let mut decoder = flate2::read::GzDecoder::new(data);
                let mut out = Vec::new();
                decoder
                    .read_to_end(&mut out)
                    .context("gzip decompression failed")?;
                Ok(out)
            }
            #[cfg(feature = "bzip2")]
            "bzip2" => {
                let mut decoder = bzip2::read::BzDecoder::new(data);
                let mut out = Vec::new();
                decoder
                    .read_to_end(&mut out)
                    .context("bzip2 decompression failed")?;
                Ok(out)
            }
            other => Err(anyhow!("unsupported compression: {other}")),
        }
    }

    /// Install a file by pre-computed path hash. Prefer the [`file!`] macro.
    pub fn file_hashed(&self, source_hash: u64, dest_path: &str) -> Result<()> {
        let dest = self.resolve_out_path(dest_path)?;
        for entry in self.entries {
            if let EmbeddedEntry::File {
                source_path_hash,
                data,
                compression,
            } = entry
            {
                if *source_path_hash == source_hash {
                    let bytes = Self::decompress(data, compression)?;
                    return write_file(&dest, &bytes);
                }
            }
        }
        Err(anyhow!(
            "file not embedded in installer (hash: {source_hash:#018x})"
        ))
    }

    /// Install a directory tree by pre-computed path hash. Prefer the [`dir!`] macro.
    pub fn dir_hashed(&self, source_hash: u64, dest_path: &str) -> Result<()> {
        let dest = self.resolve_out_path(dest_path)?;
        for entry in self.entries {
            if let EmbeddedEntry::Dir {
                source_path_hash,
                children,
            } = entry
            {
                if *source_path_hash == source_hash {
                    std::fs::create_dir_all(&dest).with_context(|| {
                        format!("failed to create directory: {}", dest.display())
                    })?;
                    return Self::install_children(children, &dest);
                }
            }
        }
        Err(anyhow!(
            "directory not embedded in installer (hash: {source_hash:#018x})"
        ))
    }

    fn install_children(children: &[DirChild], dest: &Path) -> Result<()> {
        for child in children {
            let target = dest.join(child.name);
            match &child.kind {
                DirChildKind::File { data, compression } => {
                    let bytes = Self::decompress(data, compression)?;
                    write_file(&target, &bytes)?;
                }
                DirChildKind::Dir { children } => {
                    std::fs::create_dir_all(&target)
                        .with_context(|| format!("failed to create dir: {}", target.display()))?;
                    Self::install_children(children, &target)?;
                }
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

    /// Enable self-deletion of the executable after it finishes.
    ///
    /// On Windows this copies the running executable to a temporary directory and
    /// relaunches from there (with `--self-delete`) so the original file can
    /// be removed during uninstallation. After `uninstall_main` returns, a
    /// PowerShell process cleans up the temp copy.
    ///
    /// This method is only available on Windows. On Unix, use `i.remove()` to
    /// delete the uninstaller as part of your normal cleanup.
    #[cfg(target_os = "windows")]
    pub fn enable_self_delete(&mut self) {
        if std::env::args().any(|a| a == "--self-delete") {
            self.self_delete = true;
            return;
        }

        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Error getting executable path: {e}");
                std::process::exit(1);
            }
        };

        let tmp_dir = std::env::temp_dir().join(format!("uninstall-{}", std::process::id()));
        if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
            eprintln!("Error creating temp dir: {e}");
            std::process::exit(1);
        }

        let tmp_exe = tmp_dir.join("uninstaller.exe");
        if let Err(e) = std::fs::copy(&exe, &tmp_exe) {
            eprintln!("Error copying to temp: {e}");
            std::process::exit(1);
        }

        let mut args: Vec<String> = std::env::args().skip(1).collect();
        args.push("--self-delete".to_string());

        match std::process::Command::new(&tmp_exe).args(&args).spawn() {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error spawning temp uninstaller: {e}");
                std::process::exit(1);
            }
        }

        std::process::exit(0);
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
        if let Err(e) = uninstall_fn(self) {
            eprintln!("Error: {e:#}");
            std::process::exit(1);
        }
        #[cfg(target_os = "windows")]
        if self.self_delete {
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    let dir = dir.to_string_lossy().into_owned();
                    let _ = std::process::Command::new("powershell")
                        .args([
                            "-ExecutionPolicy",
                            "Bypass",
                            "-Command",
                            &format!("Start-Sleep 5; Remove-Item -Path '{}' -Recurse -Force", dir),
                        ])
                        .spawn();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn leak_entries(entries: Vec<EmbeddedEntry>) -> &'static [EmbeddedEntry] {
        Box::leak(entries.into_boxed_slice())
    }

    fn leak_children(children: Vec<DirChild>) -> &'static [DirChild] {
        Box::leak(children.into_boxed_slice())
    }

    fn leak_bytes(data: Vec<u8>) -> &'static [u8] {
        Box::leak(data.into_boxed_slice())
    }

    fn make_installer(entries: Vec<EmbeddedEntry>, out_dir: &std::path::Path) -> Installer {
        let mut i = Installer::new(leak_entries(entries), leak_bytes(vec![]), "");
        i.set_out_dir(&out_dir.to_string_lossy());
        i
    }

    fn compress_gzip(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(data).unwrap();
        enc.finish().unwrap()
    }

    fn compress_lzma(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        lzma_rs::lzma_compress(&mut std::io::Cursor::new(data), &mut out).unwrap();
        out
    }

    fn compress_bzip2(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
        enc.write_all(data).unwrap();
        enc.finish().unwrap()
    }

    fn file_entry(path: &str, data: &'static [u8]) -> EmbeddedEntry {
        let norm = path.replace('\\', "/");
        EmbeddedEntry::File {
            source_path_hash: source_path_hash_const(&norm),
            data,
            compression: "",
        }
    }

    fn dir_entry(path: &str, children: Vec<DirChild>) -> EmbeddedEntry {
        let norm = path.replace('\\', "/");
        EmbeddedEntry::Dir {
            source_path_hash: source_path_hash_const(&norm),
            children: leak_children(children),
        }
    }

    fn child_file(name: &str, data: &'static [u8]) -> DirChild {
        DirChild {
            name: Box::leak(name.to_string().into_boxed_str()),
            kind: DirChildKind::File {
                data,
                compression: "",
            },
        }
    }

    fn child_dir(name: &str, children: Vec<DirChild>) -> DirChild {
        DirChild {
            name: Box::leak(name.to_string().into_boxed_str()),
            kind: DirChildKind::Dir {
                children: leak_children(children),
            },
        }
    }

    // ── source_path_hash_const ───────────────────────────────────────────────────────

    #[test]
    fn source_path_hash_const_is_stable() {
        assert_eq!(
            source_path_hash_const("foo/bar.txt"),
            source_path_hash_const("foo/bar.txt")
        );
    }

    #[test]
    fn source_path_hash_const_normalizes_backslashes() {
        assert_eq!(
            source_path_hash_const("foo\\bar.txt"),
            source_path_hash_const("foo/bar.txt")
        );
    }

    #[test]
    fn source_path_hash_const_different_inputs_differ() {
        assert_ne!(
            source_path_hash_const("a.txt"),
            source_path_hash_const("b.txt")
        );
        assert_ne!(source_path_hash_const(""), source_path_hash_const("a"));
    }

    #[test]
    fn source_path_hash_const_known_value() {
        let expected: u64 = {
            let mut h: u64 = 14695981039346656037;
            for b in "hello".bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(1099511628211);
            }
            h
        };
        assert_eq!(source_path_hash_const("hello"), expected);
    }

    // ── file! macro ───────────────────────────────────────────────────────────

    #[test]
    fn file_macro_writes_content() {
        let tmp = tempfile::TempDir::new().unwrap();
        let i = make_installer(vec![file_entry("vendor/lib.so", b"ELF")], tmp.path());
        file!(i, "vendor/lib.so", "lib.so").unwrap();
        assert_eq!(std::fs::read(tmp.path().join("lib.so")).unwrap(), b"ELF");
    }

    #[test]
    fn file_macro_creates_parent_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let i = make_installer(vec![file_entry("data.txt", b"x")], tmp.path());
        file!(i, "data.txt", "a/b/out.txt").unwrap();
        assert!(tmp.path().join("a/b/out.txt").exists());
    }

    #[test]
    fn file_macro_error_when_not_embedded() {
        let tmp = tempfile::TempDir::new().unwrap();
        let i = make_installer(vec![], tmp.path());
        assert!(file!(i, "missing.txt", "out.txt").is_err());
    }

    #[test]
    fn file_macro_decompresses_on_write() {
        let tmp = tempfile::TempDir::new().unwrap();
        let original = b"compressed content";
        let compressed = compress_gzip(original);
        let data: &'static [u8] = leak_bytes(compressed);
        let entry = EmbeddedEntry::File {
            source_path_hash: source_path_hash_const("comp.gz"),
            data,
            compression: "gzip",
        };
        let i = make_installer(vec![entry], tmp.path());
        file!(i, "comp.gz", "out.txt").unwrap();
        assert_eq!(std::fs::read(tmp.path().join("out.txt")).unwrap(), original);
    }

    // ── dir! macro ────────────────────────────────────────────────────────────

    #[test]
    fn dir_macro_installs_tree() {
        let tmp = tempfile::TempDir::new().unwrap();
        let entries = vec![dir_entry(
            "pkg",
            vec![
                child_file("readme.txt", b"readme"),
                child_dir("bin", vec![child_file("app", b"binary")]),
            ],
        )];
        let i = make_installer(entries, tmp.path());
        dir!(i, "pkg", "out").unwrap();
        assert_eq!(
            std::fs::read(tmp.path().join("out/readme.txt")).unwrap(),
            b"readme"
        );
        assert_eq!(
            std::fs::read(tmp.path().join("out/bin/app")).unwrap(),
            b"binary"
        );
    }

    #[test]
    fn dir_macro_installs_flat_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        let entries = vec![dir_entry("assets", vec![child_file("logo.png", b"PNG")])];
        let i = make_installer(entries, tmp.path());
        dir!(i, "assets", "out").unwrap();
        assert_eq!(
            std::fs::read(tmp.path().join("out/logo.png")).unwrap(),
            b"PNG"
        );
    }

    // ── decompress ───────────────────────────────────────────────────────────

    const SAMPLE: &[u8] = b"Hello, InstallRS test data! Hello, InstallRS!";

    #[test]
    fn decompress_none_empty() {
        assert_eq!(Installer::decompress(SAMPLE, "").unwrap(), SAMPLE);
    }

    #[test]
    fn decompress_none_explicit() {
        assert_eq!(Installer::decompress(SAMPLE, "none").unwrap(), SAMPLE);
    }

    #[test]
    fn decompress_lzma_roundtrip() {
        let compressed = compress_lzma(SAMPLE);
        assert_eq!(Installer::decompress(&compressed, "lzma").unwrap(), SAMPLE);
    }

    #[test]
    fn decompress_gzip_roundtrip() {
        let compressed = compress_gzip(SAMPLE);
        assert_eq!(Installer::decompress(&compressed, "gzip").unwrap(), SAMPLE);
    }

    #[test]
    fn decompress_bzip2_roundtrip() {
        let compressed = compress_bzip2(SAMPLE);
        assert_eq!(Installer::decompress(&compressed, "bzip2").unwrap(), SAMPLE);
    }

    #[test]
    fn decompress_unknown_method_errors() {
        let err = Installer::decompress(b"data", "zstd").unwrap_err();
        assert!(err.to_string().contains("unsupported compression"));
    }

    // ── Installer::mkdir() ───────────────────────────────────────────────────

    #[test]
    fn mkdir_creates_nested_directories() {
        let tmp = tempfile::TempDir::new().unwrap();
        let i = make_installer(vec![], tmp.path());
        i.mkdir("a/b/c/d").unwrap();
        assert!(tmp.path().join("a/b/c/d").is_dir());
    }

    #[test]
    fn mkdir_is_idempotent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let i = make_installer(vec![], tmp.path());
        i.mkdir("exists").unwrap();
        i.mkdir("exists").unwrap();
    }

    #[test]
    fn mkdir_requires_out_dir() {
        let i = Installer::new(leak_entries(vec![]), leak_bytes(vec![]), "none");
        assert!(i.mkdir("foo").is_err());
    }

    // ── Installer::remove() ──────────────────────────────────────────────────

    #[test]
    fn remove_deletes_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("victim.txt"), b"x").unwrap();
        let i = make_installer(vec![], tmp.path());
        i.remove("victim.txt").unwrap();
        assert!(!tmp.path().join("victim.txt").exists());
    }

    #[test]
    fn remove_deletes_directory_recursively() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("tree/leaf")).unwrap();
        std::fs::write(tmp.path().join("tree/leaf/f.txt"), b"x").unwrap();
        let i = make_installer(vec![], tmp.path());
        i.remove("tree").unwrap();
        assert!(!tmp.path().join("tree").exists());
    }

    #[test]
    fn remove_noop_when_nonexistent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let i = make_installer(vec![], tmp.path());
        i.remove("does_not_exist.txt").unwrap();
    }

    // ── Installer::exists() ──────────────────────────────────────────────────

    #[test]
    fn exists_true_for_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("present.txt"), b"hi").unwrap();
        let i = make_installer(vec![], tmp.path());
        assert!(i.exists("present.txt").unwrap());
    }

    #[test]
    fn exists_false_for_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let i = make_installer(vec![], tmp.path());
        assert!(!i.exists("absent.txt").unwrap());
    }

    #[test]
    fn exists_true_for_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("mydir")).unwrap();
        let i = make_installer(vec![], tmp.path());
        assert!(i.exists("mydir").unwrap());
    }
}

fn write_file(dest: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir for: {}", dest.display()))?;
    }
    std::fs::write(dest, data).with_context(|| format!("failed to write: {}", dest.display()))
}
