use std::path::Path;

use flux_plugin_sdk::{PluginAction, PluginExecute, PluginPermissions, PluginQuery};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::native_host::PluginHost;

#[derive(Debug, Deserialize)]
struct HostRequest {
    id: Value,
    method: String,
    #[serde(default)]
    plugin: Option<String>,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct HostResponse<T: Serialize> {
    jsonrpc: &'static str,
    id: Value,
    result: T,
}

#[derive(Debug, Serialize)]
struct HostError {
    code: i32,
    message: String,
}

#[derive(Debug, Serialize)]
struct HostErrorResponse {
    jsonrpc: &'static str,
    id: Value,
    error: HostError,
}

#[derive(Debug, Serialize)]
pub(crate) struct HostQueryResponse {
    pub(crate) results: Vec<flux_plugin_sdk::PluginResult>,
    pub(crate) errors: Vec<String>,
}

pub(crate) fn handle_request(host: &mut PluginHost, line: &str) -> Value {
    let request: HostRequest = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return serde_json::to_value(HostErrorResponse {
                jsonrpc: "2.0",
                id: Value::from(1),
                error: HostError {
                    code: -32700,
                    message: error.to_string(),
                },
            })
            .unwrap_or(Value::Null);
        }
    };
    let response: Result<Value, String> = match request.method.as_str() {
        "query" => {
            let query: Result<PluginQuery, String> =
                serde_json::from_value(request.params).map_err(|error| error.to_string());
            query.and_then(|query| {
                serde_json::to_value(HostResponse {
                    jsonrpc: "2.0",
                    id: request.id.clone(),
                    result: host.query(query),
                })
                .map_err(|error| error.to_string())
            })
        }
        "execute" => {
            let Some(plugin) = request.plugin.as_deref() else {
                return error_value(request.id, "execute requires plugin");
            };
            let execute: Result<PluginExecute, String> =
                serde_json::from_value(request.params).map_err(|error| error.to_string());
            execute
                .and_then(|execute| host.execute(plugin, execute.action))
                .and_then(|result| {
                    serde_json::to_value(HostResponse {
                        jsonrpc: "2.0",
                        id: request.id.clone(),
                        result,
                    })
                    .map_err(|error| error.to_string())
                })
        }
        _ => Err(format!("unknown method: {}", request.method)),
    };
    match response {
        Ok(response) => response,
        Err(error) => error_value(request.id, &error),
    }
}

fn error_value(id: Value, message: &str) -> Value {
    serde_json::to_value(HostErrorResponse {
        jsonrpc: "2.0",
        id,
        error: HostError {
            code: -32000,
            message: message.to_owned(),
        },
    })
    .unwrap_or(Value::Null)
}

pub(crate) fn action_allowed(action: &PluginAction, permissions: &PluginPermissions) -> bool {
    match action {
        PluginAction::CopyText { .. } => true,
        PluginAction::OpenUrl { url } => {
            let Some(host) = url
                .strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .and_then(|rest| rest.split(['/', '?', '#']).next())
            else {
                return false;
            };
            let host_lower = host.to_ascii_lowercase();
            permissions.network.iter().any(|allowed| {
                let allowed_lower = allowed.to_ascii_lowercase();
                allowed == "*"
                    || host_lower == allowed_lower
                    || host_lower.ends_with(&format!(".{allowed_lower}"))
            })
        }
        PluginAction::OpenPath { path } => {
            let path = Path::new(path);
            permissions
                .filesystem
                .iter()
                .any(|allowed| allowed == "*" || path.starts_with(allowed))
        }
    }
}

pub(crate) fn split_action_keyword(query: &str, keywords: &[String]) -> Option<(String, String)> {
    keywords.iter().find_map(|keyword| {
        if query == keyword {
            return Some((keyword.clone(), String::new()));
        }
        query.strip_prefix(keyword).and_then(|rest| {
            (rest.starts_with(':') || rest.chars().next().is_some_and(char::is_whitespace)).then(
                || {
                    (
                        keyword.clone(),
                        rest.trim_start_matches(|character: char| {
                            character == ':' || character.is_whitespace()
                        })
                        .trim()
                        .to_owned(),
                    )
                },
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_matching_requires_boundary() {
        assert_eq!(
            split_action_keyword("gh issue", &[String::from("gh")]),
            Some((String::from("gh"), String::from("issue")))
        );
        assert_eq!(
            split_action_keyword("gh:issue", &[String::from("gh")]),
            Some((String::from("gh"), String::from("issue")))
        );
        assert!(split_action_keyword("ghost", &[String::from("gh")]).is_none());
    }

    #[test]
    fn declarative_actions_follow_permissions() {
        let unrestricted = flux_plugin_sdk::PluginPermissions {
            network: vec![String::from("example.com")],
            filesystem: vec![String::from("C:\\Vault")],
            shell: false,
        };
        assert!(action_allowed(
            &PluginAction::CopyText {
                text: String::from("safe"),
            },
            &unrestricted
        ));
        assert!(action_allowed(
            &PluginAction::OpenUrl {
                url: String::from("https://api.example.com/search"),
            },
            &unrestricted
        ));
        assert!(!action_allowed(
            &PluginAction::OpenUrl {
                url: String::from("file:///etc/passwd"),
            },
            &unrestricted
        ));
        assert!(action_allowed(
            &PluginAction::OpenPath {
                path: String::from("C:\\Vault\\note.md"),
            },
            &unrestricted
        ));
        assert!(!action_allowed(
            &PluginAction::OpenPath {
                path: String::from("C:\\Windows\\win.ini"),
            },
            &unrestricted
        ));
    }
}
