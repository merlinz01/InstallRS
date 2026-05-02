<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->
<!-- markdownlint-disable-file MD024 -->

# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Since the crate is pre-1.0, minor-version bumps (`0.x.0`) may contain
breaking changes; patch bumps (`0.x.y`) will not.

## [Unreleased]

### Added

- `--help` / `-h` now prints a usage summary listing built-in flags,
  every option registered with `add_option` (with their `help` strings),
  and the registered components, then exits.

### Changed

- **Breaking:** `Installer::add_option` now returns `()` instead of `&mut Self`.
- **Breaking:** `Installer::cancellation_flag()` replaced by
  `Installer::cancellation_token()` returning a new `CancellationToken` newtype with
  `.cancel()` / `.is_cancelled()` methods.
- **Breaking:** Registry methods renamed for symmetry: `Registry::set` → `set_value`,
  `get` → `get_value`, `remove` → `delete_key`, `delete` → `delete_value`.
- **Breaking:** Path-taking methods now accept `impl AsRef<Path>`
  instead of `impl AsRef<str>`: `Installer::set_out_dir`, `file`,
  `dir`, `uninstaller`, `mkdir`, `remove`, `exists`, plus `shortcut`
  (and its `working_dir` / `icon` setters). Pass `&Path` / `PathBuf`
  directly without `to_string_lossy()`. `&str` and `String` callers
  are unaffected.
- **Breaking:** `Installer::headless` is no longer a public field.
  Use `Installer::is_headless()` instead.
- **Breaking:** Custom-page widgets (`text`, `number`, `checkbox`,
  `dropdown`, `radio`, `file_picker`, `dir_picker`, `multiline`) no
  longer take a `default` argument. Seed defaults via
  `i.set_option_if_unset(key, value)` before `w.run(i)`.
- `w.run(i)` now panics if a widget's expected option kind doesn't
  match the kind the option was registered with (e.g. `p.number(...)`
  bound to a `String` option).
- Most GUI string-taking methods now accept `impl AsRef<str>`.

### Removed

- **Breaking:** `InstallerGui::directory_picker(heading, label, key)`
  removed. Use a `custom_page` with a `dir_picker` widget instead:
  `w.custom_page(heading, "", |p| p.dir_picker(key, label));`.

## [0.1.0-rc14] — 2026-05-01

### Changed

- **Breaking:** `InstallerGui::wizard(title)` renamed to `InstallerGui::new(title)`.
- **Breaking:** `Registry::default` (which was a misnamed shorthand
  for `set(..., "", value)`) removed. Pass `""` as the name argument
  to `set` to write the unnamed default value.
- `RegSetOp::overwrite(OverwriteMode)` now mirrors the file/dir
  overwrite policy: `Skip` leaves an existing value untouched,
  `Error` fails if it exists, `Overwrite` (default) and `Backup`
  always write.
- **Breaking:** option-API renames. `get_option` → `option`,
  `set_option_default` → `set_option_if_unset`, and the registration
  method previously named `option` is now `add_option`. The raw
  `option_value` / `set_option_value` accessors are no longer public —
  use the typed `option<T>` / `set_option` instead.
- **Breaking:** Rename `Installer::component` to `Installer::add_component`.
- `Installer::add_component` now panics if the same component ID is added more than once.
- **Breaking:** Remove `Component::default_off`. Call `set_component_selected(id, false)` instead.

### Fixed

- Files referenced from both `install` and `uninstall` no longer fail
  the build with "failed to read &lt;hash&gt;-lzma for payload hash".
  The compressed-blob cache now lives in a single shared
  `build/files/` directory used by both generated crates.

## [0.1.0-rc13] — 2026-04-30

### Changed

- LZMA backend swapped from `lzma-rs` to `lzma-rust2` (pure-Rust, with
  the `optimization` feature) at preset 9, yielding much smaller
  installers. Output format is now `.xz`.

### Fixed

- `FileVersion` and `ProductVersion` now also populate the
  VERSIONINFO `FIXEDFILEINFO` numeric block. Pre-release suffixes are
  stripped when converting to integer format for these fields.

## [0.1.0-rc12] — 2026-04-30

### Added

- `PageHandle::hide_log()` on install / uninstall pages to omit the
  rolling log textbox, leaving only the status label and progress bar.
- `file-version` and `product-version` default to the user crate's
  `[package].version` when not set in
  `[package.metadata.installrs]`, so bumping `Cargo.toml` automatically
  restamps the installer. Explicit metadata values still win.
- `--metadata KEY=VALUE` (alias `-m`) on the CLI overrides any
  `[package.metadata.installrs]` key for a build, including dotted
  paths like `installer.file-version=1.2.3` for subtable-specific
  overrides. Useful for stamping CI-provided versions without
  touching `Cargo.toml`.

## [0.1.0-rc11] — 2026-04-30

### Added

- `[package.metadata.installrs.feature.<name>]` subtables are
  deep-merged onto the base metadata when the feature is active via
  `--feature <name>`, so one crate can produce multiple installers
  that differ in product name, icon, version info, GUI mode, etc.

### Fixed

- Focus is set to the Next button on navigation so users can just hit Enter to proceed,
  except for the license page where the focus starts on the checkbox.

### Removed

- **Breaking:** `InstallerGui::on_start` / `on_exit` callbacks. Run
  setup and cleanup code directly before and after `w.run(i)` instead.

## [0.1.0-rc10] — 2026-04-29

### Changed

- String-taking setters now accept `impl AsRef<str>`, so `String`,
  `&str`, `Cow<str>`, `&Cow<str>` (e.g. results of `rust_i18n::t!`),
  and `format!(...)` all work without explicit conversion.
- **Breaking:** wizard callbacks now receive `&mut Installer` /
  `&Installer` directly; `GuiContext` and `PageContext` are gone.
  Status, progress, log, and install-dir helpers moved onto `Installer`.
  Non-GUI installs auto-attach a stderr progress sink.
- **Breaking:** `directory_picker(heading, label, key)` binds to a
  named option instead of a literal default. New helpers
  `set_option`, `set_option_if_unset`, `is_option_registered`.
- **Breaking:** `Installer::option` takes a third `help: impl AsRef<str>` argument.
- **Breaking:** `InstallerGui::wizard(title)` takes the window title;
  the standalone `.title(...)` method is gone. `buttons`, `on_start`,
  `on_exit` return `()` instead of `&mut Self` (statement-style,
  matching the rest of the wizard API).
- Improve font size and layout of heading labels on Windows.
- Set SS_NOPREFIX styles on Win32 labels to allow ampersands in text.
- `PageHandle::with_widgets(|p| ...)` adds a column of input widgets
  (text, checkbox, dropdown, etc.) below welcome and finish pages.
  Widgets bind to installer options the same way as `custom_page`.
- Internal refactors

## [0.1.0-rc9] — 2026-04-23

### Changed

- **Breaking:** `InstallerGui` builder methods now take `&mut self`
  instead of consuming by value. Bind the wizard with
  `let mut w = InstallerGui::wizard();`, configure it, then call `w.run(i)`.
- **Breaking:** `on_enter`, `on_before_leave`, and `skip_if` are no
  longer methods on `InstallerGui`. They live on the `PageHandle`
  returned by each page-adding method (`welcome`, `license`,
  `custom_page`, etc.) — so they always attach to the page you just
  added, with no silent drop when you forget to add a page first.

## [0.1.0-rc8] — 2026-04-23

### Added

- Cargo-feature gating for embedded sources via
  `source!(path, features = [...])` and `installrs --feature <name>`.

## [0.1.0-rc7] — 2026-04-22

### Added

- `Installer::registry()` for Windows registry operations.

## [0.1.0-rc6] — 2026-04-22

### Added

- `Installer::shortcut(dst, target)` for creating Windows `.lnk` files.

### Removed

- `Installer::exec_shell()`. Use `std::process::Command` directly;
  call `i.step("label", weight)` beforehand if you want a labeled
  progress step.

## [0.1.0-rc5] — 2026-04-22

### Fixed

- Subsystem `"auto"` resolution now runs before the uninstaller sources
  are generated, so both installer and uninstaller get `"windows"` as
  intended in GUI builds.
- `process_commandline()` silently accepts `--self-delete` on Windows
  when it's the first arg (used by `enable_self_delete` relaunch).

## [0.1.0-rc4] — 2026-04-22

### Added

- `.skip_if(|ctx| bool)` on any wizard page to hide it dynamically.

### Fixed

- Generated `Cargo.toml` now uses the user crate's real `[package].name`
  instead of mangling underscores to hyphens.
- Generated `build.rs` no longer warns `unused_mut` on `res` when no
  resource keys are set.

[Unreleased]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc14...HEAD
[0.1.0-rc14]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc13...v0.1.0-rc14
[0.1.0-rc13]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc12...v0.1.0-rc13
[0.1.0-rc12]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc11...v0.1.0-rc12
[0.1.0-rc11]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc10...v0.1.0-rc11
[0.1.0-rc10]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc9...v0.1.0-rc10
[0.1.0-rc9]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc8...v0.1.0-rc9
[0.1.0-rc8]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc7...v0.1.0-rc8
[0.1.0-rc7]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc6...v0.1.0-rc7
[0.1.0-rc6]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc5...v0.1.0-rc6
[0.1.0-rc5]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc4...v0.1.0-rc5
[0.1.0-rc4]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc3...v0.1.0-rc4
