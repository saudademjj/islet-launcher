use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::core::platform::{java_executable, normalize_arch, quiet_command, simplify_verbatim};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstallation {
    pub path: String,
    pub version: String,
    pub major: u32,
    pub arch: String,
    pub vendor: String,
}

pub fn find_all() -> Vec<JavaInstallation> {
    find_all_in(&[], &[])
}

pub fn find_all_in(roots: &[PathBuf], explicit: &[PathBuf]) -> Vec<JavaInstallation> {
    let mut candidates = candidate_paths();
    for root in roots {
        scan_jvms(root, 7, &mut candidates);
    }
    candidates.extend(explicit.iter().map(|p| resolve_binary(p)));
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for candidate in candidates {
        let Ok(canonical) = std::fs::canonicalize(&candidate) else {
            continue;
        };
        if seen.insert(canonical.clone()) {
            // The path is shown, saved and used to launch, so keep it free of
            // the Windows `\\?\` prefix that canonicalize adds.
            let path = canonical.to_str().map_or_else(
                || canonical.clone(),
                |p| PathBuf::from(simplify_verbatim(p)),
            );
            unique.push(path);
        }
    }
    let mut result = if unique.len() <= 1 {
        unique.iter().filter_map(|p| inspect(p)).collect()
    } else {
        use std::sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        };
        let next = AtomicUsize::new(0);
        let found = Mutex::new(Vec::with_capacity(unique.len()));
        let workers = unique.len().min(6);
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| loop {
                    let idx = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = unique.get(idx) else {
                        break;
                    };
                    if let Some(java) = inspect(path) {
                        found.lock().unwrap_or_else(|e| e.into_inner()).push(java);
                    }
                });
            }
        });
        found.into_inner().unwrap_or_else(|e| e.into_inner())
    };
    result.sort_by(|a, b| a.major.cmp(&b.major).then_with(|| a.path.cmp(&b.path)));
    result
}

pub fn find_for_major(major: u32, arch: &str) -> Option<JavaInstallation> {
    find_all()
        .into_iter()
        .find(|java| java.major == major && same_arch(&java.arch, arch))
}

pub fn compatible_arch(arch: &str) -> bool {
    same_arch(arch, std::env::consts::ARCH)
}

pub fn same_arch(a: &str, b: &str) -> bool {
    normalize_arch(a) == normalize_arch(b)
}

pub fn resolve_binary(path: &Path) -> PathBuf {
    if path.is_dir() {
        let direct = path.join(java_executable());
        if direct.is_file() {
            return direct;
        }
        return path.join("bin").join(java_executable());
    }
    if path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("javaw.exe"))
    {
        return path.with_file_name("java.exe");
    }
    path.to_path_buf()
}

pub fn inspect(java_binary: &Path) -> Option<JavaInstallation> {
    let resolved = resolve_binary(java_binary);
    let java_binary = resolved.as_path();
    if !java_binary.is_file() {
        return None;
    }
    let mut child = quiet_command(java_binary)
        .args(["-XshowSettings:properties", "-version"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    // Windows pipes buffer only a few KiB and -XshowSettings prints the whole
    // PATH, so read both streams while waiting rather than after exit.
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let deadline = Instant::now() + Duration::from_secs(4);
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !status.success() {
        return None;
    }
    let collect = |reader: Option<std::thread::JoinHandle<Vec<u8>>>| {
        reader
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    };
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&collect(stdout)),
        String::from_utf8_lossy(&collect(stderr))
    );
    parse_properties(java_binary, &text)
}

fn drain(mut stream: impl std::io::Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    const MAX_CAPTURE: usize = 2 * 1024 * 1024;
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let room = MAX_CAPTURE.saturating_sub(bytes.len());
                    if room > 0 {
                        bytes.extend_from_slice(&buf[..n.min(room)]);
                    }
                }
            }
        }
        bytes
    })
}

fn parse_properties(path: &Path, text: &str) -> Option<JavaInstallation> {
    let get = |key: &str| {
        text.lines().find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| value.trim().to_string())
        })
    };
    let version = get("java.version")?;
    let specification = get("java.specification.version").unwrap_or_else(|| version.clone());
    let major = parse_major(&specification)?;
    Some(JavaInstallation {
        path: path.to_string_lossy().into_owned(),
        version,
        major,
        arch: normalize_arch(&get("os.arch")?).to_string(),
        vendor: get("java.vendor").unwrap_or_default(),
    })
}

fn parse_major(value: &str) -> Option<u32> {
    value
        .trim()
        .strip_prefix("1.")
        .unwrap_or(value.trim())
        .split(['.', '-', '+'])
        .next()?
        .parse()
        .ok()
}

fn scan_jvms(root: &Path, depth: usize, candidates: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let bin = root.join("bin").join(java_executable());
    let bundle = root.join("Contents/Home/bin").join(java_executable());
    let found_bin = bin.is_file();
    let found_bundle = bundle.is_file();
    if found_bin {
        candidates.push(bin);
    }
    if found_bundle {
        candidates.push(bundle);
    }
    // Once a directory is already a JDK/JRE home, skip recursing into its
    // lib/, jmods/, legal/, include/, and conf/ trees.
    if found_bin || found_bundle {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|ty| ty.is_dir()) {
                scan_jvms(&entry.path(), depth - 1, candidates);
            }
        }
    }
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for key in ["JAVA_HOME", "JDK_HOME"] {
        if let Some(root) = std::env::var_os(key) {
            candidates.push(PathBuf::from(root).join("bin").join(java_executable()));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|p| p.join(java_executable())));
    }
    #[cfg(windows)]
    {
        for key in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(root) = std::env::var_os(key) {
                let root = PathBuf::from(root);
                for vendor in [
                    "Java",
                    "Eclipse Adoptium",
                    "Eclipse Foundation",
                    "Microsoft",
                    "Amazon Corretto",
                    "Zulu",
                    "BellSoft",
                ] {
                    scan_jvms(&root.join(vendor), 3, &mut candidates);
                }
            }
        }
        if let Some(home) = std::env::var_os("USERPROFILE") {
            let home = PathBuf::from(home);
            scan_jvms(&home.join(".jdks"), 3, &mut candidates);
            scan_jvms(&home.join(".jabba/jdk"), 3, &mut candidates);
        }
        for key in ["APPDATA", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(key) {
                let root = PathBuf::from(root);
                for folder in [
                    ".minecraft/runtime",
                    "Programs/Eclipse Adoptium",
                    "Programs/Java",
                    "Programs/Microsoft",
                    "Packages/Microsoft.4297127D64EC6_8wekyb3d8bbwe/LocalCache/Local/runtime",
                ] {
                    scan_jvms(&root.join(folder), 6, &mut candidates);
                }
            }
        }
        for key in [
            r"HKLM\SOFTWARE\JavaSoft",
            r"HKLM\SOFTWARE\WOW6432Node\JavaSoft",
        ] {
            if let Ok(output) = quiet_command("reg.exe").args(["query", key, "/s"]).output() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    if line.trim_start().starts_with("JavaHome")
                        && let Some((_, value)) = line.split_once("REG_SZ")
                    {
                        candidates.push(PathBuf::from(value.trim()).join("bin/java.exe"));
                    }
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        scan_jvms(
            Path::new("/Library/Java/JavaVirtualMachines"),
            3,
            &mut candidates,
        );
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            scan_jvms(&home.join("Library/Java/JavaVirtualMachines"), 3, &mut candidates);
            scan_jvms(&home.join(".jdks"), 3, &mut candidates);
            scan_jvms(&home.join(".sdkman/candidates/java"), 3, &mut candidates);
        }
        for root in ["/opt/homebrew/opt", "/usr/local/opt"] {
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries
                    .flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with("openjdk"))
                {
                    candidates.push(entry.path().join("bin/java"));
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        scan_jvms(Path::new("/usr/lib/jvm"), 3, &mut candidates);
        scan_jvms(Path::new("/usr/lib64/jvm"), 3, &mut candidates);
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            scan_jvms(&home.join(".jdks"), 3, &mut candidates);
            scan_jvms(&home.join(".sdkman/candidates/java"), 3, &mut candidates);
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_windows_java_with_spaces_and_arch_alias() {
        let java = parse_properties(Path::new(r"C:\Program Files\Java\bin\java.exe"), "java.version = 21.0.4\njava.specification.version = 21\nos.arch = amd64\njava.vendor = Eclipse Adoptium").unwrap();
        assert_eq!(java.major, 21);
        assert_eq!(java.arch, "x86_64");
        assert!(java.path.contains("Program Files"));
        assert_eq!(parse_major("1.8.0_452"), Some(8));
        assert_eq!(parse_major("25-ea"), Some(25));
    }

    /// A probe whose output exceeds the pipe buffer must not stall until the
    /// timeout. Windows buffers are far smaller than this 300 KB, so a long
    /// PATH in `java.library.path` hits the same failure there.
    #[cfg(unix)]
    #[test]
    fn probe_with_output_larger_than_pipe_buffer_finishes() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let java = dir.path().join("java");
        std::fs::write(
            &java,
            "#!/bin/sh\nhead -c 300000 /dev/zero | tr '\\0' x >&2\nprintf '\\njava.version = 21.0.4\\njava.specification.version = 21\\nos.arch = aarch64\\n' >&2\n",
        )
        .unwrap();
        std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = Instant::now();
        let found = inspect(&java).expect("probe should finish");
        assert_eq!(found.major, 21);
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
