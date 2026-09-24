use std::process::Child;

const HOST_MEMORY_LIMIT_BYTES: usize = 128 * 1024 * 1024;
const HOST_CPU_RATE_BASIS_POINTS: u32 = 2_000;

/// 作业对象限制标志。`KILL_ON_JOB_CLOSE` 保证 Flux 异常退出（崩溃、panic=abort、
/// 被任务管理器结束）时，作业内的 `--plugin-host` 子进程随之终止，
/// 不会成为不受内存 / CPU 限制约束的孤儿进程。
/// 注：`BitOr` 非 const fn，故 const 上下文用元组字段位或构造。
#[cfg(windows)]
const HOST_JOB_LIMIT_FLAGS: JOB_OBJECT_LIMIT = JOB_OBJECT_LIMIT(
    JOB_OBJECT_LIMIT_PROCESS_MEMORY.0
        | JOB_OBJECT_LIMIT_JOB_MEMORY.0
        | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.0,
);

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
    JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
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
                    LimitFlags: HOST_JOB_LIMIT_FLAGS,
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
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
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
                    size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
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

    /// 作业对象必须带 KILL_ON_JOB_CLOSE：宿主进程随 Flux 异常退出一并终止，
    /// 否则崩溃后会留下不受内存 / CPU 限制的孤儿进程（审查发现 3）。
    #[cfg(windows)]
    #[test]
    fn job_limit_flags_kill_host_on_close() {
        assert!(
            HOST_JOB_LIMIT_FLAGS.contains(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE),
            "JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE 缺失：Flux 崩溃时插件宿主会成为孤儿进程"
        );
        assert!(
            HOST_JOB_LIMIT_FLAGS.contains(JOB_OBJECT_LIMIT_PROCESS_MEMORY),
            "PROCESS_MEMORY 限制缺失"
        );
        assert!(
            HOST_JOB_LIMIT_FLAGS.contains(JOB_OBJECT_LIMIT_JOB_MEMORY),
            "JOB_MEMORY 限制缺失"
        );
    }
}
