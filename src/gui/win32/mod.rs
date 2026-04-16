mod pages;
mod window;

use anyhow::Result;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};

use super::types::{ConfiguredPage, GuiMessage, WizardConfig, WizardPage};
use crate::Installer;

/// Run the wizard GUI on the main thread, spawning the install callback on a
/// background thread when the install page is reached.
pub fn run_wizard(config: WizardConfig, installer: &mut Installer) -> Result<()> {
    // Take ownership of the installer into a shared handle so the background
    // thread can use it.  We swap in a dummy that will be replaced after the
    // wizard finishes.
    let installer_taken = std::mem::replace(installer, Installer::new(&[], &[], "none"));
    let installer_arc = Arc::new(Mutex::new(installer_taken));

    // Shared install directory — updated by the directory picker page.
    let default_dir = find_default_dir(&config.pages);
    let install_dir = Arc::new(Mutex::new(default_dir));

    // Cancellation flag.
    let cancelled = Arc::new(AtomicBool::new(false));

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
        } = configured;
        let page = match page {
            WizardPage::Install { callback } => {
                install_callback = Some(callback);
                WizardPage::Install {
                    // Placeholder — the real callback is moved out.
                    callback: Box::new(|_| Ok(())),
                }
            }
            other => other,
        };
        pages_without_callback.push(ConfiguredPage {
            page,
            on_enter,
            on_before_leave,
        });
    }

    let wizard_config = WizardConfig {
        title: config.title,
        pages: pages_without_callback,
        buttons: config.buttons,
    };

    // Build and run the wizard window.
    let result = window::run(
        wizard_config,
        installer_arc.clone(),
        install_dir,
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

fn find_default_dir(pages: &[ConfiguredPage]) -> String {
    for configured in pages {
        if let WizardPage::DirectoryPicker { default, .. } = &configured.page {
            return default.clone();
        }
    }
    String::new()
}
