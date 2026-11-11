use windui::prelude::*;

use super::settings_actions::{
    apply_settings, close_settings, has_unsaved_changes, reset_active_tab, revert_settings,
    tab_is_resettable,
};
use super::settings_ui::SettingsUi;
use super::ui_constants::SETTINGS_SIDEBAR_WIDTH;

/// Sidebar order is the tab-index contract: the smoke runs select a tab by its
/// numeric index through `FLUX_SMOKE_SETTINGS_TAB`, and every tab body gates on
/// the same index, so renaming alone is safe but reordering is not.
const TAB_NAMES: [&str; 5] = ["General", "Visual", "Priorities", "Plugins", "Stats"];
/// Segoe Fluent Icons codepoints for the navigation rows, taken from the
/// documented glyph table: Settings, Color, Sort, Puzzle, AreaChart. The font
/// ships with Windows 11 and is already used for the Recycle Bin rows.
const TAB_GLYPHS: [&str; 5] = ["\u{e713}", "\u{e790}", "\u{e8cb}", "\u{ea86}", "\u{e9d2}"];
const ICON_FONT: &str = "Segoe Fluent Icons";
const TAB_DESCRIPTIONS: [&str; 5] = [
    "Startup, activation and update behavior.",
    "Caret, selection color and launcher size.",
    "Applications that always rank first.",
    "File, note and web search providers.",
    "What you searched and opened.",
];

/// A settings group title: semibold, slightly larger than row text, no fill so
/// the Acrylic backdrop stays untouched.
pub(crate) fn settings_section_header(title: &str) -> Element {
    Element::label(title)
        .font_size(15.0)
        .font_weight(600)
        .fg_role(Role::Text)
}

/// One settings group: a title above the panel that holds its rows.
///
/// On the solid fallback surface the panel is a raised card with a hairline
/// edge, which is what carries the hierarchy in the reference. Over live
/// Acrylic the same step is made of light instead of hue: a restrained white
/// plate lifts a group off the material without covering it, so the desktop
/// still reads through every panel and the surface stops looking like one
/// flat field of blur.
pub(crate) fn settings_group(title: &str, rows: Vec<Element>) -> Element {
    let mut body = Element::col()
        .width_match()
        .padding_edges(16, 12, 16, 12)
        .spacing(4);
    for row in rows {
        body = body.child(row);
    }
    body = if super::theme_text::needs_opaque_surface() {
        body.bg(super::theme_text::FALLBACK_PANEL)
            .corner(12.0)
            .border(super::theme_text::FALLBACK_EDGE, 1)
    } else {
        body.bg(Color::rgba(255, 255, 255, 22))
            .corner(12.0)
            .border(Color::rgba(255, 255, 255, 40), 1)
    };
    Element::col()
        .width_match()
        .spacing(8)
        .child(settings_section_header(title))
        .child(body)
}

/// Right gutter for settings tab scroll content.
///
/// windui's overlay scrollbar reserves only TRACK_W + MARGIN + CONTENT_GAP
/// for layout while its grab zone (HIT_W) is wider, so controls glued to the
/// content edge feel overlapped by the thumb. Wrap the tab body as
/// `scroll > row[width_match] > col[weight(1.0)] + gutter`: the fixed spacer
/// keeps right-edge controls (checkboxes, buttons) clear of the thumb and
/// its hit zone, gives breathing room against the panel edge when the tab
/// does not overflow, and leaves the left edge aligned with the header.
pub(crate) fn settings_scroll_gutter() -> Element {
    // Wide enough that the panels keep their inset while the track itself moves
    // out to the window edge: the rail between them is the scrollbar's grab band.
    Element::leaf().size(14, 1)
}

pub(crate) fn build_settings_panel(
    ui: &SettingsUi,
    tab_general: Element,
    tab_visual: Element,
    tab_priorities: Element,
    tab_plugins: Element,
    tab_stats: Element,
) -> Element {
    let settings_tab = ui.settings_tab;
    let ui_for_reset = ui.clone();
    let ui_for_apply = ui.clone();
    let ui_for_close = ui.clone();

    // Only the active tab's title and summary are shown; the hidden pairs take
    // no space, so the header keeps a constant height instead of jumping when
    // descriptions differ in length. No weight here: the header is a column of
    // the body now, and a weighted column would claim the whole remaining
    // height and push the tab content below it.
    let mut heading = Element::col().spacing(4);
    for index in 0..TAB_NAMES.len() {
        heading = heading
            .child(
                Element::label(TAB_NAMES[index])
                    .font_size(24.0)
                    .font_weight(600)
                    .fg(Color::WHITE)
                    .visible_when(move || settings_tab.get() == index),
            )
            .child(
                Element::label(TAB_DESCRIPTIONS[index])
                    .font_size(12.0)
                    .fg(Color::rgba(235, 241, 255, 235))
                    .visible_when(move || settings_tab.get() == index),
            );
    }

    // The reference has no Back control: the title strip's close button
    // dismisses the window and the hotkey opens the search strip again.
    let header = heading;

    let ui_for_hint = ui.clone();
    let ui_for_revert = ui.clone();
    let ui_for_revert_visible = ui.clone();
    let ui_for_apply_visible = ui.clone();
    let footer = Element::row()
        .width_match()
        .spacing(10)
        .cross(Align::Center)
        .child(
            Element::label("Changes are written when you press Apply settings.")
                .font_size(11.0)
                .fg(Color::rgba(235, 241, 255, 210))
                .weight(1.0)
                .visible_when(move || has_unsaved_changes(&ui_for_hint)),
        )
        .child(
            Element::button("Revert")
                .neutral()
                .visible_when(move || has_unsaved_changes(&ui_for_revert_visible))
                .tooltip("Discard unsaved changes and restore the saved values.")
                .on_click(move |ctx| revert_settings(&ui_for_revert, ctx)),
        )
        .child(
            Element::button("\u{2713} Apply settings")
                .visible_when(move || has_unsaved_changes(&ui_for_apply_visible))
                .on_click(move |ctx| apply_settings(&ui_for_apply, ctx)),
        );

    let reset = Element::button("Reset tab defaults")
        .small()
        .neutral()
        .width_match()
        .tooltip("Restore this tab's defaults. Nothing is saved until you press Apply settings.")
        .visible_when(move || tab_is_resettable(settings_tab.get()))
        .on_click(move |ctx| reset_active_tab(&ui_for_reset, ctx));

    let fallback = super::theme_text::needs_opaque_surface();

    let mut sidebar = Element::col()
        .width(SETTINGS_SIDEBAR_WIDTH)
        .height_match()
        .padding_edges(14, 12, 12, 14)
        .spacing(14)
        .child(
            Element::list_pill_glyphs(
                TAB_NAMES
                    .iter()
                    .zip(TAB_GLYPHS)
                    .map(|(name, glyph)| ((*name).to_string(), glyph.to_string()))
                    .collect(),
                settings_tab,
                ICON_FONT,
            )
            .weight(1.0),
        )
        .child(reset);

    if fallback {
        // The reference gives the navigation column the same tone as the group
        // panels; the darker window fill is what shows between the groups and
        // behind the footer band.
        sidebar = sidebar.bg(super::theme_text::FALLBACK_PANEL);
    } else {
        // The other half of the layering: the column sits just below the
        // material while the group plates rise above it. The step stays small
        // because a heavy dark plate here reads as a black hole, not as depth.
        sidebar = sidebar.bg(Color::rgba(0, 0, 0, 22));
    }

    // The reference frame carries the product name and a close control in a
    // strip above both columns, so the frameless window stays movable and
    // dismissible without a system titlebar. There is no logo here: the mark in
    // the reference is not this app's. Maximize is absent because the Settings
    // canvas has one fixed size, and minimize because the window is a
    // tray-resident surface that the hotkey already hides and shows.
    let titlebar = Element::row()
        .width_match()
        .height(40)
        .padding_edges(16, 6, 6, 0)
        .spacing(8)
        .cross(Align::Center)
        .child(
            Element::label("Flux Launcher")
                .font_size(13.0)
                .font_weight(650)
                .fg(Color::WHITE),
        )
        .child(Element::flex_spacer().window_drag())
        .child(
            Element::icon_button("\u{e8bb}")
                .font_family(ICON_FONT)
                .font_size(12.0)
                .tooltip("Hide Flux Launcher")
                .on_click(move |_| close_settings(&ui_for_close)),
        );

    // No right padding: the scroll container has to reach the window edge so the
    // scrollbar's grab zone covers the whole right rail instead of leaving a dead
    // strip there. The visible inset comes from the width the scrollbar reserves
    // plus the gutter below, which keeps the panels symmetric with the left side.
    let content = Element::col()
        .weight(1.0)
        .height_match()
        .padding_edges(18, 16, 0, 8)
        .spacing(10)
        .child(header)
        .child(tab_general)
        .child(tab_visual)
        .child(tab_priorities)
        .child(tab_plugins)
        .child(tab_stats)
        .child(footer);

    // Settings shares the same continuous Acrylic surface as the launcher: no
    // dark card here, or the blur disappears behind the old opaque slab. The
    // solid fallback has no material to edge it, so it draws its own hairline
    // border and a divider between the navigation column and the rows.
    let mut body_children = vec![sidebar];
    if fallback {
        body_children.push(
            Element::leaf()
                .width(1)
                .height_match()
                .bg(super::theme_text::FALLBACK_EDGE),
        );
    }
    body_children.push(content);

    let body = Element::row()
        .width_match()
        .weight(1.0)
        .spacing(0)
        .children(body_children);

    let mut root = Element::col()
        .fill()
        .spacing(0)
        .bg(super::theme_text::window_background())
        .child(titlebar)
        .child(body);
    if fallback {
        root = root
            .corner(10.0)
            .border(super::theme_text::FALLBACK_EDGE, 1);
    }
    root
}
