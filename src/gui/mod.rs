mod types;

#[cfg(feature = "gui-win32")]
mod win32;

pub use types::{GuiContext, GuiMessage, InstallCallback, WizardConfig, WizardPage};

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
            },
        }
    }

    /// Set the window title.
    pub fn title(mut self, title: &str) -> Self {
        self.config.title = title.to_string();
        self
    }

    /// Add a welcome page with a title and description message.
    pub fn welcome(mut self, title: &str, message: &str) -> Self {
        self.config.pages.push(WizardPage::Welcome {
            title: title.to_string(),
            message: message.to_string(),
        });
        self
    }

    /// Add a license agreement page.
    ///
    /// `heading` is the title displayed above the license text, and `accept_label`
    /// is the label on the acceptance checkbox (both translatable by the caller).
    pub fn license(mut self, heading: &str, text: &str, accept_label: &str) -> Self {
        self.config.pages.push(WizardPage::License {
            heading: heading.to_string(),
            text: text.to_string(),
            accept_label: accept_label.to_string(),
        });
        self
    }

    /// Add a directory picker page with a default path.
    pub fn directory_picker(mut self, default: &str) -> Self {
        self.config.pages.push(WizardPage::DirectoryPicker {
            default: default.to_string(),
        });
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
        self.config.pages.push(WizardPage::Install {
            callback: Box::new(callback),
        });
        self
    }

    /// Add a finish page shown after installation completes.
    pub fn finish_page(mut self, title: &str, message: &str) -> Self {
        self.config.pages.push(WizardPage::Finish {
            title: title.to_string(),
            message: message.to_string(),
        });
        self
    }

    /// Run the wizard GUI. This blocks until the user closes the window.
    ///
    /// On Windows with the `gui-win32` feature, this creates a native Win32 wizard.
    /// Falls back to an error on unsupported platforms.
    pub fn run(self, installer: &mut Installer) -> Result<()> {
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

    #[cfg(all(feature = "gui", not(feature = "gui-win32")))]
    fn run_platform(self, _installer: &mut Installer) -> Result<()> {
        Err(anyhow::anyhow!(
            "No GUI backend available for this platform. Enable the `gui-win32` feature on Windows."
        ))
    }
}
