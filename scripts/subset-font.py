#!/usr/bin/env python3
"""Regenerate web/src/assets/fonts/fusion-pixel-12.woff2.

Keeps printable ASCII, a few punctuation marks and every non-Latin character
the front end shows. Run it after adding interface text, with the full
Fusion Pixel 12px proportional zh_hans font from the project's releases
(https://github.com/TakWolf/fusion-pixel-font, OFL) as the source. Needs
fontTools and brotli.

    python3 scripts/subset-font.py path/to/fusion-pixel-12px-proportional-zh_hans.otf.woff2
"""
import pathlib
import sys

from fontTools import subset

ROOT = pathlib.Path(__file__).resolve().parent.parent
WEB = ROOT / "web" / "src"
OUT = WEB / "assets" / "fonts" / "fusion-pixel-12.woff2"

chars = {chr(c) for c in range(0x20, 0x7F)}
chars |= set("·—…「」『』（）：，。、！？；“”‘’←→↑↓↵⌘⇧×✓▪█░%°")
for path in sorted(WEB.rglob("*.ts*")):
    # Demo data and the self-test never reach users; scanning them would hide
    # glyphs that real names are missing.
    rel = path.relative_to(WEB).as_posix()
    if rel == "api/mock.ts" or rel.startswith("dev/"):
        continue
    chars |= {ch for ch in path.read_text(encoding="utf-8") if ord(ch) > 0x2000}

subset.main([
    sys.argv[1],
    "--text=" + "".join(sorted(chars)),
    "--flavor=woff2",
    "--output-file=" + str(OUT),
    "--layout-features=*",
    "--no-hinting",
])
print(f"{len(chars)} characters, {OUT.stat().st_size} bytes -> {OUT.relative_to(ROOT)}")
