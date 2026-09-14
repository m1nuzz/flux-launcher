use std::collections::HashMap;
use std::fs;
#[cfg(not(windows))]
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    mpsc::{self, SyncSender},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use crate::plugin_limits::HostResourceGuard;
use crate::plugin_transport::{connect_host_io, ClientIo};
use crate::plugins::{terminate_process, PluginAction};
use flux_core::SearchResult;
use windui::prelude::Sender;

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
                let mut restart_count = 0_u32;
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
                            restart_count = restart_count.saturating_add(1);
                            NativePluginQueryResponse {
                                sequence: request.sequence,
                                query: request.query,
                                results: Vec::new(),
                                status: format!(
                                    "Native plugin host restarted (attempt {}): {error}",
                                    restart_count
                                ),
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
    io: ClientIo,
    _limits: HostResourceGuard,
}

impl NativePluginHost {
    fn start() -> Result<Self, String> {
        let executable = native_plugin_host_executable();
        let root = native_plugin_root();
        if !native_plugin_root_has_plugins(&root) {
            return Err(String::from("native plugin directory is empty"));
        }
        #[cfg(windows)]
        let (child, io, limits) = {
            let pipe_name = format!("\\\\.\\pipe\\flux-plugin-host-{}", std::process::id());
            let mut child = Command::new(&executable)
                .arg("--plugin-host")
                .arg(&root)
                .arg(&pipe_name)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| format!("{}: {error}", executable.display()))?;
            let limits = match HostResourceGuard::attach(&child) {
                Ok(limits) => limits,
                Err(error) => {
                    terminate_process(&mut child);
                    return Err(error);
                }
            };
            let io = match connect_host_io(&pipe_name, Duration::from_secs(3)) {
                Ok(io) => io,
                Err(error) => {
                    terminate_process(&mut child);
                    return Err(error);
                }
            };
            (child, ClientIo::Pipe(io), limits)
        };
        #[cfg(not(windows))]
        let (child, io, limits) = {
            let mut child = Command::new(&executable)
                .arg("--plugin-host")
                .arg(&root)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| format!("{}: {error}", executable.display()))?;
            let stdin = match child.stdin.take() {
                Some(stdin) => stdin,
                None => {
                    terminate_process(&mut child);
                    return Err(String::from("native plugin host stdin unavailable"));
                }
            };
            let stdout = match child.stdout.take() {
                Some(stdout) => stdout,
                None => {
                    terminate_process(&mut child);
                    return Err(String::from("native plugin host stdout unavailable"));
                }
            };
            let limits = match HostResourceGuard::attach(&child) {
                Ok(limits) => limits,
                Err(error) => {
                    terminate_process(&mut child);
                    return Err(error);
                }
            };
            (
                child,
                ClientIo::Stdio {
                    stdin,
                    stdout: BufReader::new(stdout),
                },
                limits,
            )
        };
        Ok(Self {
            child,
            io,
            _limits: limits,
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
        self.io
            .write_line(&line)
            .map_err(|error| error.to_string())?;
        let mut response = String::new();
        self.io
            .read_line(&mut response)
            .map_err(|error| error.to_string())?;
        if response.is_empty() {
            return Err(String::from("native plugin host returned no response"));
        }
        parse_native_response(sequence, query, response.trim_end())
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
