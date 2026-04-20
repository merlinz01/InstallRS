<div align="center">

![InstallRS](installrs.svg)

# InstallRS

**A Rust-based framework for building self-contained software installers.**

</div>

Very early development stage - not ready for production use.

Why? Because NSIS is a pain for anything non-trivial.
We can do better in 2026.

## Features

- Write your installer logic in plain Rust
- Full access to Rust's standard library and third-party crates
- Scans your source code to detect which files need to be embedded
- Embeds those files into a self-contained executable using `include_bytes!`
- Fluent builder API for installing files and directories, with options for
  overwrite behavior, Unix permissions, directory filters, and error handlers
- Automatic byte-weighted progress tracking with pluggable sinks
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

See the `example/` directory for a working example.

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

The `Installer` has a byte-weighted progress counter. By default the total is
the sum of all embedded bytes (plus the uninstaller size if present). Override
with `set_total_bytes(n)` or compute partial totals via `bytes_of(&[sources])`.

Attach any `ProgressSink` via `set_progress_sink`; the GUI wizard attaches one
automatically. All `.status()`, `.log()`, and progress updates flow through
the sink.

## Installer API

| Method                                              | Description                                                                                                        |
| --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `set_out_dir(dir)`                                  | Set the base output directory for relative destination paths                                                       |
| `file(src, dest)`                                   | Install a single embedded file (returns a `FileOp` builder)                                                        |
| `dir(src, dest)`                                    | Install an embedded directory tree (returns a `DirOp`)                                                             |
| `mkdir(dir)`                                        | Create a directory (returns a `MkdirOp` builder)                                                                   |
| `uninstaller(dest)`                                 | Write the uninstaller executable (returns an `UninstallerOp`)                                                      |
| `remove(path)`                                      | Remove a file or directory (returns a `RemoveOp`)                                                                  |
| `exists(path)`                                      | Check whether a path exists                                                                                        |
| `exec_shell(cmd)`                                   | Run a shell command                                                                                                |
| `set_progress_sink(sink)`                           | Attach a `ProgressSink` for status / progress / log events                                                         |
| `set_total_bytes(n)`                                | Override the progress bar's total (default: sum of embeds)                                                         |
| `bytes_of(&[sources])`                              | Compute the byte count for a subset of embedded sources                                                            |
| `reset_progress()`                                  | Reset `bytes_installed` to zero                                                                                    |
| `enable_self_delete()`                              | Windows: re-launch from temp so the install dir can be wiped                                                       |
| `component(id, label)`                              | Register an optional component (returns `&mut Component` for chaining)                                             |
| `is_component_selected(id)`                         | Check whether a component is currently selected                                                                    |
| `set_component_selected(id, on)`                    | Force a component on/off (required components ignore off)                                                          |
| `process_commandline()`                             | **Required.** Parse `--headless`/`--list-components`/`--components`/`--with`/`--without` from argv                 |
| `cancellation_flag()`                               | Returns `Arc<AtomicBool>` — shared flag set by Cancel button / Ctrl+C                                              |
| `is_cancelled()` / `cancel()` / `check_cancelled()` | Read / set / error-if-set the cancellation flag                                                                    |
| `install_ctrlc_handler()`                           | Install a SIGINT handler (called from generated `main()`); first press cancels, second press exits with status 130 |

## Components

Register optional features with `i.component(id, label)` and chain builder
methods on the returned `&mut Component`:

```rust
i.component("core", "Core files")
    .description("Always installed")
    .required(true);
i.component("docs", "Documentation")
    .description("User manual and readme");
i.component("extras", "Extra samples")
    .description("Optional example files")
    .default(false);
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

**All installers must call `i.process_commandline()?`** after registering
components (before running the wizard or doing headless work). This parses
argv and applies the flags above.

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

```
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

Configure icons, version info, and manifests via `[package.metadata.installrs]` in your installer's `Cargo.toml`. Shared settings apply to both installer and uninstaller; use `[package.metadata.installrs.installer]` and `[…uninstaller]` sub-tables to override per-binary.

```toml
# Shared defaults
[package.metadata.installrs]
icon = "assets/app.png"                   # .png or .ico — PNG auto-converts to ICO
icon-sizes = [16, 32, 48, 256]            # optional, defaults to [16, 32, 48, 64, 128, 256]
language = 0x0409                          # Windows LANGID
subsystem = "console"                      # "console" or "windows"
execution-level = "requireAdministrator"   # manifest: asInvoker, requireAdministrator, highestAvailable
dpi-aware = "permonitorv2"                 # manifest: true, false, system, permonitor, permonitorv2
supported-os = ["7", "8", "8.1", "10"]    # manifest: vista, 7, 8, 8.1, 10 (defaults to all)
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
