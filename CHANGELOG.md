<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Since the crate is pre-1.0, minor-version bumps (`0.x.0`) may contain
breaking changes; patch bumps (`0.x.y`) will not.

## [Unreleased]

### Fixed

- Generated installer / uninstaller `Cargo.toml` now uses the user
  installer crate's real `[package].name` verbatim instead of
  reconstructing it from the lib crate name via `.replace('_', "-")`.
- Generated `build.rs` suppresses the spurious `unused_mut` warning on
  `let mut res = …` when no resource-modifying config keys are set.

[Unreleased]: https://github.com/merlinz01/InstallRS/commits/main
