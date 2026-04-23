//! Small shared types: overwrite policy, dir-install error action, and the
//! closure type aliases used by directory installs.

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
