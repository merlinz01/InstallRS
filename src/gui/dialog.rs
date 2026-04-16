//! Native message-box helpers for use inside wizard callbacks.
//!
//! These functions show a modal dialog parented to the current active window
//! (typically the wizard). All functions block until the user dismisses the
//! dialog.

use anyhow::Result;

#[cfg(feature = "gui-win32")]
use winsafe::{co, prelude::*, HWND};

#[cfg(feature = "gui-win32")]
fn show(title: &str, message: &str, flags: co::MB) -> Result<co::DLGID> {
    let parent = HWND::GetActiveWindow().unwrap_or(HWND::NULL);
    parent
        .MessageBox(message, title, flags)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Show an informational dialog with an OK button.
pub fn info(title: &str, message: &str) -> Result<()> {
    #[cfg(feature = "gui-win32")]
    {
        show(title, message, co::MB::OK | co::MB::ICONINFORMATION).map(|_| ())
    }
    #[cfg(not(feature = "gui-win32"))]
    {
        let _ = (title, message);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}

/// Show a warning dialog with an OK button.
pub fn warn(title: &str, message: &str) -> Result<()> {
    #[cfg(feature = "gui-win32")]
    {
        show(title, message, co::MB::OK | co::MB::ICONWARNING).map(|_| ())
    }
    #[cfg(not(feature = "gui-win32"))]
    {
        let _ = (title, message);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}

/// Show an error dialog with an OK button.
pub fn error(title: &str, message: &str) -> Result<()> {
    #[cfg(feature = "gui-win32")]
    {
        show(title, message, co::MB::OK | co::MB::ICONERROR).map(|_| ())
    }
    #[cfg(not(feature = "gui-win32"))]
    {
        let _ = (title, message);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}

/// Show a Yes/No confirmation dialog. Returns `true` if the user clicked Yes.
pub fn confirm(title: &str, message: &str) -> Result<bool> {
    #[cfg(feature = "gui-win32")]
    {
        let r = show(title, message, co::MB::YESNO | co::MB::ICONQUESTION)?;
        Ok(r == co::DLGID::YES)
    }
    #[cfg(not(feature = "gui-win32"))]
    {
        let _ = (title, message);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}
