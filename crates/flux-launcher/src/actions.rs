use std::collections::HashMap;

#[cfg(windows)]
use windows::core::BOOL;
#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;
#[cfg(windows)]
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
#[cfg(windows)]
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
#[cfg(windows)]
use windows::Win32::UI::Shell::DROPFILES;

use crate::plugins::PluginAction;
use flux_core::{ResultKind, SearchResult};
use windui::core::ClipboardProvider;

#[derive(Clone, Debug)]
pub(crate) enum ActionKind {
    Open,
    RunAsAdmin,
    OpenLocation,
    CopyFile,
    CopyFolderPath,
    CopyName,
    SetPriority,
    RunPlugin(PluginAction),
}

#[derive(Clone, Debug)]
pub(crate) struct ActionItem {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) kind: ActionKind,
}

fn plugin_action_label(action: &PluginAction) -> &'static str {
    match action {
        PluginAction::Flow(_) => "Run plugin action",
        PluginAction::OpenUrl(_) => "Open web result",
        PluginAction::OpenPath(_) => "Open path",
        PluginAction::CopyText(_) => "Copy text",
    }
}

pub(crate) fn actions_for_result(
    result: &SearchResult,
    plugin_actions: &HashMap<String, PluginAction>,
) -> Vec<ActionItem> {
    let mut actions = Vec::with_capacity(6);
    if matches!(result.id.as_str(), "empty-recycle-bin" | "open-recycle-bin") {
        return actions;
    }
    if result.id.starts_with("system:") {
        actions.push(ActionItem {
            id: format!("{}:open", result.id),
            label: String::from("Open"),
            kind: ActionKind::Open,
        });
        actions.push(ActionItem {
            id: format!("{}:copy-name", result.id),
            label: String::from("Copy name"),
            kind: ActionKind::CopyName,
        });
        return actions;
    }
    if result.target.is_some() {
        if matches!(result.kind, ResultKind::Application) {
            actions.push(ActionItem {
                id: format!("{}:set-priority", result.id),
                label: String::from("Set as priority (move to top)"),
                kind: ActionKind::SetPriority,
            });
        }
        actions.push(ActionItem {
            id: format!("{}:open", result.id),
            label: String::from("Open"),
            kind: ActionKind::Open,
        });
        actions.push(ActionItem {
            id: format!("{}:run-as-admin", result.id),
            label: String::from("Run as admin"),
            kind: ActionKind::RunAsAdmin,
        });
        actions.push(ActionItem {
            id: format!("{}:open-location", result.id),
            label: String::from("Open file location"),
            kind: ActionKind::OpenLocation,
        });
        actions.push(ActionItem {
            id: format!("{}:copy-file", result.id),
            label: String::from("Copy file"),
            kind: ActionKind::CopyFile,
        });
        actions.push(ActionItem {
            id: format!("{}:copy-folder-path", result.id),
            label: String::from("Copy folder path"),
            kind: ActionKind::CopyFolderPath,
        });
    }
    if let Some(invocation) = plugin_actions.get(&result.id).cloned() {
        actions.push(ActionItem {
            id: format!("{}:plugin", result.id),
            label: String::from(plugin_action_label(&invocation)),
            kind: ActionKind::RunPlugin(invocation),
        });
    }
    if !matches!(result.kind, ResultKind::Application) {
        actions.push(ActionItem {
            id: format!("{}:copy-name", result.id),
            label: String::from("Copy name"),
            kind: ActionKind::CopyName,
        });
    }
    actions
}

pub(crate) fn selected_result(
    results: &[SearchResult],
    selected_id: &str,
    selected_index: usize,
) -> Option<SearchResult> {
    results
        .iter()
        .find(|result| result.id == selected_id)
        .cloned()
        .or_else(|| results.get(selected_index).cloned())
        .or_else(|| results.first().cloned())
}

pub(crate) fn quoted_result_path(result: &SearchResult) -> Option<String> {
    let target = result.target.as_deref()?.trim();
    if target.is_empty() {
        return None;
    }
    let target = target
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(target);
    Some(format!("\"{target}\""))
}

#[cfg(windows)]
pub(crate) fn copy_result_file(result: &SearchResult) -> bool {
    let Some(path) = result.target.as_deref() else {
        return false;
    };
    let path: Vec<u16> = path.encode_utf16().chain([0]).collect();
    let header = std::mem::size_of::<DROPFILES>();
    let bytes = header + path.len() * 2 + 2;
    unsafe {
        let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes) else {
            return false;
        };
        let ptr = GlobalLock(hmem) as *mut u8;
        if ptr.is_null() {
            return false;
        }
        std::ptr::write_bytes(ptr, 0, bytes);
        let drop = ptr as *mut DROPFILES;
        (*drop).pFiles = header as u32;
        (*drop).fWide = BOOL(1);
        std::ptr::copy_nonoverlapping(path.as_ptr() as *const u8, ptr.add(header), path.len() * 2);
        let _ = GlobalUnlock(hmem);
        if OpenClipboard(None).is_err() {
            return false;
        }
        let ok = EmptyClipboard().is_ok() && SetClipboardData(15, Some(HANDLE(hmem.0))).is_ok();
        let _ = CloseClipboard();
        ok
    }
}

#[cfg(not(windows))]
pub(crate) fn copy_result_file(_result: &SearchResult) -> bool {
    false
}

pub(crate) fn copy_result_path(result: &SearchResult) -> bool {
    let Some(path) = quoted_result_path(result) else {
        return false;
    };
    windui::platform::Clipboard.set_text(&path);
    true
}

pub(crate) fn execute_result_action(result: &SearchResult, action: &ActionKind) -> bool {
    match action {
        ActionKind::Open => {
            if let Some(target) = result.target.as_deref() {
                crate::launch::open_path_async(target);
                true
            } else {
                false
            }
        }
        ActionKind::RunAsAdmin => result
            .target
            .as_deref()
            .map(crate::launch::run_as_admin)
            .unwrap_or(false),
        ActionKind::OpenLocation => {
            if let Some(target) = result.target.as_deref() {
                let _ = crate::launch::open_file_location(target);
                true
            } else {
                false
            }
        }
        ActionKind::CopyFile => copy_result_file(result),
        ActionKind::CopyFolderPath => copy_result_path(result),
        ActionKind::CopyName => {
            windui::platform::Clipboard.set_text(&result.title);
            true
        }
        ActionKind::SetPriority => false,
        ActionKind::RunPlugin(invocation) => {
            crate::plugins::execute_async(invocation.clone());
            true
        }
    }
}
