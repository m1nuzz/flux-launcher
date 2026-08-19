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

use crate::builtin::{query_builtin_providers, BuiltinAction, BuiltinQuery};
use flux_core::{
    query_request_line_with_keyword, FlowAction, FlowPluginManifest, FlowResponse, SearchResult,
};
use windui::core::ClipboardProvider;
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
    OpenPath(String),
    CopyText(String),
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

pub fn native_plugin_install_path() -> String {
    if let Some(app_data) = std::env::var_os("APPDATA") {
        return PathBuf::from(app_data)
            .join("FluxLauncher")
            .join("NativePlugins")
            .display()
            .to_string();
    }
    String::from("%APPDATA%\\FluxLauncher\\NativePlugins")
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
            PluginAction::OpenPath(path) => {
                let _ = crate::launch::open_path(&path);
            }
            PluginAction::CopyText(text) => {
                windui::platform::Clipboard.set_text(&text);
            }
        })
        .expect("failed to create Flow plugin action thread");
}

fn query_plugins(plugins: &[PluginDescriptor], request: PluginRequest) -> PluginQueryResponse {
    let candidates = plugins
        .iter()
        .filter(|plugin| !is_builtin_plugin(plugin))
        .filter_map(|plugin| {
            action_keyword_for_plugin(plugin, &request).map(|keyword| (plugin, keyword))
        })
        .collect::<Vec<_>>();

    let mut results = Vec::with_capacity(MAX_RESULTS);
    let mut actions = HashMap::new();
    append_builtin_results(&request, &mut results, &mut actions);
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
        let search = plugin
            .manifest
            .search_for_action_keyword(&request.query, &action_keyword);
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

fn append_builtin_results(
    request: &PluginRequest,
    results: &mut Vec<SearchResult>,
    actions: &mut HashMap<String, PluginAction>,
) {
    let builtin_request = BuiltinQuery {
        query: request.query.clone(),
        google_enabled: request.google_enabled,
        google_keyword: request.google_keyword.clone(),
        obsidian_enabled: request.obsidian_enabled,
        obsidian_keyword: request.obsidian_keyword.clone(),
    };
    for builtin in query_builtin_providers(&builtin_request) {
        if results.len() >= MAX_RESULTS {
            break;
        }
        let id = builtin.result.id.clone();
        if let Some(action) = builtin.action {
            let action = match action {
                BuiltinAction::OpenUrl(url) => PluginAction::OpenUrl(url),
            };
            actions.insert(id, action);
        }
        results.push(builtin.result);
    }
}

fn action_keyword_for_plugin(plugin: &PluginDescriptor, request: &PluginRequest) -> Option<String> {
    plugin
        .manifest
        .matching_action_keyword(&request.query)
        .map(str::to_owned)
}

fn is_builtin_plugin(plugin: &PluginDescriptor) -> bool {
    plugin
        .manifest
        .id
        .eq_ignore_ascii_case("flux.obsidian.builtin")
        || plugin
            .manifest
            .id
            .eq_ignore_ascii_case("flux.google.builtin")
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

#[derive(Clone, Debug)]
pub struct NativePluginQueryResponse {
    pub sequence: u64,
    pub query: String,
    pub results: Vec<SearchResult>,
    pub status: String,
    pub available: bool,
    pub actions: HashMap<String, PluginAction>,
}

#[derive(Clone, Debug)]
struct NativePluginRequest {
    sequence: u64,
    query: String,
}

pub struct NativePluginWorker {
    latest: Arc<Mutex<Option<NativePluginRequest>>>,
    wake: SyncSender<()>,
}

impl NativePluginWorker {
    pub fn spawn(output: Sender<NativePluginQueryResponse>) -> Self {
        let latest = Arc::new(Mutex::new(None::<NativePluginRequest>));
        let latest_for_worker = Arc::clone(&latest);
        let (wake, receiver) = mpsc::sync_channel::<()>(1);

        thread::Builder::new()
            .name(String::from("flux-native-plugins"))
            .spawn(move || {
                let mut host = None::<NativePluginHost>;
                while receiver.recv().is_ok() {
                    let Some(request) = latest_for_worker
                        .lock()
                        .ok()
                        .and_then(|mut slot| slot.take())
                    else {
                        continue;
                    };
                    if host.is_none() {
                        host = NativePluginHost::start().ok();
                    }
                    let query_result = host
                        .as_mut()
                        .map(|active_host| active_host.query(request.sequence, &request.query));
                    let response = match query_result {
                        Some(Ok(response)) => response,
                        Some(Err(error)) => {
                            if let Some(active_host) = host.as_mut() {
                                active_host.stop();
                            }
                            host = None;
                            NativePluginQueryResponse {
                                sequence: request.sequence,
                                query: request.query,
                                results: Vec::new(),
                                status: format!("Native plugin host unavailable: {error}"),
                                available: false,
                                actions: HashMap::new(),
                            }
                        }
                        None => NativePluginQueryResponse {
                            sequence: request.sequence,
                            query: request.query,
                            results: Vec::new(),
                            status: String::from("No native Rust plugin host installed"),
                            available: false,
                            actions: HashMap::new(),
                        },
                    };
                    let _ = output.send(response);
                }
            })
            .expect("failed to create native plugin worker thread");

        Self { latest, wake }
    }

    pub fn request(&self, sequence: u64, query: String) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(NativePluginRequest { sequence, query });
            let _ = self.wake.try_send(());
        }
    }
}

struct NativePluginHost {
    child: Child,
    stdin: std::process::ChildStdin,
    responses: mpsc::Receiver<Result<String, String>>,
}

impl NativePluginHost {
    fn start() -> Result<Self, String> {
        let executable = native_plugin_host_executable();
        let root = native_plugin_root();
        if !native_plugin_root_has_plugins(&root) {
            return Err(String::from("native plugin directory is empty"));
        }
        let mut child = Command::new(&executable)
            .arg("--plugin-host")
            .arg(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("{}: {error}", executable.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| String::from("native plugin host stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| String::from("native plugin host stdout unavailable"))?;
        let (sender, responses) = mpsc::channel();
        thread::Builder::new()
            .name(String::from("flux-native-plugin-reader"))
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            if sender.send(Ok(line.trim().to_owned())).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = sender.send(Err(error.to_string()));
                            break;
                        }
                    }
                }
            })
            .map_err(|error| format!("native plugin reader thread: {error}"))?;
        Ok(Self {
            child,
            stdin,
            responses,
        })
    }

    fn query(&mut self, sequence: u64, query: &str) -> Result<NativePluginQueryResponse, String> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": sequence,
            "method": "query",
            "params": {
                "query": query,
                "action_keyword": "",
                "locale": "en-US"
            }
        });
        let line = serde_json::to_string(&request).map_err(|error| error.to_string())?;
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|error| error.to_string())?;
        let response = self
            .responses
            .recv_timeout(QUERY_TIMEOUT)
            .map_err(|_| String::from("native plugin host query timed out"))??;
        parse_native_response(sequence, query, &response)
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for NativePluginHost {
    fn drop(&mut self) {
        self.stop();
    }
}

fn native_plugin_root() -> PathBuf {
    if let Some(root) = std::env::var_os("FLUX_NATIVE_PLUGIN_DIR") {
        return PathBuf::from(root);
    }
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("FluxLauncher").join("NativePlugins"))
        .unwrap_or_else(|| PathBuf::from("NativePlugins"))
}

fn native_plugin_root_has_plugins(root: &Path) -> bool {
    fs::read_dir(root)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .any(|entry| entry.path().join("plugin.toml").is_file())
}

fn native_plugin_host_executable() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| {
        PathBuf::from(if cfg!(windows) {
            "flux-launcher.exe"
        } else {
            "flux-launcher"
        })
    })
}

#[derive(Debug, serde::Deserialize)]
struct NativeHostResponse {
    result: Option<NativeHostPayload>,
    error: Option<NativeHostError>,
}

#[derive(Debug, serde::Deserialize)]
struct NativeHostPayload {
    results: Vec<flux_plugin_sdk::PluginResult>,
    errors: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct NativeHostError {
    message: String,
}

fn parse_native_response(
    sequence: u64,
    query: &str,
    line: &str,
) -> Result<NativePluginQueryResponse, String> {
    let response: NativeHostResponse = serde_json::from_str(line)
        .map_err(|error| format!("invalid native host response: {error}"))?;
    if let Some(error) = response.error {
        return Err(error.message);
    }
    let payload = response
        .result
        .ok_or_else(|| String::from("native host response has no result"))?;
    let mut actions = HashMap::new();
    let mut results = Vec::with_capacity(payload.results.len());
    for item in payload.results {
        let id = format!("native:{sequence}:{}", item.id);
        let action = item.action.map(|action| match action {
            flux_plugin_sdk::PluginAction::OpenUrl { url } => PluginAction::OpenUrl(url),
            flux_plugin_sdk::PluginAction::OpenPath { path } => PluginAction::OpenPath(path),
            flux_plugin_sdk::PluginAction::CopyText { text } => PluginAction::CopyText(text),
        });
        if let Some(action) = action {
            actions.insert(id.clone(), action);
        }
        results.push(SearchResult {
            id,
            title: item.title,
            subtitle: item.subtitle,
            kind: flux_core::ResultKind::Placeholder,
            source: flux_core::ResultSource::Plugin,
            target: None,
        });
    }
    let status = if payload.errors.is_empty() {
        format!("{} native plugin result(s)", results.len())
    } else {
        payload.errors.join("; ")
    };
    Ok(NativePluginQueryResponse {
        sequence,
        query: query.to_owned(),
        results,
        status,
        available: true,
        actions,
    })
}
