<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# GUI Wizard

InstallRS ships an optional native wizard — Win32 on Windows, GTK3 on
Linux — with welcome / license / components / directory picker / install
/ finish / error pages, plus custom pages for arbitrary text inputs,
checkboxes, dropdowns, and file pickers. The same wizard definition runs
headless with `--headless`, so you have one code path for both modes.

## Enabling the wizard

Set `gui = true` in your installer crate's `Cargo.toml`:

```toml
[package.metadata.installrs]
gui = true
```

That injects the `gui` feature plus the platform backend (`gui-win32` on
Windows targets, `gui-gtk` on Linux).

On Linux, the **build host** needs GTK3 dev headers:

- Debian/Ubuntu: `libgtk-3-dev`
- Fedora/RHEL: `gtk3-devel`

Target systems need the GTK3 runtime (`libgtk-3-0` on Debian/Ubuntu, or
equivalent) — present by default on virtually all desktop distros.

Windows has no extra system dependency at build or runtime.

## Wizard builder

Build the wizard with `InstallerGui::wizard()`, configure it via its
builder methods, then call `run`:

```rust
use installrs::gui::*;

let mut w = InstallerGui::wizard();
w.title("My App Installer")
    .welcome("Welcome!", "Click Next to continue.")
    .license("License Agreement", include_str!("../LICENSE"), "I accept")
    .components_page("Select Components", "Choose features to install:")
    .directory_picker("Choose Install Location", "Install to:", "C:/MyApp")
    .on_before_leave(|ctx| {
        confirm("Confirm", &format!("Install to {}?", ctx.install_dir()))
    })
    .install_page(|ctx| {
        let mut i = ctx.installer();
        i.file(source!("app.exe"), "app.exe").install()?;
        if i.is_component_selected("docs") {
            i.dir(source!("docs"), "docs").install()?;
        }
        i.uninstaller("uninstall.exe").install()?;
        Ok(())
    })
    .finish_page("Done!", "Click Finish to exit.")
    .error_page(
        "Installation Failed",
        "The installation did not complete. Details are shown below.",
    );
w.run(i)?;
```

Builder methods take `&mut self` and return `&mut Self`, so you can
chain them off the binding or call them as separate statements —
whichever reads best for conditional / looped configuration:

```rust
let mut w = InstallerGui::wizard();
w.title("My App");
w.welcome("Welcome!", "...");
if include_license {
    w.license("License", LICENSE, "I accept");
}
for page in custom_pages {
    w.custom_page(&page.heading, &page.label, |b| page.build(b));
}
w.run(i)?;
```

Pages appear in the order you add them. `.on_enter(...)`,
`.on_before_leave(...)`, and `.skip_if(...)` attach to the
most-recently-added page.

### Error page

If the install callback returns `Err` (including cancellation via the
Cancel button or Ctrl+C), the wizard navigates to the error page. The
provided `message` sits above an auto-populated text area showing the
actual error string. Without an `.error_page(...)`, failures fall back to
a native modal error dialog.

### Uninstall flow

For uninstallers, use `.uninstall_page(cb)` instead of `.install_page(cb)`.
Behaves identically but the preceding Next button renders
`ButtonLabels::uninstall` (default `"Uninstall"`) instead of
`ButtonLabels::install` — so users see "Uninstall" rather than "Install"
on the button that kicks off the operation.

### Forward-only callbacks

`on_enter` and `on_before_leave` fire only on forward navigation. The
Back button walks backwards without re-running either callback, so you
won't prompt the user for confirmation when they're just retreating.

### Dynamic page skipping

Chain `.skip_if(|ctx| bool)` on any page to hide it when the predicate
returns `true`. The wizard evaluates the predicate each time it
navigates past the page, so the outcome can change mid-install as
options or component selections evolve. Both the Next and Back buttons
respect the skip — a page hidden on forward nav is also skipped on
backward nav, so the user can't get stranded on it.

Common pattern: skip pages whose input has already been supplied via
CLI flags:

```rust
.license("License", include_str!("../LICENSE"), "I accept")
.skip_if(|ctx| ctx.installer().get_option::<bool>("accept-license").unwrap_or(false))

.directory_picker("Install Location", "Install to:", default_dir)
.skip_if(|ctx| ctx.installer().get_option::<String>("install-dir").is_some())
```

The predicate must be **pure** — no side effects, no I/O. Side effects
belong in `on_enter`, which only fires for pages the user actually
sees. Skipped pages don't fire `on_enter` or `on_before_leave`.

Next-button label computation accounts for skipped pages too: if the
page immediately after the current one is skipped and the _next visible_
page is the install page, the Next button reads "Install" (or
"Uninstall") as expected.

### Translatable buttons

Customize button labels via `.buttons(...)`:

```rust
.buttons(ButtonLabels {
    back: "Atrás".into(),
    next: "Siguiente".into(),
    install: "Instalar".into(),
    uninstall: "Desinstalar".into(),
    finish: "Finalizar".into(),
    cancel: "Cancelar".into(),
})
```

## Custom pages

`.custom_page(heading, label, |p| { ... })` lays out a column of simple
widgets — text fields, passwords, numbers, multiline, checkboxes, radio
groups, dropdowns, and file/directory pickers — each bound to an
installer option by key:

```rust
.custom_page("Settings", "Configure your install:", |p| {
    p.text("username", "Username:", "admin");
    p.password("password", "Password:");
    p.number("port", "Port:", 8080);
    p.checkbox("desktop_shortcut", "Create a desktop shortcut", true);
    p.radio(
        "install_type",
        "Install type:",
        &[("typical", "Typical"), ("minimal", "Minimal"), ("custom", "Custom")],
        "typical",
    );
    p.dropdown(
        "db_backend",
        "Database:",
        &[("sqlite", "SQLite"), ("postgres", "PostgreSQL")],
        "sqlite",
    );
    p.file_picker(
        "license_file",
        "License file:",
        "",
        &[("License", "*.lic;*.key"), ("All files", "*.*")],
    );
    p.dir_picker("data_dir", "Data directory:", "");
    p.multiline("notes", "Notes:", "", 3);
})
.on_before_leave(|ctx| {
    let user: String = ctx.installer().get_option("username").unwrap_or_default();
    if user.trim().is_empty() {
        let _ = installrs::gui::error("Required", "Please enter a username.");
        return Ok(false);
    }
    Ok(true)
})
```

Widgets pre-fill from the options store on entry and write back on
forward navigation — so `--username=alice` on the command line pre-fills
the field (as long as you registered the option via
`i.option("username", OptionKind::String)` before `process_commandline`).
Validation lives in `on_before_leave`: return `Ok(false)` to keep the
user on the page.

Splitting widgets across multiple custom pages is fine — each
`.custom_page(...)` call adds a new page.

## Native dialogs

For one-off prompts outside the wizard flow:

```rust
installrs::gui::info("Done", "Installation complete.")?;
installrs::gui::warn("Heads up", "Restarting in 30s...")?;
installrs::gui::error("Failed", "Couldn't write to registry.")?;
let ok = installrs::gui::confirm("Really?", "Proceed with uninstall?")?;
```

These wrap `MessageBox` (Win32) or `gtk::MessageDialog` (GTK3), parented
to the current active window if any.

## Pre-wizard language selector

For i18n setups where you want the user to pick a language _before_ the
wizard builds (page strings get captured eagerly, so the locale must be
final):

```rust
init_locale(); // read system locale
if let Some(code) = installrs::gui::choose_language(
    &t!("installer.language.title"), // already localized
    &t!("installer.language.prompt"),
    &[("en", "English"), ("es", "Español"), ("de", "Deutsch")],
    Some(&rust_i18n::locale()),
)? {
    rust_i18n::set_locale(&code);
}
let mut w = InstallerGui::wizard();
w.title(&t!("installer.title")) // now uses chosen locale
    // ...
    ;
w.run(i)?;
```

Returns the selected code, or `None` if the user cancelled the dialog.

## Headless mode

When the user passes `--headless`, `i.process_commandline()` flips
`installer.headless = true`, and `InstallerGui::run()` skips the window
entirely — running the `install_page` callback inline on the current
thread. Status and log messages stream to stderr instead of an in-window
log.

The same wizard definition serves both modes. Use `.on_start(...)` and
`.on_exit(...)` for setup and cleanup that must happen either way:

```rust
let mut w = InstallerGui::wizard();
w.on_start(|i| {
    if i.headless {
        eprintln!("Running headless install...");
    }
    Ok(())
})
.on_exit(|i| {
    if i.headless {
        eprintln!("Done.");
    }
    Ok(())
})
// ... pages ...
.install_page(|ctx| {
    // runs in both modes
    Ok(())
});
w.run(i)?;
```

`on_start` runs before the window opens (or before the install callback
in headless mode). `on_exit` runs after the window closes (or after
install in headless mode) — **even if the install failed**.

## See also

- [Installer API](installer-api.md) — the
  `components_page` renders registered components; custom-page widgets
  bind to registered CLI options by key.
- [Internationalization](internationalization.md) — translating page
  strings, button labels, and the pre-wizard language picker.
- [Embedded files, builder ops, and progress](embedded-files.md) — the
  `install_page` callback uses these ops to do the actual work.
- [Windows Resources](windows-resources.md) — icons and manifests that
  affect wizard appearance on Windows.
