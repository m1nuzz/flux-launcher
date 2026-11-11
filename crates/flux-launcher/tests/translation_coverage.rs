use std::collections::{HashMap, HashSet};
use std::path::Path;

use syn::visit::{self, Visit};
use syn::{
    Expr, ExprCall, ExprLit, ExprMethodCall, ExprStruct, ItemConst, ItemFn, ItemMod, Lit, Local,
    Meta,
};

#[derive(Default)]
struct Calls {
    keys: HashSet<String>,
    translated_protocol_arguments: Vec<String>,
    untranslated_status_formats: Vec<String>,
    raw_ui_strings: Vec<String>,
}

fn invariant_ui_text(value: &str) -> bool {
    // Product names, key names, units, command aliases, color values, and canonical application metadata are
    // intentionally invariant. The latter is localized at the result-row boundary.
    !value.chars().any(char::is_alphabetic)
        || matches!(
            value,
            "Flux Launcher"
                | "Flux"
                | "Google"
                | "Obsidian"
                | "Everything"
                | "Windows"
                | "GitHub"
                | "Rust"
                | "DIP"
                | "ms"
                | "px"
                | "R"
                | "G"
                | "B"
                | "H"
                | "S"
                | "V"
                | "Ctrl"
                | "Alt"
                | "Shift"
                | "RGB"
                | "HSV"
                | "HEX"
                | "Hex"
                | "Application • Start Menu"
                | "Application • App Paths"
                | "Application • {source}"
                | "Obsidian • {}"
                | "Obsidian • {} • {}"
                | "Flux Launcher v{version} • github.com/m1nuzz/flux-launcher"
                | "{rank}"
                | "g"
                | "ob"
                | "#4C8BF4"
        )
}

#[derive(Default)]
struct RawUiText(Vec<String>);

impl<'ast> Visit<'ast> for RawUiText {
    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if !translation_call(call) {
            visit::visit_expr_call(self, call);
        }
    }

    fn visit_expr_lit(&mut self, expression: &'ast ExprLit) {
        if let Lit::Str(value) = &expression.lit {
            let value = value.value();
            if !invariant_ui_text(&value) {
                self.0.push(value);
            }
        }
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        if invocation.path.is_ident("format") {
            use syn::parse::Parser;
            let parser = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated;
            if let Ok(arguments) = parser.parse2(invocation.tokens.clone()) {
                if let Some(value) = arguments.first().and_then(literal) {
                    if !invariant_ui_text(&value) {
                        self.0.push(value);
                    }
                }
            }
        }
    }
}

#[derive(Default)]
struct StatusFormats(Vec<String>);

impl<'ast> Visit<'ast> for StatusFormats {
    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        if invocation.path.is_ident("format") {
            self.0.push(invocation.tokens.to_string());
        }
        visit::visit_macro(self, invocation);
    }
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
    fn visit_expr_struct(&mut self, expression: &'ast ExprStruct) {
        for field in &expression.fields {
            if matches!(&field.member, syn::Member::Named(name) if name == "subtitle") {
                let mut raw = RawUiText::default();
                raw.visit_expr(&field.expr);
                self.raw_ui_strings.extend(raw.0);
            }
            if matches!(&field.member, syn::Member::Named(name) if name == "status") {
                let mut formats = StatusFormats::default();
                formats.visit_expr(&field.expr);
                self.untranslated_status_formats.extend(formats.0);
            }
        }
        visit::visit_expr_struct(self, expression);
    }

    fn visit_local(&mut self, local: &'ast Local) {
        if matches!(&local.pat, syn::Pat::Ident(name) if name.ident == "status") {
            if let Some(initializer) = &local.init {
                let mut formats = StatusFormats::default();
                formats.visit_expr(&initializer.expr);
                self.untranslated_status_formats.extend(formats.0);
            }
        }
        visit::visit_local(self, local);
    }

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
        if let Expr::Path(path) = &*call.func {
            let name = path
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default();
            let element = path.path.segments.iter().any(|segment| {
                matches!(segment.ident.to_string().as_str(), "Element" | "MenuItem")
            });
            let (skip, count) = match name.as_str() {
                "settings_group" | "settings_section_header" => (0, 1),
                "setting_row_desc" if element => (0, 2),
                "text_input" if element => (1, 1),
                "label" | "button" | "checkbox" | "nav_row" | "setting_row" | "new" if element => {
                    (0, 1)
                }
                _ => (0, 0),
            };
            for argument in call.args.iter().skip(skip).take(count) {
                let mut raw = RawUiText::default();
                raw.visit_expr(argument);
                self.raw_ui_strings.extend(raw.0);
            }
        }
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
            "TAB_NAMES" | "TAB_DESCRIPTIONS" | "APPLICATION_SUBTITLES"
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
            "tooltip" | "title" | "subtitle" | "placeholder" | "hint" | "description"
        ) {
            if let Some(argument) = call.args.first() {
                let mut raw = RawUiText::default();
                raw.visit_expr(argument);
                self.raw_ui_strings.extend(raw.0);
            }
        }
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
    assert!(
        calls.untranslated_status_formats.is_empty(),
        "provider status formats must use the catalog: {:?}",
        calls.untranslated_status_formats
    );
    calls.raw_ui_strings.sort();
    calls.raw_ui_strings.dedup();
    assert!(
        calls.raw_ui_strings.is_empty(),
        "UI text must use the catalog: {:?}",
        calls.raw_ui_strings
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

#[test]
fn scanner_detects_raw_status_formats_and_accepts_localized_formats() {
    let source = r#"fn build() {
        let status = if failed { format!("{count} plugins did not respond") } else { i18n::tr("Ready").into() };
        Response { status: format!("Host restarted: {error}") };
        Response { status: i18n::format("Unable to start winget: {error}", &[("error", error.to_string())]) };
    }"#;
    let mut calls = Calls::default();
    calls.visit_file(&syn::parse_file(source).unwrap());
    assert_eq!(calls.untranslated_status_formats.len(), 2);
    assert!(calls.keys.contains("Unable to start winget: {error}"));
}

#[test]
fn scanner_rejects_raw_tooltips_labels_and_subtitle_formats() {
    let source = r#"fn build() {
        Element::label("Untranslated label").tooltip("Untranslated tooltip");
        Result { subtitle: format!("Untranslated {name}") };
        Element::label(i18n::tr("Translated")).tooltip(i18n::text("Also translated"));
        Element::label("Flux Launcher");
        Element::setting_row_desc(i18n::tr("Safe caption"), "Untranslated setting description", control);
    }"#;
    let mut calls = Calls::default();
    calls.visit_file(&syn::parse_file(source).unwrap());
    calls.raw_ui_strings.sort();
    assert_eq!(
        calls.raw_ui_strings,
        [
            "Untranslated label",
            "Untranslated setting description",
            "Untranslated tooltip",
            "Untranslated {name}"
        ]
    );
    assert!(calls.keys.contains("Translated"));
}
