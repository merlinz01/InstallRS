mod pages;
mod window;

use anyhow::Result;
use std::sync::{mpsc, Arc, Mutex};

use super::types::{ConfiguredPage, GuiMessage, WizardConfig, WizardPage};
use crate::Installer;

/// Run the wizard GUI on the main thread, spawning the install callback on a
/// background thread when the install page is reached.
pub fn run_wizard(config: WizardConfig, installer: &mut Installer) -> Result<()> {
    // Take ownership of the installer into a shared handle so the background
    // thread can use it.  We swap in a dummy that will be replaced after the
    // wizard finishes.
    // Grab the real installer's cancellation flag BEFORE we swap it out, so
    // the Cancel button, the Ctrl+C handler, and `check_cancelled()` inside
    // the install callback all see the same flag.
    let cancelled = installer.cancellation_flag();

    let installer_taken = std::mem::replace(installer, Installer::new(&[], &[], "none"));
    let installer_arc = Arc::new(Mutex::new(installer_taken));

    // Channel for background → GUI messages.
    let (tx, rx) = mpsc::channel::<GuiMessage>();

    // Extract the install callback from the config pages.
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
                    // Placeholder — the real callback is moved out.
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

    // Build and run the wizard window.
    let result = window::run(
        wizard_config,
        installer_arc.clone(),
        cancelled,
        tx,
        rx,
        install_callback,
    );

    // Restore the installer back to the caller.
    let restored = Arc::try_unwrap(installer_arc)
        .map_err(|_| anyhow::anyhow!("installer still referenced after wizard closed"))?
        .into_inner()
        .map_err(|e| anyhow::anyhow!("installer mutex poisoned: {e}"))?;
    *installer = restored;

    result
}
