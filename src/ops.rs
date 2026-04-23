//! Cross-platform builder operations (file / dir / uninstaller / mkdir /
//! remove) plus the shared helpers that implement their file-system work.

use std::path::Path;

use anyhow::{anyhow, Context, Result};

use crate::embedded::{DirChild, DirChildKind, EmbeddedEntry};
use crate::types::{
    DirErrorHandler, DirErrorHandlerRef, DirFilter, DirFilterRef, ErrorAction, OverwriteMode,
};
use crate::{Installer, Source};

// ── Builder ops ─────────────────────────────────────────────────────────────

pub struct FileOp<'i> {
    pub(crate) installer: &'i mut Installer,
    pub(crate) source: Source,
    pub(crate) dst: String,
    pub(crate) status: Option<String>,
    pub(crate) log: Option<String>,
    pub(crate) overwrite: OverwriteMode,
    pub(crate) mode: Option<u32>,
    pub(crate) weight: u32,
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
    /// Step weight this op consumes from the component budget. Default 1.
    pub fn weight(mut self, w: u32) -> Self {
        self.weight = w;
        self
    }
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
            match overwrite {
                OverwriteMode::Overwrite => {}
                OverwriteMode::Skip => {
                    if dest.exists() {
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
            apply_mode(&dest, mode)?;
            Ok(())
        })
    }
}

pub struct DirOp<'i> {
    pub(crate) installer: &'i mut Installer,
    pub(crate) source: Source,
    pub(crate) dst: String,
    pub(crate) status: Option<String>,
    pub(crate) log: Option<String>,
    pub(crate) overwrite: OverwriteMode,
    pub(crate) mode: Option<u32>,
    pub(crate) filter: Option<DirFilter>,
    pub(crate) on_error: Option<DirErrorHandler>,
    /// Weight applied per-file inside the directory tree. Default 1.
    pub(crate) per_file_weight: u32,
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
    /// Step weight applied per-file inside the directory tree. Default 1.
    pub fn weight(mut self, w: u32) -> Self {
        self.per_file_weight = w;
        self
    }
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
            self.per_file_weight,
        )
    }
}

pub struct UninstallerOp<'i> {
    pub(crate) installer: &'i mut Installer,
    pub(crate) dst: String,
    pub(crate) status: Option<String>,
    pub(crate) log: Option<String>,
    pub(crate) overwrite: OverwriteMode,
    pub(crate) weight: u32,
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
    /// Step weight this op consumes from the component budget. Default 1.
    pub fn weight(mut self, w: u32) -> Self {
        self.weight = w;
        self
    }
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
            match overwrite {
                OverwriteMode::Overwrite => {}
                OverwriteMode::Skip => {
                    if dest.exists() {
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

pub struct MkdirOp<'i> {
    pub(crate) installer: &'i mut Installer,
    pub(crate) dst: String,
    pub(crate) status: Option<String>,
    pub(crate) log: Option<String>,
    pub(crate) weight: u32,
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
    /// Step weight this op consumes from the component budget. Default 1.
    pub fn weight(mut self, w: u32) -> Self {
        self.weight = w;
        self
    }
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

pub struct RemoveOp<'i> {
    pub(crate) installer: &'i mut Installer,
    pub(crate) path: String,
    pub(crate) status: Option<String>,
    pub(crate) log: Option<String>,
    pub(crate) weight: u32,
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
    /// Step weight this op consumes from the component budget. Default 1.
    pub fn weight(mut self, w: u32) -> Self {
        self.weight = w;
        self
    }
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
    per_file_weight: u32,
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
                let res = installer.run_weighted_step(per_file_weight, || {
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
                    children,
                    &target,
                    &rel,
                    installer,
                    overwrite,
                    mode,
                    filter,
                    on_error,
                    per_file_weight,
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
    match overwrite {
        OverwriteMode::Overwrite => {}
        OverwriteMode::Skip => {
            if dest.exists() {
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
    Ok(())
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
