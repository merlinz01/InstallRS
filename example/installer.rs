use anyhow::Result;
use installrs::Installer;
use rust_i18n::t;

// Load translations from any .yml files in this directory, with English as the fallback language.
rust_i18n::i18n!(".", fallback = "en");

/// Detect and apply the system locale, falling back to English.
fn init_locale() {
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
    // Use just the language prefix (e.g. "de-DE" → "de").
    let lang = locale.split('-').next().unwrap_or("en");
    rust_i18n::set_locale(lang);
}

/// Translated wizard button labels.
fn button_labels() -> installrs::gui::ButtonLabels {
    installrs::gui::ButtonLabels {
        back: t!("wizard.back").into(),
        next: t!("wizard.next").into(),
        install: t!("wizard.install").into(),
        finish: t!("wizard.finish").into(),
        cancel: t!("wizard.cancel").into(),
    }
}

/// Default installation directory, e.g. "C:\Program Files\MyApp" on Windows or "/opt/myapp" on Unix.
fn default_install_dir() -> &'static str {
    #[cfg(windows)]
    {
        "C:\\Program Files\\MyApp"
    }
    #[cfg(not(windows))]
    {
        "/opt/myapp"
    }
}

pub fn install(i: &mut Installer) -> Result<()> {
    use installrs::gui::*;

    init_locale();

    // Register selectable components. "core" is required; "docs" is on by
    // default; "extras" is off by default.
    i.component("core", t!("installer.components.core"))
        .description(t!("installer.components.core_desc"))
        .required(true);
    i.component("docs", t!("installer.components.docs"))
        .description(t!("installer.components.docs_desc"));
    i.component("extras", t!("installer.components.extras"))
        .description(t!("installer.components.extras_desc"))
        .default(false);

    // Parse CLI (--headless, --list-components, --components, etc.).
    i.process_commandline()?;

    InstallerGui::wizard()
        .title(&t!("installer.title"))
        .buttons(button_labels())
        .welcome(
            &t!("installer.welcome.title"),
            &t!("installer.welcome.message"),
        )
        .license(
            &t!("installer.license.heading"),
            include_str!("../LICENSE.txt"),
            &t!("installer.license.accept"),
        )
        .components_page(
            &t!("installer.components.heading"),
            &t!("installer.components.label"),
        )
        .directory_picker(
            &t!("installer.directory.heading"),
            &t!("installer.directory.label"),
            default_install_dir(),
        )
        .on_before_leave(|ctx| {
            installrs::gui::confirm(
                &t!("installer.confirm.title"),
                &t!("installer.confirm.message", dir = ctx.install_dir()),
            )
        })
        .install_page(|ctx| {
            let mut i = ctx.installer();

            let out_dir = ctx.install_dir();
            i.set_out_dir(&out_dir);

            // core: always installed (required component)
            i.dir(installrs::source!("testdir"), "testdir")
                .status(t!("installer.install.status_installing"))
                .log(t!("installer.install.log_testdir"))
                .install()?;

            if i.is_component_selected("docs") {
                i.file(installrs::source!("test.txt"), "docs/readme.txt")
                    .log(t!("installer.install.log_docs"))
                    .install()?;
            }

            if i.is_component_selected("extras") {
                i.file(installrs::source!("test.txt"), "testfile.txt")
                    .log(t!("installer.install.log_testfile"))
                    .install()?;
            }

            #[cfg(windows)]
            i.uninstaller("uninstall.exe")
                .log(t!("installer.install.log_uninstaller"))
                .install()?;
            #[cfg(not(windows))]
            i.uninstaller("uninstall")
                .log(t!("installer.install.log_uninstaller"))
                .install()?;

            ctx.set_status(&t!("installer.install.status_complete"));
            Ok(())
        })
        .finish_page(
            &t!("installer.finish.title"),
            &t!("installer.finish.message"),
        )
        .run(i)?;

    Ok(())
}

pub fn uninstall(i: &mut Installer) -> Result<()> {
    use installrs::gui::*;

    init_locale();

    i.process_commandline()?;

    let install_dir = std::env::current_exe()?
        .parent()
        .expect("Executable must be in a directory")
        .to_str()
        .expect("Directory path must be valid UTF-8")
        .to_string();

    #[cfg(windows)]
    i.enable_self_delete();

    InstallerGui::wizard()
        .title(&t!("uninstaller.title"))
        .buttons(button_labels())
        .welcome(
            &t!("uninstaller.welcome.title"),
            &t!("uninstaller.welcome.message"),
        )
        .install_page(|ctx| {
            let mut i = ctx.installer();

            i.remove(install_dir)
                .status(t!("uninstaller.status_removing"))
                .log(t!("uninstaller.log_removing"))
                .install()?;

            ctx.set_status(&t!("uninstaller.status_complete"));
            Ok(())
        })
        .finish_page(
            &t!("uninstaller.finish.title"),
            &t!("uninstaller.finish.message"),
        )
        .run(i)?;

    Ok(())
}
