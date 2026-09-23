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

/// Hero number size adapts to magnitude so both total=2 and total=1284000
/// fill their column without clipping or dwarfing.
pub(crate) fn hero_font_size(total: u64) -> f32 {
    if total < 100 {
        190.0
    } else if total < 10_000 {
        150.0
    } else {
        115.0
    }
}

/// Full length of the inline share bar in the top list.
pub(crate) const BAR_TRACK_WIDTH: i32 = 170;

/// Fill length of one share bar, scaled against the busiest row so bars
/// compare with each other: (42, 42, 170) -> 170, (21, 42, 170) -> 85.
/// A nonzero count never rounds away to zero.
pub(crate) fn bar_fill_width(count: u64, max: u64, track: i32) -> i32 {
    if max == 0 || track <= 0 {
        return 0;
    }
    let scaled = (count as f32 / max as f32 * track as f32).round() as i32;
    scaled.clamp(1, track)
}

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

/// Procedural deep-space backdrop: vertical base gradient, two nebula
/// glows, one deterministic starfield. Pure raster work, no platform or
/// text engine involved, so this is unit-testable anywhere.
pub(crate) fn paint_space_background(pixmap: &mut tiny_skia::Pixmap) {
    use tiny_skia::{
        BlendMode, Color as SkColor, FillRule, GradientStop, LinearGradient, Paint as SkPaint,
        PathBuilder, Point, RadialGradient, Rect, Shader, SpreadMode, Transform,
    };

    let w = pixmap.width() as f32;
    let h = pixmap.height() as f32;
    let full = Rect::from_xywh(0.0, 0.0, w, h).expect("share card has nonzero size");
    let identity = Transform::identity();
    let mut paint = SkPaint {
        blend_mode: BlendMode::SourceOver,
        anti_alias: true,
        ..SkPaint::default()
    };

    // Base: midnight top melting into deep blue.
    let base = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(0.0, h),
        vec![
            GradientStop::new(0.0, SkColor::from_rgba8(4, 7, 22, 255)),
            GradientStop::new(1.0, SkColor::from_rgba8(13, 22, 54, 255)),
        ],
        SpreadMode::Pad,
        identity,
    )
    .expect("two stops always build");
    paint.shader = base;
    pixmap.fill_rect(full, &paint, identity, None);

    // Nebula glows: violet upper-right, teal lower-left. Transparent edge +
    // Pad spread keeps the falloff seamless.
    for (cx, cy, radius, r, g, b, alpha) in [
        (950.0, 130.0, 380.0, 96u8, 110u8, 255u8, 64u8),
        (170.0, 520.0, 330.0, 46u8, 150u8, 225u8, 52u8),
    ] {
        let glow = RadialGradient::new(
            Point::from_xy(cx, cy),
            0.0,
            Point::from_xy(cx, cy),
            radius,
            vec![
                GradientStop::new(0.0, SkColor::from_rgba8(r, g, b, alpha)),
                GradientStop::new(1.0, SkColor::from_rgba8(r, g, b, 0)),
            ],
            SpreadMode::Pad,
            identity,
        );
        if let Some(shader) = glow {
            paint.shader = shader;
            pixmap.fill_rect(full, &paint, identity, None);
        }
    }

    // Deterministic starfield: fixed seed, same sky on every render.
    // Stars inside text zones are dimmed and shrunk so small type stays
    // legible: header, left hero block, right top list, footer strip.
    let in_text_zone = |x: f32, y: f32| {
        let header = (60.0..=1150.0).contains(&x) && (30.0..=120.0).contains(&y);
        let hero = (80.0..=730.0).contains(&x) && (140.0..=480.0).contains(&y);
        let list = (720.0..=1150.0).contains(&x) && (80.0..=540.0).contains(&y);
        let footer = (60.0..=1150.0).contains(&x) && (530.0..=610.0).contains(&y);
        header || hero || list || footer
    };
    let mut rng: u64 = 0x9E3779B97F4A7C15;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    for _ in 0..130 {
        let x = (next() % 1200) as f32;
        let y = (next() % 630) as f32;
        let (mut radius, mut alpha): (f32, u8) = match next() % 10 {
            0..=5 => (0.8, 110u8),
            6..=7 => (1.2, 160u8),
            8 => (1.7, 205u8),
            _ => (2.2, 235u8),
        };
        if in_text_zone(x, y) {
            radius = radius.min(1.2);
            alpha = alpha.min(80);
        }
        if let Some(circle) = PathBuilder::from_circle(x, y, radius) {
            paint.shader = Shader::SolidColor(SkColor::from_rgba8(255, 255, 255, alpha));
            pixmap.fill_path(&circle, &paint, FillRule::Winding, identity, None);
        }
    }
}

/// Share card layout: one hero total, the top opened list, one footer line.
/// The backdrop is painted procedurally (see `paint_space_background`), so the
/// tree only carries text and the hairline. The middle band grows to absorb
/// leftover height and centers its columns, so slack splits above and below
/// the content instead of pooling under it — the footer stays on the last line.
pub(crate) fn build_share_card(total: u64, version: &str, top: &[(String, u64)]) -> Element {
    let max_count = top.iter().map(|(_, count)| *count).max().unwrap_or(0);
    let mut list = Element::col().width(560).spacing(18);
    if top.is_empty() {
        list = list.child(
            Element::label("No data yet.")
                .font_size(28.0)
                .fg(Color::rgb(180, 195, 230)),
        );
    }
    for (title, count) in top {
        // Every slot has a fixed width (300 + 14 + 170 + 14 + 60 = the list
        // width): a flexible title would absorb the differing count widths
        // and slide the track sideways from row to row.
        let fill = bar_fill_width(*count, max_count, BAR_TRACK_WIDTH);
        list = list.child(
            Element::row()
                .width_match()
                .cross(Align::Center)
                .spacing(14)
                .child(
                    Element::label(title.clone())
                        .font_size(30.0)
                        .font_weight(600)
                        .fg(Color::WHITE)
                        .max_lines(1)
                        .truncate(Truncate::End)
                        .width(300),
                )
                .child(
                    Element::row()
                        .width(BAR_TRACK_WIDTH)
                        .height(8)
                        .cross(Align::Center)
                        .bg(Color::rgba(255, 255, 255, 26))
                        .corner(4.0)
                        .child(
                            Element::leaf()
                                .width(fill)
                                .height(8)
                                .corner(4.0)
                                .bg_gradient(Gradient::linear(
                                    (0.0, 0.0),
                                    (1.0, 0.0),
                                    vec![
                                        (0.0, Color::rgb(105, 155, 255)),
                                        (1.0, Color::rgb(140, 110, 255)),
                                    ],
                                )),
                        ),
                )
                .child(
                    Element::label(format_count(*count))
                        .font_size(30.0)
                        .font_weight(700)
                        .fg(Color::rgb(143, 176, 255))
                        .width(60)
                        .text_align(Align::End),
                ),
        );
    }
    Element::col()
        .width(SHARE_CARD_WIDTH as i32)
        .height(SHARE_CARD_HEIGHT as i32)
        .padding_edges(64, 56, 60, 56)
        .spacing(22)
        .child(
            Element::label("FLUX LAUNCHER")
                .font_size(22.0)
                .font_weight(700)
                .fg(Color::rgb(180, 195, 230)),
        )
        .child(
            Element::row()
                .width_match()
                .weight(1.0)
                .cross(Align::Center)
                .spacing(56)
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(14)
                        .child(
                            Element::label(format_count(total))
                                .font_size(hero_font_size(total))
                                .font_weight(700)
                                .fg(Color::WHITE),
                        )
                        .child(
                            Element::label("QUERIES RUN")
                                .font_size(22.0)
                                .font_weight(700)
                                .fg(Color::rgb(180, 195, 230)),
                        ),
                )
                .child(list),
        )
        .child(
            Element::leaf()
                .width_match()
                .height(1)
                .bg(Color::rgba(255, 255, 255, 30)),
        )
        .child(
            Element::label(format!(
                "Flux Launcher v{version} • github.com/m1nuzz/flux-launcher"
            ))
            .font_size(22.0)
            .font_weight(600)
            .fg(Color::rgb(160, 175, 210)),
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
    paint_space_background(&mut pixmap);
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
    fn hero_font_size_adapts_to_digit_count() {
        assert_eq!(hero_font_size(2), 190.0);
        assert_eq!(hero_font_size(1284), 150.0);
        assert_eq!(hero_font_size(1_000_000), 115.0);
    }

    #[test]
    fn bar_fill_width_scales_against_top_row() {
        assert_eq!(bar_fill_width(42, 42, BAR_TRACK_WIDTH), BAR_TRACK_WIDTH);
        assert_eq!(bar_fill_width(21, 42, BAR_TRACK_WIDTH), 85);
        assert_eq!(bar_fill_width(1, 1_000, BAR_TRACK_WIDTH), 1);
        assert_eq!(bar_fill_width(5, 0, BAR_TRACK_WIDTH), 0);
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

    fn channel_near(actual: u8, expected: u8, tolerance: u8) -> bool {
        (actual as i16 - expected as i16).abs() <= tolerance as i16
    }

    #[test]
    fn space_background_has_gradient_nebula_and_stars() {
        let mut pixmap = tiny_skia::Pixmap::new(SHARE_CARD_WIDTH, SHARE_CARD_HEIGHT).unwrap();
        paint_space_background(&mut pixmap);
        // Base vertical gradient: near-black navy top, deep blue bottom.
        let top = pixmap.pixel(600, 4).unwrap();
        assert!(channel_near(top.red(), 5, 4));
        assert!(channel_near(top.green(), 8, 4));
        assert!(channel_near(top.blue(), 26, 6));
        let bottom = pixmap.pixel(600, 625).unwrap();
        assert!(channel_near(bottom.red(), 13, 4));
        assert!(channel_near(bottom.green(), 22, 4));
        assert!(channel_near(bottom.blue(), 54, 6));
        // Violet nebula core lifts the blue channel well above the base.
        let glow = pixmap.pixel(950, 130).unwrap();
        assert!(glow.blue() > 60, "nebula core should glow, got {:?}", glow);
        // Deterministic starfield: white dots exist somewhere.
        let data = pixmap.data();
        let bright = data
            .chunks_exact(4)
            .filter(|px| px[0] > 150 && px[1] > 150 && px[2] > 150)
            .count();
        assert!(bright > 30, "expected star pixels, got {bright}");
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

    #[cfg(windows)]
    #[test]
    fn share_card_mock_small_data_writes_png_for_review() {
        let top = vec![
            (String::from("Grok"), 2),
            (String::from("Google Chrome"), 1),
            (String::from("Perplexity"), 1),
        ];
        let png = render_share_card_png(4, "0.1.128", &top).expect("share card should render");
        let path = std::env::temp_dir().join("flux-stats-card-mock-small.png");
        std::fs::write(&path, &png).unwrap();
        eprintln!("CARD_MOCK_SMALL={}", path.display());
    }
}
