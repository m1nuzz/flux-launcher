use serde::{Deserialize, Serialize};

/// Persisted interface language. Unknown future values safely fall back to English.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Locale {
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
    #[serde(rename = "zh-TW")]
    TraditionalChinese,
    #[serde(rename = "ja-JP")]
    Japanese,
    #[serde(rename = "ru-RU")]
    Russian,
    #[serde(rename = "ko")]
    Korean,
    #[serde(rename = "uk-UA")]
    Ukrainian,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "es-419")]
    LatinAmericanSpanish,
    #[serde(rename = "it")]
    Italian,
    #[serde(rename = "pt-BR")]
    BrazilianPortuguese,
    #[serde(rename = "pt-PT")]
    EuropeanPortuguese,
    #[serde(rename = "pl")]
    Polish,
    #[serde(rename = "tr")]
    Turkish,
    #[serde(rename = "nl")]
    Dutch,
    #[serde(rename = "cs")]
    Czech,
    #[serde(rename = "da")]
    Danish,
    #[serde(rename = "sk")]
    Slovak,
    #[serde(rename = "sr", alias = "sr-Cyrl-RS")]
    Serbian,
    #[serde(rename = "vi")]
    Vietnamese,
    #[serde(rename = "ar")]
    Arabic,
    #[serde(rename = "he")]
    Hebrew,
    #[serde(rename = "nb", alias = "nb-NO")]
    NorwegianBokmal,
    #[default]
    #[serde(rename = "en-US", other)]
    English,
}

impl Locale {
    /// Match supported BCP 47 variants without confusing scripts or regions.
    pub fn matching(tag: &str) -> Option<Self> {
        let latin_modifier = tag
            .split_once('@')
            .is_some_and(|(_, modifier)| modifier.eq_ignore_ascii_case("latin"));
        let normalized = tag
            .split(['.', '@'])
            .next()?
            .replace('_', "-")
            .to_ascii_lowercase();
        // Extension keys such as the Unicode collation key "co" are not regions.
        let parts: Vec<_> = normalized
            .split('-')
            .take_while(|part| part.len() != 1)
            .collect();
        match *parts.first()? {
            "en" => Some(Self::English),
            "ja" => Some(Self::Japanese),
            "ru" => Some(Self::Russian),
            "ko" => Some(Self::Korean),
            "uk" => Some(Self::Ukrainian),
            "de" => Some(Self::German),
            "fr" => Some(Self::French),
            "es" if parts.iter().any(|part| {
                matches!(
                    *part,
                    "419"
                        | "mx"
                        | "ar"
                        | "bo"
                        | "cl"
                        | "co"
                        | "cr"
                        | "cu"
                        | "do"
                        | "ec"
                        | "sv"
                        | "gt"
                        | "hn"
                        | "ni"
                        | "pa"
                        | "py"
                        | "pe"
                        | "pr"
                        | "uy"
                        | "ve"
                        | "bz"
                )
            }) =>
            {
                Some(Self::LatinAmericanSpanish)
            }
            "es" => Some(Self::Spanish),
            "it" => Some(Self::Italian),
            "pt" if parts.contains(&"br") => Some(Self::BrazilianPortuguese),
            "pt" => Some(Self::EuropeanPortuguese),
            "pl" => Some(Self::Polish),
            "tr" => Some(Self::Turkish),
            "nl" => Some(Self::Dutch),
            "cs" => Some(Self::Czech),
            "da" => Some(Self::Danish),
            "sk" => Some(Self::Slovak),
            // The shipped Serbian catalog is Cyrillic; never select it for an
            // explicitly requested Latin-script UI.
            "sr" if parts.contains(&"latn") || latin_modifier => None,
            "sr" => Some(Self::Serbian),
            "vi" => Some(Self::Vietnamese),
            "ar" => Some(Self::Arabic),
            "he" | "iw" => Some(Self::Hebrew),
            "nb" | "no" => Some(Self::NorwegianBokmal),
            "zh" if parts.contains(&"hant") => Some(Self::TraditionalChinese),
            "zh" if parts.contains(&"hans") => Some(Self::SimplifiedChinese),
            "zh" if parts.iter().any(|part| matches!(*part, "tw" | "hk" | "mo")) => {
                Some(Self::TraditionalChinese)
            }
            "zh" => Some(Self::SimplifiedChinese),
            _ => None,
        }
    }

    pub fn preferred<'a>(tags: impl IntoIterator<Item = &'a str>) -> Self {
        tags.into_iter()
            .find_map(Self::matching)
            .unwrap_or_default()
    }
    pub const ALL: [Self; 25] = [
        Self::English,
        Self::SimplifiedChinese,
        Self::TraditionalChinese,
        Self::Japanese,
        Self::Russian,
        Self::Korean,
        Self::Ukrainian,
        Self::German,
        Self::French,
        Self::Spanish,
        Self::LatinAmericanSpanish,
        Self::Italian,
        Self::BrazilianPortuguese,
        Self::EuropeanPortuguese,
        Self::Polish,
        Self::Turkish,
        Self::Dutch,
        Self::Czech,
        Self::Danish,
        Self::Slovak,
        Self::Serbian,
        Self::Vietnamese,
        Self::Arabic,
        Self::Hebrew,
        Self::NorwegianBokmal,
    ];

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|locale| *locale == self)
            .unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or_default()
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en-US",
            Self::SimplifiedChinese => "zh-CN",
            Self::TraditionalChinese => "zh-TW",
            Self::Japanese => "ja-JP",
            Self::Russian => "ru-RU",
            Self::Korean => "ko",
            Self::Ukrainian => "uk-UA",
            Self::German => "de",
            Self::French => "fr",
            Self::Spanish => "es",
            Self::LatinAmericanSpanish => "es-419",
            Self::Italian => "it",
            Self::BrazilianPortuguese => "pt-BR",
            Self::EuropeanPortuguese => "pt-PT",
            Self::Polish => "pl",
            Self::Turkish => "tr",
            Self::Dutch => "nl",
            Self::Czech => "cs",
            Self::Danish => "da",
            Self::Slovak => "sk",
            Self::Serbian => "sr",
            Self::Vietnamese => "vi",
            Self::Arabic => "ar",
            Self::Hebrew => "he",
            Self::NorwegianBokmal => "nb",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;

    #[test]
    fn language_tags_match_base_language_scripts_and_regions() {
        for (tag, expected) in [
            ("en-GB", Locale::English),
            ("zh-Hans-TW", Locale::SimplifiedChinese),
            ("zh-Hant", Locale::TraditionalChinese),
            ("zh-HK", Locale::TraditionalChinese),
            ("ja_JP.UTF-8", Locale::Japanese),
            ("ru-KZ", Locale::Russian),
            ("ko-KR", Locale::Korean),
            ("uk_UA.UTF-8", Locale::Ukrainian),
            ("de-AT", Locale::German),
            ("fr-CA", Locale::French),
            ("es-ES", Locale::Spanish),
            ("es-419", Locale::LatinAmericanSpanish),
            ("es-MX", Locale::LatinAmericanSpanish),
            ("es-AR", Locale::LatinAmericanSpanish),
            ("es-GQ", Locale::Spanish),
            ("es-u-co-trad", Locale::Spanish),
            ("pt-BR", Locale::BrazilianPortuguese),
            ("pt-PT", Locale::EuropeanPortuguese),
            ("pt-AO", Locale::EuropeanPortuguese),
            ("sr-Cyrl-RS", Locale::Serbian),
            ("sr", Locale::Serbian),
            ("nb-NO", Locale::NorwegianBokmal),
            ("no-NO", Locale::NorwegianBokmal),
            ("ar-EG", Locale::Arabic),
            ("he-IL", Locale::Hebrew),
        ] {
            assert_eq!(Locale::matching(tag), Some(expected));
        }
        assert_eq!(Locale::matching("sr-Latn-RS"), None);
        assert_eq!(Locale::matching("sr_RS.UTF-8@latin"), None);
        assert_eq!(Locale::matching("nn-NO"), None);
        assert_eq!(Locale::preferred(["fi-FI", "ja-JP"]), Locale::Japanese);
        assert_eq!(Locale::preferred(["fi-FI", "hu-HU"]), Locale::English);
    }

    #[test]
    fn old_and_future_settings_preserve_other_preferences() {
        for json in [
            r#"{"google_alias":"web"}"#,
            r#"{"language":"future","google_alias":"web"}"#,
        ] {
            let settings: Settings = serde_json::from_str(json).unwrap();
            assert_eq!(settings.language, Locale::English);
            assert_eq!(settings.google_alias, "web");
        }
    }

    #[test]
    fn every_language_round_trips_through_settings() {
        let codes: std::collections::HashSet<_> =
            Locale::ALL.iter().map(|locale| locale.code()).collect();
        assert_eq!(codes.len(), 25, "catalog locales must be unique");
        for language in Locale::ALL {
            let settings = Settings {
                language,
                ..Settings::default()
            };
            let json = serde_json::to_string(&settings).unwrap();
            let restored: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(restored.language, language);
            assert_eq!(Locale::from_index(language.index()), language);
            assert!(json.contains(language.code()));
            assert_eq!(Locale::matching(language.code()), Some(language));
        }
        assert_eq!(Locale::from_index(usize::MAX), Locale::English);
    }

    #[test]
    fn locale_aliases_use_one_catalog_and_canonical_saved_code() {
        for (alias, expected) in [
            ("nb-NO", Locale::NorwegianBokmal),
            ("sr-Cyrl-RS", Locale::Serbian),
        ] {
            let language: Locale = serde_json::from_str(&format!("\"{alias}\"")).unwrap();
            assert_eq!(language, expected);
            assert_eq!(
                serde_json::to_string(&language).unwrap(),
                format!("\"{}\"", expected.code())
            );
        }
    }
}
