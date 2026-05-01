<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# Internationalization

InstallRS doesn't ship a translation system of its own — it defers to
whatever you plug in for your installer crate. This guide walks through
the pattern used by the repository's [`example/`](../example) installer,
which uses [`rust-i18n`](https://crates.io/crates/rust-i18n) plus
[`sys-locale`](https://crates.io/crates/sys-locale) for automatic
locale detection.

The same pattern works for any translation library you prefer — the
interesting bits are _where_ to set the locale, _which_ strings the
installer needs translated, and how to handle the pre-wizard language
selector.

## 1. Add translation dependencies

```toml
[dependencies]
installrs = "0.1"
rust-i18n = "3"
sys-locale = "0.3"
anyhow = "1"
```

## 2. Wire up `rust-i18n`

Create a `strings.yml` next to your `Cargo.toml`:

```yaml
_version: 2

installer.title:
  en: "My App Installer"
  de: "Mein-App-Installer"
  es: "Instalador de Mi App"

wizard.back:
  en: "< Back"
  de: "< Zurück"
  es: "< Atrás"

wizard.next:
  en: "Next >"
  de: "Weiter >"
  es: "Siguiente >"

# ... one entry per translatable string
```

Load it at the top of your installer lib:

```rust
use anyhow::Result;
use installrs::{source, Installer};
use rust_i18n::t;

// Load translations from any .yml files in this directory, English fallback.
rust_i18n::i18n!(".", fallback = "en");
```

The `i18n!` macro reads every `.yml` file next to `Cargo.toml` at build
time and embeds the strings into your crate — no external files to ship.

## 3. Detect the system locale

```rust
/// Detect and apply the system locale, falling back to English.
fn init_locale() {
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
    // Use just the language prefix (e.g. "de-DE" → "de").
    let lang = locale.split('-').next().unwrap_or("en");
    rust_i18n::set_locale(lang);
}
```

Call this as the **first thing** in your `install()` / `uninstall()`
functions — before registering components or building the wizard — so
every subsequent `t!()` call returns the right language.

## 4. Let the user override the language

Users on multilingual systems or in situations where the OS locale is
wrong deserve a way to pick. Use the built-in
`installrs::gui::choose_language` modal before the wizard is built:

```rust
pub fn install(i: &mut Installer) -> Result<()> {
    init_locale();

    // In GUI mode, let the user pick a language before we build the wizard.
    // Skip in headless mode — there's no GUI to show a dialog in.
    if !std::env::args().any(|a| a == "--headless") {
        let choices: &[(&str, &str)] = &[
            ("en", "English"),
            ("es", "Español"),
            ("de", "Deutsch"),
        ];
        let default = rust_i18n::locale().to_string();
        if let Some(code) = installrs::gui::choose_language(
            &t!("installer.language.title"),
            &t!("installer.language.prompt"),
            choices,
            Some(&default),
        )? {
            rust_i18n::set_locale(&code);
        }
    }

    // ... now build the wizard with strings in the chosen locale
}
```

Important: **the dialog's own title and prompt are taken from the
already-detected locale** set by `init_locale()`. Make sure those
`installer.language.*` keys have translations for every language you
support, or the pre-wizard dialog will show unlocalized fallback text.

## 5. The eager string capture gotcha

The wizard builder captures all page strings **eagerly** when you call
the `.welcome(...)`, `.license(...)`, etc. methods. That means the locale
must be final **before** you start chaining wizard page methods:

```rust
// ✅ Correct order
init_locale();
show_language_dialog_if_needed();
rust_i18n::set_locale(&chosen);

let mut w = InstallerGui::wizard(&t!("installer.title")); // chosen locale
w.welcome(&t!("installer.welcome.title"), &t!("installer.welcome.message"));
// ...
w.run(i)?;

// ❌ Wrong order — strings captured in the detected locale, not the chosen one
let mut w = InstallerGui::wizard(&t!("installer.title"));
// ...
w.run(i)?;

rust_i18n::set_locale(&chosen); // too late
```

If you need to switch the language after the wizard has been built (e.g.
a combo box on the first page), you'd have to exit and rebuild — or
restructure to put all localized content inside callbacks that re-read
`t!()` each time they fire. For most installers, the pre-wizard picker
is fine.

## 6. What to translate

Beyond the obvious (page titles, labels, button labels), don't forget:

### Button labels

Pass a localized `ButtonLabels` to `w.buttons(...)`:

```rust
w.buttons(installrs::gui::ButtonLabels {
    back: t!("wizard.back").into(),
    next: t!("wizard.next").into(),
    install: t!("wizard.install").into(),
    uninstall: t!("wizard.uninstall").into(),
    finish: t!("wizard.finish").into(),
    cancel: t!("wizard.cancel").into(),
});
```

Without this, buttons render as English defaults ("< Back", "Next >",
etc.) regardless of your page-string translations.

### Component labels and descriptions

```rust
i.add_component(
    "core",
    t!("components.core"),           // label
    t!("components.core_desc"),      // description
    10,
).required();
```

The wizard's components page pulls these strings from the registered
components, so they must be localized at registration time.

### Status and log strings emitted during install

`.status(...)` / `.log(...)` on builder ops show up in the progress page
and the log file:

```rust
i.file(source!("app.exe"), "app.exe")
    .status(t!("install.status.app"))
    .log(t!("install.log.app"))
    .install()?;
```

### Error messages

Native dialog helpers (`installrs::gui::error`, `confirm`, etc.) show
whatever text you pass. Localize the title and body:

```rust
installrs::gui::error(
    &t!("errors.install_failed.title"),
    &t!("errors.install_failed.message"),
)?;
```

Anyhow errors that propagate out of your install callback end up on the
error page (if you registered one) or in a native error dialog. If
those errors might be user-facing, construct them with localized
strings:

```rust
Err(anyhow::anyhow!("{}", t!("errors.disk_full")))
```

### Interpolated values

`rust-i18n` supports `%{name}` placeholders. Declare them in your YAML
and pass substitutions at the call site:

```yaml
confirm.install_to:
  en: "Install to %{dir}?"
  de: "In %{dir} installieren?"
  es: "¿Instalar en %{dir}?"
```

```rust
let dir: String = i.option("install-dir").unwrap_or_default();
t!("confirm.install_to", dir = dir)
```

## 7. Testing

`rust-i18n` responds to `LANG` in the environment via `sys-locale`, so:

```sh
LANG=de_DE.UTF-8 ./my-installer    # German
LANG=es_ES.UTF-8 ./my-installer    # Spanish
LANG=en_US.UTF-8 ./my-installer    # English (or unset)
```

Or force a locale programmatically in a test path — `rust_i18n::set_locale("de")`.

Verify each translation by walking through every page; the common
mistakes are missing keys (show up as `"installer.welcome.title"`
verbatim instead of the translated string) and missing language codes
in a particular `.yml` entry (fall back to the configured fallback
language, often English — worth checking in a German-only run).

## See also

- [GUI Wizard](gui-wizard.md) — the `ButtonLabels` struct and the
  eager-string-capture pattern referenced in §5.
- [Installer API](installer-api.md) — component
  labels and descriptions that need localizing at registration time.
- [`example/`](../example) — the repository's reference installer,
  translated into English, German, and Spanish.
