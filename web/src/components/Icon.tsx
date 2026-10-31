// Line icons from assets/icons (24 px grid, 1.8 stroke), plus a few
// additions drawn on the same grid. The gear is an exact 8-tooth outline.

const PATHS: Record<string, string[]> = {
  alert: ["M12 9v4", "M12 17h.01", "M10.3 3.9 2.4 18a2 2 0 0 0 1.7 3h15.8a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"],
  box: ["M21 8.5 12 3 3 8.5v7L12 21l9-5.5z", "M3 8.5 12 14l9-5.5", "M12 14v7"],
  check: ["m20 6-11 11-5-5"],
  close: ["M6 6l12 12", "M18 6 6 18"],
  copy: ["M8 8h12v12H8z", "M4 16V4h12"],
  download: ["M12 3v12", "m7 10 5 5 5-5", "M5 21h14"],
  folder: ["M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"],
  home: ["M3 10.5 12 3l9 7.5", "M5 9.5V21h14V9.5"],
  link: ["M10 13a5 5 0 0 0 7.5.5l2-2a5 5 0 0 0-7-7l-1.5 1.5", "M14 11a5 5 0 0 0-7.5-.5l-2 2a5 5 0 0 0 7 7L13 18"],
  log: ["M4 4h16v16H4z", "M8 8h8", "M8 12h8", "M8 16h5"],
  monitor: ["M3 5h18v12H3z", "M8 21h8", "M12 17v4"],
  plus: ["M12 5v14", "M5 12h14"],
  refresh: ["M21 12a9 9 0 0 1-15.5 6.2L3 16", "M3 21v-5h5", "M3 12a9 9 0 0 1 15.5-6.2L21 8", "M21 3v5h-5"],
  search: ["M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z", "m21 21-4.35-4.35"],
  settings: [
    "M10.87 2.47 L13.13 2.47 L13.58 5.18 L15.70 6.06 L17.94 4.46 L19.54 6.06 L17.94 8.30 L18.82 10.42 L21.53 10.87 L21.53 13.13 L18.82 13.58 L17.94 15.70 L19.54 17.94 L17.94 19.54 L15.70 17.94 L13.58 18.82 L13.13 21.53 L10.87 21.53 L10.42 18.82 L8.30 17.94 L6.06 19.54 L4.46 17.94 L6.06 15.70 L5.18 13.58 L2.47 13.13 L2.47 10.87 L5.18 10.42 L6.06 8.30 L4.46 6.06 L6.06 4.46 L8.30 6.06 L10.42 5.18Z",
    "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
  ],
  shield: ["M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z", "m9 12 2 2 4-4"],
  stop: ["M7 6h10a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1z"],
  trash: ["M4 7h16", "M9 7V4h6v3", "m6 7 1 14h10l1-14", "M10 11v6", "M14 11v6"],
  user: ["M20 21a8 8 0 0 0-16 0", "M12 13a5 5 0 1 0 0-10 5 5 0 0 0 0 10z"],
  // Additions on the same grid.
  chevronRight: ["m9 6 6 6-6 6"],
  chevronLeft: ["m15 6-6 6 6 6"],
  external: ["M14 4h6v6", "M20 4 11 13", "M19 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h5"],
  archive: ["M3 5h18v4H3z", "M5 9v10h14V9", "M10 13h4"],
  layers: ["M12 3 3 8l9 5 9-5z", "m3 13 9 5 9-5"],
  cpu: ["M7 7h10v10H7z", "M10 3v4", "M14 3v4", "M10 17v4", "M14 17v4", "M3 10h4", "M3 14h4", "M17 10h4", "M17 14h4"],
  coffee: ["M5 8h11v6a5 5 0 0 1-5 5h-1a5 5 0 0 1-5-5z", "M16 10h1.5a2.5 2.5 0 0 1 0 5H16", "M8 3v2", "M12 3v2"],
  info: ["M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z", "M12 11v5", "M12 8h.01"],
  palette: ["M12 3a9 9 0 1 0 0 18c1 0 1.6-.8 1.6-1.7 0-.5-.2-.8-.5-1.2-.3-.4-.5-.7-.5-1.2 0-.9.7-1.6 1.6-1.6H16a5 5 0 0 0 5-5c0-4.1-4-7.3-9-7.3z", "M7.5 11.5h.01", "M10 7.5h.01", "M14.5 7.5h.01"],
  sparkle: ["M12 3v5", "M12 16v5", "M3 12h5", "M16 12h5", "m6 6 2.5 2.5", "m15.5 15.5 2.5 2.5", "m6 18 2.5-2.5", "m15.5 8.5 2.5-2.5"],
  edit: ["M4 20h4L19 9l-4-4L4 16z", "m13.5 6.5 4 4"],
  chevronUp: ["m6 15 6-6 6 6"],
  chevronDown: ["m6 9 6 6 6-6"],
  disk: ["M4 5h16v14H4z", "M4 14h16", "M16.5 16.5h.01"],
  image: ["M4 5h16v14H4z", "m4 16 5-5 4 4 3-3 4 4", "M15.5 9.5h.01"],
  star: ["m12 3.5 2.6 5.4 5.9.8-4.3 4.1 1 5.9L12 16.9l-5.2 2.8 1-5.9-4.3-4.1 5.9-.8z"],
  globe: ["M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z", "M3 12h18", "M12 3a14 14 0 0 1 0 18", "M12 3a14 14 0 0 0 0 18"],
  grid: ["M4 4h7v7H4z", "M13 4h7v7h-7z", "M4 13h7v7H4z", "M13 13h7v7h-7z"],
  rows: ["M4 4h16v6H4z", "M4 14h16v6H4z"],
  tag: ["M3 3h8l10 10-8 8L3 11z", "M7.5 7.5h.01"],
};

export type IconName = keyof typeof PATHS | "play";

export function Icon({ name, size = 16, className }: { name: IconName; size?: number; className?: string }) {
  const cls = `icon${className ? ` ${className}` : ""}`;
  if (name === "play") {
    return (
      <svg className={cls} width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden>
        <path d="M8 5.5v13l11-6.5z" />
      </svg>
    );
  }
  return (
    <svg
      className={cls}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {PATHS[name].map((d) => (
        <path key={d} d={d} />
      ))}
    </svg>
  );
}
