//! 启动器窗口的元素树组装（搜索框、动作栏、结果列表、对话框与根表面）。
//!
//! 从 `main.rs` 整体外移（阶段十一 步骤 4）。这里只构造 Element 树并把
//! `*_for_rows` 句柄别名收敛为局部变量；窗口材质与显示/隐藏生命周期仍由
//! `main.rs` 的 app 链负责（§3、§4 不变）。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use flux_core::{PriorityEntry, SearchResult, Settings};
use rust_i18n::t;
use windui::app::WindowSizeHandle;
use windui::core::Widget;
use windui::prelude::*;
use windui::render::Canvas;

use crate::action_panel;
use crate::actions::ActionItem;
use crate::i18n::I18nHub;
use crate::launcher_dialogs;
use crate::plugins::PluginAction;
use crate::query::ProviderResults;
use crate::result_row::result_row;
use crate::ui_helpers::action_bar_content;
use crate::{ACTION_BAR_HEIGHT, ACTION_BAR_WIDTH, LAUNCHER_FONT_FAMILY, RESULT_VIEWPORT_HEIGHT};

/// 动作栏几何探针：仅在 `FLUX_SMOKE_ACTION_BAR` 设置时打印一次实际布局矩形，
/// 用于验证动作栏在启动器内边距之间确实居中。
#[derive(Default)]
struct ActionBarGeometryProbe {
    last: Cell<Option<(i32, i32, i32, i32)>>,
}

impl Widget for ActionBarGeometryProbe {
    fn paint(
        &self,
        bounds: Rect,
        _content: Rect,
        _focused: bool,
        _enabled: bool,
        _canvas: &mut dyn Canvas,
        _style: &Style,
    ) {
        if std::env::var_os("FLUX_SMOKE_ACTION_BAR").is_none() {
            return;
        }
        let geometry = (bounds.x, bounds.y, bounds.w, bounds.h);
        if self.last.get() != Some(geometry) {
            eprintln!(
                "ActionBarGeometry: x={} y={} width={} height={}",
                geometry.0, geometry.1, geometry.2, geometry.3
            );
            self.last.set(Some(geometry));
        }
    }
}

/// 组装启动器元素树所需的句柄与设置快照。
pub(crate) struct LauncherLayoutContext {
    pub(crate) i18n_hub: I18nHub,
    pub(crate) smooth_caret: bool,
    pub(crate) smooth_caret_duration_ms: u16,
    pub(crate) query: Signal<String>,
    pub(crate) inline_completion: Signal<String>,
    pub(crate) results: Signal<Vec<SearchResult>>,
    pub(crate) selected_id: Signal<String>,
    pub(crate) selected_index: Signal<usize>,
    pub(crate) selection_touched: Signal<bool>,
    pub(crate) action_mode: Signal<bool>,
    pub(crate) action_index: Signal<usize>,
    pub(crate) action_items: Signal<Vec<ActionItem>>,
    pub(crate) action_scroll_pending: Signal<bool>,
    pub(crate) launcher_width: Signal<u16>,
    pub(crate) launcher_height: Signal<u16>,
    pub(crate) selection_color: Signal<Color>,
    pub(crate) show_results: Signal<bool>,
    pub(crate) settings_visible: Signal<bool>,
    pub(crate) icon_refresh_generation: Signal<u64>,
    pub(crate) history_mode: Signal<bool>,
    pub(crate) recycle_bin_confirmation: Signal<bool>,
    pub(crate) everything_prompt_visible: Signal<bool>,
    pub(crate) everything_status: Signal<String>,
    pub(crate) status: Signal<String>,
    pub(crate) priorities: Signal<Vec<PriorityEntry>>,
    pub(crate) plugin_actions: Rc<RefCell<HashMap<String, PluginAction>>>,
    pub(crate) provider_results: Rc<RefCell<ProviderResults>>,
    pub(crate) query_history: Rc<RefCell<Vec<String>>>,
    pub(crate) shared_settings: Arc<RwLock<Settings>>,
    pub(crate) action_window_slot: Rc<RefCell<Option<WindowSizeHandle>>>,
}

/// 组装结果：`main.rs` 后续接线需要的三个值。
pub(crate) struct LauncherLayout {
    pub(crate) scroll_request: Signal<bool>,
    pub(crate) query_caret_position: Signal<usize>,
    pub(crate) launcher_surface: Element,
}

/// 构建启动器根表面，与 `main.rs` 原内联块逐行等价。
pub(crate) fn build_launcher_layout(context: LauncherLayoutContext) -> LauncherLayout {
    let LauncherLayoutContext {
        i18n_hub,
        smooth_caret,
        smooth_caret_duration_ms,
        query,
        inline_completion,
        results,
        selected_id,
        selected_index,
        selection_touched,
        action_mode,
        action_index,
        action_items,
        action_scroll_pending,
        launcher_width,
        launcher_height,
        selection_color,
        show_results,
        settings_visible,
        icon_refresh_generation,
        history_mode,
        recycle_bin_confirmation,
        everything_prompt_visible,
        everything_status,
        status,
        priorities,
        ref plugin_actions,
        ref provider_results,
        ref query_history,
        ref shared_settings,
        ref action_window_slot,
    } = context;

    let result_source = results;
    let selected_for_rows = selected_id;
    let selected_index_for_rows = selected_index;
    let selection_touched_for_rows = selection_touched;
    let actions_for_rows = Rc::clone(plugin_actions);
    let settings_for_rows = Arc::clone(shared_settings);
    let history_for_rows = Rc::clone(query_history);
    let history_mode_for_rows = history_mode;
    let action_items_for_rows = action_items;
    let action_index_for_rows = action_index;
    let action_mode_for_rows = action_mode;
    let launcher_width_for_rows = launcher_width;
    let query_for_rows = query;
    let scroll_request_for_rows = signal(false);
    let settings_visible_for_rows = settings_visible;
    let window_size_slot_for_rows = Rc::clone(action_window_slot);
    let query_caret_position = signal(query.with(|text| text.chars().count()));

    let search_placeholder = i18n_hub.tr(|| t!("search.placeholder").into_owned());
    let search_box = Element::text_input(query, search_placeholder)
        .cursor_position(query_caret_position)
        .leading_icon('⌕')
        .transparent_surface()
        .smooth_caret(smooth_caret, smooth_caret_duration_ms)
        .inline_completion(inline_completion)
        .show_focus_ring(false)
        .width_match()
        .font_family(LAUNCHER_FONT_FAMILY)
        .font_size(15.0)
        .font_weight(500)
        .corner(10.0)
        // The entire Search control stays transparent so the Windows Acrylic
        // material remains visible through the input, caret, and leading icon.
        .border(Color::rgba(0, 0, 0, 0), 0)
        .padding_xy(13, 0);

    let action_bar_content = action_bar_content(i18n_hub.clone());
    let action_bar = Element::stack()
        .width(ACTION_BAR_WIDTH)
        .height(ACTION_BAR_HEIGHT)
        .child(action_bar_content.align(Align::Center))
        // Keep the probe inside the same real frame so its telemetry describes
        // the exact slot that is centered between the launcher insets.
        .child(
            Element::leaf()
                .widget(ActionBarGeometryProbe::default())
                .fill(),
        )
        .align(Align::Center)
        .visible_when(move || show_results.get() && !action_mode.get());

    let result_list_body = Element::host_signal(result_source, move |result| {
        result_row(
            result,
            selected_for_rows,
            selected_index_for_rows,
            selection_touched_for_rows,
            result_source,
            icon_refresh_generation,
            Rc::clone(&actions_for_rows),
            action_items_for_rows,
            action_index_for_rows,
            action_scroll_pending,
            action_mode_for_rows,
            launcher_width_for_rows,
            query_for_rows,
            scroll_request_for_rows,
            selection_color,
            Arc::clone(&settings_for_rows),
            Rc::clone(&history_for_rows),
            history_mode_for_rows,
            recycle_bin_confirmation,
            settings_visible_for_rows,
            Rc::clone(&window_size_slot_for_rows),
        )
    })
    .width_match()
    // Keep the result body transparent so the window remains one continuous
    // Acrylic surface. Only individual result rows draw controls. The extra
    // right inset is local to the scroll content: it keeps the thumb clear of
    // row cards without changing the launcher window width.
    .padding_edges(6, 6, 18, 6);
    let result_list = Element::scroll()
        .width_match()
        .height(RESULT_VIEWPORT_HEIGHT)
        .child(result_list_body)
        .visible_when(move || show_results.get() && !action_mode.get());

    let everything_install_prompt = launcher_dialogs::everything_install_prompt(
        launcher_dialogs::EverythingInstallPromptContext {
            visible: everything_prompt_visible,
            status: everything_status,
            settings: Arc::clone(shared_settings),
            i18n_hub: i18n_hub.clone(),
        },
    );
    let recycle_bin_dialog =
        launcher_dialogs::recycle_bin_dialog(launcher_dialogs::RecycleBinDialogContext {
            visible: recycle_bin_confirmation,
            status,
            i18n_hub: i18n_hub.clone(),
        });

    let action_list = action_panel::action_list(action_panel::ActionListContext {
        action_items: action_items_for_rows,
        action_index: action_index_for_rows,
        action_scroll_pending,
        result_source,
        selected_id: selected_for_rows,
        selected_index: selected_index_for_rows,
        action_mode: action_mode_for_rows,
        launcher_width: launcher_width_for_rows,
        launcher_height,
        action_window_slot: Rc::clone(action_window_slot),
        settings: Arc::clone(shared_settings),
        priorities,
        provider_results: Rc::clone(provider_results),
        query,
    });

    // The HWND itself owns the system Acrylic surface. Keep this root transparent so
    // the blur fills the complete client area instead of becoming an inset card. The
    // content must match the live window width so result rows expand with resizing.
    // Keep the empty search strip and the results palette intrinsically sized. A
    // full-height column plus a weighted spacer made the compact state look too
    // tall and left an oversized gap between the last result and the footer.
    // Keep the compact Search baseline genuinely centered: equal vertical
    // insets avoid moving the empty-state control toward either edge.
    let launcher_content = Element::col()
        .width_match()
        .padding_edges(10, 7, 10, 7)
        .spacing(4)
        .child(search_box)
        .child(result_list)
        // The result viewport now ends immediately before the fixed footer. Do not
        // add a weighted spacer: it creates a visible blank band for short queries.
        .child(action_bar)
        .child(action_list)
        .child(recycle_bin_dialog)
        .child(everything_install_prompt);
    let launcher_surface = Element::stack()
        .fill()
        .bg(Color::rgba(0, 0, 0, 0))
        .child(launcher_content.align(Align::Center));

    LauncherLayout {
        scroll_request: scroll_request_for_rows,
        query_caret_position,
        launcher_surface,
    }
}
