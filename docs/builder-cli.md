<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# `installrs` CLI

The `installrs` command-line tool builds a self-contained installer
executable from an installer crate.

## Synopsis

```sh
installrs build <dir> --output <file> [options]
```

## Arguments and flags

| Arg / flag                | Description                                                        |
| ------------------------- | ------------------------------------------------------------------ |
| `<PATH>` (positional)     | Directory containing the installer source crate. Default: `.`      |
| `--output <file>`         | Output installer file path. Default: `./installer`                 |
| `--compression <method>`  | Compression method for embedded files (default `lzma`)             |
| `--ignore <patterns>`     | Comma-separated glob patterns to ignore when gathering directories |
| `--target <triple>`       | Rust target triple for cross-compilation                           |
| `--feature <name>`        | Enable a user-library cargo feature (repeatable)                   |
| `-m` / `--metadata <K=V>` | Override a `[package.metadata.installrs]` key (repeatable)         |
| `--installrs-path <path>` | Depend on `installrs` via `path = "<path>"` instead of crates.io   |
| `-v` / `-vv`              | Debug output / trace output                                        |
| `-q` / `--quiet`          | Suppress non-error output                                          |
| `-s` / `--silent`         | Suppress all output                                                |

The `--installrs-path` flag is for InstallRS-on-InstallRS development
(CI, integration tests, the release script). End users should leave it
unset; the generated crate then pulls a version-pinned `installrs`
runtime from crates.io that matches the CLI version.

## Examples

Build an installer with defaults:

```sh
installrs build my-installer --output installer
```

Cross-compile for Windows from Linux (requires mingw):

```sh
installrs build my-installer --output installer.exe \
  --target x86_64-pc-windows-gnu
```

Use gzip compression for faster builds at the cost of larger binaries:

```sh
installrs build my-installer --output installer --compression gzip
```

Verbose build to see what files got embedded:

```sh
installrs build my-installer --output installer -v
```

Enable user-library cargo features — gates `source!(..., features =
[...])` entries and activates matching `#[cfg(feature = "...")]` code
in your installer library:

```sh
installrs build my-installer --feature pro --feature docs
```

Override individual `[package.metadata.installrs]` keys at build time
without touching `Cargo.toml` — handy for stamping CI-supplied version
strings or per-build values:

```sh
installrs build . -m file-version="$(cat version.txt)" \
                     -m product-version="$(cat version.txt)" \
                     -m installer.file-description="My App Installer (CI)" \
                     -m gui=true \
                     -m language=0x0409
```

`KEY` is a dotted path into the metadata table (`file-version`,
`installer.file-description`, etc.). `VALUE` parses as TOML — `true` /
`false` become booleans, integers (decimal or `0x`-hex) become
integers, everything else stays a string. Overrides apply _after_
feature overlays and after the package-version fallback for
`file-version` / `product-version`, so they always win.

The named features must exist in your installer crate's `[features]`
table. The builder passes them through to the user-crate dependency of
the generated installer and uninstaller, so one `--feature` flag
covers both the embedded-file gating and the compile-time code
gating.

### Per-feature metadata overlays

`[package.metadata.installrs.feature.<name>]` subtables are merged
onto the base `[package.metadata.installrs]` table when `<name>` is
passed via `--feature`. This lets one crate produce multiple installers
that differ in product name, icon, version info, or any other metadata
key — without duplicating source.

```toml
[package.metadata.installrs]
product-name = "My App"
icon = "assets/base.png"

[package.metadata.installrs.feature.pro]
product-name = "My App Pro"
icon = "assets/pro.png"
file-version = "1.2.3.4"

[package.metadata.installrs.feature.lite]
product-name = "My App Lite"
```

```sh
installrs build . --feature pro  --output installer-pro
installrs build . --feature lite --output installer-lite
installrs build . --output installer-base   # no overlay
```

Per-feature overlays can also override individual keys inside the
`installer` / `uninstaller` subtables — useful when only the installer
or uninstaller side differs per edition:

```toml
[package.metadata.installrs.installer]
file-description = "My App Installer"
internal-name = "myapp-installer"

[package.metadata.installrs.feature.pro.installer]
file-description = "My App Pro Installer"
# internal-name inherits "myapp-installer" from the base
```

Merge rules:

- Top-level keys: overlay keys replace base keys wholesale.
- `installer` / `uninstaller` subtables: merged key-by-key, so an
  overlay can override individual fields without restating the rest.
- Multiple `--feature` flags apply in argument order; later wins on
  conflicts.
- Overlay subtables can set `gui = true` to enable the wizard for
  certain editions only.

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
installrs build my-installer
```

Expect a 3–5× speedup at the cost of a larger binary. Drop these for
the real release build.

## See also

- [Building for production](building.md) — cross-compilation,
  reproducibility, payload integrity, code signing.
- [Getting Started](getting-started.md) — the walkthrough that uses
  these flags in context.
