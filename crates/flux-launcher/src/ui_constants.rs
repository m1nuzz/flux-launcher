use std::time::Duration;

pub(crate) const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const SINGLE_INSTANCE_ID: &str = "m1nuzz.flux-launcher";
pub(crate) const SETTINGS_WINDOW_WIDTH: i32 = 720;
pub(crate) const EVERYTHING_PROMPT_WINDOW_WIDTH: i32 = 440;
pub(crate) const EVERYTHING_PROMPT_WINDOW_HEIGHT: i32 = 242;
// The empty launcher is a compact search strip; the results state keeps the user-configured height.
pub(crate) const COMPACT_WINDOW_HEIGHT: i32 = 56;
pub(crate) const VISUAL_SLIDER_WIDTH: i32 = 200;
// The action group stays narrower than the minimum launcher content width so it
// can be centered between the same left/right content insets at every size.
pub(crate) const ACTION_BAR_WIDTH: i32 = 340;
pub(crate) const ACTION_BAR_HEIGHT: i32 = 22;
// Keep the result palette compact like the reference while exposing a six-row
// viewport; additional results remain available through the native wheel scroll.
pub(crate) const ACTION_WINDOW_HEIGHT: i32 = 250;
// Six 46-DIP result rows plus local scroll padding keep the footer close to the results.
pub(crate) const RESULT_VIEWPORT_HEIGHT: i32 = 288;
/// How many result rows fit before the list scrolls locally, and the pitch each
/// one occupies inside the fixed viewport.
pub(crate) const RESULT_ROWS_VISIBLE: i32 = 6;
pub(crate) const RESULT_ROW_PITCH: i32 = RESULT_VIEWPORT_HEIGHT / RESULT_ROWS_VISIBLE;
/// The search strip plus the footer and their padding: everything the window shows
/// apart from the result rows themselves.
pub(crate) const LAUNCHER_CHROME_HEIGHT: i32 = COMPACT_WINDOW_HEIGHT + 38;
/// How long the query must go untouched before late-arriving results and shell
/// icons are allowed to touch the element tree. While the user types, the list is
/// painted once per keystroke: a second publish for the same query rebuilds every
/// row and reads as a full-list flash. The window has to exceed a normal inter-key
/// interval, otherwise the deferred snapshot still lands between two keystrokes.
pub(crate) const TYPING_QUIET_MS: u64 = 250;
/// How many search ticks a page of shell icons may be held back waiting for the
/// icon thread to drain before the partial set is propagated anyway. At
/// `SEARCH_INTERVAL` this caps the wait at roughly half a second, so a shell item
/// that never answers cannot hide the icons.
pub(crate) const ICON_SETTLE_TICKS: u32 = 30;
pub(crate) const SETTINGS_WINDOW_HEIGHT: i32 = 520;
pub(crate) const LAUNCHER_FONT_FAMILY: &str = "Segoe UI Variable";
pub(crate) const SEARCH_INTERVAL: Duration = Duration::from_millis(16);
// Slow providers (Everything/plugins/native) fire for a settled query
// generation (Flow's SearchDelayTime default is 150ms). Zero disables the
// wait: every generation fans out on the next tick; the commit guards still
// suppress duplicate publishes. Applications always stay immediate.
pub(crate) const SLOW_PROVIDER_DEBOUNCE_MS: u64 = 0;
pub(crate) const EVERYTHING_MIN_QUERY_LEN: usize = 1;
pub(crate) const PLUGIN_MIN_QUERY_LEN: usize = 2;
pub(crate) const MAX_VISIBLE_RESULTS: usize = 16;
