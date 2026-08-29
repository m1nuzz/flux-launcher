//! Platform-independent core state and policies for Flux Launcher.

pub mod flow;
pub mod game_mode;
pub mod search;
pub mod settings;

pub use flow::{
    query_request_line, query_request_line_with_keyword, FlowAction, FlowPluginManifest,
    FlowResponse, FlowResult,
};
pub use game_mode::{
    is_flow_excluded_class, matches_display_bounds, should_suppress_activation, WindowBounds,
    WindowClass,
};
pub use search::{
    history_results, matches_search_text, rank_results, rank_results_with_priorities, ResultKind,
    ResultSource, SearchModel, SearchResult,
};
pub use settings::{
    HotkeyConfig, Language, MonitorPreference, PriorityEntry, Settings, DEFAULT_LAUNCHER_HEIGHT,
    DEFAULT_LAUNCHER_WIDTH, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT,
    MIN_LAUNCHER_WIDTH,
};
