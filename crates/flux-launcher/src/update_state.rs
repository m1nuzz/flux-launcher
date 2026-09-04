use std::cell::Cell;

use flux_core::Settings;
use windui::prelude::Sender;

#[derive(Clone, Debug)]
pub(crate) enum UpdateInstallResponse {
    Progress {
        version: String,
        progress: crate::updater::DownloadProgress,
    },
    Started {
        version: String,
    },
    Failed {
        version: String,
        error: String,
    },
}

pub(crate) fn request_update_check(
    sender: Sender<crate::updater::UpdateCheckResponse>,
    in_flight: &Cell<bool>,
) -> bool {
    if in_flight.replace(true) {
        return false;
    }
    spawn_update_check(sender);
    true
}

fn spawn_update_check(sender: Sender<crate::updater::UpdateCheckResponse>) {
    let _ = std::thread::Builder::new()
        .name(String::from("flux-update-check"))
        .spawn(move || {
            let checked_at = crate::updater::unix_now();
            let result = crate::updater::check_stable(crate::CURRENT_VERSION);
            let _ = sender.send(crate::updater::UpdateCheckResponse { checked_at, result });
        });
}

pub(crate) fn request_update_install(
    update: crate::updater::StableUpdate,
    sender: Sender<UpdateInstallResponse>,
    in_flight: &Cell<bool>,
    relaunch_mode: crate::updater::RelaunchMode,
) -> bool {
    if in_flight.replace(true) {
        return false;
    }
    spawn_update_install(update, sender, relaunch_mode);
    true
}

fn spawn_update_install(
    update: crate::updater::StableUpdate,
    sender: Sender<UpdateInstallResponse>,
    relaunch_mode: crate::updater::RelaunchMode,
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
            let download = crate::updater::download_installer_to_path(
                &update,
                &installer_path,
                move |progress| {
                    trace_update_event(&format!(
                        "update-progress\t{}\t{}\t{:?}",
                        version_for_progress, progress.received_bytes, progress.total_bytes
                    ));
                    let _ = progress_sender.send(UpdateInstallResponse::Progress {
                        version: version_for_progress.clone(),
                        progress,
                    });
                },
            );
            match download {
                Ok(_) => match crate::updater::handoff_installer(&installer_path, relaunch_mode) {
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

pub(crate) fn format_bytes(bytes: u64) -> String {
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
        || crate::updater::should_check(
            crate::updater::unix_now(),
            settings.last_update_check_unix,
            settings.update_interval_hours,
        )
}

pub(crate) fn format_update_progress(
    version: &str,
    progress: &crate::updater::DownloadProgress,
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
