#![cfg_attr(windows, windows_subsystem = "windows")]

mod fullscreen;

use flux_core::Settings;
use windui::prelude::*;

fn main() {
    let _settings = Settings::load_or_default();
    let content = Element::col()
        .fill()
        .padding(28)
        .spacing(12)
        .child(
            Element::label("Flux Launcher")
                .font_size(28.0)
                .fg(Color::WHITE)
                .width_match(),
        )
        .child(
            Element::label("Windows 11 Mica compositor probe")
                .font_size(14.0)
                .fg(Color::WHITE)
                .width_match(),
        );

    App::new("Flux Launcher - Mica Probe", 720, 420)
        .bg(Color::rgba(0, 0, 0, 0))
        .centered()
        .frameless()
        .resizable(false)
        .min_size(520, 280)
        .renderer(Renderer::Auto)
        .backdrop(Backdrop::Mica)
        .content(content)
        .run();
}
