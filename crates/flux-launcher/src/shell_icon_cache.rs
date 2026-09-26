use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
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

/// File types whose icon belongs to the file itself rather than to its
/// extension: PE binaries carry icon resources, and shortcuts name their own
/// icon location. Everything else - a video, a document, an archive - shows the
/// icon of whatever Windows opens it with, so all rows of that extension share
/// one image.
const PER_FILE_ICON_EXTENSIONS: &[&str] = &[
    "exe",
    "dll",
    "ocx",
    "sys",
    "drv",
    "cpl",
    "msc",
    "tlb",
    "efi",
    "lnk",
    "url",
    "appref-ms",
];

/// Whether one shell extraction can serve several targets.
///
/// A page of sixteen `.mp4` files used to cost sixteen serial round trips into
/// the shell for the same picture, which is the load-in the owner watches while
/// he types. The class is decided on the worker thread, where asking the file
/// system costs nothing: on a row build the same question would run on the UI
/// thread for every row, and a folder named `v1.0` would otherwise be classed as
/// a `.0` file and lose its folder icon.
enum IconClass {
    /// An icon shared by every file of this extension.
    Extension(String),
    /// An icon that belongs to this one target.
    Single,
}

fn icon_class(target: &str) -> IconClass {
    let trimmed = target.trim().trim_matches('"');
    // Only filesystem paths have an extension association to share. A launcher
    // URI like `ms-settings:network-status` would otherwise parse as a `.status`
    // file and every Settings row on the page would be handed one icon.
    let is_path = trimmed.contains(['\\', '/']) || matches!(trimmed.as_bytes().get(1), Some(b':'));
    if !is_path {
        return IconClass::Single;
    }
    let path = std::path::Path::new(trimmed);
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return IconClass::Single;
    };
    let lowered = extension.to_ascii_lowercase();
    if PER_FILE_ICON_EXTENSIONS.contains(&lowered.as_str()) || path.is_dir() {
        return IconClass::Single;
    }
    IconClass::Extension(lowered)
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
    /// Requests handed to the thread but not finished yet. The result list waits
    /// for this to reach zero so a page of icons arrives in one repaint instead of
    /// one full-window repaint per completed icon.
    in_flight: Arc<AtomicUsize>,
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
        let in_flight = Arc::new(AtomicUsize::new(0));
        let in_flight_for_worker = Arc::clone(&in_flight);
        let (wake, receiver) = mpsc::sync_channel::<String>(64);
        thread::Builder::new()
            .name(String::from("flux-shell-icons"))
            .spawn(move || {
                #[cfg(windows)]
                let owns_com_apartment = initialize_shell_icon_worker_com();

                while let Ok(target) = receiver.recv() {
                    // A queued target can already have been served by a sibling of the
                    // same extension while it waited, so an item that is no longer
                    // pending is dropped here instead of paying the shell twice.
                    if !take_pending(&pending_for_worker, &target) {
                        continue;
                    }
                    #[cfg(windows)]
                    let image = shell_icon_rgba(&target);
                    #[cfg(not(windows))]
                    let image = None;
                    settle_icon(&in_flight_for_worker, &target, true);
                    // A failure is never spread: one unreadable file would otherwise
                    // blank every row of its extension.
                    let Some(image) = image else { continue };
                    for sibling in take_pending_siblings(&pending_for_worker, &target) {
                        cache_icon(&sibling, &image);
                        settle_icon(&in_flight_for_worker, &sibling, false);
                    }
                }

                #[cfg(windows)]
                if owns_com_apartment {
                    unsafe { windows::Win32::System::Com::CoUninitialize() };
                }
            })
            .expect("failed to create shell icon worker thread");
        Self {
            pending,
            in_flight,
            wake,
        }
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
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        if self.wake.try_send(target.clone()).is_err() {
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&target);
            }
        }
    }
}

/// Takes the job out of the queue, false when somebody already settled it.
fn take_pending(pending: &Mutex<HashSet<String>>, target: &str) -> bool {
    pending
        .lock()
        .map(|mut pending| pending.remove(target))
        .unwrap_or(false)
}

/// The queued targets whose icon is the one that was just extracted, removed from
/// the queue in the same pass so no sibling can be claimed twice.
fn take_pending_siblings(pending: &Mutex<HashSet<String>>, target: &str) -> Vec<String> {
    let IconClass::Extension(extension) = icon_class(target) else {
        return Vec::new();
    };
    let mut siblings = Vec::new();
    if let Ok(mut pending) = pending.lock() {
        pending.retain(|queued| {
            let shares =
                matches!(icon_class(queued), IconClass::Extension(other) if other == extension);
            if shares {
                siblings.push(queued.clone());
            }
            !shares
        });
    }
    siblings
}

fn settle_icon(in_flight: &AtomicUsize, target: &str, extracted: bool) {
    in_flight.fetch_sub(1, Ordering::AcqRel);
    let generation = SHELL_ICON_COMPLETION_GENERATION.fetch_add(1, Ordering::Release) + 1;
    crate::paint_trace::note(
        "icon-loaded",
        &format!(
            "generation={generation} extracted={} target={target}",
            extracted as u8
        ),
    );
}

#[cfg(windows)]
fn cache_icon(target: &str, image: &[u8]) {
    let cache = SHELL_ICON_CACHE.get_or_init(|| Mutex::new(ShellIconCache::new()));
    if let Ok(mut cache) = cache.lock() {
        cache.insert(target.to_owned(), Some(image.to_vec()));
    }
}

#[cfg(not(windows))]
fn cache_icon(_target: &str, _image: &[u8]) {}

static SHELL_ICON_WORKER: OnceLock<ShellIconWorker> = OnceLock::new();
pub(crate) fn shell_icon_worker() -> &'static ShellIconWorker {
    SHELL_ICON_WORKER.get_or_init(ShellIconWorker::spawn)
}

/// True while the icon thread still owes results for the rows on screen.
pub(crate) fn shell_icons_in_flight() -> bool {
    shell_icon_worker().in_flight.load(Ordering::Acquire) > 0
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
    // Exercise the same resolution chain the rows use, and require visible
    // pixels: a correctly sized but fully transparent buffer once passed this
    // smoke while the icon was invisible on screen.
    shell_icon_rgba(target).is_some_and(|rgba| {
        rgba.len() == 32 * 32 * 4 && rgba.chunks_exact(4).any(|pixel| pixel[3] != 0)
    })
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

    #[test]
    fn files_of_one_extension_share_an_icon_while_apps_and_uris_do_not() {
        assert!(matches!(
            icon_class(r"D:\Music\track.mp4"),
            IconClass::Extension(extension) if extension == "mp4"
        ));
        assert!(matches!(
            icon_class(r"D:\Music\TRACK.MP4"),
            IconClass::Extension(extension) if extension == "mp4"
        ));
        for own_icon in [
            r"C:\Apps\chrome.exe",
            r"C:\Start Menu\Counter-Strike 2.url",
            r"C:\Windows\System32\devmgmt.msc",
            "ms-settings:network-status",
            "notepad",
            r"D:\Music\playlist",
        ] {
            assert!(
                matches!(icon_class(own_icon), IconClass::Single),
                "{own_icon} must not share an icon with anything else"
            );
        }
    }

    #[test]
    fn a_directory_keeps_its_own_icon_even_when_its_name_holds_a_dot() {
        // The class is decided where the file system can be asked, so the case that
        // matters is a real folder whose last component looks like `.0`.
        let root = std::env::temp_dir().join(format!("flux-icon-class-{}", std::process::id()));
        let dotted = root.join("release-v1.0");
        std::fs::create_dir_all(&dotted).expect("the temporary folder is created");
        let verdict = icon_class(&dotted.to_string_lossy());
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            matches!(verdict, IconClass::Single),
            "a folder named `release-v1.0` is not a `.0` file"
        );
    }

    #[test]
    fn one_extraction_settles_every_queued_file_of_the_same_extension() {
        let pending = Mutex::new(HashSet::from([
            String::from(r"D:\Music\b.mp4"),
            String::from(r"D:\Videos\c.mkv"),
            String::from(r"C:\Apps\chrome.exe"),
        ]));
        let siblings = take_pending_siblings(&pending, r"D:\Music\a.MP4");
        assert_eq!(siblings, vec![String::from(r"D:\Music\b.mp4")]);
        let left = pending.lock().expect("the queue lock is never poisoned");
        assert_eq!(left.len(), 2, "the sibling left the queue");
        assert!(!left.contains(r"D:\Music\b.mp4"));
    }
}
