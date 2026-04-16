#[cfg(feature = "gui")]
pub mod gui;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};

/// Compile-time FNV-1a 64-bit hash of a path string (backslashes normalized to forward slashes).
///
/// Used by the [`source!`] macro.
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

/// Compile-time reference to an embedded file or directory.
///
/// Create one with the [`source!`] macro, then pass it to
/// [`Installer::file`] or [`Installer::dir`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Source(pub u64);

/// Produce a [`Source`] from a literal path, hashed at compile time.
///
/// The path string itself never appears in the final binary. Paths are
/// relative to the project root as seen by the build tool.
///
/// ```rust,ignore
/// i.file(installrs::source!("assets/config.toml"), "etc/myapp/config.toml")
///     .install()?;
/// ```
#[macro_export]
macro_rules! source {
    ($path:literal) => {{
        const H: u64 = $crate::source_path_hash_const($path);
        $crate::Source(H)
    }};
}

/// A top-level embedded entry baked into the installer binary at compile time.
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

/// What to do when a destination file already exists.
#[derive(Default, Copy, Clone, Debug, PartialEq, Eq)]
pub enum OverwriteMode {
    /// Replace the existing file (default).
    #[default]
    Overwrite,
    /// Leave the existing file untouched and skip this file.
    Skip,
    /// Return an error if the destination exists.
    Error,
    /// Rename the existing file to `<name>.bak` (replacing any existing backup) before writing.
    Backup,
}

/// Decision returned from an on-error handler inside a directory install.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ErrorAction {
    /// Skip this file and continue installing the rest of the directory.
    Skip,
    /// Abort the directory install by propagating the error.
    Abort,
}

/// Filter closure for directory installs. Receives the relative path within
/// the directory (e.g. `"bin/app.exe"`) and returns `true` to include the file.
pub type DirFilter = Box<dyn Fn(&str) -> bool + 'static>;

/// Per-file error handler for directory installs.
pub type DirErrorHandler = Box<dyn Fn(&str, &anyhow::Error) -> ErrorAction + 'static>;

/// Sink for progress, status, and log events emitted by installer operations.
///
/// Attach one to an [`Installer`] with [`Installer::set_progress_sink`] (the
/// wizard GUI does this automatically inside the install page).
pub trait ProgressSink: Send + Sync {
    fn set_status(&self, status: &str);
    fn set_progress(&self, fraction: f64);
    fn log(&self, message: &str);
}

struct ProgressState {
    bytes_installed: u64,
    bytes_total: u64,
}

pub struct Installer {
    pub headless: bool,
    entries: &'static [EmbeddedEntry],
    out_dir: Option<PathBuf>,
    uninstaller_data: &'static [u8],
    uninstaller_compression: &'static str,
    #[cfg(target_os = "windows")]
    self_delete: bool,
    sink: Option<Box<dyn ProgressSink>>,
    progress: Mutex<ProgressState>,
}

impl Installer {
    pub fn new(
        entries: &'static [EmbeddedEntry],
        uninstaller_data: &'static [u8],
        uninstaller_compression: &'static str,
    ) -> Self {
        // Default total = sum of all embedded bytes (uncompressed size is
        // unknown without decompressing, so use raw data length as a proxy,
        // plus the uninstaller).
        let total = sum_embedded_bytes(entries) + uninstaller_data.len() as u64;
        Installer {
            headless: false,
            entries,
            out_dir: None,
            uninstaller_data,
            uninstaller_compression,
            #[cfg(target_os = "windows")]
            self_delete: false,
            sink: None,
            progress: Mutex::new(ProgressState {
                bytes_installed: 0,
                bytes_total: total,
            }),
        }
    }

    /// Set the base output directory for relative destination paths.
    pub fn set_out_dir(&mut self, dir: &str) {
        self.out_dir = Some(PathBuf::from(dir));
    }

    /// Attach a [`ProgressSink`] that receives status, progress, and log events.
    pub fn set_progress_sink(&mut self, sink: Box<dyn ProgressSink>) {
        self.sink = Some(sink);
    }

    /// Remove the progress sink (if any).
    pub fn clear_progress_sink(&mut self) {
        self.sink = None;
    }

    /// Override the total byte count used for progress reporting.
    ///
    /// By default, total = sum of all embedded file bytes + uninstaller bytes.
    /// Override this if you plan to install only a subset of the embedded
    /// content and want progress to reach 100% at the end.
    pub fn set_total_bytes(&mut self, total: u64) {
        self.progress.lock().unwrap().bytes_total = total;
    }

    /// Reset the bytes-installed counter to zero.
    pub fn reset_progress(&mut self) {
        self.progress.lock().unwrap().bytes_installed = 0;
    }

    /// Sum the uncompressed byte size of one or more [`Source`]s.
    ///
    /// Useful for budgeting a custom total, e.g.
    /// `i.set_total_bytes(i.bytes_of(&[source!("a"), source!("b")]))`.
    pub fn bytes_of(&self, sources: &[Source]) -> u64 {
        let mut total = 0u64;
        for s in sources {
            total += self.bytes_of_source(*s);
        }
        total
    }

    fn bytes_of_source(&self, source: Source) -> u64 {
        for entry in self.entries {
            match entry {
                EmbeddedEntry::File {
                    source_path_hash,
                    data,
                    ..
                } if *source_path_hash == source.0 => {
                    return data.len() as u64;
                }
                EmbeddedEntry::Dir {
                    source_path_hash,
                    children,
                    ..
                } if *source_path_hash == source.0 => {
                    return sum_children_bytes(children);
                }
                _ => {}
            }
        }
        0
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

    fn emit_status(&self, status: &Option<String>) {
        if let (Some(sink), Some(s)) = (self.sink.as_ref(), status.as_ref()) {
            sink.set_status(s);
        }
    }

    fn emit_log(&self, log: &Option<String>) {
        if let (Some(sink), Some(l)) = (self.sink.as_ref(), log.as_ref()) {
            sink.log(l);
        }
    }

    fn advance_bytes(&self, bytes: u64) {
        let mut state = self.progress.lock().unwrap();
        state.bytes_installed = state.bytes_installed.saturating_add(bytes);
        let fraction = if state.bytes_total == 0 {
            0.0
        } else {
            (state.bytes_installed as f64 / state.bytes_total as f64).clamp(0.0, 1.0)
        };
        drop(state);
        if let Some(sink) = self.sink.as_ref() {
            sink.set_progress(fraction);
        }
    }

    // ── Builders ─────────────────────────────────────────────────────────────

    /// Install a single embedded file. Pair with [`source!`]:
    ///
    /// ```rust,ignore
    /// i.file(installrs::source!("app.exe"), "app.exe")
    ///     .status("Installing app.exe")
    ///     .install()?;
    /// ```
    pub fn file<'i>(&'i mut self, source: Source, dst: impl Into<String>) -> FileOp<'i> {
        FileOp {
            installer: self,
            source,
            dst: dst.into(),
            status: None,
            log: None,
            overwrite: OverwriteMode::default(),
            mode: None,
        }
    }

    /// Install an embedded directory tree.
    pub fn dir<'i>(&'i mut self, source: Source, dst: impl Into<String>) -> DirOp<'i> {
        DirOp {
            installer: self,
            source,
            dst: dst.into(),
            status: None,
            log: None,
            overwrite: OverwriteMode::default(),
            mode: None,
            filter: None,
            on_error: None,
        }
    }

    /// Write the embedded uninstaller executable.
    pub fn uninstaller<'i>(&'i mut self, dst: impl Into<String>) -> UninstallerOp<'i> {
        UninstallerOp {
            installer: self,
            dst: dst.into(),
            status: None,
            log: None,
            overwrite: OverwriteMode::default(),
        }
    }

    /// Create a directory (and its parents) on the target system.
    pub fn mkdir<'i>(&'i mut self, dst: impl Into<String>) -> MkdirOp<'i> {
        MkdirOp {
            installer: self,
            dst: dst.into(),
            status: None,
            log: None,
        }
    }

    /// Remove a file or directory from the target system.
    pub fn remove<'i>(&'i mut self, path: impl Into<String>) -> RemoveOp<'i> {
        RemoveOp {
            installer: self,
            path: path.into(),
            status: None,
            log: None,
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

        // Run the temp copy from the temp dir so the install directory isn't
        // locked as the child's cwd (Windows refuses to delete a directory
        // that any process has open as its current directory).
        match std::process::Command::new(&tmp_exe)
            .args(&args)
            .current_dir(&tmp_dir)
            .spawn()
        {
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
                    use std::os::windows::process::CommandExt;
                    use std::process::Stdio;
                    // CREATE_NO_WINDOW: no console window flashes. Note this is
                    // mutually exclusive with DETACHED_PROCESS — using both
                    // causes CreateProcess to fail. Null stdio handles below
                    // are enough to let the parent exit cleanly.
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    let dir_str = dir.to_string_lossy().into_owned();
                    // PowerShell's cwd must NOT be the directory we're trying
                    // to delete — Windows refuses to remove a directory that
                    // is any process's current directory. The parent of our
                    // temp dir is the system temp dir, which is always safe.
                    let ps_cwd = std::env::temp_dir();
                    let _ = std::process::Command::new("powershell")
                        .args([
                            "-ExecutionPolicy",
                            "Bypass",
                            "-Command",
                            &format!(
                                "Start-Sleep 5; Remove-Item -Path '{}' -Recurse -Force",
                                dir_str
                            ),
                        ])
                        .current_dir(&ps_cwd)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .creation_flags(CREATE_NO_WINDOW)
                        .spawn();
                }
            }
        }
    }
}

// ── Builder types ───────────────────────────────────────────────────────────

pub struct FileOp<'i> {
    installer: &'i mut Installer,
    source: Source,
    dst: String,
    status: Option<String>,
    log: Option<String>,
    overwrite: OverwriteMode,
    mode: Option<u32>,
}

impl<'i> FileOp<'i> {
    pub fn status(mut self, s: impl Into<String>) -> Self {
        self.status = Some(s.into());
        self
    }
    pub fn log(mut self, s: impl Into<String>) -> Self {
        self.log = Some(s.into());
        self
    }
    pub fn overwrite(mut self, mode: OverwriteMode) -> Self {
        self.overwrite = mode;
        self
    }
    /// Unix file permissions (octal, e.g. `0o755`). No-op on Windows.
    pub fn mode(mut self, mode: u32) -> Self {
        self.mode = Some(mode);
        self
    }
    pub fn install(self) -> Result<()> {
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);

        let (raw_bytes, compression) = find_file(self.installer.entries, self.source.0)?;
        let dest = self.installer.resolve_out_path(&self.dst)?;

        match self.overwrite {
            OverwriteMode::Overwrite => {}
            OverwriteMode::Skip => {
                if dest.exists() {
                    self.installer.advance_bytes(raw_bytes.len() as u64);
                    return Ok(());
                }
            }
            OverwriteMode::Error => {
                if dest.exists() {
                    return Err(anyhow!("destination already exists: {}", dest.display()));
                }
            }
            OverwriteMode::Backup => {
                if dest.exists() {
                    backup_path(&dest)?;
                }
            }
        }

        let bytes = Installer::decompress(raw_bytes, compression)?;
        write_file(&dest, &bytes)?;
        apply_mode(&dest, self.mode)?;

        self.installer.advance_bytes(raw_bytes.len() as u64);
        Ok(())
    }
}

pub struct DirOp<'i> {
    installer: &'i mut Installer,
    source: Source,
    dst: String,
    status: Option<String>,
    log: Option<String>,
    overwrite: OverwriteMode,
    mode: Option<u32>,
    filter: Option<DirFilter>,
    on_error: Option<DirErrorHandler>,
}

impl<'i> DirOp<'i> {
    pub fn status(mut self, s: impl Into<String>) -> Self {
        self.status = Some(s.into());
        self
    }
    pub fn log(mut self, s: impl Into<String>) -> Self {
        self.log = Some(s.into());
        self
    }
    pub fn overwrite(mut self, mode: OverwriteMode) -> Self {
        self.overwrite = mode;
        self
    }
    /// Unix file permissions applied to each installed file. No-op on Windows.
    pub fn mode(mut self, mode: u32) -> Self {
        self.mode = Some(mode);
        self
    }
    /// Filter closure: receives relative path within the directory; return
    /// `true` to install the file.
    pub fn filter<F: Fn(&str) -> bool + 'static>(mut self, f: F) -> Self {
        self.filter = Some(Box::new(f));
        self
    }
    /// Per-file error handler: receives the relative path and error, returns
    /// [`ErrorAction::Skip`] to continue or [`ErrorAction::Abort`] to propagate.
    pub fn on_error<F: Fn(&str, &anyhow::Error) -> ErrorAction + 'static>(mut self, f: F) -> Self {
        self.on_error = Some(Box::new(f));
        self
    }
    pub fn install(self) -> Result<()> {
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);

        let children = find_dir(self.installer.entries, self.source.0)?;
        let dest = self.installer.resolve_out_path(&self.dst)?;
        std::fs::create_dir_all(&dest)
            .with_context(|| format!("failed to create directory: {}", dest.display()))?;

        install_children(
            children,
            &dest,
            "",
            self.installer,
            self.overwrite,
            self.mode,
            self.filter.as_deref(),
            self.on_error.as_deref(),
        )
    }
}

pub struct UninstallerOp<'i> {
    installer: &'i mut Installer,
    dst: String,
    status: Option<String>,
    log: Option<String>,
    overwrite: OverwriteMode,
}

impl<'i> UninstallerOp<'i> {
    pub fn status(mut self, s: impl Into<String>) -> Self {
        self.status = Some(s.into());
        self
    }
    pub fn log(mut self, s: impl Into<String>) -> Self {
        self.log = Some(s.into());
        self
    }
    pub fn overwrite(mut self, mode: OverwriteMode) -> Self {
        self.overwrite = mode;
        self
    }
    pub fn install(self) -> Result<()> {
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);

        let dest = self.installer.resolve_out_path(&self.dst)?;

        match self.overwrite {
            OverwriteMode::Overwrite => {}
            OverwriteMode::Skip => {
                if dest.exists() {
                    self.installer
                        .advance_bytes(self.installer.uninstaller_data.len() as u64);
                    return Ok(());
                }
            }
            OverwriteMode::Error => {
                if dest.exists() {
                    return Err(anyhow!("destination already exists: {}", dest.display()));
                }
            }
            OverwriteMode::Backup => {
                if dest.exists() {
                    backup_path(&dest)?;
                }
            }
        }

        let data = Installer::decompress(
            self.installer.uninstaller_data,
            self.installer.uninstaller_compression,
        )?;
        write_file(&dest, &data)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
                .with_context(|| format!("failed to set permissions on: {}", dest.display()))?;
        }
        self.installer
            .advance_bytes(self.installer.uninstaller_data.len() as u64);
        Ok(())
    }
}

pub struct MkdirOp<'i> {
    installer: &'i mut Installer,
    dst: String,
    status: Option<String>,
    log: Option<String>,
}

impl<'i> MkdirOp<'i> {
    pub fn status(mut self, s: impl Into<String>) -> Self {
        self.status = Some(s.into());
        self
    }
    pub fn log(mut self, s: impl Into<String>) -> Self {
        self.log = Some(s.into());
        self
    }
    pub fn install(self) -> Result<()> {
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);
        let path = self.installer.resolve_out_path(&self.dst)?;
        std::fs::create_dir_all(&path)
            .with_context(|| format!("failed to create directory: {}", path.display()))
    }
}

pub struct RemoveOp<'i> {
    installer: &'i mut Installer,
    path: String,
    status: Option<String>,
    log: Option<String>,
}

impl<'i> RemoveOp<'i> {
    pub fn status(mut self, s: impl Into<String>) -> Self {
        self.status = Some(s.into());
        self
    }
    pub fn log(mut self, s: impl Into<String>) -> Self {
        self.log = Some(s.into());
        self
    }
    pub fn install(self) -> Result<()> {
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);
        let p = self.installer.resolve_out_path(&self.path)?;
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
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn sum_embedded_bytes(entries: &[EmbeddedEntry]) -> u64 {
    let mut total = 0u64;
    for e in entries {
        match e {
            EmbeddedEntry::File { data, .. } => total += data.len() as u64,
            EmbeddedEntry::Dir { children, .. } => total += sum_children_bytes(children),
        }
    }
    total
}

fn sum_children_bytes(children: &[DirChild]) -> u64 {
    let mut total = 0u64;
    for c in children {
        match &c.kind {
            DirChildKind::File { data, .. } => total += data.len() as u64,
            DirChildKind::Dir { children } => total += sum_children_bytes(children),
        }
    }
    total
}

fn find_file(
    entries: &'static [EmbeddedEntry],
    hash: u64,
) -> Result<(&'static [u8], &'static str)> {
    for entry in entries {
        if let EmbeddedEntry::File {
            source_path_hash,
            data,
            compression,
        } = entry
        {
            if *source_path_hash == hash {
                return Ok((data, compression));
            }
        }
    }
    Err(anyhow!(
        "file not embedded in installer (hash: {hash:#018x})"
    ))
}

fn find_dir(entries: &'static [EmbeddedEntry], hash: u64) -> Result<&'static [DirChild]> {
    for entry in entries {
        if let EmbeddedEntry::Dir {
            source_path_hash,
            children,
        } = entry
        {
            if *source_path_hash == hash {
                return Ok(children);
            }
        }
    }
    Err(anyhow!(
        "directory not embedded in installer (hash: {hash:#018x})"
    ))
}

#[allow(clippy::too_many_arguments)]
fn install_children(
    children: &[DirChild],
    dest: &Path,
    rel_prefix: &str,
    installer: &Installer,
    overwrite: OverwriteMode,
    mode: Option<u32>,
    filter: Option<&(dyn Fn(&str) -> bool + 'static)>,
    on_error: Option<&(dyn Fn(&str, &anyhow::Error) -> ErrorAction + 'static)>,
) -> Result<()> {
    for child in children {
        let target = dest.join(child.name);
        let rel = if rel_prefix.is_empty() {
            child.name.to_string()
        } else {
            format!("{rel_prefix}/{}", child.name)
        };

        match &child.kind {
            DirChildKind::File { data, compression } => {
                if let Some(f) = filter {
                    if !f(&rel) {
                        continue;
                    }
                }
                let res = install_one_file(data, compression, &target, overwrite, mode, installer);
                if let Err(e) = res {
                    match on_error {
                        Some(h) => match h(&rel, &e) {
                            ErrorAction::Skip => continue,
                            ErrorAction::Abort => return Err(e),
                        },
                        None => return Err(e),
                    }
                }
            }
            DirChildKind::Dir { children } => {
                std::fs::create_dir_all(&target)
                    .with_context(|| format!("failed to create dir: {}", target.display()))?;
                install_children(
                    children, &target, &rel, installer, overwrite, mode, filter, on_error,
                )?;
            }
        }
    }
    Ok(())
}

fn install_one_file(
    data: &[u8],
    compression: &str,
    dest: &Path,
    overwrite: OverwriteMode,
    mode: Option<u32>,
    installer: &Installer,
) -> Result<()> {
    match overwrite {
        OverwriteMode::Overwrite => {}
        OverwriteMode::Skip => {
            if dest.exists() {
                installer.advance_bytes(data.len() as u64);
                return Ok(());
            }
        }
        OverwriteMode::Error => {
            if dest.exists() {
                return Err(anyhow!("destination already exists: {}", dest.display()));
            }
        }
        OverwriteMode::Backup => {
            if dest.exists() {
                backup_path(dest)?;
            }
        }
    }

    let bytes = Installer::decompress(data, compression)?;
    write_file(dest, &bytes)?;
    apply_mode(dest, mode)?;
    installer.advance_bytes(data.len() as u64);
    Ok(())
}

fn backup_path(path: &Path) -> Result<()> {
    let backup = path.with_extension(match path.extension() {
        Some(ext) => format!("{}.bak", ext.to_string_lossy()),
        None => "bak".to_string(),
    });
    if backup.exists() {
        if backup.is_dir() {
            std::fs::remove_dir_all(&backup)
                .with_context(|| format!("failed to remove old backup: {}", backup.display()))?;
        } else {
            std::fs::remove_file(&backup)
                .with_context(|| format!("failed to remove old backup: {}", backup.display()))?;
        }
    }
    std::fs::rename(path, &backup)
        .with_context(|| format!("failed to back up: {}", path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn apply_mode(path: &Path, mode: Option<u32>) -> Result<()> {
    if let Some(m) = mode {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(m))
            .with_context(|| format!("failed to set permissions on: {}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn apply_mode(_path: &Path, _mode: Option<u32>) -> Result<()> {
    Ok(())
}

fn write_file(dest: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir for: {}", dest.display()))?;
    }
    std::fs::write(dest, data).with_context(|| format!("failed to write: {}", dest.display()))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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

    fn src(path: &str) -> Source {
        let norm = path.replace('\\', "/");
        Source(source_path_hash_const(&norm))
    }

    // ── source_path_hash_const ───────────────────────────────────────────────

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

    // ── file() builder ───────────────────────────────────────────────────────

    #[test]
    fn file_writes_content() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut i = make_installer(vec![file_entry("vendor/lib.so", b"ELF")], tmp.path());
        i.file(src("vendor/lib.so"), "lib.so").install().unwrap();
        assert_eq!(std::fs::read(tmp.path().join("lib.so")).unwrap(), b"ELF");
    }

    #[test]
    fn file_creates_parent_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut i = make_installer(vec![file_entry("data.txt", b"x")], tmp.path());
        i.file(src("data.txt"), "a/b/out.txt").install().unwrap();
        assert!(tmp.path().join("a/b/out.txt").exists());
    }

    #[test]
    fn file_errors_when_not_embedded() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut i = make_installer(vec![], tmp.path());
        assert!(i.file(src("missing.txt"), "out.txt").install().is_err());
    }

    #[test]
    fn file_decompresses_on_write() {
        let tmp = tempfile::TempDir::new().unwrap();
        let original = b"compressed content";
        let compressed = compress_gzip(original);
        let data: &'static [u8] = leak_bytes(compressed);
        let entry = EmbeddedEntry::File {
            source_path_hash: source_path_hash_const("comp.gz"),
            data,
            compression: "gzip",
        };
        let mut i = make_installer(vec![entry], tmp.path());
        i.file(src("comp.gz"), "out.txt").install().unwrap();
        assert_eq!(std::fs::read(tmp.path().join("out.txt")).unwrap(), original);
    }

    #[test]
    fn file_overwrite_skip_leaves_existing() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("out.txt"), b"OLD").unwrap();
        let mut i = make_installer(vec![file_entry("x.txt", b"NEW")], tmp.path());
        i.file(src("x.txt"), "out.txt")
            .overwrite(OverwriteMode::Skip)
            .install()
            .unwrap();
        assert_eq!(std::fs::read(tmp.path().join("out.txt")).unwrap(), b"OLD");
    }

    #[test]
    fn file_overwrite_error_fails_if_exists() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("out.txt"), b"OLD").unwrap();
        let mut i = make_installer(vec![file_entry("x.txt", b"NEW")], tmp.path());
        let r = i
            .file(src("x.txt"), "out.txt")
            .overwrite(OverwriteMode::Error)
            .install();
        assert!(r.is_err());
    }

    #[test]
    fn file_overwrite_backup_renames_existing() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("out.txt"), b"OLD").unwrap();
        let mut i = make_installer(vec![file_entry("x.txt", b"NEW")], tmp.path());
        i.file(src("x.txt"), "out.txt")
            .overwrite(OverwriteMode::Backup)
            .install()
            .unwrap();
        assert_eq!(std::fs::read(tmp.path().join("out.txt")).unwrap(), b"NEW");
        assert_eq!(
            std::fs::read(tmp.path().join("out.txt.bak")).unwrap(),
            b"OLD"
        );
    }

    // ── dir() builder ────────────────────────────────────────────────────────

    #[test]
    fn dir_installs_tree() {
        let tmp = tempfile::TempDir::new().unwrap();
        let entries = vec![dir_entry(
            "pkg",
            vec![
                child_file("readme.txt", b"readme"),
                child_dir("bin", vec![child_file("app", b"binary")]),
            ],
        )];
        let mut i = make_installer(entries, tmp.path());
        i.dir(src("pkg"), "out").install().unwrap();
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
    fn dir_filter_excludes_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let entries = vec![dir_entry(
            "pkg",
            vec![child_file("keep.txt", b"K"), child_file("drop.txt", b"D")],
        )];
        let mut i = make_installer(entries, tmp.path());
        i.dir(src("pkg"), "out")
            .filter(|rel| !rel.starts_with("drop"))
            .install()
            .unwrap();
        assert!(tmp.path().join("out/keep.txt").exists());
        assert!(!tmp.path().join("out/drop.txt").exists());
    }

    #[test]
    fn dir_on_error_skip_continues() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Second file uses bogus compression to force an error.
        let bad: &'static [u8] = b"garbage";
        let children = vec![
            child_file("good.txt", b"OK"),
            DirChild {
                name: Box::leak("bad.bin".to_string().into_boxed_str()),
                kind: DirChildKind::File {
                    data: bad,
                    compression: "zstd", // unsupported → decompress error
                },
            },
            child_file("tail.txt", b"T"),
        ];
        let entries = vec![dir_entry("pkg", children)];
        let mut i = make_installer(entries, tmp.path());
        let skipped = std::sync::atomic::AtomicBool::new(false);
        let skipped_ref: &'static std::sync::atomic::AtomicBool = Box::leak(Box::new(skipped));
        i.dir(src("pkg"), "out")
            .on_error(|_rel, _err| {
                skipped_ref.store(true, std::sync::atomic::Ordering::Relaxed);
                ErrorAction::Skip
            })
            .install()
            .unwrap();
        assert!(skipped_ref.load(std::sync::atomic::Ordering::Relaxed));
        assert!(tmp.path().join("out/good.txt").exists());
        assert!(tmp.path().join("out/tail.txt").exists());
        assert!(!tmp.path().join("out/bad.bin").exists());
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

    // ── mkdir() ──────────────────────────────────────────────────────────────

    #[test]
    fn mkdir_creates_nested_directories() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut i = make_installer(vec![], tmp.path());
        i.mkdir("a/b/c/d").install().unwrap();
        assert!(tmp.path().join("a/b/c/d").is_dir());
    }

    #[test]
    fn mkdir_is_idempotent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut i = make_installer(vec![], tmp.path());
        i.mkdir("exists").install().unwrap();
        i.mkdir("exists").install().unwrap();
    }

    #[test]
    fn mkdir_requires_out_dir() {
        let mut i = Installer::new(leak_entries(vec![]), leak_bytes(vec![]), "none");
        assert!(i.mkdir("foo").install().is_err());
    }

    // ── remove() ─────────────────────────────────────────────────────────────

    #[test]
    fn remove_deletes_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("victim.txt"), b"x").unwrap();
        let mut i = make_installer(vec![], tmp.path());
        i.remove("victim.txt").install().unwrap();
        assert!(!tmp.path().join("victim.txt").exists());
    }

    #[test]
    fn remove_deletes_directory_recursively() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("tree/leaf")).unwrap();
        std::fs::write(tmp.path().join("tree/leaf/f.txt"), b"x").unwrap();
        let mut i = make_installer(vec![], tmp.path());
        i.remove("tree").install().unwrap();
        assert!(!tmp.path().join("tree").exists());
    }

    #[test]
    fn remove_noop_when_nonexistent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut i = make_installer(vec![], tmp.path());
        i.remove("does_not_exist.txt").install().unwrap();
    }

    // ── exists() ─────────────────────────────────────────────────────────────

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

    // ── progress ─────────────────────────────────────────────────────────────

    struct TestSink {
        statuses: Mutex<Vec<String>>,
        progresses: Mutex<Vec<f64>>,
        logs: Mutex<Vec<String>>,
    }

    impl ProgressSink for TestSink {
        fn set_status(&self, s: &str) {
            self.statuses.lock().unwrap().push(s.to_string());
        }
        fn set_progress(&self, f: f64) {
            self.progresses.lock().unwrap().push(f);
        }
        fn log(&self, m: &str) {
            self.logs.lock().unwrap().push(m.to_string());
        }
    }

    #[test]
    fn file_install_reports_progress_and_status() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut i = make_installer(vec![file_entry("a.txt", b"HELLO")], tmp.path());
        let sink = std::sync::Arc::new(TestSink {
            statuses: Mutex::new(Vec::new()),
            progresses: Mutex::new(Vec::new()),
            logs: Mutex::new(Vec::new()),
        });
        struct Forward(std::sync::Arc<TestSink>);
        impl ProgressSink for Forward {
            fn set_status(&self, s: &str) {
                self.0.set_status(s)
            }
            fn set_progress(&self, f: f64) {
                self.0.set_progress(f)
            }
            fn log(&self, m: &str) {
                self.0.log(m)
            }
        }
        i.set_progress_sink(Box::new(Forward(sink.clone())));
        i.set_total_bytes(5);
        i.file(src("a.txt"), "out.txt")
            .status("installing")
            .log("copying a.txt")
            .install()
            .unwrap();

        assert_eq!(sink.statuses.lock().unwrap().as_slice(), &["installing"]);
        assert_eq!(sink.logs.lock().unwrap().as_slice(), &["copying a.txt"]);
        let progs = sink.progresses.lock().unwrap();
        assert_eq!(progs.len(), 1);
        assert!((progs[0] - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn bytes_of_sums_file_and_dir() {
        let entries = vec![
            file_entry("a.txt", b"ABC"),
            dir_entry("d", vec![child_file("x", b"12"), child_file("y", b"34567")]),
        ];
        let i = Installer::new(leak_entries(entries), leak_bytes(vec![]), "");
        assert_eq!(i.bytes_of(&[src("a.txt")]), 3);
        assert_eq!(i.bytes_of(&[src("d")]), 7);
        assert_eq!(i.bytes_of(&[src("a.txt"), src("d")]), 10);
    }
}
