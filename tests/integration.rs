use std::path::PathBuf;
use std::process::Command;

fn installrs_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_installrs"))
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/simple")
}

/// Full end-to-end test: build an installer from the simple fixture, run it,
/// verify files are installed, run the uninstaller, verify cleanup.
///
/// This test calls `cargo build` internally and may take a minute on first run.
#[test]
fn integration_build_install_uninstall() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let installer_bin = tmp.path().join("installer");
    let out_dir = tmp.path().join("installed");

    // ── Step 1: build the installer ──────────────────────────────────────────
    let output = Command::new(installrs_bin())
        .args([
            "--target",      &fixture_dir().to_string_lossy(),
            "--output",      &installer_bin.to_string_lossy(),
            "--compression", "none",
            "--silent",
        ])
        .output()
        .expect("failed to spawn installrs");

    assert!(
        output.status.success(),
        "installrs build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        installer_bin.exists(),
        "installer binary was not created at {}",
        installer_bin.display()
    );

    // ── Step 2: run the installer ─────────────────────────────────────────────
    std::fs::create_dir_all(&out_dir).unwrap();
    let output = Command::new(&installer_bin)
        .arg("--headless")
        .env("INSTALLRS_TEST_OUT", &out_dir)
        .output()
        .expect("failed to spawn installer");

    assert!(
        output.status.success(),
        "installer failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // ── Step 3: verify installed files ────────────────────────────────────────
    let installed_data = out_dir.join("data.txt");
    assert!(installed_data.exists(), "data.txt was not installed");
    assert!(
        std::fs::read_to_string(&installed_data).unwrap()
            .contains("Hello from the simple test fixture!"),
        "installed data.txt has unexpected content"
    );

    let uninstaller_path = out_dir.join("uninstall");
    assert!(uninstaller_path.exists(), "uninstaller was not installed");

    // ── Step 4: run the uninstaller ───────────────────────────────────────────
    let output = Command::new(&uninstaller_path)
        .arg("--headless")
        .env("INSTALLRS_TEST_OUT", &out_dir)
        .output()
        .expect("failed to spawn uninstaller");

    assert!(
        output.status.success(),
        "uninstaller failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // ── Step 5: verify uninstallation ────────────────────────────────────────
    assert!(
        !installed_data.exists(),
        "data.txt was not removed by the uninstaller"
    );

    // On Unix, the uninstaller binary self-deletes via self_destruct::destruct().
    #[cfg(unix)]
    assert!(
        !uninstaller_path.exists(),
        "uninstaller did not self-delete on Unix"
    );
}
