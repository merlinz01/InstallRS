//! Native message-box helpers for use inside wizard callbacks.
//!
//! These functions show a modal dialog parented to the current active window
//! (typically the wizard). All functions block until the user dismisses the
//! dialog.

use anyhow::Result;

#[cfg(feature = "gui-win32")]
use winsafe::{co, prelude::*, HWND};

#[cfg(feature = "gui-win32")]
fn show_win32(title: &str, message: &str, flags: co::MB) -> Result<co::DLGID> {
    let parent = HWND::GetActiveWindow().unwrap_or(HWND::NULL);
    parent
        .MessageBox(message, title, flags)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

#[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
fn show_gtk(
    title: &str,
    message: &str,
    kind: gtk::MessageType,
    buttons: gtk::ButtonsType,
) -> Result<gtk::ResponseType> {
    use gtk::prelude::*;
    gtk::init().map_err(|e| anyhow::anyhow!("gtk init failed: {e}"))?;
    let parent = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.is_active());
    let dialog = gtk::MessageDialog::new(
        parent.as_ref(),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        kind,
        buttons,
        message,
    );
    dialog.set_title(title);
    let response = dialog.run();
    unsafe {
        dialog.destroy();
    }
    Ok(response)
}

/// Show an informational dialog with an OK button.
pub fn info(title: &str, message: &str) -> Result<()> {
    #[cfg(feature = "gui-win32")]
    {
        return show_win32(title, message, co::MB::OK | co::MB::ICONINFORMATION).map(|_| ());
    }
    #[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
    {
        return show_gtk(title, message, gtk::MessageType::Info, gtk::ButtonsType::Ok).map(|_| ());
    }
    #[cfg(not(any(feature = "gui-win32", feature = "gui-gtk")))]
    {
        let _ = (title, message);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}

/// Show a warning dialog with an OK button.
pub fn warn(title: &str, message: &str) -> Result<()> {
    #[cfg(feature = "gui-win32")]
    {
        return show_win32(title, message, co::MB::OK | co::MB::ICONWARNING).map(|_| ());
    }
    #[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
    {
        return show_gtk(
            title,
            message,
            gtk::MessageType::Warning,
            gtk::ButtonsType::Ok,
        )
        .map(|_| ());
    }
    #[cfg(not(any(feature = "gui-win32", feature = "gui-gtk")))]
    {
        let _ = (title, message);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}

/// Show an error dialog with an OK button.
pub fn error(title: &str, message: &str) -> Result<()> {
    #[cfg(feature = "gui-win32")]
    {
        return show_win32(title, message, co::MB::OK | co::MB::ICONERROR).map(|_| ());
    }
    #[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
    {
        return show_gtk(
            title,
            message,
            gtk::MessageType::Error,
            gtk::ButtonsType::Ok,
        )
        .map(|_| ());
    }
    #[cfg(not(any(feature = "gui-win32", feature = "gui-gtk")))]
    {
        let _ = (title, message);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}

/// Show a Yes/No confirmation dialog. Returns `true` if the user clicked Yes.
pub fn confirm(title: &str, message: &str) -> Result<bool> {
    #[cfg(feature = "gui-win32")]
    {
        let r = show_win32(title, message, co::MB::YESNO | co::MB::ICONQUESTION)?;
        return Ok(r == co::DLGID::YES);
    }
    #[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
    {
        let r = show_gtk(
            title,
            message,
            gtk::MessageType::Question,
            gtk::ButtonsType::YesNo,
        )?;
        return Ok(r == gtk::ResponseType::Yes);
    }
    #[cfg(not(any(feature = "gui-win32", feature = "gui-gtk")))]
    {
        let _ = (title, message);
        Err(anyhow::anyhow!("No dialog backend available"))
    }
}
