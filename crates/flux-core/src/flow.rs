use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct FlowPluginManifest {
    #[serde(rename = "ID", alias = "id")]
    pub id: String,
    #[serde(rename = "Name", alias = "name")]
    pub name: String,
    #[serde(rename = "Description", alias = "description", default)]
    pub description: String,
    #[serde(rename = "Language", alias = "language")]
    pub language: String,
    #[serde(rename = "ExecuteFileName", alias = "executeFileName")]
    pub execute_file_name: String,
    #[serde(rename = "ActionKeywords", alias = "actionKeywords", default)]
    pub action_keywords: Vec<String>,
}

impl FlowPluginManifest {
    pub fn is_native_executable(&self) -> bool {
        self.language.eq_ignore_ascii_case("Executable")
            || self.language.eq_ignore_ascii_case("Executable_V2")
    }

    pub fn accepts_query(&self, query: &str) -> bool {
        self.matching_action_keyword(query).is_some()
    }

    pub fn matching_action_keyword(&self, query: &str) -> Option<&str> {
        if self.action_keywords.is_empty() {
            return Some("*");
        }
        self.action_keywords
            .iter()
            .filter(|keyword| *keyword == "*" || has_action_keyword_prefix(query, keyword))
            .max_by_key(|keyword| keyword.len())
            .map(String::as_str)
    }

    pub fn search_for_action_keyword<'a>(&self, query: &'a str, keyword: &str) -> &'a str {
        if keyword == "*" {
            return query.trim();
        }
        let Some(rest) = query.strip_prefix(keyword) else {
            return query.trim();
        };
        rest.trim_start_matches(|character: char| character == ':' || character.is_whitespace())
    }
}

fn has_action_keyword_prefix(query: &str, keyword: &str) -> bool {
    query == keyword
        || query.strip_prefix(keyword).is_some_and(|rest| {
            rest.starts_with(':') || rest.chars().next().is_some_and(char::is_whitespace)
        })
}

#[derive(Clone, Debug, Serialize)]
pub struct FlowQueryParams<'a> {
    #[serde(rename = "rawQuery")]
    pub raw_query: &'a str,
    #[serde(rename = "search")]
    pub search: &'a str,
    #[serde(rename = "actionKeyword")]
    pub action_keyword: &'a str,
}

#[derive(Clone, Debug, Serialize)]
pub struct FlowRequest<'a, T> {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: &'a str,
    pub params: T,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct FlowResponse {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub result: Vec<FlowResult>,
    #[serde(default)]
    pub error: Option<FlowError>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct FlowError {
    pub code: i64,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct FlowResult {
    #[serde(rename = "Title", alias = "title")]
    pub title: String,
    #[serde(rename = "SubTitle", alias = "subtitle", alias = "subTitle", default)]
    pub subtitle: String,
    #[serde(rename = "Score", alias = "score", default)]
    pub score: i32,
    #[serde(rename = "JsonRPCAction", alias = "jsonRPCAction", default)]
    pub action: Option<FlowAction>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct FlowAction {
    #[serde(rename = "Method", alias = "method")]
    pub method: String,
    #[serde(rename = "Parameters", alias = "parameters", default)]
    pub parameters: Vec<serde_json::Value>,
    #[serde(rename = "DontHideAfterAction", alias = "dontHideAfterAction", default)]
    pub dont_hide_after_action: bool,
}

pub fn query_request_line(id: u64, query: &str) -> serde_json::Result<String> {
    query_request_line_with_keyword(id, query, query, "*")
}

pub fn query_request_line_with_keyword(
    id: u64,
    raw_query: &str,
    search: &str,
    action_keyword: &str,
) -> serde_json::Result<String> {
    let request = FlowRequest {
        jsonrpc: "2.0",
        id,
        method: "query",
        params: [
            serde_json::to_value(FlowQueryParams {
                raw_query,
                search,
                action_keyword,
            })?,
            serde_json::json!({}),
        ],
    };
    serde_json::to_string(&request)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXECUTABLE_MANIFEST: &str = r#"{
        "ID": "flux.fixture.native",
        "Name": "Native Fixture",
        "Description": "A test executable plugin",
        "Language": "Executable_V2",
        "ExecuteFileName": "fixture-plugin.exe",
        "ActionKeywords": ["fx:", "*"]
    }"#;

    #[test]
    fn accepts_only_native_flow_languages() {
        let manifest: FlowPluginManifest = serde_json::from_str(EXECUTABLE_MANIFEST).unwrap();
        assert!(manifest.is_native_executable());
        assert!(manifest.accepts_query("fx: query"));
        assert!(manifest.accepts_query("fxylophone"));
        assert!(manifest.accepts_query("ordinary query"));
        let specific: FlowPluginManifest =
            serde_json::from_str(&EXECUTABLE_MANIFEST.replace(", \"*\"]", "]")).unwrap();
        assert!(!specific.accepts_query("fxylophone"));
        assert_eq!(manifest.matching_action_keyword("fx: query"), Some("fx:"));
        assert_eq!(
            manifest.search_for_action_keyword("fx: query", "fx:"),
            "query"
        );

        let csharp: FlowPluginManifest =
            serde_json::from_str(&EXECUTABLE_MANIFEST.replace("Executable_V2", "CSharp")).unwrap();
        assert!(!csharp.is_native_executable());
    }

    #[test]
    fn serializes_newline_transport_query_payload() {
        let line = query_request_line(17, "notepad").unwrap();
        assert!(line.contains("\"jsonrpc\":\"2.0\""));
        assert!(line.contains("\"method\":\"query\""));
        assert!(line.contains("\"rawQuery\":\"notepad\""));
        let line = query_request_line_with_keyword(18, "ob Notes", "Notes", "ob").unwrap();
        assert!(line.contains("\"search\":\"Notes\""));
        assert!(line.contains("\"actionKeyword\":\"ob\""));
    }

    #[test]
    fn parses_flow_result_with_optional_action() {
        let response: FlowResponse = serde_json::from_str(
            r#"{
                "id": 2,
                "result": [{
                    "Title": "Fixture result",
                    "SubTitle": "From native executable",
                    "Score": 42,
                    "JsonRPCAction": {"Method": "execute", "Parameters": ["x"]}
                }]
            }"#,
        )
        .unwrap();
        assert_eq!(response.id, 2);
        assert_eq!(response.result[0].title, "Fixture result");
        assert_eq!(
            response.result[0].action.as_ref().unwrap().method,
            "execute"
        );
    }
}
