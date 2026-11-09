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

    pub fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::SimplifiedChinese => "简体中文",
            Self::TraditionalChinese => "繁體中文",
            Self::Japanese => "日本語",
            Self::Russian => "Русский",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;

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
