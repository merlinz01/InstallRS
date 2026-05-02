<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# Architecture

This doc explains how InstallRS is structured for contributors and
curious users — the layout of the crate, how the runtime and build
tool fit together, what the generated installer crates look like, and
the handful of subtle mechanisms (path hashing, content dedup, payload
integrity, self-deletion) that cross file boundaries.

For _using_ InstallRS, see the other guides in this folder. This one
is "how it works," not "how to use it."

## Crate layout

InstallRS is a single crate with two targets defined in one
`Cargo.toml`:

- **Library target** (`src/lib.rs`) — the runtime API. This is what
  gets linked into _every_ installer binary.
- **Binary target** (`src/bin/installrs.rs`) — the build tool users
  invoke. This only runs on developer machines and CI.

Both targets ship in the same published crate. When a user runs
`cargo install installrs`, only the binary target is installed. When
they write `installrs = "X.Y.Z"` in their installer crate's
`Cargo.toml`, cargo links the library target.

## Package structure

```text
Cargo.toml              # single [package] with lib + [[bin]] targets
src/
  lib.rs                # Installer struct + install/uninstall entry points;
                        # re-exports every public type from the submodules below
  source.rs             # Source newtype, source! macro, FNV-1a path hash
  embedded.rs           # EmbeddedEntry / DirChild / DirChildKind + verify_payload
  types.rs              # OverwriteMode, ErrorAction, DirFilter / DirErrorHandler
  progress.rs           # ProgressSink trait + internal ProgressState
  options.rs            # OptionKind / OptionValue / FromOptionValue + CmdOption
  component.rs          # Component struct with .required() builder
  ops.rs                # FileOp / DirOp / UninstallerOp / MkdirOp / RemoveOp
                        # plus install_children / backup / write helpers
  shortcut.rs           # (Windows-only) ShortcutOp + SHChangeNotify plumbing
  registry.rs           # (Windows-only) RegistryHive / Registry / Reg*Op
  gui/                  # optional GUI module (behind `gui` feature)
    mod.rs              # InstallerGui wizard builder + platform dispatch
    types.rs            # WizardConfig, WizardPage, GuiMessage, ChannelSink
    dialog.rs           # info / warn / error / confirm wrappers
    win32/              # Win32 backend (behind `gui-win32`)
      mod.rs
      window.rs         # WizardWindow: message loop, channel pump, nav buttons
      pages.rs          # Welcome / License / Components / ... / Error panels
    gtk/                # GTK3 backend (behind `gui-gtk`; Linux only)
      mod.rs
      window.rs         # wizard window: gtk::Stack pages, glib timeout channel pump
      pages.rs          # same page set using Box / Label / TextView / ProgressBar
  build/                # build-tool internals (private to the binary)
    mod.rs
    builder.rs          # core orchestration; generates installer + uninstaller crates
    scanner.rs          # AST walks for source!(...) invocations
    compress.rs         # lzma / gzip / bzip2 / none
    ico_convert.rs      # PNG-to-ICO conversion with content-addressed caching
  bin/
    installrs.rs        # CLI entry point (includes build/ via #[path])
example/                # reference installer demonstrating the API
tests/
  integration.rs        # end-to-end CLI → build → install → uninstall tests
```

## Runtime library (`src/lib.rs` + submodules)

The public API is split across the submodules listed above; `lib.rs`
owns the `Installer` struct itself (fields, constructor, CLI parser,
log file, cancellation flag, Ctrl+C handler, decompression,
component/option registries, `install_main` / `uninstall_main` entry
points, Windows self-deletion) and re-exports every type from the
submodules so `installrs::FileOp`, `installrs::Component`, etc. stay
at the crate root. Several `Installer` methods and fields are
`pub(crate)` so the builder-op modules (`ops.rs`, `shortcut.rs`,
`registry.rs`) can drive them without exposing internal plumbing to
user code.

Everything users touch at runtime:

- `Installer` — the central context passed into `install()` /
  `uninstall()`.
- `Source` + `source!` macro — compile-time path hash.
- Builder op types: `FileOp`, `DirOp`, `UninstallerOp`, `MkdirOp`,
  `RemoveOp` (plus Windows-only `ShortcutOp` and registry ops). Each
  has `.status()` / `.log()` / `.weight()` / terminal `.install()`;
  file / dir / uninstaller also support `.overwrite(OverwriteMode)`
  and `.mode(u32)`; `DirOp` adds `.filter(Fn)` and `.on_error(Fn)`.
- `ProgressSink` trait; step-weighted progress driven by an
  Installer-owned `ProgressState`.
- `EmbeddedEntry` / `DirChild` / `DirChildKind` — the entries table
  the generated `ENTRIES` static is shaped to; `verify_payload`
  checks its SHA-256 on process start.
- `Component { id, label, description, progress_weight, required,
  selected }` with `.required()`.
- User-defined CLI options (`OptionKind`, `OptionValue`,
  `FromOptionValue` for `bool`/`String`/`i64`/`i32`/`u64`/`u32`) and
  `process_commandline()` — errors on unknown flags.
- `log_error()` / `set_log_file()` — the log-file plumbing that
  `--log <path>` hooks into.
- `install_ctrlc_handler()` — SIGINT / console-Ctrl handler that flips
  the shared cancel flag on first press and exits on second.

## GUI module (`src/gui/`, feature-gated)

Behind `gui` (implies exactly one platform backend — `gui-win32` or
`gui-gtk`). The backend is picked at compile time by cfg:

- **`mod.rs`** — `InstallerGui` wizard builder, page methods,
  `error_page(title, message)`,
  `install_page` vs `uninstall_page` distinction, `run_headless` path
  that extracts the install callback and invokes it inline on stderr.
  Reads `installer.cancellation_flag()` _before_ the `std::mem::replace`
  dummy-swap so the Cancel button and Ctrl+C flip the real
  installer's flag.
- **`types.rs`** — `WizardConfig`, `WizardPage` variants including
  `Install { callback, is_uninstall }` and `Custom { heading, label,
widgets }`, `ButtonLabels` with `uninstall: String`, `ChannelSink`
  (forwards `ProgressSink` events over the wizard's mpsc channel),
  and the callback type aliases. `on_enter` / `on_before_leave` only
  fire on forward navigation (Next / auto-advance / initial entry);
  the Back button skips both.
- **`dialog.rs`** — Native `info` / `warn` / `error` / `confirm`
  wrappers around `MessageBox` (Win32) or `gtk::MessageDialog` (GTK3),
  plus `choose_language(...)` — a pre-wizard language picker. On
  Win32, the dialog loads embedded icon resource ID 1 so title bar and
  taskbar match the wizard.
- **`win32/mod.rs`** and **`win32/window.rs`** — main thread runs the
  Win32 message loop; install callback runs on a background thread
  communicating via `mpsc::Sender<GuiMessage>`. `WM_TIMER` drains the
  channel. A `ProgressSink` is auto-attached around the install
  callback.
- **`win32/pages.rs`** — per-page Win32 panel impls. `ComponentsPage`
  uses `SysListView32` with `LVS_EX::CHECKBOXES`; required items are
  rendered via `nm_custom_draw` with `GetSysColor(COLOR::GRAYTEXT)`
  and their unchecks blocked via `lvn_item_changing`.
- **`gtk/mod.rs`** and **`gtk/window.rs`** — GTK main loop replaces
  WM_TIMER with `glib::timeout_add_local(50ms)` for the channel pump;
  `Rc<RefCell<T>>` replaces `Arc<Mutex<T>>` for UI-thread shared state
  (GTK is single-threaded). Calls `gtk::disable_setlocale_once()`
  before `gtk::init()` so short locale codes like `"es"` that C's
  `setlocale()` rejects don't crash. After `gtk::main()` returns,
  explicitly destroys the window and pumps pending events so captured
  `Arc<Mutex<Installer>>` refs drop.
- **`gtk/pages.rs`** — GTK3 `Box`-based page widgets. `ComponentsPage`
  puts checkboxes in a scrollable `ListBox`. Required items stay
  sensitive (so hover events fire) but use `set_opacity(0.5)` and
  revert unchecks via a `toggled` handler.

## Build tool (`src/bin/installrs.rs` + `src/build/`)

The binary target includes `src/build/` via `#[path]`.

- **`build/builder.rs`** — core orchestration: reads user's
  `Cargo.toml`, scans source, gathers files, generates two Rust crates
  from string templates, compiles them, embeds the uninstaller into
  the installer. `gather_source` dispatches to `gather_file` or
  `gather_dir` based on filesystem metadata.
- **`build/scanner.rs`** — AST scanning via `syn`. Parses `.rs` files
  for `install()` / `uninstall()` function definitions and for
  `source!("path" [, key = value]*)` macro invocations. `visit_macro`
  catches them in any syntactic position. Produces a
  `Vec<SourceRef { path, ignore, features }>` per scope; repeat
  invocations with different `ignore` lists merge (union), and
  `features` merges as union — but an unconditional reference (empty
  `features`) anywhere wins and clears the gate.
- **`build/compress.rs`** — LZMA / gzip / bzip2 / none. Validates
  methods, compresses files during build, decompresses on cache hit
  to verify integrity.
- **`build/ico_convert.rs`** — PNG-to-ICO with content-addressed
  caching in `build/icons/`; uses the `image` + `ico` crates.

## Code generation: the two generated crates

`builder.rs` generates complete Rust crates under `build/installer/`
and `build/uninstaller/`. Each has a `Cargo.toml` and a `src/main.rs`.

### Generated `Cargo.toml`

- Pins `installrs` to `=X.Y.Z` (exact version of the CLI that
  generated it) — or `path = "..."` when `--installrs-path <PATH>` is
  passed, for InstallRS-on-InstallRS development.
- Depends on the user's installer library by path.
- Feature flags injected from config: compression method (`lzma`,
  `gzip`, `bzip2`) plus, when `gui = true`, both `gui` and the platform
  backend (`gui-win32` on Windows targets, `gui-gtk` on Linux).
- `winresource` is a build-dep **only when the target is Windows**.
  Stale `build.rs` files from prior Windows builds are cleaned up when
  retargeting to Linux.
- Release profile: `opt-level = "z"`, `strip = true`, `lto = true`,
  `codegen-units = 1` — small-binary, slow-compile territory.

### Generated `main.rs`

```rust
// Embedded file blobs — one static per unique content (SHA-256 dedup).
static D_<HASH>_LZMA: &[u8] = include_bytes!("../../files/<hash>-lzma");

// Metadata table referenced at runtime.
static ENTRIES: &[installrs::EmbeddedEntry] = &[ ... ];

// Flat list of unique blobs for payload integrity check.
static PAYLOAD_BLOBS: &[&[u8]] = &[D_<HASH>_LZMA, ...];
static PAYLOAD_HASH: [u8; 32] = [...];

// Uninstaller binary embedded into the installer.
static UNINSTALLER_DATA: &[u8] = include_bytes!("../../uninstaller-bin");

fn main() {
    if let Err(e) = installrs::verify_payload(PAYLOAD_BLOBS, UNINSTALLER_DATA, &PAYLOAD_HASH) {
        eprintln!("{e}");
        std::process::exit(1);
    }
    let mut i = installrs::Installer::new(ENTRIES, UNINSTALLER_DATA, "lzma");
    i.install_ctrlc_handler();
    i.install_main(user_crate::install);
}
```

Build order:

1. Generate + compile the uninstaller crate first → produces
   `build/uninstaller-bin`.
2. Generate the installer crate, `include_bytes!`-linking the
   uninstaller binary above.
3. Compile the installer crate → `build/installer/target/.../installer`.
4. Copy to `--output`.

## Feature flags

| Feature     | Effect                                                                      |
| ----------- | --------------------------------------------------------------------------- |
| `lzma`      | Pulls in `lzma-rust2` for LZMA / LZMA2 / `.xz` support.                     |
| `gzip`      | Pulls in `flate2`.                                                          |
| `bzip2`     | Pulls in the `bzip2` crate.                                                 |
| `gui`       | Compiles the `gui` module (platform-agnostic types + wizard builder).       |
| `gui-win32` | Implies `gui`; adds the Win32 backend (`winsafe`). Target must be Windows.  |
| `gui-gtk`   | Implies `gui`; adds the GTK3 backend (`gtk-rs`). Target must be Linux.      |

## Key design details

Cross-file mechanisms that wouldn't be obvious from reading any single
source file.

### Source paths and path hashing

User code references embedded assets with `source!("path")`, which
const-evaluates to a `Source(u64)` FNV-1a hash. The build scanner
finds these literal invocations by visiting every macro; `Installer::file`
and `Installer::dir` take the `Source` and look it up in the embedded
entries table. **`source_path_hash_const` in `lib.rs` and `fnv1a` in
`builder.rs` must stay in sync** — a drift would silently break
lookups.

The macro also accepts build-time-only keyword options:
`source!("path", ignore = ["*.bak", ...])` adds per-source glob
ignores when gathering a directory, and
`source!("path", features = ["name", ...])` gates the entry on an
active cargo feature. The scanner's `SourceRef { path, ignore,
features }` dedups by path and merges options across repeat
invocations. Feature filtering happens in `builder::build` — gated
sources are dropped from `install_sources` / `uninstall_sources`
before `gather_source`, so the generated `ENTRIES` table contains
only the active set. Active features are also injected into the
user-crate dependency's `features = [...]` list in the generated
installer and uninstaller `Cargo.toml`, so `#[cfg(feature = "name")]`
blocks in user code are compiled in consistently with the gating.

### Content deduplication

Identical files (matched by SHA-256 of their raw bytes) share a single
`&[u8]` reference in generated code. The storage filename is
`<hash>-<compression>`, so two copies of the same file with different
compression methods would be separate blobs, but that doesn't happen
in practice — one build run uses one method.

Compressed blobs live in a single `build/files/` directory shared by
both generated crates (`installer/` and `uninstaller/` each
`include_bytes!("../../files/<hash>-<compression>")`). Files
referenced from both `install` and `uninstall` are written to disk
once and cache-validated once per build, though each crate still emits
its own static for them so the bytes still appear twice in the final
linked installer (no cross-crate dedup).

### Payload integrity

`builder.rs::compute_payload_hash` SHA-256s each unique compressed
blob once (in `D_*` static declaration order), then hashes
`UNINSTALLER_DATA`. Emits:

```rust
static PAYLOAD_BLOBS: &[&[u8]] = &[D_A, D_B, ...];
static PAYLOAD_HASH: [u8; 32] = [...];
```

Generated `main()` calls `installrs::verify_payload(PAYLOAD_BLOBS,
UNINSTALLER_DATA, &PAYLOAD_HASH)` before anything else. Hashing the
flat blob list instead of the `ENTRIES` tree avoids double-counting
deduplicated files. Uninstaller binaries with no embedded sources skip
both the blobs table and the verify call.

### Windows self-deletion

`Installer::enable_self_delete()` (Windows only) copies the running
uninstaller to `%TEMP%/uninstall-{pid}/`, relaunches with
`--self-delete` and `.current_dir(&tmp_dir)` (so the install directory
isn't locked as the child's cwd), and exits. After `uninstall_main`
returns, a detached PowerShell process sleeps 5s then removes the
temp copy directory.

### Cancellation flag

`Installer` owns `cancelled: Arc<AtomicBool>`. Every builder op's
`.install()` calls `check_cancelled()?` at the top; `install_children`
checks between each file. The Cancel button (Win32 + GTK) and the
Ctrl+C handler both flip the same flag via
`installer.cancellation_flag()`.

**Key subtlety:** wizard backends do a `std::mem::replace(installer,
Installer::new(...))` dummy-swap to move the installer into the
background thread. They must read `installer.cancellation_flag()`
_before_ the swap — otherwise the flag the UI flips belongs to the
dummy, and `check_cancelled()` inside the callback never trips.

The Ctrl+C handler uses the `ctrlc` crate (SIGINT on Unix + console
Ctrl events on Windows): first press flips the flag + prints "Press
Ctrl+C again to exit immediately"; second press does
`std::process::exit(130)`. A `std::sync::Once` guards against repeat
installs.

### Forward-only page callbacks

`on_enter` fires on forward navigation only (Next, auto-advance after
install, initial-page entry). `on_before_leave` fires only before
forward navigation. The Back button skips both — users walking
backwards through the wizard won't see confirmation prompts or re-entry
effects.

### Version compatibility preflight

`builder.rs::check_installrs_version_compat` reads the user's
installer-crate `Cargo.toml`, extracts the `installrs` dep's `version
= "..."` requirement, and compares via `semver` against
`env!("CARGO_PKG_VERSION")`. Mismatch errors out _before_ any code
generation — not deep in cargo's later output. Skipped silently when
the user's dep is path-only / git-only (no `version` key).

### Generated `installrs` dep spec

`builder.rs::installrs_dep_spec` emits either:

- `installrs = { version = "=X.Y.Z", ... }` (default) — exact pin to
  the CLI's version. Generated crates compile against precisely the
  runtime the CLI was built from, no patch-level drift.
- `installrs = { path = "<PATH>", ... }` when `--installrs-path <PATH>`
  is passed on the command line. For local development of InstallRS
  itself and for integration tests (which pass it explicitly).

End users running a `cargo install`-ed CLI omit `--installrs-path` and
always get the version-pinned spec, so generated crates pull the
matching `installrs` runtime from crates.io.

### Build caching

- Generated source files use `write_if_changed` — preserves mtimes,
  skips cargo rebuilds when the output is identical.
- Compressed file entries are integrity-checked on cache hit
  (decompress + SHA-256 verify); corrupt entries get recompressed.
- Icon conversion is cached by content hash + size set in
  `build/icons/`.

### PNG-to-ICO

`.png` icons declared in `[package.metadata.installrs].icon` are
automatically converted to multi-resolution `.ico` at build time
before being passed to `winresource`. Configurable sizes via
`icon-sizes`. Conversion runs on the build host — no Windows tooling
needed to build Windows installers from Linux.

## CLI verbosity

| Flag      | Level      | Cargo behavior       |
| --------- | ---------- | -------------------- |
| (default) | info       | `cargo --quiet`      |
| `-v`      | debug      | cargo default output |
| `-vv`     | trace      | `cargo -vv`          |
| `-q`      | error only | `cargo --quiet`      |
| `-s`      | silent     | `cargo --quiet`      |

## See also

- [Building for production](building.md) — cross-compilation,
  reproducibility, code signing — all using the mechanisms described
  above.
- [Embedded files, builder ops, and progress](embedded-files.md) — the
  runtime API the generated crates call into.
- [GUI Wizard](gui-wizard.md) — user-facing view of the wizard module
  architecture covered in §GUI module.
