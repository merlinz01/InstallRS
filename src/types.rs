//! Small shared types: overwrite policy, dir-install error action, the
//! closure type aliases used by directory installs, and the
//! cancellation token shared between the installer, the wizard's
//! Cancel button, and the Ctrl+C handler.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Shared cancellation flag. Cloning yields another handle to the
/// same flag — flipping any clone trips every `check_cancelled()`
/// call across the wizard, the install thread, and the Ctrl+C
/// handler.
///
/// Obtain one from [`Installer::cancellation_token`](crate::Installer::cancellation_token).
#[derive(Clone, Debug)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Request cancellation. Subsequent
    /// [`check_cancelled`](crate::Installer::check_cancelled) calls
    /// return an error before doing any work.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
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

// Borrowed forms of the filter / error-handler closures, used internally by
// `install_children`. Aliased so the function signature stays readable and
// clippy's `type_complexity` lint stays quiet.
pub(crate) type DirFilterRef = dyn Fn(&str) -> bool + 'static;
pub(crate) type DirErrorHandlerRef = dyn Fn(&str, &anyhow::Error) -> ErrorAction + 'static;
