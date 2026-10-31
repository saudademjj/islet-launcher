"""Draws the app icon into src-tauri/icons: the sidebar's grass block
(BrandMark in web/src/components/Pixel.tsx) floating under a square sun in
the home page's night sky. Every size is drawn on its own whole-pixel grid
instead of being scaled down, so small icons stay sharp. Explorer's small
icons use a compact island with the same sun and sky as the large icon.

    python3 scripts/app-icon.py        (needs Pillow; macOS for the .icns)
"""
import io
import shutil
import struct
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

ICONS = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"

# Colour tokens from web/src/styles/tokens.css.
HEX = {
    "K": "#1e4027",  # --hero, the sky
    "g": "#7fb238", "G": "#64952a", "h": "#9ccb52",  # grass, dark, light
    "d": "#8b5a33", "D": "#74482a", "e": "#a06b40",  # dirt, dark, light
    "s": "#4f5a4c", "z": "#414b3f",  # stone, dark
    "S": "#efc63f", "T": "#f7dc6a",  # sun, lit corner
    "o": "#d3e6b4", "O": "#4f7a57",  # stars, bright and dim
}
RGBA = {k: (int(v[1:3], 16), int(v[3:5], 16), int(v[5:7], 16), 255) for k, v in HEX.items()}

BLOCK = ["ghgggghg", "gggGgggg", "GdgGdGgd", "dddGdddd", "dDdddeDd", "ddedDddd", "dDdddddD", "dddeddDd"]


def island():
    """32x32 cells: the sky tile with stepped corners, the sun, a few stars,
    the block at twice its size and a ragged underside."""
    n = 32
    cells = [["K"] * n for _ in range(n)]
    for y in range(4, 8):
        for x in range(23, 27):
            cells[y][x] = "S"
    cells[4][23] = cells[4][24] = cells[5][23] = "T"
    for y, x, ch in ((5, 6, "o"), (9, 12, "O"), (3, 15, "O"), (12, 27, "o"), (14, 4, "O")):
        cells[y][x] = ch
    for y, row in enumerate(BLOCK):
        for x, ch in enumerate(row):
            for dy in range(2):
                for dx in range(2):
                    cells[9 + 2 * y + dy][8 + 2 * x + dx] = ch
    depth = [1, 2, 1, 3, 2, 4, 3, 5, 4, 3, 5, 2, 3, 1, 2, 1]
    for i, rows in enumerate(depth):
        x = 8 + i
        for j in range(rows):
            y = 25 + j
            if j >= 2:
                cells[y][x] = "z" if (x + y) % 3 == 0 else "s"
            else:
                cells[y][x] = "D" if (x + y) % 2 else "d"
    for k in range(2):
        for j in range(2 - k):
            for y, x in ((k, j), (k, n - 1 - j), (n - 1 - k, j), (n - 1 - k, n - 1 - j)):
                cells[y][x] = None
    return cells


def render(cells, scale, size):
    """Cells at a whole-pixel scale, centred on a transparent square."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    off = (size - len(cells) * scale) // 2
    px = img.load()
    for y, row in enumerate(cells):
        for x, ch in enumerate(row):
            if ch:
                for yy in range(scale):
                    for xx in range(scale):
                        px[off + x * scale + xx, off + y * scale + yy] = RGBA[ch]
    return img


def icon(size, margin=0.0):
    """`margin` is the share of the canvas left empty on each side (macOS
    icons sit inside a margin; Windows icons fill the square)."""
    inner = size - 2 * round(size * margin)
    if inner < 32:
        cells = [["K"] * 16 for _ in range(16)]
        for y in (2, 3):
            for x in (11, 12):
                cells[y][x] = "S"
        cells[2][11] = "T"
        cells[3][3] = "o"
        cells[6][13] = "O"
        for y, row in enumerate(BLOCK):
            for x, ch in enumerate(row):
                cells[5 + y][4 + x] = ch
        for x, depth in enumerate((1, 2, 1, 2, 2, 1, 2, 1)):
            for y in range(depth):
                cells[13 + y][4 + x] = "z" if y else "D"
        for y, x in ((0, 0), (0, 15), (15, 0), (15, 15)):
            cells[y][x] = None
        return render(cells, max(1, inner // 16), size)
    return render(island(), inner // 32, size)


def png(img):
    out = io.BytesIO()
    img.save(out, "PNG", optimize=True)
    return out.getvalue()


def write_ico(path, sizes):
    images = [png(icon(s)) for s in sizes]
    header = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries = b""
    for size, data in zip(sizes, images):
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    path.write_bytes(header + entries + b"".join(images))


def write_icns(path):
    mac = 0.09  # the usual macOS icon margin
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "icon.iconset"
        iconset.mkdir()
        for base in (16, 32, 128, 256, 512):
            icon(base, mac).save(iconset / f"icon_{base}x{base}.png")
            icon(base * 2, mac).save(iconset / f"icon_{base}x{base}@2x.png")
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(path)], check=True)


def main():
    icon(32).save(ICONS / "32x32.png")
    icon(128).save(ICONS / "128x128.png")
    icon(256).save(ICONS / "128x128@2x.png")
    icon(512).save(ICONS / "icon.png")
    write_ico(ICONS / "icon.ico", [16, 20, 24, 32, 40, 48, 64, 256])
    if shutil.which("iconutil"):
        write_icns(ICONS / "icon.icns")
    else:
        print("iconutil not found (macOS only); icon.icns left as it was")


if __name__ == "__main__":
    main()
