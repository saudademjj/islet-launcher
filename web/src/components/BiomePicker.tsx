import { useEffect } from "react";

import { BIOME_IDS, BIOMES, biomeFor, biomeStyle, useBiomeChoices, useCover, useCovers, type BiomeId } from "../lib/biome";
import { shotTime, shotUrl, useShots } from "../lib/screenshots";
import { dayLabel } from "../overlays/Screenshots";
import { Landscape, PX } from "./Pixel";
import { t } from "../lib/i18n";

/** Pick the scenery for a world: "auto" keeps the one generated from its
    id, a biome replaces it, and one of the world's own screenshots can stand
    in for all of it. */
export function BiomePicker({ seed }: { seed: string }) {
  const chosen = useBiomeChoices((s) => s.choices[seed] ?? null);
  const choose = useBiomeChoices((s) => s.choose);
  const cover = useCover(seed);
  const setCover = useCovers((s) => s.setCover);
  const shots = useShots((s) => s.lists[seed]);
  useEffect(() => {
    if (useShots.getState().lists[seed] === undefined) void useShots.getState().load(seed);
  }, [seed]);
  const auto = biomeFor(seed);
  const options: (BiomeId | null)[] = [null, ...BIOME_IDS];
  return (
    <div className="col gap-12">
      <div className="biomes">
        {options.map((id) => {
          const biome = id ? BIOMES[id] : auto;
          return (
            <button
              key={id ?? "auto"}
              type="button"
              className="biome"
              aria-pressed={!cover && chosen === id}
              onClick={() => {
                setCover(seed, null);
                choose(seed, id);
              }}
            >
              <span className="biome-scene" style={biomeStyle(biome)}>
                {/* Same 3px pixel as the header above it: a crop, not a miniature. */}
                <Landscape seed={seed} unit={PX} cols={16} ground={[1, 1]} sky={5} treesFrom={2} treeChance={0.6} far={[2, 4]} biome={biome} />
                {!id && <span className="biome-auto">{t("种子", "Seed")}</span>}
              </span>
              <span className="biome-name">{id ? biome.name : t(`自动 · ${auto.name}`, `Auto · ${auto.name}`)}</span>
            </button>
          );
        })}
      </div>
      {shots && shots.length > 0 && (
        <>
          <span className="field-hint">{t("或者用你在这个世界里的截图做封面", "Or use one of your screenshots from this world as the cover")}</span>
          <div className="biomes">
            {shots.slice(0, 8).map((shot) => (
              <button
                key={shot.name}
                type="button"
                className="biome"
                aria-pressed={cover?.name === shot.name}
                onClick={() => setCover(seed, { name: shot.name, modifiedMs: shot.modifiedMs, demoUrl: shot.demoUrl })}
              >
                <span className="biome-scene">
                  <img className="scene-cover" src={shotUrl(seed, shot)} alt="" loading="lazy" draggable={false} />
                </span>
                <span className="biome-name">{dayLabel(shotTime(shot))}</span>
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
