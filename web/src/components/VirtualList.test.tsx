// @vitest-environment happy-dom
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { installDomShims } from "../test/dom";
import { VirtualList } from "./overlay";

describe("VirtualList", () => {
  let shims: ReturnType<typeof installDomShims>;

  beforeEach(() => {
    shims = installDomShims();
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  function setClientHeight(el: HTMLElement, value: number) {
    Object.defineProperty(el, "clientHeight", { configurable: true, value });
  }

  it("virtualizes 500 rows to a bounded DOM window", () => {
    const items = Array.from({ length: 500 }, (_, i) => `item-${i}`);
    const { container } = render(
      <VirtualList
        items={items}
        rowHeight={56}
        overscan={4}
        getKey={(item) => item}
        render={(item) => <div className="test-row">{item}</div>}
      />,
    );

    const viewport = container.querySelector<HTMLElement>(".virtual")!;
    setClientHeight(viewport, 400);
    act(() => shims.triggerResize(viewport));

    const inner = container.querySelector<HTMLElement>(".virtual-inner")!;
    expect(inner.style.height).toBe(`${500 * 56}px`);

    const rendered = container.querySelectorAll(".test-row");
    // 400 / 56 = 8 visible rows + 4 overscan below = 12..14 mounted rows, never 500.
    expect(rendered.length).toBeGreaterThan(8);
    expect(rendered.length).toBeLessThan(25);
  });

  it("shifts rows below an expanded item using ResizeObserver measurement", () => {
    const items = Array.from({ length: 40 }, (_, i) => `row-${i}`);
    const { container, rerender } = render(
      <VirtualList
        items={items}
        rowHeight={56}
        overscan={4}
        expandedIndex={null}
        getKey={(item) => item}
        render={(item) => <div data-row={item}>{item}</div>}
      />,
    );

    const viewport = container.querySelector<HTMLElement>(".virtual")!;
    setClientHeight(viewport, 400);
    act(() => shims.triggerResize(viewport));

    const row3WrapperBefore = container.querySelector("[data-row='row-3']")!.parentElement as HTMLElement;
    expect(row3WrapperBefore.style.top).toBe(`${3 * 56}px`);

    rerender(
      <VirtualList
        items={items}
        rowHeight={56}
        overscan={4}
        expandedIndex={2}
        getKey={(item) => item}
        render={(item) => <div data-row={item}>{item}</div>}
      />,
    );

    const expandedWrapper = container.querySelector("[data-row='row-2']")!.parentElement as HTMLElement;
    vi.spyOn(expandedWrapper, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      width: 600,
      height: 176,
      top: 0,
      right: 600,
      bottom: 176,
      left: 0,
      toJSON: () => ({}),
    });
    act(() => shims.triggerResize(expandedWrapper));

    const inner = container.querySelector<HTMLElement>(".virtual-inner")!;
    expect(inner.style.height).toBe(`${40 * 56 + 120}px`);
    const row3WrapperAfter = container.querySelector("[data-row='row-3']")!.parentElement as HTMLElement;
    expect(row3WrapperAfter.style.top).toBe(`${3 * 56 + 120}px`);

    // Collapsing restores the exact base height and offset.
    rerender(
      <VirtualList
        items={items}
        rowHeight={56}
        overscan={4}
        expandedIndex={null}
        getKey={(item) => item}
        render={(item) => <div data-row={item}>{item}</div>}
      />,
    );
    expect(inner.style.height).toBe(`${40 * 56}px`);
    expect((container.querySelector("[data-row='row-3']")!.parentElement as HTMLElement).style.top).toBe(`${3 * 56}px`);
  });

  it("resets scrollTop when resetScrollKey changes and clamps when items shrink", () => {
    const items = Array.from({ length: 300 }, (_, i) => `entry-${i}`);
    const { container, rerender } = render(
      <VirtualList
        items={items}
        rowHeight={56}
        overscan={2}
        resetScrollKey="all"
        getKey={(item) => item}
        render={(item) => <div className="entry">{item}</div>}
      />,
    );

    const viewport = container.querySelector<HTMLElement>(".virtual")!;
    setClientHeight(viewport, 400);
    act(() => shims.triggerResize(viewport));

    viewport.scrollTop = 2800;
    fireEvent.scroll(viewport);
    expect(container.textContent).not.toContain("entry-0");

    // Filter/query change resets scroll to the top.
    rerender(
      <VirtualList
        items={items}
        rowHeight={56}
        overscan={2}
        resetScrollKey="filtered"
        getKey={(item) => item}
        render={(item) => <div className="entry">{item}</div>}
      />,
    );
    expect(viewport.scrollTop).toBe(0);
    expect(container.textContent).toContain("entry-0");

    // Scroll deep down, then shrink the list without a synthetic scroll event (WKWebView behavior).
    viewport.scrollTop = 10000;
    fireEvent.scroll(viewport);
    rerender(
      <VirtualList
        items={["entry-0", "entry-1", "entry-2"]}
        rowHeight={56}
        overscan={2}
        resetScrollKey="filtered"
        getKey={(item) => item}
        render={(item) => <div className="entry">{item}</div>}
      />,
    );
    expect(viewport.scrollTop).toBe(0);
    expect(container.querySelectorAll(".entry").length).toBe(3);
  });
});
