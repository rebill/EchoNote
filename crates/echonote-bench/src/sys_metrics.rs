//! Process resource usage: peak resident memory and CPU time.

#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    pub peak_rss_mb: f64,
    /// User plus system CPU time of the whole process, in seconds.
    pub cpu_s: f64,
}

#[cfg(unix)]
pub fn usage() -> Usage {
    // SAFETY: getrusage only writes into the zeroed struct we pass.
    let raw = unsafe {
        let mut raw: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut raw);
        raw
    };
    let tv = |value: libc::timeval| value.tv_sec as f64 + value.tv_usec as f64 / 1e6;
    // ru_maxrss is bytes on macOS and kilobytes on Linux.
    let rss_bytes = if cfg!(target_os = "macos") {
        raw.ru_maxrss as f64
    } else {
        raw.ru_maxrss as f64 * 1024.0
    };
    Usage {
        peak_rss_mb: rss_bytes / 1_048_576.0,
        cpu_s: tv(raw.ru_utime) + tv(raw.ru_stime),
    }
}

#[cfg(windows)]
pub fn usage() -> Usage {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    let filetime_s = |value: FILETIME| {
        ((value.dwHighDateTime as u64) << 32 | value.dwLowDateTime as u64) as f64 / 1e7
    };
    // SAFETY: both calls only write into the zeroed structs we pass for the current process.
    unsafe {
        let process = GetCurrentProcess();
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        GetProcessMemoryInfo(process, &mut counters, counters.cb);
        let (mut created, mut exited, mut kernel, mut user) =
            std::mem::zeroed::<(FILETIME, FILETIME, FILETIME, FILETIME)>();
        GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user);
        Usage {
            peak_rss_mb: counters.PeakWorkingSetSize as f64 / 1_048_576.0,
            cpu_s: filetime_s(kernel) + filetime_s(user),
        }
    }
}

/// Current resident memory in MiB (steady state, unlike the peak in [`usage`]).
#[cfg(target_os = "macos")]
pub fn rss_now_mb() -> f64 {
    // SAFETY: proc_pidinfo writes at most size_of::<proc_taskinfo>() bytes into `info`.
    let info = unsafe {
        let mut info: libc::proc_taskinfo = std::mem::zeroed();
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        );
        info
    };
    info.pti_resident_size as f64 / 1_048_576.0
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn rss_now_mb() -> f64 {
    let pages = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|statm| {
            statm
                .split_whitespace()
                .nth(1)
                .and_then(|value| value.parse::<f64>().ok())
        })
        .unwrap_or(0.0);
    // SAFETY: sysconf has no preconditions.
    pages * unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as f64 / 1_048_576.0
}

#[cfg(windows)]
pub fn rss_now_mb() -> f64 {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: GetProcessMemoryInfo only writes into the zeroed struct for the current process.
    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb);
        counters.WorkingSetSize as f64 / 1_048_576.0
    }
}

pub fn logical_cpus() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
}
