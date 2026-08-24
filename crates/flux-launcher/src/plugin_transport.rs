use std::io::{self, BufRead, BufReader, BufWriter, Write};
#[cfg(not(windows))]
use std::process::{ChildStdin, ChildStdout};
use std::time::{Duration, Instant};

const PIPE_QUERY_TIMEOUT: Duration = Duration::from_millis(500);

#[cfg(windows)]
use std::fs::File;

#[cfg(windows)]
use std::os::windows::io::FromRawHandle;

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE,
};
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
#[cfg(windows)]
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, SetNamedPipeHandleState, WaitNamedPipeW, PIPE_NOWAIT,
    PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
};

const PIPE_BUFFER_BYTES: u32 = 64 * 1024;

pub struct HostIo {
    reader: Box<dyn BufRead + Send>,
    writer: Box<dyn Write + Send>,
}

impl HostIo {
    pub fn read_line(&mut self, line: &mut String) -> io::Result<usize> {
        self.reader.read_line(line)
    }

    pub fn write_line(&mut self, value: &str) -> io::Result<()> {
        self.writer.write_all(value.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }
}

pub fn stdio_host_io() -> HostIo {
    HostIo {
        reader: Box::new(BufReader::new(io::stdin())),
        writer: Box::new(BufWriter::new(io::stdout())),
    }
}

#[cfg(not(windows))]
pub fn connect_host_io(_pipe_name: &str, _timeout: Duration) -> Result<HostIo, String> {
    Err(String::from("named pipes are only available on Windows"))
}

#[cfg(windows)]
pub fn create_host_io(pipe_name: &str) -> Result<HostIo, String> {
    let name = wide(pipe_name);
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            1,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            0,
            None,
        )
    };
    if handle.is_invalid() {
        return Err(format!("CreateNamedPipeW failed for {pipe_name}"));
    }

    let connected = unsafe { ConnectNamedPipe(handle, None) }.is_ok()
        || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
    if !connected {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Err(format!("ConnectNamedPipe failed for {pipe_name}"));
    }

    let file = unsafe { File::from_raw_handle(handle.0 as _) };
    let reader = file
        .try_clone()
        .map_err(|error| format!("clone named pipe reader: {error}"))?;
    Ok(HostIo {
        reader: Box::new(BufReader::new(reader)),
        writer: Box::new(BufWriter::new(file)),
    })
}

#[cfg(windows)]
pub fn connect_host_io(pipe_name: &str, timeout: Duration) -> Result<HostIo, String> {
    let name = wide(pipe_name);
    let deadline = Instant::now() + timeout;
    loop {
        let available = unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), 100) }.as_bool();
        if available {
            let handle = unsafe {
                CreateFileW(
                    PCWSTR(name.as_ptr()),
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_NONE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
            };
            if let Ok(handle) = handle {
                let mode = PIPE_READMODE_BYTE | PIPE_NOWAIT;
                unsafe {
                    SetNamedPipeHandleState(handle, Some(&mode), None, None)
                        .map_err(|error| format!("SetNamedPipeHandleState failed: {error}"))?;
                }
                let file = unsafe { File::from_raw_handle(handle.0 as _) };
                let reader = file
                    .try_clone()
                    .map_err(|error| format!("clone named pipe client reader: {error}"))?;
                return Ok(HostIo {
                    reader: Box::new(BufReader::new(reader)),
                    writer: Box::new(BufWriter::new(file)),
                });
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out connecting to plugin host pipe {pipe_name}"
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub enum ClientIo {
    #[cfg(not(windows))]
    Stdio {
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
    },
    #[cfg(windows)]
    Pipe(HostIo),
}

impl ClientIo {
    pub fn write_line(&mut self, value: &str) -> io::Result<()> {
        match self {
            #[cfg(not(windows))]
            Self::Stdio { stdin, .. } => {
                stdin.write_all(value.as_bytes())?;
                stdin.write_all(b"\n")?;
                stdin.flush()
            }
            #[cfg(windows)]
            Self::Pipe(io) => io.write_line(value),
        }
    }

    pub fn read_line(&mut self, line: &mut String) -> io::Result<usize> {
        match self {
            #[cfg(not(windows))]
            Self::Stdio { stdout, .. } => stdout.read_line(line),
            #[cfg(windows)]
            Self::Pipe(io) => {
                let deadline = Instant::now() + PIPE_QUERY_TIMEOUT;
                loop {
                    match io.read_line(line) {
                        Ok(count) => return Ok(count),
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                return Err(io::Error::new(
                                    io::ErrorKind::TimedOut,
                                    "native plugin host pipe read timed out",
                                ));
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
        }
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdio_host_io_is_constructible() {
        let _ = stdio_host_io();
    }
}
