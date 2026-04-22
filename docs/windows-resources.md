<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# Windows Resource Configuration

Configure icons, version info, and manifests for Windows builds via
`[package.metadata.installrs]` in your installer crate's `Cargo.toml`.

Shared settings at the top-level table apply to both the installer and
uninstaller binaries. Use `[package.metadata.installrs.installer]` and
`[package.metadata.installrs.uninstaller]` sub-tables to override
specific fields per binary (merged field-by-field; unspecified fields
inherit from the shared table).

## Quickstart

```toml
# Shared defaults for both installer and uninstaller
[package.metadata.installrs]
icon = "assets/app.png"                   # .png or .ico — PNG auto-converts
icon-sizes = [16, 32, 48, 256]            # optional; default covers 16..256
language = 0x0409                         # en-US
subsystem = "console"                     # "console" | "windows" | "auto"
execution-level = "requireAdministrator"  # manifest: UAC level
dpi-aware = "permonitorv2"                # manifest: DPI awareness
supported-os = ["7", "8", "8.1", "10"]    # manifest: compat declarations
product-name = "My App"
file-version = "1.0.0.0"
product-version = "1.0.0.0"
legal-copyright = "Copyright (c) 2026"

# Installer-only overrides
[package.metadata.installrs.installer]
file-description = "My App Installer"
original-filename = "installer.exe"

# Uninstaller-only overrides
[package.metadata.installrs.uninstaller]
file-description = "My App Uninstaller"
original-filename = "uninstaller.exe"
```

## Reference

All keys use kebab-case.

| Key                 | Type             | Description                                             |
| ------------------- | ---------------- | ------------------------------------------------------- |
| `icon`              | string           | Path to `.png` or `.ico` file                           |
| `icon-sizes`        | array of ints    | ICO frame sizes for PNG conversion                      |
| `language`          | integer          | Windows LANGID (e.g. `0x0409` for en-US)                |
| `subsystem`         | string           | `"console"` (default), `"windows"`, or `"auto"`         |
| `execution-level`   | string           | `"asInvoker"/"requireAdministrator"/"highestAvailable"` |
| `dpi-aware`         | bool / string    | DPI awareness mode                                      |
| `long-path-aware`   | bool             | Long-path manifest entry                                |
| `supported-os`      | array of strings | OS compatibility list (default all)                     |
| `manifest-file`     | string           | Path to external `.manifest` file                       |
| `manifest-raw`      | string           | Raw manifest XML                                        |
| `product-name`      | string           | VERSIONINFO ProductName                                 |
| `file-description`  | string           | VERSIONINFO FileDescription                             |
| `file-version`      | string           | VERSIONINFO FileVersion                                 |
| `product-version`   | string           | VERSIONINFO ProductVersion                              |
| `original-filename` | string           | VERSIONINFO OriginalFilename                            |
| `legal-copyright`   | string           | VERSIONINFO LegalCopyright                              |
| `legal-trademarks`  | string           | VERSIONINFO LegalTrademarks                             |
| `company-name`      | string           | VERSIONINFO CompanyName                                 |
| `internal-name`     | string           | VERSIONINFO InternalName                                |
| `comments`          | string           | VERSIONINFO Comments                                    |

Options for Windows version compatibility declarations (`supported-os`):
`"vista"`, `"7"`, `"8"`, `"8.1"`, `"10"`

## Notes

- **Icons**: PNG files auto-convert to multi-resolution ICO at build
  time. Conversion is cached by content hash + `icon-sizes`, so repeat
  builds are instant. Specify an explicit `.ico` file if you want tighter
  control over the frame set.
- **Subsystem `"auto"`**: resolves to `"windows"` when
  `[package.metadata.installrs].gui = true`, otherwise `"console"`. Use
  this if you want the wizard to not pop a console window while keeping
  a fallback to console when GUI is off.
- **Manifest keys are mutually exclusive**: you can use at most one of
  `manifest-file`, `manifest-raw`, or the generated-manifest keys
  (`execution-level`, `dpi-aware`, `long-path-aware`, `supported-os`).
  The build tool errors at configuration time if more than one is used.
- **Non-Windows targets**: when cross-compiling to a non-Windows triple,
  all Windows resource config is ignored — the icon may still be used
  for the GTK wizard if it's a PNG, since the `[package.metadata.installrs].icon`
  key is also read on Linux.

## See also

- [Building for production](building.md) — cross-compiling to Windows
  from Linux, and Authenticode code signing with signtool.
- [GUI Wizard](gui-wizard.md) — the icon configured here also appears
  in the wizard's title bar and taskbar entry.
