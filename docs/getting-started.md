<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# Getting Started with InstallRS

This guide walks you through building your first self-contained installer
with InstallRS — from zero to a working executable that copies files,
creates directories, and ships an uninstaller.

## 1. Install the CLI

```sh
cargo install installrs --locked
```

This downloads the latest `installrs` binary to `~/.cargo/bin/installrs`.
Verify it works:

```sh
installrs --help
```

On Linux, if you want to use the GUI wizard, you'll also need GTK3 dev
headers at build time:

```sh
sudo apt-get install -y libgtk-3-dev      # Debian/Ubuntu
sudo dnf install -y gtk3-devel            # Fedora/RHEL
```

Windows has no extra system dependency.

## 2. Create your installer crate

Make a new library crate — **not** a binary. InstallRS generates the
binary for you from your library's `install` and `uninstall` functions.

```sh
cargo new --lib my-installer
cd my-installer
```

Edit `Cargo.toml` to depend on `installrs`:

```toml
[package]
name = "my-installer"
version = "0.1.0"
edition = "2021"

[dependencies]
installrs = "0.1"
anyhow = "1"
```

## 3. Write `install` and `uninstall` functions

Replace the generated `src/lib.rs` with something like this:

```rust
use anyhow::Result;
use installrs::{source, Installer};

pub fn install(i: &mut Installer) -> Result<()> {
    // Parse CLI args (--headless, --components, --log, etc.). Required.
    i.process_commandline()?;

    i.set_out_dir("/opt/my-app");
    i.file(source!("app"), "app").mode(0o755).install()?;
    i.file(source!("config.toml"), "etc/config.toml").install()?;
    i.dir(source!("assets"), "assets").install()?;
    i.uninstaller("uninstall").install()?;

    Ok(())
}

pub fn uninstall(i: &mut Installer) -> Result<()> {
    i.process_commandline()?;
    i.remove("/opt/my-app").install()?;
    Ok(())
}
```

Every `source!("path")` references a file or directory the build tool
should embed. Paths are relative to your crate's root. Put the files you
want to ship alongside your `Cargo.toml`:

```text
my-installer/
├── Cargo.toml
├── src/
│   └── lib.rs
├── app                 # your application binary
├── config.toml
└── assets/
    ├── icon.png
    └── logo.svg
```

## 4. Build the installer

```sh
installrs --target . --output my-installer
```

That's it. You now have a single executable `my-installer` (or
`my-installer.exe` if you pass `--target-triple x86_64-pc-windows-gnu`)
containing all the embedded files, the install logic, and a matching
uninstaller binary.

Pass `-v` for debug output or `-vv` for trace-level build logs if you
want to see what's happening.

## 5. Run the installer

```sh
./my-installer
```

By default, the installer runs in console mode, prints status to stderr,
and installs to the path you set with `set_out_dir`.

**Headless mode** (no GUI, for CI / unattended installs):

```sh
./my-installer --headless
```

**Log to a file:**

```sh
./my-installer --log install.log
```

After install, the uninstaller lives at `/opt/my-app/uninstall` — run it
to remove everything.

## 6. Add a GUI wizard

For a native wizard with welcome screen, license agreement, component
selection, directory picker, progress bar, and finish page, add this to
your installer crate's `Cargo.toml`:

```toml
[package.metadata.installrs]
gui = true
```

Then update `install()` to build a wizard:

```rust
use installrs::gui::*;

pub fn install(i: &mut Installer) -> Result<()> {
    i.add_component("core", "Core files", "Required files", 5).required();
    i.add_component("docs", "Documentation", "User guide + examples", 2);

    i.process_commandline()?;

    let mut w = InstallerGui::new("My App Installer");
    w.welcome("Welcome", "This wizard will install My App.");
    w.license("License", include_str!("../LICENSE.txt"), "I accept");
    w.components_page("Components", "Select features:");
    i.set_option_if_unset("install-dir", "/opt/my-app");
    w.directory_picker("Install Location", "Install to:", "install-dir");
    w.install_page(|i| {
        i.set_out_dir(i.option::<String>("install-dir").unwrap_or_default());
        i.file(source!("app"), "app").mode(0o755).install()?;
        if i.is_component_selected("docs") {
            i.dir(source!("docs"), "docs").install()?;
        }
        i.uninstaller("uninstall").install()?;
        Ok(())
    });
    w.finish_page("Done!", "Click Finish to exit.");
    w.run(i)?;

    Ok(())
}
```

Rebuild and run — you'll get a native Win32 wizard on Windows, a GTK3
wizard on Linux. The same code path also handles `--headless` mode
automatically by running the `install_page` callback inline without a
window.

## 7. Polish — icons, version info, manifest

Add Windows resources (icon, VERSIONINFO, UAC manifest) via
`[package.metadata.installrs]`:

```toml
[package.metadata.installrs]
gui = true
icon = "assets/icon.png"                    # PNG auto-converts to ICO
execution-level = "requireAdministrator"    # UAC elevation
product-name = "My App"
file-version = "1.0.0.0"
legal-copyright = "Copyright (c) 2026 Me"
```

See [Windows Resources](windows-resources.md) for every option.

## What next?

- **Full API reference:** [docs.rs/installrs](https://docs.rs/installrs)
- **Working example:** the [`example/`](../example) directory in the
  repo — it demonstrates components, custom pages, translation,
  cancellation, and the headless `--yes` flag.
- **Component system:** see the [Installer API](installer-api.md#components)
  doc for details on `--with` / `--without` / `--components` flags.
- **Custom wizard pages:** text inputs, checkboxes, dropdowns — see the
  [GUI Wizard custom pages section](gui-wizard.md#custom-pages).

## Troubleshooting

**`installrs` version mismatch error when building:**
Your installer crate's `Cargo.toml` specifies an `installrs` version
range that doesn't include the version of the CLI you're running. Either
update the version requirement in your `Cargo.toml`, or install the
matching CLI with `cargo install installrs@<version>`.

**Linux installer doesn't show a GUI:**
Check that `libgtk-3-0` is installed on the target system (runtime, not
just build-time). It's present on virtually every desktop distro by
default but may be absent on minimal server installs.

**"destination already exists" error on install:**
Use `.overwrite(OverwriteMode::Overwrite)` (or `Skip` / `Backup`) on the
`.file()` / `.dir()` builder. Default is `Overwrite`, but you may have
set something stricter.

**Build is slow:**
The generated installer crate uses `lto = true` + `codegen-units = 1` +
`opt-level = "z"` for minimal binary size — this is slow. For faster
iteration during development, override via env vars:

```sh
CARGO_PROFILE_RELEASE_LTO=false \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
CARGO_PROFILE_RELEASE_OPT_LEVEL=1 \
installrs --target .
```

## See also

- [Embedded files, builder ops, and progress](embedded-files.md) — the
  full API reference for everything you call inside `install` /
  `uninstall`.
- [GUI Wizard](gui-wizard.md) — beyond the quick snippet in §6, covers
  custom pages, error page, dialogs, and headless mode.
- [Installer API](installer-api.md) — selectable
  components plus registering your own `--flags`.
- [Building for production](building.md) — cross-compilation, size and
  speed tuning, code signing, release CI.
