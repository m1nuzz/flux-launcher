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
/// How long the query must be untouched before asynchronously loaded shell
/// icons are allowed to repaint the result rows. While typing, the rows are
/// already correct without them; propagating every arrival flashed the list 2-4
/// times per keystroke.
pub(crate) const ICON_REFRESH_QUIET_MS: u64 = 120;
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
