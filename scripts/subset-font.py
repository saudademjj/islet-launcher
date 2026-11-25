#!/usr/bin/env python3
"""Regenerate or verify web/src/assets/fonts/fusion-pixel-12.woff2.

Keeps printable ASCII, a few punctuation marks and every non-Latin character
the front end shows. Run it after adding interface text:

    # Automatically fetch/cache the official OFL source font into .cache/fonts/:
    python3 scripts/subset-font.py --fetch

    # Or point to a local copy (or set FUSION_PIXEL_FONT):
    python3 scripts/subset-font.py path/to/fusion-pixel-12px-proportional-zh_hans.otf.woff2

    # Verify in CI / locally that no production CJK UI characters are missing:
    python3 scripts/subset-font.py --check

Needs `fonttools` and `brotli`; if missing and `uv` is installed on PATH, the
script delegates automatically to `uv run --with fonttools --with brotli`.
"""
import argparse
import io
import json
import os
import pathlib
import shutil
import sys
import urllib.request
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
WEB = ROOT / "web" / "src"
OUT = WEB / "assets" / "fonts" / "fusion-pixel-12.woff2"
CACHE_DIR = ROOT / ".cache" / "fonts"
SOURCE_NAME = "fusion-pixel-12px-proportional-zh_hans.otf.woff2"
CACHED_SOURCE = CACHE_DIR / SOURCE_NAME
RELEASE_API = "https://api.github.com/repos/TakWolf/fusion-pixel-font/releases/latest"

BASE_CHARS = {chr(c) for c in range(0x20, 0x7F)} | set("·—…「」『』（）：，。、！？；“”‘’←→↑↓↵⌘⇧×✓▪█░%°")


def is_excluded(rel: str) -> bool:
    """Demo data, dev self-test, and unit tests/helpers never ship to users."""
    return (
        rel == "api/mock.ts"
        or rel.startswith("dev/")
        or rel.startswith("test/")
        or ".test." in rel
    )


def collect_subset_chars() -> set[str]:
    chars = set(BASE_CHARS)
    for path in sorted(WEB.rglob("*.ts*")):
        rel = path.relative_to(WEB).as_posix()
        if is_excluded(rel):
            continue
        chars |= {ch for ch in path.read_text(encoding="utf-8") if ord(ch) > 0x2000}
    return chars


def collect_prod_cjk_chars() -> set[str]:
    # Verify common CJK ideographs and fullwidth punctuation in production UI files
    # (skipping unit tests, regex range endpoints like \u4dbf/\u9fff/\ufaff and emoji).
    prod_chars: set[str] = set()
    for path in sorted(WEB.rglob("*.ts*")):
        rel = path.relative_to(WEB).as_posix()
        if is_excluded(rel):
            continue
        for ch in path.read_text(encoding="utf-8"):
            cp = ord(ch)
            if (0x3000 <= cp <= 0x303F) or (0x4E00 <= cp <= 0x9FA5) or (0xFF00 <= cp <= 0xFFEF):
                prod_chars.add(ch)
    return prod_chars


def import_fonttools(argv: list[str]):
    try:
        from fontTools import subset  # type: ignore
        from fontTools.ttLib import TTFont  # type: ignore
        import brotli  # noqa: F401
        return subset, TTFont
    except ImportError as exc:
        uv = shutil.which("uv")
        if uv and os.environ.get("_ISLET_FONT_UV") != "1":
            env = dict(os.environ, _ISLET_FONT_UV="1")
            os.execve(
                uv,
                [uv, "run", "--with", "fonttools", "--with", "brotli", "python3", str(pathlib.Path(__file__).resolve()), *argv],
                env,
            )
        print(
            f"error: missing Python dependency ({exc.name}). Install fonttools + brotli, or run:\n"
            "  uv run --with fonttools --with brotli python3 scripts/subset-font.py --fetch",
            file=sys.stderr,
        )
        sys.exit(2)


def write_atomic(dest: pathlib.Path, data: bytes) -> None:
    part = dest.with_suffix(dest.suffix + ".part")
    try:
        part.write_bytes(data)
        part.replace(dest)
    except Exception:
        part.unlink(missing_ok=True)
        raise


def fetch_source_font(force: bool = False) -> pathlib.Path:
    if CACHED_SOURCE.exists() and not force:
        return CACHED_SOURCE
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    req = urllib.request.Request(
        RELEASE_API,
        headers={"User-Agent": "islet-launcher-font-subset", "Accept": "application/vnd.github+json"},
    )
    print(f"fetching release metadata from {RELEASE_API} ...")
    with urllib.request.urlopen(req, timeout=30) as resp:
        release = json.loads(resp.read().decode("utf-8"))
    assets = release.get("assets") or []

    direct = next((a for a in assets if a.get("name") == SOURCE_NAME), None)
    if direct:
        url = direct["browser_download_url"]
        print(f"downloading {url} ...")
        with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "islet-launcher-font-subset"}), timeout=120) as resp:
            write_atomic(CACHED_SOURCE, resp.read())
        return CACHED_SOURCE

    zip_asset = next(
        (
            a
            for a in assets
            if "12px" in a.get("name", "")
            and "proportional" in a.get("name", "")
            and "woff2" in a.get("name", "")
            and a.get("name", "").endswith(".zip")
        ),
        None,
    ) or next(
        (a for a in assets if "woff2" in a.get("name", "") and a.get("name", "").endswith(".zip")),
        None,
    )
    if not zip_asset:
        raise RuntimeError(f"could not find a 12px proportional WOFF2 asset in {RELEASE_API}")

    url = zip_asset["browser_download_url"]
    print(f"downloading {url} ...")
    with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "islet-launcher-font-subset"}), timeout=180) as resp:
        archive_bytes = resp.read()
    with zipfile.ZipFile(io.BytesIO(archive_bytes)) as zf:
        member = next((n for n in zf.namelist() if n.endswith(SOURCE_NAME)), None)
        if not member:
            raise RuntimeError(f"{SOURCE_NAME} not found inside {zip_asset['name']}")
        write_atomic(CACHED_SOURCE, zf.read(member))
    print(f"cached -> {CACHED_SOURCE.relative_to(ROOT)}")
    return CACHED_SOURCE


def check_subset(argv: list[str]) -> int:
    _, TTFont = import_fonttools(argv)
    font = TTFont(str(OUT))
    cmap: set[int] = set()
    for table in font["cmap"].tables:
        cmap |= set(table.cmap.keys())
    prod_chars = collect_prod_cjk_chars()
    missing = sorted(ch for ch in prod_chars if ord(ch) not in cmap)
    if missing:
        codes = ", ".join(f"{ch}(U+{ord(ch):04X})" for ch in missing)
        print(
            f"error: {len(missing)} UI character(s) missing from {OUT.relative_to(ROOT)}: {''.join(missing)}\n"
            f"  code points: {codes}\n"
            "  rebuild subset: npm run font:subset  (or: uv run --with fonttools --with brotli python3 scripts/subset-font.py --fetch)",
            file=sys.stderr,
        )
        return 1
    print(f"ok: all {len(prod_chars)} CJK UI characters present in {OUT.relative_to(ROOT)}")
    return 0


def build_subset(source: pathlib.Path, argv: list[str]) -> int:
    subset, _ = import_fonttools(argv)
    chars = collect_subset_chars()
    subset.main([
        str(source),
        "--text=" + "".join(sorted(chars)),
        "--flavor=woff2",
        "--output-file=" + str(OUT),
        "--layout-features=*",
        "--no-hinting",
    ])
    print(f"{len(chars)} characters, {OUT.stat().st_size} bytes -> {OUT.relative_to(ROOT)}")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Verify or regenerate the Fusion Pixel 12px UI font subset.")
    parser.add_argument("source", nargs="?", help="Path to full Fusion Pixel 12px proportional zh_hans font file")
    parser.add_argument("--check", action="store_true", help="Verify all production CJK UI characters are present")
    parser.add_argument("--fetch", action="store_true", help="Download and cache the official OFL source font into .cache/fonts/")
    parser.add_argument("--force", action="store_true", help="Re-download the cached source font when used with --fetch")
    args = parser.parse_args(argv)

    if args.check:
        return check_subset(argv)

    if args.source:
        source = pathlib.Path(args.source)
    elif os.environ.get("FUSION_PIXEL_FONT"):
        source = pathlib.Path(os.environ["FUSION_PIXEL_FONT"])
    elif args.fetch or CACHED_SOURCE.exists():
        source = fetch_source_font(force=args.force)
    else:
        parser.error("provide a font file path, set FUSION_PIXEL_FONT, or pass --fetch")
        return 2

    if not source.exists():
        print(f"error: font source not found: {source}", file=sys.stderr)
        return 1
    return build_subset(source, argv)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
