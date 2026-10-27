//! CPU time counters and physical memory snapshots.

use std::io;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
/// Cumulative CPU times in 100-nanosecond ticks. `kernel` includes `idle`.
pub struct CpuTimes {
    pub idle: u64,
    pub kernel: u64,
    pub user: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Physical memory in bytes and the operating system's memory load percentage.
pub struct MemoryStatus {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub load_percent: u32,
}

/// Returns CPU usage in 0..=100 for two valid cumulative samples.
///
/// An all-zero `previous` is the uninitialized, first-sample baseline. It, counter
/// rollback, impossible idle times and zero elapsed CPU time return `None`.
/// Callers must replace the baseline with the current sample after a reset.
pub fn cpu_usage(previous: CpuTimes, current: CpuTimes) -> Option<f64> {
    if previous == CpuTimes::default()
        || previous.idle > previous.kernel
        || current.idle > current.kernel
    {
        return None;
    }
    let idle = current.idle.checked_sub(previous.idle)?;
    let kernel = current.kernel.checked_sub(previous.kernel)?;
    let user = current.user.checked_sub(previous.user)?;
    if idle > kernel {
        return None;
    }
    let total = u128::from(kernel) + u128::from(user);
    if total == 0 {
        return None;
    }
    let busy = total - u128::from(idle);
    Some(busy as f64 / total as f64 * 100.0)
}

/// Reads Windows `GetSystemTimes` counters without maintaining a baseline.
///
/// With more than 64 logical processors, Windows reports the calling thread's
/// primary processor group. See [GetSystemTimes] for the platform contract.
///
/// [GetSystemTimes]: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getsystemtimes
#[cfg(windows)]
pub fn cpu_times() -> io::Result<CpuTimes> {
    windows::cpu_times()
}

/// Reads physical memory availability and load using `GlobalMemoryStatusEx`.
#[cfg(windows)]
pub fn memory_status() -> io::Result<MemoryStatus> {
    windows::memory_status()
}

/// CPU sampling requires Windows; no synthetic sample is returned.
#[cfg(not(windows))]
pub fn cpu_times() -> io::Result<CpuTimes> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "CPU sampling requires Windows",
    ))
}

/// Memory sampling requires Windows; no synthetic sample is returned.
#[cfg(not(windows))]
pub fn memory_status() -> io::Result<MemoryStatus> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Memory sampling requires Windows",
    ))
}

#[cfg(windows)]
mod windows {
    use super::*;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    pub(super) fn cpu_times() -> io::Result<CpuTimes> {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: All three pointers refer to distinct, initialized FILETIME
        // values, valid for writes throughout this synchronous Win32 call.
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(CpuTimes {
            idle: filetime_ticks(idle),
            kernel: filetime_ticks(kernel),
            user: filetime_ticks(user),
        })
    }

    pub(super) fn memory_status() -> io::Result<MemoryStatus> {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: status is writable for the full structure size, and dwLength
        // is initialized as required by GlobalMemoryStatusEx. It outlives the call.
        if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(from_memory_status(status))
    }

    fn filetime_ticks(value: FILETIME) -> u64 {
        (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
    }

    fn from_memory_status(value: MEMORYSTATUSEX) -> MemoryStatus {
        MemoryStatus {
            total_bytes: value.ullTotalPhys,
            available_bytes: value.ullAvailPhys,
            load_percent: value.dwMemoryLoad,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn filetime_preserves_both_words() {
            let value = FILETIME {
                dwLowDateTime: 0x89ab_cdef,
                dwHighDateTime: 0x0123_4567,
            };
            assert_eq!(filetime_ticks(value), 0x0123_4567_89ab_cdef);
        }

        #[test]
        fn memory_preserves_physical_bytes_and_os_load() {
            let value = MEMORYSTATUSEX {
                dwMemoryLoad: 73,
                ullTotalPhys: 64 * 1024 * 1024 * 1024,
                ullAvailPhys: 17 * 1024 * 1024 * 1024,
                ..Default::default()
            };
            assert_eq!(
                from_memory_status(value),
                MemoryStatus {
                    total_bytes: 64 * 1024 * 1024 * 1024,
                    available_bytes: 17 * 1024 * 1024 * 1024,
                    load_percent: 73,
                }
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times(idle: u64, kernel: u64, user: u64) -> CpuTimes {
        CpuTimes { idle, kernel, user }
    }

    #[test]
    fn usage_subtracts_idle_already_included_in_kernel() {
        assert_eq!(
            cpu_usage(times(100, 200, 100), times(130, 250, 150)),
            Some(70.0)
        );
    }

    #[test]
    fn first_sample_without_baseline_has_no_usage() {
        assert_eq!(cpu_usage(CpuTimes::default(), times(30, 50, 50)), None);
    }

    #[test]
    fn zero_interval_has_no_usage() {
        let sample = times(10, 20, 30);
        assert_eq!(cpu_usage(sample, sample), None);
    }

    #[test]
    fn any_counter_rollback_invalidates_usage() {
        let previous = times(10, 20, 30);
        for current in [times(9, 21, 31), times(11, 19, 31), times(11, 21, 29)] {
            assert_eq!(cpu_usage(previous, current), None);
        }
    }

    #[test]
    fn idle_cannot_exceed_cumulative_kernel_time() {
        assert_eq!(cpu_usage(times(11, 10, 5), times(12, 20, 6)), None);
        assert_eq!(cpu_usage(times(1, 2, 5), times(21, 20, 6)), None);
    }

    #[test]
    fn interval_idle_cannot_exceed_interval_kernel_time() {
        assert_eq!(cpu_usage(times(10, 100, 5), times(30, 110, 105)), None);
    }

    #[test]
    fn fully_idle_interval_is_zero_percent() {
        assert_eq!(cpu_usage(times(10, 20, 5), times(110, 120, 5)), Some(0.0));
    }

    #[test]
    fn fully_busy_interval_is_one_hundred_percent() {
        assert_eq!(
            cpu_usage(times(10, 20, 5), times(10, 120, 105)),
            Some(100.0)
        );
    }

    #[test]
    fn user_time_only_interval_is_valid() {
        assert_eq!(cpu_usage(times(10, 20, 5), times(10, 20, 55)), Some(100.0));
    }

    #[test]
    fn interval_sum_does_not_overflow_u64() {
        assert_eq!(
            cpu_usage(times(1, 1, 1), times(1, u64::MAX, u64::MAX)),
            Some(100.0)
        );
    }

    #[test]
    fn fractional_usage_keeps_precision() {
        let actual = cpu_usage(times(1, 10, 1), times(2, 12, 2)).unwrap();
        assert!((actual - 200.0 / 3.0).abs() < 1e-10);
    }

    #[cfg(windows)]
    #[test]
    fn native_cpu_and_memory_sampling_returns_physical_values() {
        let cpu = cpu_times().expect("GetSystemTimes should succeed");
        assert!(cpu.kernel >= cpu.idle);
        assert!(cpu.kernel > 0 || cpu.user > 0);
        let memory = memory_status().expect("GlobalMemoryStatusEx should succeed");
        assert!(memory.total_bytes > 0);
        assert!(memory.available_bytes <= memory.total_bytes);
        assert!(memory.load_percent <= 100);
    }

    #[cfg(not(windows))]
    #[test]
    fn unsupported_system_does_not_fabricate_counters() {
        assert_eq!(cpu_times().unwrap_err().kind(), io::ErrorKind::Unsupported);
        assert_eq!(
            memory_status().unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
}
