pub fn prepare() {
    if std::env::args().any(|a| a == "--tempuninstaller") {
        return;
    }

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error getting executable path: {e}");
            std::process::exit(1);
        }
    };

    let tmp_dir = std::env::temp_dir().join(format!("uninstall-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
        eprintln!("Error creating temp dir: {e}");
        std::process::exit(1);
    }

    let tmp_exe = tmp_dir.join("uninstaller.exe");
    if let Err(e) = std::fs::copy(&exe, &tmp_exe) {
        eprintln!("Error copying to temp: {e}");
        std::process::exit(1);
    }

    let mut args: Vec<String> = std::env::args().skip(1).collect();
    args.push("--tempuninstaller".to_string());

    match std::process::Command::new(&tmp_exe).args(&args).spawn() {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Error spawning temp uninstaller: {e}");
            std::process::exit(1);
        }
    }

    std::process::exit(0);
}

pub fn destruct() {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error getting executable path: {e}");
            return;
        }
    };

    let dir = match exe.parent() {
        Some(d) => d.to_string_lossy().into_owned(),
        None => return,
    };

    let _ = std::process::Command::new("powershell")
        .args([
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &format!("Start-Sleep 5; Remove-Item -Path '{}' -Recurse -Force", dir),
        ])
        .spawn();
}
