use anyhow::Result;
use installrs::Installer;

pub fn install(i: &mut Installer) -> Result<()> {
    i.set_in_dir(".");           // calls are detected by the build tool
    i.set_out_dir("C:/installer1_test");
    i.dir("testdir", "testdir")?;
    i.file("test.txt", "testfile.txt")?;
    i.uninstaller("uninstall.exe")?;
    Ok(())
}

pub fn uninstall(i: &mut Installer) -> Result<()> {
    i.remove("C:/installer1_test")?;
    Ok(())
}
