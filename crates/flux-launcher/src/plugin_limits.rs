use std::process::Child;

const HOST_MEMORY_LIMIT_BYTES: usize = 128 * 1024 * 1024;
const HOST_CPU_RATE_BASIS_POINTS: u32 = 2_000;

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    CreateJobObjectW, JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
    JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
    JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};

pub struct HostResourceGuard {
    #[cfg(windows)]
    job: HANDLE,
}

impl HostResourceGuard {
    pub fn attach(child: &Child) -> Result<Self, String> {
        #[cfg(windows)]
        {
            let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
                .map_err(|error| format!("CreateJobObjectW failed: {error}"))?;
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                    LimitFlags: JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_JOB_MEMORY,
                    ..Default::default()
                },
                ProcessMemoryLimit: HOST_MEMORY_LIMIT_BYTES,
                JobMemoryLimit: HOST_MEMORY_LIMIT_BYTES,
                ..Default::default()
            };
            let result = unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &mut limits as *mut _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if let Err(error) = result {
                unsafe {
                    let _ = CloseHandle(job);
                }
                return Err(format!(
                    "SetInformationJobObject memory limit failed: {error}"
                ));
            }
            let mut cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
                ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                    | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
                Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 {
                    CpuRate: HOST_CPU_RATE_BASIS_POINTS,
                },
            };
            if let Err(error) = unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectCpuRateControlInformation,
                    &mut cpu as *mut _ as *const _,
                    std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                )
            } {
                unsafe {
                    let _ = CloseHandle(job);
                }
                return Err(format!("SetInformationJobObject CPU limit failed: {error}"));
            }
            let process = HANDLE(child.as_raw_handle() as _);
            if let Err(error) = unsafe {
                windows::Win32::System::JobObjects::AssignProcessToJobObject(job, process)
            } {
                unsafe {
                    let _ = CloseHandle(job);
                }
                return Err(format!("AssignProcessToJobObject failed: {error}"));
            }
            Ok(Self { job })
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            Ok(Self {})
        }
    }
}

#[cfg(windows)]
impl Drop for HostResourceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.job);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_limits_are_bounded() {
        assert_eq!(HOST_MEMORY_LIMIT_BYTES, 128 * 1024 * 1024);
        let cpu_rate = std::hint::black_box(HOST_CPU_RATE_BASIS_POINTS);
        assert!(cpu_rate <= 10_000);
    }
}
