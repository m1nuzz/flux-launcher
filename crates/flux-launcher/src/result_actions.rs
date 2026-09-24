use std::collections::HashMap;

#[cfg(windows)]
use windows::core::BOOL;
#[cfg(windows)]
use windows::Win32::Foundation::{GlobalFree, HANDLE};
#[cfg(windows)]
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
#[cfg(windows)]
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
#[cfg(windows)]
use windows::Win32::UI::Shell::DROPFILES;

use flux_core::{ResultKind, SearchResult};
use windui::core::ClipboardProvider;

use super::launch;
use super::plugins::{execute_async, PluginAction};

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
            let _ = GlobalFree(Some(hmem));
            return false;
        }
        std::ptr::write_bytes(ptr, 0, bytes);
        let drop = ptr as *mut DROPFILES;
        (*drop).pFiles = header as u32;
        (*drop).fWide = BOOL(1);
        std::ptr::copy_nonoverlapping(path.as_ptr() as *const u8, ptr.add(header), path.len() * 2);
        let _ = GlobalUnlock(hmem);
        if OpenClipboard(None).is_err() {
            let _ = GlobalFree(Some(hmem));
            return false;
        }
        let ok = EmptyClipboard().is_ok() && SetClipboardData(15, Some(HANDLE(hmem.0))).is_ok();
        let _ = CloseClipboard();
        // A successful SetClipboardData hands the block to the clipboard; every
        // other outcome leaves this process as its owner.
        if !ok {
            let _ = GlobalFree(Some(hmem));
        }
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
                launch::open_path_async(target);
                true
            } else {
                false
            }
        }
        ActionKind::RunAsAdmin => result
            .target
            .as_deref()
            .map(launch::run_as_admin)
            .unwrap_or(false),
        ActionKind::OpenLocation => {
            if let Some(target) = result.target.as_deref() {
                let _ = launch::open_file_location(target);
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
            execute_async(invocation.clone());
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_core::{ResultKind, ResultSource, SearchResult};

    #[test]
    fn application_results_offer_priority_and_launch_actions_in_order() {
        let result = SearchResult {
            id: String::from("app:probe"),
            title: String::from("Result Mouse Probe"),
            subtitle: String::from("Application • Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\ResultMouseProbe.lnk")),
        };
        let actions = actions_for_result(&result, &std::collections::HashMap::new());
        let labels: Vec<_> = actions.iter().map(|action| action.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Set as priority (move to top)",
                "Open",
                "Run as admin",
                "Open file location",
                "Copy file",
                "Copy folder path",
            ]
        );
        assert!(matches!(actions[0].kind, ActionKind::SetPriority));
        assert!(matches!(actions[1].kind, ActionKind::Open));
        assert!(matches!(actions[2].kind, ActionKind::RunAsAdmin));
        assert!(matches!(actions[3].kind, ActionKind::OpenLocation));
        assert!(matches!(actions[4].kind, ActionKind::CopyFile));
        assert!(matches!(actions[5].kind, ActionKind::CopyFolderPath));
    }

    #[test]
    fn system_results_only_offer_open_and_copy_name_actions() {
        let result = SearchResult {
            id: String::from("system:settings"),
            title: String::from("Settings"),
            subtitle: String::from("Windows Settings"),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: Some(String::from("ms-settings:")),
        };
        let actions = actions_for_result(&result, &std::collections::HashMap::new());
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[0].kind, ActionKind::Open));
        assert!(matches!(actions[1].kind, ActionKind::CopyName));
    }

    #[test]
    fn copy_path_always_uses_one_pair_of_quotes() {
        let result = SearchResult {
            id: String::from("file:test"),
            title: String::from("Roaming"),
            subtitle: String::new(),
            kind: ResultKind::File,
            source: ResultSource::Everything,
            target: Some(String::from(r#"C:\Users\m1nus\AppData\Roaming"#)),
        };
        assert_eq!(
            quoted_result_path(&result).as_deref(),
            Some(r#""C:\Users\m1nus\AppData\Roaming""#)
        );

        let mut already_quoted = result.clone();
        already_quoted.target = Some(String::from(r#""C:\Users\m1nus\AppData\Roaming""#));
        assert_eq!(
            quoted_result_path(&already_quoted).as_deref(),
            Some(r#""C:\Users\m1nus\AppData\Roaming""#)
        );
    }
}
