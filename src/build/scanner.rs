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
            in_dir: String::new(),
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
    in_dir: String,
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

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let method = node.method.to_string();

        match method.as_str() {
            "set_in_dir" => {
                if let Some(s) = first_string_arg(&node.args) {
                    self.in_dir = s;
                }
            }
            "file" | "include_file" => {
                if let Some(s) = first_string_arg(&node.args) {
                    let path = join_in_dir(&self.in_dir, &s);
                    if !self.included_files.contains(&path) {
                        self.included_files.push(path);
                    }
                }
            }
            "dir" | "include_dir" => {
                if let Some(s) = first_string_arg(&node.args) {
                    let path = join_in_dir(&self.in_dir, &s);
                    if !self.included_dirs.contains(&path) {
                        self.included_dirs.push(path);
                    }
                }
            }
            _ => {}
        }

        syn::visit::visit_expr_method_call(self, node);
    }
}

fn first_string_arg(args: &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>) -> Option<String> {
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

fn join_in_dir(in_dir: &str, path: &str) -> String {
    if in_dir.is_empty() || path.starts_with('/') || path.contains(':') {
        path.to_string()
    } else {
        format!("{}/{}", in_dir.trim_end_matches('/'), path)
    }
}
