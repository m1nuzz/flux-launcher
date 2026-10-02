use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    mpsc::{self, SyncSender},
    Arc, Mutex, OnceLock, Weak,
};
use std::thread;

#[cfg(windows)]
use super::shell_icon_extract::{
    extract_icon_rgba_from_source, extract_shell_icon_rgba, extract_shell_thumbnail_rgba,
    is_executable_icon_target, shortcut_icon_location,
};

/// How many icon classes the class table holds at once. A session sees a few dozen
/// extensions, and every one of them pins the blob it was given, so the table is
/// bounded rather than grown: a class that falls out of it is paid for again, which
/// is exactly what every row cost before the table existed.
const MAX_ICON_CLASS_TABLE_ENTRIES: usize = 64;

/// The picture the shell last gave for each icon class, so a class is paid for once
/// rather than once per publish that happens to show it.
///
/// The sibling sweep in `take_pending_siblings` can only serve the rows that happen
/// to be queued at the instant an extraction finishes. A page is not published in
/// one go: the short provider answer lands first, then the complete set does, and
/// the second publish asks for rows nobody had queued yet. A page of four `.pyi`
/// rows split that way paid the shell twice for one picture, which is what the
/// paint trace recorded as `extracted=1` for two different targets of one extension
/// inside a single keystroke window.
struct IconClasses {
    entries: HashMap<String, Arc<Vec<u8>>>,
    order: VecDeque<String>,
}

impl IconClasses {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, extension: &str) -> Option<Arc<Vec<u8>>> {
        let picture = self.entries.get(extension)?.clone();
        self.touch(extension);
        Some(picture)
    }

    fn remember(&mut self, extension: &str, picture: Arc<Vec<u8>>) {
        self.entries.insert(extension.to_owned(), picture);
        self.touch(extension);
        while self.entries.len() > MAX_ICON_CLASS_TABLE_ENTRIES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }

    fn touch(&mut self, extension: &str) {
        if let Some(position) = self.order.iter().position(|key| key == extension) {
            self.order.remove(position);
        }
        self.order.push_back(extension.to_owned());
    }
}

/// What one row's icon cost the shell.
enum IconPurchase {
    /// The shell was asked. The answer may still be nothing: a failure is never
    /// spread to the other rows of the same class.
    Paid(Option<Arc<Vec<u8>>>),
    /// The class table already held this class's picture.
    Reused(Arc<Vec<u8>>),
}

/// The picture for one row, from the class table when it is there and from the shell
/// otherwise. `extract` is the shell round trip, so the number of times it runs is
/// the number the paint trace reports as an extraction.
fn purchase_icon(
    target: &str,
    class: &IconClass,
    classes: &mut IconClasses,
    extract: &mut impl FnMut(&str) -> Option<Arc<Vec<u8>>>,
) -> IconPurchase {
    if let IconClass::Extension(extension) = class {
        if let Some(picture) = classes.get(extension) {
            return IconPurchase::Reused(picture);
        }
    }
    let picture = extract(target);
    if let (IconClass::Extension(extension), Some(picture)) = (class, picture.as_ref()) {
        classes.remember(extension, Arc::clone(picture));
    }
    IconPurchase::Paid(picture)
}

/// Milliseconds since the first icon was ever asked for, so the warm-up can tell a
/// pause from a keystroke stream without reading the UI thread's state.
fn icon_clock_ms() -> u64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

/// How long the last row request has to age before the catalog may be warmed. A
/// warm icon costs the same shell round trip a page of rows is waiting for, and one
/// thread cannot serve both: warming in a pause is free, while warming between two
/// keystrokes delayed a page's icons by 189 ms.
const WARM_QUIET_MS: u64 = 250;

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
    "ico",
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
    // `is_dir`, not `!is_file`: a stat that fails - a path Everything still
    // indexes after deletion - is not a folder, so it shares its extension's
    // picture instead of paying a shell round trip for a ghost. Only a folder
    // that really is one keeps its own icon: a folder named `v1.0` would
    // otherwise be bound to a file picture for the rest of the session.
    if PER_FILE_ICON_EXTENSIONS.contains(&lowered.as_str()) || path.is_dir() {
        return IconClass::Single;
    }
    IconClass::Extension(lowered)
}

/// Four kilobytes per decoded 32x32 icon, so the whole table costs about four
/// megabytes: enough to hold the application catalog, which is what the idle
/// warm-up fills. The warm-up stops here rather than evicting its own best work.
///
/// Not gated on Windows: the catalog reads it to size its warm list, and the
/// catalog is built on every target.
pub(crate) const MAX_SHELL_ICON_CACHE_ENTRIES: usize = 1024;

#[cfg(windows)]
struct ShellIconCache {
    /// One entry per target, but targets that resolve to the same picture share a
    /// single blob. The catalog holds hundreds of shortcuts and executables that
    /// resolve to a few hundred distinct icons, so a copy per target paid for the
    /// same pixels again and again.
    entries: HashMap<String, Option<Arc<Vec<u8>>>>,
    lru_order: VecDeque<String>,
    /// Content hash to the live blobs carrying it, weakly held: `entries` owns the
    /// blobs, so evicting the last target pointing at one frees it and the weak
    /// handle here is simply left dead. Several blobs can share a hash, so the
    /// caller still compares the bytes before reusing one.
    blobs: HashMap<u64, Vec<Weak<Vec<u8>>>>,
}

/// FNV-1a, only ever used to pick candidates that are then compared byte for byte.
fn icon_content_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(windows)]
impl ShellIconCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            lru_order: VecDeque::new(),
            blobs: HashMap::new(),
        }
    }

    /// Returns the blob already held for these bytes, or stores them as a new one.
    fn intern(&mut self, bytes: Vec<u8>) -> Arc<Vec<u8>> {
        let hash = icon_content_hash(&bytes);
        let reused = self.blobs.get(&hash).and_then(|candidates| {
            candidates
                .iter()
                .filter_map(Weak::upgrade)
                .find(|blob| blob.as_slice() == bytes.as_slice())
        });
        if let Some(blob) = reused {
            return blob;
        }
        // A miss is rare once the catalog is warm, so dropping the handles whose
        // blob has already been evicted costs less than the memory they pin.
        self.blobs.retain(|_, candidates| {
            candidates.retain(|blob| blob.strong_count() > 0);
            !candidates.is_empty()
        });
        let blob = Arc::new(bytes);
        self.blobs
            .entry(hash)
            .or_default()
            .push(Arc::downgrade(&blob));
        blob
    }

    fn get(&mut self, target: &str) -> Option<Option<Vec<u8>>> {
        let icon = self.entries.get(target).cloned()?;
        self.touch(target);
        Some(icon.map(|blob| (*blob).clone()))
    }

    /// The blob held for a target, without copying the pixels out of it.
    fn get_shared(&mut self, target: &str) -> Option<Option<Arc<Vec<u8>>>> {
        let icon = self.entries.get(target).cloned()?;
        self.touch(target);
        Some(icon)
    }

    /// Stores the bytes a target resolved to and hands back the blob they were
    /// interned into, so a caller that needs the shared handle gets the pixels the
    /// entry holds rather than a second copy of them.
    fn insert(&mut self, target: String, icon: Option<Vec<u8>>) -> Option<Arc<Vec<u8>>> {
        // Only a found icon is shared. "This target has no icon" is a fact about
        // that one target, so a negative result stays its own entry and keeps
        // reading back as cached.
        let icon = icon.map(|bytes| self.intern(bytes));
        self.insert_shared(target, icon.as_ref().map(Arc::clone));
        icon
    }

    /// The same entry for a picture that is already interned, which is how a class
    /// the table already paid for reaches a target under its own name: the same
    /// blob, copied nothing.
    fn insert_shared(&mut self, target: String, icon: Option<Arc<Vec<u8>>>) {
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

/// One unit of icon work.
///
/// A row request is what the drain gate waits for. A warm request is best effort
/// and never joins the queue, so a full channel or a dropped send cannot leave a
/// visible row waiting for an icon that will never arrive.
enum IconJob {
    Row(String),
    Warm(String),
}

pub(crate) struct ShellIconWorker {
    /// Row targets the icon thread still owes.
    pending: Arc<Mutex<HashSet<String>>>,
    /// Row requests a visible row is waiting for. The result list waits for this to
    /// reach zero so a page of icons arrives in one repaint instead of one
    /// full-window repaint per completed icon.
    in_flight: Arc<AtomicUsize>,
    /// Warm jobs the thread has not finished. The warm-up keeps this at one so a
    /// row never waits behind the catalog, and so the bounded channel cannot drop
    /// the rest of it.
    warm_outstanding: Arc<AtomicUsize>,
    /// When a visible row last asked for an icon, on the icon clock.
    last_row_request: Arc<AtomicU64>,
    wake: SyncSender<IconJob>,
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

/// Asks the shell for one icon, filling the cache on the way. The bytes come back
/// as the interned blob, so a row served from the class table stores the same
/// pixels the shell returned rather than a copy of them.
#[cfg(windows)]
fn extract_icon(target: &str) -> Option<Arc<Vec<u8>>> {
    shell_icon_shared(target)
}

#[cfg(not(windows))]
fn extract_icon(_target: &str) -> Option<Arc<Vec<u8>>> {
    None
}

impl ShellIconWorker {
    fn spawn() -> Self {
        let pending = Arc::new(Mutex::new(HashSet::<String>::new()));
        let pending_for_worker = Arc::clone(&pending);
        let in_flight = Arc::new(AtomicUsize::new(0));
        let in_flight_for_worker = Arc::clone(&in_flight);
        let warm_outstanding = Arc::new(AtomicUsize::new(0));
        let last_row_request = Arc::new(AtomicU64::new(0));
        let warm_for_worker = Arc::clone(&warm_outstanding);
        let (wake, receiver) = mpsc::sync_channel::<IconJob>(64);
        thread::Builder::new()
            .name(String::from("flux-shell-icons"))
            .spawn(move || {
                #[cfg(windows)]
                let owns_com_apartment = initialize_shell_icon_worker_com();
                // Read and written only between two `recv()` calls, so the class
                // table needs no lock: it is this thread's own memory.
                let mut classes = IconClasses::new();

                while let Ok(job) = receiver.recv() {
                    match job {
                        IconJob::Row(target) => {
                            serve_row_job(
                                &target,
                                &pending_for_worker,
                                &in_flight_for_worker,
                                &mut classes,
                                &mut |target| extract_icon(target),
                            );
                        }
                        // The catalog is loaded one icon per quiet moment and never tells
                        // the tree: the next row build finds the picture in the cache.
                        IconJob::Warm(target) => {
                            // A warm job fills the cache and nothing else. It must not
                            // close a row's own request: that would move the completion
                            // generation from inside the warm pass and repaint the page a
                            // second time under a keystroke that was waiting for one.
                            if !icon_is_cached(&target) {
                                extract_icon(&target);
                                settle_icon(&in_flight_for_worker, &target, false, true);
                            }
                            warm_for_worker.fetch_sub(1, Ordering::AcqRel);
                        }
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
            warm_outstanding,
            last_row_request,
            wake,
        }
    }

    /// Queues a target a visible row is waiting for.
    fn request(&self, target: String) {
        self.last_row_request
            .store(icon_clock_ms(), Ordering::Release);
        let fresh = self
            .pending
            .lock()
            .map(|mut pending| pending.insert(target.clone()))
            .unwrap_or(false);
        if !fresh {
            return;
        }
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        if self.wake.try_send(IconJob::Row(target.clone())).is_err() {
            // The row asks again on its next build. Give the slot back only while we
            // still own it: a sibling of the same icon class can settle this target
            // between the increment and the failed send, and subtracting twice would
            // wrap the counter and stall the drain gate for the rest of the session.
            let owned = self
                .pending
                .lock()
                .map(|mut pending| pending.remove(&target))
                .unwrap_or(false);
            if owned {
                release_icon_slot(&self.in_flight);
            }
        }
    }

    /// Feeds the catalog into the same thread while the launcher is idle, so the
    /// first query of a session does not watch a page of icons arrive one row at a
    /// time. Warm targets never enter `pending`, which is what keeps the drain gate
    /// from waiting on the catalog: that stall is how held-back icons used to cost
    /// 207-771 ms.
    fn warm(&self, targets: Vec<String>) {
        let pending = Arc::clone(&self.pending);
        let outstanding = Arc::clone(&self.warm_outstanding);
        let sender = self.wake.clone();
        let last_row_request = Arc::clone(&self.last_row_request);
        let _ = thread::Builder::new()
            .name(String::from("flux-icon-warm"))
            .spawn(move || {
                for target in targets {
                    if icon_is_cached(&target) {
                        continue;
                    }
                    // Only in a real pause, or the catalog competes with the page the
                    // user is looking at: see `WARM_QUIET_MS`.
                    while icon_clock_ms().saturating_sub(last_row_request.load(Ordering::Acquire))
                        < WARM_QUIET_MS
                    {
                        thread::sleep(std::time::Duration::from_millis(25));
                    }
                    // One warm job in front of a row, and never a queue of them:
                    // the channel is bounded, and a send it cannot hold is dropped.
                    while outstanding.load(Ordering::Acquire) > 0
                        || pending
                            .lock()
                            .map(|pending| !pending.is_empty())
                            .unwrap_or(false)
                    {
                        thread::sleep(std::time::Duration::from_millis(25));
                    }
                    outstanding.fetch_add(1, Ordering::AcqRel);
                    if sender.try_send(IconJob::Warm(target)).is_err() {
                        outstanding.fetch_sub(1, Ordering::AcqRel);
                        thread::sleep(std::time::Duration::from_millis(25));
                    }
                }
            });
    }
}

/// Gives back one slot of the drain gate without ever wrapping below zero.
fn release_icon_slot(in_flight: &AtomicUsize) {
    let _ = in_flight.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
        value.checked_sub(1)
    });
}

/// Takes a row job out of the queue, false when a sibling already settled it.
fn take_pending(pending: &Mutex<HashSet<String>>, target: &str) -> bool {
    pending
        .lock()
        .map(|mut pending| pending.remove(target))
        .unwrap_or(false)
}

/// One queued row: its own icon, the siblings that come free with it, and both
/// halves of the drain gate. Every row job on the icon thread goes through here, so
/// the class table, the sibling sweep and the trace stay one code path.
fn serve_row_job(
    target: &str,
    pending: &Mutex<HashSet<String>>,
    in_flight: &AtomicUsize,
    classes: &mut IconClasses,
    extract: &mut impl FnMut(&str) -> Option<Arc<Vec<u8>>>,
) {
    // The target may already have been served by a sibling of the same extension
    // while it waited in the queue.
    if !take_pending(pending, target) {
        return;
    }
    let class = icon_class(target);
    let (picture, paid_shell) = match purchase_icon(target, &class, classes, &mut *extract) {
        IconPurchase::Paid(picture) => (picture, true),
        IconPurchase::Reused(picture) => {
            // The class table served this row, so nothing has put the picture
            // under its own name in the cache. Without that entry every rebuild of
            // the row would queue another job, move the completion generation
            // again, and repaint a page of icons it already has.
            cache_shared(target, &picture);
            (Some(picture), false)
        }
    };
    settle_icon(in_flight, target, true, paid_shell);
    // A failure is never spread: one unreadable file would otherwise blank every
    // row of its extension.
    let Some(picture) = picture else { return };
    for sibling in take_pending_siblings(pending, &class) {
        cache_shared(&sibling, &picture);
        settle_icon(in_flight, &sibling, true, false);
    }
}

/// The queued row targets whose icon is the one just extracted, removed from the
/// queue in the same pass so no sibling can be claimed twice.
///
/// The queue is snapshotted and classified BEFORE the lock is taken. `icon_class`
/// states a path, and a stat on a sleeping or absent volume can take tens of
/// milliseconds; spending that with the queue locked blocks every other row request,
/// and it is paid per queued entry, so a page of sixteen rows pays it sixteen times.
/// The classification is the same either way - only *when* the stat happens moves.
fn take_pending_siblings(pending: &Mutex<HashSet<String>>, class: &IconClass) -> Vec<String> {
    let IconClass::Extension(extension) = class else {
        return Vec::new();
    };
    let extension = extension.as_str();
    let Ok(queue) = pending.lock() else {
        return Vec::new();
    };
    let queued: Vec<String> = queue.iter().cloned().collect();
    drop(queue);
    let mut siblings: Vec<String> = queued
        .into_iter()
        .filter(|candidate| {
            matches!(icon_class(candidate), IconClass::Extension(other) if other == extension)
        })
        .collect();
    if siblings.is_empty() {
        return siblings;
    }
    match pending.lock() {
        Ok(mut pending) => {
            // A sibling another thread settled in between is simply not ours to hand
            // out; it keeps its slot and asks again on its next build.
            siblings.retain(|sibling| pending.remove(sibling));
        }
        Err(_) => siblings.clear(),
    }
    siblings
}

/// Reports a ready icon. Only a target a row waits on moves the drain gate or the
/// completion generation: a warm icon is simply there when the next row is built.
fn settle_icon(in_flight: &AtomicUsize, target: &str, awaited: bool, extracted: bool) {
    let generation = if awaited {
        release_icon_slot(in_flight);
        SHELL_ICON_COMPLETION_GENERATION.fetch_add(1, Ordering::Release) + 1
    } else {
        SHELL_ICON_COMPLETION_GENERATION.load(Ordering::Acquire)
    };
    crate::paint_trace::note(
        "icon-loaded",
        &format!(
            "generation={generation} extracted={} awaited={} target={target}",
            extracted as u8, awaited as u8
        ),
    );
}

#[cfg(windows)]
fn icon_is_cached(target: &str) -> bool {
    shell_icon_cache_lookup(target).is_some()
}

#[cfg(not(windows))]
fn icon_is_cached(_target: &str) -> bool {
    true
}

/// Records a picture for a target under its own name. The bytes are already
/// interned - the shell returned them, or a sibling's extraction did - so the
/// entry points at the same blob instead of a copy of it.
#[cfg(windows)]
fn cache_shared(target: &str, picture: &Arc<Vec<u8>>) {
    let cache = SHELL_ICON_CACHE.get_or_init(|| Mutex::new(ShellIconCache::new()));
    if let Ok(mut cache) = cache.lock() {
        cache.insert_shared(target.to_owned(), Some(Arc::clone(picture)));
    }
}

#[cfg(not(windows))]
fn cache_shared(_target: &str, _picture: &Arc<Vec<u8>>) {}
static SHELL_ICON_WORKER: OnceLock<ShellIconWorker> = OnceLock::new();
pub(crate) fn shell_icon_worker() -> &'static ShellIconWorker {
    SHELL_ICON_WORKER.get_or_init(ShellIconWorker::spawn)
}

/// True while the icon thread still owes results for the rows on screen.
pub(crate) fn shell_icons_in_flight() -> bool {
    shell_icon_worker().in_flight.load(Ordering::Acquire) > 0
}

/// Loads the icons of the application catalog while the launcher is idle, so the
/// first query of a session does not watch them arrive one row at a time.
pub(crate) fn warm_shell_icons(targets: Vec<String>) {
    shell_icon_worker().warm(targets);
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
    shell_icon_shared(target).map(|blob| (*blob).clone())
}

#[cfg(not(windows))]
pub(crate) fn shell_icon_rgba(_target: &str) -> Option<Vec<u8>> {
    None
}

/// The icon of one target, as the interned blob every target resolving to the same
/// picture shares. The row path takes it as it is, so a row served from the class
/// table stores the pixels the shell returned rather than a second copy of them.
#[cfg(windows)]
pub(crate) fn shell_icon_shared(target: &str) -> Option<Arc<Vec<u8>>> {
    let cache = SHELL_ICON_CACHE.get_or_init(|| Mutex::new(ShellIconCache::new()));
    if let Ok(mut cache) = cache.lock() {
        if let Some(icon) = cache.get_shared(target) {
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
    // A poisoned lock costs the sharing, not the picture: the row still gets an
    // icon, and the next extraction interns its own copy of the same bytes.
    let Ok(mut cache) = cache.lock() else {
        return icon.map(Arc::new);
    };
    cache.insert(target.to_owned(), icon)
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
    fn targets_that_resolve_to_one_picture_share_a_single_blob() {
        let mut cache = ShellIconCache::new();
        let picture = vec![7u8; 32 * 32 * 4];
        cache.insert(String::from("target-a"), Some(picture.clone()));
        cache.insert(String::from("target-b"), Some(picture.clone()));
        cache.insert(String::from("target-c"), Some(picture));

        let live: Vec<Arc<Vec<u8>>> = cache
            .entries
            .values()
            .filter_map(|icon| icon.as_ref().map(Arc::clone))
            .collect();
        assert_eq!(live.len(), 3, "every target still has its icon");
        assert!(
            live.windows(2).all(|pair| Arc::ptr_eq(&pair[0], &pair[1])),
            "three targets, one picture, must not hold three copies"
        );
        assert_eq!(cache.blobs.values().map(Vec::len).sum::<usize>(), 1);
    }

    #[test]
    fn different_pictures_are_never_shared() {
        let mut cache = ShellIconCache::new();
        cache.insert(String::from("target-a"), Some(vec![1u8; 32 * 32 * 4]));
        cache.insert(String::from("target-b"), Some(vec![2u8; 32 * 32 * 4]));

        let live: Vec<Arc<Vec<u8>>> = cache
            .entries
            .values()
            .filter_map(|icon| icon.as_ref().map(Arc::clone))
            .collect();
        assert_eq!(live.len(), 2);
        assert!(!Arc::ptr_eq(&live[0], &live[1]));
        assert_ne!(live[0][0], live[1][0]);
    }

    #[test]
    fn a_shared_blob_outlives_the_eviction_of_one_of_its_targets() {
        let mut cache = ShellIconCache::new();
        let picture = vec![9u8; 32 * 32 * 4];
        for index in 0..=MAX_SHELL_ICON_CACHE_ENTRIES {
            cache.insert(format!("target-{index}"), Some(picture.clone()));
        }
        // The first target was pushed out of the table, so its blob may not be the
        // one a later target still points at.
        assert_eq!(cache.entries.len(), MAX_SHELL_ICON_CACHE_ENTRIES);
        assert!(!cache.entries.contains_key("target-0"));
        assert_eq!(cache.blobs.values().map(Vec::len).sum::<usize>(), 1);
        assert!(cache
            .get(&format!("target-{MAX_SHELL_ICON_CACHE_ENTRIES}"))
            .is_some_and(|icon| icon.is_some()));
    }

    #[test]
    fn a_negative_result_stays_its_own_entry() {
        let mut cache = ShellIconCache::new();
        cache.insert(String::from("no-icon-a"), None);
        cache.insert(String::from("no-icon-b"), None);

        assert!(
            cache.blobs.is_empty(),
            "nothing to share for a missing icon"
        );
        assert!(cache.get("no-icon-a").is_some_and(|icon| icon.is_none()));
        assert!(cache.get("no-icon-b").is_some_and(|icon| icon.is_none()));
    }

    #[test]
    fn icon_generation_changes_only_after_completion() {
        assert!(!icon_completion_generation_changed(4, 4));
        assert!(icon_completion_generation_changed(4, 5));
        assert!(icon_completion_generation_changed(u64::MAX, 0));
    }

    #[test]
    fn files_of_one_extension_share_an_icon_while_apps_and_uris_do_not() {
        // The class is decided by asking the file system, so the shared case needs
        // real files: a path that cannot be stated as a file keeps its own icon.
        let root = std::env::temp_dir().join(format!("flux-icon-class-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("the temporary folder is created");
        for name in ["track.mp4", "TRACK.MP4"] {
            let path = root.join(name);
            std::fs::write(&path, b"").expect("the temporary file is created");
            assert!(
                matches!(
                    icon_class(&path.to_string_lossy()),
                    IconClass::Extension(extension) if extension == "mp4"
                ),
                "{} must share the .mp4 picture",
                path.display()
            );
        }
        let _ = std::fs::remove_dir_all(&root);
        for own_icon in [
            r"C:\Apps\chrome.exe",
            r"C:\Icons\player.ico",
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
        let root = std::env::temp_dir().join(format!("flux-icon-dir-{}", std::process::id()));
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
    fn a_path_that_is_gone_shares_its_extension_instead_of_paying_for_a_ghost() {
        // Everything keeps indexing a temp file after its owner deletes it. That
        // row is not a folder - nothing on disk answers to the name anymore - so
        // it takes its extension's picture instead of a shell round trip that can
        // only fail.
        let missing =
            std::env::temp_dir().join(format!("flux-icon-ghost-{}.tmp", std::process::id()));
        assert!(
            !missing.exists(),
            "the fixture must be absent for the verdict to mean anything"
        );
        assert!(
            matches!(
                icon_class(&missing.to_string_lossy()),
                IconClass::Extension(extension) if extension == "tmp"
            ),
            "a deleted file is not a folder and shares the type picture"
        );
    }

    #[test]
    fn one_extraction_settles_every_queued_file_of_the_same_extension() {
        let root = std::env::temp_dir().join(format!("flux-icon-siblings-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("the temporary folder is created");
        let paths: Vec<String> = ["a.mp4", "b.mp4", "c.mkv", "chrome.exe"]
            .iter()
            .map(|name| {
                let path = root.join(name);
                std::fs::write(&path, b"").expect("the temporary file is created");
                path.to_string_lossy().into_owned()
            })
            .collect();
        let pending = Mutex::new(HashSet::from_iter(paths[1..].iter().cloned()));

        let siblings = take_pending_siblings(&pending, &icon_class(&paths[0]));

        assert_eq!(siblings, vec![paths[1].clone()]);
        let left = pending.lock().expect("the queue lock is never poisoned");
        assert_eq!(left.len(), 2, "only the sibling left the queue");
        assert!(!left.contains(&paths[1]));
        drop(left);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The negative control for the flicker gate's `paid the shell N times for M rows
    /// that share one icon`: a page is not published in one go, so the rows of one
    /// class reach the icon thread in separate rounds with an empty queue between
    /// them - which is what the paint trace recorded as two `extracted=1` events for
    /// four `.pyi` rows inside one keystroke window.
    #[test]
    fn one_shell_round_trip_serves_a_class_whose_rows_arrive_in_separate_publishes() {
        let root = std::env::temp_dir().join(format!("flux-icon-rounds-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("the temporary folder is created");
        let paths: Vec<String> = ["a.pyi", "b.pyi", "c.pyi", "d.pyi"]
            .iter()
            .map(|name| {
                let path = root.join(name);
                std::fs::write(&path, b"").expect("the temporary file is created");
                path.to_string_lossy().into_owned()
            })
            .collect();

        let in_flight = AtomicUsize::new(0);
        let mut classes = IconClasses::new();
        let mut extractions = 0usize;
        let mut extract = |_target: &str| {
            extractions += 1;
            Some(Arc::new(vec![5u8; 32 * 32 * 4]))
        };
        for path in &paths {
            // One publish: the row asks, the icon thread serves it, and the next
            // keystroke's page replaces the queue before this row's extraction is
            // even finished, so the sibling sweep has nothing left to serve.
            let pending = Mutex::new(HashSet::from_iter([path.clone()]));
            in_flight.store(1, Ordering::Release);
            serve_row_job(path, &pending, &in_flight, &mut classes, &mut extract);
            assert_eq!(
                in_flight.load(Ordering::Acquire),
                0,
                "{path}: the drain gate must close"
            );
        }

        assert_eq!(
            extractions, 1,
            "four .pyi rows in four publishes paid the shell {extractions} time(s) for one picture"
        );
        // Every row the class table served has to be cached under its own name, or the
        // next rebuild of that row queues another job and the page repaints again for
        // an icon it already has.
        for path in &paths[1..] {
            assert!(
                shell_icon_cache_lookup(path).is_some_and(|icon| icon.is_some()),
                "{path}: a row served from the class table must be cached by name"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The same promise when the rows arrive while the extraction is still inside the
    /// shell, which is the arrival order the row tree actually produces: the queue
    /// fills from the UI thread while the icon thread is busy.
    #[test]
    fn one_shell_round_trip_serves_a_class_that_arrives_during_an_extraction() {
        let root = std::env::temp_dir().join(format!("flux-icon-race-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("the temporary folder is created");
        let paths: Vec<String> = ["a.pyi", "b.pyi", "c.pyi", "d.pyi"]
            .iter()
            .map(|name| {
                let path = root.join(name);
                std::fs::write(&path, b"").expect("the temporary file is created");
                path.to_string_lossy().into_owned()
            })
            .collect();

        let in_flight = AtomicUsize::new(0);
        let pending = Mutex::new(HashSet::from_iter(paths.iter().cloned()));
        in_flight.store(paths.len(), Ordering::Release);
        let mut classes = IconClasses::new();
        let mut extractions = 0usize;
        let mut extract = |_target: &str| {
            extractions += 1;
            Some(Arc::new(vec![6u8; 32 * 32 * 4]))
        };
        serve_row_job(&paths[0], &pending, &in_flight, &mut classes, &mut extract);
        assert_eq!(
            in_flight.load(Ordering::Acquire),
            0,
            "the drain gate must close"
        );
        for path in &paths[1..] {
            assert!(
                shell_icon_cache_lookup(path).is_some_and(|icon| icon.is_some()),
                "{path}: every queued row of the class must be served"
            );
        }
        assert_eq!(
            extractions, 1,
            "the sibling sweep alone must serve the queue"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The class table must not over-share: a different extension that lands in the
    /// same queue, and a target that owns its picture, are still paid for one by one.
    #[test]
    fn a_class_table_never_serves_one_class_picture_to_another() {
        let root = std::env::temp_dir().join(format!("flux-icon-classes-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("the temporary folder is created");
        let paths: Vec<String> = ["a.pyi", "b.mkv", "c.exe", "d.pyi"]
            .iter()
            .map(|name| {
                let path = root.join(name);
                std::fs::write(&path, b"").expect("the temporary file is created");
                path.to_string_lossy().into_owned()
            })
            .collect();

        let in_flight = AtomicUsize::new(0);
        let mut classes = IconClasses::new();
        let mut extractions = 0usize;
        let mut extract = |target: &str| {
            extractions += 1;
            Some(Arc::new(vec![target.len() as u8; 32 * 32 * 4]))
        };
        for path in &paths {
            let pending = Mutex::new(HashSet::from_iter([path.clone()]));
            in_flight.store(1, Ordering::Release);
            serve_row_job(path, &pending, &in_flight, &mut classes, &mut extract);
        }

        assert_eq!(
            extractions, 3,
            "only the second .pyi row is free: one extraction per distinct class, \
             and an .exe owns its picture"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
