//! Platform-independent core state and policies for Flux Launcher.

pub mod flow;
pub mod game_mode;
pub mod search;
pub mod settings;

pub use flow::{query_request_line, FlowAction, FlowPluginManifest, FlowResponse, FlowResult};
pub use game_mode::{
    is_flow_excluded_class, matches_display_bounds, should_suppress_activation, WindowBounds,
    WindowClass,
};
pub use search::{ResultKind, SearchModel, SearchResult};
pub use settings::{HotkeyConfig, Settings};
