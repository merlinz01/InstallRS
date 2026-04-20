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

/// Run the wizard GUI on the main thread, spawning the install callback on a
/// background thread when the install page is reached.
pub fn run_wizard(config: WizardConfig, installer: &mut Installer) -> Result<()> {
    // Grab the real installer's cancellation flag BEFORE we swap it out, so
    // the Cancel button, the Ctrl+C handler, and `check_cancelled()` inside
    // the install callback all see the same flag.
    let cancelled = installer.cancellation_flag();

    let installer_taken = std::mem::replace(installer, Installer::new(&[], &[], "none"));
    let installer_arc = Arc::new(Mutex::new(installer_taken));

    let default_dir = find_default_dir(&config.pages);
    let install_dir = Arc::new(Mutex::new(default_dir));

    let (tx, rx) = mpsc::channel::<GuiMessage>();

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
        on_start: None,
        on_exit: None,
    };

    let result = window::run(
        wizard_config,
        installer_arc.clone(),
        install_dir,
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

fn find_default_dir(pages: &[ConfiguredPage]) -> String {
    for configured in pages {
        if let WizardPage::DirectoryPicker { default, .. } = &configured.page {
            return default.clone();
        }
    }
    String::new()
}
