//! Command-line argument parsing for the `Installer`. Handles built-in
//! flags (`--headless`, `--list-components`, `--components`, `--with`,
//! `--without`, `--log`) and dispatches user-defined options registered
//! via [`Installer::option`].

use anyhow::{anyhow, Result};

use crate::options::OptionValue;
use crate::{Installer, OptionKind};

impl Installer {
    /// Parse command-line arguments and apply them to the installer state.
    ///
    /// **All installers must call this.** Typical placement is the first
    /// line inside the `install` / `uninstall` function, right after
    /// registering components:
    ///
    /// ```rust,ignore
    /// pub fn install(i: &mut Installer) -> Result<()> {
    ///     i.add_component("docs", "Documentation", "", 3);
    ///     i.process_commandline()?;
    ///     // ... wizard or headless install flow ...
    /// }
    /// ```
    ///
    /// Recognized flags:
    /// - `--help` / `-h` — print a usage summary (built-in flags + every
    ///   option registered with [`Installer::add_option`]) and exit
    ///   status 0
    /// - `--headless` — sets `self.headless = true`, disables GUI
    /// - `--list-components` — print the component table and exit status 0
    /// - `--components a,b,c` — install exactly this set (plus required)
    /// - `--with a,b` — enable these in addition to defaults
    /// - `--without a,b` — disable these (required cannot be disabled)
    /// - `--log <path>` — tee all status / log / error messages to a file
    ///   (append mode; see [`Installer::set_log_file`])
    ///
    /// Component-flag precedence: `--components` (when present) replaces
    /// the default selection entirely; otherwise `--with` adds to defaults
    /// and `--without` removes from them. `--with` and `--without` are
    /// applied on top of `--components` when both appear. Required
    /// components stay selected regardless.
    ///
    /// **Ordering:** register every component
    /// ([`component`](Installer::add_component)) and every custom flag
    /// ([`option`](Installer::option)) *before* calling this. Unknown
    /// component ids and unregistered flags both error out.
    pub fn process_commandline(&mut self) -> Result<()> {
        let args: Vec<String> = std::env::args().collect();
        self.process_commandline_from(&args)
    }

    pub(crate) fn process_commandline_from(&mut self, args: &[String]) -> Result<()> {
        let mut exact: Option<Vec<String>> = None;
        let mut with: Vec<String> = Vec::new();
        let mut without: Vec<String> = Vec::new();
        let mut list = false;

        let mut i = 1;
        #[cfg(windows)]
        // `--self-delete` is recognized only as the very first argument
        // so we don't have to worry about it appearing as a value later on.
        if args.get(i).map(|s| s.as_str()) == Some("--self-delete") {
            i += 1;
        }
        while i < args.len() {
            let a = &args[i];
            let (flag, inline_val): (&str, Option<&str>) = if let Some(eq) = a.find('=') {
                (&a[..eq], Some(&a[eq + 1..]))
            } else {
                (a.as_str(), None)
            };
            let take_val = |i: &mut usize| -> Result<String> {
                if let Some(v) = inline_val {
                    Ok(v.to_string())
                } else {
                    *i += 1;
                    args.get(*i)
                        .cloned()
                        .ok_or_else(|| anyhow!("{flag} requires a value"))
                }
            };
            match flag {
                "--help" | "-h" => {
                    print!("{}", self.help_text(args));
                    std::process::exit(0);
                }
                "--headless" => self.headless = true,
                "--list-components" => list = true,
                "--components" => {
                    let v = take_val(&mut i)?;
                    exact = Some(v.split(',').map(|s| s.trim().to_string()).collect());
                }
                "--with" => {
                    let v = take_val(&mut i)?;
                    with.extend(v.split(',').map(|s| s.trim().to_string()));
                }
                "--without" => {
                    let v = take_val(&mut i)?;
                    without.extend(v.split(',').map(|s| s.trim().to_string()));
                }
                "--log" => {
                    let v = take_val(&mut i)?;
                    self.set_log_file(&v)?;
                }
                _ => {
                    // User-defined option? Strip the leading `--`.
                    let bare = flag.strip_prefix("--").unwrap_or(flag);
                    let opt = self
                        .options
                        .iter()
                        .find(|o| o.name == bare)
                        .cloned()
                        .ok_or_else(|| anyhow!("unknown flag: {flag}"))?;
                    let parsed = match opt.kind {
                        OptionKind::Flag => {
                            if inline_val.is_some() {
                                return Err(anyhow!(
                                    "--{} is a flag and does not take a value",
                                    opt.name
                                ));
                            }
                            OptionValue::Flag(true)
                        }
                        OptionKind::String => OptionValue::String(take_val(&mut i)?),
                        OptionKind::Int => {
                            let v = take_val(&mut i)?;
                            let n: i64 = v.parse().map_err(|_| {
                                anyhow!("--{} expected an integer, got {v:?}", opt.name)
                            })?;
                            OptionValue::Int(n)
                        }
                        OptionKind::Bool => {
                            let v = take_val(&mut i)?;
                            let b = match v.to_ascii_lowercase().as_str() {
                                "true" | "1" | "yes" | "on" => true,
                                "false" | "0" | "no" | "off" => false,
                                _ => {
                                    return Err(anyhow!(
                                        "--{} expected true/false, got {v:?}",
                                        opt.name
                                    ))
                                }
                            };
                            OptionValue::Bool(b)
                        }
                    };
                    self.option_values.insert(opt.name.clone(), parsed);
                }
            }
            i += 1;
        }

        // Flags default to `false` so `option::<bool>("flag")` always
        // returns `Some(...)` for registered flags, regardless of presence.
        for opt in &self.options {
            if matches!(opt.kind, OptionKind::Flag) && !self.option_values.contains_key(&opt.name) {
                self.option_values
                    .insert(opt.name.clone(), OptionValue::Flag(false));
            }
        }

        if list {
            println!("Available components:");
            for c in &self.components {
                let marker = if c.required {
                    "*"
                } else if c.selected {
                    "+"
                } else {
                    "-"
                };
                println!("  {} {:<20} {}", marker, c.id, c.label);
                if !c.description.is_empty() {
                    println!("    {}", c.description);
                }
            }
            println!("\n  * required   + selected   - unselected");
            std::process::exit(0);
        }

        let known: std::collections::HashSet<String> =
            self.components.iter().map(|c| c.id.clone()).collect();
        for id in exact
            .iter()
            .flatten()
            .chain(with.iter())
            .chain(without.iter())
        {
            if !id.is_empty() && !known.contains(id) {
                return Err(anyhow!("unknown component: {id}"));
            }
        }

        if let Some(wanted) = exact {
            let wanted: std::collections::HashSet<String> = wanted.into_iter().collect();
            for c in self.components.iter_mut() {
                let on = c.required || wanted.contains(&c.id);
                c.selected = on;
            }
        }
        for id in with {
            self.set_component_selected(&id, true);
        }
        for id in without {
            self.set_component_selected(&id, false);
        }

        Ok(())
    }

    pub(crate) fn help_text(&self, args: &[String]) -> String {
        use std::fmt::Write;

        let prog = args
            .first()
            .and_then(|p| std::path::Path::new(p).file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("installer");

        let mut s = String::new();
        let _ = writeln!(s, "USAGE: {prog} [OPTIONS]");
        let _ = writeln!(s);
        let _ = writeln!(s, "OPTIONS:");
        let _ = writeln!(s, "      --help, -h              Show this help and exit");
        let _ = writeln!(s, "      --headless              Run without a GUI");
        let _ = writeln!(
            s,
            "      --list-components       Print the component list and exit"
        );
        let _ = writeln!(
            s,
            "      --components <LIST>     Install exactly this comma-separated set"
        );
        let _ = writeln!(
            s,
            "      --with <LIST>           Enable these components in addition to defaults"
        );
        let _ = writeln!(s, "      --without <LIST>        Disable these components");
        let _ = writeln!(
            s,
            "      --log <PATH>            Tee status/log/error output to a file"
        );

        if !self.options.is_empty() {
            let _ = writeln!(s);
            let _ = writeln!(s, "USER OPTIONS:");
            let max_left = self
                .options
                .iter()
                .map(|o| format_option_left(o.name.as_str(), o.kind).len())
                .max()
                .unwrap_or(0);
            for o in &self.options {
                let left = format_option_left(o.name.as_str(), o.kind);
                if o.help.is_empty() {
                    let _ = writeln!(s, "      {left}");
                } else {
                    let _ = writeln!(s, "      {left:<max_left$}  {}", o.help);
                }
            }
        }

        if !self.components.is_empty() {
            let _ = writeln!(s);
            let _ = writeln!(
                s,
                "COMPONENTS (use with --components / --with / --without):"
            );
            for c in &self.components {
                let marker = if c.required { " (required)" } else { "" };
                let _ = writeln!(s, "      {:<20}  {}{marker}", c.id, c.label);
            }
        }

        s
    }
}

fn format_option_left(name: &str, kind: OptionKind) -> String {
    match kind {
        OptionKind::Flag => format!("--{name}"),
        OptionKind::String => format!("--{name} <STRING>"),
        OptionKind::Int => format!("--{name} <INT>"),
        OptionKind::Bool => format!("--{name} <true|false>"),
    }
}
