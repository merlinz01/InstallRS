<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Since the crate is pre-1.0, minor-version bumps (`0.x.0`) may contain
breaking changes; patch bumps (`0.x.y`) will not.

## [Unreleased]

### Fixed

- Subsystem `"auto"` resolution now runs before the uninstaller sources
  are generated, so both installer and uninstaller get `"windows"` as
  intended in GUI builds.

## [0.1.0-rc4] — 2026-04-22

### Added

- `.skip_if(|ctx| bool)` on any wizard page to hide it dynamically.

### Fixed

- Generated `Cargo.toml` now uses the user crate's real `[package].name`
  instead of mangling underscores to hyphens.
- Generated `build.rs` no longer warns `unused_mut` on `res` when no
  resource keys are set.

[Unreleased]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc4...HEAD
[0.1.0-rc4]: https://github.com/merlinz01/InstallRS/compare/v0.1.0-rc3...v0.1.0-rc4
