<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# Embedded files, builder ops, and progress

This guide covers what goes inside your `install()` / `uninstall()`
callbacks: referencing files with the `source!` macro, using the
fluent builder to copy / remove / run commands, and driving the
progress bar.

## The `source!` macro

Embedded files and directories are referenced by the `source!("path")`
macro, which evaluates to a `Source` newtype at compile time. The build
tool scans your source for `source!(...)` invocations and embeds the
corresponding file — or directory tree, decided by filesystem metadata
at build time.

```rust
i.file(source!("app.exe"), "app.exe").install()?;
i.dir(source!("data"), "data").install()?;
```

Paths are relative to your installer crate's root (next to `Cargo.toml`).

### Keyword options

The macro accepts build-time-only keyword arguments. The runtime
expansion still evaluates to a `Source(u64)`; the options are parsed by
the scanner and affect _how_ the file/dir is gathered, not the runtime
behavior.

- `ignore = ["glob", ...]` — extra glob patterns applied when gathering
  a directory, merged (union) with the CLI `--ignore` list. Repeat
  references to the same path across installer and uninstaller merge
  their ignore lists, so only one declaration needs to carry it.

```rust
i.dir(source!("assets", ignore = ["*.bak", "scratch"]), "assets").install()?;
```

- `features = ["name", ...]` — cargo feature gates. The source is
  embedded only when at least one listed feature is active for the
  build (`installrs --feature <name>`). With no `features` key, the
  source is always embedded. The matching name must exist in your
  user crate's `[features]` table; the builder passes it through to
  the user-crate dependency of the generated installer, so
  `#[cfg(feature = "name")]` on the surrounding `Installer::file(...)`
  call lines up. Repeat references to the same path union their
  feature lists; an unconditional reference anywhere clears the
  gate.

```rust
#[cfg(feature = "pro")]
i.file(source!("pro-assets.dat", features = ["pro"]), "assets.dat").install()?;
```

Then build with `installrs --target . --feature pro`.

## Builder ops

Every install operation returns a builder that terminates with
`.install()`. Available ops on `Installer`:

| Method               | Purpose                                 |
| -------------------- | --------------------------------------- |
| `file(src, dest)`    | Install a single embedded file.         |
| `dir(src, dest)`     | Install an embedded directory tree.     |
| `mkdir(dest)`        | Create a directory.                     |
| `uninstaller(dest)`  | Write the uninstaller executable.       |
| `remove(path)`       | Remove a file or directory.             |
| `shortcut(dst, tgt)` | Create a Windows `.lnk` (Windows-only). |
| `registry()`         | Windows registry ops (Windows-only).    |

Common chainable options on the builders:

```rust
i.file(source!("app.exe"), "app.exe")
    .status("Installing application")      // status label on the GUI
    .log("Writing app.exe")                // appears in the install log
    .overwrite(OverwriteMode::Backup)      // Overwrite | Skip | Error | Backup
    .mode(0o755)                           // Unix file mode (no-op on Windows)
    .weight(2)                             // progress weight (default 1)
    .install()?;

i.dir(source!("data"), "data")
    .filter(|rel| !rel.ends_with(".bak"))  // skip matching entries
    .on_error(|_path, _err| ErrorAction::Skip)
    .install()?;
```

`OverwriteMode` options:

- `Overwrite` _(default)_ — replace any existing file at the destination.
- `Skip` — leave the existing file in place.
- `Error` — fail the install if the file already exists.
- `Backup` — rename the existing file to `<name>.bak` before writing.

## Windows shortcuts

On Windows, create `.lnk` shortcuts with `shortcut(dst, target)`. Both
paths resolve against `out_dir` when relative, like every other op.
The method is gated behind `#[cfg(target_os = "windows")]` — wrap
calls with the same cfg if your installer is cross-platform.

```rust
#[cfg(target_os = "windows")]
i.shortcut("shortcuts/MyApp.lnk", "app.exe")
    .arguments("--flag")
    .working_dir(".")                   // launch in the install dir
    .description("Launch MyApp")
    .icon("app.exe", 0)                 // path + icon resource index
    .install()?;
```

The target must exist at the time the shortcut is created — install
files first, then their shortcuts. To place shortcuts on the Desktop
or Start Menu, pass an absolute path (e.g. via `std::env::var` of
`USERPROFILE` / `APPDATA`); `out_dir` resolution only kicks in for
relative paths.

## Windows registry

`i.registry()` returns a short-lived handle for registry ops. Like
`shortcut`, this is Windows-only — wrap with
`#[cfg(target_os = "windows")]` in cross-platform installers.

```rust
use installrs::RegistryHive::*;

// Set named values (intermediate keys are created automatically).
i.registry()
    .set(LocalMachine, r"Software\MyApp", "InstallDir", "C:\\MyApp")
    .install()?;
i.registry()
    .set::<u32>(CurrentUser, r"Software\MyApp", "Version", 42)
    .install()?;
i.registry()
    .set(LocalMachine, r"Software\MyApp", "Tags",
         vec!["alpha".to_string(), "beta".to_string()])
    .install()?;

// Set the (unnamed) default value — pass "" as the value name.
i.registry()
    .set(ClassesRoot, r"myapp\shell\open\command", "",
         "\"C:\\MyApp\\app.exe\" \"%1\"")
    .install()?;

// Read a value.
let dir: String = i.registry()
    .get(LocalMachine, r"Software\MyApp", "InstallDir")?;

// Uninstall: remove the whole subtree.
i.registry()
    .remove(LocalMachine, r"Software\MyApp")
    .recursive()
    .install()?;

// Or delete a specific named value.
i.registry()
    .delete(CurrentUser, r"Software\MyApp", "Version")
    .install()?;
```

Value type is inferred from the argument; supported types are anything
implementing `winreg::types::ToRegValue` — `&str`, `String`, `u32`,
`u64`, `Vec<String>` (REG_MULTI_SZ), `OsString`, etc. `get::<T>`
takes any `winreg::types::FromRegValue`. Missing keys or values on
`remove` / `delete` are treated as success (idempotent uninstalls).

## Progress reporting

Progress is **step-weighted per component**. Each component you register
declares a `progress_weight` — the number of step units it contributes
to the bar when selected. The total is the sum of selected components'
weights.

Every builder op advances the cursor by its own weight (default `1`,
override with `.weight(n)`). If your actual op count differs from the
declared `progress_weight`, the bar just over- or undershoots — it's
an estimate, not a contract.

### Custom step progress

For custom work that isn't a builder op (downloads, service
registration, post-install scripts, etc.):

```rust
// One-shot: advance cursor + emit status message.
i.step("Registering service", 1);

// Streaming: advance smoothly from step-start to step-end.
i.begin_step("Downloading", 5);
for (done, total) in download_chunks(&url)? {
    i.set_step_progress(done as f64 / total as f64);
}
i.end_step();
```

### Progress sinks

Attach a `ProgressSink` via `set_progress_sink` to receive status,
progress, and log events. The GUI wizard attaches one automatically;
non-GUI installers also auto-attach a built-in
[`StderrProgressSink`](https://docs.rs/installrs/latest/installrs/struct.StderrProgressSink.html)
that renders a TTY-aware progress bar to stderr — so the common
headless path works without any setup.

To replace the default with your own (e.g., to forward events to an
external bus), implement the `ProgressSink` trait:

```rust
use installrs::ProgressSink;

struct MySink;
impl ProgressSink for MySink {
    fn set_status(&self, s: &str)      { eprintln!("[*] {s}"); }
    fn set_progress(&self, frac: f64)  { eprintln!("[{:.0}%]", frac * 100.0); }
    fn log(&self, m: &str)             { eprintln!("    {m}"); }
}

i.set_progress_sink(Box::new(MySink));
```

All `.status()`, `.log()`, and progress updates flow through the sink.
`Installer` also exposes `set_status(...)`, `set_progress(...)`, and
`log(...)` directly for emitting events from user code without
constructing a builder op first.

## See also

- [How-to](how-to.md) — recipes for progress, registry, shortcuts, and more.
- [Installer API](installer-api.md) — the
  `progress_weight` declared on each component, and how it interacts
  with per-op weights.
- [GUI Wizard](gui-wizard.md) — the install page consumes the status
  and log messages emitted by builder ops.
- [Getting Started](getting-started.md) — a walkthrough that uses the
  `source!` macro and builder ops end-to-end.
- [Builder CLI reference](builder-cli.md) — `--compression` and
  `--ignore` flags that affect what and how files are embedded.
