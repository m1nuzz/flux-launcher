use std::collections::{HashMap, HashSet};
use std::path::Path;

use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ExprLit, ExprMethodCall, ItemConst, ItemFn, ItemMod, Lit, Meta};

#[derive(Default)]
struct Calls {
    keys: HashSet<String>,
    translated_protocol_arguments: Vec<String>,
}

fn literal(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) => Some(value.value()),
        _ => None,
    }
}

fn translation_call(call: &ExprCall) -> bool {
    matches!(&*call.func, Expr::Path(path) if path.path.segments.last().is_some_and(|segment| matches!(segment.ident.to_string().as_str(), "tr" | "text" | "format" | "count")))
}

impl<'ast> Visit<'ast> for Calls {
    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        // syn deliberately treats macro bodies as opaque. Settings rows live
        // inside vec![...], so parse its expression list instead of skipping it.
        if invocation.path.is_ident("vec") {
            if let Ok(array) = syn::parse_str::<syn::ExprArray>(&format!("[{}]", invocation.tokens))
            {
                for expression in &array.elems {
                    self.visit_expr(expression);
                }
            }
        }
        visit::visit_macro(self, invocation);
    }
    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        if item.attrs.iter().any(|attribute| matches!(&attribute.meta, Meta::List(list) if list.path.is_ident("cfg") && list.tokens.to_string() == "test")) {
            return;
        }
        visit::visit_item_mod(self, item);
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        if !item
            .attrs
            .iter()
            .any(|attribute| attribute.path().is_ident("test"))
        {
            visit::visit_item_fn(self, item);
        }
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if translation_call(call) {
            let count = match &*call.func {
                Expr::Path(path)
                    if path
                        .path
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == "count") =>
                {
                    2
                }
                _ => 1,
            };
            self.keys
                .extend(call.args.iter().take(count).filter_map(literal));
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_item_const(&mut self, item: &'ast ItemConst) {
        if matches!(
            item.ident.to_string().as_str(),
            "TAB_NAMES" | "TAB_DESCRIPTIONS"
        ) {
            if let Expr::Array(array) = &*item.expr {
                self.keys.extend(array.elems.iter().filter_map(literal));
            }
        }
        visit::visit_item_const(self, item);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        if matches!(
            call.method.to_string().as_str(),
            "join" | "arg" | "env" | "env_remove"
        ) {
            for argument in &call.args {
                let mut nested = Calls::default();
                nested.visit_expr(argument);
                if !nested.keys.is_empty() {
                    self.translated_protocol_arguments
                        .push(call.method.to_string());
                }
            }
        }
        visit::visit_expr_method_call(self, call);
    }
}

fn inspect_directory(path: &Path, calls: &mut Calls) {
    for entry in std::fs::read_dir(path).expect("source directory is readable") {
        let path = entry.expect("source entry is readable").path();
        if path.is_dir() {
            inspect_directory(&path, calls);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = std::fs::read_to_string(&path).expect("source is UTF-8");
            calls.visit_file(&syn::parse_file(&source).expect("source parses as Rust"));
        }
    }
}

#[test]
fn every_translation_call_and_dynamic_settings_caption_has_a_catalog_entry() {
    let mut calls = Calls::default();
    inspect_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut calls,
    );
    let catalog: HashMap<String, serde_json::Value> =
        serde_json::from_str(include_str!("../locales/translations.json")).unwrap();
    let mut missing: Vec<_> = calls
        .keys
        .iter()
        .filter(|key| !catalog.contains_key(*key))
        .collect();
    missing.sort_unstable();
    assert!(
        missing.is_empty(),
        "translation keys missing from catalog: {missing:?}"
    );
    assert!(
        calls.translated_protocol_arguments.is_empty(),
        "never translate path or process arguments: {:?}",
        calls.translated_protocol_arguments
    );
    assert!(
        calls.keys.len() > 150,
        "coverage must inspect the application, not just a fixture"
    );
}

#[test]
fn scanner_detects_escaped_missing_keys_and_translated_paths() {
    let source = r#"fn build() { label(crate::i18n::tr("\u{2713} Typo")); root.join(crate::i18n::tr("Plugins")); }"#;
    let mut calls = Calls::default();
    calls.visit_file(&syn::parse_file(source).unwrap());
    assert!(calls.keys.contains("\u{2713} Typo"));
    assert_eq!(calls.translated_protocol_arguments, ["join"]);
}
