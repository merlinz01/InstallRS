<!-- markdownlint-configure-file { "MD013": { "line_length": 250 } } -->

# `[package.metadata.installrs]` reference

Every key the `installrs` build tool reads from your installer crate's
`Cargo.toml`, in one place. The full schema lives under
`[package.metadata.installrs]`; subtables and feature overlays let
installer and uninstaller binaries diverge.

## Conventions

- **Kebab-case for all InstallRS-defined keys** (`file-version`,
  `execution-level`, `icon-sizes`). New keys will follow the same
  convention.
- **Subtable names are bare lowercase identifiers** (`installer`,
  `uninstaller`, `feature.<name>`).
- **Unknown keys are silently ignored** — a typo (`fil-version` vs
  `file-version`) does nothing rather than erroring. Double-check key
  names against this reference.
- **TOML types are strict.** Strings stay strings; integers stay
  integers (no parsing of strings as numbers). Booleans accept `true`
  / `false` only.

## Top-level keys

These apply to both the installer and the uninstaller binary unless
overridden in the `installer` / `uninstaller` subtable.

### Build-shape keys

| Key          | Type              | Default                      | Description                                                                                                                                                                       |
| ------------ | ----------------- | ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `gui`        | bool              | `false`                      | Compile in the wizard GUI (Win32 on Windows, GTK3 on Linux). Headless still works.                                                                                                |
| `icon`       | string (path)     | none                         | `.png` or `.ico` icon path, relative to the installer crate root. PNG auto-converts to multi-resolution ICO on Windows; the same PNG is embedded as the GTK window icon on Linux. |
| `icon-sizes` | array of integers | `[16, 32, 48, 64, 128, 256]` | ICO frame sizes generated when converting from PNG. Smaller lists yield smaller binaries.                                                                                         |

### VERSIONINFO keys (Windows)

Each maps to a Windows VERSIONINFO field of the same name in CamelCase.
On non-Windows targets these are ignored.

| Key                 | Type   | Default             | VERSIONINFO field                                                                               |
| ------------------- | ------ | ------------------- | ----------------------------------------------------------------------------------------------- |
| `product-name`      | string | none                | `ProductName`                                                                                   |
| `file-description`  | string | none                | `FileDescription`                                                                               |
| `file-version`      | string | `[package].version` | `FileVersion` (also stamps the numeric `FILEVERSION` block, with pre-release suffixes stripped) |
| `product-version`   | string | `[package].version` | `ProductVersion` (also stamps the numeric `PRODUCTVERSION` block)                               |
| `original-filename` | string | none                | `OriginalFilename`                                                                              |
| `legal-copyright`   | string | none                | `LegalCopyright`                                                                                |
| `legal-trademarks`  | string | none                | `LegalTrademarks`                                                                               |
| `company-name`      | string | none                | `CompanyName`                                                                                   |
| `internal-name`     | string | none                | `InternalName`                                                                                  |
| `comments`          | string | none                | `Comments`                                                                                      |

### Manifest / runtime keys (Windows)

Generate or supply a Windows application manifest. **Mutually
exclusive** — supply at most one of `manifest-file`, `manifest-raw`,
or the four generated-manifest keys (`execution-level`, `dpi-aware`,
`long-path-aware`, `supported-os`). The builder errors at config time
if you mix them.

| Key               | Type             | Default          | Description                                                                                                      |
| ----------------- | ---------------- | ---------------- | ---------------------------------------------------------------------------------------------------------------- |
| `subsystem`       | string           | `"console"`      | One of `"console"`, `"windows"`, `"auto"`. `"auto"` resolves to `"windows"` when `gui = true`, else `"console"`. |
| `language`        | integer          | `0x0409` (en-US) | Windows LANGID for the resource.                                                                                 |
| `execution-level` | string           | `"asInvoker"`    | UAC level — `"asInvoker"`, `"requireAdministrator"`, or `"highestAvailable"`.                                    |
| `dpi-aware`       | bool / string    | unset            | DPI awareness mode. `true` / `"true"` → `"true"`; or `"permonitorv2"`, `"permonitor"`, `"system"`, `"unaware"`.  |
| `long-path-aware` | bool             | unset            | Adds the long-path entry to the manifest.                                                                        |
| `supported-os`    | array of strings | all              | Compat declarations: `"vista"`, `"7"`, `"8"`, `"8.1"`, `"10"`.                                                   |
| `manifest-file`   | string (path)    | unset            | Path to an external `.manifest` file. Replaces all generated manifest keys.                                      |
| `manifest-raw`    | string           | unset            | Raw manifest XML inline. Replaces all generated manifest keys.                                                   |

## Subtables: `installer` and `uninstaller`

Override individual keys for one binary without restating the rest.
Subtable values merge field-by-field onto the top-level table.

```toml
[package.metadata.installrs]
product-name = "My App"
file-description = "My App"

[package.metadata.installrs.installer]
file-description = "My App Installer"

[package.metadata.installrs.uninstaller]
file-description = "My App Uninstaller"
original-filename = "uninstall.exe"
```

The subtables accept any of the keys listed above. Top-level `gui`,
`icon`, and `icon-sizes` apply to both binaries (they're not split per
binary at present).

## Per-feature overlays: `feature.<name>`

Subtables under `feature.<NAME>` are merged onto the base when the
feature is active via `installrs build --feature <NAME>`. Multiple
features apply in argument order; later wins on conflicts.

```toml
[package.metadata.installrs]
product-name = "My App"

[package.metadata.installrs.feature.pro]
product-name = "My App Pro"
icon = "assets/pro.png"

[package.metadata.installrs.feature.pro.installer]
file-description = "My App Pro Installer"
```

```sh
installrs build --feature pro --output installer-pro
installrs build              --output installer-base
```

Overlay merge rules:

- Top-level keys: overlay value replaces the base value wholesale.
- `installer` / `uninstaller` subtables: merged key-by-key, so an
  overlay can change one field without restating the others.
- Overlay subtables can set `gui = true` to enable the wizard for
  certain editions only.

## CLI override: `--metadata`

Any key (or dotted path) in this schema can be overridden at build
time without touching `Cargo.toml`:

```sh
installrs build --metadata file-version=1.2.3 \
                --metadata product-version=1.2.3 \
                --metadata installer.file-description="My App (CI)" \
                --metadata gui=true \
                --metadata language=0x0409
```

Values parse as TOML — `true` / `false` become booleans, decimal /
hex (`0x...`) integers become integers, everything else stays a
string. Overrides apply _after_ feature overlays and after the
package-version fallback for `file-version` / `product-version`, so
they always win.

## Stability

The schema and case convention are part of InstallRS's public surface
and follow normal semver. Breaking changes (key renames, removed
keys) appear in the [`CHANGELOG`](../CHANGELOG.md) and bump the minor
version (or major after 1.0). New keys are additive and ship in patch
releases.

## See also

- [Windows Resources](windows-resources.md) — the user-facing tour
  of icons, version info, and manifests.
- [Builder CLI reference](builder-cli.md) — `--metadata`, `--feature`,
  and the rest of the CLI surface.
- [Building for production](building.md) — cross-compilation,
  reproducibility, code signing.
