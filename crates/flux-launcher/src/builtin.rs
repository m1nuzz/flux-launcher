use flux_core::{ResultKind, ResultSource, SearchResult};

use super::builtin_calc::CalculatorProvider;
use super::builtin_obsidian::ObsidianProvider;

pub(crate) const MAX_RESULTS: usize = 16;

#[derive(Clone, Debug)]
pub struct BuiltinQuery {
    pub query: String,
    pub google_enabled: bool,
    pub google_keyword: String,
    pub obsidian_enabled: bool,
    pub obsidian_keyword: String,
}

#[derive(Clone, Debug)]
pub enum BuiltinAction {
    OpenUrl(String),
    CopyText(String),
}

#[derive(Clone, Debug)]
pub struct BuiltinResult {
    pub result: SearchResult,
    pub action: Option<BuiltinAction>,
}

pub trait BuiltinProvider: Send + Sync {
    fn query(&self, request: &BuiltinQuery) -> Vec<BuiltinResult>;
}

pub fn query_builtin_providers(request: &BuiltinQuery) -> Vec<BuiltinResult> {
    let providers: [&dyn BuiltinProvider; 3] =
        [&CalculatorProvider, &GoogleProvider, &ObsidianProvider];
    providers
        .into_iter()
        .flat_map(|provider| provider.query(request))
        .take(MAX_RESULTS)
        .collect()
}

struct GoogleProvider;

impl BuiltinProvider for GoogleProvider {
    fn query(&self, request: &BuiltinQuery) -> Vec<BuiltinResult> {
        if !request.google_enabled {
            return Vec::new();
        }
        let keyword = normalized_keyword(&request.google_keyword, "g");
        let Some(search) = search_for_keyword(&request.query, keyword) else {
            return Vec::new();
        };
        if search.is_empty() {
            return Vec::new();
        }
        let url = format!(
            "https://www.google.com/search?q={}",
            encode_uri_component(search)
        );
        vec![BuiltinResult {
            result: SearchResult {
                id: String::from("builtin:google-search"),
                title: format!("Search Google: {search}"),
                subtitle: String::from("Open the Google search in the default browser"),
                kind: ResultKind::Placeholder,
                source: ResultSource::Plugin,
                target: None,
            },
            action: Some(BuiltinAction::OpenUrl(url)),
        }]
    }
}

pub(crate) fn normalized_keyword<'a>(keyword: &'a str, fallback: &'a str) -> &'a str {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        fallback
    } else {
        keyword
    }
}

pub(crate) fn search_for_keyword<'a>(query: &'a str, keyword: &str) -> Option<&'a str> {
    if query != keyword
        && !query.strip_prefix(keyword).is_some_and(|rest| {
            rest.starts_with(':') || rest.chars().next().is_some_and(char::is_whitespace)
        })
    {
        return None;
    }
    Some(
        query
            .strip_prefix(keyword)
            .unwrap_or_default()
            .trim_start_matches(|character: char| character == ':' || character.is_whitespace())
            .trim(),
    )
}

pub(crate) fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_provider_matches_alias_and_encodes_url() {
        let request = BuiltinQuery {
            query: String::from("g space exploration"),
            google_enabled: true,
            google_keyword: String::from("g"),
            obsidian_enabled: true,
            obsidian_keyword: String::from("ob"),
        };
        let results = GoogleProvider.query(&request);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result.title, "Search Google: space exploration");
        assert!(matches!(
            &results[0].action,
            Some(BuiltinAction::OpenUrl(url)) if url == "https://www.google.com/search?q=space%20exploration"
        ));
    }

    #[test]
    fn aliases_require_a_token_boundary() {
        assert!(search_for_keyword("ob meeting", "ob").is_some());
        assert!(search_for_keyword("ob:meeting", "ob").is_some());
        assert!(search_for_keyword("ob", "ob").is_some_and(str::is_empty));
        assert!(search_for_keyword("object", "ob").is_none());
    }
}
