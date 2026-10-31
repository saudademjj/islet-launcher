// 5×5 mirrored pixel pattern derived from a seed, matching the GPUI launcher:
// instances and accounts are told apart by shape, not by hue.

function seedHash(seed: string): bigint {
  let hash = 0xcbf29ce484222325n;
  for (const byte of new TextEncoder().encode(seed)) {
    hash = ((hash ^ BigInt(byte)) * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return hash;
}

const cache = new Map<string, boolean[][]>();

export function identiconCells(seed: string): boolean[][] {
  const hit = cache.get(seed);
  if (hit) return hit;
  const hash = seedHash(seed);
  const bits = Array.from({ length: 5 }, () => Array<boolean>(5).fill(false));
  let filled = 0;
  for (let y = 0; y < 5; y++) {
    for (let x = 0; x < 3; x++) {
      const on = ((hash >> BigInt(y * 3 + x)) & 1n) === 1n;
      bits[y][x] = on;
      bits[y][4 - x] = on;
      if (on) filled += 1;
    }
  }
  // Nearly empty or nearly full patterns are hard to tell apart; invert.
  if (filled < 4 || filled > 11) {
    for (const row of bits) row.forEach((cell, i) => (row[i] = !cell));
  }
  cache.set(seed, bits);
  return bits;
}
