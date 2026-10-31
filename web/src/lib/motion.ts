// Motion tokens, mirrored from tokens.css: short durations, ease-out to
// enter, ease-in to leave, small offsets only. No bounce.

export const EASE_OUT = [0.2, 0.8, 0.2, 1] as const;
export const EASE_IN = [0.4, 0, 1, 1] as const;

export const fast = { duration: 0.12, ease: EASE_OUT };
export const normal = { duration: 0.2, ease: EASE_OUT };
export const slow = { duration: 0.3, ease: EASE_OUT };

/** Sliding highlights (nav, tabs, segmented controls). */
export const glide = { type: "spring", stiffness: 520, damping: 42, mass: 0.9 } as const;

export const fadeRise = {
  initial: { opacity: 0, y: 6 },
  animate: { opacity: 1, y: 0, transition: normal },
  exit: { opacity: 0, y: 4, transition: { duration: 0.12, ease: EASE_IN } },
};

export const panelIn = {
  initial: { opacity: 0, y: 10, scale: 0.985 },
  animate: { opacity: 1, y: 0, scale: 1, transition: slow },
  exit: { opacity: 0, y: 6, scale: 0.99, transition: { duration: 0.14, ease: EASE_IN } },
};

/** Children of a page enter one after another, 30 ms apart. */
export const stagger = {
  animate: { transition: { staggerChildren: 0.03 } },
};

export const item = {
  initial: { opacity: 0, y: 6 },
  animate: { opacity: 1, y: 0, transition: normal },
};
