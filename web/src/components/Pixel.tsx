import { memo, useMemo } from "react";

import { SCENE_PX } from "../lib/pixelScale";
import { clouds, gridPath, hills, paintGrid, PAINT_ORDER, stars, terrain, type Flora, type PixelArt, type TerrainOptions } from "../lib/pixel";

/** One pixel of scenery is about 3 CSS pixels everywhere it appears, so
    scenes, the sun, avatars and the brand mark never mix pixel sizes. On
    fractional display scaling it is nudged to land on whole device pixels. */
export const PX = SCENE_PX;

/** The largest whole multiple of PX that fits in `size`, at least PX. */
export const snap = (size: number, grid: number) => Math.max(1, Math.round(size / grid / PX)) * PX;

/** Pixel art at an integer scale; paints are coloured by CSS (`.px-*`). */
export function PixelSvg({ art, unit, className }: { art: PixelArt; unit: number; className?: string }) {
  return (
    <svg
      className={`pixel${className ? ` ${className}` : ""}`}
      width={art.width * unit}
      height={art.height * unit}
      viewBox={`0 0 ${art.width} ${art.height}`}
      shapeRendering="crispEdges"
      aria-hidden
    >
      {PAINT_ORDER.map((paint) => art.paths[paint] && <path key={paint} className={`px-${paint}`} d={art.paths[paint]} />)}
    </svg>
  );
}

type TerrainProps = TerrainOptions & { seed: string; unit: number; className?: string };

/** The instance's own landscape, generated from its id. */
export const Terrain = memo(function Terrain({ seed, unit, className, cols, minHeight, maxHeight, sky, trees, treesFrom, treeChance, flora, strata }: TerrainProps) {
  const art = useMemo(
    () => terrain(seed, { cols, minHeight, maxHeight, sky, trees, treesFrom, treeChance, flora, strata }),
    [seed, cols, minHeight, maxHeight, sky, trees, treesFrom, treeChance, flora, strata],
  );
  return <PixelSvg art={art} unit={unit} className={className} />;
});

type Range = [number, number];

/** Layered scenery: far hills, near hills, then the ground with trees. Each
    layer is its own SVG, so hovering pans them at different speeds. */
export const Landscape = memo(function Landscape({
  seed,
  unit,
  cols,
  ground,
  sky,
  treesFrom,
  treeChance,
  far,
  near,
  biome,
  soilDepth,
}: {
  seed: string;
  unit: number;
  cols: number;
  ground: Range;
  sky: number;
  treesFrom?: number;
  treeChance?: number;
  far?: Range;
  near?: Range;
  /** What grows and how the soil is drawn; plains when omitted. */
  biome?: { flora: Flora; strata?: boolean };
  /** Show stone, ores and a cave below this many blocks of soil. */
  soilDepth?: number;
}) {
  const art = useMemo(
    () => ({
      far: far && hills(seed, { cols, minHeight: far[0], maxHeight: far[1], paint: "h1", step: 2 }),
      near: near && hills(seed, { cols, minHeight: near[0], maxHeight: near[1], paint: "h2", step: 1, bumps: true }),
      ground: terrain(seed, { cols, minHeight: ground[0], maxHeight: ground[1], sky, trees: true, treesFrom, treeChance, flora: biome?.flora, strata: biome?.strata, soilDepth }),
    }),
    // Ranges are literals at the call sites; compare their values.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [seed, cols, sky, treesFrom, treeChance, biome?.flora, biome?.strata, soilDepth, far?.[0], far?.[1], near?.[0], near?.[1], ground[0], ground[1]],
  );
  return (
    <div className="landscape" aria-hidden>
      {art.far && (
        <div className="land far">
          <PixelSvg art={art.far} unit={unit} />
        </div>
      )}
      {art.near && (
        <div className="land near">
          <PixelSvg art={art.near} unit={unit} />
        </div>
      )}
      <div className="land ground">
        <PixelSvg art={art.ground} unit={unit} />
      </div>
    </div>
  );
});

export const Clouds = memo(function Clouds({ seed, cols, rows, unit, className }: { seed: string; cols: number; rows: number; unit: number; className?: string }) {
  const art = useMemo(() => clouds(seed, cols, rows), [seed, cols, rows]);
  return <PixelSvg art={art} unit={unit} className={className} />;
});

export const Stars = memo(function Stars({ seed, cols, rows, unit, className }: { seed: string; cols: number; rows: number; unit: number; className?: string }) {
  const art = useMemo(() => stars(seed, cols, rows), [seed, cols, rows]);
  return <PixelSvg art={art} unit={unit} className={className} />;
});

// Square sun and moon, as in the game, on a 16×16 grid.
const SUN = paintGrid(
  Array.from({ length: 16 }, (_, y) => Array.from({ length: 16 }, (_, x) => (x < 2 || x > 13 || y < 2 || y > 13 ? "a" : (x + y) % 7 === 0 && x > 3 && x < 12 ? "a" : "b")).join("")),
  { a: "s1", b: "s2" },
);
const MOON = paintGrid(
  [
    "aaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaa",
    "aabbaaaaaaaaaaaa",
    "aabbaaaaaabbbaaa",
    "aaaaaaaaaabbbaaa",
    "aaaaaaaaaabbbaaa",
    "aaaaabbaaaaaaaaa",
    "aaaaabbaaaaaaaaa",
    "aaaaaaaaaaaaaaaa",
    "abbbaaaaaaaabbaa",
    "abbbaaaaaaaabbaa",
    "abbbaaaaaaaaaaaa",
    "aaaaaaaabbaaaaaa",
    "aaaaaaaabbaaaaaa",
    "aaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaa",
  ],
  { a: "m1", b: "m2" },
);

/** 16 pixels square at PX: 48 CSS pixels. */
export function Celestial({ night }: { night: boolean }) {
  return <PixelSvg art={night ? MOON : SUN} unit={PX} />;
}

const BRAND = paintGrid(
  ["ghgggghg", "gggGgggg", "GdgGdGgd", "dddGdddd", "dDdddeDd", "ddedDddd", "dDdddddD", "dddeddDd"],
  { g: "g", h: "g3", G: "g2", d: "d", D: "d2", e: "d3" },
);

/** A grass block: the launcher's mark. Its size snaps to whole PX pixels. */
export function BrandMark({ size = 32 }: { size?: number }) {
  const unit = snap(size, 8);
  return (
    <span className="brand-mark" style={{ width: unit * 8, height: unit * 8 }}>
      <PixelSvg art={BRAND} unit={unit} />
    </span>
  );
}

// A signpost, the way the game posts a message: what a notice shows in
// place of a warning triangle. Two pale planks on a post; the mark is
// painted on in the notice's tone, so a failure is a few red pixels on
// wood, not a red tile.
const SIGN = [
  ".eeeeeeeeee.",
  "elllllllllle",
  "effffffffffe",
  "effffffffffe",
  "esssffffssse",
  "elllffffllle",
  "effffffffffe",
  "esssssssssse",
  ".eeeeeeeeee.",
  ".....pq.....",
  ".....pq.....",
  ".....pq.....",
];

const SIGN_PAINTS = { e: "edge", l: "light", f: "face", s: "shade", p: "post", q: "post-shade" };

const SIGN_PATHS = Object.entries(SIGN_PAINTS).map(([ch, paint]) => [paint, gridPath(SIGN.map((row) => row.replace(/./g, (c) => (c === ch ? "#" : "."))))]);

// Painted on the board, rows 2 to 6.
const SIGN_MARKS = {
  alert: ["", "", ".....##.....", ".....##.....", ".....##.....", "............", ".....##....."],
  info: ["", "", ".....##.....", "............", ".....##.....", ".....##.....", ".....##....."],
  check: ["", "", "........##..", ".......##...", "..##..##....", "...####.....", "....##......"],
  download: ["", "", ".....##.....", ".....##.....", "...#.##.#...", "....####....", ".....##....."],
};

export type SignMark = keyof typeof SIGN_MARKS;

const SIGN_MARK_PATHS = Object.fromEntries(Object.entries(SIGN_MARKS).map(([name, rows]) => [name, gridPath(rows.map((row) => row.padEnd(12, ".")))])) as Record<SignMark, string>;

/** 12×12 at whole scenery pixels: 36 CSS pixels, or 24 at `unit` 2. */
export function Sign({ mark = "alert", tone, unit = PX }: { mark?: SignMark; tone?: "danger" | "warn" | "ok"; unit?: number }) {
  return (
    <svg className={`pixel sign${tone ? ` ${tone}` : ""}`} width={12 * unit} height={12 * unit} viewBox="0 0 12 12" shapeRendering="crispEdges" aria-hidden>
      {SIGN_PATHS.map(([paint, d]) => (
        <path key={paint} className={`sign-${paint}`} d={d} />
      ))}
      <path className="sign-mark" d={SIGN_MARK_PATHS[mark]} />
    </svg>
  );
}
