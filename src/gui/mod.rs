pub mod dialog;
mod types;

#[cfg(feature = "gui-win32")]
mod win32;

#[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
mod gtk;

pub use dialog::{confirm, error, info, warn};
pub use types::{
    ButtonLabels, ConfiguredPage, GuiContext, GuiMessage, InstallCallback, OnBeforeLeaveCallback,
    OnEnterCallback, PageContext, WizardConfig, WizardPage,
};

use anyhow::Result;

use crate::Installer;

/// Builder for a wizard-style installer GUI.
///
/// # Example
///
/// ```rust,ignore
/// use installrs::gui::*;
///
/// InstallerGui::wizard()
///     .title("My App Installer")
///     .welcome("Welcome!", "Click Next to continue.")
///     .license(include_str!("../LICENSE"))
///     .directory_picker("C:/Program Files/MyApp")
///     .install_page(|ctx| {
///         ctx.set_status("Installing...");
///         ctx.installer().set_out_dir(&ctx.install_dir());
///         // ... install files ...
///         ctx.set_progress(1.0);
///         ctx.set_status("Done!");
///         Ok(())
///     })
///     .finish_page("Complete!", "Click Finish to exit.")
///     .run(i)?;
/// ```
pub struct InstallerGui {
    config: WizardConfig,
}

impl InstallerGui {
    /// Create a new wizard builder with default settings.
    pub fn wizard() -> Self {
        Self {
            config: WizardConfig {
                title: "Installer".to_string(),
                pages: Vec::new(),
                buttons: ButtonLabels::default(),
            },
        }
    }

    /// Set the window title.
    pub fn title(mut self, title: &str) -> Self {
        self.config.title = title.to_string();
        self
    }

    /// Override the navigation button labels (e.g. for translation).
    ///
    /// Use struct update syntax to override only specific labels:
    ///
    /// ```rust,ignore
    /// .buttons(ButtonLabels {
    ///     next: "Weiter".into(),
    ///     back: "Zurück".into(),
    ///     ..Default::default()
    /// })
    /// ```
    pub fn buttons(mut self, labels: ButtonLabels) -> Self {
        self.config.buttons = labels;
        self
    }

    /// Add a welcome page with a title and description message.
    pub fn welcome(mut self, title: &str, message: &str) -> Self {
        self.config
            .pages
            .push(ConfiguredPage::new(WizardPage::Welcome {
                title: title.to_string(),
                message: message.to_string(),
            }));
        self
    }

    /// Add a license agreement page.
    ///
    /// `heading` is the title displayed above the license text, and `accept_label`
    /// is the label on the acceptance checkbox (both translatable by the caller).
    pub fn license(mut self, heading: &str, text: &str, accept_label: &str) -> Self {
        self.config
            .pages
            .push(ConfiguredPage::new(WizardPage::License {
                heading: heading.to_string(),
                text: text.to_string(),
                accept_label: accept_label.to_string(),
            }));
        self
    }

    /// Add a components page (list of checkboxes for optional features).
    ///
    /// Components must be registered on the `Installer` via
    /// [`Installer::component`](crate::Installer::component) before calling
    /// [`run`](Self::run). The page renders one checkbox per registered
    /// component; required components render greyed-out.
    ///
    /// `heading` is the bold title at the top; `label` is the intro sentence
    /// above the checkbox list (e.g. "Select the features to install:").
    pub fn components_page(mut self, heading: &str, label: &str) -> Self {
        self.config
            .pages
            .push(ConfiguredPage::new(WizardPage::Components {
                heading: heading.to_string(),
                label: label.to_string(),
            }));
        self
    }

    /// Add a directory picker page with a default path.
    ///
    /// `heading` is the bold title at the top of the page, `label` is the
    /// prompt next to the path input (e.g. "Install to:"), and `default` is
    /// the initial path.
    pub fn directory_picker(mut self, heading: &str, label: &str, default: &str) -> Self {
        self.config
            .pages
            .push(ConfiguredPage::new(WizardPage::DirectoryPicker {
                heading: heading.to_string(),
                label: label.to_string(),
                default: default.to_string(),
            }));
        self
    }

    /// Add the install page with a callback that performs the actual installation.
    ///
    /// The callback receives a [`GuiContext`] for updating progress and accessing
    /// the [`Installer`].
    pub fn install_page(
        mut self,
        callback: impl FnOnce(&mut GuiContext) -> Result<()> + Send + 'static,
    ) -> Self {
        self.config
            .pages
            .push(ConfiguredPage::new(WizardPage::Install {
                callback: Box::new(callback),
            }));
        self
    }

    /// Add a finish page shown after installation completes.
    pub fn finish_page(mut self, title: &str, message: &str) -> Self {
        self.config
            .pages
            .push(ConfiguredPage::new(WizardPage::Finish {
                title: title.to_string(),
                message: message.to_string(),
            }));
        self
    }

    /// Attach an `on_enter` callback to the most recently added page.
    ///
    /// The callback runs on the GUI thread after the page becomes visible.
    pub fn on_enter<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut PageContext) -> Result<()> + 'static,
    {
        if let Some(last) = self.config.pages.last_mut() {
            last.on_enter = Some(Box::new(f));
        }
        self
    }

    /// Attach an `on_before_leave` callback to the most recently added page.
    ///
    /// Returning `Ok(false)` cancels the navigation and keeps the page visible.
    /// Returning `Err(_)` also cancels navigation.
    pub fn on_before_leave<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut PageContext) -> Result<bool> + 'static,
    {
        if let Some(last) = self.config.pages.last_mut() {
            last.on_before_leave = Some(Box::new(f));
        }
        self
    }

    /// Run the wizard GUI. This blocks until the user closes the window.
    ///
    /// On Windows with the `gui-win32` feature, this creates a native Win32 wizard.
    /// Falls back to an error on unsupported platforms.
    pub fn run(self, installer: &mut Installer) -> Result<()> {
        // Apply component CLI args (handles --list-components and exits, or
        // applies --components / --with / --without to the selection state).
        installer.apply_component_args()?;

        // In headless mode, skip the GUI entirely
        if installer.headless {
            return Err(anyhow::anyhow!("GUI installer cannot run in headless mode"));
        }

        self.run_platform(installer)
    }

    #[cfg(feature = "gui-win32")]
    fn run_platform(self, installer: &mut Installer) -> Result<()> {
        win32::run_wizard(self.config, installer)
    }

    #[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
    fn run_platform(self, installer: &mut Installer) -> Result<()> {
        gtk::run_wizard(self.config, installer)
    }

    #[cfg(all(feature = "gui", not(any(feature = "gui-win32", feature = "gui-gtk"))))]
    fn run_platform(self, _installer: &mut Installer) -> Result<()> {
        Err(anyhow::anyhow!(
            "No GUI backend available for this platform. Enable `gui-win32` on Windows or `gui-gtk` on Linux."
        ))
    }
}
