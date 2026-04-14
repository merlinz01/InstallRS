use anyhow::Result;
use installrs::Installer;

pub fn install(i: &mut Installer) -> Result<()> {
    i.set_out_dir("C:/installer1_test");
    installrs::dir!(i, "testdir", "testdir")?;
    installrs::file!(i, "test.txt", "testfile.txt")?;
    i.uninstaller("uninstall.exe")?;
    Ok(())
}

pub fn uninstall(i: &mut Installer) -> Result<()> {
    i.remove("C:/installer1_test")?;
    Ok(())
}
