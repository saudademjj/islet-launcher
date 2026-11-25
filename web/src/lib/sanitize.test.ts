import { describe, expect, it } from "vitest";

import { cssBackgroundUrl, safeHttpsUrl, sanitizeElementAttributes, type SanitizedNode } from "./sanitize";

function fakeNode(tagName: string, attrs: Record<string, string>): SanitizedNode & { attrs: Record<string, string> } {
  const map = { ...attrs };
  return {
    tagName,
    attrs: map,
    getAttribute: (name) => (name in map ? map[name] : null),
    setAttribute: (name, value) => {
      map[name] = value;
    },
    removeAttribute: (name) => {
      delete map[name];
    },
  };
}

describe("safeHttpsUrl", () => {
  it("accepts valid https URLs and rejects unsafe schemes", () => {
    expect(safeHttpsUrl("https://cdn.modrinth.com/a.png")).toBe("https://cdn.modrinth.com/a.png");
    expect(safeHttpsUrl("  https://example.com/path?x=1  ")).toBe("https://example.com/path?x=1");
    expect(safeHttpsUrl("http://example.com/a.png")).toBeNull();
    expect(safeHttpsUrl("javascript:alert(1)")).toBeNull();
    expect(safeHttpsUrl("data:image/svg+xml,<svg/>")).toBeNull();
    expect(safeHttpsUrl("file:///etc/passwd")).toBeNull();
    expect(safeHttpsUrl("")).toBeNull();
    expect(safeHttpsUrl(null)).toBeNull();
  });

  it("upgrades plain http only when requested", () => {
    expect(safeHttpsUrl("http://textures.minecraft.net/texture/abc", true)).toBe("https://textures.minecraft.net/texture/abc");
    expect(safeHttpsUrl("javascript:alert(1)", true)).toBeNull();
  });
});

describe("cssBackgroundUrl", () => {
  it("wraps safe URLs in double quotes and rejects CSS injection characters", () => {
    expect(cssBackgroundUrl("http://textures.minecraft.net/texture/abc", true)).toBe('url("https://textures.minecraft.net/texture/abc")');
    expect(cssBackgroundUrl('https://example.com/a")', true)).toBeUndefined();
    expect(cssBackgroundUrl("https://example.com/a'b", true)).toBeUndefined();
    expect(cssBackgroundUrl("https://example.com/a\\b", true)).toBeUndefined();
    expect(cssBackgroundUrl("https://example.com/?q=a\\b", true)).toBeUndefined();
    expect(cssBackgroundUrl("javascript:alert(1)", true)).toBeUndefined();
  });
});

describe("sanitizeElementAttributes", () => {
  it("keeps https images with lazy loading and no-referrer, and strips non-https src", () => {
    const ok = fakeNode("IMG", { src: "https://cdn.modrinth.com/icon.png" });
    sanitizeElementAttributes(ok);
    expect(ok.attrs).toEqual({
      src: "https://cdn.modrinth.com/icon.png",
      loading: "lazy",
      referrerpolicy: "no-referrer",
    });

    for (const badSrc of ["http://example.com/a.png", "data:image/png;base64,AA==", "file:///tmp/x.png", "javascript:alert(1)"]) {
      const bad = fakeNode("IMG", { src: badSrc });
      sanitizeElementAttributes(bad);
      expect(bad.attrs.src).toBeUndefined();
    }
  });

  it("keeps https links with noopener noreferrer and strips unsafe hrefs", () => {
    const ok = fakeNode("A", { href: "https://modrinth.com/mod/sodium" });
    sanitizeElementAttributes(ok);
    expect(ok.attrs).toEqual({
      href: "https://modrinth.com/mod/sodium",
      rel: "noopener noreferrer",
    });

    for (const badHref of ["javascript:alert(1)", "http://example.com", "file:///etc/hosts", "data:text/html,hi"]) {
      const bad = fakeNode("A", { href: badHref });
      sanitizeElementAttributes(bad);
      expect(bad.attrs.href).toBeUndefined();
    }
  });
});
