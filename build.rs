//! Embeds a Windows application manifest into the `installrs` CLI binary.
//!
//! The binary is named `installrs`, which contains the substring "install".
//! Windows' legacy installer-detection heuristic flags such executables and
//! auto-elevates them via UAC unless they ship a manifest declaring an explicit
//! `requestedExecutionLevel`. `installrs.manifest` declares `asInvoker`, so the
//! build tool runs with the caller's privileges instead of prompting for
//! elevation on every invocation. No-op on non-Windows targets.

fn main() {
    println!("cargo:rerun-if-changed=installrs.manifest");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_manifest_file("installrs.manifest");
        res.compile()
            .expect("failed to embed Windows manifest into the installrs binary");
    }
}
