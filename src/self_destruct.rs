pub fn prepare() {}

pub fn destruct() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe);
    }
}
