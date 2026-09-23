use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use flux_core::{LaunchCount, Settings};
use windui::prelude::*;
use windui::signal::Signal;
use windui::text::TextEngine;

use super::settings_shell::{settings_scroll_gutter, settings_section_header};
use super::settings_ui::SettingsUi;
use super::ui_constants::CURRENT_VERSION;

/// Index of the Stats tab in the settings segmented control. The tab is
/// appended last so the General/Visual/Priorities/Plugins indices stay put.
pub(crate) const STATS_TAB_INDEX: usize = 4;
/// How many top queries the Stats tab and the share card list.
pub(crate) const MAX_TOP_QUERIES: usize = 5;

/// Usage block for the Stats tab. The history ring only remembers the last
/// queries, while `total` counts every committed search since install.
pub(crate) fn usage_summary(total: u64) -> String {
    if total == 0 {
        return String::from("No committed searches yet. Run a search to see stats here.");
    }
    format!("Queries run: {total}")
}

/// Usage and top-opened texts from one snapshot.
pub(crate) fn stats_texts(launches: &HashMap<String, LaunchCount>, total: u64) -> (String, String) {
    (usage_summary(total), top_launches_block(launches))
}

/// Top opened results by lifetime launch count. Titles refresh on every
/// launch, so renames heal without losing the count.
pub(crate) fn top_launches(
    launches: &HashMap<String, LaunchCount>,
    limit: usize,
) -> Vec<(String, u64)> {
    let mut top: Vec<(String, u64)> = launches
        .values()
        .map(|entry| (entry.title.clone(), entry.count))
        .collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top.truncate(limit);
    top
}

/// Top-opened block for the Stats tab and the share card.
pub(crate) fn top_launches_block(launches: &HashMap<String, LaunchCount>) -> String {
    let top = top_launches(launches, MAX_TOP_QUERIES);
    if top.is_empty() {
        return String::from("No data yet.");
    }
    top.iter()
        .enumerate()
        .map(|(i, (title, count))| format!("{}. {title}: {count}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Share text for social media. Aggregates only: committed query contents
/// may contain file names or paths, so they are never shared implicitly.
/// (The share *card* does list top queries, but only because the user
/// explicitly asked for it when pressing Share.)
pub(crate) fn share_stats_text(total: u64) -> String {
    format!("My Flux Launcher stats: {total} queries run. https://github.com/m1nuzz/flux-launcher")
}

/// Clear remembered queries and launch counts, then refresh the Stats
/// display. Shared by the clear-confirm dialog (both the Stats and the
/// General tab route through it). Disk save stays with the caller so unit
/// tests never touch the real settings file.
pub(crate) fn clear_history_state(
    settings: &Arc<RwLock<Settings>>,
    history: &Rc<RefCell<Vec<String>>>,
    history_cursor: Signal<Option<usize>>,
    usage: Signal<String>,
    top: Signal<String>,
) -> bool {
    let Ok(mut guard) = settings.write() else {
        return false;
    };
    guard.clear_query_history();
    let total = guard.total_queries_committed;
    drop(guard);
    history.borrow_mut().clear();
    history_cursor.set(None);
    refresh_stats_texts(&HashMap::new(), total, usage, top);
    true
}

/// Recompute the Stats tab display signals from the current state. Called on
/// every committed search and on history clear; the tab itself only reads.
pub(crate) fn refresh_stats_texts(
    launches: &HashMap<String, LaunchCount>,
    total: u64,
    usage: Signal<String>,
    top: Signal<String>,
) {
    let (usage_text, top_text) = stats_texts(launches, total);
    usage.set(usage_text);
    top.set(top_text);
}

pub(crate) fn build_stats_tab(ui: &SettingsUi) -> Element {
    let settings_tab = ui.settings_tab;
    let usage = ui.stats_usage;
    let top = ui.stats_top;
    let clear_confirm = ui.clear_history_confirm;
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
                        .child(settings_section_header("Top opened"))
                        .child(
                            Element::label_signal(top)
                                .font_size(12.0)
                                .fg(Color::rgba(235, 241, 255, 235)),
                        )
                        .child(Element::divider())
                        .child(Element::setting_row_desc(
                            "Clear history",
                            "Forget remembered queries and top counts",
                            Element::button("Clear history").small().on_click(move |_| {
                                clear_confirm.set(true);
                            }),
                        ))
                        .child(Element::divider())
                        .child(Element::setting_row_desc(
                            "Share stats",
                            "Copy a stats card image plus a short summary",
                            Element::button("Share stats").on_click(move |ctx| {
                                let snapshot = settings_for_share
                                    .read()
                                    .map(|settings| {
                                        (
                                            settings.total_queries_committed,
                                            settings.launch_counts.clone(),
                                        )
                                    })
                                    .ok();
                                let Some((total, launches)) = snapshot else {
                                    ctx.toast_ok("Could not lock Flux settings");
                                    return;
                                };
                                let text = share_stats_text(total);
                                let top = top_launches(&launches, MAX_TOP_QUERIES);
                                #[cfg(windows)]
                                {
                                    if let Some((width, height, rgba)) =
                                        render_share_card_rgba(total, CURRENT_VERSION, &top)
                                    {
                                        ctx.clipboard_set_text_and_image(
                                            &text, width, height, &rgba,
                                        );
                                        ctx.toast_ok("Stats card copied to clipboard");
                                        return;
                                    }
                                }
                                ctx.clipboard_set(&text);
                                ctx.toast_ok("Stats copied as text");
                            }),
                        )),
                )
                .child(settings_scroll_gutter()),
        )
}

/// Share card dimensions: landscape OG size, unfurls on X/Discord/Telegram.
pub(crate) const SHARE_CARD_WIDTH: u32 = 1200;
pub(crate) const SHARE_CARD_HEIGHT: u32 = 630;

/// Thousands separator for hero numbers: 1284 -> "1,284".
pub(crate) fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn share_tile(value: String, caption: String, value_size: f32) -> Element {
    Element::col()
        .bg(Color::rgb(22, 33, 58))
        .corner(20.0)
        .padding_xy(28, 22)
        .spacing(4)
        .child(
            Element::label(value)
                .font_size(value_size)
                .font_weight(700)
                .fg(Color::WHITE),
        )
        .child(
            Element::label(caption)
                .font_size(18.0)
                .fg(Color::rgb(142, 162, 200)),
        )
}

/// Share card layout. Text lives on flat surfaces only; the single gradient
/// accent band carries no text (Wrapped rule: legibility over effects).
/// `top` holds up to MAX_TOP_QUERIES (display text, run count) pairs.
pub(crate) fn build_share_card(total: u64, version: &str, top: &[(String, u64)]) -> Element {
    let mut top_col = Element::col().width(400).spacing(10).child(
        Element::label("TOP OPENED")
            .font_size(22.0)
            .font_weight(600)
            .fg(Color::rgb(142, 162, 200)),
    );
    if top.is_empty() {
        top_col = top_col.child(
            Element::label("No data yet.")
                .font_size(26.0)
                .fg(Color::rgb(142, 162, 200)),
        );
    }
    for (i, (query, count)) in top.iter().enumerate() {
        top_col = top_col.child(
            Element::label(format!("{}. {query}: {count}", i + 1))
                .font_size(26.0)
                .fg(Color::WHITE)
                .max_lines(1)
                .truncate(Truncate::End),
        );
    }
    Element::row()
        .width(SHARE_CARD_WIDTH as i32)
        .height(SHARE_CARD_HEIGHT as i32)
        .bg(Color::rgb(11, 18, 32))
        .child(
            Element::leaf()
                .width(28)
                .height(SHARE_CARD_HEIGHT as i32)
                .bg_gradient(Gradient::linear(
                    (0.0, 0.0),
                    (0.0, 1.0),
                    vec![
                        (0.0, Color::rgb(76, 139, 244)),
                        (1.0, Color::rgb(37, 78, 216)),
                    ],
                )),
        )
        .child(
            Element::col()
                .weight(1.0)
                .padding_edges(64, 52, 60, 48)
                .spacing(20)
                .child(
                    Element::row()
                        .width_match()
                        .cross(Align::Center)
                        .child(
                            Element::label("FLUX LAUNCHER")
                                .font_size(20.0)
                                .font_weight(600)
                                .fg(Color::rgb(142, 162, 200)),
                        )
                        .child(Element::flex_spacer())
                        .child(
                            Element::label("MY STATS")
                                .font_size(20.0)
                                .font_weight(600)
                                .fg(Color::rgb(142, 162, 200)),
                        ),
                )
                .child(
                    Element::row()
                        .width_match()
                        .spacing(48)
                        .child(
                            Element::col()
                                .weight(1.0)
                                .spacing(16)
                                .child(
                                    Element::label(format_count(total))
                                        .font_size(140.0)
                                        .font_weight(700)
                                        .fg(Color::WHITE),
                                )
                                .child(
                                    Element::label("QUERIES RUN")
                                        .font_size(22.0)
                                        .font_weight(600)
                                        .fg(Color::rgb(142, 162, 200)),
                                )
                                .child(share_tile(
                                    format!("v{version}"),
                                    String::from("VERSION"),
                                    56.0,
                                )),
                        )
                        .child(top_col),
                )
                .child(
                    Element::label("github.com/m1nuzz/flux-launcher")
                        .font_size(20.0)
                        .fg(Color::rgb(91, 107, 140)),
                ),
        )
}
/// Render the share card offscreen (Windows only: needs the platform text
/// engine). Pixels come out premultiplied, like every tiny-skia buffer.
#[cfg(windows)]
fn render_share_card_pixmap(
    total: u64,
    version: &str,
    top: &[(String, u64)],
) -> Option<tiny_skia::Pixmap> {
    let card = build_share_card(total, version, top);
    let mut tree = windui::core::Tree::new();
    let root = card.build(&mut tree);
    tree.root = Some(root);
    let mut engine = windui::text::PlatformTextEngine::new();
    engine.set_scale(1.0);
    tree.layout_root(
        Size::new(SHARE_CARD_WIDTH as i32, SHARE_CARD_HEIGHT as i32),
        &mut engine,
    );
    let mut pixmap = tiny_skia::Pixmap::new(SHARE_CARD_WIDTH, SHARE_CARD_HEIGHT)?;
    {
        let mut target = windui::render::PixmapTarget {
            pixmap: &mut pixmap,
        };
        let mut canvas = target.make_canvas(&mut engine, 1.0);
        tree.paint(canvas.as_mut());
    }
    Some(pixmap)
}

/// Undo premultiplication: clipboard DIB and PNG expect straight channels.
pub(crate) fn unpremultiply_rgba(premultiplied: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(premultiplied.len());
    for px in premultiplied.chunks_exact(4) {
        let a = px[3] as u32;
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else if a == 255 {
            out.extend_from_slice(&[px[0], px[1], px[2], 255]);
        } else {
            out.extend_from_slice(&[
                ((px[0] as u32 * 255 + a / 2) / a).min(255) as u8,
                ((px[1] as u32 * 255 + a / 2) / a).min(255) as u8,
                ((px[2] as u32 * 255 + a / 2) / a).min(255) as u8,
                255,
            ]);
        }
    }
    out
}

/// Straight (non-premultiplied) RGBA bytes plus dimensions, ready for the
/// clipboard DIB payload.
#[cfg(windows)]
pub(crate) fn render_share_card_rgba(
    total: u64,
    version: &str,
    top: &[(String, u64)],
) -> Option<(u32, u32, Vec<u8>)> {
    let pixmap = render_share_card_pixmap(total, version, top)?;
    let width = pixmap.width();
    let height = pixmap.height();
    Some((width, height, unpremultiply_rgba(pixmap.data())))
}

/// PNG bytes for the design mock test below. The Share button uses the RGBA
/// path above, so this stays test-only until a save-to-file feature needs it.
#[cfg(test)]
pub(crate) fn render_share_card_png(
    total: u64,
    version: &str,
    top: &[(String, u64)],
) -> Option<Vec<u8>> {
    let (width, height, rgba) = render_share_card_rgba(total, version, top)?;
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&rgba).ok()?;
    }
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_summary_zero_shows_empty_state() {
        assert_eq!(
            usage_summary(0),
            "No committed searches yet. Run a search to see stats here."
        );
    }

    #[test]
    fn usage_summary_shows_lifetime_total() {
        assert_eq!(usage_summary(41), "Queries run: 41");
    }

    #[test]
    fn stats_texts_compose_all_blocks() {
        let mut launches = HashMap::new();
        launches.insert(
            String::from("application:steam"),
            LaunchCount {
                title: String::from("Steam"),
                count: 3,
            },
        );
        let (usage, top) = stats_texts(&launches, 5);
        assert_eq!(usage, "Queries run: 5");
        assert_eq!(top, "1. Steam: 3");
    }

    fn launch_entry(title: &str, count: u64) -> LaunchCount {
        LaunchCount {
            title: String::from(title),
            count,
        }
    }

    #[test]
    fn top_launches_rank_by_count_then_title() {
        let mut launches = HashMap::new();
        launches.insert(String::from("a:steam"), launch_entry("Steam", 2));
        launches.insert(String::from("a:zip"), launch_entry("ext:zip", 5));
        launches.insert(String::from("a:notes"), launch_entry("Notepad", 5));
        launches.insert(String::from("a:old"), launch_entry("Gone Long Ago", 9));
        let top = top_launches(&launches, 3);
        assert_eq!(
            top,
            vec![
                (String::from("Gone Long Ago"), 9),
                (String::from("Notepad"), 5),
                (String::from("ext:zip"), 5),
            ]
        );
    }

    #[test]
    fn top_launches_block_empty_state() {
        assert_eq!(top_launches_block(&HashMap::new()), "No data yet.");
    }

    #[test]
    fn share_stats_text_carries_total_but_no_query_contents() {
        let text = share_stats_text(128);
        assert!(text.contains("128"));
        assert!(!text.contains("steam"));
        assert!(!text.contains("ext:zip"));
        assert!(text.contains("https://github.com/m1nuzz/flux-launcher"));
    }

    #[test]
    fn format_count_groups_thousands() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(41), "41");
        assert_eq!(format_count(1284), "1,284");
        assert_eq!(format_count(1_000_000), "1,000,000");
    }

    #[test]
    fn unpremultiply_rgba_restores_straight_channels() {
        assert_eq!(unpremultiply_rgba(&[10, 20, 30, 0]), vec![0, 0, 0, 0]);
        assert_eq!(
            unpremultiply_rgba(&[10, 20, 30, 255]),
            vec![10, 20, 30, 255]
        );
        // Half-transparent red (128,0,0,128) straightens back to (255,0,0,255).
        assert_eq!(unpremultiply_rgba(&[128, 0, 0, 128]), vec![255, 0, 0, 255]);
    }

    #[test]
    fn clear_history_state_forgets_queries_and_refreshes_display() {
        use flux_core::Settings;

        let settings = Arc::new(RwLock::new(Settings::default()));
        {
            let mut guard = settings.write().unwrap();
            guard.record_query("steam");
            guard.record_launch("application:steam", "Steam");
        }
        let history = Rc::new(RefCell::new(vec![String::from("steam")]));
        let cursor = signal(Some(3_usize));
        let usage = signal(String::new());
        let top = signal(String::new());
        assert!(clear_history_state(&settings, &history, cursor, usage, top));
        assert!(history.borrow().is_empty());
        assert_eq!(cursor.get(), None);
        assert!(settings.read().unwrap().query_history.is_empty());
        assert!(settings.read().unwrap().launch_counts.is_empty());
        assert_eq!(
            usage.get(),
            "No committed searches yet. Run a search to see stats here."
        );
        assert_eq!(top.get(), "No data yet.");
    }

    #[cfg(windows)]
    #[test]
    fn share_card_mock_writes_png_for_review() {
        let top = vec![
            (String::from("steam"), 42),
            (String::from("ext:zip"), 27),
            (String::from("notepad"), 13),
            (String::from("calculator"), 8),
            (String::from("spotify"), 5),
        ];
        let png = render_share_card_png(1284, "0.1.128", &top).expect("share card should render");
        let decoded = tiny_skia::Pixmap::decode_png(&png).expect("valid PNG output");
        assert_eq!(decoded.width(), SHARE_CARD_WIDTH);
        assert_eq!(decoded.height(), SHARE_CARD_HEIGHT);
        let path = std::env::temp_dir().join("flux-stats-card-mock.png");
        std::fs::write(&path, &png).unwrap();
        eprintln!("CARD_MOCK={}", path.display());
    }
}
