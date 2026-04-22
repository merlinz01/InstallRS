<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# Components and CLI

InstallRS has a **component system** that lets users pick which optional
features to install, plus a **CLI parser** that handles built-in flags
(`--headless`, `--log`, etc.) and any custom flags you register. This
doc covers both.

## Components

Register optional features with `Installer::component`:

```rust
i.component(id, label, description, progress_weight)
```

Arguments:

- `id` — internal identifier (stable string, used by CLI flags and
  `is_component_selected`).
- `label` — what the user sees in the wizard checkbox list.
- `description` — a short blurb shown when the user hovers/selects the
  component.
- `progress_weight` — how many step units the component contributes to
  the progress bar when selected. (See
  [progress reporting](../README.md#progress-reporting).)

Components start **selected** by default. Chain modifiers to change
that:

- `.default_off()` — start unchecked; user has to opt in.
- `.required()` — start checked and cannot be unchecked (renders
  greyed-out in the wizard, always on in headless mode).

```rust
i.component("core", "Core files", "Always installed", 10)
    .required();
i.component("docs", "Documentation", "User manual and readme", 3);
i.component("extras", "Extra samples", "Optional example files", 1)
    .default_off();
```

Query selection state inside the install callback:

```rust
if i.is_component_selected("docs") {
    i.dir(source!("docs"), "docs").install()?;
}
```

The wizard renders a `components_page(...)` with one checkbox per
component. Users can also drive selection from the command line via
`--components`, `--with`, `--without` — see [Built-in flags](#built-in-flags)
below.

## Command-line parsing

**Every installer must call `i.process_commandline()?`** after registering
components and custom options (before running the wizard or doing headless
work). This parses `argv` and applies all recognized flags.

### Built-in flags

- `--headless` — disable the GUI; run the install callback inline.
- `--list-components` — print available components and exit 0.
- `--components a,b,c` — install exactly this set (required components
  are always included even if not listed).
- `--with a,b` / `--without c` — delta from defaults.
- `--log <path>` — tee every status / log / error message to a file
  (append mode). Format: `[*] <status>`, `<log>`, `[ERROR] <msg>`.

Unknown flags cause `process_commandline()` to return an error — so
register every custom flag you expect users to pass _before_ calling it.

### Custom CLI options

Register your own flags via `i.option(name, OptionKind)`, then read them
after `process_commandline()` with a typed getter:

```rust
use installrs::OptionKind;

i.option("config", OptionKind::String);  // --config /path
i.option("port", OptionKind::Int);       // --port 8080
i.option("verbose", OptionKind::Flag);   // --verbose (presence = true)
i.option("fast", OptionKind::Bool);      // --fast true|false|yes|no|on|off

i.process_commandline()?;

let config: Option<String> = i.get_option("config");
let port: i64 = i.get_option("port").unwrap_or(8080);
let verbose: bool = i.get_option("verbose").unwrap_or(false);
```

`OptionKind` variants:

| Variant  | Syntax         | Semantics                                 |
| -------- | -------------- | ----------------------------------------- |
| `Flag`   | `--name`       | Boolean (`true` if passed, else `false`). |
| `String` | `--name value` | Arbitrary string.                         |
| `Int`    | `--name 42`    | Signed integer (`i64`).                   |
| `Bool`   | `--name true`  | Explicit boolean                          |

Bool options accept `true`/`false`, `yes`/`no`, `on`/`off`, `1`/`0` (case-insensitive).

`get_option::<T>` is generic over `FromOptionValue`, which is implemented
for `bool`, `String`, `i64`, `i32`, `u64`, and `u32`. Mismatched types
(e.g. `get_option::<i64>` on a `String` option) return `None`.

### Integration with custom wizard pages

Custom-page widgets are automatically wired to installer options by key:

- On page entry, widget values pre-fill from `installer.option_value(key)`.
- On forward navigation, widget values write back via `set_option_value`.

So CLI pre-fills work for free — if you register `--username` via
`i.option("username", OptionKind::String)` before `process_commandline`,
passing `--username=alice` on the command line pre-fills the `username`
text widget on the custom page.

See [GUI wizard — custom pages](gui-wizard.md#custom-pages) for the
widget API.

## See also

- [GUI Wizard](gui-wizard.md) — the components page and custom pages
  that render the components and custom options registered here.
- [Embedded files, builder ops, and progress](embedded-files.md) —
  each component declares a `progress_weight`; this page explains how
  it maps to the progress bar.
- [Internationalization](internationalization.md) — translating
  component labels and descriptions.
