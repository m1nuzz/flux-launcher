use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{self, SyncSender},
    Arc, Mutex, OnceLock,
};
use std::thread;

#[cfg(windows)]
use super::shell_icon_extract::{
    extract_icon_rgba_from_source, extract_shell_icon_rgba, extract_shell_thumbnail_rgba,
    is_executable_icon_target, shortcut_icon_location,
};

pub(crate) fn icon_completion_generation_changed(previous: u64, current: u64) -> bool {
    previous != current
}

#[cfg(windows)]
pub(crate) const MAX_SHELL_ICON_CACHE_ENTRIES: usize = 128;

#[cfg(windows)]
struct ShellIconCache {
    entries: HashMap<String, Option<Vec<u8>>>,
    lru_order: VecDeque<String>,
}

#[cfg(windows)]
impl ShellIconCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            lru_order: VecDeque::new(),
        }
    }

    fn get(&mut self, target: &str) -> Option<Option<Vec<u8>>> {
        let icon = self.entries.get(target).cloned()?;
        self.touch(target);
        Some(icon)
    }

    fn insert(&mut self, target: String, icon: Option<Vec<u8>>) {
        if self.entries.contains_key(&target) {
            self.entries.insert(target.clone(), icon);
            self.touch(&target);
            return;
        }
        while self.entries.len() >= MAX_SHELL_ICON_CACHE_ENTRIES {
            let Some(oldest) = self.lru_order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.lru_order.push_back(target.clone());
        self.entries.insert(target, icon);
    }

    fn touch(&mut self, target: &str) {
        if let Some(position) = self.lru_order.iter().position(|key| key == target) {
            self.lru_order.remove(position);
        }
        self.lru_order.push_back(target.to_owned());
    }
}

#[cfg(windows)]
static SHELL_ICON_CACHE: OnceLock<Mutex<ShellIconCache>> = OnceLock::new();
pub(crate) static SHELL_ICON_COMPLETION_GENERATION: AtomicU64 = AtomicU64::new(0);

pub(crate) struct ShellIconWorker {
    pending: Arc<Mutex<HashSet<String>>>,
    wake: SyncSender<String>,
}

#[cfg(windows)]
fn initialize_shell_icon_worker_com() -> bool {
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if result.is_ok() {
        true
    } else if result == RPC_E_CHANGED_MODE {
        eprintln!("[flux] shell icon worker inherited an initialized COM apartment");
        false
    } else {
        eprintln!("[flux] shell icon worker COM initialization failed: {result:?}");
        false
    }
}

impl ShellIconWorker {
    fn spawn() -> Self {
        let pending = Arc::new(Mutex::new(HashSet::<String>::new()));
        let pending_for_worker = Arc::clone(&pending);
        let (wake, receiver) = mpsc::sync_channel::<String>(64);
        thread::Builder::new()
            .name(String::from("flux-shell-icons"))
            .spawn(move || {
                #[cfg(windows)]
                let owns_com_apartment = initialize_shell_icon_worker_com();

                while let Ok(target) = receiver.recv() {
                    if let Ok(mut pending) = pending_for_worker.lock() {
                        pending.remove(&target);
                    }
                    #[cfg(windows)]
                    let _ = shell_icon_rgba(&target);
                    SHELL_ICON_COMPLETION_GENERATION.fetch_add(1, Ordering::Release);
                }

                #[cfg(windows)]
                if owns_com_apartment {
                    unsafe { windows::Win32::System::Com::CoUninitialize() };
                }
            })
            .expect("failed to create shell icon worker thread");
        Self { pending, wake }
    }

    fn request(&self, target: String) {
        let should_send = self
            .pending
            .lock()
            .map(|mut pending| pending.insert(target.clone()))
            .unwrap_or(false);
        if !should_send {
            return;
        }
        if self.wake.try_send(target.clone()).is_err() {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&target);
            }
        }
    }
}

static SHELL_ICON_WORKER: OnceLock<ShellIconWorker> = OnceLock::new();

pub(crate) fn shell_icon_worker() -> &'static ShellIconWorker {
    SHELL_ICON_WORKER.get_or_init(ShellIconWorker::spawn)
}

#[cfg(windows)]
pub(crate) fn shell_icon_cache_lookup(target: &str) -> Option<Option<Vec<u8>>> {
    let cache = SHELL_ICON_CACHE.get_or_init(|| Mutex::new(ShellIconCache::new()));
    cache.lock().ok().and_then(|mut cache| cache.get(target))
}

#[cfg(windows)]
pub(crate) fn request_shell_icon(target: &str) -> Option<Vec<u8>> {
    if let Some(icon) = shell_icon_cache_lookup(target) {
        return icon;
    }
    shell_icon_worker().request(target.to_owned());
    None
}

#[cfg(not(windows))]
pub(crate) fn request_shell_icon(_target: &str) -> Option<Vec<u8>> {
    None
}

pub(crate) fn trace_result_icon_probe(
    title: &str,
    target: Option<&str>,
    icon_target: Option<&str>,
    initial_loaded: bool,
) {
    let Some(path) = std::env::var_os("FLUX_ICON_PROBE_FILE") else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let sanitize = |value: &str| value.replace(['\t', '\r', '\n'], " ");
    let title = sanitize(title);
    let target = target.map(sanitize).unwrap_or_default();
    let icon_target = icon_target.map(sanitize).unwrap_or_default();
    let _ = writeln!(
        file,
        "title={title}\ttarget={target}\ticon_target={icon_target}\tinitial_loaded={initial_loaded}"
    );
}

pub(crate) fn trace_shell_icon_probe(target: &str, loaded: bool) {
    let Some(path) = std::env::var_os("FLUX_ICON_PROBE_FILE") else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let target = target.replace(['\t', '\r', '\n'], " ");
    let _ = writeln!(file, "target={target}\tloaded={loaded}");
}

#[cfg(windows)]
pub(crate) fn shortcut_icon_smoke(target: &str) -> bool {
    shortcut_icon_location(target)
        .and_then(|(path, index)| extract_icon_rgba_from_source(&path, Some(index)))
        .is_some_and(|rgba| rgba.len() == 32 * 32 * 4)
}

#[cfg(windows)]
pub(crate) fn shell_icon_rgba(target: &str) -> Option<Vec<u8>> {
    let cache = SHELL_ICON_CACHE.get_or_init(|| Mutex::new(ShellIconCache::new()));
    if let Ok(mut cache) = cache.lock() {
        if let Some(icon) = cache.get(target) {
            return icon;
        }
    }

    // Steam's Start Menu entries are commonly .lnk/.url shortcuts whose icon is
    // stored separately from the launch target. Resolve that explicit icon first,
    // matching Flow Launcher's shortcut-aware image loader, then use Shell fallbacks.
    let icon = shortcut_icon_location(target)
        .and_then(|(path, index)| extract_icon_rgba_from_source(&path, Some(index)))
        .or_else(|| {
            is_executable_icon_target(target)
                .then(|| extract_shell_icon_rgba(target))
                .flatten()
        })
        .or_else(|| extract_shell_thumbnail_rgba(target))
        .or_else(|| extract_shell_icon_rgba(target));
    trace_shell_icon_probe(target, icon.is_some());
    if let Ok(mut cache) = cache.lock() {
        cache.insert(target.to_owned(), icon.clone());
    }
    icon
}

#[cfg(not(windows))]
pub(crate) fn shell_icon_rgba(_target: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_shell_icon_cache_evicts_oldest_entries() {
        let mut cache = ShellIconCache::new();
        for index in 0..=MAX_SHELL_ICON_CACHE_ENTRIES {
            cache.insert(format!("target-{index}"), Some(vec![index as u8; 32]));
        }
        assert_eq!(cache.entries.len(), MAX_SHELL_ICON_CACHE_ENTRIES);
        assert!(cache.get("target-0").is_none());
        assert!(cache
            .get(&format!("target-{MAX_SHELL_ICON_CACHE_ENTRIES}"))
            .is_some());
    }

    #[test]
    fn shell_icon_cache_touch_preserves_recent_entry_during_eviction() {
        let mut cache = ShellIconCache::new();
        for index in 0..MAX_SHELL_ICON_CACHE_ENTRIES {
            cache.insert(format!("target-{index}"), Some(vec![index as u8; 4]));
        }
        assert!(cache.get("target-0").is_some());
        cache.insert(String::from("new-target"), None);
        assert!(cache.get("target-0").is_some());
        assert!(cache.get("target-1").is_none());
        assert!(cache.get("new-target").is_some_and(|icon| icon.is_none()));
    }

    #[test]
    fn shell_icon_cache_retains_negative_results_without_unbounded_growth() {
        let mut cache = ShellIconCache::new();
        cache.insert(String::from("missing-target"), None);
        assert!(cache
            .get("missing-target")
            .is_some_and(|icon| icon.is_none()));
    }

    #[test]
    fn icon_generation_changes_only_after_completion() {
        assert!(!icon_completion_generation_changed(4, 4));
        assert!(icon_completion_generation_changed(4, 5));
        assert!(icon_completion_generation_changed(u64::MAX, 0));
    }
}
