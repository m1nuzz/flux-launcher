#![cfg_attr(windows, windows_subsystem = "windows")]

#[macro_use]
extern crate rust_i18n;
i18n!("locales", fallback = "en");

mod accent;
mod action_panel;
mod actions;
mod activation_hotkey;
mod applications;
mod builtin;
mod entry;
mod everything;
mod fullscreen;
mod hotkeys;
mod i18n;
mod icons;
mod interval_state;
mod keyboard;
mod keyboard_layout;
mod launch;
mod launcher_dialogs;
mod launcher_run;
mod launcher_settings;
mod launcher_state;
mod main_interval;
mod main_key_input;
mod main_layout;
mod monitor;
mod native_host;
mod plugin_limits;
mod plugin_transport;
mod plugins;
mod provider_state;
mod query;
mod result_row;
mod settings_state;
mod settings_view;
mod startup;
mod tray_menu;
mod ui_helpers;
mod update_state;
mod updater;
mod visual_preview;
mod window_lifecycle;
mod window_state;

use std::time::Duration;

use flux_core::{MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH};

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const SINGLE_INSTANCE_ID: &str = "lvxingqi.flux-launcher-cn";
pub(crate) const SETTINGS_WINDOW_WIDTH: i32 = 720;
const EVERYTHING_PROMPT_WINDOW_WIDTH: i32 = 440;
const EVERYTHING_PROMPT_WINDOW_HEIGHT: i32 = 242;
// The empty launcher is a compact search strip; the results state keeps the user-configured height.
pub(crate) const COMPACT_WINDOW_HEIGHT: i32 = 56;
const VISUAL_SLIDER_WIDTH: i32 = 200;
// The action group stays narrower than the minimum launcher content width so it
// can be centered between the same left/right content insets at every size.
const ACTION_BAR_WIDTH: i32 = 340;
const ACTION_BAR_HEIGHT: i32 = 22;
// Keep the result palette compact like the reference while exposing a six-row
// viewport; additional results remain available through the native wheel scroll.
const ACTION_WINDOW_HEIGHT: i32 = 250;
// Six 46-DIP result rows plus local scroll padding keep the footer close to the results.
const RESULT_VIEWPORT_HEIGHT: i32 = 288;
// 结果行行高（DIP），与 result_row 中的行高保持一致，用于推导图标请求窗口。
pub(crate) const RESULT_ROW_HEIGHT: i32 = 46;
/// 图标请求「立即提取」的行数上限：可见行（RESULT_VIEWPORT_HEIGHT / RESULT_ROW_HEIGHT）
/// 加两行余量。其余行在滚到选区附近时由响应式回调补发请求，避免一次结果集变化
/// 把几十个 Everything 文件全部塞进唯一的图标提取线程。
pub(crate) const EAGER_ICON_ROW_COUNT: usize =
    (RESULT_VIEWPORT_HEIGHT / RESULT_ROW_HEIGHT + 2) as usize;
pub(crate) const SETTINGS_WINDOW_HEIGHT: i32 = 600;
const LAUNCHER_FONT_FAMILY: &str = "Segoe UI Variable";
const SEARCH_INTERVAL: Duration = Duration::from_millis(40);
const EVERYTHING_MIN_QUERY_LEN: usize = 1;
const PLUGIN_MIN_QUERY_LEN: usize = 2;
fn main() {
    entry::init_platform();

    let single_instance_disabled = std::env::var_os("FLUX_DISABLE_SINGLE_INSTANCE").is_some();

    let (mode, mut args) = entry::split_args();
    if entry::should_exit_early(mode.as_deref(), &mut args, single_instance_disabled) {
        return;
    }

    let setup = launcher_state::load_launcher_setup(mode.as_deref());
    launcher_run::run_launcher(setup, single_instance_disabled);
}

#[cfg(test)]
mod tests;
