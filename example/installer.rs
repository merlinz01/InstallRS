use anyhow::Result;
use installrs::Installer;

pub fn install(i: &mut Installer) -> Result<()> {
    use installrs::gui::*;

    InstallerGui::wizard()
        .title("InstallRS Example")
        .welcome(
            "Welcome to the InstallRS Example!",
            "This installer will copy a few test files to your system.\n\nClick Next to continue.",
        )
        .license(include_str!("../LICENSE.txt"))
        .directory_picker("C:/InstallRS test")
        .install_page(|ctx| {
            ctx.set_status("Installing files...");
            ctx.set_progress(0.0);

            let out_dir = ctx.install_dir();
            ctx.installer().set_out_dir(&out_dir);

            ctx.log("Installing test directory...");
            installrs::dir!(ctx.installer(), "testdir", "testdir")?;
            ctx.set_progress(0.33);

            ctx.log("Installing test file...");
            installrs::file!(ctx.installer(), "test.txt", "testfile.txt")?;
            ctx.set_progress(0.66);

            ctx.log("Writing uninstaller...");
            ctx.installer().uninstaller("uninstall.exe")?;
            ctx.set_progress(1.0);

            ctx.set_status("Installation complete!");
            Ok(())
        })
        .finish_page(
            "Installation Complete!",
            "All files have been installed successfully.\n\nClick Finish to exit.",
        )
        .run(i)?;

    Ok(())
}

pub fn uninstall(i: &mut Installer) -> Result<()> {
    use installrs::gui::*;

    #[cfg(windows)]
    i.enable_self_delete();

    InstallerGui::wizard()
        .title("InstallRS Example — Uninstall")
        .welcome(
            "Uninstall InstallRS Example",
            "This will remove InstallRS Example from your system.\n\nClick Next to continue.",
        )
        .install_page(|ctx| {
            ctx.set_status("Removing files...");
            ctx.set_progress(0.0);

            ctx.log("Removing install directory...");
            ctx.installer().remove("C:/InstallRS test")?;
            ctx.set_progress(1.0);

            ctx.set_status("Uninstallation complete!");
            Ok(())
        })
        .finish_page(
            "Uninstallation Complete!",
            "All files have been removed successfully.\n\nClick Finish to exit.",
        )
        .run(i)?;

    Ok(())
}
