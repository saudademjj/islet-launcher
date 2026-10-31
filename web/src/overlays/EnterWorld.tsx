import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";

import { Celestial, Clouds, Landscape, PX, Stars } from "../components/Pixel";
import { SceneCover } from "../components/SceneCover";
import { Button } from "../components/ui";
import { biomeStyle, useBiome } from "../lib/biome";
import { skyPhase } from "../lib/format";
import { MOD_KEY } from "../lib/platform";
import { usePick } from "../store";
import { t } from "../lib/i18n";

const TIPS = [
  t("按 F3 可以看到坐标、朝向和帧率。", "Press F3 to see coordinates, facing and frame rate."),
  t("按 F2 截图，截图会出现在启动器的「截图」页。", "Press F2 for a screenshot; it shows up on the launcher's Screenshots tab."),
  t("在主世界睡一觉可以跳过夜晚，也会把重生点设在床边。", "Sleeping in the Overworld skips the night and sets your spawn by the bed."),
  t("按住 Shift 走到方块边缘不会掉下去。", "Hold Shift to walk to a block's edge without falling."),
  t("1.18 以后，钻石在 Y=-58 附近最多。", "Since 1.18, diamonds are most common around Y=-58."),
  t("火把能阻止大多数敌对生物在附近生成。", "Torches stop most hostile mobs from spawning nearby."),
  t("进游戏前可以在「管理 → 游戏选项」里调渲染距离和视场角。", "Set render distance and FOV under Manage → Game options before playing."),
  t(`随时按 ${MOD_KEY}K 搜索世界、服务器和设置。`, `Press ${MOD_KEY}K any time to search worlds, servers and settings.`),
  t("下界里一格等于主世界八格，适合用来赶路。", "One block in the Nether is eight in the Overworld: a handy shortcut."),
  t("铁镐以上才能挖下钻石矿，用石镐挖会什么也得不到。", "Diamond ore needs an iron pickaxe or better; stone gets you nothing."),
];

/** How long "the world is ready" stays up once the game is running. */
const LINGER_MS = 1600;

type Shown = { id: string; phase: "starting" | "running" };

/** The moment between "start" and the game window: the world's own sky,
    its land rising, a line in pixel type and a tip, the way the game's own
    loading screen fills the wait. It follows the launch state and never
    blocks it; "keep using the launcher" puts it away. */
export function EnterWorld() {
  const s = usePick((s) => ({ launchPending: s.launchPending, running: s.running, instances: s.instances }));
  const [shown, setShown] = useState<Shown | null>(null);
  const [dismissed, setDismissed] = useState<string | null>(null);

  // Every update below is guarded, so the effect settles instead of
  // re-rendering itself while a launch is pending.
  useEffect(() => {
    if (s.launchPending && s.launchPending !== dismissed) {
      if (shown?.id !== s.launchPending || shown.phase !== "starting") setShown({ id: s.launchPending, phase: "starting" });
      return;
    }
    if (!s.launchPending && dismissed) setDismissed(null);
    if (!shown) return;
    if (s.running === shown.id) {
      if (shown.phase === "starting") {
        setShown({ id: shown.id, phase: "running" });
        return;
      }
      const timer = setTimeout(() => setShown(null), LINGER_MS);
      return () => clearTimeout(timer);
    }
    // The launch ended without a game (an error, or it was cancelled).
    if (!s.launchPending) setShown(null);
  }, [s.launchPending, s.running, shown, dismissed]);

  useEffect(() => {
    if (!shown) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      setDismissed(shown.id);
      setShown(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [shown]);

  const instance = shown ? s.instances.find((i) => i.id === shown.id) : null;
  return (
    <AnimatePresence>
      {shown && instance && (
        <Scene
          key={shown.id}
          id={instance.id}
          name={instance.name}
          ready={shown.phase === "running"}
          onDismiss={() => {
            setDismissed(shown.id);
            setShown(null);
          }}
        />
      )}
    </AnimatePresence>
  );
}

function Scene({ id, name, ready, onDismiss }: { id: string; name: string; ready: boolean; onDismiss: () => void }) {
  const biome = useBiome(id);
  const phase = skyPhase();
  const night = phase === "night";
  const tip = useMemo(() => TIPS[Math.floor(Math.random() * TIPS.length)], []);
  return (
    <motion.div
      className={`enter ${phase}`}
      style={biomeStyle(biome, phase)}
      role="status"
      aria-live="polite"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1, transition: { duration: 0.24, ease: [0.2, 0.8, 0.2, 1] } }}
      exit={{ opacity: 0, transition: { duration: 0.32, ease: [0.4, 0, 1, 1] } }}
    >
      {night && <Stars seed={id} cols={170} rows={16} unit={PX} className="hero-stars" />}
      <Clouds seed={id} cols={170} rows={9} unit={PX} className="hero-sky" />
      <div className="enter-celestial">
        <Celestial night={night} />
      </div>
      {/* The camera sinks toward the ground while the world loads. */}
      <motion.div
        className="enter-land"
        initial={{ y: 70 }}
        animate={{ y: 0, transition: { duration: 1.6, ease: [0.2, 0.8, 0.2, 1] } }}
      >
        <Landscape seed={id} unit={PX} cols={170} ground={[7, 10]} sky={8} treesFrom={0} treeChance={0.35} far={[13, 18]} near={[10, 14]} biome={biome} soilDepth={3} />
      </motion.div>
      <SceneCover id={id} full className="enter-cover" />

      <div className="enter-top" data-tauri-drag-region>
        <Button variant="hero" size="sm" onClick={onDismiss}>
          {t("在后台继续", "Continue in background")}
        </Button>
      </div>

      <motion.div
        className="enter-body"
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0, transition: { delay: 0.12, duration: 0.45, ease: [0.2, 0.8, 0.2, 1] } }}
      >
        <span className="enter-eyebrow">{ready ? t("世界已就绪", "World ready") : t("正在进入世界", "Entering the world")}</span>
        <span className="enter-name">{name}</span>
        {ready ? <span className="enter-ready">{t("祝你玩得开心", "Have fun")}</span> : <BlockLoader />}
      </motion.div>

      <div className="enter-tip">
        <span className="enter-tip-label">{t("小贴士", "Tip")}</span>
        {tip}
      </div>
    </motion.div>
  );
}

/** Blocks lighting up left to right; it runs only while a game is starting. */
function BlockLoader() {
  return (
    <span className="enter-loader" aria-hidden>
      {Array.from({ length: 12 }, (_, i) => (
        <i key={i} style={{ animationDelay: `${i * 90}ms` }} />
      ))}
    </span>
  );
}
