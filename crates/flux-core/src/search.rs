use crate::search_match::{is_application_path, match_app_title, match_file_title, normalize};

pub(crate) const MAX_RESULTS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultKind {
    Command,
    Application,
    File,
    Placeholder,
}

/// Identifies the provider that produced a result. Flow keeps program search
/// separate from file search; the source tier lets Flux preserve that boundary
/// even when both providers return executable-looking paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultSource {
    BuiltIn,
    ApplicationCatalog,
    Everything,
    Plugin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: ResultKind,
    pub source: ResultSource,
    pub target: Option<String>,
}

impl SearchResult {
    pub fn file(path: String, title: String, subtitle: String) -> Self {
        let is_application = is_application_path(&path);
        let display_title = if is_application {
            title
                .rsplit_once('.')
                .map(|(stem, _)| stem.to_owned())
                .unwrap_or(title)
        } else {
            title
        };
        Self {
            id: format!("file:{path}"),
            title: display_title,
            subtitle,
            kind: if is_application {
                ResultKind::Application
            } else {
                ResultKind::File
            },
            source: ResultSource::Everything,
            target: Some(path),
        }
    }

    pub fn display_text(&self) -> String {
        format!("{}  -  {}", self.title, self.subtitle)
    }

    /// Lower sort key means a result is more useful for the current query.
    /// Applications deliberately outrank indexed files and folders.
    pub fn relevance(&self, query: &str) -> (u8, u8, String) {
        let (priority, _, provider_tier, title_tier, title) = self.priority_relevance(query, &[]);
        debug_assert_eq!(priority, 1);
        (provider_tier, title_tier, title)
    }

    pub(crate) fn priority_relevance(
        &self,
        query: &str,
        priorities: &[String],
    ) -> (u8, usize, u8, u8, String) {
        let query = normalize(query);
        let title = normalize(&self.title);
        let subtitle = normalize(&self.subtitle);
        let (provider_tier, title_tier) = match self.source {
            ResultSource::ApplicationCatalog => (0, match_app_title(&title, &query)),
            ResultSource::BuiltIn => (1, match_app_title(&title, &query)),
            ResultSource::Plugin => (2, match_app_title(&title, &query)),
            ResultSource::Everything => match self.kind {
                ResultKind::Application => (3, match_app_title(&title, &query)),
                ResultKind::Command | ResultKind::Placeholder | ResultKind::File => {
                    (4, match_file_title(&title, &query))
                }
            },
        };
        let subtitle_match = if !query.is_empty() && subtitle.contains(&query) {
            0
        } else {
            1
        };
        let priority = if matches!(self.kind, ResultKind::Application) {
            priorities
                .iter()
                .position(|id| id == &self.id)
                .map(|index| index.saturating_add(1))
        } else {
            None
        };
        let exact_calculator = self.id == "builtin:calculator";
        let exact_shell_command = (self.id == "system:command-prompt"
            && matches!(query.as_str(), "cmd" | "command prompt" | "command line"))
            || (self.id == "system:powershell"
                && matches!(query.as_str(), "powershell" | "pwsh" | "power shell"));
        (
            if exact_calculator || exact_shell_command {
                0
            } else {
                priority.map_or(1, |_| 0)
            },
            if exact_calculator || exact_shell_command {
                0
            } else {
                priority.unwrap_or_default()
            },
            provider_tier,
            title_tier.saturating_add(subtitle_match),
            title,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_and_executable_paths_are_applications() {
        assert_eq!(
            SearchResult::file(
                String::from("C:/Users/Test/Google Chrome.lnk"),
                String::from("Google Chrome.lnk"),
                String::new(),
            )
            .kind,
            ResultKind::Application
        );
    }
}
