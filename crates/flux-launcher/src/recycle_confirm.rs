use super::launch;

/// Entry point for the standalone `--empty-recycle-confirm` child process.
///
/// The launcher hides itself and spawns this short-lived process. The child delegates
/// entirely to the Windows shell: `empty_recycle_bin` raises the native "Delete
/// Multiple Items" confirmation, and there is no Flux-owned dialog. Running the
/// blocking shell call in its own process keeps the launcher's UI thread out of a
/// nested modal message loop. It exits 0 when the bin was emptied and 1 when the user
/// declined or the shell reported a failure.
///
/// Hiding stops the launcher's interval timers, so the parent cannot watch the child
/// while the prompt is up: it reaps it on the first search tick after the launcher is
/// shown again, and that tick is what publishes the outcome to the status bar.
pub(crate) fn run() -> ! {
    // Smoke hook: the Windows shell prompt cannot be answered without permanently
    // deleting the Recycle Bin contents, so a smoke run asks the child to report an
    // outcome without touching the bin ("emptied" = 0, anything else = 1). Unset in
    // normal use, so the real shell call below is the only production path.
    if let Some(outcome) = std::env::var_os("FLUX_SMOKE_RECYCLE_CONFIRM") {
        let emptied = outcome.to_string_lossy() == "emptied";
        std::process::exit(if emptied { 0 } else { 1 });
    }
    std::process::exit(if launch::empty_recycle_bin() { 0 } else { 1 });
}
