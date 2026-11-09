use flux_core::{Locale, Settings};

pub(crate) fn resolve(settings: &mut Settings) {
    if !settings.language_explicit {
        settings.language = detect();
    }
}

#[cfg(windows)]
pub(crate) fn detect() -> Locale {
    use windows::core::PWSTR;
    use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let mut count = 0;
    let mut length = 0;
    // SAFETY: the first call queries the required UTF-16 buffer size. The second
    // call receives exactly that writable capacity; the API checks its length.
    unsafe {
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut length).is_err()
            || length == 0
        {
            return Locale::English;
        }
        let mut buffer = vec![0_u16; length as usize];
        if GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut length,
        )
        .is_err()
        {
            return Locale::English;
        }
        let names: Vec<_> = buffer
            .split(|unit| *unit == 0)
            .filter(|name| !name.is_empty())
            .map(String::from_utf16_lossy)
            .collect();
        Locale::preferred(names.iter().map(String::as_str))
    }
}

#[cfg(not(windows))]
pub(crate) fn detect() -> Locale {
    let names: Vec<_> = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .collect();
    Locale::preferred(names.iter().map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicitly_chosen_english_is_not_overwritten_by_system_detection() {
        let mut settings = Settings {
            language: Locale::English,
            language_explicit: true,
            ..Settings::default()
        };
        resolve(&mut settings);
        assert_eq!(settings.language, Locale::English);
        assert!(settings.language_explicit);
    }
}
