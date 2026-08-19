use flux_plugin_sdk::{
    FluxPluginApiVersionFn, FluxPluginBuffer, FluxPluginCreateFn, FluxPluginDestroyFn,
    FluxPluginExecuteFn, FluxPluginFreeBufferFn, FluxPluginManifestFn, FluxPluginQueryFn,
    PluginAction, PluginExecute, PluginExecuteResponse, PluginManifestDocument, PluginQuery,
    PluginQueryResponse, MAX_RESPONSE_BYTES, MAX_RESULTS, PLUGIN_API_VERSION,
};
use libloading::{Library, Symbol};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::c_void;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

const MAX_PLUGINS: usize = 64;

struct LoadedPlugin {
    id: String,
    manifest: PluginManifestDocument,
    _library: Library,
    context: *mut c_void,
    query: FluxPluginQueryFn,
    execute: FluxPluginExecuteFn,
    free_buffer: FluxPluginFreeBufferFn,
    destroy: FluxPluginDestroyFn,
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        if !self.context.is_null() {
            unsafe { (self.destroy)(self.context) };
            self.context = std::ptr::null_mut();
        }
    }
}

struct PluginHost {
    plugins: Vec<LoadedPlugin>,
}

impl PluginHost {
    fn discover(root: &Path) -> Self {
        let mut plugins = fs::read_dir(root)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.flatten())
            .filter_map(|entry| load_plugin(&entry.path()).ok())
            .take(MAX_PLUGINS)
            .collect::<Vec<_>>();
        plugins.sort_by(|left, right| left.id.cmp(&right.id));
        Self { plugins }
    }

    fn query(&mut self, request: PluginQuery) -> HostQueryResponse {
        let mut results = Vec::new();
        let mut errors = Vec::new();
        for plugin in &mut self.plugins {
            let Some((action_keyword, search)) =
                split_action_keyword(&request.query, &plugin.manifest.plugin.action_keywords)
            else {
                continue;
            };
            let plugin_request = PluginQuery {
                query: search,
                action_keyword,
                locale: request.locale.clone(),
            };
            match plugin.query(plugin_request) {
                Ok(response) => {
                    for mut result in response.results {
                        if results.len() >= MAX_RESULTS {
                            break;
                        }
                        if let Some(action) = result.action.as_ref() {
                            if !action_allowed(action, &plugin.manifest.permissions) {
                                errors.push(format!("{}: action denied by permissions", plugin.id));
                                result.action = None;
                            }
                        }
                        results.push(result);
                    }
                }
                Err(error) => errors.push(format!("{}: {error}", plugin.id)),
            }
        }
        HostQueryResponse { results, errors }
    }

    fn execute(
        &mut self,
        plugin_id: &str,
        action: PluginAction,
    ) -> Result<PluginExecuteResponse, String> {
        let plugin = self
            .plugins
            .iter_mut()
            .find(|plugin| plugin.id == plugin_id)
            .ok_or_else(|| format!("plugin not found: {plugin_id}"))?;
        plugin.execute(PluginExecute { action })
    }
}

impl LoadedPlugin {
    fn query(&mut self, request: PluginQuery) -> Result<PluginQueryResponse, String> {
        let request = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        let buffer = unsafe {
            let mut output = FluxPluginBuffer::empty();
            let status = (self.query)(self.context, request.as_ptr(), request.len(), &mut output);
            if status != 0 {
                return Err(format!("query returned status {status}"));
            }
            output
        };
        let response = copy_and_free_buffer(buffer, self.free_buffer)?;
        serde_json::from_slice(&response)
            .map_err(|error| format!("invalid query response: {error}"))
    }

    fn execute(&mut self, request: PluginExecute) -> Result<PluginExecuteResponse, String> {
        let request = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        let buffer = unsafe {
            let mut output = FluxPluginBuffer::empty();
            let status = (self.execute)(self.context, request.as_ptr(), request.len(), &mut output);
            if status != 0 {
                return Err(format!("execute returned status {status}"));
            }
            output
        };
        let response = copy_and_free_buffer(buffer, self.free_buffer)?;
        serde_json::from_slice(&response)
            .map_err(|error| format!("invalid execute response: {error}"))
    }
}

fn load_plugin(directory: &Path) -> Result<LoadedPlugin, String> {
    let manifest_path = directory.join("plugin.toml");
    let manifest_text = fs::read_to_string(&manifest_path).map_err(|error| error.to_string())?;
    let manifest: PluginManifestDocument =
        toml::from_str(&manifest_text).map_err(|error| error.to_string())?;
    manifest.validate()?;
    let library_path = directory.join(&manifest.plugin.entry_point);
    if !library_path.is_file() {
        return Err(format!("entry point not found: {}", library_path.display()));
    }
    let library = unsafe { Library::new(&library_path).map_err(|error| error.to_string())? };
    let api_version =
        unsafe { *load_symbol::<FluxPluginApiVersionFn>(&library, b"flux_plugin_api_version")? };
    let manifest_fn =
        unsafe { *load_symbol::<FluxPluginManifestFn>(&library, b"flux_plugin_manifest_json")? };
    let create = unsafe { *load_symbol::<FluxPluginCreateFn>(&library, b"flux_plugin_create")? };
    let query = unsafe { *load_symbol::<FluxPluginQueryFn>(&library, b"flux_plugin_query")? };
    let execute = unsafe { *load_symbol::<FluxPluginExecuteFn>(&library, b"flux_plugin_execute")? };
    let free_buffer =
        unsafe { *load_symbol::<FluxPluginFreeBufferFn>(&library, b"flux_plugin_free_buffer")? };
    let destroy = unsafe { *load_symbol::<FluxPluginDestroyFn>(&library, b"flux_plugin_destroy")? };
    if unsafe { api_version() } != PLUGIN_API_VERSION {
        return Err(String::from("plugin ABI version mismatch"));
    }
    let reported_manifest = unsafe {
        let mut buffer = FluxPluginBuffer::empty();
        let status = manifest_fn(&mut buffer);
        if status != 0 {
            return Err(format!("manifest returned status {status}"));
        }
        copy_and_free_buffer(buffer, free_buffer)?
    };
    let reported_manifest: PluginManifestDocument =
        serde_json::from_slice(&reported_manifest).map_err(|error| error.to_string())?;
    reported_manifest.validate()?;
    if reported_manifest.plugin.name != manifest.plugin.name
        || reported_manifest.plugin.version != manifest.plugin.version
    {
        return Err(String::from("manifest file and DLL metadata do not match"));
    }
    let context = unsafe { create() };
    if context.is_null() {
        return Err(String::from("plugin create returned null"));
    }
    Ok(LoadedPlugin {
        id: manifest.plugin.name.clone(),
        manifest,
        _library: library,
        context,
        query,
        execute,
        free_buffer,
        destroy,
    })
}

unsafe fn load_symbol<'a, T: Copy>(
    library: &'a Library,
    name: &[u8],
) -> Result<Symbol<'a, T>, String> {
    unsafe { library.get(name).map_err(|error| error.to_string()) }
}

fn copy_and_free_buffer(
    buffer: FluxPluginBuffer,
    free_buffer: FluxPluginFreeBufferFn,
) -> Result<Vec<u8>, String> {
    if buffer.len > MAX_RESPONSE_BYTES {
        if !buffer.ptr.is_null() {
            unsafe { free_buffer(buffer) };
        }
        return Err(String::from("plugin response exceeds maximum size"));
    }
    if buffer.ptr.is_null() {
        return if buffer.len == 0 {
            Ok(Vec::new())
        } else {
            Err(String::from("plugin returned null response buffer"))
        };
    }
    let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr, buffer.len) }.to_vec();
    unsafe { free_buffer(buffer) };
    Ok(bytes)
}

fn action_allowed(action: &PluginAction, permissions: &flux_plugin_sdk::PluginPermissions) -> bool {
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

fn split_action_keyword(query: &str, keywords: &[String]) -> Option<(String, String)> {
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
struct HostQueryResponse {
    results: Vec<flux_plugin_sdk::PluginResult>,
    errors: Vec<String>,
}

fn handle_request(host: &mut PluginHost, line: &str) -> Value {
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

pub fn run(root: PathBuf) {
    let mut host = PluginHost::discover(&root);
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines().map_while(Result::ok) {
        let response = handle_request(&mut host, &line);
        if serde_json::to_writer(&mut stdout, &response).is_ok() {
            let _ = stdout.write_all(b"\n");
            let _ = stdout.flush();
        }
    }
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
    fn malformed_request_returns_json_rpc_error() {
        let root = std::env::temp_dir().join("flux-empty-plugin-host-test");
        let mut host = PluginHost {
            plugins: Vec::new(),
        };
        let response = handle_request(&mut host, "not-json");
        assert_eq!(response["error"]["code"], -32700);
        let _ = fs::remove_dir_all(root);
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
