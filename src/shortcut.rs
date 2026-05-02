//! Windows `.lnk` shortcut builder op plus the shell-notification call that
//! makes freshly-written shortcuts appear in Explorer / Start Menu without
//! a manual refresh.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::ops::impl_common_op_setters;
use crate::Installer;

/// Builder for creating a Windows `.lnk` shortcut. Created by
/// [`Installer::shortcut`](crate::Installer::shortcut).
pub struct ShortcutOp<'i> {
    pub(crate) installer: &'i mut Installer,
    pub(crate) dst: PathBuf,
    pub(crate) target: PathBuf,
    pub(crate) arguments: Option<String>,
    pub(crate) working_dir: Option<PathBuf>,
    pub(crate) description: Option<String>,
    pub(crate) icon: Option<(PathBuf, i32)>,
    pub(crate) status: Option<String>,
    pub(crate) log: Option<String>,
    pub(crate) weight: u32,
}

impl_common_op_setters!(ShortcutOp);

impl<'i> ShortcutOp<'i> {
    /// Command-line arguments passed to the target when the shortcut runs.
    pub fn arguments(mut self, s: impl AsRef<str>) -> Self {
        self.arguments = Some(s.as_ref().to_string());
        self
    }
    /// Working directory the target launches in. Resolved against
    /// `out_dir` when relative.
    pub fn working_dir(mut self, p: impl AsRef<Path>) -> Self {
        self.working_dir = Some(p.as_ref().to_path_buf());
        self
    }
    /// Tooltip / comment shown by Explorer.
    pub fn description(mut self, s: impl AsRef<str>) -> Self {
        self.description = Some(s.as_ref().to_string());
        self
    }
    /// Icon path (resolved against `out_dir` when relative) and resource
    /// index within it. Use index `0` for single-icon files.
    pub fn icon(mut self, path: impl AsRef<Path>, index: i32) -> Self {
        self.icon = Some((path.as_ref().to_path_buf(), index));
        self
    }
    /// Run the op: write the `.lnk` file and notify Explorer to pick
    /// it up. Creates parent directories as needed.
    pub fn install(self) -> Result<()> {
        self.installer.check_cancelled()?;
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);
        let dst = self.installer.resolve_out_path(&self.dst)?;
        let target = self.installer.resolve_out_path(&self.target)?;
        let working_dir = match self.working_dir {
            Some(ref w) => Some(self.installer.resolve_out_path(w)?),
            None => None,
        };
        let icon = match self.icon {
            Some((ref p, idx)) => Some((self.installer.resolve_out_path(p)?, idx)),
            None => None,
        };
        let arguments = self.arguments;
        let description = self.description;
        self.installer.run_weighted_step(self.weight, || {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create shortcut parent: {}", parent.display())
                })?;
            }
            let target_str = target
                .to_str()
                .ok_or_else(|| anyhow!("shortcut target path is not valid UTF-8"))?;
            let mut link = mslnk::ShellLink::new(target_str).with_context(|| {
                format!("failed to build shortcut for target {}", target.display())
            })?;
            if let Some(a) = arguments {
                link.set_arguments(Some(a));
            }
            if let Some(wd) = working_dir {
                let wd = wd
                    .to_str()
                    .ok_or_else(|| anyhow!("shortcut working_dir is not valid UTF-8"))?;
                link.set_working_dir(Some(wd.to_string()));
            }
            if let Some(d) = description {
                link.set_name(Some(d));
            }
            if let Some((p, idx)) = icon {
                let p = p
                    .to_str()
                    .ok_or_else(|| anyhow!("shortcut icon path is not valid UTF-8"))?;
                let loc = if idx == 0 {
                    p.to_string()
                } else {
                    format!("{p},{idx}")
                };
                link.set_icon_location(Some(loc));
            }
            link.create_lnk(&dst)
                .with_context(|| format!("failed to write shortcut: {}", dst.display()))?;
            // Notify the shell so the new shortcut shows up immediately in
            // Explorer / Start Menu / Desktop without needing a refresh.
            notify_shell_create(&dst);
            Ok(())
        })
    }
}

extern "system" {
    fn SHChangeNotify(
        w_event_id: std::os::raw::c_long,
        u_flags: std::os::raw::c_uint,
        dw_item1: *const std::ffi::c_void,
        dw_item2: *const std::ffi::c_void,
    );
}

fn notify_shell_create(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    const SHCNE_CREATE: std::os::raw::c_long = 0x0000_0002;
    const SHCNF_PATHW: std::os::raw::c_uint = 0x0005;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: SHChangeNotify accepts a null-terminated wide path as dwItem1
    // when SHCNF_PATHW is set, and a null dwItem2 is valid for SHCNE_CREATE.
    unsafe {
        SHChangeNotify(
            SHCNE_CREATE,
            SHCNF_PATHW,
            wide.as_ptr() as *const std::ffi::c_void,
            std::ptr::null(),
        );
    }
}
