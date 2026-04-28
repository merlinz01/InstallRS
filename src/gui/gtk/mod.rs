mod pages;
mod window;

use anyhow::Result;
use std::sync::{mpsc, Arc, Mutex, Once};

use super::types::{ConfiguredPage, GuiMessage, WizardConfig, WizardPage};
use crate::Installer;

/// Call `gtk::disable_setlocale()` at most once, and always before the first
/// `gtk::init()`. GTK panics if `disable_setlocale` is called after init, so
/// subsequent wizard runs and dialog calls must not re-invoke it.
pub(crate) fn disable_setlocale_once() {
    static GUARD: Once = Once::new();
    GUARD.call_once(|| {
        gtk::disable_setlocale();
    });
}

/// PNG bytes for the default window icon, stashed by
/// [`crate::gui::__set_window_icon_png`]. Read by
/// [`apply_default_window_icon`] after each `gtk::init()`.
static ICON_BYTES: std::sync::OnceLock<&'static [u8]> = std::sync::OnceLock::new();

pub(crate) fn set_icon_bytes(bytes: &'static [u8]) {
    let _ = ICON_BYTES.set(bytes);
}

/// Apply the stashed window icon as GTK's process-wide default so every
/// window and dialog inherits it (title bar, taskbar, Alt-Tab). Must be
/// called after `gtk::init()`. Safe to call repeatedly; GTK copies the
/// pixbuf internally so only the per-call parse cost applies.
pub(crate) fn apply_default_window_icon() {
    use gtk::prelude::*;
    let Some(bytes) = ICON_BYTES.get().copied() else {
        return;
    };
    let loader = gtk::gdk_pixbuf::PixbufLoader::new();
    if loader.write(bytes).is_err() {
        return;
    }
    if loader.close().is_err() {
        return;
    }
    if let Some(pixbuf) = loader.pixbuf() {
        gtk::Window::set_default_icon(&pixbuf);
    }
}

/// Run the wizard GUI on the main thread, spawning the install callback on a
/// background thread when the install page is reached.
pub fn run_wizard(config: WizardConfig, installer: &mut Installer) -> Result<()> {
    // Grab the real installer's cancellation flag BEFORE we swap it out, so
    // the Cancel button, the Ctrl+C handler, and `check_cancelled()` inside
    // the install callback all see the same flag.
    let cancelled = installer.cancellation_flag();

    let installer_taken = std::mem::replace(installer, Installer::new(&[], &[], "none"));
    let installer_arc = Arc::new(Mutex::new(installer_taken));

    let (tx, rx) = mpsc::channel::<GuiMessage>();

    let mut pages_without_callback: Vec<ConfiguredPage> = Vec::new();
    let mut install_callback = None;
    for configured in config.pages {
        let ConfiguredPage {
            page,
            on_enter,
            on_before_leave,
            skip_if,
        } = configured;
        let page = match page {
            WizardPage::Install {
                callback,
                is_uninstall,
            } => {
                install_callback = Some(callback);
                WizardPage::Install {
                    callback: Box::new(|_| Ok(())),
                    is_uninstall,
                }
            }
            other => other,
        };
        pages_without_callback.push(ConfiguredPage {
            page,
            on_enter,
            on_before_leave,
            skip_if,
        });
    }

    let wizard_config = WizardConfig {
        title: config.title,
        pages: pages_without_callback,
        buttons: config.buttons,
        on_start: None,
        on_exit: None,
    };

    let result = window::run(
        wizard_config,
        installer_arc.clone(),
        cancelled,
        tx,
        rx,
        install_callback,
    );

    let restored = Arc::try_unwrap(installer_arc)
        .map_err(|_| anyhow::anyhow!("installer still referenced after wizard closed"))?
        .into_inner()
        .map_err(|e| anyhow::anyhow!("installer mutex poisoned: {e}"))?;
    *installer = restored;

    result
}
