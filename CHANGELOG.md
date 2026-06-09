<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->
<!-- markdownlint-disable-file MD024 -->

# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Since the crate is pre-1.0, minor-version bumps (`0.x.0`) may contain
breaking changes; patch bumps (`0.x.y`) will not.

## [Unreleased]

- The `installrs` CLI now ships an embedded `asInvoker` Windows manifest, so
  Windows no longer auto-elevates it via UAC installer detection.

## [0.1.0] — 2026-05-02

- Initial stable release.

[Unreleased]: https://github.com/merlinz01/InstallRS/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/merlinz01/InstallRS/releases/tag/v0.1.0
