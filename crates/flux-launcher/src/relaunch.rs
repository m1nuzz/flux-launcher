use std::io::{self, BufRead, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::Duration;

const READY: &str = "FLUX_RELAUNCH_READY";
const COMMIT: &str = "FLUX_RELAUNCH_COMMIT";

/// Dropping an uncommitted handoff cancels the replacement and preserves the UI.
pub(crate) struct PreparedRelaunch {
    child: Child,
    input: ChildStdin,
    committed: bool,
}

impl PreparedRelaunch {
    pub(crate) fn commit(mut self) -> io::Result<()> {
        writeln!(self.input, "{COMMIT}")?;
        self.input.flush()?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for PreparedRelaunch {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn prepare_command(command: &mut Command, timeout: Duration) -> io::Result<PreparedRelaunch> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let input = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("replacement stdin unavailable"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("replacement stdout unavailable"))?;
    let mut prepared = PreparedRelaunch {
        child,
        input,
        committed: false,
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("flux-relaunch-ready".to_owned())
        .spawn(move || {
            let mut ready = false;
            for line in io::BufReader::new(output).lines().map_while(Result::ok) {
                if !ready && line.trim() == READY {
                    ready = true;
                    let _ = sender.send(true);
                }
            }
            if !ready {
                let _ = sender.send(false);
            }
        })?;
    if receiver.recv_timeout(timeout) != Ok(true) || prepared.child.try_wait()?.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "replacement did not become ready",
        ));
    }
    Ok(prepared)
}

/// Retry startup while the old instance is alive. Failed readiness never quits
/// the parent, and a slow parent shutdown cannot orphan the replacement.
pub(crate) fn prepare() -> io::Result<PreparedRelaunch> {
    let executable = std::env::current_exe()?;
    let mut last_error = io::Error::other("replacement did not start");
    for _ in 0..2 {
        let mut command = Command::new(&executable);
        command
            .arg("--relaunch")
            .arg(std::process::id().to_string())
            .env("FLUX_OPEN_SETTINGS", "1");
        match prepare_command(&mut command, Duration::from_secs(3)) {
            Ok(prepared) => return Ok(prepared),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

#[cfg(windows)]
fn complete_handoff(pid: u32) -> io::Result<()> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows::Win32::Foundation::WAIT_OBJECT_0;
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };
    // SAFETY: OpenProcess returns a uniquely owned synchronization-only handle.
    // Pin it BEFORE readiness, while the parent is still waiting. This preserves
    // the original process object even if its PID is later reused.
    let handle =
        unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.map_err(io::Error::other)?;
    // SAFETY: ownership of the valid handle is transferred exactly once.
    let _owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    println!("{READY}");
    io::stdout().flush()?;
    let mut response = String::new();
    io::stdin().read_line(&mut response)?;
    if response.trim() != COMMIT {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "replacement cancelled",
        ));
    }
    // SAFETY: the owned process handle remains valid throughout the wait. There
    // is no arbitrary parent-exit deadline: slow cleanup must not lose both UIs.
    if unsafe { WaitForSingleObject(handle, u32::MAX) } != WAIT_OBJECT_0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn complete_handoff(_pid: u32) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native restart requires Windows",
    ))
}

pub(crate) fn wait_for_parent() -> bool {
    let mut args = std::env::args();
    if args.nth(1).as_deref() != Some("--relaunch") {
        return true;
    }
    args.next()
        .and_then(|value| value.parse().ok())
        .is_some_and(|pid| complete_handoff(pid).is_ok())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn fixture(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "relaunch::tests::child_fixture", "--nocapture"])
            .env("FLUX_RELAUNCH_TEST_MODE", mode);
        command
    }

    #[test]
    fn child_fixture() {
        match std::env::var("FLUX_RELAUNCH_TEST_MODE").as_deref() {
            Ok("parent") | Ok("silent") => std::thread::sleep(Duration::from_secs(30)),
            Ok("child") => {
                let pid = std::env::var("FLUX_RELAUNCH_TEST_PARENT")
                    .unwrap()
                    .parse()
                    .unwrap();
                if complete_handoff(pid).is_ok() {
                    std::fs::write(
                        std::env::var_os("FLUX_RELAUNCH_TEST_MARKER").unwrap(),
                        b"started",
                    )
                    .unwrap();
                }
            }
            _ => {}
        }
    }

    #[test]
    fn unready_replacement_is_cancelled_without_exiting_parent() {
        assert!(prepare_command(&mut fixture("silent"), Duration::from_millis(100)).is_err());
    }

    #[test]
    fn replacement_waits_for_parent_and_cleanup_before_starting() {
        let marker =
            std::env::temp_dir().join(format!("flux-relaunch-marker-{}", std::process::id()));
        let mut parent = fixture("parent").stdout(Stdio::null()).spawn().unwrap();
        let mut command = fixture("child");
        command
            .env("FLUX_RELAUNCH_TEST_PARENT", parent.id().to_string())
            .env("FLUX_RELAUNCH_TEST_MARKER", &marker);
        let prepared = prepare_command(&mut command, Duration::from_secs(3)).unwrap();
        assert!(!marker.exists());
        let child_id = prepared.child.id();
        prepared.commit().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !marker.exists(),
            "replacement must not race the live parent"
        );
        parent.kill().unwrap();
        parent.wait().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            marker.exists(),
            "replacement {child_id} must survive parent cleanup"
        );
        std::fs::remove_file(marker).unwrap();
    }
}
