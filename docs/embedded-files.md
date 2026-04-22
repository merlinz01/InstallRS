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
the scanner and affect *how* the file/dir is gathered, not the runtime
behavior.

- `ignore = ["glob", ...]` — extra glob patterns applied when gathering
  a directory, merged (union) with the CLI `--ignore` list. Repeat
  references to the same path across installer and uninstaller merge
  their ignore lists, so only one declaration needs to carry it.

```rust
i.dir(source!("assets", ignore = ["*.bak", "scratch"]), "assets").install()?;
```

## Builder ops

Every install operation returns a builder that terminates with
`.install()`. Available ops on `Installer`:

| Method                | Purpose                              |
| --------------------- | ------------------------------------ |
| `file(src, dest)`     | Install a single embedded file.      |
| `dir(src, dest)`      | Install an embedded directory tree.  |
| `mkdir(dest)`         | Create a directory.                  |
| `uninstaller(dest)`   | Write the uninstaller executable.    |
| `remove(path)`        | Remove a file or directory.          |
| `exec_shell(cmd)`     | Run a shell command (blocking).      |

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

- `Overwrite` *(default)* — replace any existing file at the destination.
- `Skip` — leave the existing file in place.
- `Error` — fail the install if the file already exists.
- `Backup` — rename the existing file to `<name>.bak` before writing.

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
progress, and log events. The GUI wizard attaches one automatically —
you don't need to touch this directly for wizard-based installers.

For custom pipelines (e.g., writing your own progress output or
forwarding to an external bus), implement the `ProgressSink` trait:

```rust
use installrs::ProgressSink;

struct StderrSink;
impl ProgressSink for StderrSink {
    fn set_status(&self, s: &str)      { eprintln!("[*] {s}"); }
    fn set_progress(&self, frac: f64)  { eprintln!("[{:.0}%]", frac * 100.0); }
    fn log(&self, m: &str)             { eprintln!("    {m}"); }
}

i.set_progress_sink(Box::new(StderrSink));
```

All `.status()`, `.log()`, and progress updates flow through the sink.

## See also

- [Components and CLI options](components-and-cli.md) — the
  `progress_weight` declared on each component, and how it interacts
  with per-op weights.
- [GUI Wizard](gui-wizard.md) — the install page consumes the status
  and log messages emitted by builder ops.
- [Getting Started](getting-started.md) — a walkthrough that uses the
  `source!` macro and builder ops end-to-end.
- [Builder CLI reference](builder-cli.md) — `--compression` and
  `--ignore` flags that affect what and how files are embedded.
