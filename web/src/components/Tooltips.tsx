import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

/** How long a pointer rests before the tip shows, and how soon after one
    hides the next shows at once (moving along a row of icon buttons). */
const SHOW_MS = 450;
const CHAIN_MS = 300;
const GAP = 8;
const EDGE = 8;

/** Tips in the sidebar open beside it, not over the next icon up. */
type Tip = { text: string; rect: DOMRect; beside: boolean };

/** Every `title` in the app shows as a forest tooltip instead of the
    system's own box (white and grey on Windows), which sat badly beside
    the pixel art. While pointed at, an element's title moves to
    `data-tip` so the browser doesn't show its own as well; it goes back
    when the pointer leaves, so the attribute keeps naming the element. */
export function Tooltips() {
  const [tip, setTip] = useState<Tip | null>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let target: HTMLElement | null = null;
    let named = false;
    let timer = 0;
    let visible = false;
    let hiddenAt = 0;

    const release = () => {
      window.clearTimeout(timer);
      if (target) {
        const text = target.dataset.tip;
        delete target.dataset.tip;
        // React may have set a new title meanwhile; that one wins.
        if (text !== undefined && !target.hasAttribute("title")) target.setAttribute("title", text);
        if (named) target.removeAttribute("aria-label");
      }
      target = null;
      named = false;
      if (visible) hiddenAt = Date.now();
      visible = false;
      setTip(null);
    };

    const show = (el: HTMLElement) => {
      const text = el.getAttribute("title") ?? el.dataset.tip ?? "";
      if (!text.trim() || !el.isConnected) return;
      visible = true;
      setTip({ text, rect: el.getBoundingClientRect(), beside: !!el.closest(".sidebar") });
    };

    const over = (e: PointerEvent) => {
      if (e.pointerType === "touch") return;
      const el = e.target instanceof Element ? e.target.closest<HTMLElement>("[title], [data-tip]") : null;
      if (el === target) return;
      release();
      if (!el) return;
      const text = el.getAttribute("title");
      if (text === null || !text.trim()) return;
      target = el;
      el.dataset.tip = text;
      el.removeAttribute("title");
      // An icon-only control is named by its title; keep it named.
      if (!el.hasAttribute("aria-label") && !el.textContent?.trim()) {
        el.setAttribute("aria-label", text);
        named = true;
      }
      const wait = Date.now() - hiddenAt < CHAIN_MS ? 0 : SHOW_MS;
      timer = window.setTimeout(() => show(el), wait);
    };

    const leaveWindow = (e: PointerEvent) => {
      if (!e.relatedTarget) release();
    };

    document.addEventListener("pointerover", over, true);
    document.addEventListener("pointerout", leaveWindow, true);
    document.addEventListener("pointerdown", release, true);
    document.addEventListener("keydown", release, true);
    window.addEventListener("scroll", release, true);
    window.addEventListener("blur", release);
    return () => {
      release();
      document.removeEventListener("pointerover", over, true);
      document.removeEventListener("pointerout", leaveWindow, true);
      document.removeEventListener("pointerdown", release, true);
      document.removeEventListener("keydown", release, true);
      window.removeEventListener("scroll", release, true);
      window.removeEventListener("blur", release);
    };
  }, []);

  // Above the element, centred; below it when there is no room above, and
  // kept inside the window either way. In the sidebar, to its right.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el || !tip) return;
    const { width, height } = el.getBoundingClientRect();
    const { rect, beside } = tip;
    const clampX = (x: number) => Math.min(Math.max(EDGE, x), window.innerWidth - EDGE - width);
    const clampY = (y: number) => Math.min(Math.max(EDGE, y), window.innerHeight - EDGE - height);
    const above = rect.top - GAP - height;
    const left = beside ? clampX(rect.right + GAP) : clampX(rect.left + rect.width / 2 - width / 2);
    const top = beside ? clampY(rect.top + rect.height / 2 - height / 2) : above >= EDGE ? above : clampY(rect.bottom + GAP);
    el.style.left = `${Math.round(left)}px`;
    el.style.top = `${Math.round(top)}px`;
    el.style.visibility = "visible";
  }, [tip]);

  if (!tip) return null;
  return createPortal(
    <div ref={box} className="tooltip" role="tooltip" style={{ visibility: "hidden" }}>
      {tip.text}
    </div>,
    document.body,
  );
}
