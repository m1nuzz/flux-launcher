//! 系统托盘图标与右键菜单。
//!
//! 从 `main.rs` 整体外移（阶段十一 步骤 5 的托盘部分）。菜单项闭包只读写信号与
//! 请求窗口尺寸/位置，窗口显示仍走既有 `ctx.show_window()`（§3 生命周期不变）。

use std::sync::{Arc, RwLock};

use flux_core::Settings;
use windui::app::{WindowPositionHandle, WindowSizeHandle};
use windui::prelude::*;

use crate::i18n::I18nHub;
use crate::icons::tray_icon;
use crate::settings_state::set_game_mode;
use crate::window_state::request_monitor_position;
use crate::{COMPACT_WINDOW_HEIGHT, SETTINGS_WINDOW_HEIGHT, SETTINGS_WINDOW_WIDTH};

/// 构建托盘所需的句柄集合。
pub(crate) struct TrayContext {
    pub(crate) i18n_hub: I18nHub,
    pub(crate) settings: Arc<RwLock<Settings>>,
    pub(crate) game_mode: Signal<bool>,
    pub(crate) game_mode_status: Signal<String>,
    pub(crate) settings_visible: Signal<bool>,
    pub(crate) show_results: Signal<bool>,
    pub(crate) window_size: WindowSizeHandle,
    pub(crate) window_position: WindowPositionHandle,
    pub(crate) launcher_width: Signal<u16>,
    pub(crate) launcher_height: Signal<u16>,
}

/// 构建托盘图标与菜单，注册左键点击、显示、设置、游戏模式与退出项。
pub(crate) fn build_tray(context: TrayContext) -> Tray {
    let TrayContext {
        i18n_hub,
        settings: shared_settings,
        game_mode,
        game_mode_status,
        settings_visible,
        show_results,
        window_size,
        window_position,
        launcher_width,
        launcher_height,
    } = context;

    let settings_for_tray_toggle = Arc::clone(&shared_settings);
    let game_mode_for_tray = game_mode;
    let game_status_for_tray = game_mode_status;
    let settings_visible_for_tray = settings_visible;
    let settings_visible_for_left_click = settings_visible;
    let show_results_for_left_click = show_results;
    let size_for_left_click = window_size.clone();
    let position_for_left_click = window_position.clone();
    let settings_for_left_click = Arc::clone(&shared_settings);
    let show_results_for_tray = show_results;
    let size_for_tray = window_size.clone();
    let position_for_tray = window_position.clone();
    let settings_for_tray_position = Arc::clone(&shared_settings);
    let size_for_settings = window_size.clone();
    let position_for_settings = window_position.clone();
    let settings_for_settings_position = Arc::clone(&shared_settings);
    Tray::new()
        .tooltip("Flux Launcher CN")
        .icon_rgba(16, 16, &tray_icon())
        .on_left_click(move |ctx| {
            settings_visible_for_left_click.set(false);
            let height = if show_results_for_left_click.get() {
                launcher_height.get() as i32
            } else {
                COMPACT_WINDOW_HEIGHT
            };
            if let Ok(settings) = settings_for_left_click.read() {
                request_monitor_position(
                    &position_for_left_click,
                    settings.monitor_preference,
                    launcher_width.get() as i32,
                    height,
                );
            }
            size_for_left_click.set(launcher_width.get() as i32, height);
            ctx.show_window();
        })
        .menu(vec![
            TrayMenuItem::item(i18n_hub.tr(|| t!("tray.show").into_owned()), move |ctx| {
                settings_visible_for_tray.set(false);
                let height = if show_results_for_tray.get() {
                    launcher_height.get() as i32
                } else {
                    COMPACT_WINDOW_HEIGHT
                };
                if let Ok(settings) = settings_for_tray_position.read() {
                    request_monitor_position(
                        &position_for_tray,
                        settings.monitor_preference,
                        launcher_width.get() as i32,
                        height,
                    );
                }
                size_for_tray.set(launcher_width.get() as i32, height);
                ctx.show_window();
            }),
            TrayMenuItem::item(
                i18n_hub.tr(|| t!("tray.settings").into_owned()),
                move |ctx| {
                    settings_visible.set(true);
                    if let Ok(settings) = settings_for_settings_position.read() {
                        request_monitor_position(
                            &position_for_settings,
                            settings.monitor_preference,
                            SETTINGS_WINDOW_WIDTH,
                            SETTINGS_WINDOW_HEIGHT,
                        );
                    }
                    // Queue the Settings size before showing the hidden tray window. The
                    // first frame must not use the compact 72-DIP launcher height.
                    size_for_settings.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                    ctx.show_window();
                    // Keep the request after show as well because the native show lifecycle
                    // may consume a stale compact-size request from the previous hide.
                    size_for_settings.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                },
            ),
            TrayMenuItem::separator(),
            TrayMenuItem::check(
                i18n_hub.tr(|| t!("tray.game_mode").into_owned()),
                game_mode,
                move |_| {
                    let enabled = !game_mode_for_tray.get();
                    set_game_mode(
                        &settings_for_tray_toggle,
                        game_mode_for_tray,
                        game_status_for_tray,
                        enabled,
                    );
                },
            ),
            TrayMenuItem::separator(),
            TrayMenuItem::item(i18n_hub.tr(|| t!("tray.exit").into_owned()), |ctx| {
                ctx.quit()
            }),
        ])
}
