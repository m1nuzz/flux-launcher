use std::sync::{
    mpsc::{self, SyncSender},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use everything_ipc::wm::{EverythingClient, RequestFlags};
use flux_core::SearchResult;
use windui::prelude::Sender;

const MAX_RESULTS: u32 = 8;
const QUERY_TIMEOUT: Duration = Duration::from_millis(350);

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
    use super::join_everything_path;

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
