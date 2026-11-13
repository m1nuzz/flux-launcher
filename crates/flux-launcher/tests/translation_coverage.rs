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
    bindings: HashMap<String, Expr>,
}

fn invariant_ui_text(value: &str) -> bool {
    // Only products, key names, units, command aliases, color values, and
    // user-data templates are invariant. Application metadata is not exempt.
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
                | "Obsidian • {}"
                | "Obsidian • {} • {}"
                | "Flux Launcher v{version} • github.com/m1nuzz/flux-launcher"
                | "{rank}"
                | "g"
                | "ob"
                | "#4C8BF4"
        )
}

struct RawUiText<'a> {
    values: Vec<String>,
    bindings: &'a HashMap<String, Expr>,
    visiting: HashSet<String>,
}

impl<'a> RawUiText<'a> {
    fn new(bindings: &'a HashMap<String, Expr>) -> Self {
        Self {
            values: Vec::new(),
            bindings,
            visiting: HashSet::new(),
        }
    }
}

impl<'ast> Visit<'ast> for RawUiText<'ast> {
    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if !translation_call(call) {
            visit::visit_expr_call(self, call);
        }
    }

    fn visit_expr_lit(&mut self, expression: &'ast ExprLit) {
        if let Lit::Str(value) = &expression.lit {
            let value = value.value();
            if !invariant_ui_text(&value) {
                self.values.push(value);
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
                        self.values.push(value);
                    }
                }
            }
        }
    }

    fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
        if let Some(name) = expression.path.get_ident().map(ToString::to_string) {
            let bindings = self.bindings;
            if let Some(value) = bindings.get(&name) {
                if self.visiting.insert(name.clone()) {
                    self.visit_expr(value);
                    self.visiting.remove(&name);
                }
            }
        }
    }

    fn visit_expr_if(&mut self, expression: &'ast syn::ExprIf) {
        if let Some(syn::Stmt::Expr(value, _)) = expression.then_branch.stmts.last() {
            self.visit_expr(value);
        }
        if let Some((_, value)) = &expression.else_branch {
            self.visit_expr(value);
        }
    }

    fn visit_expr_match(&mut self, expression: &'ast syn::ExprMatch) {
        for arm in &expression.arms {
            self.visit_expr(&arm.body);
        }
    }
}

// Collect returned captions, not match patterns or conditions (which may be IDs).
fn translation_keys(
    expression: &Expr,
    bindings: &HashMap<String, Expr>,
    visiting: &mut HashSet<String>,
) -> Vec<String> {
    if let Some(value) = literal(expression) {
        return vec![value];
    }
    match expression {
        Expr::Match(value) => value
            .arms
            .iter()
            .flat_map(|arm| translation_keys(&arm.body, bindings, visiting))
            .collect(),
        Expr::If(value) => {
            let mut keys = value
                .then_branch
                .stmts
                .last()
                .and_then(|statement| match statement {
                    syn::Stmt::Expr(value, _) => Some(translation_keys(value, bindings, visiting)),
                    _ => None,
                })
                .unwrap_or_default();
            if let Some((_, other)) = &value.else_branch {
                keys.extend(translation_keys(other, bindings, visiting));
            }
            keys
        }
        Expr::Block(value) => value
            .block
            .stmts
            .last()
            .and_then(|statement| match statement {
                syn::Stmt::Expr(value, _) => Some(translation_keys(value, bindings, visiting)),
                _ => None,
            })
            .unwrap_or_default(),
        Expr::Paren(value) => translation_keys(&value.expr, bindings, visiting),
        Expr::Path(value) => {
            if let Some(name) = value.path.get_ident().map(ToString::to_string) {
                if visiting.insert(name.clone()) {
                    let keys = bindings
                        .get(&name)
                        .map(|value| translation_keys(value, bindings, visiting))
                        .unwrap_or_default();
                    visiting.remove(&name);
                    return keys;
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
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
                let mut raw = RawUiText::new(&self.bindings);
                raw.visit_expr(&field.expr);
                self.raw_ui_strings.extend(raw.values);
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
        let pattern = match &local.pat {
            syn::Pat::Type(value) => &*value.pat,
            value => value,
        };
        if let (syn::Pat::Ident(name), Some(initializer)) = (pattern, &local.init) {
            self.bindings
                .insert(name.ident.to_string(), (*initializer.expr).clone());
        }
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
            let previous = std::mem::take(&mut self.bindings);
            visit::visit_item_fn(self, item);
            self.bindings = previous;
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
                matches!(
                    segment.ident.to_string().as_str(),
                    "Element" | "MenuItem" | "TrayMenuItem"
                )
            });
            let (skip, count) = match name.as_str() {
                "settings_group" | "settings_section_header" => (0, 1),
                "setting_row_desc" if element => (0, 2),
                "text_input" if element => (1, 1),
                "label" | "label_signal" | "rich_signal" | "button" | "checkbox" | "nav_row"
                | "setting_row" | "new" | "item" | "check"
                    if element =>
                {
                    (0, 1)
                }
                _ => (0, 0),
            };
            for argument in call.args.iter().skip(skip).take(count) {
                let mut raw = RawUiText::new(&self.bindings);
                raw.visit_expr(argument);
                self.raw_ui_strings.extend(raw.values);
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
            for argument in call.args.iter().take(count) {
                self.keys.extend(translation_keys(
                    argument,
                    &self.bindings,
                    &mut HashSet::new(),
                ));
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_item_const(&mut self, item: &'ast ItemConst) {
        // Filesystem exclusions are data, not captions. All other string
        // arrays must be covered regardless of their names.
        const NON_UI_STRING_ARRAYS: &[&str] = &["NEVER_WALKED"];
        let string_array = matches!(&*item.ty, syn::Type::Array(array)
            if matches!(&*array.elem, syn::Type::Reference(reference)
                if matches!(&*reference.elem, syn::Type::Path(path) if path.path.is_ident("str"))));
        if string_array && !NON_UI_STRING_ARRAYS.contains(&item.ident.to_string().as_str()) {
            if let Expr::Array(array) = &*item.expr {
                self.keys.extend(
                    array
                        .elems
                        .iter()
                        .filter_map(literal)
                        .filter(|value| !invariant_ui_text(value)),
                );
            }
        }
        visit::visit_item_const(self, item);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        if matches!(
            call.method.to_string().as_str(),
            "tooltip" | "title" | "subtitle" | "placeholder" | "hint" | "description" | "toast_ok"
        ) {
            if let Some(argument) = call.args.first() {
                let mut raw = RawUiText::new(&self.bindings);
                raw.visit_expr(argument);
                self.raw_ui_strings.extend(raw.values);
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

fn system_caption_keys(source: &str) -> HashSet<String> {
    let file = syn::parse_file(source).expect("system results parse as Rust");
    let item = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Const(item) if item.ident == "SYSTEM_RESULT_SPECS" => Some(item),
            _ => None,
        })
        .expect("system result specification constant exists");
    let Expr::Reference(reference) = &*item.expr else {
        panic!("expected system result slice");
    };
    let Expr::Array(array) = &*reference.expr else {
        panic!("expected system result array");
    };
    let mut keys = HashSet::new();
    for value in &array.elems {
        let Expr::Struct(spec) = value else {
            panic!("expected system result specification");
        };
        for name in ["title", "subtitle"] {
            let field = spec
                .fields
                .iter()
                .find(|field| matches!(&field.member, syn::Member::Named(member) if member == name))
                .expect("system result has both captions");
            let caption = literal(&field.expr).expect("system captions are literal keys");
            if !matches!(
                caption.as_str(),
                "PowerShell" | "Windows PowerShell" | "Wi-Fi" | "VPN"
            ) {
                keys.insert(caption);
            }
        }
    }
    keys
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
    for invariant in ["PowerShell", "Windows PowerShell", "Wi-Fi", "VPN"] {
        if let Some(translations) = catalog.get(invariant) {
            assert!(
                translations
                    .as_object()
                    .unwrap()
                    .values()
                    .all(|value| value.as_str() == Some(invariant)),
                "technical names must remain invariant: {invariant}"
            );
        }
    }
    let update = catalog["Windows Update"].as_object().unwrap();
    for (locale, title) in update {
        assert!(
            catalog["Windows Settings • Windows Update"][locale]
                .as_str()
                .unwrap()
                .ends_with(title.as_str().unwrap()),
            "Windows Update title/subtitle disagree: {locale}"
        );
    }
    assert_eq!(update["de"], "Windows Update");
    assert_eq!(update["fr"], "Windows Update");
    // Official names (Parametry; Tsentr onovlennia Windows): https://www.microsoft.com/uk-ua/windows/get-windows-11
    assert_eq!(
        catalog["Settings"]["uk-UA"],
        "\u{41f}\u{430}\u{440}\u{430}\u{43c}\u{435}\u{442}\u{440}\u{438}"
    );
    assert_eq!(
        catalog["Windows Settings"]["uk-UA"],
        "\u{41f}\u{430}\u{440}\u{430}\u{43c}\u{435}\u{442}\u{440}\u{438} Windows"
    );
    let mut windows_settings_subtitles = 0;
    for (key, translations) in &catalog {
        if key.starts_with("Windows Settings • ") {
            windows_settings_subtitles += 1;
            let ukrainian = translations
                .get("uk-UA")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| panic!("missing uk-UA for {key}"));
            assert!(ukrainian.starts_with("\u{41f}\u{430}\u{440}\u{430}\u{43c}\u{435}\u{442}\u{440}\u{438} Windows \u{2022} "),
                "Ukrainian Windows Settings app name must be consistent: {key}");
        }
    }
    assert!(
        windows_settings_subtitles >= 10,
        "Windows Settings prefix coverage needs at least 10 keys, found {windows_settings_subtitles}"
    );
    assert_eq!(catalog["Accessibility"]["uk-UA"], "\u{421}\u{43f}\u{435}\u{446}\u{456}\u{430}\u{43b}\u{44c}\u{43d}\u{456} \u{43c}\u{43e}\u{436}\u{43b}\u{438}\u{432}\u{43e}\u{441}\u{442}\u{456}");
    assert_eq!(catalog["Windows Settings • Accessibility"]["uk-UA"], "\u{41f}\u{430}\u{440}\u{430}\u{43c}\u{435}\u{442}\u{440}\u{438} Windows \u{2022} \u{421}\u{43f}\u{435}\u{446}\u{456}\u{430}\u{43b}\u{44c}\u{43d}\u{456} \u{43c}\u{43e}\u{436}\u{43b}\u{438}\u{432}\u{43e}\u{441}\u{442}\u{456}");
    assert_eq!(catalog["Windows Settings • System"]["uk-UA"], "\u{41f}\u{430}\u{440}\u{430}\u{43c}\u{435}\u{442}\u{440}\u{438} Windows \u{2022} \u{421}\u{438}\u{441}\u{442}\u{435}\u{43c}\u{430}");
    assert_eq!(catalog["Settings applied"]["uk-UA"], "\u{41f}\u{430}\u{440}\u{430}\u{43c}\u{435}\u{442}\u{440}\u{438} \u{437}\u{430}\u{441}\u{442}\u{43e}\u{441}\u{43e}\u{432}\u{430}\u{43d}\u{43e}");
    assert_eq!(
        catalog["Flux Settings"]["uk-UA"],
        "\u{41f}\u{430}\u{440}\u{430}\u{43c}\u{435}\u{442}\u{440}\u{438} Flux"
    );
    assert_eq!(update["uk-UA"], "\u{426}\u{435}\u{43d}\u{442}\u{440} \u{43e}\u{43d}\u{43e}\u{432}\u{43b}\u{435}\u{43d}\u{43d}\u{44f} Windows");
    assert_eq!(catalog["Windows Settings • Windows Update"]["uk-UA"],
        "\u{41f}\u{430}\u{440}\u{430}\u{43c}\u{435}\u{442}\u{440}\u{438} Windows \u{2022} \u{426}\u{435}\u{43d}\u{442}\u{440} \u{43e}\u{43d}\u{43e}\u{432}\u{43b}\u{435}\u{43d}\u{43d}\u{44f} Windows");
    calls.keys.extend(system_caption_keys(include_str!(
        "../../flux-core/src/system_results.rs"
    )));
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
fn system_caption_guard_collects_both_captions_and_ignores_only_classified_names() {
    let keys = system_caption_keys(
        r#"const SYSTEM_RESULT_SPECS: &[SystemResultSpec] = &[
        SystemResultSpec { title: "New title", subtitle: "New subtitle", target: "protocol:target" },
        SystemResultSpec { title: "PowerShell", subtitle: "Windows PowerShell" },
        SystemResultSpec { title: "Wi-Fi", subtitle: "VPN" },
    ];"#,
    );
    assert_eq!(
        keys,
        HashSet::from(["New title".to_owned(), "New subtitle".to_owned()])
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
        ctx.toast_ok("Untranslated toast");
        ctx.toast_ok(i18n::tr("Translated toast"));
        Result { subtitle: format!("Untranslated {name}") };
        Result { subtitle: format!("Application • {source}") };
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
            "Application • {source}",
            "Untranslated label",
            "Untranslated setting description",
            "Untranslated toast",
            "Untranslated tooltip",
            "Untranslated {name}"
        ]
    );
    assert!(calls.keys.contains("Translated"));
}

#[test]
fn scanner_checks_tray_labels_and_bound_signal_initial_text() {
    let source = r#"fn build() {
        TrayMenuItem::item("Raw tray item", |_| {});
        TrayMenuItem::check("Raw tray check", enabled, |_| {});
        let text = signal(String::from("Raw signal"));
        let forwarded = text;
        Element::label_signal(forwarded);
        let rich = signal(RichDoc::plain("Raw rich document"));
        Element::rich_signal(rich);
        let safe = signal(i18n::tr("Translated signal"));
        Element::label_signal(safe);
    }"#;
    let mut calls = Calls::default();
    calls.visit_file(&syn::parse_file(source).unwrap());
    calls.raw_ui_strings.sort();
    assert_eq!(
        calls.raw_ui_strings,
        [
            "Raw rich document",
            "Raw signal",
            "Raw tray check",
            "Raw tray item"
        ]
    );
}

#[test]
fn scanner_collects_returned_dynamic_translation_keys_without_patterns() {
    let source = r#"fn build() {
        Element::label(i18n::tr(match code { "protocol-id" => "Known caption", _ => { "Missing match caption" } }));
        let caption = if failed { "Missing conditional caption" } else { "Known caption" };
        Element::label(i18n::tr(caption));
    }"#;
    let mut calls = Calls::default();
    calls.visit_file(&syn::parse_file(source).unwrap());
    let known = HashSet::from(["Known caption".to_owned()]);
    let mut missing: Vec<_> = calls.keys.difference(&known).map(String::as_str).collect();
    missing.sort();
    assert_eq!(
        missing,
        ["Missing conditional caption", "Missing match caption"]
    );
    assert!(!calls.keys.contains("protocol-id"));
    assert!(calls.raw_ui_strings.is_empty());
}

#[test]
fn scanner_collects_caption_arrays_without_relying_on_their_names() {
    let mut calls = Calls::default();
    let item: ItemConst = syn::parse_str(
        r#"const RENAMED_CAPTIONS: [&str; 4] = ["First caption", "Second caption", "\u{e713}", "Flux Launcher"];"#,
    )
    .unwrap();
    calls.visit_item_const(&item);
    assert_eq!(
        calls.keys,
        HashSet::from(["First caption".to_owned(), "Second caption".to_owned()])
    );

    let source = r#"
        const NEVER_WALKED: [&str; 1] = ["System Volume Information"];
        const TAB_GLYPHS: [&str; 1] = ["\u{e713}"];
        const NUMBERS: [u8; 2] = [1, 2];
        const SCALAR: &str = "Not an array";
    "#;
    calls.visit_file(&syn::parse_file(source).unwrap());
    assert_eq!(calls.keys.len(), 2);
}

#[test]
fn scanner_rejects_raw_conditional_and_match_captions_without_conditions_or_patterns() {
    let source = r#"fn build() {
        Element::label(if code == "condition-id" { "Raw if A" } else { "Raw if B" });
        Element::label(match "scrutinee-id" {
            "pattern-id" => "Raw match A",
            _ if code == "guard-id" => { "Raw match B" },
            _ => "Raw match C",
        });
    }"#;
    let mut calls = Calls::default();
    calls.visit_file(&syn::parse_file(source).unwrap());
    calls.raw_ui_strings.sort();
    assert_eq!(
        calls.raw_ui_strings,
        [
            "Raw if A",
            "Raw if B",
            "Raw match A",
            "Raw match B",
            "Raw match C"
        ]
    );
}
