import { MotionGlobalConfig } from "motion/react";

import type { Instance } from "../api/types";
import { useStore } from "../store";
import { initialState, type Store } from "../store/types";

MotionGlobalConfig.skipAnimations = true;

type ResizeRecord = { callback: ResizeObserverCallback; targets: Set<Element> };

/**
 * Installs minimal browser shims needed by layout/virtualization components
 * under happy-dom. Returns a helper to trigger ResizeObserver callbacks on
 * demand in tests.
 */
export function installDomShims() {
  const records = new Set<ResizeRecord>();

  class MockResizeObserver implements ResizeObserver {
    private record: ResizeRecord;
    constructor(callback: ResizeObserverCallback) {
      this.record = { callback, targets: new Set() };
      records.add(this.record);
    }
    observe(target: Element) {
      this.record.targets.add(target);
    }
    unobserve(target: Element) {
      this.record.targets.delete(target);
    }
    disconnect() {
      this.record.targets.clear();
      records.delete(this.record);
    }
  }

  Object.defineProperty(globalThis, "ResizeObserver", {
    configurable: true,
    writable: true,
    value: MockResizeObserver,
  });

  if (typeof window !== "undefined" && !window.matchMedia) {
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: (query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }),
    });
  }

  if (typeof HTMLElement !== "undefined") {
    HTMLElement.prototype.scrollIntoView = () => {};
    // happy-dom's Web Animations API throws unhandled AbortError rejections
    // when motion-dom cancels animations on unmount; disabling native animate
    // makes Motion complete transitions cleanly without unhandled rejections.
    Object.defineProperty(Element.prototype, "animate", {
      configurable: true,
      writable: true,
      value: undefined,
    });
  }

  return {
    triggerResize(target?: Element) {
      for (const rec of records) {
        const matched = target ? (rec.targets.has(target) ? [target] : []) : [...rec.targets];
        if (matched.length === 0) continue;
        rec.callback(
          matched.map((el) => ({ target: el }) as ResizeObserverEntry),
          {} as ResizeObserver,
        );
      }
    },
  };
}

export function makeInstance(patch: Partial<Instance> = {}): Instance {
  return {
    id: "world-1",
    name: "Forest Survival",
    versionId: "1.21.4",
    versionType: "release",
    loader: "fabric",
    loaderVersion: "0.16.10",
    installed: true,
    memoryMb: null,
    javaPath: null,
    accountId: null,
    launch: {
      jvmArgs: "",
      gameArgs: "",
      windowWidth: null,
      windowHeight: null,
      env: "",
      wrapper: "",
      preLaunch: "",
      postExit: "",
    },
    favorite: false,
    group: null,
    server: null,
    createdAt: 1_700_000_000_000,
    lastLaunchedAt: 1_700_000_100_000,
    playSeconds: 3600,
    ...patch,
  };
}

export function resetStore(patch: Partial<Store> = {}) {
  useStore.setState({
    ...initialState,
    reduceMotion: true,
    ...patch,
  });
}
