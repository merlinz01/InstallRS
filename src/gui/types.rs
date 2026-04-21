use anyhow::Result;
use std::sync::{Arc, Mutex};

use crate::Installer;

/// Callback type for the install page closure.
pub type InstallCallback = Box<dyn FnOnce(&mut GuiContext) -> Result<()> + Send + 'static>;

/// Callback that runs after a page becomes visible via forward navigation.
/// Back navigation does not fire this callback.
pub type OnEnterCallback = Box<dyn Fn(&mut PageContext) -> Result<()> + 'static>;

/// Callback that runs before forward navigation away from a page.
/// Returning `Ok(false)` cancels the navigation. Back navigation does not
/// fire this callback.
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
    pub uninstall: String,
    pub finish: String,
    pub cancel: String,
}

impl Default for ButtonLabels {
    fn default() -> Self {
        Self {
            back: "< Back".into(),
            next: "Next >".into(),
            install: "Install".into(),
            uninstall: "Uninstall".into(),
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
        /// When true, the Next button preceding this page (and visible while
        /// on it) renders `buttons.uninstall` instead of `buttons.install`.
        /// Set via [`InstallerGui::uninstall_page`].
        is_uninstall: bool,
    },
    Finish {
        title: String,
        message: String,
    },
    /// Shown after the install page when the install callback returns an
    /// error or the user cancels mid-install. The actual error text is
    /// populated at runtime.
    Error {
        title: String,
        message: String,
    },
    /// A user-defined page that lays out a column of simple widgets (text
    /// inputs, checkboxes, dropdowns). Each widget is tied to an
    /// [`crate::OptionValue`] keyed by `key`; values are pre-filled from the
    /// installer's options store on entry and written back on forward
    /// navigation. Validation belongs in `on_before_leave`.
    Custom {
        heading: String,
        label: String,
        widgets: Vec<CustomWidget>,
    },
}

/// A single row on a [`WizardPage::Custom`]. Built via
/// [`CustomPageBuilder`] and rendered as a labeled input control in a
/// vertical column.
#[derive(Clone, Debug)]
pub enum CustomWidget {
    /// Single-line text entry (optionally masked as a password).
    Text {
        key: String,
        label: String,
        default: String,
        password: bool,
    },
    /// Multi-line text area (`rows` lines tall).
    Multiline {
        key: String,
        label: String,
        default: String,
        rows: u32,
    },
    /// Integer entry. Stored as `OptionValue::Int`.
    Number {
        key: String,
        label: String,
        default: i64,
    },
    /// Single boolean toggle (the label sits next to the checkbox itself).
    Checkbox {
        key: String,
        label: String,
        default: bool,
    },
    /// One-of-N dropdown. `choices` is `(value, display_label)` — the
    /// stored option value is the `value` field of the selected pair.
    Dropdown {
        key: String,
        label: String,
        choices: Vec<(String, String)>,
        default: String,
    },
    /// Vertical stack of radio buttons; same (value, display) shape as
    /// [`CustomWidget::Dropdown`]. Exactly one is selected at a time.
    Radio {
        key: String,
        label: String,
        choices: Vec<(String, String)>,
        default: String,
    },
    /// Text entry + Browse button that opens a native file-open dialog.
    /// `filters` is `(display_label, glob_pattern)` — e.g.
    /// `[("Config", "*.toml;*.yaml"), ("All files", "*.*")]`.
    FilePicker {
        key: String,
        label: String,
        default: String,
        filters: Vec<(String, String)>,
    },
    /// Text entry + Browse button that opens a native folder-picker dialog.
    DirPicker {
        key: String,
        label: String,
        default: String,
    },
}

/// Builds the widget list for a [`WizardPage::Custom`]. Each method
/// appends one widget to the column; the returned `&mut Self` lets you
/// chain. Widget keys are the same string used by
/// [`crate::Installer::get_option`] / [`crate::Installer::option_value`]
/// — so CLI `--<key>=<value>` overrides (when the matching option is
/// registered via [`crate::Installer::option`] before
/// `process_commandline`) pre-fill the field.
pub struct CustomPageBuilder {
    pub(crate) widgets: Vec<CustomWidget>,
}

impl CustomPageBuilder {
    pub(crate) fn new() -> Self {
        Self {
            widgets: Vec::new(),
        }
    }

    /// Add a single-line text entry.
    pub fn text(&mut self, key: &str, label: &str, default: &str) -> &mut Self {
        self.widgets.push(CustomWidget::Text {
            key: key.into(),
            label: label.into(),
            default: default.into(),
            password: false,
        });
        self
    }

    /// Add a masked single-line password entry.
    pub fn password(&mut self, key: &str, label: &str) -> &mut Self {
        self.widgets.push(CustomWidget::Text {
            key: key.into(),
            label: label.into(),
            default: String::new(),
            password: true,
        });
        self
    }

    /// Add a checkbox with the given label.
    pub fn checkbox(&mut self, key: &str, label: &str, default: bool) -> &mut Self {
        self.widgets.push(CustomWidget::Checkbox {
            key: key.into(),
            label: label.into(),
            default,
        });
        self
    }

    /// Add a dropdown. `choices` is `(value, display_label)`; `default` is
    /// the `value` of the initially-selected entry (use the first entry's
    /// value if you don't care).
    pub fn dropdown(
        &mut self,
        key: &str,
        label: &str,
        choices: &[(&str, &str)],
        default: &str,
    ) -> &mut Self {
        self.widgets.push(CustomWidget::Dropdown {
            key: key.into(),
            label: label.into(),
            choices: choices
                .iter()
                .map(|(v, d)| ((*v).into(), (*d).into()))
                .collect(),
            default: default.into(),
        });
        self
    }

    /// Add a radio-button group. Same shape as [`Self::dropdown`].
    pub fn radio(
        &mut self,
        key: &str,
        label: &str,
        choices: &[(&str, &str)],
        default: &str,
    ) -> &mut Self {
        self.widgets.push(CustomWidget::Radio {
            key: key.into(),
            label: label.into(),
            choices: choices
                .iter()
                .map(|(v, d)| ((*v).into(), (*d).into()))
                .collect(),
            default: default.into(),
        });
        self
    }

    /// Add an integer entry. Stored as `OptionValue::Int`.
    pub fn number(&mut self, key: &str, label: &str, default: i64) -> &mut Self {
        self.widgets.push(CustomWidget::Number {
            key: key.into(),
            label: label.into(),
            default,
        });
        self
    }

    /// Add a multi-line text area `rows` lines tall.
    pub fn multiline(&mut self, key: &str, label: &str, default: &str, rows: u32) -> &mut Self {
        self.widgets.push(CustomWidget::Multiline {
            key: key.into(),
            label: label.into(),
            default: default.into(),
            rows,
        });
        self
    }

    /// Add a file-picker (text entry + Browse button → native file-open
    /// dialog). `filters` is `(display, glob)` — e.g.
    /// `&[("Config", "*.toml;*.yaml"), ("All files", "*.*")]`.
    pub fn file_picker(
        &mut self,
        key: &str,
        label: &str,
        default: &str,
        filters: &[(&str, &str)],
    ) -> &mut Self {
        self.widgets.push(CustomWidget::FilePicker {
            key: key.into(),
            label: label.into(),
            default: default.into(),
            filters: filters
                .iter()
                .map(|(d, p)| ((*d).into(), (*p).into()))
                .collect(),
        });
        self
    }

    /// Add a directory-picker (text entry + Browse button → native
    /// folder-picker dialog).
    pub fn dir_picker(&mut self, key: &str, label: &str, default: &str) -> &mut Self {
        self.widgets.push(CustomWidget::DirPicker {
            key: key.into(),
            label: label.into(),
            default: default.into(),
        });
        self
    }
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
