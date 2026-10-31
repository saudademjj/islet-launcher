import { useEffect, useMemo, useState } from "react";

import type { Account } from "../api/types";
import { hashSeed, random } from "../lib/pixel";

// A player is shown by their face, the 8×8 front of the head on their skin,
// as the game and every launcher show them. Microsoft accounts use their
// real skin; offline accounts (and skins that fail to load) get an original
// face generated from the name, so it stays the same across launches and
// already shows while the name is typed.

/** Mojang serves textures over plain http; the window only loads https. */
export const secure = (url: string) => url.replace(/^http:\/\//, "https://");

export interface Skin {
  url: string;
  /** An old 64×32 skin, whose left limbs mirror the right. */
  legacy: boolean;
  /** Draw the hat layer. */
  hat: boolean;
}

/** Loads a skin texture; null while loading or when it cannot be used. */
export function useSkin(url: string | null | undefined): Skin | null {
  const [skin, setSkin] = useState<Skin | null>(null);
  useEffect(() => {
    setSkin(null);
    if (!url) return;
    let live = true;
    const src = secure(url);
    const image = new Image();
    image.onload = () => {
      if (image.naturalWidth !== 64 || (image.naturalHeight !== 64 && image.naturalHeight !== 32)) return;
      if (image.naturalHeight === 64) return setSkin({ url: src, legacy: false, hat: true });
      // The game drops an old skin's hat layer when that whole area is
      // opaque (many were painted solid there), so check the pixels. If
      // they cannot be read, draw the hat as before.
      void legacyHatShows(src).then((hat) => live && setSkin({ url: src, legacy: true, hat }));
    };
    image.src = src;
    return () => {
      live = false;
      image.onload = null;
    };
  }, [url]);
  return skin;
}

/** False when the overlay half of a 64×32 skin (x 32–63) has no
    transparent pixel, which the game treats as "no overlay". */
function legacyHatShows(src: string): Promise<boolean> {
  return new Promise((resolve) => {
    const probe = new Image();
    probe.crossOrigin = "anonymous";
    probe.onload = () => {
      try {
        const canvas = document.createElement("canvas");
        canvas.width = 64;
        canvas.height = 32;
        const context = canvas.getContext("2d");
        if (!context) return resolve(true);
        context.drawImage(probe, 0, 0);
        const alpha = context.getImageData(32, 0, 32, 32).data;
        for (let i = 3; i < alpha.length; i += 4) if (alpha[i] < 128) return resolve(true);
        resolve(false);
      } catch {
        resolve(true);
      }
    };
    probe.onerror = () => resolve(true);
    probe.src = src;
  });
}

/** One rectangle of a skin texture, scaled by `px`, as a CSS background. */
function crop(url: string, px: number, x: number, y: number, legacy = false) {
  return {
    backgroundImage: `url("${url}")`,
    backgroundSize: `${64 * px}px ${(legacy ? 32 : 64) * px}px`,
    backgroundPosition: `${-x * px}px ${-y * px}px`,
  };
}

const SKIN_TONES = ["#f2c9a0", "#e0ac7e", "#c98e5f", "#a86f45", "#7d5034", "#f6d8bd"];
const HAIR = ["#3b2a1f", "#5a3a22", "#2a2522", "#8a5a2b", "#c8a45a", "#9c3f2a", "#6b6b6b", "#24324a"];
const EYES = ["#3a5fa8", "#4c8a3e", "#5a3a22", "#2a2522", "#6a4fa0"];

/** An original 8×8 face from a seed: skin tone, hair, eyes, maybe a beard. */
function faceGrid(seed: string): string[][] {
  const r = random(hashSeed(`face:${seed}`));
  const pick = <T,>(list: T[]) => list[Math.floor(r() * list.length)];
  const skin = pick(SKIN_TONES);
  const hair = pick(HAIR);
  const eye = pick(EYES);
  const shade = shadeOf(skin);
  const grid = Array.from({ length: 8 }, () => Array<string>(8).fill(skin));
  // Hair: the top rows, a fringe, and sides of a chosen length.
  const fringe = 1 + Math.floor(r() * 2);
  const sides = 2 + Math.floor(r() * 3);
  for (let y = 0; y < 8; y++)
    for (let x = 0; x < 8; x++) {
      if (y < fringe) grid[y][x] = hair;
      else if (y < sides && (x === 0 || x === 7)) grid[y][x] = hair;
    }
  if (r() < 0.5) grid[fringe][1 + Math.floor(r() * 5)] = hair;
  // Eyes on row 4: white, then the iris toward the nose.
  grid[4][1] = "#ffffff";
  grid[4][2] = eye;
  grid[4][5] = eye;
  grid[4][6] = "#ffffff";
  // Nose and mouth in a darker tone; a beard in the hair colour sometimes.
  grid[5][3] = grid[5][4] = shade;
  const beard = r() < 0.25;
  for (let x = 2; x < 6; x++) grid[6][x] = beard ? hair : x === 2 || x === 5 ? skin : shade;
  if (beard) for (let x = 1; x < 7; x++) grid[7][x] = hair;
  return grid;
}

function shadeOf(hex: string): string {
  const n = parseInt(hex.slice(1), 16);
  const c = (s: number) => Math.round(((n >> s) & 255) * 0.78);
  return `#${((c(16) << 16) | (c(8) << 8) | c(0)).toString(16).padStart(6, "0")}`;
}

/** The face for an account (its skin) or for a name being typed. */
export function PlayerFace({ account, name, size = 36, className }: { account?: Account | null; name?: string; size?: number; className?: string }) {
  const skin = useSkin(account?.skinUrl);
  const seed = (account?.username ?? name ?? "player").toLowerCase();
  const grid = useMemo(() => faceGrid(seed), [seed]);
  // Whole pixels: the face is 8 texels, so the size snaps to a multiple of 8.
  const px = Math.max(1, Math.round(size / 8));
  const box = px * 8;
  const classes = `player-face${className ? ` ${className}` : ""}`;
  if (skin) {
    return (
      <span className={classes} style={{ width: box, height: box }} aria-hidden>
        <span className="skin-part" style={{ inset: 0, ...crop(skin.url, px, 8, 8, skin.legacy) }} />
        {/* The hat layer sits over the face; old skins have it too. */}
        {skin.hat && <span className="skin-part" style={{ inset: 0, ...crop(skin.url, px, 40, 8, skin.legacy) }} />}
      </span>
    );
  }
  return (
    <svg className={classes} width={box} height={box} viewBox="0 0 8 8" shapeRendering="crispEdges" aria-hidden>
      {grid.flatMap((row, y) => row.map((fill, x) => <rect key={`${x}-${y}`} x={x} y={y} width={1} height={1} fill={fill} />))}
    </svg>
  );
}

/** The whole front of a skin (head, body, arms, legs), for the profile card.
    Null for accounts without a usable skin; the face stands in then. */
export function SkinFront({ account, px = 5 }: { account: Account; px?: number }) {
  const skin = useSkin(account.skinUrl);
  if (!skin) return null;
  const { url, legacy, hat } = skin;
  // Slim skins have 3-pixel arms against the body; old skins are never slim.
  const arm = !legacy && account.skinSlim ? 3 : 4;
  // [texture x, y, width, height, place x, y, mirrored]
  const parts: ([number, number, number, number, number, number, boolean] | null)[] = [
    [8, 8, 8, 8, 4, 0, false],
    hat ? [40, 8, 8, 8, 4, 0, false] : null,
    [20, 20, 8, 12, 4, 8, false],
    [44, 20, arm, 12, 4 - arm, 8, false],
    legacy ? [44, 20, 4, 12, 12, 8, true] : [36, 52, arm, 12, 12, 8, false],
    [4, 20, 4, 12, 4, 20, false],
    legacy ? [4, 20, 4, 12, 8, 20, true] : [20, 52, 4, 12, 8, 20, false],
  ];
  return (
    <span className="skin-front" style={{ width: 16 * px, height: 32 * px }} aria-hidden>
      {parts.map((part, i) => {
        if (!part) return null;
        const [tx, ty, w, h, x, y, mirrored] = part;
        return (
          <span
            key={i}
            className="skin-part"
            style={{ left: x * px, top: y * px, width: w * px, height: h * px, transform: mirrored ? "scaleX(-1)" : undefined, ...crop(url, px, tx, ty, legacy) }}
          />
        );
      })}
    </span>
  );
}
