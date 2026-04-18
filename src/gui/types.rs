use anyhow::Result;
use std::sync::{Arc, Mutex};

use crate::Installer;

/// Callback type for the install page closure.
pub type InstallCallback = Box<dyn FnOnce(&mut GuiContext) -> Result<()> + Send + 'static>;

/// Callback that runs after a page becomes visible.
pub type OnEnterCallback = Box<dyn Fn(&mut PageContext) -> Result<()> + 'static>;

/// Callback that runs before navigating away from a page. Returning `Ok(false)`
/// cancels the navigation.
pub type OnBeforeLeaveCallback = Box<dyn Fn(&mut PageContext) -> Result<bool> + 'static>;

/// Callback that runs at wizard startup (before the window is shown, or
/// before the install callback fires in headless mode). Inspect
/// `installer.headless` to branch on mode.
pub type StartCallback = Box<dyn FnOnce(&mut crate::Installer) -> Result<()> + 'static>;

/// Callback that runs at wizard exit (after the window closes, or after
/// the install callback completes in headless mode). Always runs, even on
/// failure.
pub type ExitCallback = Box<dyn FnOnce(&mut crate::Installer) -> Result<()> + 'static>;

/// Configuration for the wizard-style installer GUI.
pub struct WizardConfig {
    pub title: String,
    pub pages: Vec<ConfiguredPage>,
    pub buttons: ButtonLabels,
    pub on_start: Option<StartCallback>,
    pub on_exit: Option<ExitCallback>,
}

/// A wizard page with optional navigation callbacks.
pub struct ConfiguredPage {
    pub page: WizardPage,
    pub on_enter: Option<OnEnterCallback>,
    pub on_before_leave: Option<OnBeforeLeaveCallback>,
}

impl ConfiguredPage {
    pub fn new(page: WizardPage) -> Self {
        Self {
            page,
            on_enter: None,
            on_before_leave: None,
        }
    }
}

/// Labels for the wizard navigation buttons. Customize via
/// [`InstallerGui::buttons`](crate::gui::InstallerGui::buttons) to translate
/// or rename them.
pub struct ButtonLabels {
    pub back: String,
    pub next: String,
    pub install: String,
    pub finish: String,
    pub cancel: String,
}

impl Default for ButtonLabels {
    fn default() -> Self {
        Self {
            back: "< Back".into(),
            next: "Next >".into(),
            install: "Install".into(),
            finish: "Finish".into(),
            cancel: "Cancel".into(),
        }
    }
}

/// A single page in the wizard flow.
pub enum WizardPage {
    Welcome {
        title: String,
        message: String,
    },
    License {
        heading: String,
        text: String,
        accept_label: String,
    },
    Components {
        heading: String,
        label: String,
    },
    DirectoryPicker {
        heading: String,
        label: String,
        default: String,
    },
    Install {
        callback: InstallCallback,
    },
    Finish {
        title: String,
        message: String,
    },
}

/// Messages sent from the background install thread to the GUI thread.
pub enum GuiMessage {
    SetStatus(String),
    SetProgress(f64),
    Log(String),
    Finished(Result<()>),
}

/// A [`crate::ProgressSink`] implementation that forwards events over the
/// wizard's GUI message channel.
struct ChannelSink {
    tx: std::sync::mpsc::Sender<GuiMessage>,
}

impl crate::ProgressSink for ChannelSink {
    fn set_status(&self, status: &str) {
        let _ = self.tx.send(GuiMessage::SetStatus(status.to_string()));
    }
    fn set_progress(&self, fraction: f64) {
        let _ = self.tx.send(GuiMessage::SetProgress(fraction));
    }
    fn log(&self, message: &str) {
        let _ = self.tx.send(GuiMessage::Log(message.to_string()));
    }
}

/// Context passed to the install closure, providing thread-safe GUI updates
/// and access to the `Installer`.
pub struct GuiContext {
    tx: std::sync::mpsc::Sender<GuiMessage>,
    installer: Arc<Mutex<Installer>>,
    install_dir: Arc<Mutex<String>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl GuiContext {
    pub fn new(
        tx: std::sync::mpsc::Sender<GuiMessage>,
        installer: Arc<Mutex<Installer>>,
        install_dir: Arc<Mutex<String>>,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            tx,
            installer,
            install_dir,
            cancelled,
        }
    }

    /// Update the status label on the install page.
    pub fn set_status(&self, status: &str) {
        let _ = self.tx.send(GuiMessage::SetStatus(status.to_string()));
    }

    /// Update the progress bar (0.0 to 1.0).
    pub fn set_progress(&self, progress: f64) {
        let _ = self.tx.send(GuiMessage::SetProgress(progress));
    }

    /// Append a log line to the install page log area.
    pub fn log(&self, message: &str) {
        let _ = self.tx.send(GuiMessage::Log(message.to_string()));
    }

    /// Get the currently selected install directory.
    pub fn install_dir(&self) -> String {
        self.install_dir.lock().unwrap().clone()
    }

    /// Get a mutable reference to the `Installer` for calling
    /// [`Installer::file`], [`Installer::dir`], etc.
    ///
    /// The returned guard holds a mutex lock — avoid holding it across GUI calls.
    pub fn installer(&self) -> std::sync::MutexGuard<'_, Installer> {
        self.installer.lock().unwrap()
    }

    /// Build a [`crate::ProgressSink`] that forwards to this GUI context.
    ///
    /// Attach it via `ctx.installer().set_progress_sink(ctx.progress_sink())`
    /// (or rely on the wizard, which does this automatically before invoking
    /// the install-page callback).
    pub fn progress_sink(&self) -> Box<dyn crate::ProgressSink> {
        Box::new(ChannelSink {
            tx: self.tx.clone(),
        })
    }

    /// Check if the user has requested cancellation.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Context passed to page `on_enter` / `on_before_leave` callbacks. Unlike
/// [`GuiContext`], these callbacks run synchronously on the GUI thread.
pub struct PageContext {
    installer: Arc<Mutex<Installer>>,
    install_dir: Arc<Mutex<String>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl PageContext {
    pub fn new(
        installer: Arc<Mutex<Installer>>,
        install_dir: Arc<Mutex<String>>,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            installer,
            install_dir,
            cancelled,
        }
    }

    /// Get a mutable reference to the `Installer`.
    pub fn installer(&self) -> std::sync::MutexGuard<'_, Installer> {
        self.installer.lock().unwrap()
    }

    /// Get the currently selected install directory.
    pub fn install_dir(&self) -> String {
        self.install_dir.lock().unwrap().clone()
    }

    /// Override the install directory.
    pub fn set_install_dir(&self, dir: &str) {
        *self.install_dir.lock().unwrap() = dir.to_string();
    }

    /// Check if the user has requested cancellation.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }
}
