import type DOMPurifyType from "dompurify";

type Sanitizer = {
  marked: (typeof import("marked"))["marked"];
  DOMPurify: typeof DOMPurifyType;
};

let sanitizerPromise: Promise<Sanitizer> | null = null;

/** Validates that a URL is a safe `https://` URL (optionally upgrading plain `http://`). */
export function safeHttpsUrl(url: string | null | undefined, upgradeHttp = false): string | null {
  if (!url) return null;
  const trimmed = url.trim();
  if (!trimmed) return null;
  const candidate = upgradeHttp ? trimmed.replace(/^http:\/\//i, "https://") : trimmed;
  try {
    const parsed = new URL(candidate);
    if (parsed.protocol !== "https:") return null;
    return parsed.href;
  } catch {
    return null;
  }
}

/** Formats a validated `https://` URL for CSS `backgroundImage: url("...")`,
 *  rejecting characters that could break out of a CSS string. */
export function cssBackgroundUrl(url: string | null | undefined, upgradeHttp = false): string | undefined {
  if (!url || /["'()\\\s]/.test(url.trim())) return undefined;
  const safe = safeHttpsUrl(url, upgradeHttp);
  if (!safe || /["'()\\\s]/.test(safe)) return undefined;
  return `url("${safe}")`;
}

export interface SanitizedNode {
  tagName: string;
  getAttribute(name: string): string | null;
  setAttribute(name: string, value: string): void;
  removeAttribute(name: string): void;
}

/** Enforces HTTPS-only external URLs and safe attributes on sanitized HTML nodes. */
export function sanitizeElementAttributes(node: SanitizedNode): void {
  if (node.tagName === "IMG") {
    const safeSrc = safeHttpsUrl(node.getAttribute("src"));
    if (!safeSrc) {
      node.removeAttribute("src");
      return;
    }
    node.setAttribute("src", safeSrc);
    node.setAttribute("loading", "lazy");
    node.setAttribute("referrerpolicy", "no-referrer");
  } else if (node.tagName === "A") {
    const safeHref = safeHttpsUrl(node.getAttribute("href"));
    if (!safeHref) {
      node.removeAttribute("href");
      return;
    }
    node.setAttribute("href", safeHref);
    node.setAttribute("rel", "noopener noreferrer");
  }
}

function loadSanitizer(): Promise<Sanitizer> {
  if (!sanitizerPromise) {
    sanitizerPromise = Promise.all([import("marked"), import("dompurify")]).then(([{ marked }, { default: DOMPurify }]) => {
      DOMPurify.removeAllHooks();
      DOMPurify.addHook("afterSanitizeAttributes", (node) => sanitizeElementAttributes(node));
      return { marked, DOMPurify };
    });
  }
  return sanitizerPromise;
}

/** Renders Markdown or HTML project text and sanitizes it for safe display.
 *  The libraries load once on first use and hooks are registered once. */
export async function renderSanitizedHtml(body: string, format: "markdown" | "html"): Promise<string> {
  const { marked, DOMPurify } = await loadSanitizer();
  const html = format === "markdown" ? (marked.parse(body, { async: false, gfm: true }) as string) : body;
  return DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select", "iframe", "video", "audio"],
    FORBID_ATTR: ["style", "class", "id", "target"],
  });
}
