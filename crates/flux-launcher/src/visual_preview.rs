use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use windui::app::{WindowPositionHandle, WindowSizeHandle};
use windui::prelude::*;

const PREVIEW_REPORT_INTERVAL: Duration = Duration::from_millis(40);

#[derive(Debug, PartialEq, Eq)]
enum PreviewCommand {
    Resize {
        width: i32,
        height: i32,
        x: i32,
        y: i32,
    },
    Close,
}

fn parse_command(line: &str) -> Option<PreviewCommand> {
    let mut parts = line.split_whitespace();
    let command = parts.next()?;
    let parsed = match command {
        "resize" => PreviewCommand::Resize {
            width: parts.next()?.parse().ok()?,
            height: parts.next()?.parse().ok()?,
            x: parts.next()?.parse().ok()?,
            y: parts.next()?.parse().ok()?,
        },
        "close" => PreviewCommand::Close,
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(parsed)
}

fn expected_client_pixels(logical: u16, dpi: u32) -> i32 {
    ((u32::from(logical) * dpi + 48) / 96) as i32
}

fn announce(line: &str) {
    println!("{line}");
    let _ = io::stdout().flush();
}

fn relay_stderr(stream: ChildStderr) {
    thread::Builder::new()
        .name(String::from("flux-preview-stderr-relay"))
        .spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                eprintln!("VisualPreviewChild stderr: {line}");
            }
        })
        .ok();
}

fn relay_stdout(stream: ChildStdout, ready_sender: mpsc::Sender<u32>) {
    thread::Builder::new()
        .name(String::from("flux-preview-stdout-relay"))
        .spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if let Some(pid) = line
                    .strip_prefix("READY ")
                    .and_then(|value| value.parse().ok())
                {
                    let _ = ready_sender.send(pid);
                }
                eprintln!("VisualPreviewChild: {line}");
            }
        })
        .ok();
}

/// Controller for the separate native windui preview process.
///
/// The preview process owns a real HWND. The main Settings window never resizes itself;
/// it sends logical client-area dimensions and a stable screen position to this child.
pub(crate) struct PreviewProcess {
    child: Child,
    stdin: ChildStdin,
    ready_receiver: Receiver<u32>,
    pid: u32,
    ready: bool,
    started: std::time::Instant,
}

impl PreviewProcess {
    pub(crate) fn start(width: i32, height: i32, x: i32, y: i32) -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let mut child = Command::new(&executable)
            .env("FLUX_PREVIEW_PARENT_PID", std::process::id().to_string())
            .arg("--visual-preview")
            .arg(width.to_string())
            .arg(height.to_string())
            .arg(x.to_string())
            .arg(y.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("{}: {error}", executable.display()))?;
        let pid = child.id();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| String::from("visual preview stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| String::from("visual preview stderr unavailable"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| String::from("visual preview stdin unavailable"))?;
        let (ready_sender, ready_receiver) = mpsc::channel();
        relay_stdout(stdout, ready_sender);
        relay_stderr(stderr);

        eprintln!("Visual preview process started: pid={pid}");
        Ok(Self {
            child,
            stdin,
            ready_receiver,
            pid,
            ready: false,
            started: std::time::Instant::now(),
        })
    }

    pub(crate) fn poll_ready(&mut self) -> Result<bool, String> {
        if self.ready {
            return Ok(true);
        }
        match self.ready_receiver.try_recv() {
            Ok(reported_pid) if reported_pid == self.pid => {
                self.ready = true;
                eprintln!(
                    "Visual preview process ready: pid={} after {} ms",
                    self.pid,
                    self.started.elapsed().as_millis()
                );
                Ok(true)
            }
            Ok(reported_pid) => Err(format!(
                "visual preview reported pid {reported_pid}, expected {}",
                self.pid
            )),
            // Readiness is reported when the window is finally shown, which can
            // be long after start-up: a child that is merely waiting hidden is
            // not a stalled child. Liveness is checked separately.
            Err(TryRecvError::Empty) => Ok(false),
            Err(TryRecvError::Disconnected) => Err(String::from(
                "visual preview readiness channel disconnected",
            )),
        }
    }

    pub(crate) fn resize(&mut self, width: i32, height: i32, x: i32, y: i32) -> Result<(), String> {
        writeln!(self.stdin, "resize {width} {height} {x} {y}")
            .and_then(|_| self.stdin.flush())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn is_alive(&mut self) -> bool {
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .unwrap_or(false)
    }

    pub(crate) fn stop(&mut self) {
        eprintln!("Stopping visual preview process: pid={}", self.pid);
        let close_result = writeln!(self.stdin, "close").and_then(|_| self.stdin.flush());
        let kill_result = self.child.kill();
        let wait_result = self.child.wait();
        eprintln!(
            "Stopped visual preview process: pid={} close={:?} kill={:?} wait={:?}",
            self.pid, close_result, kill_result, wait_result
        );
    }
}

impl Drop for PreviewProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Whether the launcher process that spawned this preview is still running.
fn parent_process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };
        // SAFETY: OpenProcess only takes a pid; a null handle means the process
        // is already gone, and the handle is closed on every path.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) };
        let Ok(handle) = handle else {
            return false;
        };
        let exited = unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0;
        let _ = unsafe { CloseHandle(handle) };
        !exited
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        true
    }
}

fn preview_content(size_label: Signal<String>) -> Element {
    Element::col()
        .fill()
        .padding(20)
        .spacing(12)
        .bg(Color::rgba(0, 0, 0, 0))
        .child(
            Element::row()
                .width_match()
                .spacing(10)
                .child(Element::label("Flux Launcher").font_size(22.0).fg(Color::WHITE))
                .child(
                    Element::label_signal(size_label)
                        .font_size(12.0)
                        .fg(Color::rgba(235, 241, 255, 190))
                        .text_align(Align::End)
                        .weight(1.0),
                ),
        )
        .child(
            Element::row()
                .width_match()
                .height(46)
                .padding_xy(14, 0)
                .cross(Align::Center)
                .corner(8.0)
                .bg(Color::rgba(255, 255, 255, 24))
                .child(Element::label(crate::i18n::tr("Search")).font_size(15.0).fg(Color::rgba(255, 255, 255, 220))),
        )
        .child(
            Element::label(crate::i18n::tr("This is the actual launcher preview window. Its client area is resized directly, not scaled into a mock card."))
                .font_size(11.0)
                .fg(Color::rgba(235, 241, 255, 170))
                .max_lines(2)
                .truncate(Truncate::End),
        )
        .child(
            Element::col()
                .width_match()
                .spacing(6)
                .child(preview_result(crate::i18n::tr("Preview result"), crate::i18n::tr("Native windui surface")))
                .child(preview_result(
                    crate::i18n::tr("Width and height are exact"),
                    crate::i18n::tr("Measured from this window client area"),
                ))
                .child(preview_result(
                    crate::i18n::tr("Acrylic/Mica remains active"),
                    crate::i18n::tr("The Settings window stays separate"),
                )),
        )
        .child(
            Element::label(crate::i18n::tr("Logical client units (DIP); the window is resized, not scaled."))
                .font_size(10.0)
                .fg(Color::rgba(235, 241, 255, 150))
                .max_lines(1)
                .truncate(Truncate::End),
        )
}

fn preview_result(title: &str, subtitle: &str) -> Element {
    Element::col()
        .width_match()
        .padding_xy(12, 8)
        .spacing(2)
        .corner(8.0)
        .bg(Color::rgba(255, 255, 255, 18))
        .child(
            Element::label(title)
                .font_size(14.0)
                .fg(Color::WHITE)
                .max_lines(1)
                .truncate(Truncate::End),
        )
        .child(
            Element::label(subtitle)
                .font_size(11.0)
                .fg(Color::rgba(235, 241, 255, 165))
                .max_lines(1)
                .truncate(Truncate::End),
        )
}

#[cfg(windows)]
fn current_window_geometry() -> Option<(i32, i32, u32)> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClientRect, GetWindowThreadProcessId,
    };

    unsafe extern "system" fn find_window(hwnd: HWND, data: LPARAM) -> BOOL {
        let pid_slot = &mut *(data.0 as *mut Option<HWND>);
        let mut pid = 0;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            *pid_slot = Some(hwnd);
            BOOL(0)
        } else {
            BOOL(1)
        }
    }

    let mut hwnd = None;
    let _ = unsafe {
        EnumWindows(
            Some(find_window),
            LPARAM(&mut hwnd as *mut Option<HWND> as isize),
        )
    };
    let hwnd = hwnd?;
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect).ok()? };
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    Some((rect.right - rect.left, rect.bottom - rect.top, dpi.max(1)))
}

#[cfg(not(windows))]
fn current_window_geometry() -> Option<(i32, i32, u32)> {
    None
}

pub(crate) fn run(width: i32, height: i32, x: i32, y: i32) {
    let mut settings =
        flux_core::Settings::load_from(&flux_core::Settings::config_path()).unwrap_or_default();
    crate::system_locale::resolve(&mut settings);
    crate::i18n::init(settings.language);
    let width = width.clamp(1, i32::from(u16::MAX));
    let height = height.clamp(1, i32::from(u16::MAX));
    let width_signal = signal(width as u16);
    let height_signal = signal(height as u16);
    let size_label = signal(format!("{width} × {height} logical px client area"));
    let width_for_commands = width_signal;
    let height_for_commands = height_signal;
    let size_label_for_commands = size_label;

    let mut app = App::new("Flux Launcher Preview", width, height)
        .position(x, y)
        .activate_on_start(false)
        .no_activate(true)
        .frameless()
        .resizable(false)
        .renderer(Renderer::Auto)
        .backdrop(Backdrop::Acrylic)
        // It sits beside Settings, which is topmost: a plain window would open
        // under the same always-on-top apps the launcher now covers.
        .topmost()
        .bg(Color::rgba(32, 33, 35, 255))
        .content(preview_content(size_label));
    let size_handle: WindowSizeHandle = app.window_size_handle();
    let position_handle: WindowPositionHandle = app.window_position_handle();

    let preview_sender = app.channel(move |ctx, command| match command {
        PreviewCommand::Resize {
            width,
            height,
            x,
            y,
        } => {
            let width = width.clamp(1, i32::from(u16::MAX));
            let height = height.clamp(1, i32::from(u16::MAX));
            width_for_commands.set(width as u16);
            height_for_commands.set(height as u16);
            size_label_for_commands.set(format!("{width} × {height} logical px client area"));
            size_handle.set(width, height);
            position_handle.set(x, y);
        }
        PreviewCommand::Close => ctx.request_close(),
    });

    thread::Builder::new()
        .name(String::from("flux-preview-command-reader"))
        .spawn(move || {
            let stdin = io::stdin();
            for line in stdin.lock().lines().map_while(Result::ok) {
                let Some(command) = parse_command(&line) else {
                    continue;
                };
                if preview_sender.send(command).is_err() {
                    break;
                }
            }
        })
        .expect("failed to create preview command reader");

    let mut last_geometry = None;
    let parent_pid = std::env::var("FLUX_PREVIEW_PARENT_PID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    app = app
        .on_window_show(|| announce(&format!("READY {}", std::process::id())))
        .on_interval(PREVIEW_REPORT_INTERVAL, move |_ctx| {
            // The preview is part of the launcher, not an app of its own: if the
            // launcher is gone, this window must not outlive it.
            if parent_pid.is_some_and(|pid| !parent_process_alive(pid)) {
                // The preview is a child of the launcher and has nothing of its
                // own to save: end the process instead of negotiating a close,
                // which an interval callback cannot complete.
                std::process::exit(0);
            }
            let logical_width = width_signal.get();
            let logical_height = height_signal.get();
            let geometry = current_window_geometry();
            let report = geometry.and_then(|(client_width, client_height, dpi)| {
                let expected_width = expected_client_pixels(logical_width, dpi);
                let expected_height = expected_client_pixels(logical_height, dpi);
                (client_width == expected_width && client_height == expected_height).then_some((
                    logical_width,
                    logical_height,
                    client_width,
                    client_height,
                    dpi,
                ))
            });
            if report != last_geometry {
                if let Some((logical_width, logical_height, client_width, client_height, dpi)) =
                    report
                {
                    announce(&format!(
                        "GEOMETRY {} {} {} {} {} {}",
                        std::process::id(),
                        logical_width,
                        logical_height,
                        client_width,
                        client_height,
                        dpi
                    ));
                }
                last_geometry = report;
            }
        });

    app.run();
}

#[cfg(test)]
mod tests {
    use super::{expected_client_pixels, parse_command};

    #[test]
    fn rounds_logical_client_units_to_physical_pixels_using_window_dpi() {
        assert_eq!(expected_client_pixels(420, 96), 420);
        assert_eq!(expected_client_pixels(420, 144), 630);
        assert_eq!(expected_client_pixels(381, 120), 476);
    }

    #[test]
    fn parses_exact_resize_command() {
        let command = parse_command("resize 523 422 100 200").expect("resize command");
        assert!(matches!(
            command,
            super::PreviewCommand::Resize {
                width: 523,
                height: 422,
                x: 100,
                y: 200
            }
        ));
    }

    #[test]
    fn rejects_commands_with_trailing_tokens() {
        assert!(parse_command("resize 523 422 100 200 ignored").is_none());
    }

    #[test]
    fn parses_close_command() {
        assert!(matches!(
            parse_command("close"),
            Some(super::PreviewCommand::Close)
        ));
    }
}
