//! Cross-platform builder operations (file / dir / uninstaller / mkdir /
//! remove) plus the shared helpers that implement their file-system work.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::embedded::{DirChild, DirChildKind, EmbeddedEntry};
use crate::types::{
    DirErrorHandler, DirErrorHandlerRef, DirFilter, DirFilterRef, ErrorAction, OverwriteMode,
};
use crate::{Installer, Source};

/// Implement the shared `status()`, `log()`, and `weight()` setters on a
/// builder op. The op struct must have `status: Option<String>`,
/// `log: Option<String>`, and `weight: u32` fields.
macro_rules! impl_common_op_setters {
    ($ty:ident) => {
        impl<'i> $ty<'i> {
            /// Status message emitted via the progress sink (or shown in
            /// the wizard's status label) when this op runs.
            pub fn status(mut self, s: impl AsRef<str>) -> Self {
                self.status = Some(s.as_ref().to_string());
                self
            }
            /// Log line appended to the wizard's log textbox / stderr /
            /// log file when this op runs.
            pub fn log(mut self, s: impl AsRef<str>) -> Self {
                self.log = Some(s.as_ref().to_string());
                self
            }
            /// Step weight this op consumes from the component budget. Default 1.
            pub fn weight(mut self, w: u32) -> Self {
                self.weight = w;
                self
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use impl_common_op_setters;

// ── Builder ops ─────────────────────────────────────────────────────────────

/// Builder for installing a single embedded file. Created by
/// [`Installer::file`](crate::Installer::file); finalize with
/// [`install`](Self::install). Chain
/// [`status`](Self::status) / [`log`](Self::log) /
/// [`weight`](Self::weight) / [`overwrite`](Self::overwrite) /
/// [`mode`](Self::mode) before installing.
#[must_use = "builder ops do nothing until `.install()` is called"]
pub struct FileOp<'i> {
    pub(crate) installer: &'i mut Installer,
    pub(crate) source: Source,
    pub(crate) dst: PathBuf,
    pub(crate) status: Option<String>,
    pub(crate) log: Option<String>,
    pub(crate) overwrite: OverwriteMode,
    pub(crate) mode: Option<u32>,
    pub(crate) weight: u32,
}

impl_common_op_setters!(FileOp);

impl<'i> FileOp<'i> {
    /// How to react when the destination already exists. Defaults to
    /// [`OverwriteMode::Overwrite`].
    pub fn overwrite(mut self, mode: OverwriteMode) -> Self {
        self.overwrite = mode;
        self
    }
    /// Unix file permissions (octal, e.g. `0o755`). No-op on Windows.
    pub fn mode(mut self, mode: u32) -> Self {
        self.mode = Some(mode);
        self
    }
    /// Run the op: decompress the embedded blob, write it to the
    /// destination (resolved relative to the installer's out-dir),
    /// and apply the configured mode / overwrite policy.
    pub fn install(self) -> Result<()> {
        self.installer.check_cancelled()?;
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);

        let (raw_bytes, compression) = find_file(self.installer.entries, self.source.0)?;
        let dest = self.installer.resolve_out_path(&self.dst)?;
        let overwrite = self.overwrite;
        let mode = self.mode;
        let weight = self.weight;

        self.installer.run_weighted_step(weight, || {
            if apply_overwrite_policy(&dest, overwrite)? {
                return Ok(());
            }
            let bytes = Installer::decompress(raw_bytes, compression)?;
            write_file(&dest, &bytes)?;
            apply_mode(&dest, mode)?;
            Ok(())
        })
    }
}

/// Builder for installing an embedded directory tree. Created by
/// [`Installer::dir`](crate::Installer::dir). Chain
/// [`filter`](Self::filter) and [`on_error`](Self::on_error) for
/// fine-grained control.
#[must_use = "builder ops do nothing until `.install()` is called"]
pub struct DirOp<'i> {
    pub(crate) installer: &'i mut Installer,
    pub(crate) source: Source,
    pub(crate) dst: PathBuf,
    pub(crate) status: Option<String>,
    pub(crate) log: Option<String>,
    pub(crate) overwrite: OverwriteMode,
    pub(crate) mode: Option<u32>,
    pub(crate) filter: Option<DirFilter>,
    pub(crate) on_error: Option<DirErrorHandler>,
    /// Weight applied per-file inside the directory tree. Default 1.
    pub(crate) weight: u32,
}

impl_common_op_setters!(DirOp);

impl<'i> DirOp<'i> {
    /// How to react when a destination file inside the tree already
    /// exists. Defaults to [`OverwriteMode::Overwrite`].
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
    /// Run the op: walk the embedded tree and install each file under
    /// the destination directory, honoring the filter / overwrite /
    /// error policies.
    pub fn install(self) -> Result<()> {
        self.installer.check_cancelled()?;
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
            self.weight,
        )
    }
}

/// Builder for writing the embedded uninstaller binary to disk.
/// Created by [`Installer::uninstaller`](crate::Installer::uninstaller).
#[must_use = "builder ops do nothing until `.install()` is called"]
pub struct UninstallerOp<'i> {
    pub(crate) installer: &'i mut Installer,
    pub(crate) dst: PathBuf,
    pub(crate) status: Option<String>,
    pub(crate) log: Option<String>,
    pub(crate) overwrite: OverwriteMode,
    pub(crate) weight: u32,
}

impl_common_op_setters!(UninstallerOp);

impl<'i> UninstallerOp<'i> {
    /// How to react when the destination already exists. Defaults to
    /// [`OverwriteMode::Overwrite`].
    pub fn overwrite(mut self, mode: OverwriteMode) -> Self {
        self.overwrite = mode;
        self
    }
    /// Run the op: decompress and write the embedded uninstaller
    /// binary, marking it executable on Unix.
    pub fn install(self) -> Result<()> {
        self.installer.check_cancelled()?;
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);

        let dest = self.installer.resolve_out_path(&self.dst)?;
        let overwrite = self.overwrite;
        let weight = self.weight;
        let data_ptr = self.installer.uninstaller_data;
        let compression = self.installer.uninstaller_compression;

        self.installer.run_weighted_step(weight, || {
            if apply_overwrite_policy(&dest, overwrite)? {
                return Ok(());
            }
            let data = Installer::decompress(data_ptr, compression)?;
            write_file(&dest, &data)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
                    .with_context(|| format!("failed to set permissions on: {}", dest.display()))?;
            }
            Ok(())
        })
    }
}

/// Builder for creating an empty directory at install time. Created
/// by [`Installer::mkdir`](crate::Installer::mkdir).
#[must_use = "builder ops do nothing until `.install()` is called"]
pub struct MkdirOp<'i> {
    pub(crate) installer: &'i mut Installer,
    pub(crate) dst: PathBuf,
    pub(crate) status: Option<String>,
    pub(crate) log: Option<String>,
    pub(crate) weight: u32,
}

impl_common_op_setters!(MkdirOp);

impl<'i> MkdirOp<'i> {
    /// Run the op: create the directory (and any missing parents).
    pub fn install(self) -> Result<()> {
        self.installer.check_cancelled()?;
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);
        let path = self.installer.resolve_out_path(&self.dst)?;
        self.installer.run_weighted_step(self.weight, || {
            std::fs::create_dir_all(&path)
                .with_context(|| format!("failed to create directory: {}", path.display()))
        })
    }
}

/// Builder for removing a path at install / uninstall time. Created
/// by [`Installer::remove`](crate::Installer::remove). Files are
/// deleted; directories are removed recursively.
#[must_use = "builder ops do nothing until `.install()` is called"]
pub struct RemoveOp<'i> {
    pub(crate) installer: &'i mut Installer,
    pub(crate) path: PathBuf,
    pub(crate) status: Option<String>,
    pub(crate) log: Option<String>,
    pub(crate) weight: u32,
}

impl_common_op_setters!(RemoveOp);

impl<'i> RemoveOp<'i> {
    /// Run the op: delete the path if it exists, recursively for
    /// directories. Missing paths are silently ignored.
    pub fn install(self) -> Result<()> {
        self.installer.check_cancelled()?;
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);
        let p = self.installer.resolve_out_path(&self.path)?;
        self.installer.run_weighted_step(self.weight, || {
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
        })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

pub(crate) fn find_file(
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

pub(crate) fn find_dir(
    entries: &'static [EmbeddedEntry],
    hash: u64,
) -> Result<&'static [DirChild]> {
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
pub(crate) fn install_children(
    children: &[DirChild],
    dest: &Path,
    rel_prefix: &str,
    installer: &Installer,
    overwrite: OverwriteMode,
    mode: Option<u32>,
    filter: Option<&DirFilterRef>,
    on_error: Option<&DirErrorHandlerRef>,
    weight: u32,
) -> Result<()> {
    for child in children {
        installer.check_cancelled()?;
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
                let res = installer.run_weighted_step(weight, || {
                    install_one_file(data, compression, &target, overwrite, mode)
                });
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
                    children, &target, &rel, installer, overwrite, mode, filter, on_error, weight,
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
) -> Result<()> {
    if apply_overwrite_policy(dest, overwrite)? {
        return Ok(());
    }
    let bytes = Installer::decompress(data, compression)?;
    write_file(dest, &bytes)?;
    apply_mode(dest, mode)?;
    Ok(())
}

/// Apply the chosen [`OverwriteMode`] to `dest`. Returns `Ok(true)` when
/// the caller should skip writing (e.g. Skip mode and the file already
/// exists); `Ok(false)` when the caller should proceed; `Err` for Error
/// mode on an existing file or when a backup operation fails.
pub(crate) fn apply_overwrite_policy(dest: &Path, overwrite: OverwriteMode) -> Result<bool> {
    match overwrite {
        OverwriteMode::Overwrite => Ok(false),
        OverwriteMode::Skip => Ok(dest.exists()),
        OverwriteMode::Error => {
            if dest.exists() {
                Err(anyhow!("destination already exists: {}", dest.display()))
            } else {
                Ok(false)
            }
        }
        OverwriteMode::Backup => {
            if dest.exists() {
                backup_path(dest)?;
            }
            Ok(false)
        }
    }
}

pub(crate) fn backup_path(path: &Path) -> Result<()> {
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
pub(crate) fn apply_mode(path: &Path, mode: Option<u32>) -> Result<()> {
    if let Some(m) = mode {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(m))
            .with_context(|| format!("failed to set permissions on: {}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn apply_mode(_path: &Path, _mode: Option<u32>) -> Result<()> {
    Ok(())
}

pub(crate) fn write_file(dest: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir for: {}", dest.display()))?;
    }
    std::fs::write(dest, data).with_context(|| format!("failed to write: {}", dest.display()))
}
