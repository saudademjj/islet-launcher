import type { CSSProperties } from "react";
import { create } from "zustand";

import type { SkyPhase } from "./format";
import { hashSeed, type Flora } from "./pixel";
import { t } from "./i18n";

// Biomes give each world its own sky, ground and plants. Skies stay dark so
// the cream text on them reads the same everywhere; the grass-green start
// button is the one constant across worlds.

export type BiomeId = "plains" | "cherry" | "taiga" | "snowy" | "desert" | "badlands" | "mushroom";

export interface Biome {
  id: BiomeId;
  name: string;
  sky: string;
  /** Surface: base, shade, highlight. */
  top: [string, string, string];
  /** Soil below the surface: base, dark, light. */
  soil: [string, string, string];
  wood: string;
  /** Leaves (or cactus, or mushroom cap) and their specks. */
  leaf: [string, string];
  flower: string;
  flora: Flora;
  /** Soil in horizontal bands, like badlands terracotta. */
  strata?: boolean;
}

const DIRT: [string, string, string] = ["#8b5a33", "#74482a", "#a06b40"];

export const BIOMES: Record<BiomeId, Biome> = {
  plains: { id: "plains", name: t("平原", "Plains"), sky: "#1e4027", top: ["#7fb238", "#64952a", "#9ccb52"], soil: DIRT, wood: "#6b4423", leaf: ["#3f7d31", "#326827"], flower: "#f2cf4a", flora: "oak" },
  cherry: { id: "cherry", name: t("樱花林", "Cherry Grove"), sky: "#33213a", top: ["#86b84c", "#6b9a3a", "#a2cf62"], soil: DIRT, wood: "#5b3a2f", leaf: ["#eba4c3", "#d684a9"], flower: "#f6c9dc", flora: "cherry" },
  taiga: { id: "taiga", name: t("针叶林", "Taiga"), sky: "#162f2b", top: ["#6c9a4a", "#577f3b", "#83b05c"], soil: ["#7a5232", "#634127", "#8f6440"], wood: "#4e3423", leaf: ["#2e5a3e", "#244a32"], flower: "#c0473a", flora: "spruce" },
  snowy: { id: "snowy", name: t("雪原", "Snowy Plains"), sky: "#1b2f44", top: ["#e8eff2", "#c9d6dd", "#ffffff"], soil: ["#7c5a3e", "#664830", "#8f6a4a"], wood: "#4e3626", leaf: ["#2f5848", "#24493b"], flower: "#e8eff2", flora: "spruce" },
  desert: { id: "desert", name: t("沙漠", "Desert"), sky: "#3a2a1e", top: ["#e3c47f", "#cdaa63", "#f1d89c"], soil: ["#c9a462", "#b28c4e", "#d9b979"], wood: "#8a6a3a", leaf: ["#5d9040", "#477a30"], flower: "#e3c47f", flora: "cactus" },
  badlands: { id: "badlands", name: t("恶地", "Badlands"), sky: "#3a1e1f", top: ["#c8703f", "#b05e33", "#da8a55"], soil: ["#b5653e", "#8a4a33", "#d9a47d"], wood: "#7a5a32", leaf: ["#5d9040", "#477a30"], flower: "#c8703f", flora: "dead", strata: true },
  mushroom: { id: "mushroom", name: t("蘑菇岛", "Mushroom Island"), sky: "#2a2240", top: ["#8f7c99", "#786784", "#a794b0"], soil: ["#7c5a3e", "#664830", "#8f6a4a"], wood: "#e6dccb", leaf: ["#b93c35", "#f1e7d6"], flower: "#b93c35", flora: "mushroom" },
};

export const BIOME_IDS = Object.keys(BIOMES) as BiomeId[];

/** The biome generated for a world, stable for its id. */
export function biomeFor(seed: string): Biome {
  return BIOMES[BIOME_IDS[hashSeed(`${seed}:biome`) % BIOME_IDS.length]];
}

function mix(a: string, b: string, t: number): string {
  const pa = parseInt(a.slice(1), 16);
  const pb = parseInt(b.slice(1), 16);
  const ch = (shift: number) => Math.round(((pa >> shift) & 255) * (1 - t) + ((pb >> shift) & 255) * t);
  return `#${((ch(16) << 16) | (ch(8) << 8) | ch(0)).toString(16).padStart(6, "0")}`;
}

const CREAM = "#eef4e4";

/** The biome's sky at a time of day: darker at night, warmed by the low sun
    at dawn and dusk. Every result stays dark enough for the cream text. */
function skyAt(sky: string, phase: SkyPhase): string {
  switch (phase) {
    case "night":
      return mix(sky, "#000000", 0.24);
    case "dawn":
      return mix(sky, "#b9654a", 0.34);
    case "dusk":
      return mix(mix(sky, "#a8483a", 0.42), "#000000", 0.12);
    default:
      return sky;
  }
}

/** CSS variables for a scene in this biome: the sky and everything tinted by
    it (clouds, hills, buttons, secondary text), plus the terrain paints.
    `night` takes a time of day, or true for night. */
export function biomeStyle(b: Biome, night: boolean | SkyPhase = false): CSSProperties {
  const phase: SkyPhase = typeof night === "boolean" ? (night ? "night" : "day") : night;
  const sky = skyAt(b.sky, phase);
  return {
    "--hero": sky,
    "--hero-2": mix(sky, "#ffffff", 0.09),
    "--hero-3": mix(sky, "#ffffff", 0.15),
    "--hero-edge": mix(sky, "#000000", 0.4),
    "--hero-cloud": mix(sky, "#ffffff", 0.055),
    "--hill-far": mix(sky, "#000000", 0.16),
    "--hill-near": mix(sky, "#000000", 0.3),
    "--star": mix(sky, "#ffffff", 0.24),
    "--on-dark-muted": mix(sky, CREAM, 0.7),
    "--on-dark-faint": mix(sky, CREAM, 0.48),
    "--t-grass": b.top[0],
    "--t-grass-dark": b.top[1],
    "--t-grass-light": b.top[2],
    "--t-dirt": b.soil[0],
    "--t-dirt-dark": b.soil[1],
    // Soil under a header's tabs; darkened so cream text reads on sand too.
    "--t-strip": mix(b.soil[1], "#000000", 0.32),
    "--t-dirt-light": b.soil[2],
    "--t-wood": b.wood,
    "--t-leaf": b.leaf[0],
    "--t-leaf-dark": b.leaf[1],
    "--t-flower": b.flower,
    // Stone takes a little of the sky, so each biome's underground belongs to it.
    // Darker than the soil so the grass stays the brightest band.
    "--t-stone": mix("#5a5d55", sky, 0.32),
    "--t-stone-dark": mix("#484b44", sky, 0.32),
    "--t-cave": mix("#1c1d1a", sky, 0.3),
    "--t-coal": mix("#262622", sky, 0.15),
  } as CSSProperties;
}

// A biome picked for a world overrides the generated one. It only changes
// the launcher's scenery, so it lives in this WebView's storage.
const KEY = "mc-launcher.biomes";

function load(): Record<string, BiomeId> {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "{}");
  } catch {
    return {};
  }
}

export const useBiomeChoices = create<{ choices: Record<string, BiomeId>; choose(seed: string, id: BiomeId | null): void }>((set) => ({
  choices: load(),
  choose(seed, id) {
    set((s) => {
      const choices = { ...s.choices };
      if (id) choices[seed] = id;
      else delete choices[seed];
      try {
        localStorage.setItem(KEY, JSON.stringify(choices));
      } catch {
        // Storage can be unavailable; the choice still applies until restart.
      }
      return { choices };
    });
  },
}));

export function useBiome(seed: string): Biome {
  const chosen = useBiomeChoices((s) => s.choices[seed]);
  return (chosen && BIOMES[chosen]) || biomeFor(seed);
}

// A screenshot can stand in for the generated scenery: the player's own view
// of that world. Like the biome choice it only changes the launcher's look.
const COVER_KEY = "mc-launcher.covers";

export interface Cover {
  name: string;
  modifiedMs: number;
  /** Demo backend only. */
  demoUrl?: string;
}

function loadCovers(): Record<string, Cover> {
  try {
    return JSON.parse(localStorage.getItem(COVER_KEY) ?? "{}");
  } catch {
    return {};
  }
}

export const useCovers = create<{ covers: Record<string, Cover>; setCover(seed: string, cover: Cover | null): void }>((set) => ({
  covers: loadCovers(),
  setCover(seed, cover) {
    set((s) => {
      const covers = { ...s.covers };
      if (cover) covers[seed] = cover;
      else delete covers[seed];
      try {
        localStorage.setItem(COVER_KEY, JSON.stringify(covers));
      } catch {
        // Storage can be unavailable; the cover still applies until restart.
      }
      return { covers };
    });
  },
}));

export function useCover(seed: string): Cover | null {
  return useCovers((s) => s.covers[seed] ?? null);
}
