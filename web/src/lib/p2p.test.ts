import { describe, expect, it } from "vitest";

import type { P2pLevel, P2pStatus } from "../api/types";
import { codeComplete, formatCode, LEVELS, roomLive, validNode } from "./p2p";

describe("p2p room codes", () => {
  it("groups the code in sevens as it is typed", () => {
    expect(formatCode("")).toBe("");
    expect(formatCode("abc")).toBe("ABC");
    expect(formatCode("ABCDEFGH")).toBe("ABCDEFG-H");
    expect(formatCode("0123456789ABCDEFGHJKM")).toBe("0123456-789ABCD-EFGHJKM");
  });

  it("reads look-alikes the way the backend does and drops the rest", () => {
    expect(formatCode("o1l-i")).toBe("0111");
    // U is not in Crockford base32; spaces, dashes and punctuation go.
    expect(formatCode(" ab u-c.d ")).toBe("ABCD");
  });

  it("stops at 21 characters and knows when it is whole", () => {
    const pasted = "7K3M9QX-2HV8RTA-4NBWZ6P extra";
    expect(formatCode(pasted)).toBe("7K3M9QX-2HV8RTA-4NBWZ6P");
    expect(codeComplete(formatCode(pasted))).toBe(true);
    expect(codeComplete("7K3M9QX-2HV8RTA-4NBWZ6")).toBe(false);
  });

  it("accepts node addresses the backend accepts", () => {
    expect(validNode("tcp://example.com:11010")).toBe(true);
    expect(validNode("wss://et.example.cn")).toBe(true);
    expect(validNode("example.com:11010")).toBe(false);
    expect(validNode("tcp://bad host")).toBe(false);
    expect(validNode(`tcp://${"a".repeat(200)}`)).toBe(false);
  });
});

describe("p2p health", () => {
  it("maps every level to one tone, pulsing only while connecting", () => {
    const tones: Record<P2pLevel, string> = { good: "accent", fair: "warn", starting: "warn", poor: "danger", down: "danger", idle: "neutral" };
    for (const [level, tone] of Object.entries(tones) as [P2pLevel, string][]) {
      expect(LEVELS[level].tone).toBe(tone);
      expect(LEVELS[level].pulse).toBe(level === "starting");
      expect(LEVELS[level].label).not.toBe("");
    }
  });

  it("counts a room as live while it is open, being set up or being asked for", () => {
    const status = (patch: Partial<P2pStatus>) => ({ active: false, preparing: false, ...patch }) as P2pStatus;
    expect(roomLive({ busy: false, status: null })).toBe(false);
    expect(roomLive({ busy: true, status: null })).toBe(true);
    expect(roomLive({ busy: false, status: status({ preparing: true }) })).toBe(true);
    expect(roomLive({ busy: false, status: status({ active: true }) })).toBe(true);
    expect(roomLive({ busy: false, status: status({}) })).toBe(false);
  });
});
