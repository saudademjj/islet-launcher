import type { ContentKind } from "../api/types";
import { t } from "./i18n";

// What the download page can narrow results by: Modrinth's own tags, named
// the way Chinese players call them. Each group allows one choice; choices in
// different groups must all match.

export type SearchSort = "relevance" | "downloads" | "updated" | "newest";

export const SORTS: { value: SearchSort; label: string }[] = [
  { value: "relevance", label: t("相关", "Relevance") },
  { value: "downloads", label: t("下载最多", "Downloads") },
  { value: "updated", label: t("最近更新", "Updated") },
  { value: "newest", label: t("最新发布", "Newest") },
];

export interface FilterGroup {
  id: string;
  label: string;
  options: { value: string; label: string }[];
}

const option = (value: string, label: string) => ({ value, label });

export const FILTER_GROUPS: Record<ContentKind, FilterGroup[]> = {
  mod: [
    {
      id: "category",
      label: t("类型", "Type"),
      options: [
        option("optimization", t("优化", "Optimization")),
        option("technology", t("科技", "Technology")),
        option("magic", t("魔法", "Magic")),
        option("adventure", t("冒险", "Adventure")),
        option("decoration", t("装饰", "Decoration")),
        option("utility", t("实用", "Utility")),
        option("worldgen", t("世界生成", "World gen")),
        option("mobs", t("生物", "Mobs")),
        option("equipment", t("装备", "Equipment")),
        option("storage", t("存储", "Storage")),
        option("transportation", t("交通", "Transportation")),
        option("food", t("食物", "Food")),
        option("game-mechanics", t("游戏机制", "Game mechanics")),
        option("management", t("管理", "Management")),
        option("economy", t("经济", "Economy")),
        option("social", t("社交", "Social")),
        option("minigame", t("小游戏", "Minigame")),
        option("library", t("前置库", "Library")),
      ],
    },
  ],
  resourcepack: [
    {
      id: "category",
      label: t("类型", "Type"),
      options: [
        option("vanilla-like", t("原版风格", "Vanilla-like")),
        option("realistic", t("写实", "Realistic")),
        option("simplistic", t("简约", "Simplistic")),
        option("themed", t("主题", "Themed")),
        option("tweaks", t("微调", "Tweaks")),
        option("blocks", t("方块", "Blocks")),
        option("items", t("物品", "Items")),
        option("entities", t("实体", "Entities")),
        option("gui", t("界面", "GUI")),
        option("fonts", t("字体", "Fonts")),
        option("audio", t("音效", "Audio")),
        option("models", t("模型", "Models")),
        option("environment", t("环境", "Environment")),
        option("combat", t("战斗", "Combat")),
        option("modded", t("模组适配", "Modded")),
        option("locale", t("语言", "Language")),
      ],
    },
    {
      id: "resolution",
      label: t("分辨率", "Resolution"),
      options: [
        option("16x", "16x"),
        option("32x", "32x"),
        option("64x", "64x"),
        option("128x", "128x"),
        option("256x", "256x"),
        option("512x+", t("512x 以上", "512x+")),
        option("8x-", t("8x 以下", "8x or less")),
      ],
    },
  ],
  shader: [
    {
      id: "loader",
      label: t("加载器", "Loader"),
      options: [option("iris", "Iris"), option("optifine", "OptiFine"), option("canvas", "Canvas"), option("vanilla", t("原版核心光影", "Vanilla core shaders"))],
    },
    {
      id: "style",
      label: t("风格", "Style"),
      options: [
        option("realistic", t("写实", "Realistic")),
        option("semi-realistic", t("半写实", "Semi-realistic")),
        option("vanilla-like", t("原版风格", "Vanilla-like")),
        option("cartoon", t("卡通", "Cartoon")),
        option("fantasy", t("奇幻", "Fantasy")),
      ],
    },
    {
      id: "performance",
      label: t("配置", "Performance"),
      options: [option("potato", t("土豆机", "Potato")), option("low", t("低配", "Low")), option("medium", t("中配", "Medium")), option("high", t("高配", "High")), option("screenshot", t("截图级", "Screenshot"))],
    },
  ],
  datapack: [
    {
      id: "category",
      label: t("类型", "Type"),
      options: [
        option("worldgen", t("世界生成", "World generation")),
        option("adventure", t("冒险", "Adventure")),
        option("game-mechanics", t("游戏机制", "Game mechanics")),
        option("utility", t("实用", "Utility")),
        option("decoration", t("装饰", "Decoration")),
        option("magic", t("魔法", "Magic")),
        option("mobs", t("生物", "Mobs")),
        option("equipment", t("装备", "Equipment")),
      ],
    },
  ],
};

/** Chosen values by group id. */
export type FilterChoice = Record<string, string>;

/** The chosen tags, in group order, for the search request. */
export function filterCategories(kind: ContentKind, choice: FilterChoice): string[] {
  return FILTER_GROUPS[kind].flatMap((group) => {
    const value = choice[group.id];
    return value && group.options.some((o) => o.value === value) ? [value] : [];
  });
}

/** Modpack filters. The values are Modrinth's tag names; the backend maps
 *  them to CurseForge's numbers. Some exist on one side only. */
export type ModpackSource = "modrinth" | "curseforge";
interface ModpackGroup extends FilterGroup {
  options: { value: string; label: string; only?: ModpackSource }[];
}
export const MODPACK_GROUPS: ModpackGroup[] = [
  {
    id: "loader",
    label: t("加载器", "Loader"),
    options: [option("forge", "Forge"), option("neoforge", "NeoForge"), option("fabric", "Fabric"), option("quilt", "Quilt")],
  },
  {
    id: "category",
    label: t("类型", "Type"),
    options: [
      option("adventure", t("冒险", "Adventure")),
      option("technology", t("科技", "Technology")),
      option("magic", t("魔法", "Magic")),
      option("quests", t("任务", "Quests")),
      option("kitchen-sink", t("大型综合", "Kitchen sink")),
      option("lightweight", t("轻量", "Lightweight")),
      option("multiplayer", t("多人", "Multiplayer")),
      option("combat", t("战斗", "Combat")),
      option("challenging", t("硬核", "Challenging")),
      { ...option("optimization", t("优化", "Optimization")), only: "modrinth" },
      { ...option("skyblock", t("空岛", "Skyblock")), only: "curseforge" },
    ],
  },
];

/** The filter choices that still make sense on `source`. */
export function modpackChoice(source: ModpackSource, choice: FilterChoice): FilterChoice {
  return Object.fromEntries(
    Object.entries(choice).filter(([id, value]) => MODPACK_GROUPS.find((g) => g.id === id)?.options.some((o) => o.value === value && (!o.only || o.only === source))),
  );
}
