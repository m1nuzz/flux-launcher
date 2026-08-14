//! 全局热键（macOS）——**尚未实现**。
//!
//! Windows 侧已通过 `RegisterHotKey` 实现（见 `platform/win32/hotkey.rs`）。macOS 侧
//! 需要 Carbon 的 `RegisterEventHotKey` + `InstallEventHandler`（或 `CGEventTap`，但后者
//! 要求用户在「系统设置 → 隐私与安全性 → 辅助功能」手动授权，涉及引导 UI、权限被
//! 撤销后的降级路径与公证签名等一整套问题），工作量显著大于 Windows 侧。
//!
//! ## 为何是空实现而非"看着像对的代码"
//!
//! 本能力在 Windows 开发环境下编写，macOS 代码**无法编译、无法运行、无法验证**。
//! `AGENTS.md` §5 明令：只能真机验证的特性，代码自洽 + 单测覆盖可测部分后，须明确
//! 请用户实测，**别声称"已验证"**。
//!
//! 交付一份从未编译过的 Carbon 实现，比明确的"未实现"更危险——前者会让调用方以为
//! 该平台可用，直到运行时才发现热键根本不响应，且无从判断是自己用错还是库有 bug。
//!
//! ## 现状与影响
//!
//! macOS 上 `App::hotkey` 会在 **debug 期 panic**，release 期静默忽略（与"热键注册失败
//! 不阻止应用启动"的既定语义一致，见 `App::hotkey` 文档）。
//!
//! macOS 的**其余后台工具能力不受影响**：托盘、`EventCtx::show_window`/`hide_window`、
//! `App::start_hidden` 均已实现。缺口仅限全局热键这一项。

use crate::platform::HotkeyBinding;

/// 已注册的热键集合（macOS：占位）。
pub(crate) struct HotkeyState;

impl HotkeyState {
    /// 注册全部热键。macOS 未实现——全部静默失效。
    ///
    /// 返回形状与 win32 侧对齐（macOS 无需 hwnd，故参数少一个）：调用点本就分平台。
    pub(crate) fn register(bindings: Vec<HotkeyBinding>) -> Self {
        debug_assert!(
            bindings.is_empty(),
            "windui：macOS 尚未实现全局热键（App::hotkey）。\n\
             该热键不会生效。Windows 侧已实现；macOS 需 Carbon RegisterEventHotKey，\n\
             详见 src/platform/macos/hotkey.rs。"
        );
        Self
    }

    /// 运行期热键操作（改绑/启停）。macOS 未实现——注册本就未生效，静默忽略。
    ///
    /// 桩暂无调用点：win32 在意图消费点调 `apply`，macOS 侧的窗口层还没接热键意图管线。
    /// 待 macOS 实现全局热键时连同调用点一并补上，届时删掉此 `allow`。
    #[allow(dead_code)]
    pub(crate) fn apply(&mut self, _id: usize, _op: crate::event::HotkeyOp) {}
}
