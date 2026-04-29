//! Optional native wizard GUI and dialog helpers.
//!
//! Gated behind the `gui` feature. The backend is picked at compile time
//! via `gui-win32` (Win32 on Windows, using `winsafe`) or `gui-gtk`
//! (GTK3 on Linux, using `gtk-rs`). Both backends present the same
//! high-level API via [`InstallerGui`] and its page builders.
//!
//! Typical usage:
//!
//! ```rust,ignore
//! use installrs::gui::*;
//!
//! let mut w = InstallerGui::wizard();
//! w.title("My App Installer");
//! w.welcome("Welcome!", "Click Next to continue.");
//! w.license("License", include_str!("../LICENSE"), "I accept");
//! w.components_page("Components", "Choose features:");
//! w.directory_picker("Install Location", "Install to:", "install-dir");
//! w.install_page(|i| {
//!     i.file(installrs::source!("app.exe"), "app.exe").install()?;
//!     i.uninstaller("uninstall.exe").install()?;
//!     Ok(())
//! });
//! w.finish_page("Done!", "Click Finish to exit.");
//! w.run(i)?;
//! ```
//!
//! The same wizard definition runs headless (no window) when the user
//! passes `--headless` — [`InstallerGui::run`] checks `installer.headless`
//! and dispatches accordingly.
//!
//! See the repository's [GUI Wizard guide] for the full walkthrough —
//! custom pages, error page, native dialogs, pre-wizard language
//! selector, and headless mode.
//!
//! [GUI Wizard guide]: https://github.com/merlinz01/InstallRS/blob/main/docs/gui-wizard.md

pub mod dialog;
mod types;

#[cfg(feature = "gui-win32")]
mod win32;

#[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
mod gtk;

pub use dialog::{choose_language, confirm, error, info, warn};

/// Install a PNG byte-slice as the default window icon for every GTK
/// window and dialog the wizard creates. No-op on Windows (icons come
/// from the embedded `.ico` resource) and on builds without a GTK
/// backend. InstallRS's build tool emits a call to this at the top of
/// the generated Linux `main.rs` when `[package.metadata.installrs].icon`
/// points at a PNG — user code shouldn't need to call it directly.
#[doc(hidden)]
pub fn __set_window_icon_png(bytes: &'static [u8]) {
    #[cfg(all(feature = "gui-gtk", not(feature = "gui-win32")))]
    {
        gtk::set_icon_bytes(bytes);
    }
    #[cfg(not(all(feature = "gui-gtk", not(feature = "gui-win32"))))]
    {
        let _ = bytes;
    }
}
pub use types::{
    ButtonLabels, ConfiguredPage, CustomPageBuilder, CustomWidget, ExitCallback, GuiMessage,
    InstallCallback, OnBeforeLeaveCallback, OnEnterCallback, StartCallback, WizardConfig,
    WizardPage,
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
/// let mut w = InstallerGui::wizard();
/// w.title("My App Installer");
/// w.welcome("Welcome!", "Click Next to continue.");
/// w.license(include_str!("../LICENSE"));
/// w.directory_picker("Install Location", "Install to:", "install-dir");
/// w.install_page(|i| {
///     i.set_status("Installing...");
///     // ... install files ...
///     i.set_progress(1.0);
///     i.set_status("Done!");
///     Ok(())
/// });
/// w.finish_page("Complete!", "Click Finish to exit.");
/// w.run(i)?;
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
    pub fn on_start(
        &mut self,
        f: impl FnOnce(&mut Installer) -> Result<()> + 'static,
    ) -> &mut Self {
        self.config.on_start = Some(Box::new(f));
        self
    }

    /// Set a callback that runs at wizard exit, after the window closes (or
    /// after the install callback completes in headless mode). Runs even
    /// when the install flow fails.
    pub fn on_exit(&mut self, f: impl FnOnce(&mut Installer) -> Result<()> + 'static) -> &mut Self {
        self.config.on_exit = Some(Box::new(f));
        self
    }

    /// Set the window title.
    pub fn title(&mut self, title: impl AsRef<str>) -> &mut Self {
        self.config.title = title.as_ref().to_string();
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
    pub fn buttons(&mut self, labels: ButtonLabels) -> &mut Self {
        self.config.buttons = labels;
        self
    }

    /// Add a welcome page with a title and description message.
    pub fn welcome(&mut self, title: impl AsRef<str>, message: impl AsRef<str>) -> PageHandle<'_> {
        self.push_page(WizardPage::Welcome {
            title: title.as_ref().to_string(),
            message: message.as_ref().to_string(),
        })
    }

    /// Add a license agreement page.
    ///
    /// `heading` is the title displayed above the license text, and `accept_label`
    /// is the label on the acceptance checkbox (both translatable by the caller).
    pub fn license(
        &mut self,
        heading: impl AsRef<str>,
        text: impl AsRef<str>,
        accept_label: impl AsRef<str>,
    ) -> PageHandle<'_> {
        self.push_page(WizardPage::License {
            heading: heading.as_ref().to_string(),
            text: text.as_ref().to_string(),
            accept_label: accept_label.as_ref().to_string(),
        })
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
    pub fn components_page(
        &mut self,
        heading: impl AsRef<str>,
        label: impl AsRef<str>,
    ) -> PageHandle<'_> {
        self.push_page(WizardPage::Components {
            heading: heading.as_ref().to_string(),
            label: label.as_ref().to_string(),
        })
    }

    /// Add a directory picker page bound to a named installer option.
    ///
    /// `heading` is the bold title at the top of the page, `label` is the
    /// prompt next to the path input (e.g. "Install to:"), and `key` is the
    /// name of the [`crate::OptionValue::String`] option the picker reads
    /// and writes. Initial display value is the option's current value (or
    /// empty if unset) — seed it via `installer.set_option(key, ...)` (or
    /// the helper `set_option_default`) before [`run`](Self::run) for a
    /// sensible first-run default. The option is auto-registered as
    /// [`crate::OptionKind::String`] at `run()` if the user hasn't already
    /// registered it; register it explicitly before
    /// [`Installer::process_commandline`] if you want a `--<key>` CLI flag.
    ///
    /// User code is responsible for lifting the picked value into
    /// `installer.set_out_dir(...)` if relative-path resolution should
    /// honour it — typically inside the install callback:
    ///
    /// ```rust,ignore
    /// w.install_page(|i| {
    ///     i.set_out_dir(i.get_option::<String>("install-dir").unwrap_or_default());
    ///     // ...
    /// });
    /// ```
    pub fn directory_picker(
        &mut self,
        heading: impl AsRef<str>,
        label: impl AsRef<str>,
        key: impl AsRef<str>,
    ) -> PageHandle<'_> {
        self.push_page(WizardPage::DirectoryPicker {
            heading: heading.as_ref().to_string(),
            label: label.as_ref().to_string(),
            key: key.as_ref().to_string(),
        })
    }

    /// Add the install page with a callback that performs the actual installation.
    ///
    /// The callback receives `&mut Installer` directly. A progress sink
    /// that forwards to the install page is attached for the duration of
    /// the callback. The wizard does not implicitly call
    /// [`Installer::set_out_dir`] from a directory_picker page — lift the
    /// option value yourself if you want relative-path resolution.
    pub fn install_page(
        &mut self,
        callback: impl FnOnce(&mut Installer) -> Result<()> + Send + 'static,
    ) -> PageHandle<'_> {
        self.push_page(WizardPage::Install {
            callback: Box::new(callback),
            is_uninstall: false,
        })
    }

    /// Add an uninstall page — identical to [`install_page`](Self::install_page)
    /// except the Next button preceding it (and visible while the page is
    /// showing) uses `ButtonLabels::uninstall` ("Uninstall" by default) in
    /// place of `ButtonLabels::install`.
    pub fn uninstall_page(
        &mut self,
        callback: impl FnOnce(&mut Installer) -> Result<()> + Send + 'static,
    ) -> PageHandle<'_> {
        self.push_page(WizardPage::Install {
            callback: Box::new(callback),
            is_uninstall: true,
        })
    }

    /// Add a finish page shown after installation completes.
    pub fn finish_page(
        &mut self,
        title: impl AsRef<str>,
        message: impl AsRef<str>,
    ) -> PageHandle<'_> {
        self.push_page(WizardPage::Finish {
            title: title.as_ref().to_string(),
            message: message.as_ref().to_string(),
        })
    }

    /// Add a custom page — a labeled column of text inputs, checkboxes,
    /// and dropdowns. Each widget is tied to an installer option by key:
    /// values are pre-filled from `installer.option_value(key)` (useful
    /// for CLI overrides), and written back to the options store on
    /// forward navigation. Validate via `.on_before_leave(...)` on the
    /// returned page handle:
    ///
    /// ```rust,ignore
    /// w.custom_page("Settings", "Configure your install:", |p| {
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
    /// .on_before_leave(|i| {
    ///     let u: String = i.get_option("username").unwrap_or_default();
    ///     if u.is_empty() {
    ///         installrs::gui::error("Required", "Enter a username.");
    ///         return Ok(false);
    ///     }
    ///     Ok(true)
    /// });
    /// ```
    pub fn custom_page(
        &mut self,
        heading: impl AsRef<str>,
        label: impl AsRef<str>,
        build: impl FnOnce(&mut CustomPageBuilder),
    ) -> PageHandle<'_> {
        let mut b = CustomPageBuilder::new();
        build(&mut b);
        self.push_page(WizardPage::Custom {
            heading: heading.as_ref().to_string(),
            label: label.as_ref().to_string(),
            widgets: b.widgets,
        })
    }

    /// Add an error page shown after the install page if the install
    /// callback returns an error or the user cancels mid-install. `title`
    /// is the bold heading; `message` is a user-facing description shown
    /// above the actual error text (which is filled in at runtime).
    ///
    /// If no error page is registered, install failures surface as a
    /// native error dialog instead.
    pub fn error_page(
        &mut self,
        title: impl AsRef<str>,
        message: impl AsRef<str>,
    ) -> PageHandle<'_> {
        self.push_page(WizardPage::Error {
            title: title.as_ref().to_string(),
            message: message.as_ref().to_string(),
        })
    }

    fn push_page(&mut self, page: WizardPage) -> PageHandle<'_> {
        self.config.pages.push(ConfiguredPage::new(page));
        PageHandle {
            page: self.config.pages.last_mut().unwrap(),
        }
    }

    /// Run the wizard GUI. This blocks until the user closes the window.
    ///
    /// On Windows with the `gui-win32` feature, this creates a native Win32 wizard.
    /// Falls back to an error on unsupported platforms.
    pub fn run(mut self, installer: &mut Installer) -> Result<()> {
        let on_start = self.config.on_start.take();
        let on_exit = self.config.on_exit.take();

        // Auto-register any directory_picker option keys not already
        // registered, so the picker's read/write via set_option works
        // without forcing the user to call `i.option(key, ...)`. Users
        // who want a `--<key>` CLI flag should still register the option
        // explicitly before `process_commandline()`.
        for configured in &self.config.pages {
            if let WizardPage::DirectoryPicker { key, .. } = &configured.page {
                if !installer.is_option_registered(key) {
                    installer.option(key.clone(), crate::OptionKind::String, "");
                }
            }
        }

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

    /// Headless runner: pulls the install callback out of the pages and
    /// invokes it on the current thread. A default [`crate::StderrProgressSink`]
    /// is attached if the caller didn't already set one, so status / log /
    /// progress events surface readably on stderr.
    fn run_headless(self, installer: &mut Installer) -> Result<()> {
        // Extract install callback. Directory_picker pages are no-ops in
        // headless mode — user code reads / seeds the relevant option
        // directly and lifts it into out_dir as needed.
        let mut install_callback: Option<InstallCallback> = None;
        for configured in self.config.pages {
            if let WizardPage::Install { callback, .. } = configured.page {
                install_callback = Some(callback);
            }
        }

        // Attach a default stderr sink if none is already set, so headless
        // installs get readable feedback without extra setup.
        if !installer.has_progress_sink() {
            installer.set_progress_sink(Box::new(crate::StderrProgressSink::new()));
        }
        installer.reset_progress();

        let result = if let Some(cb) = install_callback {
            cb(installer)
        } else {
            Ok(())
        };

        if let Err(ref e) = result {
            installer.log_error(e);
        }

        // Detach the progress sink so its Drop runs now (the StderrProgressSink
        // finalizes its in-place progress line with a trailing newline) before
        // any subsequent `on_exit` stderr output prints.
        installer.clear_progress_sink();

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

/// Handle to the most recently added wizard page, returned by every
/// page-adding method on [`InstallerGui`]. Scope for attaching
/// page-specific callbacks — `on_enter`, `on_before_leave`, `skip_if`.
///
/// The handle borrows the wizard mutably, so it must be consumed (or
/// dropped) before the next page-adding call. In practice that means
/// one page per statement — which reads better than a 30-line chain
/// anyway.
pub struct PageHandle<'a> {
    page: &'a mut ConfiguredPage,
}

impl<'a> PageHandle<'a> {
    /// Attach an `on_enter` callback. Runs on the GUI thread after the
    /// page becomes visible, on forward navigation only.
    pub fn on_enter<F>(self, f: F) -> Self
    where
        F: Fn(&mut Installer) -> Result<()> + 'static,
    {
        self.page.on_enter = Some(Box::new(f));
        self
    }

    /// Attach an `on_before_leave` callback. Returning `Ok(false)`
    /// cancels navigation and keeps the page visible; `Err(_)` also
    /// cancels. Runs on forward navigation only.
    pub fn on_before_leave<F>(self, f: F) -> Self
    where
        F: Fn(&mut Installer) -> Result<bool> + 'static,
    {
        self.page.on_before_leave = Some(Box::new(f));
        self
    }

    /// Attach a `skip_if` predicate. Evaluated every time the wizard
    /// navigates past the page; returning `true` hides it. Must be pure
    /// — side effects belong in `on_enter`. Both Next and Back respect
    /// the skip, so a hidden page is also skipped on backward nav.
    ///
    /// ```rust,ignore
    /// w.license("License", include_str!("../LICENSE"), "I accept")
    ///     .skip_if(|i| i.get_option::<bool>("accept-license").unwrap_or(false));
    ///
    /// w.directory_picker("Install Location", "Install to:", default_dir)
    ///     .skip_if(|i| i.get_option::<String>("install-dir").is_some());
    /// ```
    pub fn skip_if<F>(self, f: F) -> Self
    where
        F: Fn(&Installer) -> bool + 'static,
    {
        self.page.skip_if = Some(Box::new(f));
        self
    }
}
