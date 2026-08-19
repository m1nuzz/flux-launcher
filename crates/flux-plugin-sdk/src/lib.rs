use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::slice;
use std::str;

pub const PLUGIN_API_VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 512 * 1024;
pub const MAX_RESULTS: usize = 16;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FluxPluginBuffer {
    pub ptr: *mut u8,
    pub len: usize,
    pub capacity: usize,
}

impl FluxPluginBuffer {
    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            capacity: 0,
        }
    }
}

impl PluginManifestDocument {
    pub fn validate(&self) -> Result<(), String> {
        let plugin = &self.plugin;
        if plugin.api_version != PLUGIN_API_VERSION {
            return Err(format!("unsupported API version {}", plugin.api_version));
        }
        if plugin.name.trim().is_empty() || plugin.name.len() > 80 {
            return Err(String::from("plugin name must contain 1..80 characters"));
        }
        if plugin.version.trim().is_empty() || plugin.version.len() > 32 {
            return Err(String::from("plugin version must contain 1..32 characters"));
        }
        if plugin.entry_point.trim().is_empty()
            || plugin.entry_point.len() > 260
            || std::path::Path::new(&plugin.entry_point).is_absolute()
            || std::path::Path::new(&plugin.entry_point)
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(String::from("entry_point must be a bounded relative path"));
        }
        if plugin.action_keywords.is_empty() {
            return Err(String::from("at least one action keyword is required"));
        }
        if plugin.action_keywords.len() > 16
            || plugin.action_keywords.iter().any(|keyword| {
                keyword.trim().is_empty()
                    || keyword.len() > 32
                    || keyword.chars().any(char::is_whitespace)
            })
        {
            return Err(String::from(
                "action keywords must be 1..32 non-whitespace characters",
            ));
        }
        if self.permissions.network.len() > 32 || self.permissions.filesystem.len() > 32 {
            return Err(String::from("permission lists are limited to 32 entries"));
        }
        Ok(())
    }
}

pub type FluxPluginApiVersionFn = unsafe extern "C" fn() -> u32;
pub type FluxPluginManifestFn = unsafe extern "C" fn(*mut FluxPluginBuffer) -> i32;
pub type FluxPluginCreateFn = unsafe extern "C" fn() -> *mut c_void;
pub type FluxPluginQueryFn =
    unsafe extern "C" fn(*mut c_void, *const u8, usize, *mut FluxPluginBuffer) -> i32;
pub type FluxPluginExecuteFn =
    unsafe extern "C" fn(*mut c_void, *const u8, usize, *mut FluxPluginBuffer) -> i32;
pub type FluxPluginFreeBufferFn = unsafe extern "C" fn(FluxPluginBuffer);
pub type FluxPluginDestroyFn = unsafe extern "C" fn(*mut c_void);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PluginManifestDocument {
    pub plugin: PluginManifest,
    #[serde(default)]
    pub permissions: PluginPermissions,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub api_version: u32,
    pub entry_point: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub action_keywords: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct PluginPermissions {
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub filesystem: Vec<String>,
    #[serde(default)]
    pub shell: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PluginQuery {
    pub query: String,
    pub action_keyword: String,
    pub locale: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PluginExecute {
    pub action: PluginAction,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginAction {
    OpenUrl { url: String },
    OpenPath { path: String },
    CopyText { text: String },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PluginResult {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub score: i32,
    #[serde(default)]
    pub action: Option<PluginAction>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PluginQueryResponse {
    pub results: Vec<PluginResult>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PluginExecuteResponse {
    pub success: bool,
    #[serde(default)]
    pub error: Option<String>,
}

pub fn buffer_from_bytes(bytes: Vec<u8>) -> FluxPluginBuffer {
    let mut bytes = bytes;
    let buffer = FluxPluginBuffer {
        ptr: bytes.as_mut_ptr(),
        len: bytes.len(),
        capacity: bytes.capacity(),
    };
    std::mem::forget(bytes);
    buffer
}

pub fn buffer_from_string(value: String) -> FluxPluginBuffer {
    buffer_from_bytes(value.into_bytes())
}

/// Reclaims a buffer allocated by the plugin-side SDK allocator.
///
/// # Safety
///
/// The buffer must have been returned by `buffer_from_bytes` or an equivalent
/// plugin allocation and must not have been reclaimed previously.
pub unsafe fn buffer_into_vec(buffer: FluxPluginBuffer) -> Option<Vec<u8>> {
    if buffer.ptr.is_null() {
        return (buffer.len == 0 && buffer.capacity == 0).then(Vec::new);
    }
    if buffer.len > buffer.capacity {
        return None;
    }
    Some(unsafe { Vec::from_raw_parts(buffer.ptr, buffer.len, buffer.capacity) })
}

/// Reclaims and decodes a plugin-owned UTF-8 buffer.
///
/// # Safety
///
/// The buffer must have been returned by a compatible plugin allocation and
/// must not have been reclaimed previously.
pub unsafe fn buffer_into_string(buffer: FluxPluginBuffer) -> Option<String> {
    let bytes = unsafe { buffer_into_vec(buffer) }?;
    String::from_utf8(bytes).ok()
}

/// Validates a bounded input pointer and views it as a byte slice.
///
/// # Safety
///
/// The caller must ensure the pointer references readable memory for `len`
/// bytes for the duration of the returned slice.
pub unsafe fn input_slice<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len > MAX_REQUEST_BYTES || (ptr.is_null() && len != 0) {
        return None;
    }
    Some(unsafe { slice::from_raw_parts(ptr, len) })
}

/// Validates a bounded input pointer and views it as UTF-8 text.
///
/// # Safety
///
/// The caller must ensure the pointer references readable memory for `len`
/// bytes for the duration of the returned string slice.
pub unsafe fn input_str<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    let bytes = unsafe { input_slice(ptr, len) }?;
    str::from_utf8(bytes).ok()
}

pub fn json_buffer<T: Serialize>(value: &T) -> Result<FluxPluginBuffer, serde_json::Error> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "plugin response exceeds maximum size",
        )));
    }
    Ok(buffer_from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trip_preserves_permissions() {
        let document = PluginManifestDocument {
            plugin: PluginManifest {
                name: String::from("Example"),
                version: String::from("1.0.0"),
                api_version: PLUGIN_API_VERSION,
                entry_point: String::from("example.dll"),
                description: String::from("Example plugin"),
                action_keywords: vec![String::from("ex")],
            },
            permissions: PluginPermissions {
                network: vec![String::from("api.example.com")],
                filesystem: Vec::new(),
                shell: false,
            },
        };
        document.validate().unwrap();
        let encoded = toml::to_string(&document).unwrap();
        let decoded: PluginManifestDocument = toml::from_str(&encoded).unwrap();
        assert_eq!(document, decoded);
    }

    #[test]
    fn buffer_ownership_round_trip_is_explicit() {
        let buffer = buffer_from_string(String::from("hello"));
        let value = unsafe { buffer_into_string(buffer) }.unwrap();
        assert_eq!(value, "hello");
    }

    #[test]
    fn manifest_validation_rejects_traversal_and_empty_keywords() {
        let mut document = PluginManifestDocument {
            plugin: PluginManifest {
                name: String::from("Example"),
                version: String::from("1.0.0"),
                api_version: PLUGIN_API_VERSION,
                entry_point: String::from("../escape.dll"),
                description: String::new(),
                action_keywords: vec![String::from("ex")],
            },
            permissions: PluginPermissions::default(),
        };
        assert!(document.validate().is_err());
        document.plugin.entry_point = String::from("example.dll");
        document.plugin.action_keywords.clear();
        assert!(document.validate().is_err());
    }
}
