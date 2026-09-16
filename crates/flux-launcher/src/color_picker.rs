use std::sync::{Arc, RwLock};

use flux_core::Settings;
use windui::app::ThemeHandle;
use windui::core::{EventCtx, Widget};
use windui::event::{Event, Key, MouseButton, PointerKind};
use windui::geometry::{Rect, Size};
use windui::prelude::*;
use windui::render::{Canvas, Gradient, Paint};
use windui::style::Style;
use windui::text::TextEngine;

use super::theme_text::{
    hsv_to_rgb, hsv_to_selection_u32, push_selection_appearance, rgb_to_hsv, selection_color_hex,
};
use super::update_tasks::save_settings;

pub(crate) const SV_PAD_WIDTH: i32 = 176;
pub(crate) const SV_PAD_HEIGHT: i32 = 148;
pub(crate) const HUE_STRIP_WIDTH: i32 = 22;
pub(crate) const PREVIEW_SWATCH: i32 = 44;

/// Pointer position to saturation/value fractions inside a pad, clamped.
pub(crate) fn sv_at(x: f32, y: f32, width: f32, height: f32) -> (f32, f32) {
    (
        (x / width.max(1.0)).clamp(0.0, 1.0),
        (1.0 - y / height.max(1.0)).clamp(0.0, 1.0),
    )
}

/// Pointer y to a hue in degrees inside a vertical strip, clamped.
pub(crate) fn hue_at(y: f32, height: f32) -> f32 {
    (y / height.max(1.0)).clamp(0.0, 1.0) * 360.0
}

fn commit_hsv(
    theme: &ThemeHandle,
    hsv: Signal<(f32, f32, f32)>,
    hex: Signal<String>,
    live: Signal<Color>,
    next: (f32, f32, f32),
) {
    let rgb = hsv_to_selection_u32(next.0, next.1, next.2);
    hsv.set(next);
    hex.set(selection_color_hex(rgb));
    push_selection_appearance(
        theme,
        live,
        (
            ((rgb >> 16) & 0xff) as u8,
            ((rgb >> 8) & 0xff) as u8,
            (rgb & 0xff) as u8,
        ),
    );
}

fn persist_current_color(
    hsv: Signal<(f32, f32, f32)>,
    hex: Signal<String>,
    shared_settings: &Arc<RwLock<Settings>>,
) {
    let (hue, saturation, value) = hsv.get();
    let rgb = hsv_to_selection_u32(hue, saturation, value);
    let Ok(mut settings) = shared_settings.write() else {
        return;
    };
    settings.custom_selection_color = rgb;
    if save_settings(&settings) {
        hex.set(selection_color_hex(rgb));
    }
}

/// Photoshop-style saturation/value square for the currently selected hue.
/// Dragging updates the hex field and the live row highlight; releasing the
/// pointer persists the choice so scrubbing does not rewrite settings.json.
pub(crate) struct SaturationValuePad {
    theme: ThemeHandle,
    hsv: Signal<(f32, f32, f32)>,
    hex: Signal<String>,
    live: Signal<Color>,
    shared_settings: Arc<RwLock<Settings>>,
    dragging: bool,
    last: (f32, f32, f32),
}

impl SaturationValuePad {
    fn apply_at(&self, ctx: &mut EventCtx, x: f32, y: f32) {
        let bounds = ctx.bounds();
        let (saturation, value) = sv_at(
            x - bounds.x as f32,
            y - bounds.y as f32,
            bounds.w as f32,
            bounds.h as f32,
        );
        let (hue, _, _) = self.hsv.get();
        commit_hsv(
            &self.theme,
            self.hsv,
            self.hex,
            self.live,
            (hue, saturation, value),
        );
        ctx.mark_dirty();
    }
}

impl Widget for SaturationValuePad {
    fn measure(&self, _avail: Size, _style: &Style, _text: &mut dyn TextEngine) -> Size {
        Size::new(SV_PAD_WIDTH, SV_PAD_HEIGHT)
    }
    fn on_update(&mut self, ctx: &mut EventCtx) {
        // Repaint when the hex field, a preset, or the hue strip moved the
        // shared HSV state (e.g. typing a hex value must move this marker).
        let current = self.hsv.get();
        if current != self.last {
            self.last = current;
            ctx.mark_dirty();
        }
    }

    fn on_event(&mut self, ctx: &mut EventCtx, event: &Event) -> bool {
        match event {
            Event::Pointer(pointer) => match pointer.kind {
                PointerKind::Down if pointer.button == MouseButton::Left => {
                    ctx.request_focus();
                    ctx.capture();
                    self.dragging = true;
                    self.apply_at(ctx, pointer.pos.x as f32, pointer.pos.y as f32);
                    true
                }
                PointerKind::Move if self.dragging => {
                    self.apply_at(ctx, pointer.pos.x as f32, pointer.pos.y as f32);
                    true
                }
                PointerKind::Up if pointer.button == MouseButton::Left => {
                    let was_dragging = self.dragging;
                    self.dragging = false;
                    ctx.release_capture();
                    if was_dragging {
                        persist_current_color(self.hsv, self.hex, &self.shared_settings);
                    }
                    ctx.mark_dirty();
                    true
                }
                _ => false,
            },
            Event::Key(key) if key.pressed => {
                let (hue, saturation, value) = self.hsv.get();
                let step = 0.02;
                let next = match key.key {
                    Key::Left => (hue, (saturation - step).max(0.0), value),
                    Key::Right => (hue, (saturation + step).min(1.0), value),
                    Key::Up => (hue, saturation, (value + step).min(1.0)),
                    Key::Down => (hue, saturation, (value - step).max(0.0)),
                    _ => return false,
                };
                commit_hsv(&self.theme, self.hsv, self.hex, self.live, next);
                persist_current_color(self.hsv, self.hex, &self.shared_settings);
                ctx.mark_dirty();
                true
            }
            _ => false,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn paint(
        &self,
        bounds: Rect,
        _content: Rect,
        _focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        _style: &Style,
    ) {
        let x = bounds.x as f32;
        let y = bounds.y as f32;
        let w = bounds.w as f32;
        let h = bounds.h as f32;
        let (hue, saturation, value) = self.hsv.get();
        let (br, bg, bb) = hsv_to_rgb(hue, 1.0, 1.0);
        let base = Color::rgb(br, bg, bb);
        let white = Color::rgba(255, 255, 255, 255);
        let clear_white = Color::rgba(255, 255, 255, 0);
        let clear_black = Color::rgba(0, 0, 0, 0);
        let black = Color::rgba(0, 0, 0, 255);
        canvas.fill_round_rect(x, y, w, h, 8.0, &Paint::fill(base));
        canvas.fill_round_rect(
            x,
            y,
            w,
            h,
            8.0,
            &Paint::gradient(Gradient::linear(
                (0.0, 0.0),
                (1.0, 0.0),
                vec![(0.0, white), (1.0, clear_white)],
            )),
        );
        canvas.fill_round_rect(
            x,
            y,
            w,
            h,
            8.0,
            &Paint::gradient(Gradient::linear(
                (0.0, 0.0),
                (0.0, 1.0),
                vec![(0.0, clear_black), (1.0, black)],
            )),
        );
        let marker_x = x + saturation.clamp(0.0, 1.0) * w;
        let marker_y = y + (1.0 - value.clamp(0.0, 1.0)) * h;
        canvas.fill_circle(marker_x, marker_y, 6.0, &Paint::fill(white));
        let (cr, cg, cb) = hsv_to_rgb(hue, saturation, value);
        canvas.fill_circle(
            marker_x,
            marker_y,
            4.0,
            &Paint::fill(Color::rgb(cr, cg, cb)),
        );
        canvas.stroke_round_rect(
            x,
            y,
            w,
            h,
            8.0,
            1.0,
            &Paint::fill(Color::rgba(255, 255, 255, 40)),
        );
    }
}

/// Vertical hue strip (red to red through the spectrum) driving the shared HSV hue.
pub(crate) struct HueStrip {
    theme: ThemeHandle,
    hsv: Signal<(f32, f32, f32)>,
    hex: Signal<String>,
    live: Signal<Color>,
    shared_settings: Arc<RwLock<Settings>>,
    dragging: bool,
    last_hue: f32,
}

impl HueStrip {
    fn rainbow_stops() -> Vec<(f32, Color)> {
        [
            (0.0, (255, 0, 0)),
            (1.0 / 6.0, (255, 255, 0)),
            (2.0 / 6.0, (0, 255, 0)),
            (3.0 / 6.0, (0, 255, 255)),
            (4.0 / 6.0, (0, 0, 255)),
            (5.0 / 6.0, (255, 0, 255)),
            (1.0, (255, 0, 0)),
        ]
        .into_iter()
        .map(|(offset, (r, g, b))| (offset, Color::rgb(r, g, b)))
        .collect()
    }

    fn apply_at(&self, ctx: &mut EventCtx, y: f32) {
        let bounds = ctx.bounds();
        let hue = hue_at(y - bounds.y as f32, bounds.h as f32);
        let (_, saturation, value) = self.hsv.get();
        commit_hsv(
            &self.theme,
            self.hsv,
            self.hex,
            self.live,
            (hue, saturation, value),
        );
        ctx.mark_dirty();
    }
}

impl Widget for HueStrip {
    fn measure(&self, _avail: Size, _style: &Style, _text: &mut dyn TextEngine) -> Size {
        Size::new(HUE_STRIP_WIDTH, SV_PAD_HEIGHT)
    }

    fn on_update(&mut self, ctx: &mut EventCtx) {
        let (hue, _, _) = self.hsv.get();
        if (hue - self.last_hue).abs() > f32::EPSILON {
            self.last_hue = hue;
            ctx.mark_dirty();
        }
    }

    fn on_event(&mut self, ctx: &mut EventCtx, event: &Event) -> bool {
        match event {
            Event::Pointer(pointer) => match pointer.kind {
                PointerKind::Down if pointer.button == MouseButton::Left => {
                    ctx.request_focus();
                    ctx.capture();
                    self.dragging = true;
                    self.apply_at(ctx, pointer.pos.y as f32);
                    true
                }
                PointerKind::Move if self.dragging => {
                    self.apply_at(ctx, pointer.pos.y as f32);
                    true
                }
                PointerKind::Up if pointer.button == MouseButton::Left => {
                    let was_dragging = self.dragging;
                    self.dragging = false;
                    ctx.release_capture();
                    if was_dragging {
                        persist_current_color(self.hsv, self.hex, &self.shared_settings);
                    }
                    ctx.mark_dirty();
                    true
                }
                _ => false,
            },
            Event::Key(key) if key.pressed => {
                let (hue, saturation, value) = self.hsv.get();
                let next = match key.key {
                    Key::Up | Key::Left => ((hue - 3.0).rem_euclid(360.0), saturation, value),
                    Key::Down | Key::Right => ((hue + 3.0).rem_euclid(360.0), saturation, value),
                    _ => return false,
                };
                commit_hsv(&self.theme, self.hsv, self.hex, self.live, next);
                persist_current_color(self.hsv, self.hex, &self.shared_settings);
                ctx.mark_dirty();
                true
            }
            _ => false,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn paint(
        &self,
        bounds: Rect,
        _content: Rect,
        _focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        _style: &Style,
    ) {
        let x = bounds.x as f32;
        let y = bounds.y as f32;
        let w = bounds.w as f32;
        let h = bounds.h as f32;
        canvas.fill_round_rect(
            x,
            y,
            w,
            h,
            6.0,
            &Paint::gradient(Gradient::linear(
                (0.0, 0.0),
                (0.0, 1.0),
                Self::rainbow_stops(),
            )),
        );
        let (hue, _, _) = self.hsv.get();
        let marker_y = y + (hue.rem_euclid(360.0) / 360.0) * h;
        canvas.fill_rect(x, marker_y - 1.5, w, 3.0, &Paint::fill(Color::WHITE));
        canvas.fill_rect(
            x,
            marker_y - 0.5,
            w,
            1.0,
            &Paint::fill(Color::rgba(0, 0, 0, 160)),
        );
    }
}

/// Small live swatch mirroring the effective row-highlight color.
pub(crate) struct ColorPreview {
    live: Signal<Color>,
    last: Color,
}

impl Widget for ColorPreview {
    fn measure(&self, _avail: Size, _style: &Style, _text: &mut dyn TextEngine) -> Size {
        Size::new(PREVIEW_SWATCH, PREVIEW_SWATCH)
    }

    fn on_update(&mut self, ctx: &mut EventCtx) {
        let current = self.live.get();
        if current != self.last {
            self.last = current;
            ctx.mark_dirty();
        }
    }

    fn paint(
        &self,
        bounds: Rect,
        _content: Rect,
        _focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        _style: &Style,
    ) {
        canvas.fill_round_rect(
            bounds.x as f32,
            bounds.y as f32,
            bounds.w as f32,
            bounds.h as f32,
            8.0,
            &Paint::fill(self.live.get()),
        );
        canvas.stroke_round_rect(
            bounds.x as f32,
            bounds.y as f32,
            bounds.w as f32,
            bounds.h as f32,
            8.0,
            1.0,
            &Paint::fill(Color::rgba(255, 255, 255, 40)),
        );
    }
}

/// Photoshop-style picker row: saturation/value square, hue strip, live preview.
/// Lives inside the Selection color section so the hex field, presets and the
/// 2D picker stay together.
pub(crate) fn build_color_picker(
    custom_selection_color: Signal<String>,
    color_hsv: Signal<(f32, f32, f32)>,
    selection_color: Signal<Color>,
    theme: &ThemeHandle,
    shared_settings: Arc<RwLock<Settings>>,
) -> Element {
    Element::col()
        .width_match()
        .spacing(8)
        .child(
            Element::label("Custom color picker")
                .font_size(13.0)
                .fg(Color::WHITE),
        )
        .child(
            Element::row()
                .width_match()
                .spacing(10)
                .child(
                    Element::leaf()
                        .widget(SaturationValuePad {
                            theme: theme.clone(),
                            hsv: color_hsv,
                            hex: custom_selection_color,
                            live: selection_color,
                            shared_settings: Arc::clone(&shared_settings),
                            dragging: false,
                            last: color_hsv.get(),
                        })
                        .reactive()
                        .width(SV_PAD_WIDTH)
                        .height(SV_PAD_HEIGHT),
                )
                .child(
                    Element::leaf()
                        .widget(HueStrip {
                            theme: theme.clone(),
                            hsv: color_hsv,
                            hex: custom_selection_color,
                            live: selection_color,
                            shared_settings: Arc::clone(&shared_settings),
                            dragging: false,
                            last_hue: color_hsv.get().0,
                        })
                        .reactive()
                        .width(HUE_STRIP_WIDTH)
                        .height(SV_PAD_HEIGHT),
                )
                .child(
                    Element::col()
                        .spacing(6)
                        .child(
                            Element::leaf()
                                .widget(ColorPreview {
                                    live: selection_color,
                                    last: selection_color.get(),
                                })
                                .reactive()
                                .width(PREVIEW_SWATCH)
                                .height(PREVIEW_SWATCH)
                                .tooltip("Current selection color"),
                        )
                        .child(
                            Element::label_signal(custom_selection_color)
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 180)),
                        ),
                ),
        )
}

/// Seed HSV state from a saved 0xRRGGBB custom color.
pub(crate) fn hsv_for_selection_u32(value: u32) -> (f32, f32, f32) {
    rgb_to_hsv(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sv_mapping_clamps_to_unit_square() {
        assert_eq!(sv_at(0.0, 0.0, 100.0, 100.0), (0.0, 1.0));
        assert_eq!(sv_at(100.0, 100.0, 100.0, 100.0), (1.0, 0.0));
        assert_eq!(sv_at(-5.0, 500.0, 100.0, 100.0), (0.0, 0.0));
        assert_eq!(sv_at(50.0, 25.0, 100.0, 100.0), (0.5, 0.75));
    }

    #[test]
    fn hue_mapping_spans_red_to_red() {
        assert_eq!(hue_at(0.0, 200.0), 0.0);
        assert_eq!(hue_at(200.0, 200.0), 360.0);
        assert_eq!(hue_at(100.0, 200.0), 180.0);
        assert_eq!(hue_at(-10.0, 200.0), 0.0);
    }

    #[test]
    fn hsv_seed_matches_saved_custom_color() {
        let (h, s, v) = hsv_for_selection_u32(0x4c8bf4);
        let (r, g, b) = hsv_to_rgb(h, s, v);
        assert!((i16::from(r) - 0x4c).abs() <= 1);
        assert!((i16::from(g) - 0x8b).abs() <= 1);
        assert!((i16::from(b) - 0xf4).abs() <= 1);
    }
}
