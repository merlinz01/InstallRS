<!-- markdownlint-configure-file {
  "MD013": {
    "line_length": 100
  }
} -->
<!-- markdownlint-disable-next-line MD033 MD041 -->
<div align="center">

![InstallRS](installrs.svg)

# InstallRS

**A Rust-based framework for building self-contained software installers.**

</div>

Are you tired of wrestling with clunky installer frameworks?
That only compile and run on a developer-unfriendly OS?
That force you to write your installer logic in a 1990's scripting language
without proper flow control and error handling?
That have restrictive licenses and closed-source implementations?
Do you want the full power of Rust's ecosystem at your fingertips?

We can do better in 2026.
InstallRS is here to revolutionize the way you create software installers.

## Features

- Write your installer logic in plain Rust
- Full access to Rust's standard library and third-party crates
- Scans your source code to detect which files need to be embedded
- Embeds those files into a self-contained executable using `include_bytes!`
- Fluent builder API for installing files and directories, with options for
  overwrite behavior, Unix permissions, directory filters, and error handlers
- Step-weighted progress tracking with pluggable sinks: each component
  declares a `progress_weight`; builder ops and manual `step()` calls
  advance a shared cursor, with a streaming API for sub-step updates
- Automatically generates both installer and uninstaller binaries
- Supports file compression (lzma, gzip, bzip2) to reduce binary size
- Small binaries — no runtime overhead
- Optional native wizard GUI (welcome, license, components, directory picker,
  progress, finish, error) with translatable button labels and page-level
  `on_enter` / `on_before_leave` callbacks — Win32 on Windows, GTK3 on Linux
- `on_start` / `on_exit` callbacks run in both GUI and headless modes — the
  same wizard definition works either way (`--headless` skips the window and
  runs the install callback inline)
- Automatic cancellation: every file/dir/remove/mkdir/uninstaller op checks
  a shared cancel flag before doing work. The wizard's Cancel button and a
  Ctrl+C handler (first press cancels, second press exits) both flip it
- Component system: let users pick optional features via wizard checkboxes or
  `--components` / `--with` / `--without` CLI flags
- Built-in native dialog helpers (`info`, `warn`, `error`, `confirm`)
- Built-in payload integrity check: a SHA-256 of the embedded compressed
  blobs is baked into the installer at build time and verified at startup,
  so a corrupted download is detected before any file operations run
- Windows resource support: icons (PNG auto-converted to ICO), version info, manifests
- Separate configuration for installer and uninstaller binaries

## Usage

Write a library crate with `install` and `uninstall` functions:

```rust
use anyhow::Result;
use installrs::{source, Installer};

pub fn install(i: &mut Installer) -> Result<()> {
    i.set_out_dir("C:/my_app");
    i.dir(source!("assets"), "assets").install()?;
    i.file(source!("app.exe"), "app.exe").install()?;
    i.uninstaller("uninstall.exe").install()?;
    Ok(())
}

pub fn uninstall(i: &mut Installer) -> Result<()> {
    i.remove("C:/my_app").install()?;
    Ok(())
}
```

Then build with the `installrs` CLI:

```sh
installrs --target ./my-installer --output installer.exe
```

For a step-by-step walkthrough from `cargo install` to your first working
installer, see the [Getting Started guide](docs/getting-started.md). For a
complete working example with GUI, components, translations, and custom
pages, see the [`example/`](example) directory.

### Source paths and the `source!` macro

Embedded files and directories are referenced by the `source!("path")` macro,
which evaluates to a `Source` newtype at compile time. The build tool scans
your source for `source!(...)` invocations, and embeds the corresponding file
(or directory tree, decided by filesystem metadata at build time).

Pass the result directly to `Installer::file` or `Installer::dir`:

```rust
i.file(source!("app.exe"), "app.exe").install()?;
i.dir(source!("data"), "data").install()?;
```

The macro also accepts build-time-only keyword options that the scanner
reads from the source — the runtime expansion still evaluates to a
`Source(u64)`. Supported keys:

- `ignore = ["glob", ...]` — extra glob patterns applied when gathering a
  directory, merged (union) with the CLI `--ignore` list. Repeat references
  to the same path across the installer/uninstaller merge their ignore
  lists, so only one declaration needs to carry it.

```rust
i.dir(source!("assets", ignore = ["*.bak", "scratch"]), "assets").install()?;
```

### Builder options

Every install operation returns a builder that terminates with `.install()`.
Common chainable options:

```rust
i.file(source!("app.exe"), "app.exe")
    .status("Installing application")      // pushes to the GUI status label
    .log("Writing app.exe")                // pushes to the log area
    .overwrite(OverwriteMode::Backup)      // Overwrite | Skip | Error | Backup
    .mode(0o755)                           // Unix only
    .install()?;

i.dir(source!("data"), "data")
    .filter(|rel| !rel.ends_with(".bak"))  // skip matching entries
    .on_error(|_path, _err| ErrorAction::Skip)
    .install()?;
```

### Progress reporting

Progress is **step-weighted per component**. Each component you register
declares a `progress_weight` — the number of step units it contributes to
the bar when selected. The total is the sum of selected components'
weights. Every builder op (`.file().install()`, `.dir().install()`, etc.)
advances the cursor by its own weight (default 1, override with
`.weight(n)`). If your actual op count differs from the declared
`progress_weight`, the bar just over- or undershoots — it's an estimate,
not a contract.

For custom work that isn't a builder op (downloads, service registration,
etc.) use:

- `i.step("msg", weight)` — one-shot advance + status message
- `i.begin_step("msg", weight)` + `i.set_step_progress(fraction)` +
  `i.end_step()` — for streaming within a single step (e.g. download
  progress advances the bar smoothly from step-start to step-end)

Attach any `ProgressSink` via `set_progress_sink`; the GUI wizard attaches
one automatically. All `.status()`, `.log()`, and progress updates flow
through the sink.

## Installer API

| Method                               | Description                                            |
| ------------------------------------ | ------------------------------------------------------ |
| `set_out_dir(dir)`                   | Set the base directory for relative output paths       |
| `file(src, dest)`                    | Install a single embedded file                         |
| `dir(src, dest)`                     | Install an embedded directory tree                     |
| `mkdir(dir)`                         | Create a directory                                     |
| `uninstaller(dest)`                  | Write the uninstaller executable                       |
| `remove(path)`                       | Remove a file or directory                             |
| `exists(path)`                       | Check whether a path exists                            |
| `exec_shell(cmd)`                    | Run a shell command                                    |
| `set_progress_sink(sink)`            | Attach a `ProgressSink` for status & progress          |
| `total_steps()`                      | Sum of selected components' `progress_weight`          |
| `step(msg, weight)`                  | One-shot: advance cursor + emit status                 |
| `begin_step(msg, weight)`            | Open a weighted step for streaming sub-updates         |
| `set_step_progress(fraction)`        | Update position within the open step (0.0..=1.0)       |
| `end_step()`                         | Close the open step (jump to its end)                  |
| `reset_progress()`                   | Reset the step cursor to zero                          |
| `enable_self_delete()`               | Windows: relaunch from copy so original can be deleted |
| `component(id, label, desc, weight)` | Register an optional component                         |
| `is_component_selected(id)`          | Check whether a component is currently selected        |
| `set_component_selected(id, on)`     | Enable/disable a component                             |
| `process_commandline()`              | **Required.** Parse registered CLI args                |
| `set_log_file(path)`                 | Tee status messages to a file (see `--log` option)     |
| `log_error(&err)`                    | Manually record an error line to the log file          |
| `option(name, kind)`                 | Register a user-defined CLI option                     |
| `get_option::<T>(name)`              | Typed accessor for a parsed user option                |
| `option_value(name)`                 | Raw `&OptionValue` for a parsed user option            |
| `cancel()` / `check_cancelled()`     | Set / error-if-set the cancellation flag               |

## Components

Register optional features with
`i.component(id, label, description, progress_weight)`. Components start
selected by default; call `.default_off()` on ones the user has to opt
into, and `.required()` on ones that can't be unchecked:

```rust
i.component("core", "Core files", "Always installed", 10)
    .required();
i.component("docs", "Documentation", "User manual and readme", 3);
i.component("extras", "Extra samples", "Optional example files", 1)
    .default_off();
```

Branch on selection inside the install callback:

```rust
if i.is_component_selected("docs") {
    i.dir(source!("docs"), "docs").install()?;
}
```

The wizard renders a `components_page(...)` with one checkbox per component
(required ones greyed-out). Users can also drive the installer from the
command line:

- `--headless` — disable the GUI
- `--list-components` — print available components and exit 0
- `--components a,b,c` — install exactly this set (required always included)
- `--with a,b` / `--without c` — delta from defaults
- `--log <path>` — tee every `status` / `log` / error message to a file (append mode).

**All installers must call `i.process_commandline()?`** after registering
components (before running the wizard or doing headless work). This parses
argv and applies the flags above.

### Custom command-line options

Register your own flags via `i.option(name, OptionKind)` _before_ calling
`process_commandline()`, then read them afterwards with a typed getter:

```rust
use installrs::OptionKind;

i.option("config", OptionKind::String); // --config /path
i.option("port", OptionKind::Int);      // --port 8080
i.option("verbose", OptionKind::Flag);  // --verbose (presence = true)
i.option("fast", OptionKind::Bool);     // --fast true|false|yes|no|on|off

i.process_commandline()?;

let config: Option<String> = i.get_option("config");
let port: i64 = i.get_option("port").unwrap_or(8080);
let verbose: bool = i.get_option("verbose").unwrap_or(false);
```

`get_option::<T>` is generic over `FromOptionValue`, which is implemented
for `bool`, `String`, `i64`, `i32`, `u64`, and `u32`. Unknown flags now
cause `process_commandline()` to return an error — register everything you
expect users to pass.

## GUI

Enable the optional wizard by setting `gui = true` in
`[package.metadata.installrs]`. The build tool picks the backend based on
the target triple: **Win32** on Windows (`winsafe`), **GTK3** on Linux
(`gtk-rs`). In your `install` / `uninstall` functions, build the wizard
with `InstallerGui::wizard()`:

```rust
use installrs::gui::*;

// Register components up front (optional).
i.component("docs", "Documentation");

// Required: parse CLI flags (--headless, --components, etc.).
i.process_commandline()?;

InstallerGui::wizard()
    .title("My App Installer")
    .welcome("Welcome!", "Click Next to continue.")
    .license("License Agreement", include_str!("../LICENSE"), "I accept")
    .components_page("Select Components", "Choose features to install:")
    .directory_picker("Choose Install Location", "Install to:", "C:/MyApp")
    .on_before_leave(|ctx| confirm("Confirm", &format!("Install to {}?", ctx.install_dir())))
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
    )
    .run(i)?;
```

If the install callback returns an error (including cancellation via the
Cancel button or Ctrl+C), the wizard navigates to the error page — the
provided `message` sits above an auto-populated text area showing the
actual error. Without an `.error_page(...)`, failures fall back to a
native error dialog instead.

For the uninstall flow, use `.uninstall_page(cb)` instead of
`.install_page(cb)`. It behaves identically but causes the preceding Next
button to render `ButtonLabels::uninstall` (default `"Uninstall"`), so
users don't see "Install" on the button that kicks off an uninstall.
Customize the label by passing `.buttons(ButtonLabels { uninstall:
"Desinstalar".into(), ..Default::default() })`.

Page-level `on_enter` and `on_before_leave` callbacks fire only on
forward navigation — the Back button walks backwards without re-running
either callback, so you won't prompt the user for confirmation when
they're just retreating.

### Custom pages

`.custom_page(heading, label, |p| { ... })` lays out a column of simple
widgets — text fields, checkboxes, and dropdowns — each bound to an
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
        "license_file", "License file:", "",
        &[("License", "*.lic;*.key"), ("All files", "*.*")],
    );
    p.dir_picker("data_dir", "Data directory:", "");
    p.multiline("notes", "Notes:", "", 3);
})
.on_before_leave(|ctx| {
    let user: String = ctx.installer().get_option("username").unwrap_or_default();
    if user.trim().is_empty() {
        installrs::gui::error("Required", "Please enter a username.");
        return Ok(false);
    }
    Ok(true)
})
```

Widgets pre-fill from the options store on entry and write back on
forward navigation — so `--username=alice` on the command line
pre-fills the field (as long as you registered the option via
`i.option("username", OptionKind::String)` before
`process_commandline`). Validation lives in `on_before_leave`:
return `Ok(false)` to keep the user on the page.

Native dialog helpers (`installrs::gui::info`, `warn`, `error`, `confirm`)
wrap `MessageBox` (Win32) or `gtk::MessageDialog` (GTK3) with the wizard
window as parent.

For a pre-wizard language selector, `installrs::gui::choose_language(title,
prompt, &[(code, display), ...], default_code) -> Result<Option<String>>`
shows a modal dropdown and returns the selected code (or `None` if
dismissed). Run it _before_ building the wizard — page strings are
captured eagerly, so the locale must be final by then:

```rust
init_locale();                                    // read system locale
if let Some(code) = installrs::gui::choose_language(
    &t!("installer.language.title"),               // already localized
    &t!("installer.language.prompt"),
    &[("en", "English"), ("es", "Español"), ("de", "Deutsch")],
    Some(&rust_i18n::locale()),
)? {
    rust_i18n::set_locale(&code);
}
InstallerGui::wizard()
    .title(&t!("installer.title"))                 // now uses chosen locale
    // ...
```

### Headless mode

When `--headless` is passed (and applied via `i.process_commandline()`),
`InstallerGui::run` skips the window and runs the install callback inline.
Status / log messages emit to stderr instead of the install-page log. Pair
with `.on_start(...)` and `.on_exit(...)` for setup and cleanup that must
happen in both modes:

```rust
InstallerGui::wizard()
    .on_start(|i| {
        if i.headless {
            eprintln!("Running headless install...");
        }
        Ok(())
    })
    .on_exit(|i| {
        if i.headless { eprintln!("Done."); }
        Ok(())
    })
    // ... pages ...
    .install_page(|ctx| { /* runs in both modes */ Ok(()) })
    .run(i)?;
```

`on_start` runs before the window opens (or before the install callback in
headless mode). `on_exit` runs after the window closes (or after install in
headless mode) — even if the install failed.

On Linux, target systems need GTK3 runtime libraries installed
(`libgtk-3-0` and its dependencies — present by default on virtually all
desktop distros). Building a Linux installer also requires GTK3 **dev**
headers on the build host (`libgtk-3-dev` on Debian/Ubuntu,
`gtk3-devel` on Fedora/RHEL).

## Command Line Options

```sh
--target <dir>           Directory containing the installer source crate (default: .)
--output <file>          Output installer file path (default: ./installer)
--compression <method>   lzma, gzip, bzip2, or none (default: lzma)
--ignore <patterns>      Comma-separated glob patterns to ignore (default: .git,.svn,node_modules)
--target-triple <triple> Rust target triple for cross-compilation (e.g. x86_64-pc-windows-gnu)
-v                       Debug output (use -vv for trace)
-q, --quiet              Suppress non-error output
-s, --silent             Suppress all output
```

## Windows Resource Configuration

Configure icons, version info, and manifests via `[package.metadata.installrs]`
in your installer's `Cargo.toml`. Shared settings apply to both installer
and uninstaller; use `[package.metadata.installrs.installer]`
and `[…uninstaller]` sub-tables to override per-binary.

```toml
# Shared defaults
[package.metadata.installrs]
icon = "assets/app.png"                  # .png or .ico — PNG auto-converts to ICO
icon-sizes = [16, 32, 48, 256]           # optional, defaults to [16, 32, 48, 64, 128, 256]
language = 0x0409                        # Windows LANGID
subsystem = "console"                    # "console" or "windows"
execution-level = "requireAdministrator" # manifest: asInvoker, requireAdministrator, highestAvailable
dpi-aware = "permonitorv2"               # manifest: true, false, system, permonitor, permonitorv2
supported-os = ["7", "8", "8.1", "10"]   # manifest: vista, 7, 8, 8.1, 10 (defaults to all)
product-name = "My App"
file-version = "1.0.0.0"
product-version = "1.0.0.0"
legal-copyright = "Copyright (c) 2026"

# Installer-specific overrides
[package.metadata.installrs.installer]
file-description = "My App Installer"
original-filename = "installer.exe"

# Uninstaller-specific overrides
[package.metadata.installrs.uninstaller]
file-description = "My App Uninstaller"
original-filename = "uninstaller.exe"
```

## Requirements

- Rust toolchain (stable)
- The target crate must be a library crate exporting `install` and `uninstall`

## License

This project is licensed under the MIT License.
See the [LICENSE.txt](LICENSE.txt) file for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
