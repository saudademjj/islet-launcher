use std::process::Command;

/// Physical memory reported by the OS. `None` when the platform cannot report
/// it; callers fall back to [`FALLBACK_TOTAL_MEMORY_MB`].
pub fn total_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut value: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        let result = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                &mut value as *mut u64 as *mut libc::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        (result == 0 && len == std::mem::size_of::<u64>() && value > 0).then_some(value)
    }

    #[cfg(target_os = "linux")]
    {
        let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGE_SIZE) };
        if pages > 0 && page_size > 0 {
            Some((pages as u64).saturating_mul(page_size as u64))
        } else {
            None
        }
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        (ok != 0 && status.ullTotalPhys > 0).then_some(status.ullTotalPhys)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        None
    }
}

/// Used when the OS does not report physical memory. 8 GB is common enough
/// that the automatic recommendation still lands on a usable value.
pub const FALLBACK_TOTAL_MEMORY_MB: u64 = 8192;

/// Total memory in MB, with a safe fallback for unsupported platforms.
pub fn total_memory_mb() -> u64 {
    total_memory_bytes()
        .map(|bytes| bytes / (1024 * 1024))
        .filter(|mb| *mb > 0)
        .unwrap_or(FALLBACK_TOTAL_MEMORY_MB)
}

/// Automatic `-Xmx` value for a machine of `total_mb` with `mod_count` jars.
///
/// The tiers favour a comfortable allocation on small machines and leave room
/// for the OS. The 60% ceiling wins over the 1024 MB floor on very small
/// machines (a 1 GB machine gets 512 MB, not 1024 MB). Values are rounded down
/// to whole 512 MB steps; 512 MB is the absolute floor because a zero-megabyte
/// heap cannot start Java.
pub fn recommended_memory_mb(total_mb: u64, mod_count: usize) -> u32 {
    let base = match total_mb {
        0..=4096 => 1536,
        4097..=8192 => 2560,
        8193..=16384 => 4096,
        16385..=32768 => 6144,
        _ => 8192,
    };
    let mod_bonus = if mod_count > 150 {
        2048
    } else if mod_count > 50 {
        1024
    } else {
        0
    };
    let cap = total_mb.saturating_mul(60) / 100;
    let desired = (base + mod_bonus).min(cap);
    ((desired / 512) * 512).max(512) as u32
}

pub fn java_executable() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

pub fn adoptium_os() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "mac",
        _ => "linux",
    }
}

pub fn normalize_arch(value: &str) -> &str {
    match value {
        "amd64" | "x86_64" | "x64" => "x86_64",
        "aarch64" | "arm64" => "aarch64",
        "i386" | "i686" | "x86" => "x86",
        other => other,
    }
}

pub fn adoptium_arch() -> &'static str {
    adoptium_arch_for(std::env::consts::ARCH)
}

/// Adoptium's name for an architecture, also the folder a runtime lives in.
pub fn adoptium_arch_for(arch: &str) -> &'static str {
    match normalize_arch(arch) {
        "aarch64" => "aarch64",
        "x86" => "x86",
        _ => "x64",
    }
}

/// Whether Intel programs can run here: always on Intel, and on Apple
/// Silicon once Rosetta 2 is installed.
pub fn can_run_arch(arch: &str) -> bool {
    let host = normalize_arch(std::env::consts::ARCH);
    let wanted = normalize_arch(arch);
    if wanted == host {
        return true;
    }
    cfg!(target_os = "macos")
        && host == "aarch64"
        && wanted == "x86_64"
        && std::path::Path::new("/Library/Apple/usr/libexec/oah/libRosettaRuntime").exists()
}

/// `fs::canonicalize` returns verbatim `\\?\C:\...` paths on Windows. Java 8
/// and many tools mishandle that prefix, so drive paths short enough for the
/// classic form are converted back. Other paths are returned unchanged.
pub fn simplify_verbatim(path: &str) -> &str {
    let Some(rest) = path.strip_prefix(r"\\?\") else {
        return path;
    };
    let bytes = rest.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes[2] == b'\\'
        && rest.len() < 260
    {
        rest
    } else {
        path
    }
}

/// Java probes and taskkill must not flash console windows on Windows.
pub fn quiet_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    let _ = &mut command;
    command
}

/// Asks Windows to run `exe` on the high-performance GPU, as Settings →
/// System → Display → Graphics does. The choice is per user and per
/// program, and does nothing on a computer with one GPU.
#[cfg(windows)]
pub fn prefer_dedicated_gpu(exe: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ};

    let wide = |text: &std::ffi::OsStr| text.encode_wide().chain([0]).collect::<Vec<u16>>();
    let key_name = wide(std::ffi::OsStr::new(r"Software\Microsoft\DirectX\UserGpuPreferences"));
    let value_name = wide(exe.as_os_str());
    let data = wide(std::ffi::OsStr::new("GpuPreference=2;"));
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: every pointer is to a live, NUL-terminated buffer or a local.
    unsafe {
        let status = RegCreateKeyExW(HKEY_CURRENT_USER, key_name.as_ptr(), 0, std::ptr::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, std::ptr::null(), &mut key, std::ptr::null_mut());
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32));
        }
        let status = RegSetValueExW(key, value_name.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32);
        RegCloseKey(key);
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32));
        }
    }
    Ok(())
}

/// Linux renders on the first GPU unless told otherwise; with a second one
/// present (a laptop's discrete card), Mesa's `DRI_PRIME=1` picks it.
#[cfg(target_os = "linux")]
pub fn has_second_gpu() -> bool {
    std::fs::read_dir("/sys/class/drm").is_ok_and(|entries| {
        entries
            .flatten()
            .filter(|e| e.file_name().to_str().and_then(|n| n.strip_prefix("card")).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
            .count()
            >= 2
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_arch_aliases_match_rust_arch_names() {
        assert_eq!(normalize_arch("amd64"), normalize_arch("x86_64"));
        assert_eq!(normalize_arch("arm64"), normalize_arch("aarch64"));
        assert_ne!(normalize_arch("x86"), normalize_arch("amd64"));
    }

    #[test]
    fn windows_manifest_requests_common_controls_v6() {
        let manifest = include_str!("../../assets/windows/app.manifest");
        assert!(manifest.contains("Microsoft.Windows.Common-Controls"));
        assert!(manifest.contains(r#"version="6.0.0.0""#));
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    fn supported_platforms_report_physical_memory() {
        let bytes = total_memory_bytes().expect("the OS should report physical memory");
        assert!(bytes >= 512 * 1024 * 1024);
    }

    #[test]
    fn memory_recommendation_follows_the_tiers() {
        assert_eq!(recommended_memory_mb(4096, 0), 1536);
        assert_eq!(recommended_memory_mb(8192, 0), 2560);
        assert_eq!(recommended_memory_mb(16384, 0), 4096);
        assert_eq!(recommended_memory_mb(32768, 0), 6144);
        assert_eq!(recommended_memory_mb(65536, 0), 8192);
        // Just above 4 GB the next tier's 2560 MB is still capped by 60%
        // (2458 MB) and rounds down to 2048 MB.
        assert_eq!(recommended_memory_mb(4097, 0), 2048);
        assert_eq!(recommended_memory_mb(8193, 0), 4096);
    }

    #[test]
    fn mod_count_adds_memory_above_the_thresholds() {
        assert_eq!(recommended_memory_mb(16384, 50), 4096);
        assert_eq!(recommended_memory_mb(16384, 51), 5120);
        assert_eq!(recommended_memory_mb(16384, 150), 5120);
        assert_eq!(recommended_memory_mb(16384, 151), 6144);
        // The 60% ceiling still applies after the mod bonus.
        assert_eq!(recommended_memory_mb(8192, 151), 4608);
        assert_eq!(recommended_memory_mb(4096, 151), 2048);
    }

    #[test]
    fn sixty_percent_cap_wins_on_tiny_machines() {
        // 2 GB: 60% is 1228 MB, rounded down to 1024 MB.
        assert_eq!(recommended_memory_mb(2048, 0), 1024);
        // 1 GB: 60% is 614 MB, below the normal 1024 MB floor, so 512 MB.
        assert_eq!(recommended_memory_mb(1024, 0), 512);
        // Even smaller machines keep a 512 MB floor rather than 0.
        assert_eq!(recommended_memory_mb(512, 0), 512);
    }

    #[test]
    fn detected_memory_is_positive() {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(total_memory_bytes().is_some_and(|bytes| bytes > 0));
        assert!(total_memory_mb() > 0);
    }

    #[test]
    fn verbatim_drive_paths_become_plain_windows_paths() {
        assert_eq!(
            simplify_verbatim(r"\\?\C:\Program Files\Java\bin\java.exe"),
            r"C:\Program Files\Java\bin\java.exe"
        );
        assert_eq!(
            simplify_verbatim(r"\\?\UNC\server\share\java.exe"),
            r"\\?\UNC\server\share\java.exe"
        );
        assert_eq!(simplify_verbatim("/usr/bin/java"), "/usr/bin/java");
    }
}
