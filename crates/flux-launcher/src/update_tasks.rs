use std::cell::Cell;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use flux_core::Settings;
use windui::prelude::Sender;

use super::ui_constants::CURRENT_VERSION;
use super::updater;

static SETTINGS_SAVE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn settings_save_lock() -> &'static Mutex<()> {
    SETTINGS_SAVE_LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn save_settings(settings: &Settings) -> bool {
    let Ok(_save_guard) = settings_save_lock().lock() else {
        return false;
    };
    settings.save().is_ok()
}

pub(crate) fn request_update_check(
    sender: Sender<updater::UpdateCheckResponse>,
    in_flight: &Cell<bool>,
) -> bool {
    if in_flight.replace(true) {
        return false;
    }
    spawn_update_check(sender);
    true
}

fn spawn_update_check(sender: Sender<updater::UpdateCheckResponse>) {
    let _ = std::thread::Builder::new()
        .name(String::from("flux-update-check"))
        .spawn(move || {
            let checked_at = updater::unix_now();
            let result = updater::check_stable(CURRENT_VERSION);
            let _ = sender.send(updater::UpdateCheckResponse { checked_at, result });
        });
}

#[derive(Clone, Debug)]
pub(crate) enum UpdateInstallResponse {
    Progress {
        version: String,
        progress: updater::DownloadProgress,
    },
    Started {
        version: String,
    },
    Failed {
        version: String,
        error: String,
    },
}

pub(crate) fn request_update_install(
    update: updater::StableUpdate,
    sender: Sender<UpdateInstallResponse>,
    in_flight: &Cell<bool>,
    relaunch_mode: updater::RelaunchMode,
) -> bool {
    if in_flight.replace(true) {
        return false;
    }
    spawn_update_install(update, sender, relaunch_mode);
    true
}

fn spawn_update_install(
    update: updater::StableUpdate,
    sender: Sender<UpdateInstallResponse>,
    relaunch_mode: updater::RelaunchMode,
) {
    let _ = std::thread::Builder::new()
        .name(String::from("flux-update-install"))
        .spawn(move || {
            let version = update.version.to_string();
            trace_update_event(&format!("update-install-start\\t{version}"));
            let installer_path =
                std::env::temp_dir().join(format!("FluxLauncher-update-{}.exe", update.version));
            let version_for_progress = version.clone();
            let progress_sender = sender.clone();
            let download =
                updater::download_installer_to_path(&update, &installer_path, move |progress| {
                    trace_update_event(&format!(
                        "update-progress\t{}\t{}\t{:?}",
                        version_for_progress, progress.received_bytes, progress.total_bytes
                    ));
                    let _ = progress_sender.send(UpdateInstallResponse::Progress {
                        version: version_for_progress.clone(),
                        progress,
                    });
                });
            match download {
                Ok(_) => match updater::handoff_installer(&installer_path, relaunch_mode) {
                    Ok(()) => {
                        trace_update_event(&format!("update-installer-started\\t{version}"));
                        let _ = sender.send(UpdateInstallResponse::Started { version });
                    }
                    Err(error) => {
                        trace_update_event(&format!("update-failed\\t{version}\\t{error}"));
                        let _ = std::fs::remove_file(&installer_path);
                        let _ = sender.send(UpdateInstallResponse::Failed { version, error });
                    }
                },
                Err(error) => {
                    trace_update_event(&format!("update-failed\\t{version}\\t{error}"));
                    let _ = sender.send(UpdateInstallResponse::Failed { version, error });
                }
            }
        });
}

fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.0} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn trace_update_event(event: &str) {
    let Some(path) = std::env::var_os("FLUX_UPDATE_TRACE_FILE") else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write as _;
        let _ = writeln!(file, "{event}");
    }
}

pub(crate) fn update_check_due(settings: &Settings) -> bool {
    let forced = std::env::var("FLUX_FORCE_UPDATE_CHECK")
        .map(|value| value == "1")
        .unwrap_or(false);
    forced
        || updater::should_check(
            updater::unix_now(),
            settings.last_update_check_unix,
            settings.update_interval_hours,
        )
}

pub(crate) fn format_update_progress(
    version: &str,
    progress: &updater::DownloadProgress,
) -> String {
    match progress.total_bytes.filter(|total| *total > 0) {
        Some(total) => {
            let received = progress.received_bytes.min(total);
            let percent = received.saturating_mul(100) / total;
            let remaining = total.saturating_sub(received);
            format!(
                "Downloading stable {version}: {percent}% — {} / {} ({} remaining)",
                format_bytes(received),
                format_bytes(total),
                format_bytes(remaining)
            )
        }
        None => format!(
            "Downloading stable {version}: {} received",
            format_bytes(progress.received_bytes)
        ),
    }
}

pub(crate) fn save_settings_async(settings: &Arc<RwLock<Settings>>) {
    let settings = Arc::clone(settings);
    let _ = std::thread::Builder::new()
        .name(String::from("flux-settings-save"))
        .spawn(move || {
            // Read the latest settings snapshot after waiting for any mutation.
            if let Ok(settings_guard) = settings.read() {
                let _ = save_settings(&settings_guard);
            }
        });
}

pub(crate) fn relaunch_mode_for_auto_install() -> updater::RelaunchMode {
    // Automatic updates must remain invisible: a restart should return to the
    // tray and never reopen Search. Manual Install now uses Visible explicitly.
    updater::RelaunchMode::Hidden
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_update_always_restarts_hidden() {
        assert_eq!(
            relaunch_mode_for_auto_install(),
            super::updater::RelaunchMode::Hidden
        );
    }

    #[test]
    fn update_progress_text_exposes_percent_bytes_and_remaining_work() {
        let progress = super::updater::DownloadProgress {
            received_bytes: 512,
            total_bytes: Some(1024),
        };
        assert_eq!(format_bytes(1024), "1 KiB");
        assert_eq!(
            format_update_progress("0.1.64", &progress),
            "Downloading stable 0.1.64: 50% — 512 B / 1 KiB (512 B remaining)"
        );
    }
}
