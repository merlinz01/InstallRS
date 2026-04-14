use anyhow::Result;
use installrs::Installer;

pub fn install(i: &mut Installer) -> Result<()> {
    let out = std::env::var("INSTALLRS_TEST_OUT")
        .expect("INSTALLRS_TEST_OUT must be set before running the test installer");
    i.set_out_dir(&out);
    installrs::file!(i, "data.txt", "data.txt")?;
    i.uninstaller("uninstall")?;
    Ok(())
}

pub fn uninstall(i: &mut Installer) -> Result<()> {
    let out = std::env::var("INSTALLRS_TEST_OUT")
        .expect("INSTALLRS_TEST_OUT must be set before running the test uninstaller");
    i.set_out_dir(&out);
    i.remove("data.txt")?;
    Ok(())
}
