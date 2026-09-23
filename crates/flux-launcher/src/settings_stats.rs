use std::sync::Arc;

use flux_core::MAX_QUERY_HISTORY;
use windui::prelude::*;
use windui::signal::Signal;

use super::settings_shell::{settings_scroll_gutter, settings_section_header};
use super::settings_ui::SettingsUi;

/// Index of the Stats tab in the settings segmented control. The tab is
/// appended last so the General/Visual/Priorities/Plugins indices stay put.
pub(crate) const STATS_TAB_INDEX: usize = 4;
/// How many recent queries the Stats tab lists.
const MAX_RECENT_QUERIES: usize = 8;

/// Usage block for the Stats tab. The history ring only remembers the last
/// queries, while `total` counts every committed search since install.
pub(crate) fn usage_summary(total: u64, remembered: usize) -> String {
    if total == 0 {
        return String::from("No committed searches yet. Run a search to see stats here.");
    }
    format!("Queries run: {total}\nQueries remembered: {remembered} of {MAX_QUERY_HISTORY}")
}

/// Newest-first recent queries, bounded for display.
pub(crate) fn recent_queries(history: &[String], limit: usize) -> Vec<String> {
    history.iter().rev().take(limit).cloned().collect()
}

/// Recent-queries block for the Stats tab.
pub(crate) fn recent_block(history: &[String]) -> String {
    let recent = recent_queries(history, MAX_RECENT_QUERIES);
    if recent.is_empty() {
        return String::from("Nothing here yet.");
    }
    recent.join("\n")
}

/// Both Stats tab texts from one snapshot. Query contents never leave this
/// tab except through the explicit Share button, and even the shared text
/// carries aggregates only.
pub(crate) fn stats_texts(history: &[String], total: u64) -> (String, String) {
    (usage_summary(total, history.len()), recent_block(history))
}

/// Share text for social media. Aggregates only: committed query contents
/// may contain file names or paths, so they are never shared implicitly.
pub(crate) fn share_stats_text(total: u64) -> String {
    format!("My Flux Launcher stats: {total} queries run. https://github.com/m1nuzz/flux-launcher")
}

/// Recompute the Stats tab display signals from the current state. Called on
/// every committed search and on history clear; the tab itself only reads.
pub(crate) fn refresh_stats_texts(
    history: &[String],
    total: u64,
    usage: Signal<String>,
    recent: Signal<String>,
) {
    let (usage_text, recent_text) = stats_texts(history, total);
    usage.set(usage_text);
    recent.set(recent_text);
}

pub(crate) fn build_stats_tab(ui: &SettingsUi) -> Element {
    let settings_tab = ui.settings_tab;
    let usage = ui.stats_usage;
    let recent = ui.stats_recent;
    let settings_for_share = Arc::clone(&ui.shared_settings);
    Element::scroll()
        .weight(1.0)
        .visible_when(move || settings_tab.get() == STATS_TAB_INDEX)
        .child(
            Element::row()
                .width_match()
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(12)
                        .child(settings_section_header("Usage"))
                        .child(
                            Element::label_signal(usage)
                                .font_size(12.0)
                                .fg(Color::rgba(235, 241, 255, 235)),
                        )
                        .child(Element::divider())
                        .child(settings_section_header("Recent queries"))
                        .child(
                            Element::label_signal(recent)
                                .font_size(12.0)
                                .fg(Color::rgba(235, 241, 255, 235)),
                        )
                        .child(Element::divider())
                        .child(Element::setting_row_desc(
                            "Share stats",
                            "Copy a short summary for social media",
                            Element::button("Share stats").on_click(move |ctx| {
                                let total = settings_for_share
                                    .read()
                                    .map(|settings| settings.total_queries_committed)
                                    .unwrap_or(0);
                                ctx.clipboard_set(&share_stats_text(total));
                                ctx.toast_ok("Stats copied to clipboard");
                            }),
                        )),
                )
                .child(settings_scroll_gutter()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_summary_zero_shows_empty_state() {
        assert_eq!(
            usage_summary(0, 0),
            "No committed searches yet. Run a search to see stats here."
        );
    }

    #[test]
    fn usage_summary_shows_counts() {
        assert_eq!(
            usage_summary(41, 7),
            "Queries run: 41\nQueries remembered: 7 of 32"
        );
    }

    #[test]
    fn recent_queries_are_newest_first_and_bounded() {
        let history = vec![
            String::from("steam"),
            String::from("ext:zip"),
            String::from("notepad"),
        ];
        assert_eq!(
            recent_queries(&history, 2),
            vec![String::from("notepad"), String::from("ext:zip")]
        );
        assert_eq!(
            recent_queries(&history, 10),
            vec![
                String::from("notepad"),
                String::from("ext:zip"),
                String::from("steam")
            ]
        );
    }

    #[test]
    fn recent_block_empty_state() {
        assert_eq!(recent_block(&[]), "Nothing here yet.");
    }

    #[test]
    fn stats_texts_compose_both_blocks() {
        let history = vec![String::from("steam")];
        let (usage, recent) = stats_texts(&history, 5);
        assert_eq!(usage, "Queries run: 5\nQueries remembered: 1 of 32");
        assert_eq!(recent, "steam");
    }

    #[test]
    fn share_stats_text_carries_total_but_no_query_contents() {
        let text = share_stats_text(128);
        assert!(text.contains("128"));
        assert!(!text.contains("steam"));
        assert!(!text.contains("ext:zip"));
        assert!(text.contains("https://github.com/m1nuzz/flux-launcher"));
    }
}
