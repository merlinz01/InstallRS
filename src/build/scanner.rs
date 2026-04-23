use std::path::Path;

use anyhow::{Context, Result};
use syn::visit::Visit;

/// A `source!(...)` invocation: the embedded path plus any build-time-only
/// options (e.g. `ignore = [...]`, `features = [...]`). Dedup is by path;
/// `ignore` merges as union. `features` merges as: empty (unconditional)
/// wins; otherwise union — meaning the source is active if *any* listed
/// feature is enabled.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SourceRef {
    pub path: String,
    pub ignore: Vec<String>,
    /// Feature gates. Empty = always active. Non-empty = active iff any
    /// feature in this list is in the builder's active-feature set.
    pub features: Vec<String>,
}

pub struct ScanResult {
    /// Source refs from `source!(...)` inside `fn install`, or at top-level
    /// (which counts for both scopes).
    pub install_sources: Vec<SourceRef>,
    pub uninstall_sources: Vec<SourceRef>,
    pub has_install_fn: bool,
    pub has_uninstall_fn: bool,
}

pub fn scan_source_dir(src_dir: &Path) -> Result<ScanResult> {
    let mut result = ScanResult {
        install_sources: Vec::new(),
        uninstall_sources: Vec::new(),
        has_install_fn: false,
        has_uninstall_fn: false,
    };

    for entry in walkdir::WalkDir::new(src_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "rs").unwrap_or(false))
    {
        let path = entry.path();
        log::trace!("Scanning source file: {}", path.display());
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read source file: {}", path.display()))?;

        let file = match syn::parse_file(&source) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("Failed to parse {}: {e}", path.display());
                continue;
            }
        };

        let mut visitor = SourceVisitor {
            install_sources: &mut result.install_sources,
            uninstall_sources: &mut result.uninstall_sources,
            has_install_fn: &mut result.has_install_fn,
            has_uninstall_fn: &mut result.has_uninstall_fn,
            current_fn: None,
        };
        visitor.visit_file(&file);
    }

    Ok(result)
}

struct SourceVisitor<'a> {
    install_sources: &'a mut Vec<SourceRef>,
    uninstall_sources: &'a mut Vec<SourceRef>,
    has_install_fn: &'a mut bool,
    has_uninstall_fn: &'a mut bool,
    current_fn: Option<String>,
}

impl SourceVisitor<'_> {
    fn push(&mut self, s: SourceRef) {
        match self.current_fn.as_deref() {
            Some("install") => merge_or_push(self.install_sources, s),
            Some("uninstall") => merge_or_push(self.uninstall_sources, s),
            _ => {
                // Outside install/uninstall — add to both scopes
                merge_or_push(self.install_sources, s.clone());
                merge_or_push(self.uninstall_sources, s);
            }
        }
    }
}

fn merge_or_push(list: &mut Vec<SourceRef>, new: SourceRef) {
    if let Some(existing) = list.iter_mut().find(|r| r.path == new.path) {
        for pat in new.ignore {
            if !existing.ignore.contains(&pat) {
                existing.ignore.push(pat);
            }
        }
        // features: empty (unconditional) wins. Otherwise union.
        if existing.features.is_empty() || new.features.is_empty() {
            existing.features.clear();
        } else {
            for f in new.features {
                if !existing.features.contains(&f) {
                    existing.features.push(f);
                }
            }
        }
    } else {
        list.push(new);
    }
}

impl<'ast> Visit<'ast> for SourceVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let name = node.sig.ident.to_string();
        if name == "install" {
            *self.has_install_fn = true;
        } else if name == "uninstall" {
            *self.has_uninstall_fn = true;
        }
        let prev = self.current_fn.take();
        self.current_fn = Some(name);
        syn::visit::visit_item_fn(self, node);
        self.current_fn = prev;
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        let name = node
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();

        if name == "source" {
            if let Some(s) = parse_source_macro(node) {
                self.push(s);
            }
        }

        syn::visit::visit_macro(self, node);
    }
}

/// Parse `source!("path" [, key = value]* )`. Returns None if the macro body
/// doesn't start with a string literal (malformed — scanner skips it).
fn parse_source_macro(mac: &syn::Macro) -> Option<SourceRef> {
    let args = mac
        .parse_body_with(syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        .ok()?;

    let mut iter = args.iter();

    let path = match iter.next()? {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => s.value(),
        _ => return None,
    };

    let mut out = SourceRef {
        path,
        ignore: Vec::new(),
        features: Vec::new(),
    };

    for arg in iter {
        // Key-value args are parsed by syn as `Expr::Assign { left, right, .. }`
        // with `left` being a path expr of a single ident.
        if let syn::Expr::Assign(syn::ExprAssign { left, right, .. }) = arg {
            let key = match left.as_ref() {
                syn::Expr::Path(p) if p.path.segments.len() == 1 => {
                    p.path.segments[0].ident.to_string()
                }
                _ => {
                    log::warn!("source!: ignoring unrecognized option form");
                    continue;
                }
            };
            match key.as_str() {
                "ignore" => {
                    if let Some(items) = extract_str_array(right) {
                        out.ignore = items;
                    } else {
                        log::warn!(
                            "source!({:?}, ignore = ...): value must be an array of string literals",
                            out.path
                        );
                    }
                }
                "features" => {
                    if let Some(items) = extract_str_array(right) {
                        out.features = items;
                    } else {
                        log::warn!(
                            "source!({:?}, features = ...): value must be an array of string literals",
                            out.path
                        );
                    }
                }
                _ => log::warn!(
                    "source!({:?}, {key} = ...): unknown option, ignoring",
                    out.path
                ),
            }
        }
    }

    Some(out)
}

fn extract_str_array(expr: &syn::Expr) -> Option<Vec<String>> {
    if let syn::Expr::Array(arr) = expr {
        let mut out = Vec::with_capacity(arr.elems.len());
        for el in &arr.elems {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = el
            {
                out.push(s.value());
            } else {
                return None;
            }
        }
        Some(out)
    } else {
        None
    }
}
