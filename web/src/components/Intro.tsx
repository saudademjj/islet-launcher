import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";

import { paintGrid } from "../lib/pixel";
import { BRAND, Celestial, Clouds, PixelSvg, PX } from "./Pixel";

// The intro, in milliseconds from the first frame the window shows: nine
// blocks fall into an island, a sun steps up behind it and a tree grows,
// the name fills in, then the whole screen breaks into squares that shrink
// away over the launcher. Solid paints only; the motion steps the way the
// game's own sprites do.

const U = PX * 2; // one pixel of the intro's art
const B = U * 8; // one block
const CELL = 60; // one square of the closing mosaic

const DROP_AT = 120;
const DROP_GAP = 60;
const DROP = 300;
const LEAVE_AT = 2100;
const LEAVE = 760;

const DIRT = paintGrid(["ddDddedd", "dedddDdd", "dddDdddd", "dDdddeDd", "ddedDddd", "dDdddddD", "dddeddDd", "ddDddddd"], { d: "d", D: "d2", e: "d3" });
const TREE = paintGrid([".llllll.", "llLlllll", "lllllLll", "lLllllll", "llllLlll", ".llllll.", "...ww...", "...ww...", "...ww..."], { l: "l", L: "l2", w: "w" });
const FLOWER = paintGrid([".f.", "fdf", ".f.", ".l."], { f: "f", d: "d3", l: "l" });

// Bottom up, so each block lands on the one below it.
const ISLAND = [
  [2, 2],
  [1, 1],
  [2, 1],
  [3, 1],
  [0, 0],
  [1, 0],
  [2, 0],
  [3, 0],
  [4, 0],
].map(([col, row], i) => ({ col, row, at: DROP_AT + i * DROP_GAP }));
const LANDED = DROP_AT + (ISLAND.length - 1) * DROP_GAP + DROP;

const RAYS = Array.from({ length: 8 }, (_, i) => {
  const angle = (i / 8) * Math.PI * 2;
  return { x: Math.round(Math.cos(angle) * 1.25 * B), y: Math.round(Math.sin(angle) * 1.25 * B) };
});

const at = (ms: number) => ({ "--d": `${ms}ms` }) as CSSProperties;

/** Squares covering the window, each with when it shrinks away: nearest
    the middle first, with a little scatter so the edge breaks unevenly. */
function mosaic() {
  const cols = Math.ceil(window.innerWidth / CELL);
  const rows = Math.ceil(window.innerHeight / CELL);
  const cx = (cols - 1) / 2;
  const cy = (rows - 1) / 2;
  const far = Math.hypot(cx, cy) || 1;
  const delays = Array.from({ length: cols * rows }, (_, i) => {
    const x = i % cols;
    const y = Math.floor(i / cols);
    const scatter = ((x * 73 + y * 151) % 7) * 10;
    return Math.round(60 + (Math.hypot(x - cx, y - cy) / far) * 360 + scatter);
  });
  // Clouds wide enough to drift a little without showing an edge.
  return { cols, delays, sky: Math.ceil(window.innerWidth / (PX * 4)) + 24 };
}

/** Plays once over the launcher at start; any click or key skips ahead. */
export function Intro({ onDone }: { onDone(): void }) {
  const [phase, setPhase] = useState<"wait" | "play" | "leave">("wait");
  const done = useRef(onDone);
  done.current = onDone;
  const grid = useMemo(mosaic, []);

  // WebKit holds animation frames while the window is still hidden, so the
  // first one marks when the intro can be seen. A timer backs it up, so a
  // window that never paints can't keep the launcher covered.
  useEffect(() => {
    const start = () => setPhase((p) => (p === "wait" ? "play" : p));
    const frame = requestAnimationFrame(start);
    const timer = window.setTimeout(start, 1500);
    return () => {
      cancelAnimationFrame(frame);
      window.clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    if (phase === "wait") return;
    const timer = window.setTimeout(() => (phase === "play" ? setPhase("leave") : done.current()), phase === "play" ? LEAVE_AT : LEAVE);
    return () => window.clearTimeout(timer);
  }, [phase]);

  // Until it starts to leave, input skips the intro and goes no further.
  useEffect(() => {
    if (phase === "leave") return;
    const skip = (e: Event) => {
      e.preventDefault();
      e.stopPropagation();
      setPhase("leave");
    };
    window.addEventListener("keydown", skip, true);
    window.addEventListener("pointerdown", skip, true);
    return () => {
      window.removeEventListener("keydown", skip, true);
      window.removeEventListener("pointerdown", skip, true);
    };
  }, [phase]);

  return (
    <div className={`intro ${phase}`} aria-hidden>
      <div className="intro-cells" style={{ gridTemplateColumns: `repeat(${grid.cols}, ${CELL}px)`, gridAutoRows: CELL }}>
        {grid.delays.map((delay, i) => (
          <i key={i} style={at(delay)} />
        ))}
      </div>
      <Clouds seed="islet-intro" cols={grid.sky} rows={14} unit={PX} className="intro-sky" />
      <div className="intro-stage" style={{ "--u": `${U}px`, "--b": `${B}px` } as CSSProperties}>
        <div className="intro-scene" style={{ width: 5 * B, height: 3 * B, ...at(LANDED) }}>
          <div className="intro-sun" style={{ left: 4 * B, top: -2.5 * B, ...at(760) }}>
            {RAYS.map((ray, i) => (
              <i key={i} className="intro-ray" style={{ "--x": `${ray.x}px`, "--y": `${ray.y}px`, ...at(1350) } as CSSProperties} />
            ))}
            <Celestial night={false} unit={PX} />
          </div>
          <div className="intro-tree" style={{ left: 3 * B, top: -TREE.height * U, ...at(LANDED) }}>
            <PixelSvg art={TREE} unit={U} />
          </div>
          <div className="intro-flower" style={{ left: B + 2 * U, top: -FLOWER.height * U, ...at(LANDED + 160) }}>
            <PixelSvg art={FLOWER} unit={U} />
          </div>
          <div className="intro-flower" style={{ left: 4 * B + 3 * U, top: -FLOWER.height * U, ...at(LANDED + 240) }}>
            <PixelSvg art={FLOWER} unit={U} />
          </div>
          {ISLAND.map((block) => (
            <div key={`${block.col}-${block.row}`} className={`intro-block${block.row === 0 ? " grass" : ""}`} style={{ left: block.col * B, top: block.row * B, ...at(block.at) }}>
              <PixelSvg art={block.row === 0 ? BRAND : DIRT} unit={U} />
              <i className="intro-dust left" />
              <i className="intro-dust high" />
              <i className="intro-dust right" />
            </div>
          ))}
        </div>
        <div className="intro-word">
          <span className="intro-han">
            <span style={at(1050)}>草</span>
            <span style={at(1130)}>岛</span>
          </span>
          <i className="intro-bar" style={at(1250)} />
          <span className="intro-latin">
            {"ISLET".split("").map((letter, i) => (
              <span key={i} style={at(1300 + i * 50)}>
                {letter}
              </span>
            ))}
          </span>
        </div>
      </div>
    </div>
  );
}
