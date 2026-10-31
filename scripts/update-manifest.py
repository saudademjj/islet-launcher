#!/usr/bin/env python3
"""Writes or extends the update manifest that MC_LAUNCHER_UPDATE_URL points to.

Each run adds packages for one or more platforms, with their SHA-256, so the
macOS, Windows and Linux builds can each add theirs to the same file:

    scripts/update-manifest.py build/latest.json --version 0.5.0 \\
        --page https://example.com/releases/0.5.0 --notes "What's new" \\
        --base-url https://example.com/download/0.5.0/ \\
        macos-aarch64=build/Islet-0.5.0-arm64.dmg

A platform is "<os>-<arch>" as Rust names them: macos-aarch64, macos-x86_64,
macos-universal, windows-x86_64, linux-x86_64. Each package is uploaded to
--base-url plus its file name. A new version starts the file afresh.
"""
import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

PLATFORM = re.compile(r"^(macos|windows|linux)-(aarch64|x86_64|universal)$")
# The launcher only downloads packages whose names are plain (update.rs).
PLAIN_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+-]{0,119}$")
KNOWN = (".dmg", ".exe", ".msi", ".appimage", ".deb", ".rpm", ".zip")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--page", required=True, help="the release page (https)")
    parser.add_argument("--base-url", required=True, help="where the packages are uploaded (https)")
    parser.add_argument("--notes", default="")
    parser.add_argument("packages", nargs="+", metavar="platform=file")
    args = parser.parse_args()

    for url in (args.page, args.base_url):
        if not url.startswith("https://"):
            sys.exit(f"not an https address: {url}")
    base = args.base_url if args.base_url.endswith("/") else args.base_url + "/"

    manifest = {}
    if args.manifest.exists():
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
        if manifest.get("version") != args.version:
            manifest = {}
    manifest.update({"version": args.version, "url": args.page})
    if args.notes or "notes" not in manifest:
        manifest["notes"] = args.notes
    downloads = manifest.setdefault("downloads", {})

    for item in args.packages:
        platform, _, file = item.partition("=")
        path = Path(file)
        if not PLATFORM.match(platform):
            sys.exit(f"unknown platform {platform!r}: use e.g. macos-aarch64, windows-x86_64, linux-x86_64")
        if not path.is_file():
            sys.exit(f"no such package: {path}")
        if not PLAIN_NAME.match(path.name) or not path.name.lower().endswith(KNOWN):
            sys.exit(f"the launcher won't download {path.name!r}: use letters, digits and . _ - + and a package extension")
        downloads[platform] = {"url": base + path.name, "sha256": sha256(path)}
        print(f"{platform}: {path.name} {downloads[platform]['sha256']}")

    args.manifest.parent.mkdir(parents=True, exist_ok=True)
    args.manifest.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {args.manifest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
