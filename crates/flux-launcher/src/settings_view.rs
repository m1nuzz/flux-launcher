use std::rc::Rc;

use crate::i18n::I18nHub;
use windui::prelude::*;

/// Shared reactive state for the Settings surface.
#[derive(Clone, Copy)]
pub(crate) struct SettingsUiState {
    pub(crate) visible: Signal<bool>,
    pub(crate) tab: Signal<usize>,
}

impl SettingsUiState {
    pub(crate) fn new(visible: Signal<bool>, tab: Signal<usize>) -> Self {
        Self { visible, tab }
    }
}

/// Wrap the Settings panel without changing its transparent Acrylic surface or
/// its visibility binding. Page contents remain assembled by the launcher while
/// their lifecycle callbacks are migrated incrementally.
pub(crate) fn settings_page(panel: Element, state: SettingsUiState) -> Element {
    Element::col()
        .fill()
        .padding(18)
        .child(panel)
        .visible_signal(state.visible)
}

/// Build the Settings header while keeping tab state and the Back callback explicit.
pub(crate) fn settings_header(
    settings_tab: Signal<usize>,
    i18n_hub: I18nHub,
    cancel_settings: Rc<dyn Fn()>,
) -> Element {
    Element::row()
        .width_match()
        .child(
            Element::col()
                .weight(1.0)
                .spacing(3)
                .child(
                    Element::label(i18n_hub.tr(|| t!("settings.title").into_owned()))
                        .font_size(25.0)
                        .fg(Color::WHITE),
                )
                .child(
                    Element::label(i18n_hub.tr(|| t!("settings.apply_immediate").into_owned()))
                        .font_size(12.0)
                        .fg(Color::rgba(235, 241, 255, 180)),
                ),
        )
        .child(Element::segmented_signal(
            i18n_hub.tr_vec(|| {
                vec![
                    t!("settings.tab.general").to_string(),
                    t!("settings.tab.visual").to_string(),
                    t!("settings.tab.priorities").to_string(),
                    t!("settings.tab.plugins").to_string(),
                ]
            }),
            settings_tab,
        ))
        .child(
            Element::button(i18n_hub.tr(|| t!("settings.back").into_owned()))
                .neutral()
                .on_click({
                    let cancel_settings = Rc::clone(&cancel_settings);
                    move |ctx| {
                        cancel_settings();
                        ctx.show_window();
                    }
                }),
        )
}
