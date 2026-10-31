import TinyPinyin from "tiny-pinyin";

import { MOD_NAMES } from "./modNames";

// Search that works the way Chinese players type: the characters themselves,
// full pinyin ("jixie"), initials ("jxdl") or a mix ("jixiedl"), English
// names of mods ("create" finds 机械动力), and loose subsequences for Latin
// text ("sdm" finds "sodium"). Every match reports which characters of the
// text it used, so the palette can highlight them.

const pinyinReady = (() => {
  try {
    return TinyPinyin.isSupported();
  } catch {
    return false;
  }
})();

const HAN = /[㐀-鿿]/;

interface Unit {
  /** The character, lower-cased. */
  ch: string;
  /** Its pinyin without tones, for Han characters. */
  py: string | null;
}

const unitCache = new Map<string, Unit[]>();

function units(text: string): Unit[] {
  let cached = unitCache.get(text);
  if (cached) return cached;
  cached = [...text.toLowerCase()].map((ch) => ({
    ch,
    py: pinyinReady && HAN.test(ch) ? TinyPinyin.convertToPinyin(ch, "", true) || null : null,
  }));
  if (unitCache.size > 4000) unitCache.clear();
  unitCache.set(text, cached);
  return cached;
}

export interface Match {
  /** Higher is better. */
  score: number;
  /** Indices (by code point) of the characters the match used. */
  marks: number[];
}

/** Pinyin and literal characters, consecutive from `start`: each unit takes
    its full pinyin, a prefix of it (only as the last unit), its initial, or
    the character itself. Returns how many units were used, or 0. */
function pinyinRun(list: Unit[], start: number, query: string): number {
  const step = (i: number, q: number): number => {
    if (q === query.length) return i - start;
    if (i >= list.length) return 0;
    const unit = list[i];
    if (unit.py) {
      if (query.startsWith(unit.py, q)) {
        const done = step(i + 1, q + unit.py.length);
        if (done) return done;
      }
      // The last thing typed may be the start of this syllable ("jixied").
      if (unit.py.startsWith(query.slice(q))) return i + 1 - start;
      if (unit.py[0] === query[q]) {
        const done = step(i + 1, q + 1);
        if (done) return done;
      }
    }
    if (unit.ch === query[q]) return step(i + 1, q + 1);
    return 0;
  };
  return step(start, 0);
}

/** Match one query word against a text. Null when it does not match.
    `loose` allows the subsequence match; only labels use it, since across a
    long keyword list almost any two letters appear in order. */
export function matchWord(text: string, word: string, loose = true): Match | null {
  if (!word) return { score: 0, marks: [] };
  const list = units(text);
  const chars = list.map((u) => u.ch);
  const lower = chars.join("");

  // 1. The characters themselves.
  const at = lower.indexOf(word);
  if (at >= 0) {
    const index = [...lower.slice(0, at)].length;
    const length = [...word].length;
    const boundary = index === 0 || /[\s·「（(\-_.:/]/.test(chars[index - 1]);
    return { score: 100 + (index === 0 ? 40 : boundary ? 20 : 0) - Math.min(index, 20), marks: range(index, length) };
  }

  // 2. Pinyin, full or initials, starting at any character.
  if (pinyinReady && /^[a-z]+$/.test(word) && list.some((u) => u.py)) {
    for (let i = 0; i < list.length; i++) {
      if (!list[i].py && list[i].ch !== word[0]) continue;
      const used = pinyinRun(list, i, word);
      if (used) return { score: 80 + (i === 0 ? 30 : 0) - Math.min(i, 20), marks: range(i, used) };
    }
  }

  // 3. A loose subsequence of Latin text ("sdm" in "sodium").
  if (loose && word.length >= 2 && /^[a-z0-9.]+$/.test(word)) {
    const marks: number[] = [];
    let from = 0;
    for (const ch of word) {
      const found = chars.indexOf(ch, from);
      if (found < 0) return null;
      marks.push(found);
      from = found + 1;
    }
    const spread = marks[marks.length - 1] - marks[0] + 1 - marks.length;
    if (spread > 8) return null;
    return { score: 40 - Math.min(spread, 30) - Math.min(marks[0], 10), marks };
  }
  return null;
}

function range(start: number, length: number): number[] {
  return Array.from({ length }, (_, i) => start + i);
}

/** The words of a query: lower-cased, split on spaces. */
export function queryWords(query: string): string[] {
  return query.trim().toLowerCase().split(/\s+/).filter(Boolean);
}

/** Every word must match the label or one of the other fields. The label's
    matches count fully and are highlighted; other fields count for less. */
export function matchItem(label: string, others: string[], words: string[]): Match | null {
  if (words.length === 0) return { score: 0, marks: [] };
  let score = 0;
  const marks = new Set<number>();
  for (const word of words) {
    const inLabel = matchWord(label, word);
    let best = inLabel ? inLabel.score : -1;
    if (inLabel) inLabel.marks.forEach((m) => marks.add(m));
    // Other fields count by kind of match only: where in a keyword list a
    // word happens to sit says nothing about relevance.
    for (const other of others) {
      if (!other) continue;
      const hit = matchWord(other, word, false);
      if (!hit) continue;
      const flat = hit.score >= 100 ? 60 : 48;
      if (flat > best) best = flat;
    }
    if (best < 0) return null;
    score += best;
  }
  return { score: score / words.length, marks: [...marks].sort((a, b) => a - b) };
}

/** Names a text is also known by: for 机械动力 that is Create and create. A
    world or file named after a mod can then be found by the mod's English
    name, and the other way round. */
export function aliasesOf(text: string): string {
  const lower = text.toLowerCase();
  const found: string[] = [];
  for (const mod of MOD_NAMES) {
    if (lower.includes(mod.zh.toLowerCase()) || (mod.slug.length > 3 && lower.includes(mod.slug)) || (mod.en.length > 3 && lower.includes(mod.en.toLowerCase()))) {
      found.push(mod.slug, mod.en, mod.zh, ...(mod.aliases ?? []));
    }
    if (found.length > 24) break;
  }
  return found.join(" ");
}
