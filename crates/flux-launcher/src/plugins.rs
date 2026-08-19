use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    mpsc::{self, SyncSender},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

use flux_core::{
    query_request_line_with_keyword, FlowAction, FlowPluginManifest, FlowResponse, SearchResult,
};
use windui::prelude::Sender;

const MAX_RESULTS: usize = 16;
const QUERY_TIMEOUT: Duration = Duration::from_millis(450);
const DISCOVERY_REFRESH: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct PluginInvocation {
    executable: PathBuf,
    working_directory: PathBuf,
    method: String,
    parameters: Vec<serde_json::Value>,
}

#[derive(Clone, Debug)]
pub enum PluginAction {
    Flow(PluginInvocation),
    OpenUrl(String),
}

#[derive(Clone, Debug)]
pub struct PluginQueryResponse {
    pub sequence: u64,
    pub query: String,
    pub results: Vec<SearchResult>,
    pub status: String,
    pub available: bool,
    pub actions: HashMap<String, PluginAction>,
}

#[derive(Clone, Debug)]
struct PluginRequest {
    sequence: u64,
    query: String,
    obsidian_enabled: bool,
    obsidian_keyword: String,
    google_enabled: bool,
    google_keyword: String,
}

#[derive(Clone, Debug)]
struct PluginDescriptor {
    manifest: FlowPluginManifest,
    directory: PathBuf,
    executable: PathBuf,
}

pub struct FlowPluginWorker {
    latest: Arc<Mutex<Option<PluginRequest>>>,
    wake: SyncSender<()>,
}

impl FlowPluginWorker {
    pub fn spawn(output: Sender<PluginQueryResponse>) -> Self {
        let latest = Arc::new(Mutex::new(None::<PluginRequest>));
        let latest_for_worker = Arc::clone(&latest);
        let (wake, receiver) = mpsc::sync_channel::<()>(1);

        thread::Builder::new()
            .name(String::from("flux-flow-plugins"))
            .spawn(move || {
                let mut cache = Vec::<PluginDescriptor>::new();
                let mut refreshed_at = None::<Instant>;
                while receiver.recv().is_ok() {
                    let Some(request) = latest_for_worker
                        .lock()
                        .ok()
                        .and_then(|mut slot| slot.take())
                    else {
                        continue;
                    };
                    if refreshed_at
                        .map(|timestamp| timestamp.elapsed() >= DISCOVERY_REFRESH)
                        .unwrap_or(true)
                    {
                        cache = discover_plugins();
                        refreshed_at = Some(Instant::now());
                    }
                    let response = query_plugins(&cache, request);
                    let _ = output.send(response);
                }
            })
            .expect("failed to create Flow plugin worker thread");

        Self { latest, wake }
    }

    pub fn request(
        &self,
        sequence: u64,
        query: String,
        obsidian_enabled: bool,
        obsidian_keyword: String,
        google_enabled: bool,
        google_keyword: String,
    ) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(PluginRequest {
                sequence,
                query,
                obsidian_enabled,
                obsidian_keyword,
                google_enabled,
                google_keyword,
            });
            let _ = self.wake.try_send(());
        }
    }
}

pub fn execute_async(action: PluginAction) {
    thread::Builder::new()
        .name(String::from("flux-flow-action"))
        .spawn(move || match action {
            PluginAction::Flow(invocation) => {
                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1_u64,
                    "method": invocation.method,
                    "params": invocation.parameters,
                })
                .to_string();
                let _ = invoke_json_line(
                    &invocation.executable,
                    &invocation.working_directory,
                    &request,
                );
            }
            PluginAction::OpenUrl(url) => {
                let _ = crate::launch::open_url(&url);
            }
        })
        .expect("failed to create Flow plugin action thread");
}

fn query_plugins(plugins: &[PluginDescriptor], request: PluginRequest) -> PluginQueryResponse {
    let candidates = plugins
        .iter()
        .filter(|plugin| !is_google_plugin(plugin))
        .filter_map(|plugin| {
            action_keyword_for_plugin(plugin, &request).map(|keyword| (plugin, keyword))
        })
        .collect::<Vec<_>>();

    let mut results = Vec::with_capacity(MAX_RESULTS);
    let mut actions = HashMap::new();
    append_builtin_google_result(&request, &mut results, &mut actions);
    if candidates.is_empty() && results.is_empty() {
        return PluginQueryResponse {
            sequence: request.sequence,
            query: request.query,
            results,
            status: String::from("No native Flow plugins installed"),
            available: false,
            actions,
        };
    }
    let mut failures = 0_usize;
    for (plugin, action_keyword) in candidates {
        if results.len() >= MAX_RESULTS {
            break;
        }
        let search = if is_obsidian_plugin(plugin) {
            request
                .query
                .strip_prefix(&action_keyword)
                .map(|rest| {
                    rest.trim_start_matches(|character: char| {
                        character == ':' || character.is_whitespace()
                    })
                })
                .unwrap_or(request.query.trim())
        } else {
            plugin
                .manifest
                .search_for_action_keyword(&request.query, &action_keyword)
        };
        let request_line =
            match query_request_line_with_keyword(1, &request.query, search, &action_keyword) {
                Ok(request_line) => request_line,
                Err(_) => {
                    failures += 1;
                    continue;
                }
            };
        let response = invoke_json_line(&plugin.executable, &plugin.directory, &request_line)
            .and_then(|line| {
                serde_json::from_str::<FlowResponse>(&line).map_err(|error| error.to_string())
            });
        match response {
            Ok(response) => {
                if response.error.is_some() {
                    failures += 1;
                    continue;
                }
                append_plugin_results(plugin, response.result, &mut results, &mut actions);
            }
            Err(_) => failures += 1,
        }
    }

    let status = if results.is_empty() && failures > 0 {
        format!("{failures} native Flow plugin(s) did not respond")
    } else {
        format!("{} Flow plugin result(s)", results.len())
    };
    PluginQueryResponse {
        sequence: request.sequence,
        query: request.query,
        available: true,
        results,
        status,
        actions,
    }
}

fn append_builtin_google_result(
    request: &PluginRequest,
    results: &mut Vec<SearchResult>,
    actions: &mut HashMap<String, PluginAction>,
) {
    if !request.google_enabled || results.len() >= MAX_RESULTS {
        return;
    }
    let keyword = request.google_keyword.trim();
    let keyword = if keyword.is_empty() { "g" } else { keyword };
    let Some(search) = search_for_keyword(&request.query, keyword) else {
        return;
    };
    if search.is_empty() {
        return;
    }
    let url = format!(
        "https://www.google.com/search?q={}",
        encode_uri_component(search)
    );
    let id = String::from("builtin:google-search");
    results.push(SearchResult {
        id: id.clone(),
        title: format!("Search Google: {search}"),
        subtitle: String::from("Open the Google search in the default browser"),
        kind: flux_core::ResultKind::Placeholder,
        source: flux_core::ResultSource::Plugin,
        target: None,
    });
    actions.insert(id, PluginAction::OpenUrl(url));
}

fn search_for_keyword<'a>(query: &'a str, keyword: &str) -> Option<&'a str> {
    if !has_keyword_boundary(query, keyword) {
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

fn encode_uri_component(value: &str) -> String {
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

fn action_keyword_for_plugin(plugin: &PluginDescriptor, request: &PluginRequest) -> Option<String> {
    if is_obsidian_plugin(plugin) {
        if !request.obsidian_enabled {
            return None;
        }
        let keyword = request.obsidian_keyword.trim();
        let keyword = if keyword.is_empty() { "ob" } else { keyword };
        return has_keyword_boundary(&request.query, keyword).then(|| keyword.to_owned());
    }
    if is_google_plugin(plugin) {
        if !request.google_enabled {
            return None;
        }
        let keyword = request.google_keyword.trim();
        let keyword = if keyword.is_empty() { "g" } else { keyword };
        return has_keyword_boundary(&request.query, keyword).then(|| keyword.to_owned());
    }
    plugin
        .manifest
        .matching_action_keyword(&request.query)
        .map(str::to_owned)
}

fn is_obsidian_plugin(plugin: &PluginDescriptor) -> bool {
    plugin
        .manifest
        .id
        .eq_ignore_ascii_case("flux.obsidian.builtin")
}

fn is_google_plugin(plugin: &PluginDescriptor) -> bool {
    plugin
        .manifest
        .id
        .eq_ignore_ascii_case("flux.google.builtin")
}

fn has_keyword_boundary(query: &str, keyword: &str) -> bool {
    query == keyword
        || query.strip_prefix(keyword).is_some_and(|rest| {
            rest.starts_with(':') || rest.chars().next().is_some_and(char::is_whitespace)
        })
}

fn append_plugin_results(
    plugin: &PluginDescriptor,
    response: Vec<flux_core::FlowResult>,
    results: &mut Vec<SearchResult>,
    actions: &mut HashMap<String, PluginAction>,
) {
    for (index, item) in response.into_iter().enumerate() {
        if results.len() >= MAX_RESULTS {
            return;
        }
        let id = format!("flow:{}:{index}", plugin.manifest.id);
        let subtitle = if item.subtitle.is_empty() {
            plugin.manifest.name.clone()
        } else {
            format!("{} - {}", plugin.manifest.name, item.subtitle)
        };
        results.push(SearchResult {
            id: id.clone(),
            title: item.title,
            subtitle,
            kind: flux_core::ResultKind::Placeholder,
            source: flux_core::ResultSource::Plugin,
            target: None,
        });
        if let Some(action) = item.action {
            actions.insert(
                id,
                PluginAction::Flow(invocation_from_action(plugin, action)),
            );
        }
    }
}

fn invocation_from_action(plugin: &PluginDescriptor, action: FlowAction) -> PluginInvocation {
    PluginInvocation {
        executable: plugin.executable.clone(),
        working_directory: plugin.directory.clone(),
        method: action.method,
        parameters: action.parameters,
    }
}

fn discover_plugins() -> Vec<PluginDescriptor> {
    plugin_roots()
        .into_iter()
        .flat_map(|root| read_plugin_root(&root))
        .collect()
}

fn plugin_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(app_data) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(app_data).join("FluxLauncher").join("Plugins"));
    }
    if let Some(extra) = std::env::var_os("FLUX_PLUGIN_DIR") {
        roots.push(PathBuf::from(extra));
    }
    roots
}

fn read_plugin_root(root: &Path) -> Vec<PluginDescriptor> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| load_plugin(&entry.path()))
        .collect()
}

fn load_plugin(directory: &Path) -> Option<PluginDescriptor> {
    if !directory.is_dir() {
        return None;
    }
    let manifest_path = directory.join("plugin.json");
    let manifest =
        serde_json::from_str::<FlowPluginManifest>(&fs::read_to_string(manifest_path).ok()?)
            .ok()?;
    if !manifest.is_native_executable() {
        return None;
    }

    let directory = directory.canonicalize().ok()?;
    let executable = directory
        .join(&manifest.execute_file_name)
        .canonicalize()
        .ok()?;
    executable
        .starts_with(&directory)
        .then_some(PluginDescriptor {
            manifest,
            directory,
            executable,
        })
}

fn invoke_json_line(
    executable: &Path,
    working_directory: &Path,
    request: &str,
) -> Result<String, String> {
    let mut child = Command::new(executable)
        .current_dir(working_directory)
        .env("FLOW_VERSION", "Flux Launcher MVP")
        .env("FLOW_PROGRAM_DIRECTORY", working_directory)
        .env("FLOW_APPLICATION_DIRECTORY", working_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;

    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| String::from("plugin stdin unavailable"))?;
    input
        .write_all(request.as_bytes())
        .and_then(|_| input.write_all(b"\n"))
        .and_then(|_| input.flush())
        .map_err(|error| error.to_string())?;
    drop(input);

    let output = child
        .stdout
        .take()
        .ok_or_else(|| String::from("plugin stdout unavailable"))?;
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut reader = BufReader::new(output);
        let mut line = String::new();
        let result = reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())
            .and_then(|count| {
                (count > 0)
                    .then_some(line.trim().to_owned())
                    .ok_or_else(|| String::from("plugin returned no response"))
            });
        let _ = sender.send(result);
    });

    let response = receiver
        .recv_timeout(QUERY_TIMEOUT)
        .map_err(|_| String::from("plugin query timed out"))
        .and_then(|response| response);
    terminate_process(&mut child);
    response
}

fn terminate_process(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_keyword_requires_a_token_boundary() {
        assert!(has_keyword_boundary("g space exploration", "g"));
        assert!(has_keyword_boundary("g:space exploration", "g"));
        assert!(has_keyword_boundary("g", "g"));
        assert!(!has_keyword_boundary("gaming", "g"));
        assert!(!has_keyword_boundary("", "g"));
    }

    #[test]
    fn built_in_google_provider_creates_a_browser_action() {
        let request = PluginRequest {
            sequence: 7,
            query: String::from("g space exploration"),
            obsidian_enabled: true,
            obsidian_keyword: String::from("ob"),
            google_enabled: true,
            google_keyword: String::from("g"),
        };
        let mut results = Vec::new();
        let mut actions = HashMap::new();
        append_builtin_google_result(&request, &mut results, &mut actions);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "builtin:google-search");
        assert_eq!(results[0].title, "Search Google: space exploration");
        assert!(matches!(
            actions.get("builtin:google-search"),
            Some(PluginAction::OpenUrl(url)) if url == "https://www.google.com/search?q=space%20exploration"
        ));
    }

    #[test]
    fn built_in_google_provider_skips_alias_without_query() {
        let request = PluginRequest {
            sequence: 8,
            query: String::from("g"),
            obsidian_enabled: true,
            obsidian_keyword: String::from("ob"),
            google_enabled: true,
            google_keyword: String::from("g"),
        };
        let mut results = Vec::new();
        let mut actions = HashMap::new();
        append_builtin_google_result(&request, &mut results, &mut actions);
        assert!(results.is_empty());
        assert!(actions.is_empty());
    }
}
