import { useState } from "react";

import { useCover } from "../lib/biome";
import { shotUrl } from "../lib/screenshots";

/** The screenshot chosen as a world's cover, laid over its generated
    scenery; nothing when there is none or it can no longer be loaded (the
    scenery then simply shows). `full` loads the picture itself rather than
    the thumbnail, for the home stage. */
export function SceneCover({ id, full, className }: { id: string; full?: boolean; className?: string }) {
  const cover = useCover(id);
  const url = cover ? shotUrl(id, { name: cover.name, modifiedMs: cover.modifiedMs, bytes: 0, demoUrl: cover.demoUrl }, full) : null;
  const [failed, setFailed] = useState<string | null>(null);
  if (!url || failed === url) return null;
  return <img className={`scene-cover${className ? ` ${className}` : ""}`} src={url} alt="" draggable={false} onError={() => setFailed(url)} />;
}
