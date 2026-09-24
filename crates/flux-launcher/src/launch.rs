use std::path::PathBuf;

#[cfg(windows)]
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
static TRACE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Append a launch lifecycle event when the smoke harness requests tracing.
///
/// The trace is intentionally opt-in and has no effect during normal use.
pub fn trace_launch_event(event: &str) {
    #[cfg(windows)]
    {
        let Ok(path) = std::env::var("FLUX_LAUNCH_TRACE_FILE") else {
            return;
        };
        let Ok(_guard) = TRACE_LOCK.get_or_init(|| Mutex::new(())).lock() else {
            return;
        };
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
            return;
        };
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64() * 1000.0)
            .unwrap_or_default();
        let _ = writeln!(file, "{timestamp_ms:.3}\t{event}");
    }
}

#[cfg(windows)]
pub fn open_path(path: &str) -> bool {
    shell_execute("open", path, None)
}

/// Schedule a shell launch without blocking the launcher UI thread.
///
/// The dispatch is intentionally queued before the hide operation, matching
/// Flow Launcher: the shell can start resolving the target while windui
/// applies the pending hide after the event callback returns.
pub fn open_path_async(path: &str) {
    let path = path.to_owned();
    trace_launch_event("launch-dispatch");
    let _ = std::thread::Builder::new()
        .name(String::from("flux-shell-launch"))
        .spawn(move || {
            trace_launch_event("shell-worker-start");
            let target = clean_shell_target(&path);
            if target.is_dir() {
                trace_launch_event("directory-launch-dispatch");
                let _ = open_directory(target);
            } else {
                let _ = open_path(&path);
            }
        });
}

/// Schedule opening the Recycle Bin without blocking the launcher UI thread.
pub fn open_recycle_bin_async() {
    trace_launch_event("launch-dispatch");
    let _ = std::thread::Builder::new()
        .name(String::from("flux-shell-launch"))
        .spawn(|| {
            trace_launch_event("shell-worker-start");
            let _ = open_recycle_bin();
        });
}

#[cfg(windows)]
pub fn open_directory(path: PathBuf) -> bool {
    let path = path.to_string_lossy().replace('"', "");
    let arguments = format!("/e,\"{path}\"");
    shell_execute("open", "explorer.exe", Some(&arguments))
}

#[cfg(windows)]
pub fn open_url(url: &str) -> bool {
    shell_execute("open", url, None)
}

/// 在后台 shell 线程执行系统命令（如关机、锁定），供系统命令 provider 使用。
#[cfg(windows)]
pub fn run_command(program: &str, arguments: &str) -> bool {
    shell_execute("open", program, Some(arguments))
}

#[cfg(not(windows))]
pub fn run_command(_program: &str, _arguments: &str) -> bool {
    false
}

#[cfg(windows)]
pub fn open_recycle_bin() -> bool {
    shell_execute("open", "shell:RecycleBinFolder", None)
}

#[cfg(windows)]
pub fn empty_recycle_bin() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOPROGRESSUI, SHERB_NOSOUND};

    // Keep the Windows confirmation prompt enabled. Flux asks for confirmation
    // first, then Windows provides its standard final safety prompt.
    unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), SHERB_NOPROGRESSUI | SHERB_NOSOUND).is_ok() }
}

#[cfg(windows)]
pub fn run_as_admin(path: &str) -> bool {
    shell_execute("runas", path, None)
}

#[cfg(windows)]
pub fn open_file_location(path: &str) -> bool {
    let argument = format!("/select,\"{path}\"");
    shell_execute("open", "explorer.exe", Some(&argument))
}

#[cfg(windows)]
fn shell_execute(verb: &str, target: &str, arguments: Option<&str>) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let directory = Path::new(target)
        .parent()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>()
        });
    let target = target
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let verb = verb
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let arguments = arguments.map(|value| {
        value
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
    });
    let arguments_ptr = arguments
        .as_ref()
        .map(|value| PCWSTR(value.as_ptr()))
        .unwrap_or_else(PCWSTR::null);
    let directory_ptr = directory
        .as_ref()
        .map(|value| PCWSTR(value.as_ptr()))
        .unwrap_or_else(PCWSTR::null);

    trace_launch_event("shell-execute-start");
    let mut execute_info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: HWND::default(),
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(target.as_ptr()),
        lpParameters: arguments_ptr,
        lpDirectory: directory_ptr,
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    let success = unsafe { ShellExecuteExW(&mut execute_info).is_ok() };
    if !success {
        trace_launch_event("shell-execute-failed");
        return false;
    }
    if !execute_info.hProcess.is_invalid() {
        trace_launch_event("process-created");
        unsafe {
            let _ = CloseHandle(execute_info.hProcess);
        }
    } else {
        trace_launch_event("shell-return");
    }
    true
}

#[cfg(not(windows))]
pub fn open_path(_path: &str) -> bool {
    false
}

fn clean_shell_target(path: &str) -> PathBuf {
    let trimmed = path.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(trimmed);
    PathBuf::from(unquoted)
}

#[cfg(not(windows))]
pub fn open_directory(_path: PathBuf) -> bool {
    false
}

#[cfg(not(windows))]
pub fn open_url(_url: &str) -> bool {
    false
}

#[cfg(not(windows))]
pub fn open_recycle_bin() -> bool {
    false
}

#[cfg(not(windows))]
pub fn empty_recycle_bin() -> bool {
    false
}

#[cfg(not(windows))]
pub fn run_as_admin(_path: &str) -> bool {
    false
}

#[cfg(not(windows))]
pub fn open_file_location(_path: &str) -> bool {
    false
}

/// Append a query-stage profiling event when the diagnostics harness requests it.
///
/// `FLUX_QUERY_PROFILE_FILE` opts in; each line is `{unix_ms}\t{event}\t{detail}`,
/// so a single run can be analyzed as a table. Stages covered today: catalog
/// search duration, per-icon extraction duration and icon queue depth.
pub fn trace_query_profile(event: &str, detail: &str) {
    let Ok(path) = std::env::var("FLUX_QUERY_PROFILE_FILE") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let _ = write_query_profile_line(Path::new(&path), event, detail);
}

/// 诊断日志由 UI 线程与 shell 图标 worker 线程并发追加。行必须一次性写入，
/// 否则格式化的多段 write 会互相交错，产生无法解析的坏行（见
/// `doc/archive/plan/u-key-lag-remediation-2026-09-18.md` 步骤 1）。
static QUERY_PROFILE_WRITE_LOCK: Mutex<()> = Mutex::new(());

fn write_query_profile_line(path: &Path, event: &str, detail: &str) -> std::io::Result<()> {
    use std::io::Write;

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .unwrap_or_default();
    let line = format!(
        "{timestamp_ms:.3}\t{event}\t{}\n",
        sanitize_profile_detail(detail)
    );
    // 先整体成行再单次 write_all；锁用于跨线程串行化，避免不同线程的写入互相穿插。
    let _guard = QUERY_PROFILE_WRITE_LOCK.lock();
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    file.flush()
}

/// detail 中的换行会把一条记录拆成两行无法解析的文本，统一压成空格。
fn sanitize_profile_detail(detail: &str) -> std::borrow::Cow<'_, str> {
    if detail.contains(['\r', '\n']) {
        std::borrow::Cow::Owned(detail.replace(['\r', '\n'], " "))
    } else {
        std::borrow::Cow::Borrowed(detail)
    }
}

#[cfg(test)]
mod tests {
    use super::{clean_shell_target, sanitize_profile_detail, write_query_profile_line};

    #[test]
    fn clean_shell_target_removes_outer_quotes_and_whitespace() {
        assert_eq!(
            clean_shell_target(r#"  "C:\Users\m1nus\Music Pack"  "#),
            std::path::PathBuf::from(r"C:\Users\m1nus\Music Pack")
        );
    }

    #[test]
    fn existing_directory_target_is_classified_as_directory() {
        let path =
            std::env::temp_dir().join(format!("flux-folder-launch-test-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        assert!(clean_shell_target(&format!(r#""{}""#, path.display())).is_dir());
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn query_profile_line_appends_timestamped_event() {
        let path =
            std::env::temp_dir().join(format!("flux-query-profile-{}.log", std::process::id()));
        write_query_profile_line(&path, "catalog-search", "1.5ms\t3\tu").unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        let line = contents.lines().last().unwrap();
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts[0].parse::<f64>().is_ok(), "时间戳必须可解析：{line}");
        assert_eq!(parts[1], "catalog-search");
        assert_eq!(parts[2], "1.5ms\t3\tu");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn query_profile_detail_newlines_are_collapsed() {
        assert_eq!(sanitize_profile_detail("a\tb"), "a\tb");
        assert_eq!(sanitize_profile_detail("a\r\nb"), "a  b");
    }

    #[test]
    fn query_profile_line_is_written_whole_from_concurrent_threads() {
        let path = std::env::temp_dir().join(format!(
            "flux-query-profile-concurrent-{}.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let writers = ["icon-request", "icon-extract"];
        std::thread::scope(|scope| {
            for event in writers {
                let path = path.clone();
                scope.spawn(move || {
                    for line in 0..500 {
                        write_query_profile_line(
                            &path,
                            event,
                            &format!("queued={}\ttarget-{}.lnk", line % 16, line),
                        )
                        .unwrap();
                    }
                });
            }
        });

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(
            lines.len(),
            1000,
            "并发写入不得产生坏行或丢失记录：实际 {}",
            lines.len()
        );
        for line in lines {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            assert_eq!(parts.len(), 3, "行必须是三段式：{line}");
            assert!(parts[0].parse::<f64>().is_ok(), "时间戳必须可解析：{line}");
            assert!(writers.contains(&parts[1]), "事件名必须完整：{line}");
            assert!(parts[2].starts_with("queued="), "detail 必须完整：{line}");
        }
        std::fs::remove_file(path).unwrap();
    }
}
