// Pixel art and pixel type stay crisp only on whole device pixels. At 100%
// and 200% the design sizes (3px pixels, 12px type) already are; at
// fractional scaling (Windows 125%, 150%, 175%) they are snapped here.

const dpr = typeof window === "undefined" ? 1 : window.devicePixelRatio || 1;

/** One scenery pixel in CSS pixels: about 3, landing on whole device pixels. */
export const SCENE_PX = Math.max(1, Math.round(3 * dpr)) / dpr;

/** Two thirds of that, for pixel art beside a single line of text. */
export const SMALL_PX = Math.max(1, Math.round(2 * dpr)) / dpr;

/** Sets the pixel font sizes. The font is drawn on a 12px grid, so a size
    is only crisp at a whole multiple of 12 device pixels; when the nearest
    such size would be unreadably small or oversized, 12px stays and the
    glyphs are simply smoothed. */
export function applyPixelType() {
  const scale = Math.round(dpr);
  const size = (scale * 12) / dpr;
  if (Math.abs(dpr - scale) < 0.01 || size < 11 || size > 17) return;
  const root = document.documentElement.style;
  root.setProperty("--px-font", `${size}px`);
  root.setProperty("--px-font-2", `${size * 2}px`);
}
