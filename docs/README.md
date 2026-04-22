<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# InstallRS documentation

Guides covering every part of InstallRS, from your first installer to
shipping signed production binaries. If you're new, start with
[Getting Started](getting-started.md); if you already have an installer
working and need to solve a specific problem, jump straight to the
relevant guide below.

## First steps

- **[Getting Started](getting-started.md)** — install the CLI, create a
  library crate, write `install` / `uninstall` functions, build your
  first installer, add a GUI wizard.

## Writing installers

- **[Embedded files, builder ops, and progress](embedded-files.md)** —
  the `source!` macro, the fluent `file` / `dir` / `mkdir` / `remove` /
  `uninstaller` / `shortcut` / `step` API, and the step-weighted
  progress model.
- **[GUI Wizard](gui-wizard.md)** — wizard builder, custom pages,
  error page, native dialogs, pre-wizard language selector, and headless
  mode.
- **[Components and CLI options](components-and-cli.md)** — selectable
  components with `--components` / `--with` / `--without`, plus custom
  CLI flags (`--config /path`, `--port 8080`, etc.) that auto-wire to
  custom-page widgets.
- **[Internationalization](internationalization.md)** — translating
  wizard strings with `rust-i18n`, automatic system-locale detection,
  and the pre-wizard language picker.

## Building and shipping

- **[Builder CLI reference](builder-cli.md)** — every flag and
  environment variable the `installrs` command understands.
- **[Building for production](building.md)** — cross-compilation to
  Windows / macOS, size and speed tuning, reproducible builds, payload
  integrity, code signing, release CI patterns.
- **[Windows Resources](windows-resources.md)** — icons (PNG
  auto-converted to ICO), VERSIONINFO, UAC manifests, DPI awareness.

## Under the hood

- **[Architecture](architecture.md)** — how the codebase is organized,
  what the generated installer crates look like, path hashing, content
  deduplication, payload integrity, self-deletion, cancellation — the
  mechanisms that span multiple source files. For contributors and
  curious users.

## Outside this folder

- **[API reference on docs.rs](https://docs.rs/installrs)** — every
  public type, method, trait, and feature flag.
- **[`example/`](../example)** — a complete working installer in the
  repo that exercises components, translations, custom pages,
  cancellation, and headless mode.
- **[README](../README.md)** — project overview and feature highlights.
