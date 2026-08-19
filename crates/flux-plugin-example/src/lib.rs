use flux_plugin_sdk::{
    buffer_into_string, input_str, json_buffer, FluxPluginBuffer, PluginAction, PluginExecute,
    PluginExecuteResponse, PluginManifest, PluginManifestDocument, PluginPermissions, PluginQuery,
    PluginQueryResponse, PluginResult, PLUGIN_API_VERSION,
};
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};

fn manifest() -> PluginManifestDocument {
    PluginManifestDocument {
        plugin: PluginManifest {
            name: String::from("Example Native"),
            version: String::from("0.1.0"),
            api_version: PLUGIN_API_VERSION,
            entry_point: String::from("flux_plugin_example.dll"),
            description: String::from("Example native Rust community plugin"),
            action_keywords: vec![String::from("ex")],
        },
        permissions: PluginPermissions::default(),
    }
}

fn write_json<T: serde::Serialize>(output: *mut FluxPluginBuffer, value: &T) -> i32 {
    let Some(output) = (unsafe { output.as_mut() }) else {
        return -2;
    };
    match json_buffer(value) {
        Ok(buffer) => {
            *output = buffer;
            0
        }
        Err(_) => -3,
    }
}

fn query_response(request: &PluginQuery) -> PluginQueryResponse {
    let query = request.query.trim();
    if query.is_empty() {
        return PluginQueryResponse {
            results: Vec::new(),
        };
    }
    PluginQueryResponse {
        results: vec![PluginResult {
            id: String::from("example:copy"),
            title: format!("Example: {query}"),
            subtitle: String::from("Copy the query text"),
            score: 100,
            action: Some(PluginAction::CopyText {
                text: query.to_owned(),
            }),
        }],
    }
}

fn execute_response(request: &PluginExecute) -> PluginExecuteResponse {
    let success = matches!(request.action, PluginAction::CopyText { .. });
    PluginExecuteResponse {
        success,
        error: (!success).then(|| String::from("unsupported action")),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn flux_plugin_api_version() -> u32 {
    PLUGIN_API_VERSION
}

/// Returns the plugin manifest as a host-owned buffer.
///
/// # Safety
///
/// The host must pass a valid writable output pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flux_plugin_manifest_json(output: *mut FluxPluginBuffer) -> i32 {
    catch_unwind(AssertUnwindSafe(|| write_json(output, &manifest()))).unwrap_or(-4)
}

/// Creates an opaque plugin context owned by the host.
///
/// # Safety
///
/// This function has no pointer preconditions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flux_plugin_create() -> *mut c_void {
    catch_unwind(AssertUnwindSafe(|| {
        Box::into_raw(Box::new(())) as *mut c_void
    }))
    .unwrap_or(std::ptr::null_mut())
}

/// Executes a bounded query request and writes a JSON response.
///
/// # Safety
///
/// The input pointer must reference `request_len` readable bytes and output
/// must be a valid writable buffer pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flux_plugin_query(
    _context: *mut c_void,
    request_ptr: *const u8,
    request_len: usize,
    output: *mut FluxPluginBuffer,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(request) = (unsafe { input_str(request_ptr, request_len) }) else {
            return -2;
        };
        let Ok(request) = serde_json::from_str::<PluginQuery>(request) else {
            return -3;
        };
        write_json(output, &query_response(&request))
    }))
    .unwrap_or(-4)
}

/// Executes a declarative plugin action request.
///
/// # Safety
///
/// The input pointer must reference `request_len` readable bytes and output
/// must be a valid writable buffer pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flux_plugin_execute(
    _context: *mut c_void,
    request_ptr: *const u8,
    request_len: usize,
    output: *mut FluxPluginBuffer,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(request) = (unsafe { input_str(request_ptr, request_len) }) else {
            return -2;
        };
        let Ok(request) = serde_json::from_str::<PluginExecute>(request) else {
            return -3;
        };
        write_json(output, &execute_response(&request))
    }))
    .unwrap_or(-4)
}

/// Frees a response buffer allocated by this plugin.
///
/// # Safety
///
/// The buffer must have been returned by this plugin and must not have been
/// freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flux_plugin_free_buffer(buffer: FluxPluginBuffer) {
    if buffer.ptr.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        let _ = buffer_into_string(buffer);
    }));
}

/// Destroys an opaque plugin context.
///
/// # Safety
///
/// The context must have been returned by `flux_plugin_create` and must not
/// have been destroyed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flux_plugin_destroy(context: *mut c_void) {
    if context.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        drop(Box::from_raw(context as *mut ()))
    }));
}
