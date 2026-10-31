import { GLYPH_ALIASES, GLYPHS, type GlyphName } from "../lib/glyphs";
import { SCENE_PX, SMALL_PX, TYPE_PX } from "../lib/pixelScale";

export type IconName = GlyphName;

/** Runs of one character in a drawing as an SVG path of unit rectangles. */
function runs(rows: string[], ch: string) {
  let d = "";
  rows.forEach((row, y) => {
    for (let x = 0; x < row.length; ) {
      if (row[x] !== ch) {
        x++;
        continue;
      }
      let end = x + 1;
      while (row[end] === ch) end++;
      d += `M${x} ${y}h${end - x}v1h${x - end}z`;
      x = end;
    }
  });
  return d;
}

const PATHS = Object.fromEntries(
  Object.entries(GLYPHS).map(([name, art]) => {
    const rows = art.split("\n").map((row) => row.trim()).filter(Boolean);
    return [name, { ink: runs(rows, "#"), tone: runs(rows, "+") }];
  }),
) as Record<keyof typeof GLYPHS, { ink: string; tone: string }>;

/** A 16x16 pixel icon at one of three sizes: 16 beside text (one pixel per
    pixel-font pixel), 32 in tiles and empty states, 48 at the scenery pixel.
    Each is snapped to whole device pixels. */
export function Icon({ name, size = 16, className }: { name: IconName; size?: 16 | 32 | 48; className?: string }) {
  const glyph = PATHS[name in GLYPH_ALIASES ? GLYPH_ALIASES[name as keyof typeof GLYPH_ALIASES] : (name as keyof typeof GLYPHS)];
  const px = 16 * (size === 16 ? TYPE_PX : size === 32 ? SMALL_PX : SCENE_PX);
  return (
    <svg
      className={`icon${className ? ` ${className}` : ""}`}
      width={px}
      height={px}
      viewBox="0 0 16 16"
      shapeRendering="crispEdges"
      fill="currentColor"
      aria-hidden
    >
      {glyph.tone && <path className="icon-tone" d={glyph.tone} />}
      <path d={glyph.ink} />
    </svg>
  );
}
