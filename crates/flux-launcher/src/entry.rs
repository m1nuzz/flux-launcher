use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;

use crate::{launch, native_host, visual_preview};
use flux_core::Settings;

#[cfg(windows)]
use crate::icons::shortcut_icon_smoke;

pub(crate) fn should_claim_single_instance(mode: Option<&OsStr>) -> bool {
    !matches!(
        mode,
        Some(mode)
            if mode == OsStr::new("--plugin-host")
                || mode == OsStr::new("--folder-launch-smoke")
                || mode == OsStr::new("--shortcut-icon-smoke")
    )
}

pub(crate) fn is_shutdown_mode(mode: Option<&OsStr>) -> bool {
    mode == Some(OsStr::new("--shutdown"))
}

pub(crate) fn is_startup_mode(mode: Option<&OsStr>) -> bool {
    mode == Some(OsStr::new("--startup"))
}

pub(crate) fn load_settings(mode: Option<&OsStr>) -> (Settings, bool) {
    let settings = Settings::load_or_default();
    crate::i18n::apply_configured_locale(settings.language);
    if let Err(error) = crate::startup::set_enabled(settings.start_with_windows) {
        eprintln!("Could not synchronize Windows startup setting: {error}");
    }
    (settings, is_startup_mode(mode))
}

pub(crate) fn run_special_mode(
    mode: Option<&OsStr>,
    args: &mut impl Iterator<Item = OsString>,
) -> bool {
    if mode == Some(OsStr::new("--visual-preview")) {
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
        let locale = args
            .next()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("en"));
        visual_preview::run(values[0], values[1], values[2], values[3], &locale);
        return true;
    }

    if mode == Some(OsStr::new("--plugin-host")) {
        let root = args
            .next()
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("FLUX_NATIVE_PLUGIN_DIR").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("NativePlugins"));
        let pipe_name = args
            .next()
            .map(|value| value.to_string_lossy().into_owned());
        native_host::run(root, pipe_name);
        return true;
    }

    if mode == Some(OsStr::new("--folder-launch-smoke")) {
        if let Some(target) = args.next() {
            launch::open_path_async(&target.to_string_lossy());
            std::thread::sleep(Duration::from_millis(900));
        }
        return true;
    }

    if mode == Some(OsStr::new("--shortcut-icon-smoke")) {
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
        return true;
    }

    false
}

pub(crate) fn init_platform() {
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
    crate::i18n::apply_system_locale();
}

pub(crate) fn split_args() -> (Option<OsString>, std::env::ArgsOs) {
    let mut args = std::env::args_os();
    let _executable = args.next();
    (args.next(), args)
}

pub(crate) fn should_exit_early(
    mode: Option<&std::ffi::OsStr>,
    args: &mut std::env::ArgsOs,
    single_instance_disabled: bool,
) -> bool {
    if run_special_mode(mode, &mut *args) {
        return true;
    }

    if !single_instance_disabled
        && should_claim_single_instance(mode)
        && matches!(
            windui::claim_instance(crate::SINGLE_INSTANCE_ID),
            windui::InstanceRole::Handoff
        )
    {
        return true;
    }

    // The uninstaller uses this one-shot mode only to reach the already-running
    // instance through the single-instance listener. Never create a new UI if
    // there is no instance left to shut down.
    if is_shutdown_mode(mode) {
        return true;
    }

    false
}
