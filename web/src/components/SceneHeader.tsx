import type { ReactNode } from "react";

import { biomeStyle, useBiome, type Biome } from "../lib/biome";
import { Button } from "./ui";
import { Clouds, Landscape, PX, Stars } from "./Pixel";
import { SceneCover } from "./SceneCover";
import { t } from "../lib/i18n";

/** A dialog header that is a window onto the world it is about: the world's
    own sky and land behind the title, with an optional strip (tabs, a status
    line) along the ground. */
export function SceneHeader({
  seed,
  title,
  eyebrow,
  meta,
  aside,
  night,
  tone,
  onClose,
  strip,
  biome: forced,
  compact,
  className,
}: {
  seed: string;
  title: ReactNode;
  eyebrow?: ReactNode;
  meta?: ReactNode;
  /** Shown before the title, such as the world's avatar. */
  aside?: ReactNode;
  night?: boolean;
  /** "danger" darkens the sky and edges the strip in red. */
  tone?: "danger";
  onClose: () => void;
  strip?: ReactNode;
  /** Shows this biome instead of the world's own, e.g. while choosing one. */
  biome?: Biome;
  /** A low strip of land with just the title, leaving room for the body. */
  compact?: boolean;
  className?: string;
}) {
  const own = useBiome(seed);
  const biome = forced ?? own;
  const dark = night || tone === "danger";
  return (
    <div className={["scene-head", tone, compact && "compact", className].filter(Boolean).join(" ")} style={biomeStyle(biome, dark)}>
      <div className="scene-head-view">
        {dark ? <Stars seed={seed} cols={110} rows={10} unit={3} className="hero-stars" /> : <Clouds seed={seed} cols={110} rows={6} unit={3} className="hero-sky" />}
        <Landscape seed={seed} unit={PX} cols={110} ground={[2, 4]} sky={6} treesFrom={58} treeChance={0.35} far={[6, 10]} near={[4, 7]} biome={biome} />
        <SceneCover id={seed} className="head-cover" />
        {aside && <div className="scene-head-aside">{aside}</div>}
        <div className="scene-head-text">
          {eyebrow && <div className="scene-head-eyebrow">{eyebrow}</div>}
          <div className="scene-head-title truncate">{title}</div>
          {meta && <div className="scene-head-meta">{meta}</div>}
        </div>
        <Button variant="hero" size="sm" icon="close" className="scene-head-close" onClick={onClose} aria-label={t("关闭", "Close")} />
      </div>
      {strip && <div className="scene-head-strip">{strip}</div>}
    </div>
  );
}
