import { AnimatePresence, motion } from "motion/react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { create } from "zustand";

import { Icon, type IconName } from "./Icon";

export interface MenuItem {
  label: string;
  icon?: IconName;
  danger?: boolean;
  disabled?: boolean;
  run(): void;
}

type Menu = { x: number; y: number; items: (MenuItem | "-")[] };

export const useContextMenu = create<{ menu: Menu | null; open(x: number, y: number, items: (MenuItem | "-")[]): void; close(): void }>((set) => ({
  menu: null,
  open: (x, y, items) => set({ menu: { x, y, items } }),
  close: () => set({ menu: null }),
}));

/** The right-click menu, one for the whole window. It closes on any click
    outside it, Escape, scrolling or the window losing focus. */
export function ContextMenuHost() {
  const menu = useContextMenu((s) => s.menu);
  const close = useContextMenu((s) => s.close);
  const ref = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<{ left: number; top: number } | null>(null);
  const [active, setActive] = useState(-1);
  // Read by the key handler, which must not run commands inside a state
  // update (React may call those twice).
  const activeRef = useRef(-1);
  activeRef.current = active;

  // Keep the menu inside the window.
  useLayoutEffect(() => {
    if (!menu || !ref.current) return setPlace(null);
    const box = ref.current.getBoundingClientRect();
    setPlace({ left: Math.min(menu.x, window.innerWidth - box.width - 8), top: Math.min(menu.y, window.innerHeight - box.height - 8) });
    setActive(-1);
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const items = menu.items.filter((i): i is MenuItem => i !== "-" && !i.disabled);
    const onDown = (e: PointerEvent) => !ref.current?.contains(e.target as Node) && close();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        if (items.length) setActive((i) => (i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length);
      } else if (e.key === "Enter") {
        e.preventDefault();
        const chosen = items[activeRef.current];
        if (chosen) {
          close();
          chosen.run();
        }
      }
    };
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    document.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      document.removeEventListener("scroll", close, true);
    };
  }, [menu, close]);

  let index = -1;
  return (
    <AnimatePresence>
      {menu && (
        <motion.div
          ref={ref}
          className="context-menu"
          role="menu"
          style={{ left: place?.left ?? menu.x, top: place?.top ?? menu.y, visibility: place ? "visible" : "hidden" }}
          initial={{ opacity: 0, y: -4 }}
          animate={{ opacity: 1, y: 0, transition: { duration: 0.12, ease: [0.2, 0.8, 0.2, 1] } }}
          exit={{ opacity: 0, transition: { duration: 0.08 } }}
          onContextMenu={(e) => e.preventDefault()}
        >
          {menu.items.map((item, i) => {
            if (item === "-") return <div key={`sep-${i}`} className="context-sep" role="separator" />;
            if (!item.disabled) index++;
            const at = item.disabled ? -2 : index;
            return (
              <button
                key={item.label}
                type="button"
                role="menuitem"
                className={`context-item${item.danger ? " danger" : ""}`}
                disabled={item.disabled}
                aria-selected={at === active}
                onMouseMove={() => at >= 0 && setActive(at)}
                onClick={() => {
                  close();
                  item.run();
                }}
              >
                {item.icon ? <Icon name={item.icon} /> : <span style={{ width: 14 }} />}
                {item.label}
              </button>
            );
          })}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
