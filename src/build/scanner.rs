use std::path::Path;

use anyhow::{Context, Result};
use syn::visit::Visit;

pub struct ScanResult {
    /// Source paths referenced via `source!(...)` inside `fn install`, or at
    /// top-level (which counts for both scopes).
    pub install_sources: Vec<String>,
    /// Source paths referenced via `source!(...)` inside `fn uninstall`, or at
    /// top-level.
    pub uninstall_sources: Vec<String>,
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
    install_sources: &'a mut Vec<String>,
    uninstall_sources: &'a mut Vec<String>,
    has_install_fn: &'a mut bool,
    has_uninstall_fn: &'a mut bool,
    current_fn: Option<String>,
}

impl SourceVisitor<'_> {
    fn push(&mut self, path: String) {
        match self.current_fn.as_deref() {
            Some("install") => {
                if !self.install_sources.contains(&path) {
                    self.install_sources.push(path);
                }
            }
            Some("uninstall") => {
                if !self.uninstall_sources.contains(&path) {
                    self.uninstall_sources.push(path);
                }
            }
            _ => {
                // Outside install/uninstall — add to both scopes
                if !self.install_sources.contains(&path) {
                    self.install_sources.push(path.clone());
                }
                if !self.uninstall_sources.contains(&path) {
                    self.uninstall_sources.push(path);
                }
            }
        }
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
            if let Some(s) = macro_single_str_arg(node) {
                self.push(s);
            }
        }

        syn::visit::visit_macro(self, node);
    }
}

/// Extract the single string literal argument from a `source!("path")`
/// macro invocation.
fn macro_single_str_arg(mac: &syn::Macro) -> Option<String> {
    let args = mac
        .parse_body_with(syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        .ok()?;

    if let Some(syn::Expr::Lit(syn::ExprLit {
        lit: syn::Lit::Str(s),
        ..
    })) = args.first()
    {
        Some(s.value())
    } else {
        None
    }
}
