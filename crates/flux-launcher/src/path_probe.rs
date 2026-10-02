//! Resolve the place a typed path names by asking the filesystem directly.
//!
//! Two facts make this worker necessary. Everything answers a path query with the
//! *contents* of that folder and never the folder itself - `es 'd:\vocal_ai'` returns
//! 82 261 rows in which `D:\vocal_ai` appears as an object zero times - and it answers
//! nothing at all once the query is wrapped in quotes. On top of that a removable
//! volume is commonly outside the index. So the row for the place the user named can
//! only come from the filesystem itself.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use flux_core::{normalized_path_key, path_query_body, SearchResult};
use windui::prelude::Sender;

use super::ui_constants::PATH_PROBE_PUBLISH_GATE_MS;

/// The probe and the publish gate share one number ([`PATH_PROBE_PUBLISH_GATE_MS`]):
/// this thread stops looking when the budget is spent, and the tick stops waiting at
/// the same moment. On an awake volume a path is a couple of `stat` calls, so the
/// budget only ever binds on a sleeping disk - where the call cannot be cancelled at
/// all, and the budget is what the rest of the list pays instead.
/// How long the probe may hold the list is a different question from how long the
/// user has been typing, so this is deliberately not `TYPING_QUIET_MS`.
fn probe_deadline() -> Instant {
    Instant::now() + Duration::from_millis(PATH_PROBE_PUBLISH_GATE_MS)
}
/// Rows one keystroke may add: the probe completes a name, it does not page a folder.
const MAX_ROWS: usize = 8;
/// The drive-less fallback walks for a folder the first step names. Bounded by depth
/// and by visited directories, because one stray `temp` folder under `C:\` is otherwise
/// the whole volume.
const MAX_WALK_DEPTH: usize = 4;
const MAX_WALKED_DIRECTORIES: usize = 1_200;
/// A listing larger than this is read for the query that asked and not kept: a hundred
/// thousand names are not worth the memory, and cutting the read short would hide the
/// folder being looked for, since a directory has no useful order.
const MAX_CACHED_CHILDREN: usize = 8_192;
const MAX_LISTINGS: usize = 256;
/// Nobody means to search here and every visit is answered with a denial, so the name
/// is skipped before the syscall instead of after it.
const NEVER_WALKED: [&str; 1] = ["System Volume Information"];

#[derive(Clone, Debug)]
pub struct PathProbeResponse {
    pub sequence: u64,
    pub query: String,
    pub results: Vec<SearchResult>,
}

#[derive(Clone, Debug)]
struct PathProbeRequest {
    sequence: u64,
    query: String,
}

/// What a name in a listing is, and whether the walk may go inside it. A reparse point
/// is a place worth naming but not a route: following junctions is how a bounded walk
/// becomes a cycle.
#[derive(Clone, Copy)]
enum Child {
    Item,
    Folder,
    Reparse,
}

impl Child {
    fn is_walkable(self) -> bool {
        matches!(self, Child::Folder)
    }
}

pub struct PathProbeWorker {
    latest: Arc<Mutex<Option<PathProbeRequest>>>,
    wake: SyncSender<()>,
}

impl PathProbeWorker {
    pub fn spawn(output: Sender<PathProbeResponse>) -> Self {
        let latest = Arc::new(Mutex::new(None::<PathProbeRequest>));
        let latest_for_worker = Arc::clone(&latest);
        let (wake, receiver) = mpsc::sync_channel::<()>(1);

        thread::Builder::new()
            .name(String::from("flux-path-probe"))
            .spawn(move || {
                // One thread, so the listings it reads can be kept for the rest of the
                // session: the next letter of a path costs one new folder, never a
                // re-read of every parent.
                let mut session = ProbeSession::default();
                while receiver.recv().is_ok() {
                    let Some(request) = latest_for_worker
                        .lock()
                        .ok()
                        .and_then(|mut slot| slot.take())
                    else {
                        continue;
                    };
                    let results = session.resolve(&request.query);
                    let _ = output.send(PathProbeResponse {
                        sequence: request.sequence,
                        query: request.query,
                        results,
                    });
                }
            })
            .expect("failed to create the path probe worker thread");

        Self { latest, wake }
    }

    pub fn request(&self, sequence: u64, query: String) {
        // Anything that does not name a place costs nothing: no thread wake-up, no
        // volume touched.
        if path_query_body(&query).is_none() {
            return;
        }
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(PathProbeRequest { sequence, query });
            let _ = self.wake.try_send(());
        }
    }
}

/// True when the typed path names a share rather than a volume this machine owns.
///
/// A `stat` on a dead share cannot be interrupted and holds this one thread for as long
/// as the redirector's own timeout, so the tick asks about these only after the query
/// has gone quiet: one answer for the text the user left on screen, instead of a queue
/// that cannot drain. Prefix completion is not something a share can be given either.
pub(crate) fn is_remote_path_query(query: &str) -> bool {
    path_query_body(query).is_some_and(|body| body.starts_with("\\\\"))
}

#[derive(Default)]
struct ProbeSession {
    /// Children already read, keyed by the comparison form of their folder.
    listings: HashMap<String, Vec<(String, Child)>>,
}

impl ProbeSession {
    fn resolve(&mut self, query: &str) -> Vec<SearchResult> {
        let deadline = probe_deadline();
        let Some(typed) = path_query_body(query) else {
            return Vec::new();
        };
        let mut results = Vec::new();
        if is_volume_qualified(&typed) {
            self.push_place(&mut results, &typed, deadline);
            return results;
        }
        let Some((anchor, rest)) = typed.split_once('\\') else {
            return results;
        };
        let roots = local_volume_roots();
        // First the chain straight off each volume root: `maxim\cmd` is `F:\maxim\cmd`
        // and `Users\m1nuzz\Downloads` is `C:\Users\m1nuzz\Downloads`, and two stats
        // answer either without reading a single listing. Every root is asked here,
        // because the same chain on two volumes really is two places.
        for root in &roots {
            if Instant::now() >= deadline {
                break;
            }
            self.push_place(&mut results, &format!("{root}{typed}"), deadline);
        }
        if !results.is_empty() {
            return results;
        }
        // Only a miss buys the walk, and the walk stops paying at the first volume that
        // answers: a drive-less chain names one place, not one relative folder per
        // volume.
        for root in &roots {
            for found in self.anchor_folders(root, anchor, deadline) {
                let before = results.len();
                self.push_place(&mut results, &joined(&found, rest), deadline);
                if results.len() > before {
                    return results;
                }
            }
        }
        results
    }

    fn push_place(&mut self, results: &mut Vec<SearchResult>, candidate: &str, deadline: Instant) {
        if results.len() >= MAX_ROWS {
            return;
        }
        let place = openable_path(candidate);
        if Path::new(&place).exists() {
            results.push(SearchResult::from_existing_path(&place));
            return;
        }
        // The last step is still being typed: complete it from the folder that does exist.
        let Some((parent, prefix)) = unfinished_step(&place) else {
            return;
        };
        if Instant::now() >= deadline {
            return;
        }
        let completing: Vec<String> = self
            .children(&parent, deadline)
            .into_iter()
            .filter(|(name, _)| name.to_lowercase().starts_with(&prefix))
            .map(|(name, _)| name)
            .take(MAX_ROWS.saturating_sub(results.len()))
            .collect();
        for name in completing {
            results.push(SearchResult::from_existing_path(&joined(
                &strip_root(&parent),
                &name,
            )));
        }
    }

    /// Every folder named `anchor` under `root`, breadth first, bounded by depth, by
    /// visited directories and by the budget. The root's own listing is the first
    /// level, so the usual case - the chain hangs off a volume root - is a cache read
    /// rather than a walk.
    fn anchor_folders(&mut self, root: &str, anchor: &str, deadline: Instant) -> Vec<String> {
        let mut found = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back((root.to_owned(), 0_usize));
        let mut visited = 0_usize;
        while let Some((directory, depth)) = queue.pop_front() {
            if depth >= MAX_WALK_DEPTH
                || visited >= MAX_WALKED_DIRECTORIES
                || Instant::now() >= deadline
            {
                break;
            }
            visited += 1;
            for (name, kind) in self.children(&directory, deadline) {
                if !kind.is_walkable() {
                    continue;
                }
                let child = joined(&directory, &name);
                if name.eq_ignore_ascii_case(anchor) && found.len() < MAX_ROWS {
                    found.push(child.clone());
                }
                if NEVER_WALKED
                    .iter()
                    .any(|skipped| name.eq_ignore_ascii_case(skipped))
                {
                    continue;
                }
                queue.push_back((child, depth + 1));
            }
        }
        found
    }

    fn children(&mut self, directory: &str, deadline: Instant) -> Vec<(String, Child)> {
        let key = normalized_path_key(directory);
        if let Some(known) = self.listings.get(&key) {
            return known.clone();
        }
        let (read, complete) = read_children(directory, deadline);
        // A read the budget abandoned says nothing about the names past its end, and
        // caching it would hide the very folder a later keystroke is looking for.
        if complete && read.len() <= MAX_CACHED_CHILDREN {
            if self.listings.len() >= MAX_LISTINGS {
                self.listings.clear();
            }
            self.listings.insert(key, read.clone());
        }
        read
    }
}

/// The folder's entries, and whether the list is the whole folder. `false` means the
/// budget ran out part-way and what came back is a prefix of an unordered enumeration.
fn read_children(directory: &str, deadline: Instant) -> (Vec<(String, Child)>, bool) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return (Vec::new(), true);
    };
    let mut children = Vec::new();
    for entry in entries.flatten() {
        // A huge folder is read through rather than cut short, but a read that outlives
        // the budget is abandoned instead of kept counting.
        if children.len() % 4_096 == 0 && Instant::now() >= deadline {
            return (children, false);
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let kind = if file_type.is_symlink() {
            Child::Reparse
        } else if file_type.is_dir() {
            Child::Folder
        } else {
            Child::Item
        };
        children.push((name, kind));
    }
    (children, true)
}

/// The folder and the half-typed name a path ends with, when the folder exists and the
/// name does not.
fn unfinished_step(place: &str) -> Option<(String, String)> {
    let path = Path::new(place);
    let name = path.file_name()?.to_str()?;
    if name.is_empty() {
        return None;
    }
    let parent = path.parent()?;
    let parent = parent.to_str()?;
    if parent.is_empty() || !Path::new(parent).exists() {
        return None;
    }
    Some((parent.to_owned(), name.to_lowercase()))
}

/// `X:\` plus a child is `X:\child`; every other folder joins with one separator.
fn joined(directory: &str, name: &str) -> String {
    if directory.ends_with('\\') {
        format!("{directory}{name}")
    } else {
        format!("{directory}\\{name}")
    }
}

fn strip_root(directory: &str) -> String {
    directory.trim_end_matches('\\').to_owned()
}

/// The form a stat reads as the place the user named: to Windows a bare `F:` is
/// relative to the current directory on that drive, so a drive root needs its
/// separator. The spelling is left exactly as it was typed, because a folder row has to
/// read like the path that produced it.
fn openable_path(candidate: &str) -> String {
    let mut characters = candidate.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    if first.is_ascii_alphabetic() && characters.as_str() == ":" {
        return format!("{first}:\\");
    }
    candidate.to_owned()
}

fn is_volume_qualified(typed: &str) -> bool {
    typed.starts_with("\\\\")
        || typed
            .as_bytes()
            .get(1)
            .is_some_and(|byte| *byte == b':' && typed.as_bytes()[0].is_ascii_alphabetic())
}

/// Drive letters of the volumes this machine holds locally, as `C:\` roots. Mapped
/// network letters are not among them: they are the `\\host\share` case, which the tick
/// deliberately asks late.
#[cfg(windows)]
fn local_volume_roots() -> Vec<String> {
    // The 0.62 bindings return the drive type as a plain number.
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};

    // SAFETY: `GetLogicalDrives` takes no arguments, and `GetDriveTypeW` is given a
    // NUL-terminated UTF-16 buffer outlives the call and is only read.
    let present = unsafe { GetLogicalDrives() };
    let mut roots = Vec::new();
    for index in 0..26 {
        if present & (1 << index) == 0 {
            continue;
        }
        let root = format!("{}:\\", char::from(b'A' + index));
        let encoded: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let kind = unsafe { GetDriveTypeW(PCWSTR(encoded.as_ptr())) };
        if kind == DRIVE_FIXED || kind == DRIVE_REMOVABLE {
            roots.push(root);
        }
    }
    roots
}

#[cfg(not(windows))]
fn local_volume_roots() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The folder the walk must refuse, named once for both the tree and the assert.
    const DENIED: &str = NEVER_WALKED[0];

    fn scratch(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("flux-path-probe-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch folder has to exist");
        root
    }

    fn file_at(path: &std::path::Path) -> &std::path::Path {
        std::fs::create_dir_all(path.parent().unwrap()).expect("the parent has to exist");
        std::fs::write(path, b"x").expect("the scratch file has to exist");
        path
    }

    fn session() -> ProbeSession {
        ProbeSession::default()
    }

    fn past_deadline() -> Instant {
        Instant::now() + Duration::from_secs(5)
    }

    #[test]
    fn only_path_shaped_text_is_probed() {
        let mut session = session();
        for word in [
            "cmd",
            "notepad.exe",
            "ext:zip",
            "dm:today",
            "2026-08",
            "steam",
        ] {
            assert!(
                session.resolve(word).is_empty(),
                "{word} names no place, so no volume is touched"
            );
        }
    }

    #[test]
    fn a_typed_folder_answers_with_itself_and_nothing_else() {
        let root = scratch("place");
        file_at(&root.join("download office.url"));
        file_at(&root.join("video.mp4"));
        let mut session = session();
        let results = session.resolve(&root.to_string_lossy());

        assert_eq!(results.len(), 1);
        let row = &results[0];
        assert_eq!(
            row.title,
            root.file_name().unwrap().to_str().unwrap(),
            "a folder is titled by its leaf, like a provider row"
        );
        assert_eq!(row.kind, flux_core::ResultKind::File);
        assert_eq!(row.source, flux_core::ResultSource::FileSystem);
        assert_eq!(row.target.as_deref(), Some(&*root.to_string_lossy()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_full_path_to_a_file_shows_that_file() {
        let root = scratch("file");
        let target = file_at(&root.join("clip.mp4")).to_path_buf();
        let mut session = session();
        let results = session.resolve(&target.to_string_lossy());

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "clip.mp4");
        assert_eq!(results[0].kind, flux_core::ResultKind::File);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_half_typed_name_completes_from_the_folder_that_exists() {
        let root = scratch("prefix");
        file_at(&root.join("alpha.txt"));
        file_at(&root.join("beta.txt"));
        let typed = format!("{}\\al", root.to_string_lossy());
        let mut session = session();
        let results = session.resolve(&typed);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "alpha.txt");
        assert_eq!(
            results[0].target.as_deref(),
            Some(&*format!("{}\\alpha.txt", root.to_string_lossy()))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_trailing_separator_does_not_turn_the_row_into_a_listing() {
        let root = scratch("trailing");
        file_at(&root.join("one.txt"));
        file_at(&root.join("two.txt"));
        let typed = format!("{}\\", root.to_string_lossy());
        let mut session = session();
        let results = session.resolve(&typed);

        assert_eq!(
            results.len(),
            1,
            "the separator is not a request for contents: while the query is live it is
             indistinguishable from the moment before the next segment"
        );
        assert_eq!(results[0].target.as_deref(), Some(&*root.to_string_lossy()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_walk_reaches_an_anchor_below_a_volume_root_and_stops_at_its_ceiling() {
        let root = scratch("walk");
        let nested = root.join("a").join("b").join("cache");
        std::fs::create_dir_all(&nested).expect("the nested folder has to exist");
        let mut session = session();
        let found = session.anchor_folders(&root.to_string_lossy(), "cache", past_deadline());
        assert!(
            found.contains(&nested.to_string_lossy().to_string()),
            "{nested:?} is three levels down and inside the depth ceiling"
        );

        let past = root.join("a").join("b").join("c").join("d").join("far");
        std::fs::create_dir_all(&past).expect("the deep folder has to exist");
        let found = session.anchor_folders(&root.to_string_lossy(), "far", past_deadline());
        assert!(found.is_empty(), "depth {MAX_WALK_DEPTH} is the ceiling");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_walk_never_enters_the_folder_that_only_answers_with_denied() {
        let root = scratch("skip");
        let inside = root.join(DENIED).join("cache");
        std::fs::create_dir_all(&inside).expect("the folder inside has to exist");
        let mut session = session();
        let found = session.anchor_folders(&root.to_string_lossy(), "cache", past_deadline());

        assert!(
            found.is_empty(),
            "{DENIED} is skipped before the syscall, not after it"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_read_the_budget_abandoned_is_not_cached_as_the_whole_folder() {
        let root = scratch("abandoned");
        file_at(&root.join("wanted.txt"));
        let expired = Instant::now();
        std::thread::sleep(Duration::from_millis(2));
        let mut session = session();

        let starved = session.children(&root.to_string_lossy(), expired);
        let later = session.children(&root.to_string_lossy(), past_deadline());

        assert!(
            starved.is_empty(),
            "an expired budget stops the read before it starts"
        );
        assert_eq!(
            later.len(),
            1,
            "the next read has to see the folder, not a cached stub of a partial read"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_root_listing_is_read_once_per_session() {
        let root = scratch("cached");
        file_at(&root.join("one.txt"));
        let mut session = session();
        let first = session
            .children(&root.to_string_lossy(), past_deadline())
            .len();
        file_at(&root.join("two.txt"));
        let second = session
            .children(&root.to_string_lossy(), past_deadline())
            .len();

        assert_eq!(
            first, second,
            "the second read is the cache, not the volume"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_share_is_the_query_the_tick_asks_late() {
        assert!(is_remote_path_query(r"\\nas\media\films"));
        assert!(is_remote_path_query(r"\\nas"));
        assert!(!is_remote_path_query(r"c:\tools\cmd"));
        assert!(!is_remote_path_query(r"tools\cmd"));
        assert!(!is_remote_path_query("steam"));
    }

    #[test]
    fn the_local_roots_are_drive_roots_reported_once_in_order() {
        let roots = local_volume_roots();
        if cfg!(windows) {
            assert!(
                !roots.is_empty(),
                "a Windows machine always has at least one local volume"
            );
        }
        for root in &roots {
            assert!(root.ends_with(":\\"), "{root} is a volume root");
        }
        let mut sorted = roots.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, roots, "the mask reports each letter once, in order");
    }

    #[test]
    fn a_bare_drive_answers_as_the_volume_root() {
        if !cfg!(windows) {
            return;
        }
        let mut session = session();
        let results = session.resolve("c:");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "c:");
        assert_eq!(results[0].target.as_deref(), Some("c:\\"));
    }

    #[test]
    fn openable_path_repairs_the_form_a_stat_would_read_as_a_different_place() {
        assert_eq!(openable_path("c:"), r"c:\");
        assert_eq!(openable_path(r"C:\Tools\cmd"), r"C:\Tools\cmd");
        assert_eq!(joined(r"C:\", "cmd"), r"C:\cmd");
        assert_eq!(joined(r"C:\Tools", "cmd"), r"C:\Tools\cmd");
    }
}
