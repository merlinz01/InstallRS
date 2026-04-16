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

pub fn install(i: &mut Installer) -> Result<()> {
    use installrs::gui::*;

    init_locale();

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
        .directory_picker(
            &t!("installer.directory.heading"),
            &t!("installer.directory.label"),
            "C:/InstallRS test",
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

            i.dir(installrs::source!("testdir"), "testdir")
                .status(t!("installer.install.status_installing"))
                .log(t!("installer.install.log_testdir"))
                .install()?;

            i.file(installrs::source!("test.txt"), "testfile.txt")
                .log(t!("installer.install.log_testfile"))
                .install()?;

            i.uninstaller("uninstall.exe")
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

            i.remove("C:/InstallRS test")
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
