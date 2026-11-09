use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::LazyLock;

use flux_core::Locale;

type Catalog = HashMap<String, [String; 4]>;
static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../locales/translations.json"))
        .expect("embedded translations must be valid")
});
static LANGUAGE: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn init(language: Locale) {
    LANGUAGE.store(language.index(), Ordering::Relaxed);
}

pub(crate) fn current() -> Locale {
    Locale::from_index(LANGUAGE.load(Ordering::Relaxed))
}

pub(crate) fn translate(language: Locale, english: &'static str) -> &'static str {
    let Some(index) = language.index().checked_sub(1) else {
        return english;
    };
    CATALOG
        .get(english)
        .map(|translations| translations[index].as_str())
        .unwrap_or(english)
}

pub(crate) fn tr(english: &'static str) -> &'static str {
    translate(current(), english)
}

/// Localize Flux-owned result captions without changing provider data or ranking.
pub(crate) fn text(english: &str) -> String {
    let Some(index) = current().index().checked_sub(1) else {
        return english.to_owned();
    };
    CATALOG
        .get(english)
        .map(|translations| translations[index].clone())
        .unwrap_or_else(|| english.to_owned())
}

pub(crate) fn format(english: &'static str, values: &[(&str, String)]) -> String {
    let mut output = tr(english).to_owned();
    for (key, value) in values {
        output = output.replace(&format!("{{{key}}}"), value);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_are_complete_and_preserve_placeholders() {
        fn placeholders(value: &str) -> Vec<&str> {
            let mut keys: Vec<_> = value
                .split('{')
                .skip(1)
                .filter_map(|part| part.split_once('}').map(|(key, _)| key))
                .collect();
            keys.sort_unstable();
            keys
        }
        for (key, translations) in CATALOG.iter() {
            for translated in translations {
                assert!(!translated.trim().is_empty(), "missing translation: {key}");
                assert_eq!(
                    placeholders(key),
                    placeholders(translated),
                    "placeholder mismatch: {key}"
                );
            }
        }
        assert_eq!(translate(Locale::English, "Language"), "Language");
        assert_eq!(translate(Locale::SimplifiedChinese, "Language"), "语言");
        assert_eq!(
            translate(Locale::Japanese, "untranslated plugin text"),
            "untranslated plugin text"
        );
    }
}
