#!/usr/bin/env node
/**
 * Zero-dependency WOFF2 pixel-font subset verifier.
 *
 * Mirrors `scripts/subset-font.py --check` using Node's built-in Brotli
 * decompressor so `npm test`, `npm run build`, and every CI/release runner
 * (macOS, Windows, Linux) verify CJK UI glyph coverage offline in ~25 ms.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import zlib from "node:zlib";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const WEB = path.join(ROOT, "web", "src");
const FONT = path.join(WEB, "assets", "fonts", "fusion-pixel-12.woff2");

// W3C WOFF2 §5.1 known table tags (index 0..62).
const KNOWN_TAGS = [
  "cmap", "head", "hhea", "hmtx", "maxp", "name", "OS/2", "post", "cvt ", "fpgm",
  "glyf", "loca", "prep", "CFF ", "VORG", "EBDT", "EBLC", "gasp", "hdmx", "kern",
  "LTSH", "PCLT", "VDMX", "vhea", "vmtx", "BASE", "GDEF", "GPOS", "GSUB", "EBSC",
  "JSTF", "MATH", "CBDT", "CBLC", "COLR", "CPAL", "SVG ", "sbix", "acnt", "avar",
  "bdat", "bloc", "bsln", "cvar", "fdsc", "feat", "fmtx", "fvar", "gvar", "hsty",
  "just", "lcar", "mort", "morx", "opbd", "prop", "trak", "Zapf", "Silf", "Glat",
  "Gloc", "Feat", "Sill",
];

function readUIntBase128(buf, state) {
  let accum = 0;
  for (let i = 0; i < 5; i++) {
    if (state.pos >= buf.length) throw new Error("Unexpected EOF in UIntBase128");
    const b = buf[state.pos++];
    if (i === 0 && b === 0x80) throw new Error("Leading zero in UIntBase128");
    if (accum & 0xfe000000) throw new Error("Overflow in UIntBase128");
    accum = (accum << 7) | (b & 0x7f);
    if ((b & 0x80) === 0) return accum >>> 0;
  }
  throw new Error("UIntBase128 exceeds 5 bytes");
}

/** Extracts all mapped Unicode code points from a WOFF2 font's `cmap` table. */
export function readWoff2CodePoints(buf) {
  if (buf.length < 48 || buf.toString("ascii", 0, 4) !== "wOF2") {
    throw new Error("Not a WOFF2 file");
  }
  const flavor = buf.readUInt32BE(4);
  if (flavor === 0x74746366) {
    throw new Error("WOFF2 font collections (ttcf) are not supported");
  }
  const numTables = buf.readUInt16BE(12);
  const totalCompressedSize = buf.readUInt32BE(20);

  const state = { pos: 48 };
  let streamOffset = 0;
  let cmapRange = null;

  for (let i = 0; i < numTables; i++) {
    const flags = buf[state.pos++];
    const tagIndex = flags & 0x3f;
    const transformVersion = (flags >> 6) & 0x03;
    const tag = tagIndex === 63
      ? buf.toString("ascii", state.pos, (state.pos += 4))
      : KNOWN_TAGS[tagIndex];
    const origLength = readUIntBase128(buf, state);
    const isGlyfOrLoca = tag === "glyf" || tag === "loca";
    const hasTransformLength = isGlyfOrLoca
      ? transformVersion === 0
      : transformVersion !== 0;
    const storedLength = hasTransformLength ? readUIntBase128(buf, state) : origLength;

    if (tag === "cmap") {
      if (transformVersion !== 0) throw new Error("Unexpected transformed cmap table");
      cmapRange = { offset: streamOffset, length: storedLength };
    }
    streamOffset += storedLength;
  }

  if (!cmapRange) throw new Error("WOFF2 font has no cmap table");
  const compressed = buf.subarray(state.pos, state.pos + totalCompressedSize);
  const raw = zlib.brotliDecompressSync(compressed);
  const cmap = raw.subarray(cmapRange.offset, cmapRange.offset + cmapRange.length);

  const codePoints = new Set();
  const numSubtables = cmap.readUInt16BE(2);
  for (let i = 0; i < numSubtables; i++) {
    const subOffset = cmap.readUInt32BE(4 + i * 8 + 4);
    const format = cmap.readUInt16BE(subOffset);

    if (format === 4) {
      const segCount = cmap.readUInt16BE(subOffset + 6) >>> 1;
      const endCodeOff = subOffset + 14;
      const startCodeOff = endCodeOff + segCount * 2 + 2;
      const idDeltaOff = startCodeOff + segCount * 2;
      const idRangeOff = idDeltaOff + segCount * 2;
      for (let s = 0; s < segCount; s++) {
        const start = cmap.readUInt16BE(startCodeOff + s * 2);
        const end = cmap.readUInt16BE(endCodeOff + s * 2);
        if (start === 0xffff && end === 0xffff) continue;
        const delta = cmap.readInt16BE(idDeltaOff + s * 2);
        const rangeOffset = cmap.readUInt16BE(idRangeOff + s * 2);
        for (let cp = start; cp <= end; cp++) {
          let glyphId;
          if (rangeOffset === 0) {
            glyphId = (cp + delta) & 0xffff;
          } else {
            const addr = idRangeOff + s * 2 + rangeOffset + (cp - start) * 2;
            const rawGlyph = cmap.readUInt16BE(addr);
            glyphId = rawGlyph === 0 ? 0 : (rawGlyph + delta) & 0xffff;
          }
          if (glyphId !== 0) codePoints.add(cp);
        }
      }
    } else if (format === 12) {
      const numGroups = cmap.readUInt32BE(subOffset + 12);
      for (let g = 0; g < numGroups; g++) {
        const base = subOffset + 16 + g * 12;
        const start = cmap.readUInt32BE(base);
        const end = cmap.readUInt32BE(base + 4);
        const startGlyph = cmap.readUInt32BE(base + 8);
        for (let cp = start; cp <= end; cp++) {
          if (startGlyph + (cp - start) !== 0) codePoints.add(cp);
        }
      }
    }
  }
  return codePoints;
}

function walkTsFiles(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walkTsFiles(full, out);
    } else if (/\.tsx?$/.test(entry.name)) {
      out.push(full);
    }
  }
  return out;
}

/** Collects CJK and fullwidth punctuation characters from production UI files. */
export function collectProdUiChars(webDir = WEB) {
  const prodChars = new Set();
  for (const full of walkTsFiles(webDir).sort()) {
    const rel = path.relative(webDir, full).replace(/\\/g, "/");
    if (rel === "api/mock.ts" || rel.startsWith("dev/") || rel.startsWith("test/") || rel.includes(".test.")) continue;
    const text = fs.readFileSync(full, "utf8");
    for (const ch of text) {
      const cp = ch.codePointAt(0);
      if ((cp >= 0x3000 && cp <= 0x303f) || (cp >= 0x4e00 && cp <= 0x9fa5) || (cp >= 0xff00 && cp <= 0xffef)) {
        prodChars.add(ch);
      }
    }
  }
  return prodChars;
}

export function checkFontSubset(fontPath = FONT, webDir = WEB) {
  const cmap = readWoff2CodePoints(fs.readFileSync(fontPath));
  const prodChars = collectProdUiChars(webDir);
  const missing = [...prodChars].filter((ch) => !cmap.has(ch.codePointAt(0))).sort();
  return { total: prodChars.size, glyphCount: cmap.size, missing };
}

function sameFile(a, b) {
  try {
    return fs.realpathSync(a) === fs.realpathSync(b);
  } catch {
    return path.resolve(a) === path.resolve(b);
  }
}

if (process.argv[1] && sameFile(process.argv[1], fileURLToPath(import.meta.url))) {
  const { total, missing } = checkFontSubset();
  const relFont = path.relative(ROOT, FONT).replace(/\\/g, "/");
  if (missing.length > 0) {
    const codes = missing.map((ch) => `${ch}(U+${ch.codePointAt(0).toString(16).toUpperCase().padStart(4, "0")})`).join(", ");
    console.error(`error: ${missing.length} UI character(s) missing from ${relFont}: ${missing.join("")}`);
    console.error(`  code points: ${codes}`);
    console.error("  rebuild subset: npm run font:subset  (or: uv run --with fonttools --with brotli python3 scripts/subset-font.py --fetch)");
    process.exit(1);
  }
  console.log(`ok: all ${total} CJK UI characters present in ${relFont}`);
}
