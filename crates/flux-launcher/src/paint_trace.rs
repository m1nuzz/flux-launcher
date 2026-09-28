//! Env-gated trace of every event that can repaint the result list.
//!
//! The list rebuilds all of its rows whenever the results signal is written, so
//! a flicker report only becomes diagnosable with the exact order of keystroke,
//! list write, icon arrival and window resize on one clock. Set
//! `FLUX_PAINT_TRACE_FILE` to a path and each event is appended as one line;
//! without it, every call is a single env lookup that returns.

use std::fs::File;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static START: OnceLock<Instant> = OnceLock::new();
static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();

fn sink() -> Option<&'static Mutex<File>> {
    SINK.get_or_init(|| {
        let path = std::env::var_os("FLUX_PAINT_TRACE_FILE")?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Append `event=<name> <detail>` with both a monotonic and a wall-clock stamp,
/// so the trace can be aligned against screenshots taken by a driver script.
pub(crate) fn note(event: &str, detail: &str) {
    let Some(sink) = sink() else {
        return;
    };
    let started = START.get_or_init(Instant::now);
    let Ok(mut file) = sink.lock() else {
        return;
    };
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    // Milliseconds cannot order two events that land in the same tick, and the
    // launcher legitimately paints in the same millisecond Everything answers.
    // The sequence number, taken under the sink lock, is the exact write order,
    // so a driver can compare (unix, seq) instead of guessing from the clock.
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let _ = writeln!(
        file,
        "t={} unix={} seq={} event={} {}",
        started.elapsed().as_millis(),
        unix_ms,
        seq,
        event,
        detail
    );
}
