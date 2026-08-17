use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;
use std::sync::{
    mpsc::{self, SyncSender},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use everything_ipc::wm::{EverythingClient, RequestFlags, Sort};
use flux_core::SearchResult;
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
    installed_executable()
        .map(InstallationState::Installed)
        .unwrap_or(InstallationState::Missing)
}

pub fn winget_install_args() -> [&'static str; 4] {
    ["install", "-e", "--id", WINGET_PACKAGE_ID]
}

pub fn start_background_if_installed() -> Result<InstallationState, String> {
    let state = installation_state();
    let InstallationState::Installed(path) = &state else {
        return Ok(state);
    };

    #[cfg(windows)]
    {
        Command::new(path)
            .args(startup_args())
            .spawn()
            .map_err(|error| format!("Unable to start Everything in the background: {error}"))?;
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
            status: String::from("Everything is not available"),
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
                    Some(SearchResult::file(path, title, folder))
                })
                .collect::<Vec<_>>();
            EverythingResponse {
                sequence: request.sequence,
                query: request.query,
                status: format!("{} Everything result(s)", results.len()),
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
                status: format!("Everything query failed: {error}"),
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
    use super::{join_everything_path, startup_args, winget_install_args, WINGET_PACKAGE_ID};

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
