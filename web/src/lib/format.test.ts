import { describe, expect, it } from "vitest";

import { formatBytes, formatCount, formatPlayTime, progressRatio, sameArch, skyPhase, updateName, validGuid } from "./format";

describe("format", () => {
  it("formats sizes and counts compactly", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(150 * 1024 * 1024)).toBe("150 MB");
    expect(formatCount(950)).toBe("950");
    expect(formatCount(1234)).toBe("1.2K");
    expect(formatCount(34_000)).toBe("34K");
    expect(formatCount(10_500_000)).toBe("11M");
  });

  it("formats play time in minutes, then hours", () => {
    expect(formatPlayTime(30)).toBe("不到 1 分钟");
    expect(formatPlayTime(600)).toBe("10 分钟");
    expect(formatPlayTime(3600 * 2.5)).toBe("2.5 小时");
    expect(formatPlayTime(3600 * 12.4)).toBe("12 小时");
  });

  it("prefers byte progress over file counts", () => {
    expect(progressRatio(1, 4, 50, 100)).toBe(0.5);
    expect(progressRatio(1, 4, 0, 0)).toBe(0.25);
    expect(progressRatio(0, 0, 0, 0)).toBeNull();
    expect(progressRatio(5, 4, 0, 0)).toBe(1);
  });

  it("compares CPU families and client IDs", () => {
    expect(sameArch("aarch64", "arm64")).toBe(true);
    expect(sameArch("amd64", "x86_64")).toBe(true);
    expect(sameArch("arm64", "x64")).toBe(false);
    expect(validGuid("00000000-0000-0000-0000-00000000abcd")).toBe(true);
    expect(validGuid("not-a-guid")).toBe(false);
  });

  it("names updates and the sky by the hour", () => {
    expect(updateName("1.20.1")).toBe("足迹与故事");
    expect(updateName("1.21.4")).toBe("苍园觉醒");
    expect(updateName("24w14a")).toBeNull();
    expect(skyPhase(new Date(2026, 0, 1, 6))).toBe("dawn");
    expect(skyPhase(new Date(2026, 0, 1, 12))).toBe("day");
    expect(skyPhase(new Date(2026, 0, 1, 18))).toBe("dusk");
    expect(skyPhase(new Date(2026, 0, 1, 23))).toBe("night");
  });
});
