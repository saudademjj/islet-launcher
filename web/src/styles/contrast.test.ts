import { describe, expect, it } from "vitest";

import css from "./tokens.css?raw";

// Reads the colour tokens straight from tokens.css, so a new shade that
// makes text hard to read fails here instead of on someone's screen.

function block(selector: string): Record<string, string> {
  const start = css.indexOf(selector);
  if (start < 0) throw new Error(`no ${selector} in tokens.css`);
  const body = css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
  const tokens: Record<string, string> = {};
  for (const [, name, value] of body.matchAll(/--([\w-]+):\s*(#[0-9a-f]{6})\s*;/gi)) tokens[name] = value;
  return tokens;
}

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

// Text and the grounds it sits on. 4.5:1 is the bar for the 12 px text the
// launcher uses for hints, tags and status.
const TEXT: [string[], string[]][] = [
  [["text", "muted", "faint"], ["bg", "surface", "surface-2", "surface-3"]],
  [["accent-text"], ["bg", "surface", "accent-soft"]],
  [["danger-text"], ["surface", "danger-soft"]],
  [["warn"], ["surface", "warn-soft"]],
  [["accent-ink"], ["accent", "accent-hover"]],
  [["on-dark", "on-dark-muted", "on-dark-faint", "on-dark-accent", "on-dark-warn", "on-dark-danger"], ["frame", "hero"]],
  // Raised hero buttons are a step lighter; the faintest text stays off them.
  [["on-dark", "on-dark-muted", "on-dark-accent", "on-dark-warn", "on-dark-danger"], ["hero-2"]],
];

const THEMES = {
  light: block(':root[data-theme="light"]'),
  dark: { ...block(':root[data-theme="light"]'), ...block(':root[data-theme="dark"]') },
};

describe("colour contrast", () => {
  for (const [theme, tokens] of Object.entries(THEMES)) {
    it(`keeps text readable in the ${theme} theme`, () => {
      const low: string[] = [];
      for (const [fronts, grounds] of TEXT) {
        for (const front of fronts) {
          for (const ground of grounds) {
            expect(tokens[front], `--${front}`).toBeDefined();
            expect(tokens[ground], `--${ground}`).toBeDefined();
            const ratio = contrast(tokens[front], tokens[ground]);
            if (ratio < 4.5) low.push(`--${front} on --${ground}: ${ratio.toFixed(2)}`);
          }
        }
      }
      expect(low).toEqual([]);
    });
  }

  // Marks painted on the notice signpost are graphics: 3:1 against the
  // board. The sign's colours live in the theme-free :root block.
  const SIGNS = {
    light: block(":root {"),
    dark: { ...block(":root {"), ...block(':root[data-theme="dark"]') },
  };
  for (const [theme, tokens] of Object.entries(SIGNS)) {
    it(`keeps sign marks visible in the ${theme} theme`, () => {
      const low = ["sign-ink", "sign-danger", "sign-warn", "sign-ok"]
        .map((mark) => [mark, contrast(tokens[mark], tokens["sign-face"])] as const)
        .filter(([, ratio]) => ratio < 3)
        .map(([mark, ratio]) => `--${mark} on --sign-face: ${ratio.toFixed(2)}`);
      expect(tokens["sign-face"]).toBeDefined();
      expect(low).toEqual([]);
    });
  }

  it("measures the way WCAG does", () => {
    expect(contrast("#000000", "#ffffff")).toBeCloseTo(21, 5);
    expect(contrast("#777777", "#ffffff")).toBeCloseTo(4.48, 2);
  });
});
