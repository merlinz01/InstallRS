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
//! let mut w = InstallerGui::new("My App Installer");
//! w.welcome("Welcome!", "Click Next to continue.");
//! w.license("License", include_str!("../LICENSE"), "I accept");
//! w.components_page("Components", "Choose features:");
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

mod dialog;
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
    ButtonLabels, ConfiguredPage, CustomPageBuilder, CustomWidget, GuiMessage, InstallCallback,
    OnBeforeLeaveCallback, OnEnterCallback, WizardConfig, WizardPage,
};

use anyhow::Result;

use crate::Installer;

/// Builder for a wizard-style installer GUI.
///
/// Wizard configuration is statement-style — every method takes
/// `&mut self` and you call them on their own line. Page-adding methods
/// ([`welcome`](Self::welcome), [`license`](Self::license),
/// [`custom_page`](Self::custom_page), etc.) return a [`PageHandle`]
/// scoped to the page just added, so you can chain
/// [`on_enter`](PageHandle::on_enter),
/// [`on_before_leave`](PageHandle::on_before_leave), and
/// [`skip_if`](PageHandle::skip_if) on that handle.
///
/// # Example
///
/// ```rust,ignore
/// use installrs::gui::*;
///
/// let mut w = InstallerGui::new("My App Installer");
/// w.welcome("Welcome!", "Click Next to continue.");
/// w.license("License", include_str!("../LICENSE"), "I accept")
///     .skip_if(|i| i.option::<bool>("accept-license").unwrap_or(false));
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
    /// Create a new wizard builder. `title` is the window title shown by
    /// the OS — typically your app name plus "Installer".
    pub fn new(title: impl AsRef<str>) -> Self {
        Self {
            config: WizardConfig {
                title: title.as_ref().to_string(),
                pages: Vec::new(),
                buttons: ButtonLabels::default(),
            },
        }
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
    pub fn buttons(&mut self, labels: ButtonLabels) {
        self.config.buttons = labels;
    }

    /// Add a welcome page with a title and description message.
    pub fn welcome(&mut self, title: impl AsRef<str>, message: impl AsRef<str>) -> PageHandle<'_> {
        self.push_page(WizardPage::Welcome {
            title: title.as_ref().to_string(),
            message: message.as_ref().to_string(),
            widgets: Vec::new(),
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
    /// [`Installer::add_component`](crate::Installer::add_component) before calling
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
            show_log: true,
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
            show_log: true,
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
            widgets: Vec::new(),
        })
    }

    /// Add a custom page — a labeled column of text inputs, checkboxes,
    /// and dropdowns. Each widget is tied to an installer option by key:
    /// values are pre-filled from the option store (useful for CLI
    /// overrides), and written back on forward navigation. Validate via
    /// `.on_before_leave(...)` on the returned page handle:
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
    ///     let u: String = i.option("username").unwrap_or_default();
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
    pub fn run(self, installer: &mut Installer) -> Result<()> {
        if installer.headless {
            self.run_headless(installer)
        } else {
            self.run_platform(installer)
        }
    }

    /// Headless runner: pulls the install callback out of the pages and
    /// invokes it on the current thread. A default [`crate::StderrProgressSink`]
    /// is attached if the caller didn't already set one, so status / log /
    /// progress events surface readably on stderr.
    fn run_headless(self, installer: &mut Installer) -> Result<()> {
        // Extract install callback. Non-install pages are no-ops in
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
        // finalizes its in-place progress line with a trailing newline).
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
    ///     .skip_if(|i| i.option::<bool>("accept-license").unwrap_or(false));
    /// ```
    pub fn skip_if<F>(self, f: F) -> Self
    where
        F: Fn(&Installer) -> bool + 'static,
    {
        self.page.skip_if = Some(Box::new(f));
        self
    }

    /// Hide the rolling log textbox on an install / uninstall page so
    /// only the status label and progress bar are visible. Useful when
    /// the install steps' status messages alone are sufficient feedback
    /// and the per-line log would be noisy.
    ///
    /// Panics if called on a page kind other than install / uninstall.
    pub fn hide_log(self) -> Self {
        match &mut self.page.page {
            WizardPage::Install { show_log, .. } => {
                *show_log = false;
            }
            _ => panic!("hide_log is only supported on install / uninstall pages"),
        }
        self
    }

    /// Append a column of input widgets below the page's main content.
    /// Supported on welcome and finish pages; calling on any other page
    /// kind panics. Widget keys bind to installer options exactly like
    /// [`custom_page`](InstallerGui::custom_page) — values pre-fill from
    /// the options store on entry and write back on forward navigation.
    ///
    /// ```rust,ignore
    /// w.finish_page("All done!", "Click Finish to exit.")
    ///     .with_widgets(|p| {
    ///         p.checkbox("launch_app", "Launch My App now", true);
    ///         p.checkbox("show_readme", "Show the README", false);
    ///     });
    /// ```
    pub fn with_widgets(self, build: impl FnOnce(&mut CustomPageBuilder)) -> Self {
        let mut b = CustomPageBuilder::new();
        build(&mut b);
        match &mut self.page.page {
            WizardPage::Welcome { widgets, .. } | WizardPage::Finish { widgets, .. } => {
                widgets.extend(b.widgets);
            }
            _ => panic!(
                "with_widgets is only supported on welcome and finish pages; \
                 use custom_page for other widget layouts"
            ),
        }
        self
    }
}
