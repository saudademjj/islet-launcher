#!/usr/bin/env python3
"""Regenerate docs/THIRD-PARTY.md from Cargo metadata.

Lists every crate linked into the Windows build (normal dependencies only,
not build or dev tools) with its declared license.

    python3 scripts/third-party.py
"""
import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-msvc"

meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", TARGET],
    cwd=ROOT,
))
packages = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
# Both the core library and the Tauri shell end up in the build.
seen, stack = set(), list(meta["workspace_members"])
while stack:
    node = nodes[stack.pop()]
    for dep in node["deps"]:
        if any(kind["kind"] is None for kind in dep["dep_kinds"]) and dep["pkg"] not in seen:
            seen.add(dep["pkg"])
            stack.append(dep["pkg"])

rows = sorted(
    (packages[i]["name"], packages[i]["version"], packages[i].get("license") or "见源码仓库")
    for i in seen - set(meta["workspace_members"])
)
lines = [
    "# 第三方组件",
    "",
    "构建草岛 Windows 版时用到的 Rust 组件如下（含编译期使用的过程宏），许可证信息来自各组件发布时的声明。",
    "完整许可证文本可在 crates.io 对应版本的源码包中找到。",
    "",
    "Minecraft 是 Mojang Studios 的商标；本启动器与 Mojang 和 Microsoft 无关，不包含任何游戏文件。",
    "",
    f"共 {len(rows)} 个组件。此文件由 `python3 scripts/third-party.py` 生成。",
    "",
    "| 组件 | 版本 | 许可证 |",
    "|---|---|---|",
]
lines += [f"| {name} | {version} | {license} |" for name, version, license in rows]
lines += [
    "",
    "## 运行时下载和内嵌的资源",
    "",
    "| 资源 | 许可证 | 说明 |",
    "|---|---|---|",
    "| [EasyTier](https://github.com/EasyTier/EasyTier) 2.6.4（easytier-core、easytier-cli） | LGPL-3.0 | 第一次联机时从官方发布页（或镜像）下载到 `tools/easytier-2.6.4`，校验 SHA-256 后使用。未修改，作为独立进程运行；用户可以换成自己编译的同版本程序。不随安装包分发。 |",
    "| Packet.dll（Windows，来自 [Npcap](https://npcap.com/)） | Npcap 许可证 | 包含在 EasyTier 的 Windows 发布包里，随它一起下载。Npcap 免费版不允许第三方再分发，所以不要把它打进安装包。 |",
    "| [Fusion Pixel](https://github.com/TakWolf/fusion-pixel-font) 12px 字体子集 | OFL-1.1 | 内嵌在前端里，只含界面用到的字，由 `scripts/subset-font.py` 生成；许可证见 `web/src/assets/fonts/OFL-fusion-pixel.txt`。 |",
]
(ROOT / "docs" / "THIRD-PARTY.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
print(f"wrote docs/THIRD-PARTY.md ({len(rows)} crates)")
