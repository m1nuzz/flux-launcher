use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;
#[cfg(windows)]
use std::sync::OnceLock;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, SyncSender},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use everything_ipc::wm::{EverythingClient, RequestFlags, Sort};
use flux_core::{ResultKind, SearchResult};

use crate::applications::canonical_application_id;
use windui::prelude::Sender;

const MAX_RESULTS: u32 = 16;
const QUERY_TIMEOUT: Duration = Duration::from_millis(350);
pub const WINGET_PACKAGE_ID: &str = "voidtools.Everything";

fn startup_args() -> [&'static str; 1] {
    ["-startup"]
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallationState {
    Installed(PathBuf),
    Missing,
}

impl InstallationState {
    pub fn is_installed(&self) -> bool {
        matches!(self, Self::Installed(_))
    }
}

pub fn installation_state() -> InstallationState {
    if std::env::var_os("FLUX_SMOKE_EVERYTHING_MISSING").is_some() {
        return InstallationState::Missing;
    }
    if std::env::var_os("FLUX_SMOKE_EVERYTHING_INSTALLED").is_some() {
        return InstallationState::Installed(PathBuf::from("Everything.exe"));
    }
    installed_executable()
        .map(InstallationState::Installed)
        .unwrap_or(InstallationState::Missing)
}

pub fn winget_install_args() -> [&'static str; 4] {
    ["install", "-e", "--id", WINGET_PACKAGE_ID]
}

#[cfg(windows)]
fn everything_start_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(windows)]
fn everything_start_requested() -> &'static AtomicBool {
    static REQUESTED: AtomicBool = AtomicBool::new(false);
    &REQUESTED
}

fn should_start_everything(ipc_available: bool, process_running: bool) -> bool {
    !ipc_available && !process_running
}

#[cfg(windows)]
fn everything_process_running() -> bool {
    let Ok(output) = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq Everything.exe", "/FO", "CSV", "/NH"])
        .output()
    else {
        return false;
    };

    if !output.status.success() {
        return false;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.trim_start().starts_with("\"Everything.exe\""))
}

pub fn start_background_if_installed() -> Result<InstallationState, String> {
    let state = installation_state();
    let InstallationState::Installed(path) = &state else {
        return Ok(state);
    };

    #[cfg(windows)]
    {
        let _guard = everything_start_lock()
            .lock()
            .map_err(|_| String::from("Everything startup lock is poisoned"))?;
        let ipc_available = EverythingClient::new().is_ok();
        let process_running = everything_process_running();
        if !should_start_everything(ipc_available, process_running) {
            everything_start_requested().store(false, Ordering::Release);
            return Ok(state);
        }
        if everything_start_requested().load(Ordering::Acquire) {
            return Ok(state);
        }

        everything_start_requested().store(true, Ordering::Release);
        if let Err(error) = Command::new(path).args(startup_args()).spawn() {
            everything_start_requested().store(false, Ordering::Release);
            return Err(format!(
                "Unable to start Everything in the background: {error}"
            ));
        }
    }
    Ok(state)
}

pub fn launch_winget_install() -> Result<(), String> {
    #[cfg(windows)]
    {
        Command::new("winget")
            .args(winget_install_args())
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Unable to start winget: {error}"))
    }
    #[cfg(not(windows))]
    {
        Err(String::from("winget is only available on Windows"))
    }
}

#[cfg(windows)]
fn installed_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(root) = std::env::var_os(variable).map(PathBuf::from) {
            candidates.push(root.join("Everything").join("Everything.exe"));
            candidates.push(root.join("Everything 1.4").join("Everything.exe"));
            candidates.push(root.join("Everything 1.5").join("Everything.exe"));
            if let Ok(entries) = std::fs::read_dir(&root) {
                candidates.extend(entries.flatten().filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                    (name.starts_with("everything") && entry.path().is_dir())
                        .then(|| entry.path().join("Everything.exe"))
                }));
            }
        }
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .or_else(|| {
            let output = Command::new("where").arg("Everything.exe").output().ok()?;
            if !output.status.success() {
                return None;
            }
            output
                .stdout
                .split(|byte| *byte == b'\r' || *byte == b'\n')
                .filter(|line| !line.is_empty())
                .filter_map(|line| std::str::from_utf8(line).ok().map(PathBuf::from))
                .find(|path| path.is_file())
        })
}

#[cfg(not(windows))]
fn installed_executable() -> Option<PathBuf> {
    None
}

#[derive(Clone, Debug)]
pub struct EverythingResponse {
    pub sequence: u64,
    pub query: String,
    pub results: Vec<SearchResult>,
    pub status: String,
    pub available: bool,
}

#[derive(Clone, Debug)]
struct EverythingRequest {
    sequence: u64,
    query: String,
}

pub struct EverythingWorker {
    latest: Arc<Mutex<Option<EverythingRequest>>>,
    wake: SyncSender<()>,
}

impl EverythingWorker {
    pub fn spawn(output: Sender<EverythingResponse>) -> Self {
        let latest = Arc::new(Mutex::new(None::<EverythingRequest>));
        let latest_for_worker = Arc::clone(&latest);
        let (wake, receiver) = mpsc::sync_channel::<()>(1);

        thread::Builder::new()
            .name(String::from("flux-everything"))
            .spawn(move || {
                let mut client = None::<EverythingClient>;
                while receiver.recv().is_ok() {
                    let Some(request) = latest_for_worker
                        .lock()
                        .ok()
                        .and_then(|mut slot| slot.take())
                    else {
                        continue;
                    };
                    let response = query_everything(&mut client, request);
                    let _ = output.send(response);
                }
            })
            .expect("failed to create Everything worker thread");

        Self { latest, wake }
    }

    pub fn request(&self, sequence: u64, query: String) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(EverythingRequest { sequence, query });
            let _ = self.wake.try_send(());
        }
    }
}

fn query_everything(
    client: &mut Option<EverythingClient>,
    request: EverythingRequest,
) -> EverythingResponse {
    if client.is_none() {
        *client = EverythingClient::new().ok();
    }

    let Some(ipc_client) = client.as_ref() else {
        return EverythingResponse {
            sequence: request.sequence,
            query: request.query,
            results: Vec::new(),
            status: t!("everything.unavailable").into_owned(),
            available: false,
        };
    };

    let query = request.query.clone();
    let list = ipc_client
        .query_wait(&query)
        .request_flags(RequestFlags::FileName | RequestFlags::Path)
        .sort(Sort::DateModifiedDescending)
        .max_results(MAX_RESULTS)
        .timeout(QUERY_TIMEOUT)
        .call();

    match list {
        Ok(list) => {
            let results = list
                .iter()
                .filter_map(|item| {
                    let title = item.get_string(RequestFlags::FileName)?;
                    let folder = item.get_string(RequestFlags::Path).unwrap_or_default();
                    let path = join_everything_path(&folder, &title);
                    let mut result = SearchResult::file(path.clone(), title, folder);
                    if result.kind == ResultKind::Application {
                        if let Some(canonical_id) = canonical_application_id(&path) {
                            result.id = canonical_id;
                        }
                    }
                    Some(result)
                })
                .collect::<Vec<_>>();
            EverythingResponse {
                sequence: request.sequence,
                query: request.query,
                status: t!("everything.result_count", count = results.len()).into_owned(),
                results,
                available: true,
            }
        }
        Err(error) => {
            *client = None;
            EverythingResponse {
                sequence: request.sequence,
                query: request.query,
                results: Vec::new(),
                status: t!("everything.query_failed", error = error).into_owned(),
                available: false,
            }
        }
    }
}

fn join_everything_path(folder: &str, filename: &str) -> String {
    if folder.is_empty() {
        return filename.to_owned();
    }
    if folder.ends_with('\\') {
        return format!("{folder}{filename}");
    }
    format!("{folder}\\{filename}")
}

#[cfg(test)]
mod tests {
    use super::{
        join_everything_path, should_start_everything, startup_args, winget_install_args,
        WINGET_PACKAGE_ID,
    };

    #[test]
    fn startup_uses_official_background_option() {
        assert_eq!(startup_args(), ["-startup"]);
    }

    #[test]
    fn winget_install_uses_exact_official_package_id() {
        assert_eq!(
            winget_install_args(),
            ["install", "-e", "--id", WINGET_PACKAGE_ID]
        );
    }

    #[test]
    fn does_not_start_when_everything_ipc_is_already_available() {
        assert!(!should_start_everything(true, false));
    }

    #[test]
    fn does_not_start_when_everything_process_is_already_running() {
        assert!(!should_start_everything(false, true));
    }

    #[test]
    fn starts_only_when_ipc_and_process_are_both_absent() {
        assert!(should_start_everything(false, false));
    }

    #[test]
    fn joins_windows_folder_and_filename_without_duplicate_separator() {
        assert_eq!(
            join_everything_path(r"C:\Windows", "explorer.exe"),
            r"C:\Windows\explorer.exe"
        );
        assert_eq!(
            join_everything_path(r"C:\Windows\", "explorer.exe"),
            r"C:\Windows\explorer.exe"
        );
    }
}
