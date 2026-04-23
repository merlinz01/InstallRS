<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->
<!-- markdownlint-disable-file MD024 -->

# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Since the crate is pre-1.0, minor-version bumps (`0.x.0`) may contain
breaking changes; patch bumps (`0.x.y`) will not.

## [Unreleased]

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

[Unreleased]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc8...HEAD
[0.1.0-rc8]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc7...v0.1.0-rc8
[0.1.0-rc7]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc6...v0.1.0-rc7
[0.1.0-rc6]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc5...v0.1.0-rc6
[0.1.0-rc5]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc4...v0.1.0-rc5
[0.1.0-rc4]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc3...v0.1.0-rc4
