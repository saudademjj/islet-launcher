//! Crash diagnosis for a single launch. The analysis is a pure string scan so
//! it can be unit tested without the launcher services or a real game, and so
//! a malformed log can never abort the UI.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub fix: Option<Fix>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Fix {
    UseJava { major: u32 },
    RaiseMemory { mb: u32 },
    InstallMod { query: String },
    OpenMods,
    OpenTools,
    Repair,
    None,
}

/// Diagnose a crash from the launcher-captured log and, when present, the
/// game's crash report. Rules run in priority order and may contribute several
/// diagnoses (one per missing mod); duplicate findings are dropped.
pub fn analyze(log: &str, crash_report: Option<&str>) -> Vec<Diagnosis> {
    analyze_with_memory_limit(log, crash_report, 0)
}

/// Like [`analyze`], but caps memory advice by the machine's physical memory.
/// The detail text is built with the capped value, so the number the user
/// reads is exactly the one the offered fix will set.
pub fn analyze_with_memory_limit(
    log: &str,
    crash_report: Option<&str>,
    total_mb: u64,
) -> Vec<Diagnosis> {
    let mut text = String::with_capacity(log.len() + crash_report.map_or(0, str::len) + 1);
    text.push_str(log);
    if let Some(report) = crash_report {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(report);
    }
    diagnose(&text, total_mb)
}

fn diagnose(text: &str, total_mb: u64) -> Vec<Diagnosis> {
    let lower = text.to_ascii_lowercase();
    let mut diagnoses = Vec::new();

    // 1. A class compiled for a newer Java is the clearest early-launch error.
    if let Some(version) = class_file_version(&lower)
        && let Some(major) = class_major(version)
    {
        diagnoses.push(Diagnosis {
            kind: "javaVersion".to_string(),
            title: crate::tr!("需要 Java {major}", "Needs Java {major}"),
            detail: crate::tr!("当前 Java 版本过低，该游戏或模组需要 Java {major}。准备完成后启动器会自动为此世界切换。", "The current Java version is too old; this game or mod requires Java {major}. Preparing it will switch this world automatically."
            ),
            fix: Some(Fix::UseJava { major }),
        });
    }

    // 2. Heap exhaustion needs more memory; failing to reserve address space
    //    usually means the configured heap is already too large.
    if let Some(diagnosis) = memory_diagnosis(&lower, total_mb) {
        diagnoses.push(diagnosis);
    }

    // 3/4. Fabric, Forge, and NeoForge report missing or mismatched dependencies
    //      in different formats.
    let mut missing = HashSet::new();
    let mut mismatched = HashSet::new();
    for id in fabric_missing(&lower, text) {
        push_missing(&mut diagnoses, &mut missing, &id);
    }
    let (forge_miss, forge_mismatch) = forge_dependencies(&lower, text);
    for id in forge_miss {
        push_missing(&mut diagnoses, &mut missing, &id);
    }
    let (neo_miss, neo_mismatch) = neoforge_dependencies(&lower, text);
    for id in neo_miss {
        push_missing(&mut diagnoses, &mut missing, &id);
    }
    for id in fabric_version_mismatches(&lower, text)
        .into_iter()
        .chain(forge_mismatch)
        .chain(neo_mismatch)
    {
        push_version_mismatch(&mut diagnoses, &missing, &mut mismatched, &id);
    }

    // 5. Two versions of the same mod.
    if lower.contains("duplicatemodsfoundexception")
        || lower.contains("found duplicate mods")
        || lower.contains("duplicate mods")
    {
        diagnoses.push(Diagnosis {
            kind: "duplicateMods".to_string(),
            title: crate::core::i18n::pick("发现重复的模组", "Duplicate mods").to_string(),
            detail:
                crate::core::i18n::pick("同一模组安装了多个版本，请打开「模组」页，每个模组只保留一个。", "Multiple versions of the same mod are installed. Open the Mods tab and keep only one per mod.")
                    .to_string(),
            fix: Some(Fix::OpenMods),
        });
    }

    // 5b. OptiFine / OptiFabric installed alongside Sodium / Iris / Embeddium / Oculus,
    //     or Fabric reporting explicit "breaks with" mod incompatibilities.
    let optifine_conflict = has_optifine_render_conflict(&lower);
    if optifine_conflict {
        diagnoses.push(Diagnosis {
            kind: "modConflict".to_string(),
            title: crate::core::i18n::pick("OptiFine 与光影/优化模组冲突", "OptiFine conflicts with rendering mods").to_string(),
            detail: crate::core::i18n::pick(
                "OptiFine 不能与 Sodium、Iris、Embeddium、Rubidium 或 Oculus 同时安装。请打开「模组」页移除 OptiFine（或改用 Sodium + Iris 组合）。",
                "OptiFine can't run alongside Sodium, Iris, Embeddium, Rubidium, or Oculus. Open Mods and remove OptiFine (or switch to Sodium + Iris).",
            )
            .to_string(),
            fix: Some(Fix::OpenMods),
        });
    }
    let mut conflict_mods = HashSet::new();
    for id in fabric_conflicts(&lower, text) {
        if optifine_conflict && matches!(id.as_str(), "optifine" | "optifabric" | "sodium" | "iris") {
            continue;
        }
        if !NON_MOD_IDS.contains(&id.as_str()) && conflict_mods.insert(id.clone()) {
            diagnoses.push(Diagnosis {
                kind: "modConflict".to_string(),
                title: crate::tr!("模组 {id} 与其他模组冲突", "Mod {id} conflicts with another mod"),
                detail: crate::tr!(
                    "加载器报告模组 {id} 与当前已安装的其他模组不兼容。请打开「模组」页禁用或移除冲突模组。",
                    "The loader reports that {id} is incompatible with another installed mod. Open Mods to disable or remove the conflicting mod."
                ),
                fix: Some(Fix::OpenMods),
            });
        }
    }

    // 6. A mixin that no longer matches the game or loader classes.
    let mut conflicted = HashSet::new();
    for id in mixin_conflicts(text) {
        if conflicted.insert(id.clone()) {
            diagnoses.push(Diagnosis {
                kind: "mixinConflict".to_string(),
                title: crate::tr!("模组 {id} 与当前版本不兼容", "Mod {id} doesn't fit this version"),
                detail: crate::tr!("模组 {id} 与当前游戏版本或加载器不兼容，请更新到适配版本或先禁用它。", "Mod {id} is incompatible with this Minecraft or loader version. Update it to a matching version or disable it for now."
                ),
                fix: Some(Fix::OpenMods),
            });
        }
    }

    // 6b. Server-only mod loaded in a client environment.
    if is_wrong_side_mod(&lower) {
        diagnoses.push(Diagnosis {
            kind: "wrongSideMod".to_string(),
            title: crate::core::i18n::pick("安装了仅限服务端的模组", "Server-only mod installed on client").to_string(),
            detail: crate::core::i18n::pick(
                "日志显示有模组尝试在客户端加载仅限服务端的代码（Invalid dist: CLIENT）。请打开「模组」页排查并移除仅服务端模组。",
                "A mod tried to load server-only code on the client (invalid dist: CLIENT). Open Mods and disable or remove server-only mods.",
            )
            .to_string(),
            fix: Some(Fix::OpenMods),
        });
    }

    // 6c. Corrupted NightConfig / TOML / JSON mod configuration file.
    if is_corrupt_config(&lower) {
        let file = config_file_name(text);
        let title = match file {
            Some(ref name) => crate::tr!("配置文件 {name} 已损坏", "Config file {name} is corrupted"),
            None => crate::core::i18n::pick("模组配置文件已损坏", "A mod config file is corrupted").to_string(),
        };
        diagnoses.push(Diagnosis {
            kind: "corruptConfig".to_string(),
            title,
            detail: crate::core::i18n::pick(
                "模组配置文件格式损坏导致无法读取。请前往世界管理的「工具 → 配置文件」检查或删除损坏的文件，游戏重启后会自动重建默认配置。",
                "A mod config file couldn't be parsed. Open Tools → Configs for this world to inspect or delete the broken file; the game will recreate a default copy on next launch.",
            )
            .to_string(),
            fix: Some(Fix::OpenTools),
        });
    }

    // 7. The driver cannot provide the OpenGL context the game needs.
    if lower.contains("glfw error 65542")
        || lower.contains("pixel format not accelerated")
        || text.lines().any(|line| {
            let line = line.to_ascii_lowercase();
            line.contains("opengl") && line.contains("not supported")
        })
    {
        diagnoses.push(Diagnosis {
            kind: "graphics".to_string(),
            title: crate::core::i18n::pick("显卡驱动太旧或不支持", "Graphics driver too old or unsupported").to_string(),
            detail:
                crate::core::i18n::pick("无法初始化显卡画面（OpenGL）。请更新显卡驱动；虚拟机或远程桌面通常无法运行游戏。", "Failed to initialize graphics (OpenGL). Update your graphics driver; virtual machines and remote desktops usually can't run the game.")
                    .to_string(),
            fix: Some(Fix::None),
        });
    }

    // 8. Missing or damaged jars after an interrupted download.
    if lower.contains("could not find or load main class")
        || (lower.contains("classnotfoundexception") && lower.contains("net.minecraft"))
        || lower.contains("invalid or corrupt jarfile")
        || lower.contains("java.util.zipexception")
        || lower.contains("zip file is corrupt")
    {
        diagnoses.push(Diagnosis {
            kind: "corruptFiles".to_string(),
            title: crate::core::i18n::pick("游戏文件缺失或损坏", "Game files missing or damaged").to_string(),
            detail:
                crate::core::i18n::pick("游戏核心文件缺失或损坏，请点击「校验并修复」重新下载，不会影响存档。", "Core game files are missing or damaged. Use Verify and repair to redownload them without touching your saves.")
                    .to_string(),
            fix: Some(Fix::Repair),
        });
    }

    // 8b. Port binding conflict (usually another server or game holding the port).
    if lower.contains("java.net.bindexception")
        || lower.contains("failed to bind to port")
        || lower.contains("address already in use: bind")
    {
        diagnoses.push(Diagnosis {
            kind: "portInUse".to_string(),
            title: crate::core::i18n::pick("网络端口已被占用", "Network port is already in use").to_string(),
            detail: crate::core::i18n::pick(
                "目标端口已被其他游戏或服务端进程占用。请关闭重复启动的实例，或更换端口后重试。",
                "Another game or server process is already using that port. Close the duplicate instance or switch to a different port.",
            )
            .to_string(),
            fix: Some(Fix::None),
        });
    }

    // 9. The Java VM itself crashed and left an hs_err_pid file: usually a
    // graphics driver, sometimes a mod's native library. Running out of
    // native memory has its own advice above.
    if lower.contains("a fatal error has been detected by the java runtime environment")
        && !lower.contains("insufficient memory for the java runtime environment")
    {
        let frame = problematic_frame(text);
        let driver = frame.as_deref().is_some_and(|frame| {
            let frame = frame.to_ascii_lowercase();
            DRIVER_LIBRARIES.iter().any(|library| frame.contains(library))
        });
        let (title, detail) = if driver {
            let frame = frame.clone().unwrap_or_default();
            (
                crate::core::i18n::pick("显卡驱动崩溃了", "The graphics driver crashed").to_string(),
                crate::tr!("显卡驱动发生崩溃（{frame}）。请更新显卡驱动，双显卡笔记本请确认使用独立显卡，或排查光影与渲染模组。", "Graphics driver crashed ({frame}). Update the driver, ensure a laptop uses its discrete GPU, or check shaders and rendering mods."),
            )
        } else {
            let at = frame.map(|frame| crate::tr!("（位置：{frame}）", " ({frame})")).unwrap_or_default();
            (
                crate::core::i18n::pick("Java 运行时崩溃了", "Java runtime crashed").to_string(),
                crate::tr!("Java 运行时意外崩溃{at}。常见原因包括显卡驱动过旧、近期安装的模组冲突或当前 Java 异常，建议更新驱动、排查新装模组或更换 Java。", "The Java runtime crashed unexpectedly{at}. Common causes are an outdated graphics driver, a recently added mod, or a faulty Java build. Try updating drivers, removing recent mods, or switching Java."),
            )
        };
        diagnoses.push(Diagnosis { kind: "jvmCrash".to_string(), title, detail, fix: Some(Fix::None) });
    }

    // 10. Keep early JVM failures visible even when no rule recognises them.
    if diagnoses.is_empty()
        && let Some(line) = first_exception_line(text)
    {
        diagnoses.push(Diagnosis {
            kind: "unknown".to_string(),
            title: crate::core::i18n::pick("未识别的错误", "Unrecognized error").to_string(),
            detail: crate::tr!("启动日志包含未识别的异常：{line}", "The log has an unrecognized exception: {line}"),
            fix: Some(Fix::None),
        });
    }

    diagnoses
}

/// Cap a suggested heap size by the machine's physical memory, leaving room
/// for the system. Keeps 512 MB steps and the instance-configuration limits.
pub fn clamp_memory_mb(suggested: u32, total_mb: u64) -> u32 {
    let suggested = suggested.clamp(512, 32768);
    if total_mb == 0 {
        return suggested;
    }
    let usable = total_mb
        .saturating_sub(1024)
        .min(total_mb.saturating_mul(3) / 4)
        .clamp(512, 32768);
    let capped = suggested.min(usable as u32);
    ((capped / 512).max(1) * 512).min(32768)
}

pub fn clamp_memory_fix(fix: &mut Fix, total_mb: u64) {
    if let Fix::RaiseMemory { mb } = fix {
        *mb = clamp_memory_mb(*mb, total_mb);
    }
}

fn class_file_version(lower: &str) -> Option<u32> {
    let class_error = lower.find("unsupportedclassversionerror")?;
    let marker = "class file version ";
    let rest = &lower[class_error..];
    let offset = rest.find(marker)?;
    leading_u32(&rest[offset + marker.len()..])
}

fn class_major(version: u32) -> Option<u32> {
    let major = version.checked_sub(44)?;
    (8..=64).contains(&major).then_some(major)
}

fn memory_diagnosis(lower: &str, total_mb: u64) -> Option<Diagnosis> {
    let current = current_memory_mb(lower);

    // Only a real heap exhaustion should raise the limit. Native-thread or
    // metaspace exhaustion usually means the opposite: the process is already
    // using more address space than the system can provide.
    let heap_exhausted = lower.contains("java heap space")
        || lower.contains("gc overhead limit exceeded")
        || (lower.contains("java.lang.outofmemoryerror")
            && !lower.contains("unable to create")
            && !lower.contains("metaspace")
            && !lower.contains("native memory"));
    if heap_exhausted {
        let suggested = current
            .map(|memory| round_up_512((u64::from(memory) * 3 / 2).max(1024)))
            .unwrap_or(6144)
            .min(32768);
        let next = clamp_memory_mb(suggested, total_mb);
        // If the machine cannot safely provide more than the instance already
        // uses, do not offer a value that is not actually an increase.
        if let Some(current) = current.filter(|current| next <= *current) {
            return Some(Diagnosis {
                kind: "memory".to_string(),
                title: crate::core::i18n::pick("内存不足", "Out of memory").to_string(),
                detail: crate::tr!("游戏内存已耗尽。当前已分配 {current} MB，已接近电脑安全上限（约 {next} MB），继续调高可能影响系统。请关闭其他程序、减少模组或调低渲染距离后重试。", "The game ran out of memory. It already uses {current} MB, near this computer's safe limit (about {next} MB). Close other programs, reduce mods, or lower render distance and try again."
                ),
                fix: Some(Fix::None),
            });
        }
        return Some(Diagnosis {
            kind: "memory".to_string(),
            title: crate::core::i18n::pick("内存不足", "Out of memory").to_string(),
            detail: crate::tr!("游戏内存已耗尽，常见于大型整合包或高渲染距离。建议将此世界内存提高到 {next} MB，必要时关闭其他程序后重试。", "The game ran out of memory, common with large modpacks or high render distance. Raise this world's memory to {next} MB and close other programs if needed."
            ),
            fix: Some(Fix::RaiseMemory { mb: next }),
        });
    }

    let cannot_reserve = lower.contains("could not reserve enough space")
        || lower.contains("insufficient memory for the java runtime environment")
        || lower.contains("native memory allocation (mmap) failed");
    if cannot_reserve {
        let suggested = current
            .map(|memory| round_down_512((memory / 2).max(512)))
            .unwrap_or(2048)
            .clamp(512, 32768);
        let next = clamp_memory_mb(suggested, total_mb);
        if let Some(current) = current.filter(|current| next >= *current) {
            return Some(Diagnosis {
                kind: "memory".to_string(),
                title: crate::core::i18n::pick("内存设置过高", "Memory set too high").to_string(),
                detail: crate::tr!("系统无法分配当前设定的 {current} MB 内存。请先关闭其他占用内存的程序，或手动调低该世界的内存设置。", "The system couldn't allocate the configured {current} MB of memory. Close other programs or lower this world's memory setting."
                ),
                fix: Some(Fix::None),
            });
        }
        return Some(Diagnosis {
            kind: "memory".to_string(),
            title: crate::core::i18n::pick("内存设置过高", "Memory set too high").to_string(),
            detail: crate::tr!("系统可用内存不足，无法分配当前设定的游戏内存。建议降到 {next} MB，或改用自动内存。", "Not enough free system memory for the current setting. Lower it to {next} MB or switch to automatic memory."
            ),
            fix: Some(Fix::RaiseMemory { mb: next }),
        });
    }
    None
}

fn current_memory_mb(lower: &str) -> Option<u32> {
    let mut from = 0;
    while let Some(haystack) = lower.get(from..) {
        let Some(offset) = haystack.find("-xmx") else {
            break;
        };
        let start = from + offset + "-xmx".len();
        let rest = &lower[start..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0
            && let Ok(value) = rest[..digits].parse::<u64>()
        {
            let unit = rest.as_bytes().get(digits).copied().unwrap_or(b'm');
            let mb = match unit {
                b'g' => value.saturating_mul(1024),
                b'k' => value / 1024,
                _ => value,
            };
            if mb >= 256 {
                return Some(mb.min(32768) as u32);
            }
        }
        // Advance past the marker plus one whole character. Stepping a single
        // byte would land inside a multi-byte character (Chinese log text)
        // and make the next slice panic.
        let step = rest.chars().next().map_or(1, char::len_utf8);
        from = start + step;
    }

    // Newer launcher logs also record the resolved heap in the header.
    let offset = lower.find("memory=")?;
    let rest = &lower[offset + "memory=".len()..];
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let value = rest.get(..digits)?.parse::<u32>().ok()?;
    (value >= 256).then_some(value.min(32768))
}

fn fabric_missing(lower: &str, text: &str) -> Vec<String> {
    let context = lower.contains("mod resolution encountered an incompatible mod set")
        || lower.contains("which is missing");
    if !context {
        return Vec::new();
    }

    let mut ids = Vec::new();
    for line in text.lines() {
        let line_lower = line.to_ascii_lowercase();
        if line_lower.contains("requires") && line_lower.contains("which is missing")
            && let Some(id) = missing_after_requires(line) {
                ids.push(id);
            }
    }
    for line in text.lines() {
        let trimmed = line.trim_start_matches(|c: char| c == '-' || c.is_whitespace());
        let lower_trimmed = trimmed.to_ascii_lowercase();
        if lower_trimmed.starts_with("install ") {
            let rest = &trimmed["install ".len()..];
            if let Some(id) = parenthesized_id(rest)
                .or_else(|| first_quoted(rest))
                .or_else(|| unquoted_required_id(rest))
            {
                ids.push(id);
            }
        }
    }
    ids
}

fn missing_after_requires(line: &str) -> Option<String> {
    let start = find_ci(line, "requires")? + "requires".len();
    let after = &line[start..];
    let end = find_ci(after, "which is missing").unwrap_or(after.len());
    let segment = &after[..end];
    let segment = if let Some(rest) = strip_prefix_ci(segment.trim(), "any version of ") {
        rest
    } else if let Some(rest) = strip_prefix_ci(segment.trim(), "version ") {
        // "requires version 1.2.3 of fabric-api"; without "of" this is not an id.
        let position = find_ci(rest, " of ")?;
        &rest[position + " of ".len()..]
    } else {
        segment
    };
    // Fabric prints the machine id in parentheses after the display name:
    // mod 'Fabric API' (fabric-api). Prefer it, because truncating the display
    // name at its space would produce a bogus id such as "fabric".
    parenthesized_id(segment)
        .or_else(|| first_quoted(segment))
        .or_else(|| unquoted_required_id(segment))
}

/// A parenthesized machine id such as `(fabric-api)`. Free text and version
/// ranges such as `(1.0)` are rejected.
fn parenthesized_id(value: &str) -> Option<String> {
    let mut from = 0;
    while let Some(haystack) = value.get(from..) {
        let Some(open_offset) = haystack.find('(') else {
            break;
        };
        let open = from + open_offset;
        let rest = &value[open + 1..];
        let Some(close_offset) = rest.find(')') else {
            break;
        };
        let inner = rest[..close_offset].trim();
        if !inner.is_empty()
            && inner
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            && let Some(id) = clean_mod_id(inner)
        {
            return Some(id);
        }
        from = open + 1 + close_offset + 1;
    }
    None
}

/// An unquoted machine id, optionally introduced by "mod". Quoted display
/// names are deliberately not accepted here: they are not machine ids.
fn unquoted_required_id(segment: &str) -> Option<String> {
    let trimmed = segment.trim();
    let trimmed = strip_prefix_ci(trimmed, "mod ").unwrap_or(trimmed);
    if trimmed.starts_with(['\'', '"']) {
        return None;
    }
    parse_mod_id(trimmed)
}

fn forge_dependencies(lower: &str, text: &str) -> (Vec<String>, Vec<String>) {
    let Some(marker) = lower.find("missing or unsupported mandatory dependencies") else {
        return (Vec::new(), Vec::new());
    };
    let mut missing = Vec::new();
    let mut mismatched = Vec::new();
    for line in text[marker..].lines().take(200) {
        let Some(offset) = find_ci(line, "mod id:") else {
            continue;
        };
        let Some(id) = parse_mod_id(&line[offset + "mod id:".len()..]) else {
            continue;
        };
        if let Some(actual_off) = find_ci(line, "actual version:") {
            let actual = line[actual_off + "actual version:".len()..]
                .trim()
                .trim_matches(|c: char| matches!(c, '\'' | '"' | '[' | ']' | ','))
                .to_ascii_lowercase();
            if !actual.is_empty() && actual != "missing" && actual != "none" {
                mismatched.push(id);
                continue;
            }
        }
        missing.push(id);
    }
    (missing, mismatched)
}

/// NeoForge 1.21+ dependency errors:
/// `Mod create requires flywheel 0.6.9 or above`
/// `Currently, flywheel is not installed` (missing)
/// `Currently, flywheel is 0.6.8` (version mismatch)
fn neoforge_dependencies(lower: &str, text: &str) -> (Vec<String>, Vec<String>) {
    if !lower.contains("currently,") {
        return (Vec::new(), Vec::new());
    }
    let has_requires = lower.contains("requires");
    let mut missing = Vec::new();
    let mut mismatched = Vec::new();
    for line in text.lines() {
        let Some(pos) = find_ci(line, "currently,") else {
            continue;
        };
        let rest = line[pos + "currently,".len()..].trim();
        let Some(is_pos) = find_ci(rest, " is ") else {
            continue;
        };
        let lhs = rest[..is_pos].trim();
        let Some(id) = parenthesized_id(lhs)
            .or_else(|| first_quoted(lhs))
            .or_else(|| unquoted_required_id(lhs))
        else {
            continue;
        };
        let value = rest[is_pos + " is ".len()..]
            .trim()
            .trim_matches(|c: char| matches!(c, '\'' | '"' | '[' | ']' | '.' | ','));
        let value_lower = value.to_ascii_lowercase();
        if value_lower.starts_with("not installed") {
            missing.push(id);
        } else if has_requires
            && (value.as_bytes().first().is_some_and(u8::is_ascii_digit)
                || (value_lower.starts_with('v')
                    && value.as_bytes().get(1).is_some_and(u8::is_ascii_digit)))
        {
            mismatched.push(id);
        }
    }
    (missing, mismatched)
}

fn fabric_conflicts(lower: &str, text: &str) -> Vec<String> {
    if !lower.contains("breaks with") && !lower.contains("incompatible with") {
        return Vec::new();
    }
    let mut ids = Vec::new();
    for line in text.lines() {
        let line_lower = line.to_ascii_lowercase();
        let marker = if line_lower.contains("breaks with") && line_lower.contains("which is present") {
            "breaks with"
        } else if line_lower.contains("incompatible with") && line_lower.contains("which is present") {
            "incompatible with"
        } else {
            continue;
        };
        let Some(start) = find_ci(line, marker).map(|i| i + marker.len()) else {
            continue;
        };
        let after = &line[start..];
        let end = find_ci(after, "which is present").unwrap_or(after.len());
        let segment = &after[..end];
        let segment = if let Some(rest) = strip_prefix_ci(segment.trim(), "any version of ") {
            rest
        } else if let Some(rest) = strip_prefix_ci(segment.trim(), "version ") {
            let Some(pos) = find_ci(rest, " of ") else { continue };
            &rest[pos + " of ".len()..]
        } else {
            segment
        };
        if let Some(id) = parenthesized_id(segment)
            .or_else(|| first_quoted(segment))
            .or_else(|| unquoted_required_id(segment))
        {
            ids.push(id);
        }
    }
    ids
}

fn fabric_version_mismatches(lower: &str, text: &str) -> Vec<String> {
    let context = lower.contains("mod resolution encountered an incompatible mod set")
        || lower.contains("only the wrong version is present");
    if !context {
        return Vec::new();
    }
    let mut ids = Vec::new();
    for line in text.lines() {
        let line_lower = line.to_ascii_lowercase();
        if line_lower.contains("requires") && line_lower.contains("only the wrong version is present") {
            let start = match find_ci(line, "requires") {
                Some(i) => i + "requires".len(),
                None => continue,
            };
            let after = &line[start..];
            let end = find_ci(after, ", but ")
                .or_else(|| find_ci(after, "only the wrong version"))
                .unwrap_or(after.len());
            let segment = &after[..end];
            let segment = if let Some(rest) = strip_prefix_ci(segment.trim(), "any version of ") {
                rest
            } else if let Some(rest) = strip_prefix_ci(segment.trim(), "version ") {
                let Some(pos) = find_ci(rest, " of ") else { continue };
                &rest[pos + " of ".len()..]
            } else {
                segment
            };
            if let Some(id) = parenthesized_id(segment)
                .or_else(|| first_quoted(segment))
                .or_else(|| unquoted_required_id(segment))
            {
                ids.push(id);
            }
            continue;
        }
        let trimmed = line.trim_start_matches(|c: char| c == '-' || c.is_whitespace());
        if trimmed.to_ascii_lowercase().starts_with("replace ") {
            let rest = &trimmed["replace ".len()..];
            if let Some(id) = parenthesized_id(rest)
                .or_else(|| first_quoted(rest))
                .or_else(|| unquoted_required_id(rest))
            {
                ids.push(id);
            }
        }
    }
    ids
}

fn has_optifine_render_conflict(lower: &str) -> bool {
    let mut explicit_conflict = false;
    let mut optifine_present = false;
    for line in lower.lines() {
        if !line.contains("optifine") && !line.contains("optifabric") {
            continue;
        }
        // Ignore negative capability checks such as "OptiFine not detected" or "No OptiFine found".
        if line.contains("optifine not detected")
            || line.contains("optifine was not")
            || line.contains("optifine is not installed")
            || line.contains("no optifine")
            || line.contains("without optifine")
            || line.contains("not using optifine")
            || line.contains("optifine not found")
        {
            continue;
        }
        if line.contains("optifine is not compatible with")
            || line.contains("incompatible with optifine")
            || line.contains("optifine detected")
        {
            explicit_conflict = true;
            break;
        }
        if contains_word(line, "optifabric")
            || line.contains("optifine.jar")
            || line.contains("optifine_")
            || line.contains("optifine-")
            || line.contains("net.optifine.")
            || line.contains("optifine.shaders")
            || line.trim_start().starts_with("- optifine")
            || line.contains("mod 'optifine'")
            || line.contains("(optifine)")
        {
            optifine_present = true;
        }
    }
    if explicit_conflict {
        return true;
    }
    if !optifine_present {
        return false;
    }
    let has_render_mod = ["sodium", "iris", "embeddium", "rubidium", "oculus"]
        .iter()
        .any(|m| contains_word(lower, m));
    if !has_render_mod {
        return false;
    }
    lower.contains("mixin")
        || lower.contains("exception")
        || lower.contains("error")
        || lower.contains("incompatible")
        || lower.contains("breaks with")
        || lower.contains("crash report")
}

fn is_wrong_side_mod(lower: &str) -> bool {
    lower.contains("for invalid dist client")
        || lower.contains("invalid dist: client")
        || lower.contains("invalid dist client")
        || (lower.contains("cannot load class") && lower.contains("environment type client"))
}

fn is_corrupt_config(lower: &str) -> bool {
    lower.contains("com.electronwill.nightconfig.core.io.parsingexception")
        || lower.contains("configloadingexception")
        || lower.contains("failed loading config file")
}

fn config_file_name(text: &str) -> Option<String> {
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("config") && !lower.contains("nightconfig") && !lower.contains("parsingexception") {
            continue;
        }
        for ext in [".toml", ".json5", ".json", ".cfg", ".yaml", ".yml"] {
            if let Some(end) = lower.find(ext) {
                let bytes = line.as_bytes();
                let mut start = end;
                while start > 0
                    && (bytes[start - 1].is_ascii_alphanumeric()
                        || matches!(bytes[start - 1], b'-' | b'_' | b'.'))
                {
                    start -= 1;
                }
                if start < end {
                    let name = &line[start..end + ext.len()];
                    if name.len() <= 80 {
                        return Some(name.to_string());
                    }
                }
            }
        }
    }
    None
}

fn contains_word(haystack: &str, word: &str) -> bool {
    let bytes = haystack.as_bytes();
    let wlen = word.len();
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(word) {
        let pos = from + rel;
        let left_ok = pos == 0 || !(bytes[pos - 1].is_ascii_alphanumeric() || bytes[pos - 1] == b'_');
        let right_ok = pos + wlen >= bytes.len()
            || !(bytes[pos + wlen].is_ascii_alphanumeric() || bytes[pos + wlen] == b'_');
        if left_ok && right_ok {
            return true;
        }
        from = pos + wlen;
    }
    false
}

fn mixin_conflicts(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in text.lines() {
        let line_lower = line.to_ascii_lowercase();
        // Fabric prints "Mixin apply for mod X failed", then the exception on
        // the next line, so either line can start the rule.
        let failed = line_lower.contains("mixin apply failed")
            || (line_lower.contains("mixin apply for mod ") && line_lower.contains("failed"))
            || line_lower.contains("invalidinjectionexception")
            || line_lower.contains("mixinapplyerror")
            || line_lower.contains("mixintransformererror")
            || line_lower.contains("mixinprepareerror")
            || line_lower.contains("critical injection failure");
        if !failed {
            continue;
        }
        // Only report a mod when the log actually names one. Falling back to
        // the package prefix in `org.spongepowered.asm.mixin.injection` would
        // invent a mod called "injection".
        let id = token_after_ci(line, "mixin apply for mod ")
            .or_else(|| token_after_ci(line, "mixin apply failed for mod "))
            .or_else(|| token_after_ci(line, "failed for mod "))
            .or_else(|| token_after_ci(line, "from mod "))
            .or_else(|| mixins_config_id(line));
        if let Some(id) = id {
            ids.push(id);
        }
    }
    ids
}

/// Generic mixin config stems that describe a side or layer rather than a mod id.
const GENERIC_MIXIN_STEMS: &[&str] = &[
    "common", "client", "server", "main", "default", "core", "mixin", "mixins", "compat",
    "fabric", "forge", "neoforge", "quilt", "minecraft",
];

/// The stem of an explicit `name.mixins.json` or `mixins.name.json` config on
/// a failure line, e.g. `sodium.mixins.json` -> `sodium` or
/// `mixins.oculus.json` -> `oculus`.
fn mixins_config_id(line: &str) -> Option<String> {
    const SUFFIX: &str = ".mixins.json";
    let lower = line.to_ascii_lowercase();
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(haystack) = lower.get(from..) {
        let Some(offset) = haystack.find(SUFFIX) else {
            break;
        };
        let end = from + offset;
        // Walk back over the machine-id characters of the config stem. The
        // first non-id character is a character boundary, never a byte in the
        // middle of a multi-byte character.
        let mut start = end;
        while start > 0
            && (bytes[start - 1].is_ascii_alphanumeric()
                || matches!(bytes[start - 1], b'-' | b'_'))
        {
            start -= 1;
        }
        if start < end
            && let Some(id) = clean_mod_id(&line[start..end])
            && !GENERIC_MIXIN_STEMS.contains(&id.as_str())
        {
            return Some(id);
        }
        from = end + SUFFIX.len();
    }

    const PREFIX: &str = "mixins.";
    from = 0;
    while let Some(haystack) = lower.get(from..) {
        let Some(offset) = haystack.find(PREFIX) else {
            break;
        };
        let start = from + offset + PREFIX.len();
        if let Some(json_off) = lower[start..].find(".json") {
            let stem = &line[start..start + json_off];
            if !stem.is_empty()
                && stem.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                && let Some(id) = clean_mod_id(stem)
                && !GENERIC_MIXIN_STEMS.contains(&id.as_str())
            {
                return Some(id);
            }
        }
        from = start;
    }
    None
}

/// Platform and loader ids that loaders report alongside missing mods, but
/// cannot be downloaded as mods from Modrinth or CurseForge.
const NON_MOD_IDS: &[&str] = &[
    "minecraft",
    "java",
    "forge",
    "neoforge",
    "fabricloader",
    "quilt_loader",
    "fmlcore",
    "javafml",
    "lowcodefml",
    "mclanguage",
];

fn push_missing(diagnoses: &mut Vec<Diagnosis>, seen: &mut HashSet<String>, id: &str) {
    if NON_MOD_IDS.contains(&id) || !seen.insert(id.to_string()) {
        return;
    }
    diagnoses.push(Diagnosis {
        kind: "missingMod".to_string(),
        title: crate::tr!("缺少前置模组 {id}", "Missing dependency {id}"),
        detail: crate::tr!("缺少必需模组 {id}，请在「下载」页搜索并安装后重试。", "Missing required mod {id}. Search and install it on the Download tab, then try again."
        ),
        fix: Some(Fix::InstallMod {
            query: id.to_string(),
        }),
    });
}

fn push_version_mismatch(
    diagnoses: &mut Vec<Diagnosis>,
    missing: &HashSet<String>,
    seen: &mut HashSet<String>,
    id: &str,
) {
    if NON_MOD_IDS.contains(&id) || missing.contains(id) || !seen.insert(id.to_string()) {
        return;
    }
    diagnoses.push(Diagnosis {
        kind: "modVersionMismatch".to_string(),
        title: crate::tr!("模组 {id} 版本不匹配", "Mod {id} has the wrong version"),
        detail: crate::tr!(
            "已安装的模组 {id} 版本不符合其他模组的要求，请打开「模组」页更新或更换为适配版本。",
            "The installed version of {id} doesn't match what another mod requires. Open Mods to update or replace it."
        ),
        fix: Some(Fix::OpenMods),
    });
}

/// Native libraries of graphics drivers, as named in an hs_err file's
/// "Problematic frame" line.
const DRIVER_LIBRARIES: &[&str] = &[
    "atio6axx", "atioglxx", "amdxc", "nvoglv", "ig4icd", "ig7icd", "ig75icd", "ig8icd", "ig9icd", "igxelpicd", "libnvidia-glcore",
    "radeonsi", "iris_dri", "libgl.so", "applemetal", "glengine", "appleintel", "amdmtl",
];

/// The library and function of an hs_err file's "Problematic frame", such
/// as `atio6axx.dll+0x1a2b3c`.
fn problematic_frame(text: &str) -> Option<String> {
    let mut lines = text.lines();
    lines.find(|line| line.trim_start_matches('#').trim().eq_ignore_ascii_case("problematic frame:"))?;
    let frame = lines.next()?.trim_start_matches('#').trim();
    // "C  [atio6axx.dll+0x1a2b3c]" → "atio6axx.dll+0x1a2b3c"
    let inner = frame.split_once('[').and_then(|(_, rest)| rest.split_once(']')).map_or(frame, |(inside, _)| inside);
    (!inner.is_empty()).then(|| inner.chars().take(120).collect())
}

fn first_exception_line(text: &str) -> Option<String> {
    let line = text
        .lines()
        .find(|line| line.to_ascii_lowercase().contains("exception"))?
        .trim();
    if line.is_empty() {
        return None;
    }
    let mut value: String = line.chars().take(240).collect();
    if line.chars().count() > 240 {
        value.push('…');
    }
    Some(value)
}

fn leading_u32(value: &str) -> Option<u32> {
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    value.get(..digits)?.parse().ok()
}

fn round_up_512(value: u64) -> u32 {
    (value.div_ceil(512) * 512).min(32768) as u32
}

fn round_down_512(value: u32) -> u32 {
    (value / 512).max(1) * 512
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
        .find(|&index| haystack[index..index + needle.len()].eq_ignore_ascii_case(needle))
}

fn strip_prefix_ci<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

fn first_quoted(value: &str) -> Option<String> {
    let mut current: Option<(char, String)> = None;
    for character in value.chars() {
        match &mut current {
            Some((quote, content)) if *quote == character => {
                let token = content.clone();
                current = None;
                // A display name such as "Fabric API" is not a machine id;
                // truncating it at the space would invent "fabric".
                if token.chars().any(char::is_whitespace) {
                    continue;
                }
                if let Some(id) = clean_mod_id(&token) {
                    return Some(id);
                }
            }
            Some((_, content)) => content.push(character),
            None if character == '\'' || character == '"' => {
                current = Some((character, String::new()));
            }
            None => {}
        }
    }
    None
}

fn parse_mod_id(value: &str) -> Option<String> {
    clean_mod_id(value)
}

fn clean_mod_id(value: &str) -> Option<String> {
    let value = value.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, '\'' | '"' | '`' | '(' | '[' | ',' | ':')
    });
    let id: String = value
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    let id = id.trim_matches(['-', '_']).to_ascii_lowercase();
    (!id.is_empty() && id.len() <= 64).then_some(id)
}

fn token_after_ci(line: &str, marker: &str) -> Option<String> {
    let offset = find_ci(line, marker)?;
    clean_mod_id(&line[offset + marker.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(diagnoses: &[Diagnosis]) -> Vec<&str> {
        diagnoses
            .iter()
            .map(|diagnosis| diagnosis.title.as_str())
            .collect()
    }

    #[test]
    fn unsupported_class_version_requests_the_matching_java() {
        let log = "[main/ERROR]: java.lang.UnsupportedClassVersionError: net/fabricmc/loader/impl/launch/server/FabricServerLauncher has been compiled by a more recent version of the Java Runtime (class file version 65.0), this version of the Java Runtime only recognizes class file versions up to 52.0";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "需要 Java 21");
        assert_eq!(diagnoses[0].fix, Some(Fix::UseJava { major: 21 }));
    }

    #[test]
    fn heap_exhaustion_increases_memory_from_the_current_xmx() {
        let log = "java.lang.OutOfMemoryError: Java heap space\n\
                   JVM Flags: 4 total; -Xms512m -Xmx2048m -XX:HeapDumpPath=MojangTricksIntelDriversForPerformance";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "内存不足");
        assert_eq!(diagnoses[0].fix, Some(Fix::RaiseMemory { mb: 3072 }));
    }

    #[test]
    fn heap_exhaustion_without_known_memory_uses_a_safe_default() {
        let diagnoses = analyze("java.lang.OutOfMemoryError: Java heap space", None);
        assert_eq!(diagnoses[0].fix, Some(Fix::RaiseMemory { mb: 6144 }));
    }

    #[test]
    fn launcher_header_memory_is_used_when_jvm_flags_are_absent() {
        let log = "[Islet] version=1.21 memory=4096MB java=21.0.2 (21), user=Steve\n\
                   java.lang.OutOfMemoryError: Java heap space";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses[0].fix, Some(Fix::RaiseMemory { mb: 6144 }));
    }

    #[test]
    fn launcher_header_memory_2560_is_used_without_xmx() {
        let log = "[Islet] version=1.21 memory=2560MB java=21.0.2 (21), user=Steve\n\
                   java.lang.OutOfMemoryError: Java heap space";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses[0].fix, Some(Fix::RaiseMemory { mb: 4096 }));
    }

    #[test]
    fn non_ascii_text_after_xmx_does_not_panic() {
        assert_eq!(current_memory_mb("-xmx中文"), None);
        assert_eq!(current_memory_mb("-xmx中文 -xmx4096m"), Some(4096));
        let diagnoses = analyze("-Xmx中文\njava.lang.OutOfMemoryError: Java heap space", None);
        assert_eq!(diagnoses[0].fix, Some(Fix::RaiseMemory { mb: 6144 }));
    }

    #[test]
    fn memory_detail_uses_the_clamped_value() {
        let diagnoses =
            analyze_with_memory_limit("java.lang.OutOfMemoryError: Java heap space", None, 4096);
        assert_eq!(diagnoses[0].fix, Some(Fix::RaiseMemory { mb: 3072 }));
        assert!(diagnoses[0].detail.contains("3072 MB"));
        assert!(!diagnoses[0].detail.contains("6144"));
    }

    #[test]
    fn heap_exhaustion_at_the_safe_cap_is_manual() {
        let log = "java.lang.OutOfMemoryError: Java heap space\nJVM Flags: -Xmx3072m";
        let diagnoses = analyze_with_memory_limit(log, None, 4096);
        assert_eq!(diagnoses[0].fix, Some(Fix::None));
        assert!(!diagnoses[0].detail.contains("建议把这个世界的内存提高到"));
        assert!(diagnoses[0].detail.contains("关闭其他"));
    }

    #[test]
    fn heap_exhaustion_above_the_safe_cap_is_not_reported_as_a_raise() {
        let log = "java.lang.OutOfMemoryError: Java heap space\nJVM Flags: -Xmx8192m";
        let diagnoses = analyze_with_memory_limit(log, None, 4096);
        assert_eq!(diagnoses[0].fix, Some(Fix::None));
        assert!(!diagnoses[0].detail.contains("建议把这个世界的内存提高到"));
    }

    #[test]
    fn failure_to_reserve_address_space_lowers_memory_instead() {
        let log = "Error occurred during initialization of VM\n\
                   Could not reserve enough space for 8388608KB object heap\n\
                   JVM Flags: 2 total; -Xmx8192m -Xms1024m";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "内存设置过高");
        assert_eq!(diagnoses[0].fix, Some(Fix::RaiseMemory { mb: 4096 }));
    }

    #[test]
    fn gc_overhead_counts_as_heap_exhaustion() {
        let diagnoses = analyze(
            "java.lang.OutOfMemoryError: GC overhead limit exceeded\nJVM Flags: -Xmx4096m",
            None,
        );
        assert_eq!(diagnoses[0].fix, Some(Fix::RaiseMemory { mb: 6144 }));
    }

    #[test]
    fn fabric_requires_lines_name_the_missing_mod() {
        let log = "[main/ERROR]: Mod 'create' (create) 0.5.1 requires 'flywheel' (flywheel) 0.6.9 which is missing!";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "缺少前置模组 flywheel");
        assert_eq!(
            diagnoses[0].fix,
            Some(Fix::InstallMod {
                query: "flywheel".to_string()
            })
        );
    }

    #[test]
    fn fabric_version_requirement_uses_the_parenthesized_machine_id() {
        let log = "[main/ERROR]: Mod 'Create' (create) requires version 1.0 or later of mod 'Fabric API' (fabric-api), which is missing!";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "缺少前置模组 fabric-api");
        assert_eq!(
            diagnoses[0].fix,
            Some(Fix::InstallMod {
                query: "fabric-api".to_string()
            })
        );
    }

    #[test]
    fn spaced_display_name_without_a_machine_id_is_not_truncated() {
        let diagnoses = analyze("Mod 'x' requires 'Fabric API' which is missing!", None);
        assert!(diagnoses.is_empty());
    }

    #[test]
    fn fabric_resolution_suggestions_each_become_one_diagnosis() {
        let log = "Mod resolution encountered an incompatible mod set!\n\
                   A potential solution has been determined:\n\
                   \t- Install fabric-api, version 0.92.2+1.20.1 or later.\n\
                   \t- Install cloth-config, version 11.0.0 or later.";
        let diagnoses = analyze(log, None);
        assert_eq!(
            title(&diagnoses),
            vec!["缺少前置模组 fabric-api", "缺少前置模组 cloth-config"]
        );
    }

    #[test]
    fn forge_mandatory_dependencies_are_parsed_after_the_marker() {
        let log = "[main/ERROR] [net.minecraftforge.fml.ModLoader/CORE]: Missing or unsupported mandatory dependencies:\n\
                   \tMod ID: 'flywheel', Requested by: 'create', Expected range: '[0.6.9,)', Actual version: '[MISSING]'\n\
                   \tMod ID: 'architectury', Requested by: 'create', Expected range: '[9.0,)', Actual version: '[MISSING]'";
        let diagnoses = analyze(log, None);
        assert_eq!(
            title(&diagnoses),
            vec!["缺少前置模组 flywheel", "缺少前置模组 architectury"]
        );
    }

    #[test]
    fn missing_mods_are_deduplicated_across_fabric_and_forge_formats() {
        let log = "Mod resolution encountered an incompatible mod set!\n\
                   \t- Install fabric-api, version 0.92.2 or later.\n\
                   Mod 'x' requires 'fabric-api' which is missing!\n\
                   Missing or unsupported mandatory dependencies:\n\
                   \tMod ID: 'fabric-api', Requested by: 'x'";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "缺少前置模组 fabric-api");
    }

    #[test]
    fn install_suggestions_skip_mod_keyword_and_runtime_ids() {
        let log = "Mod resolution encountered an incompatible mod set!\n\
                   \t- Install mod 'Fabric API' (fabric-api), version 0.92.2 or later.\n\
                   \t- Install java, version 21 or later.\n\
                   \t- Install minecraft, version 1.21.1.\n\
                   \t- Install fabricloader, version 0.16.0 or later.\n\
                   Missing or unsupported mandatory dependencies:\n\
                   \tMod ID: 'minecraft', Requested by: 'create', Expected range: '[1.20.1]', Actual version: '1.21.1'\n\
                   \tMod ID: 'forge', Requested by: 'create', Expected range: '[47.2,)', Actual version: '[MISSING]'";
        let diagnoses = analyze(log, None);
        assert_eq!(title(&diagnoses), vec!["缺少前置模组 fabric-api"]);
    }

    #[test]
    fn duplicate_mods_open_the_local_mod_list() {
        let diagnoses = analyze(
            "net.minecraftforge.fml.ModLoadingException: DuplicateModsFoundException: Found duplicate mods: jei",
            None,
        );
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].fix, Some(Fix::OpenMods));
    }

    #[test]
    fn mixin_failure_names_the_conflicting_mod() {
        let log = "Mixin apply for mod sodium failed: org.spongepowered.asm.mixin.injection.throwables.InvalidInjectionException: Critical injection failure";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "模组 sodium 与当前版本不兼容");
        assert_eq!(diagnoses[0].fix, Some(Fix::OpenMods));
    }

    #[test]
    fn mixin_apply_line_without_the_exception_on_the_same_line_is_found() {
        let log = "[main/ERROR]: Mixin apply for mod lithium failed\n\
                   org.spongepowered.asm.mixin.injection.throwables.InvalidInjectionException: Critical injection failure";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "模组 lithium 与当前版本不兼容");
    }

    #[test]
    fn real_fabric_mixin_config_line_is_attributed_to_the_mod() {
        let log = "[main/ERROR]: Mixin apply for mod sodium failed sodium.mixins.json:client.SodiumMixin from mod sodium\n\
                   org.spongepowered.asm.mixin.injection.throwables.InvalidInjectionException: Critical injection failure";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "模组 sodium 与当前版本不兼容");
    }

    #[test]
    fn mixin_exception_with_a_config_name_is_attributed() {
        let log = "org.spongepowered.asm.mixin.injection.throwables.InvalidInjectionException: Critical injection failure: handler in sodium.mixins.json:client.SodiumMixin threw";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "模组 sodium 与当前版本不兼容");
    }

    #[test]
    fn mixin_package_names_are_never_treated_as_mod_ids() {
        let log = "org.spongepowered.asm.mixin.injection.throwables.InvalidInjectionException: Critical injection failure";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "未识别的错误");
        assert!(!diagnoses[0].title.contains("injection"));
        assert!(!diagnoses[0].detail.contains("模组 injection"));
    }

    #[test]
    fn native_crashes_name_the_driver_or_the_frame() {
        let driver = "#\n# A fatal error has been detected by the Java Runtime Environment:\n#\n#  EXCEPTION_ACCESS_VIOLATION (0xc0000005) at pc=0x00007ffb\n#\n# Problematic frame:\n# C  [atio6axx.dll+0x1a2b3c]\n";
        let found = analyze("", Some(driver));
        assert_eq!(found[0].kind, "jvmCrash");
        assert!(found[0].detail.contains("atio6axx.dll+0x1a2b3c"));
        assert_eq!(problematic_frame(driver).as_deref(), Some("atio6axx.dll+0x1a2b3c"));

        let other = driver.replace("atio6axx.dll+0x1a2b3c", "libsomemod.so+0x10");
        let found = analyze("", Some(&other));
        assert_eq!(found.len(), 1);
        assert!(found[0].detail.contains("libsomemod.so+0x10"));
        assert_ne!(found[0].title, analyze("", Some(driver))[0].title);

        // Running out of native memory keeps its memory advice, once.
        let memory = format!("{driver}# There is insufficient memory for the Java Runtime Environment to continue.\n");
        let found = analyze("", Some(&memory));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "memory");
    }

    #[test]
    fn graphics_errors_are_manual_with_driver_advice() {
        let diagnoses = analyze(
            "[Render thread/ERROR]: GLFW error 65542: WGL: The driver does not appear to support OpenGL",
            None,
        );
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "显卡驱动太旧或不支持");
        assert_eq!(diagnoses[0].fix, Some(Fix::None));
        assert!(diagnoses[0].detail.contains("显卡驱动"));
    }

    #[test]
    fn missing_main_class_offers_repair() {
        let diagnoses = analyze(
            "Error: Could not find or load main class net.minecraft.client.main.Main",
            None,
        );
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].fix, Some(Fix::Repair));
    }

    #[test]
    fn unrecognised_exceptions_keep_the_first_exception_line() {
        let diagnoses = analyze(
            "[main/INFO]: starting\njava.lang.RuntimeException: boom at world tick",
            None,
        );
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].title, "未识别的错误");
        assert!(diagnoses[0].detail.contains("RuntimeException: boom"));
        assert_eq!(diagnoses[0].fix, Some(Fix::None));
    }

    #[test]
    fn clean_log_without_exceptions_produces_no_diagnosis() {
        assert!(analyze("[main/INFO]: Steve joined the game", None).is_empty());
    }

    #[test]
    fn rules_are_reported_in_priority_order() {
        let log = "java.lang.UnsupportedClassVersionError: X (class file version 65.0)\n\
                   Mod resolution encountered an incompatible mod set!\n\
                   \t- Install fabric-api, version 0.92.2 or later.";
        let diagnoses = analyze(log, None);
        assert_eq!(
            title(&diagnoses),
            vec!["需要 Java 21", "缺少前置模组 fabric-api"]
        );
    }

    #[test]
    fn crash_report_content_is_analyzed_too() {
        let report = "---- Minecraft Crash Report ----\n\
                      Description: Initializing game\n\
                      java.lang.OutOfMemoryError: Java heap space";
        let diagnoses = analyze("[main/INFO]: starting", Some(report));
        assert_eq!(diagnoses[0].title, "内存不足");
    }

    #[test]
    fn suggested_memory_is_capped_by_physical_memory() {
        assert_eq!(clamp_memory_mb(6144, 4096), 3072);
        assert_eq!(clamp_memory_mb(6144, 16384), 6144);
        assert_eq!(clamp_memory_mb(4096, 16384), 4096);
        assert_eq!(clamp_memory_mb(6144, 0), 6144);
        assert_eq!(clamp_memory_mb(512, 1024), 512);
        assert_eq!(clamp_memory_mb(32768, 65536), 32768);
    }

    #[test]
    fn removing_memory_cap_only_touches_memory_fixes() {
        let mut fix = Fix::RaiseMemory { mb: 6144 };
        clamp_memory_fix(&mut fix, 4096);
        assert_eq!(fix, Fix::RaiseMemory { mb: 3072 });
        let mut other = Fix::UseJava { major: 21 };
        clamp_memory_fix(&mut other, 4096);
        assert_eq!(other, Fix::UseJava { major: 21 });
    }

    #[test]
    fn neoforge_missing_and_version_mismatch_dependencies_are_diagnosed() {
        let log = "Mod create requires flywheel 0.6.9 or above\n\
                   Currently, flywheel is not installed\n\
                   Mod create requires ponder 1.0.50 or above\n\
                   Currently, ponder is 1.0.40\n\
                   Mod create requires minecraft 1.21.1\n\
                   Currently, minecraft is 1.21.4";
        let diagnoses = analyze(log, None);
        assert_eq!(
            title(&diagnoses),
            vec!["缺少前置模组 flywheel", "模组 ponder 版本不匹配"]
        );
        assert_eq!(
            diagnoses[0].fix,
            Some(Fix::InstallMod { query: "flywheel".into() })
        );
        assert_eq!(diagnoses[1].kind, "modVersionMismatch");
        assert_eq!(diagnoses[1].fix, Some(Fix::OpenMods));
    }

    #[test]
    fn fabric_and_forge_wrong_versions_suggest_opening_mods() {
        let fabric = "Mod resolution encountered an incompatible mod set!\n\
                      \t- Mod 'Create' (create) requires version 0.92.2 or later of mod 'Fabric API' (fabric-api), but only the wrong version is present: 0.88.0!\n\
                      \t- Replace mod 'Cloth Config' (cloth-config) 10.0.0 with version 11.1.0 or later.";
        let diagnoses = analyze(fabric, None);
        assert_eq!(
            title(&diagnoses),
            vec!["模组 fabric-api 版本不匹配", "模组 cloth-config 版本不匹配"]
        );
        assert!(diagnoses.iter().all(|d| d.fix == Some(Fix::OpenMods)));

        let forge = "Missing or unsupported mandatory dependencies:\n\
                     \tMod ID: 'geckolib', Requested by: 'bosses', Expected range: '[4.4,)', Actual version: '4.2.1'";
        let diagnoses = analyze(forge, None);
        assert_eq!(title(&diagnoses), vec!["模组 geckolib 版本不匹配"]);
        assert_eq!(diagnoses[0].fix, Some(Fix::OpenMods));
    }

    #[test]
    fn optifine_conflict_with_sodium_or_iris_is_detected_without_driver_false_positives() {
        let log = "org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: OptiFine is not compatible with Iris / Sodium";
        let diagnoses = analyze(log, None);
        assert!(diagnoses.iter().any(|d| d.kind == "modConflict" && d.fix == Some(Fix::OpenMods)));

        let crash = "---- Minecraft Crash Report ----\n\
                     OptiFabric loaded alongside sodium and iris\n\
                     java.lang.IncompatibleClassChangeError: boom";
        let diagnoses = analyze(crash, None);
        assert!(diagnoses.iter().any(|d| d.kind == "modConflict"));

        // Linux Mesa driver `iris_dri` alongside a mention of optifine must not false-positive.
        let driver = "# A fatal error has been detected by the Java Runtime Environment:\n\
                      # Problematic frame:\n\
                      # C  [iris_dri.so+0x1234]\n\
                      optifine";
        let diagnoses = analyze("", Some(driver));
        assert!(!diagnoses.iter().any(|d| d.kind == "modConflict"));

        // Negative checks like "OptiFine not detected" alongside Sodium must never false-positive.
        let benign = "[main/INFO]: Loading 2 mods: - sodium 0.6.0\n\
                      [main/INFO]: OptiFine not detected, continuing normally\n\
                      java.lang.RuntimeException: unrelated tick crash";
        let diagnoses = analyze(benign, None);
        assert!(!diagnoses.iter().any(|d| d.kind == "modConflict"));
    }

    #[test]
    fn fabric_breaks_with_reports_mod_conflict() {
        let log = "Mod resolution encountered an incompatible mod set!\n\
                   \t- Mod 'Sodium' (sodium) 0.5.8 breaks with any version of mod 'Indium' (indium), which is present!";
        let diagnoses = analyze(log, None);
        assert_eq!(title(&diagnoses), vec!["模组 indium 与其他模组冲突"]);
        assert_eq!(diagnoses[0].kind, "modConflict");
        assert_eq!(diagnoses[0].fix, Some(Fix::OpenMods));
    }

    #[test]
    fn mixin_transformer_and_prefix_config_names_are_attributed() {
        let log = "org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: An unexpected critical error was encountered in mixins.oculus.json";
        let diagnoses = analyze(log, None);
        assert_eq!(title(&diagnoses), vec!["模组 oculus 与当前版本不兼容"]);
        assert_eq!(diagnoses[0].fix, Some(Fix::OpenMods));

        // Generic stems like mixins.common.json must not blame a bogus "common" mod.
        let generic = "org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: failed in mixins.common.json";
        let diagnoses = analyze(generic, None);
        assert!(!diagnoses.iter().any(|d| d.title.contains("common")));
    }

    #[test]
    fn neoforge_quoted_and_parenthesized_ids_are_parsed() {
        let log = "Mod create requires flywheel 0.6.9\n\
                   Currently, mod 'Flywheel' (flywheel) is not installed\n\
                   Currently, 'ponder' is '1.0.40'.";
        let diagnoses = analyze(log, None);
        assert_eq!(
            title(&diagnoses),
            vec!["缺少前置模组 flywheel", "模组 ponder 版本不匹配"]
        );
    }

    #[test]
    fn server_only_mod_on_client_is_detected() {
        let log = "java.lang.RuntimeException: Attempted to load class net/minecraft/server/Foo for invalid dist CLIENT";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].kind, "wrongSideMod");
        assert_eq!(diagnoses[0].fix, Some(Fix::OpenMods));

        let fabric = "java.lang.RuntimeException: Cannot load class com.example.ServerOnly in environment type CLIENT";
        let diagnoses = analyze(fabric, None);
        assert_eq!(diagnoses[0].kind, "wrongSideMod");
    }

    #[test]
    fn corrupt_nightconfig_names_the_broken_config_and_offers_tools() {
        let log = "net.minecraftforge.fml.config.ConfigFileTypeHandler$ConfigLoadingException: Failed loading config file create-client.toml of type CLIENT for modid create\n\
                   Caused by: com.electronwill.nightconfig.core.io.ParsingException: Not enough data available";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].kind, "corruptConfig");
        assert_eq!(diagnoses[0].title, "配置文件 create-client.toml 已损坏");
        assert_eq!(diagnoses[0].fix, Some(Fix::OpenTools));
    }

    #[test]
    fn bind_exception_reports_port_in_use() {
        let log = "java.net.BindException: Address already in use: bind";
        let diagnoses = analyze(log, None);
        assert_eq!(diagnoses.len(), 1);
        assert_eq!(diagnoses[0].kind, "portInUse");
        assert_eq!(diagnoses[0].fix, Some(Fix::None));
    }
}
