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
