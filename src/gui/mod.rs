pub mod dialog;
mod types;

#[cfg(feature = "gui-win32")]
mod win32;

#[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
mod gtk;

pub use dialog::{choose_language, confirm, error, info, warn};
pub use types::{
    ButtonLabels, ConfiguredPage, CustomPageBuilder, CustomWidget, ExitCallback, GuiContext,
    GuiMessage, InstallCallback, OnBeforeLeaveCallback, OnEnterCallback, PageContext,
    StartCallback, WizardConfig, WizardPage,
};

use anyhow::Result;

use crate::{Installer, ProgressSink};

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
                on_start: None,
                on_exit: None,
            },
        }
    }

    /// Set a callback that runs at wizard startup, before the window is
    /// shown (or before the install callback fires in headless mode).
    ///
    /// Inspect `installer.headless` inside the callback to branch on mode.
    /// Useful for work that must happen regardless of UI — environment
    /// setup, argument validation, prerequisite checks.
    pub fn on_start(mut self, f: impl FnOnce(&mut Installer) -> Result<()> + 'static) -> Self {
        self.config.on_start = Some(Box::new(f));
        self
    }

    /// Set a callback that runs at wizard exit, after the window closes (or
    /// after the install callback completes in headless mode). Runs even
    /// when the install flow fails.
    pub fn on_exit(mut self, f: impl FnOnce(&mut Installer) -> Result<()> + 'static) -> Self {
        self.config.on_exit = Some(Box::new(f));
        self
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
                is_uninstall: false,
            }));
        self
    }

    /// Add an uninstall page — identical to [`install_page`](Self::install_page)
    /// except the Next button preceding it (and visible while the page is
    /// showing) uses `ButtonLabels::uninstall` ("Uninstall" by default) in
    /// place of `ButtonLabels::install`.
    pub fn uninstall_page(
        mut self,
        callback: impl FnOnce(&mut GuiContext) -> Result<()> + Send + 'static,
    ) -> Self {
        self.config
            .pages
            .push(ConfiguredPage::new(WizardPage::Install {
                callback: Box::new(callback),
                is_uninstall: true,
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

    /// Add a custom page — a labeled column of text inputs, checkboxes,
    /// and dropdowns. Each widget is tied to an installer option by key:
    /// values are pre-filled from `installer.option_value(key)` (useful
    /// for CLI overrides), and written back to the options store on
    /// forward navigation. Validate via `.on_before_leave(...)`:
    ///
    /// ```rust,ignore
    /// .custom_page("Settings", "Configure your install:", |p| {
    ///     p.text("username", "Username:", "admin");
    ///     p.password("password", "Password:");
    ///     p.checkbox("desktop_shortcut", "Create a desktop shortcut", true);
    ///     p.dropdown(
    ///         "db_backend",
    ///         "Database:",
    ///         &[("sqlite", "SQLite"), ("postgres", "PostgreSQL")],
    ///         "sqlite",
    ///     );
    /// })
    /// .on_before_leave(|ctx| {
    ///     let i = ctx.installer();
    ///     let u: String = i.get_option("username").unwrap_or_default();
    ///     if u.is_empty() {
    ///         installrs::gui::error("Required", "Enter a username.");
    ///         return Ok(false);
    ///     }
    ///     Ok(true)
    /// })
    /// ```
    pub fn custom_page(
        mut self,
        heading: &str,
        label: &str,
        build: impl FnOnce(&mut CustomPageBuilder),
    ) -> Self {
        let mut b = CustomPageBuilder::new();
        build(&mut b);
        self.config
            .pages
            .push(ConfiguredPage::new(WizardPage::Custom {
                heading: heading.to_string(),
                label: label.to_string(),
                widgets: b.widgets,
            }));
        self
    }

    /// Add an error page shown after the install page if the install
    /// callback returns an error or the user cancels mid-install. `title`
    /// is the bold heading; `message` is a user-facing description shown
    /// above the actual error text (which is filled in at runtime).
    ///
    /// If no error page is registered, install failures surface as a
    /// native error dialog instead.
    pub fn error_page(mut self, title: &str, message: &str) -> Self {
        self.config
            .pages
            .push(ConfiguredPage::new(WizardPage::Error {
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
    pub fn run(mut self, installer: &mut Installer) -> Result<()> {
        let on_start = self.config.on_start.take();
        let on_exit = self.config.on_exit.take();

        if let Some(cb) = on_start {
            cb(installer)?;
        }

        let result = if installer.headless {
            self.run_headless(installer)
        } else {
            self.run_platform(installer)
        };

        if let Some(cb) = on_exit {
            if let Err(e) = cb(installer) {
                eprintln!("on_exit error: {e:#}");
            }
        }

        result
    }

    /// Headless runner: pulls the install callback out of the pages, invokes
    /// it on the current thread with a `GuiContext` wired to an stderr sink
    /// so status/log messages still surface.
    fn run_headless(self, installer: &mut Installer) -> Result<()> {
        use std::sync::{mpsc, Arc, Mutex};

        // Extract install callback and default install dir.
        let mut install_callback: Option<InstallCallback> = None;
        let mut default_dir = String::new();
        for configured in self.config.pages {
            match configured.page {
                WizardPage::Install { callback, .. } => install_callback = Some(callback),
                WizardPage::DirectoryPicker { default, .. } if default_dir.is_empty() => {
                    default_dir = default;
                }
                _ => {}
            }
        }

        // Grab the real installer's cancellation flag BEFORE we swap it out,
        // so the Ctrl+C handler and `check_cancelled()` inside the install
        // callback both see the same flag.
        let cancelled = installer.cancellation_flag();

        let installer_taken = std::mem::replace(installer, Installer::new(&[], &[], "none"));
        let installer_arc = Arc::new(Mutex::new(installer_taken));
        let install_dir = Arc::new(Mutex::new(default_dir));

        let (tx, rx) = mpsc::channel::<GuiMessage>();

        // Drainer thread writes status and log messages to stderr so users
        // get feedback during the headless install.
        let drainer = std::thread::spawn(move || {
            for msg in rx {
                match msg {
                    GuiMessage::SetStatus(s) => eprintln!("[*] {s}"),
                    GuiMessage::Log(m) => eprintln!("    {m}"),
                    GuiMessage::SetProgress(_) | GuiMessage::Finished(_) => {}
                }
            }
        });

        // Attach a stderr-forwarding sink so `Installer::file`/`dir`/etc.
        // progress events also surface.
        struct HeadlessSink {
            tx: mpsc::Sender<GuiMessage>,
        }
        impl ProgressSink for HeadlessSink {
            fn set_status(&self, s: &str) {
                let _ = self.tx.send(GuiMessage::SetStatus(s.to_string()));
            }
            fn set_progress(&self, _: f64) {}
            fn log(&self, m: &str) {
                let _ = self.tx.send(GuiMessage::Log(m.to_string()));
            }
        }
        {
            let mut inst = installer_arc.lock().unwrap();
            inst.set_progress_sink(Box::new(HeadlessSink { tx: tx.clone() }));
            inst.reset_progress();
        }

        let result = (|| -> Result<()> {
            if let Some(cb) = install_callback {
                let mut ctx = GuiContext::new(
                    tx.clone(),
                    installer_arc.clone(),
                    install_dir.clone(),
                    cancelled.clone(),
                );
                cb(&mut ctx)?;
            }
            Ok(())
        })();

        // If the install failed, mirror the error to the log file (if any).
        if let Err(ref e) = result {
            installer_arc.lock().unwrap().log_error(e);
        }

        // Detach sink and close the channel so the drainer exits.
        installer_arc.lock().unwrap().clear_progress_sink();
        drop(tx);
        let _ = drainer.join();

        // Restore the installer back to the caller.
        let restored = Arc::try_unwrap(installer_arc)
            .map_err(|_| anyhow::anyhow!("installer still referenced after headless run"))?
            .into_inner()
            .map_err(|e| anyhow::anyhow!("installer mutex poisoned: {e}"))?;
        *installer = restored;

        result
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
