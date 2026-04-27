#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "gui")]
#[cfg_attr(docsrs, doc(cfg(feature = "gui")))]
pub mod gui;

mod cmdline;
mod component;
mod embedded;
mod ops;
mod options;
mod progress;
#[cfg(target_os = "windows")]
mod registry;
#[cfg(target_os = "windows")]
mod shortcut;
mod source;
mod types;

pub use component::Component;
pub use embedded::{verify_payload, DirChild, DirChildKind, EmbeddedEntry};
pub use ops::{DirOp, FileOp, MkdirOp, RemoveOp, UninstallerOp};
pub use options::{FromOptionValue, OptionKind, OptionValue};
pub use progress::ProgressSink;
#[cfg(target_os = "windows")]
pub use registry::{RegDeleteValueOp, RegRemoveKeyOp, RegSetOp, Registry, RegistryHive};
#[cfg(target_os = "windows")]
pub use shortcut::ShortcutOp;
pub use source::{source_path_hash_const, Source};
pub use types::{DirErrorHandler, DirFilter, ErrorAction, OverwriteMode};

use options::CmdOption;
use progress::ProgressState;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};

pub struct Installer {
    pub headless: bool,
    pub(crate) entries: &'static [EmbeddedEntry],
    out_dir: Option<PathBuf>,
    pub(crate) uninstaller_data: &'static [u8],
    pub(crate) uninstaller_compression: &'static str,
    #[cfg(target_os = "windows")]
    self_delete: bool,
    sink: Option<Box<dyn ProgressSink>>,
    progress: Mutex<ProgressState>,
    components: Vec<Component>,
    cancelled: Arc<AtomicBool>,
    options: Vec<CmdOption>,
    option_values: std::collections::HashMap<String, OptionValue>,
    log_file: Option<Mutex<std::fs::File>>,
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
            sink: None,
            progress: Mutex::new(ProgressState {
                steps_done: 0.0,
                step_range_start: 0.0,
                step_range_end: 0.0,
            }),
            components: Vec::new(),
            cancelled: Arc::new(AtomicBool::new(false)),
            options: Vec::new(),
            option_values: std::collections::HashMap::new(),
            log_file: None,
        }
    }

    /// Open a log file. Every subsequent `status` / `log` / error surfaced
    /// by the installer (via [`ProgressSink`] or the wizard) is also
    /// appended to this file, in the same format used by `--headless`
    /// stderr output (`[*] <status>`, `    <log>`, `[ERROR] <msg>`).
    ///
    /// The file is opened with `create + append`, so repeated runs build a
    /// chronological history. Call [`Installer::clear_log_file`] to stop
    /// logging. Errors only on file-open failure; subsequent write errors
    /// are silently swallowed so a broken log pipe can't derail an install.
    pub fn set_log_file(&mut self, path: impl AsRef<std::path::Path>) -> Result<()> {
        use std::fs::OpenOptions;
        let path = path.as_ref();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("failed to open log file {}", path.display()))?;
        self.log_file = Some(Mutex::new(file));
        // Write a session-start marker so multiple runs are easy to tell apart.
        self.write_log_line(&format!(
            "--- install session started (pid {}) ---",
            std::process::id()
        ));
        Ok(())
    }

    /// Stop writing to the log file (if one was opened).
    pub fn clear_log_file(&mut self) {
        self.log_file = None;
    }

    /// Record an error to the log file (no-op if no file is set). Called
    /// automatically by the wizard and headless runner on install failure.
    pub fn log_error(&self, err: &anyhow::Error) {
        self.write_log_line(&format!("[ERROR] {err:#}"));
    }

    fn write_log_line(&self, line: &str) {
        if let Some(f) = &self.log_file {
            if let Ok(mut f) = f.lock() {
                use std::io::Write;
                let _ = writeln!(f, "{line}");
            }
        }
    }

    /// Register a user-defined command-line option. Register all options
    /// before calling [`Installer::process_commandline`]; afterwards read
    /// values via [`Installer::get_option`] or [`Installer::option_value`].
    ///
    /// Option names must not collide with the built-ins (`headless`,
    /// `list-components`, `components`, `with`, `without`). The leading
    /// `--` is implied — pass just `"config"` for `--config`.
    ///
    /// ```rust,ignore
    /// i.option("config", OptionKind::String);
    /// i.option("port", OptionKind::Int);
    /// i.option("verbose", OptionKind::Flag);
    /// i.process_commandline()?;
    /// let config: Option<String> = i.get_option("config");
    /// let port: i64 = i.get_option("port").unwrap_or(8080);
    /// let verbose: bool = i.get_option("verbose").unwrap_or(false);
    /// ```
    pub fn option(&mut self, name: &str, kind: OptionKind) -> &mut Self {
        let name = name.trim_start_matches('-').to_string();
        if let Some(existing) = self.options.iter_mut().find(|o| o.name == name) {
            existing.kind = kind;
        } else {
            self.options.push(CmdOption { name, kind });
        }
        self
    }

    /// Typed accessor for a parsed option value.
    ///
    /// Returns `None` if the option was never registered, not provided on
    /// the command line, or the stored value doesn't convert to `T`.
    ///
    /// For `OptionKind::Flag`, `.get_option::<bool>(name)` returns
    /// `Some(false)` when the flag is absent (flags are always populated).
    pub fn get_option<T: FromOptionValue>(&self, name: &str) -> Option<T> {
        let name = name.trim_start_matches('-');
        self.option_values
            .get(name)
            .and_then(|v| T::from_option_value(v))
    }

    /// Raw parsed value for an option, or `None` if not set.
    pub fn option_value(&self, name: &str) -> Option<&OptionValue> {
        let name = name.trim_start_matches('-');
        self.option_values.get(name)
    }

    /// Store (or overwrite) a parsed option value directly. The wizard
    /// calls this when a custom-page widget commits its current value on
    /// forward navigation. User code can call it too, e.g. to seed a
    /// value from a config file before the wizard opens.
    pub fn set_option_value(&mut self, name: &str, value: OptionValue) {
        let name = name.trim_start_matches('-').to_string();
        self.option_values.insert(name, value);
    }

    /// Clone of the full parsed-options map. Used by the wizard to
    /// pre-fill custom-page widgets from already-set option values
    /// (whether from the CLI or a previous wizard run).
    pub fn option_values_snapshot(&self) -> std::collections::HashMap<String, OptionValue> {
        self.option_values.clone()
    }

    /// The shared cancellation flag. Flipping this to `true` causes the
    /// next file/dir/mkdir/remove op to error with "install cancelled".
    /// The wizard's Cancel button and the headless Ctrl+C handler both
    /// write through this.
    pub fn cancellation_flag(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }

    /// True if a cancel has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Request cancellation. Subsequent op `.install()` calls will return
    /// a "cancelled" error before doing any work.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Error out if cancellation has been requested. Called at the top of
    /// every builder op's `.install()`; user code calling the op-level
    /// helpers can rely on this without any polling.
    pub fn check_cancelled(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(anyhow!("install cancelled by user"))
        } else {
            Ok(())
        }
    }

    /// Install a Ctrl+C / SIGINT handler tied to this installer's
    /// cancellation flag. First press sets the flag (the next op errors
    /// with "cancelled"); a second press exits the process with status 130.
    ///
    /// Called from the generated installer/uninstaller `main()` before
    /// [`install_main`](Self::install_main) / [`uninstall_main`](Self::uninstall_main).
    /// Idempotent: re-invocations are no-ops.
    pub fn install_ctrlc_handler(&self) {
        static INSTALLED: std::sync::Once = std::sync::Once::new();
        let flag = self.cancelled.clone();
        INSTALLED.call_once(|| {
            let counter = Arc::new(AtomicU32::new(0));
            let flag_h = flag.clone();
            let _ = ctrlc::set_handler(move || {
                let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
                if n == 1 {
                    flag_h.store(true, Ordering::Relaxed);
                    eprintln!("\nCancellation requested. Press Ctrl+C again to exit immediately.");
                } else {
                    std::process::exit(130);
                }
            });
        });
    }

    /// Register (or update) an optional component.
    ///
    /// `progress_weight` is the number of step units the component contributes
    /// to the installer's progress total when selected. Operations inside the
    /// component's install code each advance the cursor by their own weight
    /// (default 1) — overshoot/undershoot happens if the actual op count
    /// diverges from `progress_weight`.
    ///
    /// ```rust,ignore
    /// i.component("docs", "Documentation", "User-facing docs", 3);
    /// i.component("extras", "Extras", "Optional samples", 1).default_off();
    /// i.component("core", "Core files", "Always installed", 10).required();
    /// ```
    ///
    /// Components start selected (`default = true`); call `.default_off()`
    /// on ones the user has to opt into. Call this before running the
    /// wizard or parsing CLI args. Later calls with the same `id` update
    /// the existing component in place.
    pub fn component(
        &mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        description: impl Into<String>,
        progress_weight: u32,
    ) -> &mut Component {
        let id = id.into();
        let label = label.into();
        let description = description.into();
        if let Some(pos) = self.components.iter().position(|c| c.id == id) {
            let existing = &mut self.components[pos];
            existing.label = label;
            existing.description = description;
            existing.progress_weight = progress_weight;
            existing.default = true;
            existing.selected = true;
            return existing;
        }
        self.components.push(Component {
            id,
            label,
            description,
            progress_weight,
            default: true,
            required: false,
            selected: true,
        });
        self.components.last_mut().unwrap()
    }

    /// All registered components, in registration order.
    pub fn components(&self) -> &[Component] {
        &self.components
    }

    /// Whether a component is currently selected. Unknown ids return `false`.
    pub fn is_component_selected(&self, id: &str) -> bool {
        self.components
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.selected)
            .unwrap_or(false)
    }

    /// Set the selected state of a component. Required components ignore
    /// attempts to deselect them. Unknown ids are silently ignored.
    pub fn set_component_selected(&mut self, id: &str, on: bool) {
        if let Some(c) = self.components.iter_mut().find(|c| c.id == id) {
            if c.required && !on {
                return;
            }
            c.selected = on;
        }
    }

    /// Set the base output directory for relative destination paths.
    pub fn set_out_dir(&mut self, dir: impl AsRef<str>) {
        self.out_dir = Some(PathBuf::from(dir.as_ref()));
    }

    /// Attach a [`ProgressSink`] that receives status, progress, and log events.
    pub fn set_progress_sink(&mut self, sink: Box<dyn ProgressSink>) {
        self.sink = Some(sink);
    }

    /// Remove the progress sink (if any).
    pub fn clear_progress_sink(&mut self) {
        self.sink = None;
    }

    pub(crate) fn resolve_out_path(&self, dest_path: &str) -> Result<PathBuf> {
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

    pub(crate) fn decompress(data: &[u8], compression: &str) -> Result<Vec<u8>> {
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

    pub(crate) fn emit_status(&self, status: &Option<String>) {
        if let Some(s) = status.as_ref() {
            if let Some(sink) = self.sink.as_ref() {
                sink.set_status(s);
            }
            self.write_log_line(&format!("[*] {s}"));
        }
    }

    pub(crate) fn emit_log(&self, log: &Option<String>) {
        if let Some(l) = log.as_ref() {
            if let Some(sink) = self.sink.as_ref() {
                sink.log(l);
            }
            self.write_log_line(&format!("    {l}"));
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
            weight: 1,
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
            weight: 1,
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
            weight: 1,
        }
    }

    /// Create a directory (and its parents) on the target system.
    pub fn mkdir<'i>(&'i mut self, dst: impl Into<String>) -> MkdirOp<'i> {
        MkdirOp {
            installer: self,
            dst: dst.into(),
            weight: 1,
            status: None,
            log: None,
        }
    }

    /// Remove a file or directory from the target system.
    pub fn remove<'i>(&'i mut self, path: impl Into<String>) -> RemoveOp<'i> {
        RemoveOp {
            installer: self,
            path: path.into(),
            weight: 1,
            status: None,
            log: None,
        }
    }

    /// Create a Windows `.lnk` shortcut. `dst` is the shortcut file path
    /// (relative paths resolve against `out_dir`, same as `file` / `dir`).
    /// `target` is what the shortcut points to (also resolved against
    /// `out_dir` when relative).
    ///
    /// Windows-only. On other targets this method does not exist — gate
    /// calls with `#[cfg(target_os = "windows")]` if your installer is
    /// cross-platform.
    #[cfg(target_os = "windows")]
    pub fn shortcut<'i>(
        &'i mut self,
        dst: impl Into<String>,
        target: impl Into<String>,
    ) -> ShortcutOp<'i> {
        ShortcutOp {
            installer: self,
            dst: dst.into(),
            target: target.into(),
            arguments: None,
            working_dir: None,
            description: None,
            icon: None,
            weight: 1,
            status: None,
            log: None,
        }
    }

    /// Windows registry operations. Returns a short-lived handle whose
    /// methods create registry builder ops (`set`, `default`, `get`,
    /// `remove`, `delete`). Windows-only.
    #[cfg(target_os = "windows")]
    pub fn registry(&mut self) -> Registry<'_> {
        Registry { installer: self }
    }

    /// Check whether a path exists on the target system.
    pub fn exists(&self, path: &str) -> Result<bool> {
        let p = self.resolve_out_path(path)?;
        Ok(p.exists())
    }

    /// Enable self-deletion of the executable after it finishes.
    ///
    /// On Windows this copies the running executable to a temporary directory and
    /// relaunches from there (with `--self-delete`) so the original file can
    /// be removed during uninstallation. After `uninstall_main` returns, a
    /// PowerShell process cleans up the temp copy.
    #[cfg(target_os = "windows")]
    pub fn enable_self_delete(&mut self) {
        if std::env::args().nth(1).as_deref() == Some("--self-delete") {
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

        // Pass all original args through, plus `--self-delete` to trigger the
        // self-deletion logic in the relaunched copy
        let mut args: Vec<String> = std::env::args().skip(1).collect();
        args.insert(0, "--self-delete".to_string());

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
    ///
    /// The user's `install_fn` is expected to call
    /// [`Installer::process_commandline`] itself (typically right after
    /// registering components).
    pub fn install_main(&mut self, install_fn: impl Fn(&mut Installer) -> Result<()>) {
        if let Err(e) = install_fn(self) {
            eprintln!("Error: {e:#}");
            std::process::exit(1);
        }
    }

    /// Entry point for uninstaller binaries. Call this from `main()`.
    pub fn uninstall_main(&mut self, uninstall_fn: impl Fn(&mut Installer) -> Result<()>) {
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
        i.set_out_dir(out_dir.to_string_lossy());
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
        i.component("core", "Core", "", 1).required();
        i.file(src("a.txt"), "out.txt")
            .status("installing")
            .log("copying a.txt")
            .install()
            .unwrap();

        assert_eq!(sink.statuses.lock().unwrap().as_slice(), &["installing"]);
        assert_eq!(sink.logs.lock().unwrap().as_slice(), &["copying a.txt"]);
        let progs = sink.progresses.lock().unwrap();
        // begin_step emits once at 0.0, end_step emits once at 1.0
        assert!(progs.len() >= 2);
        assert!((progs.last().unwrap() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn total_steps_sums_selected_components() {
        let mut i = make_bare_installer();
        i.component("core", "Core", "", 5).required();
        i.component("docs", "Docs", "", 3);
        i.component("extras", "Extras", "", 2).default_off();
        assert_eq!(i.total_steps(), 8);
        i.set_component_selected("extras", true);
        assert_eq!(i.total_steps(), 10);
    }

    fn make_bare_installer() -> Installer {
        Installer::new(leak_entries(vec![]), leak_bytes(vec![]), "")
    }

    #[test]
    fn component_register_and_query() {
        let mut i = make_bare_installer();
        i.component("core", "Core", "", 1).required();
        i.component("docs", "Docs", "", 1).default_off();
        i.component("extras", "Extras", "", 1);

        assert_eq!(i.components().len(), 3);
        assert!(i.is_component_selected("core"));
        assert!(!i.is_component_selected("docs"));
        assert!(i.is_component_selected("extras"));
        assert!(!i.is_component_selected("nope"));
    }

    #[test]
    fn component_required_cannot_be_deselected() {
        let mut i = make_bare_installer();
        i.component("core", "Core", "", 1).required();
        i.set_component_selected("core", false);
        assert!(i.is_component_selected("core"));
    }

    #[test]
    fn component_reregistration_updates_in_place() {
        let mut i = make_bare_installer();
        i.component("docs", "v1", "", 1);
        i.component("docs", "v2", "", 1).default_off();
        assert_eq!(i.components().len(), 1);
        assert_eq!(i.components()[0].label, "v2");
        assert!(!i.is_component_selected("docs"));
    }

    #[test]
    fn cli_exact_components_selects_only_listed() {
        let mut i = make_bare_installer();
        i.component("a", "A", "", 1);
        i.component("b", "B", "", 1);
        i.component("c", "C", "", 1);
        let args = vec!["installer".into(), "--components".into(), "a,c".into()];
        i.process_commandline_from(&args).unwrap();
        assert!(i.is_component_selected("a"));
        assert!(!i.is_component_selected("b"));
        assert!(i.is_component_selected("c"));
    }

    #[test]
    fn cli_exact_components_keeps_required() {
        let mut i = make_bare_installer();
        i.component("core", "Core", "", 1).required();
        i.component("docs", "Docs", "", 1);
        let args = vec!["installer".into(), "--components=docs".into()];
        i.process_commandline_from(&args).unwrap();
        assert!(i.is_component_selected("core"));
        assert!(i.is_component_selected("docs"));
    }

    #[test]
    fn cli_with_and_without_delta() {
        let mut i = make_bare_installer();
        i.component("a", "A", "", 1).default_off();
        i.component("b", "B", "", 1);
        let args = vec![
            "installer".into(),
            "--with".into(),
            "a".into(),
            "--without".into(),
            "b".into(),
        ];
        i.process_commandline_from(&args).unwrap();
        assert!(i.is_component_selected("a"));
        assert!(!i.is_component_selected("b"));
    }

    #[test]
    fn cli_unknown_component_errors() {
        let mut i = make_bare_installer();
        i.component("a", "A", "", 1);
        let args = vec!["installer".into(), "--with=bogus".into()];
        assert!(i.process_commandline_from(&args).is_err());
    }

    #[test]
    fn cli_without_cannot_disable_required() {
        let mut i = make_bare_installer();
        i.component("core", "Core", "", 1).required();
        let args = vec!["installer".into(), "--without=core".into()];
        i.process_commandline_from(&args).unwrap();
        assert!(i.is_component_selected("core"));
    }

    #[test]
    fn cli_headless_flag_sets_field() {
        let mut i = make_bare_installer();
        let args = vec!["installer".into(), "--headless".into()];
        i.process_commandline_from(&args).unwrap();
        assert!(i.headless);
    }

    #[test]
    fn cli_user_options_parse_and_typed_read() {
        let mut i = make_bare_installer();
        i.option("config", OptionKind::String);
        i.option("port", OptionKind::Int);
        i.option("verbose", OptionKind::Flag);
        i.option("fast", OptionKind::Bool);
        let args = vec![
            "installer".into(),
            "--config".into(),
            "/etc/my.conf".into(),
            "--port=8080".into(),
            "--verbose".into(),
            "--fast=yes".into(),
        ];
        i.process_commandline_from(&args).unwrap();
        assert_eq!(
            i.get_option::<String>("config").as_deref(),
            Some("/etc/my.conf")
        );
        assert_eq!(i.get_option::<i64>("port"), Some(8080));
        assert_eq!(i.get_option::<i32>("port"), Some(8080));
        assert_eq!(i.get_option::<bool>("verbose"), Some(true));
        assert_eq!(i.get_option::<bool>("fast"), Some(true));
    }

    #[test]
    fn cli_flag_absent_is_false() {
        let mut i = make_bare_installer();
        i.option("verbose", OptionKind::Flag);
        i.process_commandline_from(&["installer".into()]).unwrap();
        assert_eq!(i.get_option::<bool>("verbose"), Some(false));
    }

    #[test]
    fn cli_unknown_flag_errors() {
        let mut i = make_bare_installer();
        let args = vec!["installer".into(), "--nope".into()];
        assert!(i.process_commandline_from(&args).is_err());
    }

    #[test]
    fn cli_int_option_rejects_non_integer() {
        let mut i = make_bare_installer();
        i.option("port", OptionKind::Int);
        let args = vec!["installer".into(), "--port=abc".into()];
        assert!(i.process_commandline_from(&args).is_err());
    }

    #[test]
    fn cli_flag_with_value_errors() {
        let mut i = make_bare_installer();
        i.option("verbose", OptionKind::Flag);
        let args = vec!["installer".into(), "--verbose=true".into()];
        assert!(i.process_commandline_from(&args).is_err());
    }

    #[test]
    fn log_file_captures_status_log_and_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let log_path = tmp.path().join("install.log");
        let mut i = make_bare_installer();
        let args = vec![
            "installer".into(),
            "--log".into(),
            log_path.to_str().unwrap().into(),
        ];
        i.process_commandline_from(&args).unwrap();

        i.emit_status(&Some("Installing foo".into()));
        i.emit_log(&Some("wrote foo.exe".into()));
        i.log_error(&anyhow!("disk full"));
        i.clear_log_file();

        let contents = std::fs::read_to_string(&log_path).unwrap();
        assert!(contents.contains("install session started"));
        assert!(contents.contains("[*] Installing foo"));
        assert!(contents.contains("    wrote foo.exe"));
        assert!(contents.contains("[ERROR] disk full"));
    }
}
