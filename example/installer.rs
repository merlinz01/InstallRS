use anyhow::Result;
use installrs::{Installer, OptionKind};
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
        uninstall: t!("wizard.uninstall").into(),
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

    // In GUI mode, let the user pick a language before we build the wizard
    // (page strings are captured eagerly by the builder, so the locale must
    // be final by then). The dialog's own title + prompt are already
    // localized via `t!` using the detected locale set by `init_locale()`.
    if !std::env::args().any(|a| a == "--headless") {
        let choices: &[(&str, &str)] = &[("en", "English"), ("es", "Español"), ("de", "Deutsch")];
        let default = rust_i18n::locale().to_string();
        if let Some(code) = installrs::gui::choose_language(
            &t!("installer.language.title"),
            &t!("installer.language.prompt"),
            choices,
            Some(&default),
        )? {
            rust_i18n::set_locale(&code);
        }
    }

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

    // Register custom CLI options. `--yes` skips every confirmation prompt
    // (handy for CI / unattended installs); `--install-dir` overrides the
    // default install location. `--log <path>` is built-in — no need to
    // register it. All work in GUI and headless modes.
    i.option("yes", OptionKind::Flag);
    i.option("install-dir", OptionKind::String);

    // Parse CLI (--headless, --list-components, --components, --log, etc.).
    i.process_commandline()?;

    // Seed the directory picker's default from --install-dir when provided,
    // otherwise fall back to the platform default.
    let default_dir = i
        .get_option::<String>("install-dir")
        .unwrap_or_else(|| default_install_dir().to_string());

    InstallerGui::wizard()
        .title(&t!("installer.title"))
        .buttons(button_labels())
        .on_start(|i| {
            let auto_yes = i.get_option::<bool>("yes").unwrap_or(false);
            if i.headless && !auto_yes {
                eprintln!("Running headless install of {}", t!("installer.title"));
                eprint!("Proceed? [y/N] ");
                std::io::Write::flush(&mut std::io::stderr()).ok();
                let mut answer = String::new();
                std::io::stdin()
                    .read_line(&mut answer)
                    .map_err(|e| anyhow::anyhow!("failed to read stdin: {e}"))?;
                if !matches!(answer.trim(), "y" | "Y" | "yes" | "YES") {
                    return Err(anyhow::anyhow!("install cancelled by user"));
                }
            } else if i.headless && auto_yes {
                eprintln!(
                    "Running headless install of {} (auto-confirmed)",
                    t!("installer.title")
                );
            }
            Ok(())
        })
        .on_exit(|i| {
            if i.headless {
                eprintln!("Headless install complete.");
            }
            Ok(())
        })
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
            &default_dir,
        )
        .on_before_leave(|ctx| {
            // --yes skips the confirmation dialog.
            if ctx.installer().get_option::<bool>("yes").unwrap_or(false) {
                return Ok(true);
            }
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

            // Simulate a long-running step to demonstrate the progress bar and cancellation.
            for step in 1..=5 {
                ctx.set_status(&t!("installer.install.status_step", step = step));
                std::thread::sleep(std::time::Duration::from_secs(1));
                i.check_cancelled()?;
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
        .error_page(
            &t!("installer.error.title"),
            &t!("installer.error.message"),
        )
        .run(i)?;

    Ok(())
}

pub fn uninstall(i: &mut Installer) -> Result<()> {
    use installrs::gui::*;

    init_locale();

    i.option("yes", OptionKind::Flag);
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
        .on_start(|i| {
            let auto_yes = i.get_option::<bool>("yes").unwrap_or(false);
            if i.headless && !auto_yes {
                eprint!("Really uninstall? [y/N] ");
                std::io::Write::flush(&mut std::io::stderr()).ok();
                let mut answer = String::new();
                std::io::stdin()
                    .read_line(&mut answer)
                    .map_err(|e| anyhow::anyhow!("failed to read stdin: {e}"))?;
                if !matches!(answer.trim(), "y" | "Y" | "yes" | "YES") {
                    return Err(anyhow::anyhow!("uninstall cancelled by user"));
                }
            }
            Ok(())
        })
        .welcome(
            &t!("uninstaller.welcome.title"),
            &t!("uninstaller.welcome.message"),
        )
        .uninstall_page(|ctx| {
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
        .error_page(
            &t!("uninstaller.error.title"),
            &t!("uninstaller.error.message"),
        )
        .run(i)?;

    Ok(())
}
