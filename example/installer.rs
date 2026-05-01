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
    // default; "extras" is off by default. Progress weights approximate the
    // number of operations each component performs during install.
    i.component(
        "core",
        t!("installer.components.core"),
        t!("installer.components.core_desc"),
        6,
    )
    .required();
    i.component(
        "docs",
        t!("installer.components.docs"),
        t!("installer.components.docs_desc"),
        1,
    );
    i.component(
        "extras",
        t!("installer.components.extras"),
        t!("installer.components.extras_desc"),
        1,
    )
    .default_off();

    // Register custom CLI options. `--yes` skips every confirmation prompt
    // (handy for CI / unattended installs); `--install-dir` overrides the
    // default install location. `--log <path>` is built-in — no need to
    // register it. All work in GUI and headless modes.
    i.add_option("yes", OptionKind::Flag, "Skip confirmation prompts");
    i.add_option(
        "install-dir",
        OptionKind::String,
        "Override the install location",
    );

    // Parse CLI (--headless, --list-components, --components, --log, etc.).
    i.process_commandline()?;

    // Seed the install-dir option to the platform default if neither
    // `--install-dir` nor user code has already set it. The directory
    // picker reads the current option value as its initial display.
    i.set_option_if_unset("install-dir", default_install_dir());

    let mut w = InstallerGui::wizard(&t!("installer.title"));
    w.buttons(installrs::gui::ButtonLabels {
        back: t!("wizard.back").into(),
        next: t!("wizard.next").into(),
        install: t!("wizard.install").into(),
        uninstall: t!("wizard.uninstall").into(),
        finish: t!("wizard.finish").into(),
        cancel: t!("wizard.cancel").into(),
    });
    w.welcome(
        &t!("installer.welcome.title"),
        &t!("installer.welcome.message"),
    );
    w.license(
        &t!("installer.license.heading"),
        include_str!("../LICENSE.txt"),
        &t!("installer.license.accept"),
    );
    w.components_page(
        &t!("installer.components.heading"),
        &t!("installer.components.label"),
    );
    w.directory_picker(
        &t!("installer.directory.heading"),
        &t!("installer.directory.label"),
        "install-dir",
    )
    .on_before_leave(|i| {
        // --yes skips the confirmation dialog.
        if i.option::<bool>("yes").unwrap_or(false) {
            return Ok(true);
        }
        let dir: String = i.option("install-dir").unwrap_or_default();
        installrs::gui::confirm(
            &t!("installer.confirm.title"),
            &t!("installer.confirm.message", dir = dir),
        )
    });
    w.custom_page(
        &t!("installer.account.heading"),
        &t!("installer.account.label"),
        |p| {
            p.text("username", &t!("installer.account.username"), "admin");
            p.password("password", &t!("installer.account.password"));
            p.number("port", &t!("installer.account.port"), 8080);
        },
    )
    .on_before_leave(|i| {
        let user: String = i.option("username").unwrap_or_default();
        if user.trim().is_empty() {
            let _ = installrs::gui::error(
                &t!("installer.account.missing_title"),
                &t!("installer.account.missing_message"),
            );
            return Ok(false);
        }
        Ok(true)
    });
    w.custom_page(
        &t!("installer.options.heading"),
        &t!("installer.options.label"),
        |p| {
            let typical = t!("installer.options.install_type_typical").to_string();
            let minimal = t!("installer.options.install_type_minimal").to_string();
            let custom = t!("installer.options.install_type_custom").to_string();
            p.radio(
                "install_type",
                &t!("installer.options.install_type"),
                &[
                    ("typical", typical.as_str()),
                    ("minimal", minimal.as_str()),
                    ("custom", custom.as_str()),
                ],
                "typical",
            );
            p.checkbox(
                "desktop_shortcut",
                &t!("installer.options.desktop_shortcut"),
                true,
            );
            p.dropdown(
                "db_backend",
                &t!("installer.options.db_backend"),
                &[("sqlite", "SQLite"), ("postgres", "PostgreSQL")],
                "sqlite",
            );
        },
    );
    // Extra info page, only shown when the user selected "custom" on
    // the install-type radio. Demonstrates `.skip_if(|ctx| bool)` —
    // evaluated each time the wizard navigates past, so the page
    // appears or hides based on the live option value.
    w.welcome(
        &t!("installer.custom_info.title"),
        &t!("installer.custom_info.message"),
    )
    .skip_if(|i| i.option::<String>("install_type").as_deref() != Some("custom"));
    w.custom_page(
        &t!("installer.paths.heading"),
        &t!("installer.paths.label"),
        |p| {
            let license_filter = t!("installer.paths.license_filter").to_string();
            let all_files_filter = t!("installer.paths.all_files_filter").to_string();
            p.file_picker(
                "license_file",
                &t!("installer.paths.license_file"),
                "",
                &[
                    (license_filter.as_str(), "*.lic;*.key"),
                    (all_files_filter.as_str(), "*.*"),
                ],
            );
            p.dir_picker("data_dir", &t!("installer.paths.data_dir"), "");
            p.multiline("notes", &t!("installer.paths.notes"), "", 3);
        },
    );
    w.install_page(|i| {
        // Lift the picker's option into the relative-path resolution slot.
        let out_dir: String = i.option("install-dir").unwrap_or_default();
        i.set_out_dir(&out_dir);

        // core: always installed (required component)
        i.dir(installrs::source!("testdir", ignore = ["*.bak"]), "testdir")
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

        // Cargo-feature-gated content. Both the `source!` and the
        // `i.file(...)` call are compiled out unless the installer was
        // built with `installrs --feature pro`.
        #[cfg(feature = "pro")]
        {
            i.file(
                installrs::source!("pro-bonus.txt", features = ["pro"]),
                "pro-bonus.txt",
            )
            .log(t!("installer.install.log_pro_bonus"))
            .install()?;
        }

        // Simulate a long-running step to demonstrate the progress bar and cancellation.
        // Opens a single weighted step (contributes 2 units to the core component's
        // budget) and interpolates progress across 5 × 200 ms sub-ticks.
        const TICKS: u32 = 5;
        i.begin_step(&t!("installer.install.status_longrunning"), 2);
        for step in 1..=TICKS {
            i.set_status(&t!("installer.install.status_step", step = step));
            std::thread::sleep(std::time::Duration::from_millis(500));
            i.set_step_progress(step as f64 / TICKS as f64);
            i.check_cancelled()?;
        }
        i.end_step();

        #[cfg(windows)]
        i.uninstaller("uninstall.exe")
            .log(t!("installer.install.log_uninstaller"))
            .install()?;
        #[cfg(not(windows))]
        i.uninstaller("uninstall")
            .log(t!("installer.install.log_uninstaller"))
            .install()?;

        // Windows polish: Start Menu shortcut + Add/Remove Programs
        // registration. The shortcut points at the uninstaller so the
        // example is clickable; a real installer would point it at the
        // installed app's main executable.
        #[cfg(windows)]
        {
            use installrs::RegistryHive::LocalMachine;

            let program_data =
                std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string());
            let start_menu =
                format!(r"{program_data}\Microsoft\Windows\Start Menu\Programs\InstallRS Example");
            let shortcut_path = format!(r"{start_menu}\InstallRS Example.lnk");
            let uninstaller_path = format!(r"{out_dir}\uninstall.exe");

            i.shortcut(&shortcut_path, &uninstaller_path)
                .description("Uninstall InstallRS Example")
                .working_dir(&out_dir)
                .log(t!("installer.install.log_shortcut"))
                .install()?;

            const UNINSTALL_KEY: &str =
                r"Software\Microsoft\Windows\CurrentVersion\Uninstall\InstallRSExample";
            let quoted_uninstaller = format!("\"{uninstaller_path}\"");

            i.registry()
                .set(
                    LocalMachine,
                    UNINSTALL_KEY,
                    "DisplayName",
                    "InstallRS Example",
                )
                .log(t!("installer.install.log_registry"))
                .install()?;
            i.registry()
                .set(LocalMachine, UNINSTALL_KEY, "DisplayVersion", "0.1.0")
                .install()?;
            i.registry()
                .set(LocalMachine, UNINSTALL_KEY, "Publisher", "InstallRS")
                .install()?;
            i.registry()
                .set(
                    LocalMachine,
                    UNINSTALL_KEY,
                    "InstallLocation",
                    out_dir.as_str(),
                )
                .install()?;
            i.registry()
                .set(
                    LocalMachine,
                    UNINSTALL_KEY,
                    "UninstallString",
                    quoted_uninstaller.as_str(),
                )
                .install()?;
            i.registry()
                .set::<u32>(LocalMachine, UNINSTALL_KEY, "NoModify", 1)
                .install()?;
            i.registry()
                .set::<u32>(LocalMachine, UNINSTALL_KEY, "NoRepair", 1)
                .install()?;
        }

        i.set_status(&t!("installer.install.status_complete"));
        Ok(())
    });
    w.finish_page(
        &t!("installer.finish.title"),
        &t!("installer.finish.message"),
    )
    .with_widgets(|p| {
        p.checkbox("launch_app", &t!("installer.finish.launch_app"), true);
    });
    w.error_page(&t!("installer.error.title"), &t!("installer.error.message"));

    let auto_yes = i.option::<bool>("yes").unwrap_or(false);
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

    let result = w.run(i);

    if i.headless {
        eprintln!("Headless install complete.");
    }
    if i.option::<bool>("launch_app").unwrap_or(false) {
        #[cfg(windows)]
        let cmd = "notepad.exe";
        #[cfg(not(windows))]
        let cmd = "xed";
        let _ = std::process::Command::new(cmd).spawn();
    }

    result
}

pub fn uninstall(i: &mut Installer) -> Result<()> {
    use installrs::gui::*;

    init_locale();

    i.add_option("yes", OptionKind::Flag, "Skip confirmation prompts");
    i.process_commandline()?;

    #[cfg(not(windows))]
    let install_dir = std::env::current_exe()?
        .parent()
        .expect("Executable must be in a directory")
        .to_str()
        .expect("Directory path must be valid UTF-8")
        .to_string();

    #[cfg(windows)]
    i.enable_self_delete();

    let mut w = InstallerGui::wizard(&t!("uninstaller.title"));
    w.buttons(installrs::gui::ButtonLabels {
        back: t!("wizard.back").into(),
        next: t!("wizard.next").into(),
        install: t!("wizard.install").into(),
        uninstall: t!("wizard.uninstall").into(),
        finish: t!("wizard.finish").into(),
        cancel: t!("wizard.cancel").into(),
    });
    let auto_yes = i.option::<bool>("yes").unwrap_or(false);
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
    w.welcome(
        &t!("uninstaller.welcome.title"),
        &t!("uninstaller.welcome.message"),
    );
    w.uninstall_page(|i| {
        // On Windows, `enable_self_delete` relaunches from a temp dir, so
        // `current_exe()` no longer points to the real install location.
        // Read it back from the registry instead (InstallLocation is what
        // Add/Remove Programs uses). On other platforms, fall back to the
        // value captured before the wizard ran.
        #[cfg(windows)]
        let install_dir: String = {
            use installrs::RegistryHive::LocalMachine;
            const UNINSTALL_KEY: &str =
                r"Software\Microsoft\Windows\CurrentVersion\Uninstall\InstallRSExample";
            i.registry()
                .get::<String>(LocalMachine, UNINSTALL_KEY, "InstallLocation")
                .unwrap_or_else(|_| {
                    std::env::current_exe()
                        .ok()
                        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                        .and_then(|p| p.to_str().map(String::from))
                        .unwrap_or_default()
                })
        };

        #[cfg(windows)]
        {
            use installrs::RegistryHive::LocalMachine;

            let program_data =
                std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string());
            let shortcut_dir =
                format!(r"{program_data}\Microsoft\Windows\Start Menu\Programs\InstallRS Example");
            i.remove(shortcut_dir).install()?;

            i.registry()
                .remove(
                    LocalMachine,
                    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\InstallRSExample",
                )
                .recursive()
                .install()?;
        }

        i.remove(install_dir)
            .status(t!("uninstaller.status_removing"))
            .log(t!("uninstaller.log_removing"))
            .install()?;

        i.set_status(&t!("uninstaller.status_complete"));
        Ok(())
    });
    w.finish_page(
        &t!("uninstaller.finish.title"),
        &t!("uninstaller.finish.message"),
    );
    w.error_page(
        &t!("uninstaller.error.title"),
        &t!("uninstaller.error.message"),
    );
    w.run(i)?;

    Ok(())
}
