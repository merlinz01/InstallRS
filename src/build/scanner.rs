use std::path::Path;

use anyhow::{Context, Result};
use syn::visit::Visit;

pub struct ScanResult {
    pub included_files: Vec<String>,
    pub included_dirs: Vec<String>,
    pub has_install_fn: bool,
    pub has_uninstall_fn: bool,
}

pub fn scan_source_dir(src_dir: &Path) -> Result<ScanResult> {
    let mut result = ScanResult {
        included_files: Vec::new(),
        included_dirs: Vec::new(),
        has_install_fn: false,
        has_uninstall_fn: false,
    };

    for entry in walkdir::WalkDir::new(src_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "rs").unwrap_or(false))
    {
        let path = entry.path();
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
            included_files: &mut result.included_files,
            included_dirs: &mut result.included_dirs,
            has_install_fn: &mut result.has_install_fn,
            has_uninstall_fn: &mut result.has_uninstall_fn,
        };
        visitor.visit_file(&file);
    }

    Ok(result)
}

struct SourceVisitor<'a> {
    included_files: &'a mut Vec<String>,
    included_dirs: &'a mut Vec<String>,
    has_install_fn: &'a mut bool,
    has_uninstall_fn: &'a mut bool,
}

impl<'ast, 'a> Visit<'ast> for SourceVisitor<'a> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let name = node.sig.ident.to_string();
        if name == "install" {
            *self.has_install_fn = true;
        } else if name == "uninstall" {
            *self.has_uninstall_fn = true;
        }
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        let name = node.mac.path.segments.last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();

        match name.as_str() {
            "file" => {
                // file!(installer, "source/path", "dest") — source is the 2nd arg
                if let Some(s) = macro_second_str_arg(&node.mac) {
                    if !self.included_files.contains(&s) {
                        self.included_files.push(s);
                    }
                }
            }
            "dir" => {
                // dir!(installer, "source/dir", "dest") — source is the 2nd arg
                if let Some(s) = macro_second_str_arg(&node.mac) {
                    if !self.included_dirs.contains(&s) {
                        self.included_dirs.push(s);
                    }
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
