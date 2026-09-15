use super::launch;

/// Entry point for the standalone `--empty-recycle-confirm` child process.
///
/// The launcher hides itself, spawns this short-lived process, and reshows once the
/// child exits. The child delegates entirely to the Windows shell: `empty_recycle_bin`
/// raises the native "Delete Multiple Items" confirmation, and there is no Flux-owned
/// dialog. Running the blocking shell call in its own process keeps the launcher's UI
/// thread out of a nested modal message loop. It exits 0 when the bin was emptied and
/// 1 when the user declined or the shell reported a failure.
pub(crate) fn run() -> ! {
    std::process::exit(if launch::empty_recycle_bin() { 0 } else { 1 });
}
