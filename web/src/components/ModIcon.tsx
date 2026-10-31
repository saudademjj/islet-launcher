import { useState } from "react";

import { Identicon } from "./ui";
import { Icon, type IconName } from "./Icon";

/** A project's own icon (from Modrinth, or from inside the file). Without
    one, `glyph` shows what kind of thing it is, rather than a pattern that
    could pass for a real icon; without a glyph, the pixel identicon. Small
    icons are pixel art and are scaled without blurring. */
export function ModIcon({ src, seed, size = 32, glyph }: { src?: string | null; seed: string; size?: number; glyph?: IconName }) {
  const [failed, setFailed] = useState<string | null>(null);
  const [pixelArt, setPixelArt] = useState(false);
  if (!src || failed === src) {
    if (!glyph) return <Identicon seed={seed} size={size} />;
    return (
      <span className={`content-glyph ${glyph}`} style={{ width: size, height: size }} aria-hidden>
        <Icon name={glyph} size={size >= 56 ? 32 : 16} />
      </span>
    );
  }
  return (
    <img
      className={`mod-icon${pixelArt ? " pixelated" : ""}`}
      src={src}
      alt=""
      width={size}
      height={size}
      loading="lazy"
      decoding="async"
      referrerPolicy="no-referrer"
      draggable={false}
      onLoad={(e) => setPixelArt(e.currentTarget.naturalWidth > 0 && e.currentTarget.naturalWidth <= 32)}
      onError={() => setFailed(src)}
    />
  );
}
