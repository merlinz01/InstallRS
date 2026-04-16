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
        .directory_picker("C:/InstallRS test")
        .install_page(|ctx| {
            ctx.set_status(&t!("installer.install.status_installing"));
            ctx.set_progress(0.0);

            let out_dir = ctx.install_dir();
            ctx.installer().set_out_dir(&out_dir);

            ctx.log(&t!("installer.install.log_testdir"));
            installrs::dir!(ctx.installer(), "testdir", "testdir")?;
            ctx.set_progress(0.33);

            ctx.log(&t!("installer.install.log_testfile"));
            installrs::file!(ctx.installer(), "test.txt", "testfile.txt")?;
            ctx.set_progress(0.66);

            ctx.log(&t!("installer.install.log_uninstaller"));
            ctx.installer().uninstaller("uninstall.exe")?;
            ctx.set_progress(1.0);

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
            ctx.set_status(&t!("uninstaller.status_removing"));
            ctx.set_progress(0.0);

            ctx.log(&t!("uninstaller.log_removing"));
            ctx.installer().remove("C:/InstallRS test")?;
            ctx.set_progress(1.0);

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
