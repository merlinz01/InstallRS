use std::path::Path;

use anyhow::{Context, Result};
use syn::visit::Visit;

pub struct ScanResult {
    pub install_files: Vec<String>,
    pub install_dirs: Vec<String>,
    pub uninstall_files: Vec<String>,
    pub uninstall_dirs: Vec<String>,
    pub has_install_fn: bool,
    pub has_uninstall_fn: bool,
}

pub fn scan_source_dir(src_dir: &Path) -> Result<ScanResult> {
    let mut result = ScanResult {
        install_files: Vec::new(),
        install_dirs: Vec::new(),
        uninstall_files: Vec::new(),
        uninstall_dirs: Vec::new(),
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
            install_files: &mut result.install_files,
            install_dirs: &mut result.install_dirs,
            uninstall_files: &mut result.uninstall_files,
            uninstall_dirs: &mut result.uninstall_dirs,
            has_install_fn: &mut result.has_install_fn,
            has_uninstall_fn: &mut result.has_uninstall_fn,
            current_fn: None,
        };
        visitor.visit_file(&file);
    }

    Ok(result)
}

struct SourceVisitor<'a> {
    install_files: &'a mut Vec<String>,
    install_dirs: &'a mut Vec<String>,
    uninstall_files: &'a mut Vec<String>,
    uninstall_dirs: &'a mut Vec<String>,
    has_install_fn: &'a mut bool,
    has_uninstall_fn: &'a mut bool,
    current_fn: Option<String>,
}

impl SourceVisitor<'_> {
    fn push_file(&mut self, path: String) {
        match self.current_fn.as_deref() {
            Some("install") => {
                if !self.install_files.contains(&path) {
                    self.install_files.push(path);
                }
            }
            Some("uninstall") => {
                if !self.uninstall_files.contains(&path) {
                    self.uninstall_files.push(path);
                }
            }
            _ => {
                // Outside install/uninstall — add to both
                if !self.install_files.contains(&path) {
                    self.install_files.push(path.clone());
                }
                if !self.uninstall_files.contains(&path) {
                    self.uninstall_files.push(path);
                }
            }
        }
    }

    fn push_dir(&mut self, path: String) {
        match self.current_fn.as_deref() {
            Some("install") => {
                if !self.install_dirs.contains(&path) {
                    self.install_dirs.push(path);
                }
            }
            Some("uninstall") => {
                if !self.uninstall_dirs.contains(&path) {
                    self.uninstall_dirs.push(path);
                }
            }
            _ => {
                if !self.install_dirs.contains(&path) {
                    self.install_dirs.push(path.clone());
                }
                if !self.uninstall_dirs.contains(&path) {
                    self.uninstall_dirs.push(path);
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

    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        let name = node.mac.path.segments.last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();

        match name.as_str() {
            "file" => {
                if let Some(s) = macro_second_str_arg(&node.mac) {
                    self.push_file(s);
                }
            }
            "dir" => {
                if let Some(s) = macro_second_str_arg(&node.mac) {
                    self.push_dir(s);
                }
            }
            _ => {}
        }

        syn::visit::visit_expr_macro(self, node);
    }
}

/// Extract the second string literal from a comma-separated macro argument list.
/// Used to pull the source path from `file!(installer, "path", dest)`.
/// Extract the second string literal from a `file!(expr, "path", expr)` or
/// `dir!(expr, "path", expr)` macro invocation.
fn macro_second_str_arg(mac: &syn::Macro) -> Option<String> {
    let args = mac
        .parse_body_with(
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
        )
        .ok()?;

    if let Some(syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. })) = args.iter().nth(1) {
        Some(s.value())
    } else {
        None
    }
}
