export function readWoff2CodePoints(buf: Uint8Array): Set<number>;
export function collectProdUiChars(webDir?: string): Set<string>;
export function checkFontSubset(
  fontPath?: string,
  webDir?: string,
): { total: number; glyphCount: number; missing: string[] };
