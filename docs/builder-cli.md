<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# `installrs` CLI

The `installrs` command-line tool builds a self-contained installer
executable from an installer crate.

## Synopsis

```sh
installrs --target <dir> --output <file> [options]
```

## Flags

| Flag                       | Description                                                        |
| -------------------------- | ------------------------------------------------------------------ |
| `--target <dir>`           | Directory containing the installer source crate. Default: `.`      |
| `--output <file>`          | Output installer file path. Default: `./installer`                 |
| `--compression <method>`   | Compression method for embedded files (default `lzma`)             |
| `--ignore <patterns>`      | Comma-separated glob patterns to ignore when gathering directories |
| `--target-triple <triple>` | Rust target triple for cross-compilation                           |
| `--feature <name>`         | Enable a user-library cargo feature (repeatable)                   |
| `-v` / `-vv`               | Debug output / trace output                                        |
| `-q` / `--quiet`           | Suppress non-error output                                          |
| `-s` / `--silent`          | Suppress all output                                                |

## Environment variables

- `INSTALLRS_LOCAL_PATH=1`: For local development of InstallRS itself
  emits `installrs = { path = "..." }` in the generated `Cargo.toml`
  instead of depending on the published crate.

## Examples

Build an installer with defaults:

```sh
installrs --target my-installer --output installer
```

Cross-compile for Windows from Linux (requires mingw):

```sh
installrs --target my-installer --output installer.exe \
  --target-triple x86_64-pc-windows-gnu
```

Use gzip compression for faster builds at the cost of larger binaries:

```sh
installrs --target my-installer --output installer --compression gzip
```

Verbose build to see what files got embedded:

```sh
installrs --target my-installer --output installer -v
```

Enable user-library cargo features — gates `source!(..., features =
[...])` entries and activates matching `#[cfg(feature = "...")]` code
in your installer library:

```sh
installrs --target my-installer --feature pro --feature docs
```

The named features must exist in your installer crate's `[features]`
table. The builder passes them through to the user-crate dependency of
the generated installer and uninstaller, so one `--feature` flag
covers both the embedded-file gating and the compile-time code
gating.

## Customizing the generated release profile

The generated installer/uninstaller crates use an aggressive
size-optimized release profile (`lto = true`, `codegen-units = 1`,
`opt-level = "z"`) which is slow to compile. For faster iteration
during development, override via cargo env vars — they're inherited by
the inner `cargo build --release` that `installrs` spawns:

```sh
CARGO_PROFILE_RELEASE_LTO=false \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
CARGO_PROFILE_RELEASE_OPT_LEVEL=1 \
installrs --target my-installer
```

Expect a 3–5× speedup at the cost of a larger binary. Drop these for
the real release build.

## See also

- [Building for production](building.md) — cross-compilation,
  reproducibility, payload integrity, code signing.
- [Getting Started](getting-started.md) — the walkthrough that uses
  these flags in context.
