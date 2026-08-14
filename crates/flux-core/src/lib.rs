//! Platform-independent core state and policies for Flux Launcher.

pub mod game_mode;
pub mod search;
pub mod settings;

pub use game_mode::{
    is_flow_excluded_class, matches_display_bounds, should_suppress_activation, WindowBounds,
    WindowClass,
};
pub use search::{ResultKind, SearchModel, SearchResult};
pub use settings::Settings;
