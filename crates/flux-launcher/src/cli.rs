use super::launch;
use super::native_host;
use super::recycle_confirm;
use super::shell_icon_cache::shortcut_icon_smoke;
use super::ui_constants::SINGLE_INSTANCE_ID;
use super::visual_preview;

pub(crate) fn should_claim_single_instance(mode: Option<&std::ffi::OsStr>) -> bool {
    !matches!(
        mode,
        Some(mode)
            if mode == std::ffi::OsStr::new("--plugin-host")
                || mode == std::ffi::OsStr::new("--folder-launch-smoke")
                || mode == std::ffi::OsStr::new("--shortcut-icon-smoke")
    )
}

pub(crate) fn is_shutdown_mode(mode: Option<&std::ffi::OsStr>) -> bool {
    mode == Some(std::ffi::OsStr::new("--shutdown"))
}

pub(crate) enum StartupAction {
    Exit,
    Launch {
        startup: bool,
        single_instance_disabled: bool,
    },
}

pub(crate) fn handle_cli_modes() -> StartupAction {
    #[cfg(windows)]
    {
        // Monitor coordinates are queried before windui creates the HWND. Set
        // per-monitor awareness first so Windows does not virtualize the
        // 4K/mixed-DPI work area used for the initial center position.
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    }
    let mut args = std::env::args_os();
    let _executable = args.next();
    let mode = args.next();
    if mode.as_deref() == Some(std::ffi::OsStr::new("--visual-preview")) {
        let mut values = [0_i32; 4];
        for value in &mut values {
            let Some(raw) = args.next() else {
                eprintln!("visual preview requires width height x y");
                std::process::exit(2);
            };
            let Ok(parsed) = raw.to_string_lossy().parse::<i32>() else {
                eprintln!("visual preview dimensions and position must be integers");
                std::process::exit(2);
            };
            *value = parsed;
        }
        visual_preview::run(values[0], values[1], values[2], values[3]);
        return StartupAction::Exit;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--empty-recycle-confirm")) {
        // Standalone centered confirmation window; run() exits the process itself.
        recycle_confirm::run();
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--plugin-host")) {
        let root = args
            .next()
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("FLUX_NATIVE_PLUGIN_DIR").map(std::path::PathBuf::from))
            .unwrap_or_else(|| std::path::PathBuf::from("NativePlugins"));
        let pipe_name = args
            .next()
            .map(|value| value.to_string_lossy().into_owned());
        native_host::run(root, pipe_name);
        return StartupAction::Exit;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--folder-launch-smoke")) {
        if let Some(target) = args.next() {
            launch::open_path_async(&target.to_string_lossy());
            std::thread::sleep(std::time::Duration::from_millis(900));
        }
        return StartupAction::Exit;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--shortcut-icon-smoke")) {
        #[cfg(windows)]
        {
            let Some(target) = args.next() else {
                eprintln!("shortcut icon smoke requires a shortcut path");
                std::process::exit(2);
            };
            if !shortcut_icon_smoke(&target.to_string_lossy()) {
                eprintln!(
                    "shortcut icon extraction failed for {}",
                    target.to_string_lossy()
                );
                std::process::exit(1);
            }
        }
        return StartupAction::Exit;
    }
    let single_instance_disabled = std::env::var_os("FLUX_DISABLE_SINGLE_INSTANCE").is_some();
    if !single_instance_disabled
        && should_claim_single_instance(mode.as_deref())
        && matches!(
            windui::claim_instance(SINGLE_INSTANCE_ID),
            windui::InstanceRole::Handoff
        )
    {
        return StartupAction::Exit;
    }
    // The uninstaller uses this one-shot mode only to reach the already-running
    // instance through the single-instance listener. Never create a new UI if
    // there is no instance left to shut down.
    if is_shutdown_mode(mode.as_deref()) {
        return StartupAction::Exit;
    }
    StartupAction::Launch {
        startup: mode.as_deref() == Some(std::ffi::OsStr::new("--startup")),
        single_instance_disabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_host_mode_bypasses_main_single_instance_guard() {
        assert!(!should_claim_single_instance(Some(std::ffi::OsStr::new(
            "--plugin-host"
        ))));
    }

    #[test]
    fn folder_launch_smoke_mode_bypasses_main_single_instance_guard() {
        assert!(!should_claim_single_instance(Some(std::ffi::OsStr::new(
            "--folder-launch-smoke"
        ))));
    }

    #[test]
    fn normal_and_startup_modes_use_main_single_instance_guard() {
        assert!(should_claim_single_instance(None));
        assert!(should_claim_single_instance(Some(std::ffi::OsStr::new(
            "--startup"
        ))));
    }

    #[test]
    fn shutdown_mode_is_a_single_instance_command() {
        assert!(should_claim_single_instance(Some(std::ffi::OsStr::new(
            "--shutdown"
        ))));
        assert!(is_shutdown_mode(Some(std::ffi::OsStr::new("--shutdown"))));
        assert!(!is_shutdown_mode(Some(std::ffi::OsStr::new("--startup"))));
        assert!(!is_shutdown_mode(None));
    }
}
