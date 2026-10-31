import { useEffect, useState, type CSSProperties, type KeyboardEvent, type PointerEvent } from "react";

import { t } from "../lib/i18n";
import { secure, useSkin } from "./PlayerFace";

// The whole player in 3D, built from CSS boxes whose faces are cut from the
// skin texture: no WebGL, so it stays pixel-sharp at any size. It turns
// slowly on its own; dragging (or the arrow keys) turns it by hand.

type Vec = [number, number, number];

/** A box in texels: where its centre sits (y down, z toward the viewer),
    its size, and the top-left of its unfolded texture. */
export interface Box {
  at: Vec;
  size: Vec;
  uv: [number, number];
  /** Overlay layers stand off the body by this many texels. */
  inflate?: number;
  /** Old skins reuse the right limb for the left, mirrored. */
  mirror?: boolean;
}

/** The six faces of a box: which rectangle of its unfolded texture, and how
    the face turns out of the box's centre. Standard Minecraft layout. */
export function faces([w, h, d]: Vec, [u, v]: [number, number]) {
  return [
    { uv: [u + d, v + d, w, h], turn: "", out: d / 2 },
    { uv: [u + 2 * d + w, v + d, w, h], turn: "rotateY(180deg)", out: d / 2 },
    { uv: [u, v + d, d, h], turn: "rotateY(-90deg)", out: w / 2 },
    { uv: [u + d + w, v + d, d, h], turn: "rotateY(90deg)", out: w / 2 },
    { uv: [u + d, v, w, d], turn: "rotateX(90deg)", out: h / 2 },
    { uv: [u + d + w, v, w, d], turn: "rotateX(-90deg) scaleY(-1)", out: h / 2 },
  ] as const;
}

export function parts(slim: boolean, legacy: boolean, hat: boolean): Box[] {
  const arm = slim && !legacy ? 3 : 4;
  const armX = 4 + arm / 2;
  const boxes: Box[] = [
    { at: [0, -12, 0], size: [8, 8, 8], uv: [0, 0] },
    { at: [0, -2, 0], size: [8, 12, 4], uv: [16, 16] },
    { at: [-armX, -2, 0], size: [arm, 12, 4], uv: [40, 16] },
    { at: [armX, -2, 0], size: [arm, 12, 4], uv: legacy ? [40, 16] : [32, 48], mirror: legacy },
    { at: [-2, 10, 0], size: [4, 12, 4], uv: [0, 16] },
    { at: [2, 10, 0], size: [4, 12, 4], uv: legacy ? [0, 16] : [16, 48], mirror: legacy },
  ];
  if (hat) boxes.push({ at: [0, -12, 0], size: [8, 8, 8], uv: [32, 0], inflate: 0.5 });
  // Jacket, sleeves and trousers exist only on 64×64 skins.
  if (!legacy)
    boxes.push(
      { at: [0, -2, 0], size: [8, 12, 4], uv: [16, 32], inflate: 0.25 },
      { at: [-armX, -2, 0], size: [arm, 12, 4], uv: [40, 32], inflate: 0.25 },
      { at: [armX, -2, 0], size: [arm, 12, 4], uv: [48, 48], inflate: 0.25 },
      { at: [-2, 10, 0], size: [4, 12, 4], uv: [0, 32], inflate: 0.25 },
      { at: [2, 10, 0], size: [4, 12, 4], uv: [0, 48], inflate: 0.25 },
    );
  return boxes;
}

function BoxView({ box, url, sheet, px }: { box: Box; url: string; sheet: [number, number]; px: number }) {
  const grow = (box.inflate ?? 0) * 2;
  const [w, h, d] = box.size;
  const [sw, sh, sd] = [w + grow, h + grow, d + grow];
  const [x, y, z] = box.at;
  return (
    <div className="skin-box" style={{ transform: `translate3d(${x * px}px, ${y * px}px, ${z * px}px)${box.mirror ? " scaleX(-1)" : ""}` }}>
      {faces(box.size, box.uv).map((face, i) => {
        const [fu, fv, fw, fh] = face.uv;
        // Each face is drawn at texture size, then stretched by the
        // inflation so overlay layers wrap the body.
        const sx = (fw + grow) / fw;
        const sy = (fh + grow) / fh;
        const out = (i < 2 ? sd : i < 4 ? sw : sh) / 2;
        const style: CSSProperties = {
          width: fw * px,
          height: fh * px,
          marginLeft: (-fw * px) / 2,
          marginTop: (-fh * px) / 2,
          backgroundImage: `url("${url}")`,
          backgroundSize: `${sheet[0] * px}px ${sheet[1] * px}px`,
          backgroundPosition: `${-fu * px}px ${-fv * px}px`,
          transform: `${face.turn} translateZ(${out * px}px) scale(${sx}, ${sy})`,
        };
        return <span key={i} className="skin-face" style={style} />;
      })}
    </div>
  );
}

/** A cape texture (64×32), or null while it loads or when it cannot be used. */
function useCape(url: string | null | undefined): string | null {
  const [cape, setCape] = useState<string | null>(null);
  useEffect(() => {
    setCape(null);
    if (!url) return;
    const src = secure(url);
    const image = new Image();
    image.onload = () => image.naturalWidth === image.naturalHeight * 2 && setCape(src);
    image.src = src;
    return () => {
      image.onload = null;
    };
  }, [url]);
  return cape;
}

/** The player's skin as a turnable 3D figure; null until the skin loads. */
export function SkinModel({ skinUrl, slim, capeUrl, px = 5 }: { skinUrl: string | null | undefined; slim: boolean; capeUrl?: string | null; px?: number }) {
  const skin = useSkin(skinUrl);
  const cape = useCape(capeUrl);
  // Null while it turns by itself; a held angle once the player takes over.
  const [angle, setAngle] = useState<{ yaw: number; pitch: number } | null>(null);
  const [drag, setDrag] = useState<{ x: number; y: number; yaw: number; pitch: number } | null>(null);
  if (!skin) return null;

  const held = angle ?? { yaw: -28, pitch: -8 };
  const turn = (yaw: number, pitch: number) => setAngle({ yaw, pitch: Math.max(-35, Math.min(35, pitch)) });
  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    setDrag({ x: e.clientX, y: e.clientY, ...held });
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (drag) turn(drag.yaw + (e.clientX - drag.x) * 0.8, drag.pitch - (e.clientY - drag.y) * 0.5);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = { ArrowLeft: [-15, 0], ArrowRight: [15, 0], ArrowUp: [0, 10], ArrowDown: [0, -10] }[e.key];
    if (!step) return;
    e.preventDefault();
    turn(held.yaw + step[0], held.pitch + step[1]);
  };
  const sheet: [number, number] = [64, skin.legacy ? 32 : 64];
  return (
    <div
      className="skin-model"
      role="img"
      tabIndex={0}
      aria-label={t("3D 皮肤预览，拖动或用方向键转动", "3D skin preview; drag or use the arrow keys to turn it")}
      style={{ width: 24 * px, height: 40 * px }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={() => setDrag(null)}
      onPointerCancel={() => setDrag(null)}
      onKeyDown={onKeyDown}
      onDoubleClick={() => setAngle(null)}
    >
      <div className="skin-tilt" style={{ transform: `rotateX(${held.pitch}deg)` }}>
        <div className={`skin-stage${angle ? "" : " spinning"}`} style={angle ? { transform: `rotateY(${angle.yaw}deg)` } : undefined}>
          {parts(slim, skin.legacy, skin.hat).map((box, i) => (
            <BoxView key={i} box={box} url={skin.url} sheet={sheet} px={px} />
          ))}
          {cape && (
            // Hung from the shoulders, lifted a little off the back.
            <div className="skin-cape" style={{ transform: `translate3d(0, ${-8 * px}px, ${-2.5 * px}px) rotateX(-8deg) translateY(${8 * px}px) rotateY(180deg)` }}>
              <BoxView box={{ at: [0, 0, 0], size: [10, 16, 1], uv: [0, 0] }} url={cape} sheet={[64, 32]} px={px} />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
