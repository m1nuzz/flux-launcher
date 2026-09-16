use std::sync::{Arc, RwLock};

use flux_core::{ResultKind, SearchResult, Settings};
use windui::core::EventCtx;
use windui::event::Key;
use windui::prelude::*;

use super::accent;
use super::ui_constants::LAUNCHER_FONT_FAMILY;

fn custom_selection_color_rgb(value: u32) -> (u8, u8, u8) {
    (
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

pub(crate) fn selection_color_for_settings(settings: &Settings) -> Color {
    let (r, g, b) = if settings.use_system_accent {
        accent::system_accent_rgb()
            .unwrap_or_else(|| custom_selection_color_rgb(settings.custom_selection_color))
    } else {
        custom_selection_color_rgb(settings.custom_selection_color)
    };
    Color::rgba(r, g, b, 84)
}

pub(crate) fn selection_color_hex(value: u32) -> String {
    format!("#{value:06X}")
}

pub(crate) fn parse_selection_color(value: &str) -> Option<u32> {
    let trimmed = value.trim().trim_start_matches('#');
    (trimmed.len() == 6)
        .then(|| u32::from_str_radix(trimmed, 16).ok())
        .flatten()
}

/// RGB (0-255) to HSV with hue in degrees [0, 360) and saturation/value in [0, 1].
/// Achromatic colors report hue 0 by convention.
pub(crate) fn rgb_to_hsv(red: u8, green: u8, blue: u8) -> (f32, f32, f32) {
    let r = f32::from(red) / 255.0;
    let g = f32::from(green) / 255.0;
    let b = f32::from(blue) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };
    let hue = if hue < 0.0 { hue + 360.0 } else { hue };
    let saturation = if max <= f32::EPSILON {
        0.0
    } else {
        delta / max
    };
    (hue, saturation, max)
}

/// HSV (hue degrees, saturation/value in [0, 1]) to RGB (0-255), rounded.
pub(crate) fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> (u8, u8, u8) {
    let h = hue.rem_euclid(360.0);
    let s = saturation.clamp(0.0, 1.0);
    let v = value.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0).floor() as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

pub(crate) fn hsv_to_selection_u32(hue: f32, saturation: f32, value: f32) -> u32 {
    let (r, g, b) = hsv_to_rgb(hue, saturation, value);
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// Resolve the effective selection color from the Visual-tab controls without
/// touching stored settings: invalid hex falls back to the built-in default,
/// mirroring the historical Apply behavior.
pub(crate) fn resolve_selection_color(use_system_accent: bool, hex_text: &str) -> (u32, Color) {
    let rgb = parse_selection_color(hex_text).unwrap_or(0x4c8bf4);
    let (r, g, b) = if use_system_accent {
        accent::system_accent_rgb().unwrap_or_else(|| custom_selection_color_rgb(rgb))
    } else {
        custom_selection_color_rgb(rgb)
    };
    (rgb, Color::rgba(r, g, b, 84))
}

/// Single staging path for the accent toggle + custom color used by both settings
/// tabs. Writes the flag and parsed color into stored settings, refreshes the
/// live selection signal (result rows repaint from it) and normalizes the hex
/// field. Callers keep their own save call so each Apply writes exactly once.
pub(crate) fn stage_selection_color(
    use_system_accent: Signal<bool>,
    custom_selection_color: Signal<String>,
    selection_color: Signal<Color>,
    shared_settings: &Arc<RwLock<Settings>>,
    ctx: &mut EventCtx,
) -> bool {
    let valid = parse_selection_color(&custom_selection_color.get()).is_some();
    let (rgb, color) =
        resolve_selection_color(use_system_accent.get(), &custom_selection_color.get());
    let Ok(mut settings) = shared_settings.write() else {
        ctx.toast_ok("Could not lock Flux settings");
        return false;
    };
    settings.use_system_accent = use_system_accent.get();
    settings.custom_selection_color = rgb;
    selection_color.set(color);
    custom_selection_color.set(selection_color_hex(rgb));
    if !valid {
        ctx.toast_ok("Invalid hex color, restored default #4C8BF4");
    }
    true
}

pub(crate) fn selection_palette(
    custom_selection_color: Signal<String>,
    color_hsv: Signal<(f32, f32, f32)>,
    selection_color: Signal<Color>,
) -> Element {
    const COLORS: &[u32] = &[
        0x4c8bf4, 0x0078d4, 0x00a4ef, 0x107c10, 0x498205, 0xffb900, 0xd83b01, 0xe74856, 0x8764b8,
        0x744da9, 0x038387, 0x605e5c,
    ];
    let mut row = Element::row().spacing(6).width_match();
    for &value in COLORS {
        let label = selection_color_hex(value);
        let (r, g, b) = custom_selection_color_rgb(value);
        let (hue, saturation, brightness) = rgb_to_hsv(r, g, b);
        row = row.child(
            Element::col()
                .width(24)
                .height(24)
                .bg(Color::rgb(
                    ((value >> 16) & 0xff) as u8,
                    ((value >> 8) & 0xff) as u8,
                    (value & 0xff) as u8,
                ))
                .corner(6.0)
                .clickable()
                .tooltip(label)
                .on_click(move |_| {
                    custom_selection_color.set(selection_color_hex(value));
                    color_hsv.set((hue, saturation, brightness));
                    selection_color.set(Color::rgba(r, g, b, 84));
                }),
        );
    }
    row
}

pub(crate) fn display_title(title: &str) -> String {
    const MAX_TITLE_CHARS: usize = 26;
    let chars: Vec<char> = title.chars().collect();
    if chars.len() <= MAX_TITLE_CHARS {
        return title.to_owned();
    }

    let extension_start = title
        .char_indices()
        .rev()
        .find_map(|(index, character)| (character == '.' && index > 0).then_some(index));
    let (stem, extension) = extension_start
        .map(|index| title.split_at(index))
        .unwrap_or((title, ""));
    let extension_chars: Vec<char> = extension.chars().collect();
    let available_stem_chars = MAX_TITLE_CHARS
        .saturating_sub(extension_chars.len())
        .saturating_sub(1);
    if available_stem_chars < 2 {
        return chars
            .into_iter()
            .take(MAX_TITLE_CHARS.saturating_sub(1))
            .chain(std::iter::once('…'))
            .collect();
    }

    let stem_chars: Vec<char> = stem.chars().collect();
    let prefix_len = available_stem_chars.div_ceil(2);
    let suffix_len = available_stem_chars / 2;
    stem_chars
        .iter()
        .take(prefix_len)
        .chain(std::iter::once(&'…'))
        .chain(
            stem_chars
                .iter()
                .skip(stem_chars.len().saturating_sub(suffix_len)),
        )
        .copied()
        .chain(extension_chars)
        .collect()
}

pub(crate) fn title_match_doc(title: &str, query: &str) -> RichDoc {
    // Follow the Windows 11 type hierarchy: regular body text, with stronger
    // weight reserved for the characters matched by the current query.
    let normal = SpanStyle::new()
        .family(LAUNCHER_FONT_FAMILY)
        .weight(400)
        .fg(Color::rgba(255, 255, 255, 255));
    let matched = SpanStyle::new()
        .family(LAUNCHER_FONT_FAMILY)
        .weight(650)
        .fg(Color::rgba(255, 255, 255, 255));
    let mut para = Para::new();

    let query_chars: Vec<char> = query
        .trim()
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    let display_title = display_title(title);
    let title_chars: Vec<char> = display_title.chars().collect();
    let mut matched_flags = vec![false; title_chars.len()];
    let mut query_index = 0;
    for (index, character) in title_chars.iter().enumerate() {
        if query_index < query_chars.len()
            && character
                .to_lowercase()
                .eq(query_chars[query_index].to_lowercase())
        {
            matched_flags[index] = true;
            query_index += 1;
        }
    }

    let mut start = 0;
    while start < title_chars.len() {
        let is_match = matched_flags[start];
        let mut end = start + 1;
        while end < title_chars.len() && matched_flags[end] == is_match {
            end += 1;
        }
        let text: String = title_chars[start..end].iter().collect();
        para = para.span(
            text,
            if is_match {
                matched.clone()
            } else {
                normal.clone()
            },
        );
        start = end;
    }
    RichDoc::new().para(para)
}

pub(crate) fn normalize_everything_query(query: &str) -> String {
    let trimmed = query.trim();
    let Some(rest) = trimmed.strip_prefix('.') else {
        return trimmed.to_owned();
    };
    let (extension, remainder) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    if extension.is_empty()
        || !extension
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return trimmed.to_owned();
    }
    let remainder = remainder.trim();
    if remainder.is_empty() {
        format!("ext:{extension}")
    } else {
        format!("ext:{extension} {remainder}")
    }
}

pub(crate) fn inline_completion_suffix(query: &str, results: &[SearchResult]) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let query_lower = trimmed.to_lowercase();
    let query_len = trimmed.chars().count();
    results
        .iter()
        .filter(|result| matches!(result.kind, ResultKind::Application))
        .find_map(|result| {
            let title_lower = result.title.to_lowercase();
            if !title_lower.starts_with(&query_lower) {
                return None;
            }
            Some(result.title.chars().skip(query_len).collect())
        })
        .unwrap_or_default()
}

pub(crate) fn history_cursor_step(
    history_len: usize,
    cursor: Option<usize>,
    key: Key,
) -> Option<usize> {
    if history_len == 0 {
        return None;
    }
    Some(match (key, cursor) {
        (Key::Up, Some(index)) => index.saturating_sub(1),
        (Key::Down, Some(index)) => (index + 1).min(history_len - 1),
        (_, _) => history_len - 1,
    })
}

pub(crate) fn launcher_theme() -> Theme {
    let mut theme = Theme::dark();
    theme.palette.bg = Color::rgba(0, 0, 0, 0);
    theme.palette.surface = Color::rgba(38, 39, 41, 180);
    theme.palette.surface_alt = Color::rgba(48, 49, 51, 205);
    theme.palette.border = Color::rgba(255, 255, 255, 22);
    // The Search control is transparent, so its foreground must stay readable
    // over both dark and light Acrylic samples. Keep ordinary text neutral and
    // opaque; reserve accent blue for selection/focus feedback only.
    theme.palette.text = Color::rgba(250, 252, 255, 255);
    theme.palette.placeholder = Color::rgba(238, 243, 255, 230);
    theme.input.bg = Some(Color::rgba(29, 30, 32, 188));
    theme.input.border = Some(Color::rgba(255, 255, 255, 24));
    theme.input.border_focus = Some(Color::rgba(133, 181, 255, 135));
    theme.input.text = Some(Color::rgba(250, 252, 255, 255));
    theme.input.placeholder = Some(Color::rgba(238, 243, 255, 230));
    theme.input.selection = Some(Color::rgba(76, 139, 245, 150));
    theme.input.cursor = Some(Color::rgba(255, 255, 255, 255));
    theme
}

#[cfg(test)]
mod tests {
    use super::*;
    use windui::event::Key;

    #[test]
    fn history_cursor_walks_older_and_newer_queries() {
        let mut cursor = None;
        cursor = history_cursor_step(4, cursor, Key::Up);
        assert_eq!(cursor, Some(3));
        cursor = history_cursor_step(4, cursor, Key::Up);
        assert_eq!(cursor, Some(2));
        cursor = history_cursor_step(4, cursor, Key::Up);
        assert_eq!(cursor, Some(1));
        cursor = history_cursor_step(4, cursor, Key::Down);
        assert_eq!(cursor, Some(2));
        cursor = history_cursor_step(4, cursor, Key::Down);
        assert_eq!(cursor, Some(3));
        cursor = history_cursor_step(4, cursor, Key::Down);
        assert_eq!(cursor, Some(3));
        assert_eq!(history_cursor_step(0, cursor, Key::Up), None);
    }

    #[test]
    fn extension_aliases_normalize_only_the_everything_query_prefix() {
        assert_eq!(normalize_everything_query(".zip"), "ext:zip");
        assert_eq!(
            normalize_everything_query(".mp4 something"),
            "ext:mp4 something"
        );
        assert_eq!(
            normalize_everything_query("  .pdf  report  "),
            "ext:pdf report"
        );
        assert_eq!(normalize_everything_query("ext:zip"), "ext:zip");
        assert_eq!(normalize_everything_query("settings"), "settings");
        assert_eq!(normalize_everything_query("."), ".");
    }

    #[test]
    fn display_title_keeps_extension_and_filename_ending_visible() {
        let first = display_title("finishлицензии_0019.veg");
        let second = display_title("finishлицензии_0019_Untitled Timeline.veg");
        assert_eq!(first, "finishлицензии_0019.veg");
        assert!(first.ends_with(".veg"));
        assert!(second.ends_with(".veg"));
        assert!(second.contains("Timeline"));
        assert_ne!(first, second);
    }

    #[test]
    fn display_title_uses_middle_ellipsis_for_long_names() {
        let displayed = display_title("finishлицензии_0019_Untitled Timeline.veg");
        assert!(displayed.contains('…'));
        assert!(displayed.ends_with(".veg"));
        assert!(displayed.starts_with("finish"));
        assert!(displayed.contains("Timeline"));
        assert!(displayed.chars().count() <= 26);
    }

    #[test]
    fn hsv_round_trips_through_primary_and_neutral_colors() {
        for (r, g, b) in [
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            (255, 255, 255),
            (0, 0, 0),
            (128, 128, 128),
            (76, 139, 244),
        ] {
            let (h, s, v) = rgb_to_hsv(r, g, b);
            let (rr, gg, bb) = hsv_to_rgb(h, s, v);
            assert!(
                (i16::from(rr) - i16::from(r)).abs() <= 1
                    && (i16::from(gg) - i16::from(g)).abs() <= 1
                    && (i16::from(bb) - i16::from(b)).abs() <= 1,
                "round trip drifted for ({r},{g},{b})"
            );
        }
        let (h, _, _) = rgb_to_hsv(255, 0, 0);
        assert!((h - 0.0).abs() < 0.5 || (h - 360.0).abs() < 0.5);
        let (_, s, _) = rgb_to_hsv(128, 128, 128);
        assert_eq!(s, 0.0);
    }

    #[test]
    fn resolve_selection_color_prefers_valid_hex_and_falls_back() {
        let (rgb, color) = resolve_selection_color(false, "#4C8BF4");
        assert_eq!(rgb, 0x4c8bf4);
        assert_eq!(color, Color::rgba(0x4c, 0x8b, 0xf4, 84));
        let (fallback_rgb, _) = resolve_selection_color(false, "not-a-color");
        assert_eq!(fallback_rgb, 0x4c8bf4);
        assert_eq!(selection_color_hex(fallback_rgb), "#4C8BF4");
    }
}
