#[cfg(windows)]
use crate::plugin_transport::create_host_io;
use crate::plugin_transport::{stdio_host_io, HostIo};
use flux_plugin_sdk::{
    FluxPluginApiVersionFn, FluxPluginBuffer, FluxPluginCreateFn, FluxPluginDestroyFn,
    FluxPluginExecuteFn, FluxPluginFreeBufferFn, FluxPluginManifestFn, FluxPluginQueryFn,
    PluginAction, PluginExecute, PluginExecuteResponse, PluginManifestDocument, PluginQuery,
    PluginQueryResponse, MAX_RESPONSE_BYTES, MAX_RESULTS, PLUGIN_API_VERSION,
};
use libloading::{Library, Symbol};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::c_void;
use std::fs;

use super::host_protocol::{
    action_allowed, handle_request, split_action_keyword, HostQueryResponse,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

const MAX_PLUGINS: usize = 64;
const MAX_REQUEST_BYTES: usize = 64 * 1024;
const PLUGIN_FAILURE_QUARANTINE_THRESHOLD: u32 = 3;

fn should_quarantine(failure_count: u32) -> bool {
    failure_count >= PLUGIN_FAILURE_QUARANTINE_THRESHOLD
}

struct LoadedPlugin {
    id: String,
    manifest: PluginManifestDocument,
    _library: Library,
    context: *mut c_void,
    query: FluxPluginQueryFn,
    execute: FluxPluginExecuteFn,
    free_buffer: FluxPluginFreeBufferFn,
    destroy: FluxPluginDestroyFn,
    failure_count: u32,
    quarantined: bool,
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        if !self.context.is_null() {
            unsafe { (self.destroy)(self.context) };
            self.context = std::ptr::null_mut();
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct PersistedPluginHealth {
    failure_count: u32,
    quarantined: bool,
}

pub(crate) struct PluginHost {
    plugins: Vec<LoadedPlugin>,
    health_path: PathBuf,
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
        let health_path = root
            .parent()
            .unwrap_or(root)
            .join("native-plugin-health.json");
        if let Ok(bytes) = fs::read(&health_path) {
            if let Ok(health) =
                serde_json::from_slice::<HashMap<String, PersistedPluginHealth>>(&bytes)
            {
                for plugin in &mut plugins {
                    if let Some(saved) = health.get(&plugin.id) {
                        plugin.failure_count = saved.failure_count;
                        plugin.quarantined = saved.quarantined;
                    }
                }
            }
        }
        Self {
            plugins,
            health_path,
        }
    }

    fn persist_health(&self) {
        let health = self
            .plugins
            .iter()
            .map(|plugin| {
                (
                    plugin.id.clone(),
                    PersistedPluginHealth {
                        failure_count: plugin.failure_count,
                        quarantined: plugin.quarantined,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let Ok(encoded) = serde_json::to_vec_pretty(&health) else {
            return;
        };
        if let Some(parent) = self.health_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let temp = self.health_path.with_extension("json.tmp");
        if fs::write(&temp, encoded).is_ok() {
            let _ = fs::rename(temp, &self.health_path);
        }
    }

    pub(crate) fn query(&mut self, request: PluginQuery) -> HostQueryResponse {
        let mut results = Vec::new();
        let mut errors = Vec::new();
        for plugin in &mut self.plugins {
            if plugin.quarantined {
                errors.push(format!("{}: plugin is quarantined", plugin.id));
                continue;
            }
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
            match catch_unwind(AssertUnwindSafe(|| plugin.query(plugin_request))) {
                Ok(Ok(response)) => {
                    plugin.failure_count = 0;
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
                Ok(Err(error)) => {
                    plugin.failure_count = plugin.failure_count.saturating_add(1);
                    if should_quarantine(plugin.failure_count) {
                        plugin.quarantined = true;
                        errors.push(format!(
                            "{}: quarantined after {} failures",
                            plugin.id, plugin.failure_count
                        ));
                    } else {
                        errors.push(format!("{}: {error}", plugin.id));
                    }
                }
                Err(_) => {
                    plugin.failure_count = plugin.failure_count.saturating_add(1);
                    if should_quarantine(plugin.failure_count) {
                        plugin.quarantined = true;
                        errors.push(format!(
                            "{}: quarantined after {} failures",
                            plugin.id, plugin.failure_count
                        ));
                    } else {
                        errors.push(format!(
                            "{}: plugin panicked while processing query",
                            plugin.id
                        ));
                    }
                }
            }
        }
        self.persist_health();
        HostQueryResponse { results, errors }
    }

    pub(crate) fn execute(
        &mut self,
        plugin_id: &str,
        action: PluginAction,
    ) -> Result<PluginExecuteResponse, String> {
        let plugin = self
            .plugins
            .iter_mut()
            .find(|plugin| plugin.id == plugin_id)
            .ok_or_else(|| format!("plugin not found: {plugin_id}"))?;
        if plugin.quarantined {
            return Err(format!("plugin is quarantined: {plugin_id}"));
        }
        let outcome = match catch_unwind(AssertUnwindSafe(|| {
            plugin.execute(PluginExecute { action })
        })) {
            Ok(Ok(response)) => {
                plugin.failure_count = 0;
                Ok(response)
            }
            Ok(Err(error)) => {
                plugin.failure_count = plugin.failure_count.saturating_add(1);
                if should_quarantine(plugin.failure_count) {
                    plugin.quarantined = true;
                    Err(format!(
                        "plugin quarantined after {} failures: {error}",
                        plugin.failure_count
                    ))
                } else {
                    Err(error)
                }
            }
            Err(_) => {
                plugin.failure_count = plugin.failure_count.saturating_add(1);
                if should_quarantine(plugin.failure_count) {
                    plugin.quarantined = true;
                    Err(format!(
                        "plugin quarantined after {} failures: plugin panicked",
                        plugin.failure_count
                    ))
                } else {
                    Err(String::from("plugin panicked while executing action"))
                }
            }
        };
        self.persist_health();
        outcome
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
        failure_count: 0,
        quarantined: false,
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

pub fn run(root: PathBuf, pipe_name: Option<String>) {
    let host = PluginHost::discover(&root);
    #[cfg(windows)]
    if let Some(pipe_name) = pipe_name {
        match create_host_io(&pipe_name) {
            Ok(io) => run_loop(host, io),
            Err(error) => {
                eprintln!("native plugin host pipe error: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    let _ = pipe_name;
    run_loop(host, stdio_host_io());
}

fn run_loop(mut host: PluginHost, mut io: HostIo) {
    let mut line = String::new();
    loop {
        line.clear();
        match io.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.len() > MAX_REQUEST_BYTES {
                    let response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": { "code": -32600, "message": "request exceeds 64 KiB" }
                    });
                    if let Ok(encoded) = serde_json::to_string(&response) {
                        let _ = io.write_line(&encoded);
                    }
                    continue;
                }
                let response = handle_request(&mut host, line.trim_end());
                if let Ok(encoded) = serde_json::to_string(&response) {
                    if io.write_line(&encoded).is_err() {
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_quarantine_starts_after_three_failures() {
        assert!(!should_quarantine(0));
        assert!(!should_quarantine(2));
        assert!(should_quarantine(3));
    }

    #[test]
    fn malformed_request_returns_json_rpc_error() {
        let root = std::env::temp_dir().join("flux-empty-plugin-host-test");
        let mut host = PluginHost {
            plugins: Vec::new(),
            health_path: root.join("native-plugin-health.json"),
        };
        let response = handle_request(&mut host, "not-json");
        assert_eq!(response["error"]["code"], -32700);
        let _ = fs::remove_dir_all(root);
    }
}
