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
    #[default]
    #[serde(rename = "en-US", other)]
    English,
}

impl Locale {
    /// Match supported BCP 47 variants without confusing scripts or regions.
    pub fn matching(tag: &str) -> Option<Self> {
        let normalized = tag
            .split(['.', '@'])
            .next()?
            .replace('_', "-")
            .to_ascii_lowercase();
        let parts: Vec<_> = normalized.split('-').collect();
        match *parts.first()? {
            "en" => Some(Self::English),
            "ja" => Some(Self::Japanese),
            "ru" => Some(Self::Russian),
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
    pub const ALL: [Self; 5] = [
        Self::English,
        Self::SimplifiedChinese,
        Self::TraditionalChinese,
        Self::Japanese,
        Self::Russian,
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
        ] {
            assert_eq!(Locale::matching(tag), Some(expected));
        }
        assert_eq!(Locale::preferred(["de-DE", "ja-JP"]), Locale::Japanese);
        assert_eq!(Locale::preferred(["de-DE", "fr-FR"]), Locale::English);
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
        }
        assert_eq!(Locale::from_index(usize::MAX), Locale::English);
    }
}
