use std::path::PathBuf;
use std::process::Command;

fn installrs_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_installrs"))
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple")
}

/// Copy the fixture to a temp directory so each test gets its own build dir.
/// Rewrites the installrs dependency path to be absolute since the relative
/// path won't work from the temp location.
fn copy_fixture(tmp: &std::path::Path) -> PathBuf {
    let dest = tmp.join("fixture");
    copy_dir_recursive(&fixture_dir(), &dest);

    // Fix installrs path dep: replace relative path with absolute
    let cargo_toml_path = dest.join("Cargo.toml");
    let installrs_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&cargo_toml_path).unwrap();
    let content = content.replace(
        "path = \"../../..\"",
        &format!("path = {:?}", installrs_root.to_string_lossy()),
    );
    std::fs::write(&cargo_toml_path, content).unwrap();

    dest
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            if entry.file_name() == "build" || entry.file_name() == "target" {
                continue; // skip build artifacts
            }
            copy_dir_recursive(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Build an installer with the given compression, run it, verify installed
/// files, run the uninstaller, verify cleanup.
fn build_install_uninstall(compression: &str) {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let target_dir = copy_fixture(tmp.path());
    let installer_bin = tmp.path().join("installer");
    let out_dir = tmp.path().join("installed");

    // ── Step 1: build the installer ──────────────────────────────────────────
    let installrs_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(installrs_bin())
        .args([
            "--target",
            &target_dir.to_string_lossy(),
            "--output",
            &installer_bin.to_string_lossy(),
            "--compression",
            compression,
            // Integration tests exercise the current working-tree runtime, not
            // a published crate. `--installrs-path` makes the builder emit
            // `path = ".../InstallRS"` in the generated Cargo.toml.
            "--installrs-path",
            &installrs_root.to_string_lossy(),
            "--silent",
        ])
        .output()
        .expect("failed to spawn installrs");

    assert!(
        output.status.success(),
        "installrs build failed (compression={compression}):\nstdout: {}\nstderr: {}",
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
        "installer failed (compression={compression}):\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // ── Step 3: verify installed files ────────────────────────────────────────
    let installed_data = out_dir.join("data.txt");
    assert!(
        installed_data.exists(),
        "data.txt was not installed (compression={compression})"
    );
    assert!(
        std::fs::read_to_string(&installed_data)
            .unwrap()
            .contains("Hello from the simple test fixture!"),
        "installed data.txt has unexpected content (compression={compression})"
    );

    let uninstaller_path = out_dir.join("uninstall");
    assert!(
        uninstaller_path.exists(),
        "uninstaller was not installed (compression={compression})"
    );

    // ── Step 4: run the uninstaller ───────────────────────────────────────────
    let output = Command::new(&uninstaller_path)
        .arg("--headless")
        .env("INSTALLRS_TEST_OUT", &out_dir)
        .output()
        .expect("failed to spawn uninstaller");

    assert!(
        output.status.success(),
        "uninstaller failed (compression={compression}):\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // ── Step 5: verify uninstallation ────────────────────────────────────────
    assert!(
        !installed_data.exists(),
        "data.txt was not removed by the uninstaller (compression={compression})"
    );
    assert!(
        !uninstaller_path.exists(),
        "uninstaller was not removed (compression={compression})"
    );
}

#[test]
fn integration_compression_none() {
    build_install_uninstall("none");
}

#[test]
fn integration_compression_gzip() {
    build_install_uninstall("gzip");
}

#[test]
fn integration_compression_lzma() {
    build_install_uninstall("lzma");
}

#[test]
fn integration_compression_bzip2() {
    build_install_uninstall("bzip2");
}
