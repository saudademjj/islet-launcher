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
            detail: crate::tr!("游戏或模组中有用 Java {major} 编译的类（class file version {version}），当前使用的 Java 版本太低。请准备 Java {major}，启动器会把它设为这个世界使用的 Java。", "The game or a mod has classes built for Java {major} (class file version {version}), and the Java in use is too old. Get Java {major}; the launcher will set it for this world."
            ),
            fix: Some(Fix::UseJava { major }),
        });
    }

    // 2. Heap exhaustion needs more memory; failing to reserve address space
    //    usually means the configured heap is already too large.
    if let Some(diagnosis) = memory_diagnosis(&lower, total_mb) {
        diagnoses.push(diagnosis);
    }

    // 3/4. Fabric and Forge report missing dependencies in different formats.
    let mut missing = HashSet::new();
    for id in fabric_missing(&lower, text) {
        push_missing(&mut diagnoses, &mut missing, &id);
    }
    for id in forge_missing(&lower, text) {
        push_missing(&mut diagnoses, &mut missing, &id);
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
                crate::core::i18n::pick("同一个模组存在多个版本的 jar，游戏无法决定加载哪一个。请打开本地模组页，每个模组只保留一个版本。", "Several jars of the same mod are present and the game can't choose. Open the Mods tab and keep one version of each mod.")
                    .to_string(),
            fix: Some(Fix::OpenMods),
        });
    }

    // 6. A mixin that no longer matches the game or loader classes.
    let mut conflicted = HashSet::new();
    for id in mixin_conflicts(text) {
        if conflicted.insert(id.clone()) {
            diagnoses.push(Diagnosis {
                kind: "mixinConflict".to_string(),
                title: crate::tr!("模组 {id} 与当前版本不兼容", "Mod {id} doesn't fit this version"),
                detail: crate::tr!("模组 {id} 的 Mixin 注入失败，通常是因为模组版本与 Minecraft 或加载器版本不匹配。请更新该模组到适配版本，或暂时禁用它。", "Mod {id} failed to apply its mixins, usually because it doesn't match the Minecraft or loader version. Update it to a fitting version or disable it for now."
                ),
                fix: Some(Fix::OpenMods),
            });
        }
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
                crate::core::i18n::pick("游戏无法创建所需的 OpenGL 图形上下文。请更新显卡驱动，并确认这台电脑和系统满足该 Minecraft 版本的要求；虚拟机或远程桌面环境可能无法运行游戏。", "The game couldn't create the OpenGL context it needs. Update the graphics driver and check that this computer meets the version's requirements; virtual machines and remote desktops may not run the game.")
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
                crate::core::i18n::pick("游戏主类或依赖文件无法加载，通常是下载不完整或被误删。请使用「校验并修复」重新核对并下载缺失的文件，存档不会受影响。", "The game's main class or a library couldn't load, usually from an incomplete download or a deleted file. Use Verify and repair to check and download what's missing; saves are not affected.")
                    .to_string(),
            fix: Some(Fix::Repair),
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
                crate::tr!("游戏在显卡驱动里崩溃了（{frame}）。请把显卡驱动更新到最新版本；笔记本请确认游戏用的是独立显卡。光影或某些渲染模组也可能触发驱动问题。", "The game crashed inside the graphics driver ({frame}). Update the graphics driver; on a laptop, make sure the game runs on the dedicated GPU. Shaders or some rendering mods can also trigger driver bugs."),
            )
        } else {
            let at = frame.map(|frame| crate::tr!("（崩溃位置：{frame}）", " (crashed in {frame})")).unwrap_or_default();
            (
                crate::core::i18n::pick("Java 虚拟机崩溃了", "The Java VM crashed").to_string(),
                crate::tr!("Java 虚拟机本身崩溃了{at}。常见原因是显卡驱动、最近添加的模组自带的本地库，或这个 Java 版本有问题。可以更新驱动、移除最近加的模组，或换一个 Java 再试。", "The Java VM itself crashed{at}. Common causes are the graphics driver, a native library in a recently added mod, or a problem with this Java build. Update the driver, remove recently added mods, or try another Java."),
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
        .max(512)
        .min(32768);
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
                detail: crate::tr!("游戏堆内存（-Xmx）已经耗尽。这个世界已使用 {current} MB，已接近或超过这台电脑可安全分配的上限（约 {next} MB），继续提高反而可能让系统内存不足。请关闭其他占用内存的程序、减少同时加载的模组或降低渲染距离，也可以手动检查内存设置后再试。", "The game ran out of heap memory (-Xmx). This world already uses {current} MB, at or past what this computer can safely give (about {next} MB); raising it further may starve the system. Close other programs, load fewer mods or lower the render distance, or review the memory setting and try again."
                ),
                fix: Some(Fix::None),
            });
        }
        return Some(Diagnosis {
            kind: "memory".to_string(),
            title: crate::core::i18n::pick("内存不足", "Out of memory").to_string(),
            detail: crate::tr!("游戏堆内存（-Xmx）已经耗尽，常见于大型整合包或加载大量区块时。建议把这个世界的内存提高到 {next} MB；如果启动器提示系统内存不足，请关闭其他程序后再试。", "The game ran out of heap memory (-Xmx), common with big modpacks or many loaded chunks. Raise this world's memory to {next} MB; if the system then runs short, close other programs and try again."
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
                detail: crate::tr!("系统无法为 Java 预留请求的内存。这个世界已使用 {current} MB，自动调整内存无法解决这个问题。请关闭其他占用内存的程序后重试，或手动把这个世界的内存设得低一些。", "The system couldn't reserve the memory Java asked for. This world uses {current} MB and automatic memory can't fix it. Close other programs and try again, or set a lower memory for this world."
                ),
                fix: Some(Fix::None),
            });
        }
        return Some(Diagnosis {
            kind: "memory".to_string(),
            title: crate::core::i18n::pick("内存设置过高", "Memory set too high").to_string(),
            detail: crate::tr!("系统无法为 Java 预留请求的内存，通常是世界内存（-Xmx）高于可用物理内存。建议把这个世界的内存降到 {next} MB；如果仍然失败，请关闭其他程序或改用自动内存。", "The system couldn't reserve the memory Java asked for, usually because the world's memory (-Xmx) is above the free physical memory. Lower it to {next} MB; if that still fails, close other programs or use automatic memory."
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
        if line_lower.contains("requires") && line_lower.contains("which is missing") {
            if let Some(id) = missing_after_requires(line) {
                ids.push(id);
            }
        }
    }
    for line in text.lines() {
        let trimmed = line.trim_start_matches(|c: char| c == '-' || c.is_whitespace());
        let lower_trimmed = trimmed.to_ascii_lowercase();
        if lower_trimmed.starts_with("install ") {
            if let Some(id) = parse_mod_id(&trimmed["install ".len()..]) {
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
        let Some(position) = find_ci(rest, " of ") else {
            return None;
        };
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

fn forge_missing(lower: &str, text: &str) -> Vec<String> {
    let Some(marker) = lower.find("missing or unsupported mandatory dependencies") else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for line in text[marker..].lines().take(200) {
        let Some(offset) = find_ci(line, "mod id:") else {
            continue;
        };
        if let Some(id) = parse_mod_id(&line[offset + "mod id:".len()..]) {
            ids.push(id);
        }
    }
    ids
}

fn mixin_conflicts(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in text.lines() {
        let line_lower = line.to_ascii_lowercase();
        // Fabric prints "Mixin apply for mod X failed", then the exception on
        // the next line, so either line can start the rule.
        let failed = line_lower.contains("mixin apply failed")
            || (line_lower.contains("mixin apply for mod ") && line_lower.contains("failed"))
            || line_lower.contains("invalidinjectionexception");
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

/// The stem of an explicit `name.mixins.json` config on a failure line, e.g.
/// `sodium.mixins.json` -> `sodium`. This is a real config name, unlike the
/// package path `org.spongepowered.asm.mixin.injection`.
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
        {
            return Some(id);
        }
        from = end + SUFFIX.len();
    }
    None
}

fn push_missing(diagnoses: &mut Vec<Diagnosis>, seen: &mut HashSet<String>, id: &str) {
    if !seen.insert(id.to_string()) {
        return;
    }
    diagnoses.push(Diagnosis {
        kind: "missingMod".to_string(),
        title: crate::tr!("缺少前置模组 {id}", "Missing dependency {id}"),
        detail: crate::tr!("这个世界缺少模组 {id} 或它的必需依赖，游戏在解析模组时停止加载。请在下载模组页搜索并安装 {id}，然后重新启动。", "The world is missing the mod {id} or one of its required dependencies, so loading stopped. Search for and install {id} on the Download tab, then start again."
        ),
        fix: Some(Fix::InstallMod {
            query: id.to_string(),
        }),
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
}
