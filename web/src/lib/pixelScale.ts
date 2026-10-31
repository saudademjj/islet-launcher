// Pixel art and pixel type stay crisp only on whole device pixels. At 100%
// and 200% the design sizes (3px pixels, 12px type) already are; at
// fractional scaling (Windows 125%, 150%, 175%) they are snapped here.

const dpr = typeof window === "undefined" ? 1 : window.devicePixelRatio || 1;

/** One scenery pixel in CSS pixels: about 3, landing on whole device pixels. */
export const SCENE_PX = Math.max(1, Math.round(3 * dpr)) / dpr;

/** Two thirds of that, for pixel art beside a single line of text. */
export const SMALL_PX = Math.max(1, Math.round(2 * dpr)) / dpr;

/** One pixel of the pixel font (and of a 16px icon) in CSS pixels. The font
    is drawn on a 12px grid, so it is only crisp at a whole multiple of 12
    device pixels; when the nearest such size would be unreadably small or
    oversized, one CSS pixel stays and the glyphs are simply smoothed. */
export const TYPE_PX = (() => {
  const scale = Math.max(1, Math.round(dpr));
  const px = scale / dpr;
  return Math.abs(dpr - scale) < 0.01 || px * 12 < 11 || px * 12 > 17 ? 1 : px;
})();

/** Sets the pixel font sizes to whole font pixels. */
export function applyPixelType() {
  if (TYPE_PX === 1) return;
  const root = document.documentElement.style;
  root.setProperty("--px-font", `${12 * TYPE_PX}px`);
  root.setProperty("--px-font-2", `${24 * TYPE_PX}px`);
}
