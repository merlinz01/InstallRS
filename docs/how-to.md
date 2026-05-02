<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# How-to: common installer patterns

Short recipes for the things people most often want to do.

## Ask the user where to install

Use a custom page with a `dir_picker` widget bound to an option,
and read the option in the install callback.

```rust
i.add_option("install-dir", OptionKind::String, "Install location");
i.set_option_if_unset("install-dir", default_install_dir());
i.process_commandline()?;

let mut w = InstallerGui::new("My App Installer");
w.custom_page("Install Location", "", |p| {
    p.dir_picker("install-dir", "Install to:");
});
w.install_page(|i| {
    i.set_out_dir(i.option::<String>("install-dir").unwrap_or_default());
    i.file(source!("app.exe"), "app.exe").install()?;
    i.uninstaller("uninstall.exe").install()?;
    Ok(())
});
```

`--install-dir <path>` works for free because the option is registered
before `process_commandline()`.

## Detect cancellation

The Cancel button and Ctrl+C both flip a shared atomic flag. Every
builder op checks it; user code can check it too.

```rust
match w.run(i) {
    Ok(()) => {}
    Err(_) if i.is_cancelled() => {
        // user cancelled — no error dialog needed
        std::process::exit(130);
    }
    Err(e) => eprintln!("install failed: {e}"),
}
```

Inside loops doing custom work, call `i.check_cancelled()?` to bail out
cleanly:

```rust
for chunk in download_chunks()? {
    i.check_cancelled()?;
    process(chunk)?;
}
```

## Branch on component selection

```rust
i.add_component("docs", "Documentation", "User guide", 2);
i.add_component("extras", "Extra samples", "Optional examples", 1);
i.set_component_selected("extras", false);

w.install_page(|i| {
    i.file(source!("app.exe"), "app.exe").install()?;
    if i.is_component_selected("docs") {
        i.dir(source!("docs"), "docs").install()?;
    }
    if i.is_component_selected("extras") {
        i.dir(source!("extras"), "extras").install()?;
    }
    Ok(())
});
```

CLI: `--with extras --without docs`, or exact set with
`--components core,extras`.

## Skip a page based on a CLI flag

```rust
i.add_option("accept-license", OptionKind::Flag, "Auto-accept the license");
i.process_commandline()?;

w.license("License", include_str!("../LICENSE"), "I accept")
    .skip_if(|i| i.option::<bool>("accept-license").unwrap_or(false));
```

`skip_if` runs every time the wizard navigates past the page, so
predicates can depend on options or component state that change
mid-wizard.

## Confirm an action before continuing

`on_before_leave` runs on forward navigation only. Returning
`Ok(false)` keeps the user on the current page; `Err(_)` also cancels.

```rust
w.custom_page("Install Location", "", |p| {
    p.dir_picker("install-dir", "Install to:");
})
.on_before_leave(|i| {
    let dir: String = i.option("install-dir").unwrap_or_default();
    confirm("Confirm", &format!("Install to {dir}?"))
});
```

## Run a long-running step with smooth progress

```rust
w.install_page(|i| {
    i.begin_step("Downloading", 5);
    for (done, total) in download_chunks(&url)? {
        i.check_cancelled()?;
        i.set_step_progress(done as f64 / total as f64);
    }
    i.end_step();

    i.step("Registering service", 1);
    register_service()?;

    Ok(())
});
```

Match the component's `progress_weight` to the total of `step` calls
inside its work — the bar will track accurately.

## Build multiple installers from one crate

Use `[package.metadata.installrs.feature.<name>]` overlays plus
cargo-feature-gated `source!`:

```toml
[features]
pro = []

[package.metadata.installrs]
product-name = "My App"
icon = "assets/icon.png"

[package.metadata.installrs.feature.pro]
product-name = "My App Pro"
icon = "assets/icon-pro.png"
```

```rust
i.file(source!("app.exe"), "app.exe").install()?;
#[cfg(feature = "pro")]
i.file(source!("pro-features.dat", features = ["pro"]), "pro-features.dat").install()?;
```

Build:

```sh
installrs build . --output my-app
installrs build . --feature pro --output my-app-pro
```

## Stamp a CI version without touching `Cargo.toml`

```sh
installrs build . -m installer.file-version=1.2.3 -m installer.product-version=1.2.3
```

`-m` (alias `--metadata`) takes any
`[package.metadata.installrs]` key — including dotted paths into
subtables — and overrides what's in `Cargo.toml` for that build only.

## Run headless / unattended

The same wizard definition runs without a window when the user passes
`--headless` — only the install page's callback executes. Wire your own
auto-confirm flag so headless mode skips prompts:

```rust
i.add_option("yes", OptionKind::Flag, "Skip confirmation prompts");
i.process_commandline()?;

w.custom_page("Install Location", "", |p| { /* ... */ })
    .on_before_leave(|i| {
        if i.option::<bool>("yes").unwrap_or(false) {
            return Ok(true);
        }
        confirm("Confirm", "Proceed?")
    });
```

`on_before_leave` doesn't fire in headless mode (there's no navigation),
so this only matters for the GUI path.

## Write Windows registry keys

```rust
#[cfg(target_os = "windows")]
{
    use installrs::RegistryHive::*;

    i.registry()
        .set(LocalMachine, r"Software\MyApp", "InstallDir", install_dir)
        .install()?;
    i.registry()
        .set(LocalMachine, r"Software\MyApp", "Version", env!("CARGO_PKG_VERSION").to_string())
        .install()?;
}
```

In `uninstall()`:

```rust
#[cfg(target_os = "windows")]
i.registry()
    .remove(LocalMachine, r"Software\MyApp")
    .recursive()
    .install()?;
```

Missing keys on `remove` are treated as success — uninstalls are
idempotent.

## Create a Start-menu / Desktop shortcut

```rust
#[cfg(target_os = "windows")]
{
    let appdata = std::env::var("APPDATA")?;
    let start_menu = format!(r"{appdata}\Microsoft\Windows\Start Menu\Programs\My App.lnk");
    i.shortcut(&start_menu, "app.exe")
        .description("Launch My App")
        .working_dir(".")
        .install()?;
}
```

The shortcut target is resolved against `out_dir` if relative, so
"app.exe" picks up the file you just installed.

## See also

- [Getting Started](getting-started.md) — zero-to-installer walkthrough.
- [GUI Wizard](gui-wizard.md) — full wizard API reference.
- [Installer API](installer-api.md) — components, options, CLI parsing.
- [Embedded files, builder ops, and progress](embedded-files.md) — the
  full set of `file` / `dir` / `step` / `registry` / `shortcut` ops.
