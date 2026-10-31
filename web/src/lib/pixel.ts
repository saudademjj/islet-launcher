// Seeded pixel art. Every instance gets its own little world, generated from
// its id the way a Minecraft seed generates terrain; the result is a handful
// of SVG paths, one per colour, so even a wide landscape is a few DOM nodes.

export type Paint = "x1" | "x2" | "s1" | "s2" | "m1" | "m2" | "c" | "h1" | "h2" | "k" | "k2" | "v" | "o1" | "o2" | "o3" | "d" | "d2" | "d3" | "g" | "g2" | "g3" | "w" | "l" | "l2" | "f";

/** Painting order: later paints cover earlier ones. */
export const PAINT_ORDER: Paint[] = ["x1", "x2", "s1", "s2", "m1", "m2", "c", "h1", "h2", "k", "k2", "o1", "o2", "o3", "v", "d", "d2", "d3", "g", "g2", "g3", "w", "l", "l2", "f"];

export interface PixelArt {
  width: number;
  height: number;
  paths: Partial<Record<Paint, string>>;
}

/** FNV-1a, 32 bit. */
export function hashSeed(seed: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < seed.length; i++) {
    hash ^= seed.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/** mulberry32: small, fast and good enough for scenery. */
export function random(seed: number): () => number {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

class Painter {
  private parts: Partial<Record<Paint, string[]>> = {};
  rect(paint: Paint, x: number, y: number, w: number, h: number) {
    if (w <= 0 || h <= 0) return;
    (this.parts[paint] ??= []).push(`M${x} ${y}h${w}v${h}h${-w}z`);
  }
  done(width: number, height: number): PixelArt {
    const paths: PixelArt["paths"] = {};
    for (const [paint, list] of Object.entries(this.parts)) paths[paint as Paint] = list!.join("");
    return { width, height, paths };
  }
}

export interface TerrainOptions {
  /** Columns of blocks; each block is 4×4 pixels. */
  cols: number;
  minHeight: number;
  maxHeight: number;
  /** Free blocks above the highest ground, room for trees. */
  sky: number;
  trees: boolean;
  /** Leave the first columns treeless (text sits above them). */
  treesFrom?: number;
  /** Chance of a tree on a flat column. */
  treeChance?: number;
  /** What grows on the surface; oak when omitted. */
  flora?: Flora;
  /** Soil in horizontal bands that line up across columns. */
  strata?: boolean;
  /** Blocks of soil under the surface before stone begins, with ores and a
      cave: a cross-section of the world. Off when omitted. */
  soilDepth?: number;
}

export type Flora = "oak" | "cherry" | "spruce" | "cactus" | "mushroom" | "dead";

/** Terracotta bands, counted from the bottom. */
const STRATA: Paint[] = ["d", "d3", "d", "d2", "d3", "d2"];

const B = 4;

export function terrain(seed: string, o: TerrainOptions): PixelArt {
  const r = random(hashSeed(seed));
  const height = (o.maxHeight + o.sky) * B;
  const p = new Painter();

  // Ground: a random walk, one block at a time, without one-column notches.
  const heights: number[] = [];
  let h = Math.round((o.minHeight + o.maxHeight) / 2);
  for (let x = 0; x < o.cols; x++) {
    if (r() < 0.3) h = Math.min(o.maxHeight, Math.max(o.minHeight, h + (r() < 0.5 ? -1 : 1)));
    heights.push(h);
  }
  for (let x = 1; x < o.cols - 1; x++) {
    if (heights[x - 1] === heights[x + 1] && heights[x] !== heights[x - 1]) heights[x] = heights[x - 1];
  }

  let lastTree = -99;
  for (let x = 0; x < o.cols; x++) {
    const ground = heights[x];
    const top = height - ground * B;
    const left = x * B;
    const soil = o.soilDepth ? Math.min(ground, o.soilDepth) : ground;
    if (soil < ground) stone(p, left, top + soil * B, ground - soil, r);
    if (o.strata) {
      for (let b = 0; b < soil; b++) p.rect(STRATA[(ground - 1 - b) % STRATA.length], left, top + b * B, B, B);
    } else {
      p.rect("d", left, top, B, soil * B);
      // Dirt specks; the top block keeps its upper rows for the surface.
      for (let b = 0; b < soil; b++) {
        const rows = b === 0 ? [3] : [0, 1, 2, 3];
        p.rect("d2", left + Math.floor(r() * B), top + b * B + rows[Math.floor(r() * rows.length)], 1, 1);
        if (r() < 0.55) p.rect("d3", left + Math.floor(r() * B), top + b * B + rows[Math.floor(r() * rows.length)], 1, 1);
      }
    }
    // Grass: a band with ragged drips, like the side of a grass block.
    p.rect("g", left, top, B, 1);
    for (let s = 0; s < B; s++) {
      const drip = r();
      if (drip < 0.55) p.rect("g2", left + s, top + 1, 1, drip < 0.2 ? 2 : 1);
    }
    if (r() < 0.45) p.rect("g3", left + Math.floor(r() * B), top, 1, 1);

    const flora = o.flora ?? "oak";
    const flat = x > 0 && x < o.cols - 1 && heights[x - 1] === ground && heights[x + 1] === ground;
    if (o.trees && flora !== "dead" && flat && x >= (o.treesFrom ?? 2) && x - lastTree >= 5 && r() < (o.treeChance ?? 0.16)) {
      lastTree = x;
      plant(p, flora, left, top, r);
    } else if (o.trees && r() < 0.2) {
      const s = Math.floor(r() * B);
      if (flora === "cactus" || flora === "dead") {
        // Dead bush.
        p.rect("w", left + s, top - 2, 1, 2);
        p.rect("w", Math.max(left, left + s - 1), top - 3, 1, 1);
      } else {
        // Tall grass, sometimes with a flower.
        const tall = r() < 0.4 ? 2 : 1;
        p.rect("g", left + s, top - tall, 1, tall);
        if (r() < 0.3) p.rect("f", left + s, top - tall - 1, 1, 1);
      }
    }
  }
  if (o.soilDepth) cave(p, heights, o.soilDepth, height, r);
  return p.done(o.cols * B, height);
}

/** A column of stone `blocks` high from `y` down, with the odd ore. */
function stone(p: Painter, left: number, y: number, blocks: number, r: () => number) {
  p.rect("k", left, y, B, blocks * B);
  for (let b = 0; b < blocks; b++) {
    const by = y + b * B;
    if (r() < 0.5) p.rect("k2", left + Math.floor(r() * 3), by + Math.floor(r() * 3), 2, 1);
    const ore = r();
    // Deeper blocks hold rarer ores, as in the game.
    const paint: Paint | null = ore < 0.05 ? "o1" : ore < 0.075 ? "o2" : ore < 0.083 && b >= blocks - 2 ? "o3" : null;
    if (paint) {
      p.rect(paint, left + 1, by + 1, 2, 1);
      p.rect(paint, left + (r() < 0.5 ? 0 : 2), by + 2, 2, 1);
    }
  }
}

/** One small cave in the stone of the right half: a lens of dark air. */
function cave(p: Painter, heights: number[], depth: number, height: number, r: () => number) {
  const cols = heights.length;
  const start = Math.floor(cols * (0.55 + r() * 0.3));
  const width = 5 + Math.floor(r() * 4);
  for (let i = 0; i < width && start + i < cols; i++) {
    const stoneBlocks = heights[start + i] - depth;
    if (stoneBlocks < 3) continue;
    // Taller in the middle, one block at the ends.
    const edge = Math.min(i, width - 1 - i);
    const tall = Math.min(stoneBlocks - 2, edge === 0 ? 1 : 2);
    const floor = height - B;
    p.rect("v", (start + i) * B, floor - tall * B - (edge === 0 ? 0 : 1), B, tall * B + (edge === 0 ? 0 : 1));
  }
}

/** One plant on a flat column; `left` is the column, `top` the surface. */
function plant(p: Painter, flora: Flora, left: number, top: number, r: () => number) {
  switch (flora) {
    case "spruce": {
      const trunk = (2 + (r() < 0.5 ? 1 : 0)) * B;
      p.rect("w", left + 1, top - trunk, 2, trunk);
      // Tiers from the bottom up, three pixels each, centred on the trunk.
      let y = top - trunk + 2;
      for (const w of [12, 8, 10, 6, 8, 4, 2]) {
        p.rect("l", left + 2 - w / 2, y - 3, w, 3);
        y -= 3;
      }
      for (let i = 0; i < 5; i++) p.rect("l2", left - 3 + Math.floor(r() * 10), top - trunk - 12 + Math.floor(r() * 14), 1, 1);
      return;
    }
    case "cactus": {
      const h = (2 + (r() < 0.5 ? 1 : 0)) * B;
      p.rect("l", left, top - h, B, h);
      p.rect("l2", left + 1, top - h, 1, h);
      if (r() < 0.6) {
        const y = top - h + 3 + Math.floor(r() * 3);
        if (r() < 0.5) {
          p.rect("l", left - 2, y, 2, 2);
          p.rect("l", left - 3, y - 3, 2, 4);
        } else {
          p.rect("l", left + B, y, 2, 2);
          p.rect("l", left + B + 1, y - 3, 2, 4);
        }
      }
      return;
    }
    case "mushroom": {
      const stem = 3 * B;
      p.rect("w", left + 1, top - stem, 2, stem);
      const cap = top - stem - B + 1;
      p.rect("l", left - 2 * B + 2, cap, 5 * B - 4, B);
      p.rect("l", left - B, cap - 2, 3 * B, 2);
      for (let i = 0; i < 5; i++) p.rect("l2", left - 2 * B + 3 + Math.floor(r() * (5 * B - 6)), cap - 1 + Math.floor(r() * 4), 1, 1);
      return;
    }
    case "cherry": {
      const trunk = 3 * B;
      p.rect("w", left + 1, top - trunk, 2, trunk);
      const canopy = top - trunk + 2;
      p.rect("l", left - 2 * B + 2, canopy - 2 * B, 5 * B - 4, 2 * B);
      p.rect("l", left - B, canopy - 3 * B, 3 * B, B);
      for (let i = 0; i < 9; i++) p.rect("l2", left - 2 * B + 2 + Math.floor(r() * (5 * B - 4)), canopy - 3 * B + Math.floor(r() * 3 * B), 1, 1);
      // Fallen petals.
      for (let i = 0; i < 3; i++) p.rect("f", left - 2 * B + Math.floor(r() * 5 * B), top - 1, 1, 1);
      return;
    }
    default: {
      const trunk = (2 + (r() < 0.5 ? 1 : 0)) * B;
      p.rect("w", left + 1, top - trunk, 2, trunk);
      const canopy = top - trunk + 2;
      p.rect("l", left - B, canopy - 2 * B, 3 * B, 2 * B);
      p.rect("l", left - B / 2, canopy - 3 * B, 2 * B, B);
      for (let i = 0; i < 7; i++) p.rect("l2", left - B + Math.floor(r() * 3 * B), canopy - 3 * B + Math.floor(r() * 3 * B), 1, 1);
    }
  }
}

/** Flat blocky clouds for the hero sky. */
export function clouds(seed: string, cols: number, rows: number): PixelArt {
  const r = random(hashSeed(`${seed}:sky`));
  const p = new Painter();
  // Roughly one cloud per 30 columns, so a wide sky is not left empty.
  const count = Math.max(3, Math.round(cols / 30)) + Math.floor(r() * 2);
  for (let i = 0; i < count; i++) {
    const w = 4 + Math.floor(r() * 4);
    // Spread across the width, one cloud per slot, so they never pile up.
    const x = Math.floor(((i + r() * 0.6) / count) * cols);
    const y = 1 + Math.floor(r() * (rows - 2));
    p.rect("c", x * B, y * B, w * B, B);
    p.rect("c", (x + 1) * B, (y - 1) * B + B / 2, (w - 2 - Math.floor(r() * 2)) * B, B / 2);
  }
  return p.done(cols * B, rows * B);
}

/** A blocky hill silhouette in one paint. `step` is the largest change in
    height between runs; `bumps` adds half-block tree tops along the ridge. */
export function hills(seed: string, o: { cols: number; minHeight: number; maxHeight: number; paint: Paint; step: number; bumps?: boolean }): PixelArt {
  const r = random(hashSeed(`${seed}:${o.paint}`));
  const p = new Painter();
  const height = (o.maxHeight + 1) * B;
  let h = o.minHeight + Math.floor(r() * (o.maxHeight - o.minHeight + 1));
  let run = 0;
  for (let x = 0; x < o.cols; x++) {
    if (run-- <= 0) {
      h = Math.min(o.maxHeight, Math.max(o.minHeight, h + Math.floor(r() * (2 * o.step + 1)) - o.step));
      run = 1 + Math.floor(r() * 4);
    }
    const top = height - h * B - (o.bumps && r() < 0.4 ? B / 2 : 0);
    p.rect(o.paint, x * B, top, B, height - top);
  }
  return p.done(o.cols * B, height);
}

/** Night sky: dim stars with a few bright ones. */
export function stars(seed: string, cols: number, rows: number): PixelArt {
  const r = random(hashSeed(`${seed}:stars`));
  const p = new Painter();
  const width = cols * B;
  const height = rows * B;
  for (let i = 0; i < cols; i++) {
    p.rect(r() < 0.2 ? "x2" : "x1", Math.floor(r() * width), Math.floor(r() * height), 1, 1);
  }
  return p.done(width, height);
}

/** A multi-colour grid: each character maps to a paint, "." is empty. */
export function paintGrid(rows: string[], legend: Record<string, Paint>): PixelArt {
  const p = new Painter();
  rows.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      const ch = row[x];
      let end = x + 1;
      while (end < row.length && row[end] === ch) end++;
      if (legend[ch]) p.rect(legend[ch], x, y, end - x, 1);
      x = end;
    }
  });
  return p.done(rows[0].length, rows.length);
}

/** One-colour icon grid ("#" filled) as a single path. */
export function gridPath(rows: string[]): string {
  return paintGrid(rows, { "#": "g" }).paths.g ?? "";
}
