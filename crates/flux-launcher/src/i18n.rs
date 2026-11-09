use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::LazyLock;

use flux_core::Locale;

type Catalog = HashMap<String, HashMap<String, String>>;
static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../locales/translations.json"))
        .expect("embedded translations must be valid")
});
static LANGUAGE: AtomicUsize = AtomicUsize::new(0);
static LANGUAGE_NAMES: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../locales/languages.json"))
        .expect("embedded language names must be valid")
});

#[cfg(test)]
thread_local! {
    static TEST_LANGUAGE: std::cell::Cell<Option<Locale>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn native_name(language: Locale) -> &'static str {
    LANGUAGE_NAMES
        .get(language.code())
        .map(String::as_str)
        .unwrap_or("English")
}

pub(crate) fn init(language: Locale) {
    LANGUAGE.store(language.index(), Ordering::Relaxed);
}

pub(crate) fn current() -> Locale {
    #[cfg(test)]
    if let Some(language) = TEST_LANGUAGE.with(|value| value.get()) {
        return language;
    }
    Locale::from_index(LANGUAGE.load(Ordering::Relaxed))
}

pub(crate) fn translate(language: Locale, english: &'static str) -> &'static str {
    if language == Locale::English {
        return english;
    }
    CATALOG
        .get(english)
        .and_then(|translations| translations.get(language.code()))
        .map(String::as_str)
        .unwrap_or(english)
}

pub(crate) fn tr(english: &'static str) -> &'static str {
    translate(current(), english)
}

/// Localize Flux-owned result captions without changing provider data or ranking.
pub(crate) fn text(english: &str) -> String {
    if current() == Locale::English {
        return english.to_owned();
    }
    CATALOG
        .get(english)
        .and_then(|translations| translations.get(current().code()))
        .cloned()
        .unwrap_or_else(|| english.to_owned())
}

pub(crate) fn format(english: &'static str, values: &[(&str, String)]) -> String {
    let mut output = String::new();
    let mut remaining = tr(english);
    while let Some((prefix, tail)) = remaining.split_once('{') {
        output.push_str(prefix);
        let Some((key, suffix)) = tail.split_once('}') else {
            output.push('{');
            output.push_str(tail);
            return output;
        };
        if let Some((_, value)) = values.iter().find(|(name, _)| *name == key) {
            output.push_str(value);
        } else {
            output.push('{');
            output.push_str(key);
            output.push('}');
        }
        remaining = suffix;
    }
    output.push_str(remaining);
    output
}

pub(crate) fn count(singular: &'static str, plural: &'static str, value: usize) -> String {
    format(
        if value == 1 { singular } else { plural },
        &[("count", value.to_string())],
    )
}

#[cfg(test)]
pub(crate) fn with_locale<T>(language: Locale, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<Locale>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_LANGUAGE.with(|value| value.set(self.0));
        }
    }
    let _restore = Restore(TEST_LANGUAGE.with(|value| value.replace(Some(language))));
    run()
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
            for locale in Locale::ALL
                .into_iter()
                .filter(|locale| *locale != Locale::English)
            {
                let translated = translations
                    .get(locale.code())
                    .expect("every locale has a translation");
                assert!(!translated.trim().is_empty(), "missing translation: {key}");
                assert_eq!(
                    placeholders(key),
                    placeholders(translated),
                    "placeholder mismatch: {key}"
                );
            }
        }
        assert_eq!(translate(Locale::English, "Language"), "Language");
        assert_eq!(
            translate(Locale::SimplifiedChinese, "Language"),
            "\u{8bed}\u{8a00}"
        );
        assert_eq!(
            translate(Locale::TraditionalChinese, "Language"),
            "\u{8a9e}\u{8a00}"
        );
        assert_eq!(translate(Locale::Japanese, "Language"), "\u{8a00}\u{8a9e}");
        assert_eq!(
            translate(Locale::Russian, "Language"),
            "\u{42f}\u{437}\u{44b}\u{43a}"
        );
        for language in Locale::ALL {
            assert!(LANGUAGE_NAMES.contains_key(language.code()));
            assert!(!native_name(language).is_empty());
        }
        assert_eq!(
            translate(Locale::Japanese, "untranslated plugin text"),
            "untranslated plugin text"
        );
    }

    #[test]
    fn count_labels_handle_one_two_five_and_twenty_one() {
        for language in Locale::ALL {
            with_locale(language, || {
                for value in [1, 2, 5, 21] {
                    let label = count(
                        "{count} application result",
                        "{count} application results",
                        value,
                    );
                    assert!(label.contains(&value.to_string()));
                    assert!(!label.contains("(s)"));
                    assert!(!label.contains('{'));
                    if language == Locale::English {
                        assert_eq!(
                            label,
                            format!(
                                "{value} application result{}",
                                if value == 1 { "" } else { "s" }
                            )
                        );
                    }
                }
            });
        }
    }

    #[test]
    fn interpolation_does_not_expand_placeholders_inside_values() {
        with_locale(Locale::English, || {
            assert_eq!(
                format(
                    "Stable {version} update failed: {error}",
                    &[
                        ("version", "{error}".to_owned()),
                        ("error", "details".to_owned())
                    ]
                ),
                "Stable {error} update failed: details"
            );
        });
    }

    #[test]
    fn provider_failures_are_localized_and_preserve_diagnostics() {
        for language in Locale::ALL {
            with_locale(language, || {
                for value in [1, 2, 5, 21] {
                    let status = count(
                        "{count} native Flow plugin did not respond",
                        "{count} native Flow plugins did not respond",
                        value,
                    );
                    assert!(status.contains(&value.to_string()));
                    assert!(!status.contains("(s)"));
                    assert!(!status.contains('{'));
                    if language == Locale::English {
                        assert_eq!(status.contains("plugins"), value != 1);
                    }
                }
                for template in [
                    "Native plugin host restarted (attempt {attempt}): {error}",
                    "Unable to start winget: {error}",
                    "Unable to start Everything in the background: {error}",
                ] {
                    let status = format(
                        template,
                        &[
                            ("attempt", "2".to_owned()),
                            ("error", "diagnostic {attempt}".to_owned()),
                        ],
                    );
                    assert!(status.ends_with("diagnostic {attempt}"));
                    if template.contains("{attempt}") {
                        assert!(status.contains('2'));
                    }
                    if language != Locale::English {
                        assert_ne!(translate(language, template), template);
                    }
                }
            });
        }
    }
}
