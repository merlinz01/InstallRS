use anyhow::Result;

use crate::Installer;

/// Callback type for the install page closure.
#[doc(hidden)]
pub type InstallCallback = Box<dyn FnOnce(&mut Installer) -> Result<()> + Send + 'static>;

/// Callback that runs after a page becomes visible via forward navigation.
/// Back navigation does not fire this callback.
#[doc(hidden)]
pub type OnEnterCallback = Box<dyn Fn(&mut Installer) -> Result<()> + 'static>;

/// Callback that runs before forward navigation away from a page.
/// Returning `Ok(false)` cancels the navigation. Back navigation does not
/// fire this callback.
#[doc(hidden)]
pub type OnBeforeLeaveCallback = Box<dyn Fn(&mut Installer) -> Result<bool> + 'static>;

/// Pure predicate: returns `true` to hide the page. Evaluated every time
/// the wizard navigates past it, so the outcome can change mid-wizard as
/// installer state (options, component selection, etc.) evolves. Must not
/// mutate state — run side effects from `on_enter` instead.
#[doc(hidden)]
pub type SkipIfCallback = Box<dyn Fn(&Installer) -> bool + 'static>;

/// Configuration for the wizard-style installer GUI.
#[doc(hidden)]
pub struct WizardConfig {
    pub title: String,
    pub pages: Vec<ConfiguredPage>,
    pub buttons: ButtonLabels,
}

/// A wizard page with optional navigation callbacks.
#[doc(hidden)]
pub struct ConfiguredPage {
    pub page: WizardPage,
    pub on_enter: Option<OnEnterCallback>,
    pub on_before_leave: Option<OnBeforeLeaveCallback>,
    pub skip_if: Option<SkipIfCallback>,
}

impl ConfiguredPage {
    pub fn new(page: WizardPage) -> Self {
        Self {
            page,
            on_enter: None,
            on_before_leave: None,
            skip_if: None,
        }
    }
}

/// Labels for the wizard navigation buttons. Customize via
/// [`InstallerGui::buttons`](crate::gui::InstallerGui::buttons) to translate
/// or rename them.
pub struct ButtonLabels {
    /// Label for the Back button (default: `"< Back"`).
    pub back: String,
    /// Label for the Next button on non-terminal pages (default: `"Next >"`).
    pub next: String,
    /// Label shown on the Next button immediately before an install
    /// page (default: `"Install"`).
    pub install: String,
    /// Label shown on the Next button immediately before an uninstall
    /// page — set via [`crate::gui::InstallerGui::uninstall_page`]
    /// (default: `"Uninstall"`).
    pub uninstall: String,
    /// Label shown on the Next button on the finish page
    /// (default: `"Finish"`).
    pub finish: String,
    /// Label for the Cancel button (default: `"Cancel"`).
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
#[doc(hidden)]
pub enum WizardPage {
    Welcome {
        title: String,
        message: String,
        /// Optional column of widgets (same set as a custom page) shown
        /// below the message. Populated via
        /// [`PageHandle::with_widgets`](crate::gui::PageHandle::with_widgets).
        widgets: Vec<CustomWidget>,
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
    Install {
        callback: InstallCallback,
        /// When true, the Next button preceding this page (and visible while
        /// on it) renders `buttons.uninstall` instead of `buttons.install`.
        /// Set via [`crate::gui::InstallerGui::uninstall_page`].
        is_uninstall: bool,
        /// When false, the rolling log textbox is omitted from the page —
        /// only the status label and progress bar are shown. Toggle via
        /// [`crate::gui::PageHandle::hide_log`].
        show_log: bool,
    },
    Finish {
        title: String,
        message: String,
        /// Optional column of widgets (same set as a custom page) shown
        /// below the message. Populated via
        /// [`PageHandle::with_widgets`](crate::gui::PageHandle::with_widgets).
        widgets: Vec<CustomWidget>,
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
#[doc(hidden)]
#[derive(Clone, Debug)]
pub enum CustomWidget {
    /// Single-line text entry (optionally masked as a password).
    Text {
        key: String,
        label: String,
        password: bool,
    },
    /// Multi-line text area (`rows` lines tall).
    Multiline {
        key: String,
        label: String,
        rows: u32,
    },
    /// Integer entry. Stored as `OptionValue::Int`.
    Number { key: String, label: String },
    /// Single boolean toggle (the label sits next to the checkbox itself).
    Checkbox { key: String, label: String },
    /// One-of-N dropdown. `choices` is `(value, display_label)` — the
    /// stored option value is the `value` field of the selected pair.
    Dropdown {
        key: String,
        label: String,
        choices: Vec<(String, String)>,
    },
    /// Vertical stack of radio buttons; same (value, display) shape as
    /// [`CustomWidget::Dropdown`]. Exactly one is selected at a time.
    Radio {
        key: String,
        label: String,
        choices: Vec<(String, String)>,
    },
    /// Text entry + Browse button that opens a native file-open dialog.
    /// `filters` is `(display_label, glob_pattern)` — e.g.
    /// `[("Config", "*.toml;*.yaml"), ("All files", "*.*")]`.
    FilePicker {
        key: String,
        label: String,
        filters: Vec<(String, String)>,
    },
    /// Text entry + Browse button that opens a native folder-picker dialog.
    DirPicker { key: String, label: String },
}

/// Builds the widget list for a [`WizardPage::Custom`]. Each method
/// appends one widget to the column; the returned `&mut Self` lets you
/// chain. Widget keys are the same string used by
/// [`crate::Installer::option`] — so CLI `--<key>=<value>` overrides
/// (when the matching option is registered via
/// [`crate::Installer::add_option`] before `process_commandline`)
/// pre-fill the field.
pub struct CustomPageBuilder {
    pub(crate) widgets: Vec<CustomWidget>,
}

impl CustomPageBuilder {
    pub(crate) fn new() -> Self {
        Self {
            widgets: Vec::new(),
        }
    }

    /// Add a single-line text entry. Pre-fills from the option store; seed
    /// a default with [`crate::Installer::set_option_if_unset`].
    pub fn text(&mut self, key: impl AsRef<str>, label: impl AsRef<str>) -> &mut Self {
        self.widgets.push(CustomWidget::Text {
            key: key.as_ref().into(),
            label: label.as_ref().into(),
            password: false,
        });
        self
    }

    /// Add a masked single-line password entry.
    pub fn password(&mut self, key: impl AsRef<str>, label: impl AsRef<str>) -> &mut Self {
        self.widgets.push(CustomWidget::Text {
            key: key.as_ref().into(),
            label: label.as_ref().into(),
            password: true,
        });
        self
    }

    /// Add a checkbox with the given label.
    pub fn checkbox(&mut self, key: impl AsRef<str>, label: impl AsRef<str>) -> &mut Self {
        self.widgets.push(CustomWidget::Checkbox {
            key: key.as_ref().into(),
            label: label.as_ref().into(),
        });
        self
    }

    /// Add a dropdown. `choices` is `(value, display_label)`; the stored
    /// option value is the `value` field of the selected pair.
    pub fn dropdown(
        &mut self,
        key: impl AsRef<str>,
        label: impl AsRef<str>,
        choices: &[(&str, &str)],
    ) -> &mut Self {
        self.widgets.push(CustomWidget::Dropdown {
            key: key.as_ref().into(),
            label: label.as_ref().into(),
            choices: choices
                .iter()
                .map(|(v, d)| ((*v).into(), (*d).into()))
                .collect(),
        });
        self
    }

    /// Add a radio-button group. Same shape as [`Self::dropdown`].
    pub fn radio(
        &mut self,
        key: impl AsRef<str>,
        label: impl AsRef<str>,
        choices: &[(&str, &str)],
    ) -> &mut Self {
        self.widgets.push(CustomWidget::Radio {
            key: key.as_ref().into(),
            label: label.as_ref().into(),
            choices: choices
                .iter()
                .map(|(v, d)| ((*v).into(), (*d).into()))
                .collect(),
        });
        self
    }

    /// Add an integer entry. Stored as `OptionValue::Int`.
    pub fn number(&mut self, key: impl AsRef<str>, label: impl AsRef<str>) -> &mut Self {
        self.widgets.push(CustomWidget::Number {
            key: key.as_ref().into(),
            label: label.as_ref().into(),
        });
        self
    }

    /// Add a multi-line text area `rows` lines tall.
    pub fn multiline(
        &mut self,
        key: impl AsRef<str>,
        label: impl AsRef<str>,
        rows: u32,
    ) -> &mut Self {
        self.widgets.push(CustomWidget::Multiline {
            key: key.as_ref().into(),
            label: label.as_ref().into(),
            rows,
        });
        self
    }

    /// Add a file-picker (text entry + Browse button → native file-open
    /// dialog). `filters` is `(display, glob)` — e.g.
    /// `&[("Config", "*.toml;*.yaml"), ("All files", "*.*")]`.
    pub fn file_picker(
        &mut self,
        key: impl AsRef<str>,
        label: impl AsRef<str>,
        filters: &[(&str, &str)],
    ) -> &mut Self {
        self.widgets.push(CustomWidget::FilePicker {
            key: key.as_ref().into(),
            label: label.as_ref().into(),
            filters: filters
                .iter()
                .map(|(d, p)| ((*d).into(), (*p).into()))
                .collect(),
        });
        self
    }

    /// Add a directory-picker (text entry + Browse button → native
    /// folder-picker dialog).
    pub fn dir_picker(&mut self, key: impl AsRef<str>, label: impl AsRef<str>) -> &mut Self {
        self.widgets.push(CustomWidget::DirPicker {
            key: key.as_ref().into(),
            label: label.as_ref().into(),
        });
        self
    }
}

/// Messages sent from the background install thread to the GUI thread.
pub(crate) enum GuiMessage {
    SetStatus(String),
    SetProgress(f64),
    Log(String),
    Finished(Result<()>),
}

/// A [`crate::ProgressSink`] implementation that forwards events over the
/// wizard's GUI message channel. Used internally by the wizard backends.
pub(crate) struct ChannelSink {
    tx: std::sync::mpsc::Sender<GuiMessage>,
}

impl ChannelSink {
    pub(crate) fn new(tx: std::sync::mpsc::Sender<GuiMessage>) -> Self {
        Self { tx }
    }
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
