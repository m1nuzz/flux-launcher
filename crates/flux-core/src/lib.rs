//! Platform-independent core state and policies for Flux Launcher.

pub mod flow;
pub mod game_mode;
pub mod search;
mod search_match;
mod search_model;
pub mod settings;
mod settings_store;
mod system_results;

pub use flow::{
    query_request_line, query_request_line_with_keyword, FlowAction, FlowPluginManifest,
    FlowResponse, FlowResult,
};
pub use game_mode::{
    is_flow_excluded_class, matches_display_bounds, should_suppress_activation, WindowBounds,
    WindowClass,
};
pub use search::{ResultKind, ResultSource, SearchResult};
pub use search_match::{
    is_application_path, matches_search_text, rank_results, rank_results_with_priorities,
};
pub use search_model::{history_results, SearchModel};
pub use settings::{
    HotkeyConfig, LaunchCount, MonitorPreference, PriorityEntry, Settings, DEFAULT_LAUNCHER_HEIGHT,
    DEFAULT_LAUNCHER_WIDTH, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH, MAX_QUERY_HISTORY,
    MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
pub use settings_store::SettingsLoadOutcome;
