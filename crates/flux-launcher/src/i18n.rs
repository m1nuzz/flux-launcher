use std::cell::RefCell;
use std::rc::Rc;

use flux_core::Language;
use windui::prelude::{signal, Signal};

/// Set the UI language based on the system locale at startup; only Simplified
/// Chinese and English are supported, everything else falls back to `en`.
pub(crate) fn apply_system_locale() {
    let Some(raw) = sys_locale::get_locale() else {
        return;
    };
    rust_i18n::set_locale(select_locale(&raw));
}

/// Map a system locale tag to a supported Flux UI language.
/// Simplified Chinese tags (`zh`, `zh-CN`, `zh-Hans`, ...) map to `zh-CN`;
/// everything else (including Traditional `zh-TW`/`zh-Hant`) falls back to `en`.
pub(crate) fn select_locale(raw: &str) -> &'static str {
    let locale = raw.split('.').next().unwrap_or(raw).to_ascii_lowercase();
    if locale.starts_with("zh") && !locale.contains("tw") && !locale.contains("hant") {
        "zh-CN"
    } else {
        "en"
    }
}

/// Language preference dropdown index: follow system / English / Simplified Chinese.
pub(crate) fn language_preference_index(language: Language) -> usize {
    match language {
        Language::System => 0,
        Language::English => 1,
        Language::SimplifiedChinese => 2,
    }
}

/// Language preference dropdown index back to the enum.
pub(crate) fn language_preference_from_index(index: usize) -> Language {
    match index {
        1 => Language::English,
        2 => Language::SimplifiedChinese,
        _ => Language::System,
    }
}

/// Apply the configured UI language from settings.
pub(crate) fn apply_configured_locale(language: Language) {
    let locale = match language {
        Language::English => "en",
        Language::SimplifiedChinese => "zh-CN",
        Language::System => sys_locale::get_locale()
            .map(|raw| select_locale(&raw))
            .unwrap_or("en"),
    };
    rust_i18n::set_locale(locale);
}

pub(crate) fn configured_locale(language: Language) -> String {
    match language {
        Language::English => String::from("en"),
        Language::SimplifiedChinese => String::from("zh-CN"),
        Language::System => sys_locale::get_locale()
            .map(|raw| select_locale(&raw))
            .unwrap_or("en")
            .to_string(),
    }
}

/// Reactive manager for localized string signals that update when locale changes.
#[derive(Clone)]
pub(crate) struct I18nHub {
    signals: Rc<RefCell<Vec<(Box<dyn Fn() -> String>, Signal<String>)>>>,
    vec_signals: Rc<RefCell<Vec<(Box<dyn Fn() -> Vec<String>>, Signal<Vec<String>>)>>>,
}

impl I18nHub {
    pub(crate) fn new() -> Self {
        Self {
            signals: Rc::new(RefCell::new(Vec::new())),
            vec_signals: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub(crate) fn tr(&self, f: impl Fn() -> String + 'static) -> Signal<String> {
        let sig = signal(f());
        self.signals.borrow_mut().push((Box::new(f), sig));
        sig
    }

    pub(crate) fn tr_vec(&self, f: impl Fn() -> Vec<String> + 'static) -> Signal<Vec<String>> {
        let sig = signal(f());
        self.vec_signals.borrow_mut().push((Box::new(f), sig));
        sig
    }

    pub(crate) fn refresh(&self) {
        for (f, sig) in self.signals.borrow().iter() {
            sig.set(f());
        }
        for (f, sig) in self.vec_signals.borrow().iter() {
            sig.set(f());
        }
    }
}
