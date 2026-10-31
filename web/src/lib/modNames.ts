// Chinese names for the mods people actually search for. A project only goes
// in here when its Modrinth slug is known, so lookups never point at a
// similarly named mod. Aliases are extra search terms, not translations.

export interface ModName {
  /** Modrinth project slug. */
  slug: string;
  en: string;
  zh: string;
  aliases?: string[];
}

export const MOD_NAMES: ModName[] = [
  // Performance and rendering
  { slug: "sodium", en: "Sodium", zh: "钠", aliases: ["钠优化"] },
  { slug: "lithium", en: "Lithium", zh: "锂", aliases: ["锂优化"] },
  { slug: "iris", en: "Iris Shaders", zh: "鸢尾花光影", aliases: ["鸢尾花", "光影"] },
  { slug: "ferrite-core", en: "FerriteCore", zh: "铁氧体核心", aliases: ["铁氧体"] },
  { slug: "modernfix", en: "ModernFix", zh: "现代修复", aliases: ["现代化修复"] },
  { slug: "entityculling", en: "EntityCulling", zh: "实体剔除", aliases: ["实体渲染剔除"] },
  { slug: "immediatelyfast", en: "ImmediatelyFast", zh: "即时渲染优化", aliases: ["即时渲染"] },
  { slug: "starlight", en: "Starlight", zh: "星光", aliases: ["星光光照"] },
  { slug: "krypton", en: "Krypton", zh: "氪", aliases: ["氪网络"] },
  { slug: "c2me-fabric", en: "C2ME", zh: "C2ME 区块优化", aliases: ["C2ME", "并发区块"] },
  { slug: "servercore", en: "ServerCore", zh: "服务器核心优化", aliases: ["服务器优化"] },
  { slug: "spark", en: "spark", zh: "火花分析", aliases: ["性能分析", "Spark 分析"] },
  { slug: "embeddium", en: "Embeddium", zh: "Embeddium 渲染优化", aliases: ["铷优化"] },
  { slug: "rubidium", en: "Rubidium", zh: "铷", aliases: ["Rubidium 优化"] },
  { slug: "canary", en: "Canary", zh: "金丝雀优化" },
  { slug: "saturn", en: "Saturn", zh: "土星", aliases: ["内存优化"] },
  { slug: "memoryleakfix", en: "MemoryLeakFix", zh: "内存泄漏修复", aliases: ["内存修复"] },
  { slug: "fastload", en: "FastLoad", zh: "快速加载" },
  { slug: "fastquit", en: "FastQuit", zh: "快速退出" },
  { slug: "noisium", en: "Noisium", zh: "噪声优化" },
  { slug: "badoptimizations", en: "BadOptimizations", zh: "优化修复" },
  { slug: "exordium", en: "Exordium", zh: "界面渲染优化", aliases: ["界面降帧"] },
  { slug: "moreculling", en: "MoreCulling", zh: "更多剔除" },
  { slug: "dynamic-fps", en: "Dynamic FPS", zh: "动态帧率" },
  { slug: "sodium-extra", en: "Sodium Extra", zh: "钠扩展" },
  { slug: "indium", en: "Indium", zh: "铟" },
  { slug: "continuity", en: "Continuity", zh: "连接纹理", aliases: ["连续材质"] },
  { slug: "nvidium", en: "Nvidium", zh: "Nvidium 渲染", aliases: ["英伟达优化"] },
  { slug: "voxy", en: "Voxy", zh: "体素远景" },
  { slug: "distanthorizons", en: "Distant Horizons", zh: "远处的地平线", aliases: ["远景", "DH"] },
  { slug: "bobby", en: "Bobby", zh: "波比", aliases: ["缓存区块"] },
  { slug: "debugify", en: "Debugify", zh: "调试修复" },
  { slug: "carpet", en: "Carpet", zh: "地毯" },
  { slug: "clumps", en: "Clumps", zh: "经验球合并", aliases: ["经验合并"] },

  // Inventory, recipes and HUD
  { slug: "jei", en: "Just Enough Items", zh: "JEI 物品管理器", aliases: ["JEI", "合成表"] },
  { slug: "rei", en: "Roughly Enough Items", zh: "REI 物品管理器", aliases: ["REI", "配方查询"] },
  { slug: "emi", en: "EMI", zh: "EMI 物品管理器", aliases: ["EMI", "配方管理器"] },
  { slug: "jade", en: "Jade", zh: "Jade 信息显示", aliases: ["玉", "信息提示"] },
  { slug: "journeymap", en: "JourneyMap", zh: "旅行地图", aliases: ["旅行小地图"] },
  { slug: "xaeros-minimap", en: "Xaero's Minimap", zh: "Xaero 的小地图", aliases: ["小地图"] },
  { slug: "xaeros-world-map", en: "Xaero's World Map", zh: "Xaero 的世界地图", aliases: ["世界地图"] },
  { slug: "appleskin", en: "AppleSkin", zh: "苹果皮", aliases: ["饱食度显示"] },
  { slug: "modmenu", en: "Mod Menu", zh: "模组菜单", aliases: ["模组列表"] },
  { slug: "wthit", en: "WTHIT", zh: "方块信息显示", aliases: ["WTHIT", "这是什么"] },
  { slug: "just-enough-resources-jer", en: "Just Enough Resources", zh: "JER 资源信息", aliases: ["JER", "资源查询"] },
  { slug: "inventory-profiles-next", en: "Inventory Profiles Next", zh: "物品栏配置 Next", aliases: ["背包整理", "IPN"] },
  { slug: "better-advancements", en: "Better Advancements", zh: "更好的进度", aliases: ["进度优化"] },
  { slug: "advancementinfo", en: "AdvancementInfo", zh: "进度信息", aliases: ["成就信息"] },
  { slug: "toast-control", en: "Toast Control", zh: "提示控制", aliases: ["吐司控制"] },
  { slug: "mouse-tweaks", en: "Mouse Tweaks", zh: "鼠标手势", aliases: ["鼠标调整"] },
  { slug: "betterf3", en: "BetterF3", zh: "更好的 F3", aliases: ["F3 面板"] },
  { slug: "shulkerboxtooltip", en: "ShulkerBoxTooltip", zh: "潜影盒提示", aliases: ["潜影盒工具提示"] },
  { slug: "enchantment-descriptions", en: "Enchantment Descriptions", zh: "附魔描述" },
  { slug: "tooltipfix", en: "TooltipFix", zh: "提示框修复" },
  { slug: "legendary-tooltips", en: "Legendary Tooltips", zh: "传奇提示框" },
  { slug: "polymorph", en: "Polymorph", zh: "多态合成", aliases: ["配方冲突解决"] },
  { slug: "crafting-tweaks", en: "Crafting Tweaks", zh: "合成调整", aliases: ["合成辅助"] },
  { slug: "minihud", en: "MiniHUD", zh: "迷你信息面板" },
  { slug: "ping-wheel", en: "Ping Wheel", zh: "标记轮盘" },
  { slug: "better-ping-display", en: "Better Ping Display", zh: "更好的延迟显示" },
  { slug: "status-effect-bars", en: "Status Effect Bars", zh: "状态效果条" },

  // Technology and automation
  { slug: "create", en: "Create", zh: "机械动力", aliases: ["机械", "Create 模组"] },
  { slug: "ae2", en: "Applied Energistics 2", zh: "应用能源2", aliases: ["AE2", "应用能源"] },
  { slug: "mekanism", en: "Mekanism", zh: "通用机械" },
  { slug: "thermal-expansion", en: "Thermal Expansion", zh: "热力膨胀", aliases: ["热力", "TE"] },
  { slug: "thermal-foundation", en: "Thermal Foundation", zh: "热力基础", aliases: ["热力前置"] },
  { slug: "thermal-dynamics", en: "Thermal Dynamics", zh: "热力动力学", aliases: ["热动力学"] },
  { slug: "thermal-cultivation", en: "Thermal Cultivation", zh: "热力农业" },
  { slug: "thermal-integration", en: "Thermal Integration", zh: "热力集成" },
  { slug: "thermal-locomotion", en: "Thermal Locomotion", zh: "热力运输" },
  { slug: "immersiveengineering", en: "Immersive Engineering", zh: "沉浸工程", aliases: ["沉浸", "IE"] },
  { slug: "refined-storage", en: "Refined Storage", zh: "精致存储", aliases: ["RS", "精制存储"] },
  { slug: "createaddition", en: "Create Crafts & Additions", zh: "机械动力：工艺与附加", aliases: ["工艺与附加", "CCA"] },
  { slug: "create-connected", en: "Create: Connected", zh: "机械动力：连接", aliases: ["Create Connected"] },
  { slug: "copycats", en: "Create: Copycats+", zh: "机械动力：复制猫", aliases: ["Copycats", "复制猫"] },
  { slug: "create-steam-n-rails", en: "Create: Steam 'n' Rails", zh: "机械动力：蒸汽与铁路", aliases: ["蒸汽铁路", "Create Steam n Rails"] },
  { slug: "create-track-map", en: "Create: Track Map", zh: "机械动力：列车地图", aliases: ["列车地图"] },
  { slug: "create-big-cannons", en: "Create: Big Cannons", zh: "机械动力：大型火炮", aliases: ["大型火炮", "CBC"] },
  { slug: "create-ore-excavation", en: "Create: Ore Excavation", zh: "机械动力：矿石开掘", aliases: ["矿石开掘"] },
  { slug: "create-diesel-generators", en: "Create: Diesel Generators", zh: "机械动力：柴油发电机", aliases: ["柴油发电机"] },
  { slug: "create-central-kitchen", en: "Create: Central Kitchen", zh: "机械动力：中央厨房", aliases: ["中央厨房"] },
  { slug: "create-garnished", en: "Create: Garnished", zh: "机械动力：佳肴", aliases: ["佳肴"] },
  { slug: "create-alloyed", en: "Create: Alloyed", zh: "机械动力：合金", aliases: ["合金"] },
  { slug: "create-encased", en: "Create: Encased", zh: "机械动力：封装", aliases: ["封装"] },
  { slug: "create-enchantment-industry", en: "Create: Enchantment Industry", zh: "机械动力：附魔工业", aliases: ["附魔工业"] },
  { slug: "create-jetpack", en: "Create: Jetpack", zh: "机械动力：喷气背包", aliases: ["喷气背包"] },
  { slug: "modern-industrialization", en: "Modern Industrialization", zh: "现代工业化" },
  { slug: "industrial-foregoing", en: "Industrial Foregoing", zh: "工业先锋" },
  { slug: "powah", en: "Powah!", zh: "Powah 能源", aliases: ["能量", "Powah"] },
  { slug: "extreme-reactors", en: "Extreme Reactors", zh: "极限反应堆" },
  { slug: "draconic-evolution", en: "Draconic Evolution", zh: "龙之进化" },
  { slug: "actually-additions", en: "Actually Additions", zh: "实用扩展" },
  { slug: "xnet", en: "XNet", zh: "XNet 网络" },
  { slug: "pneumaticcraft-repressurized", en: "PneumaticCraft: Repressurized", zh: "气动工艺：重制版", aliases: ["气动工艺"] },
  { slug: "immersivepetroleum", en: "Immersive Petroleum", zh: "沉浸石油" },
  { slug: "mystical-agriculture", en: "Mystical Agriculture", zh: "神秘农业" },
  { slug: "productivebees", en: "Productive Bees", zh: "资源蜜蜂", aliases: ["生产蜜蜂"] },
  { slug: "applied-mekanistics", en: "Applied Mekanistics", zh: "应用机械学", aliases: ["AE2 通用机械"] },
  { slug: "mekanism-generators", en: "Mekanism Generators", zh: "通用机械：发电机" },
  { slug: "mekanism-tools", en: "Mekanism Tools", zh: "通用机械：工具" },
  { slug: "mekanism-additions", en: "Mekanism Additions", zh: "通用机械：附加" },

  // Magic
  { slug: "botania", en: "Botania", zh: "植物魔法" },
  { slug: "ars-nouveau", en: "Ars Nouveau", zh: "新生魔艺", aliases: ["新生魔法", "Ars"] },
  { slug: "irons-spells-n-spellbooks", en: "Iron's Spells 'n Spellbooks", zh: "铁魔法", aliases: ["铁之法术", "Iron's Spells"] },
  { slug: "occultism", en: "Occultism", zh: "神秘学" },
  { slug: "hex-casting", en: "Hex Casting", zh: "咒法", aliases: ["Hex"] },
  { slug: "blood-magic", en: "Blood Magic", zh: "血魔法" },
  { slug: "malum", en: "Malum", zh: "邪灵魔法" },
  { slug: "goety", en: "Goety", zh: "死灵巫术" },
  { slug: "mahou-tsukai", en: "Mahou Tsukai", zh: "魔法使" },
  { slug: "mana-and-artifice", en: "Mana and Artifice", zh: "法力与魔法" },
  { slug: "forbidden-arcanus", en: "Forbidden Arcanus", zh: "禁忌秘术" },
  { slug: "eidolonrepraised", en: "Eidolon Repraised", zh: "邪术重生" },
  { slug: "bewitchment", en: "Bewitchment", zh: "巫术" },
  { slug: "apotheosis", en: "Apotheosis", zh: "神化" },
  { slug: "simply-swords", en: "Simply Swords", zh: "简易剑" },
  { slug: "spell-engine", en: "Spell Engine", zh: "法术引擎" },
  { slug: "elemental-craft", en: "Elemental Craft", zh: "元素工艺" },
  { slug: "natures-aura", en: "Nature's Aura", zh: "自然灵气" },
  { slug: "mystical-agradditions", en: "Mystical Agradditions", zh: "神秘农业扩展" },
  { slug: "spectrum", en: "Spectrum", zh: "光谱" },

  // Adventure, mobs and world generation
  { slug: "biomes-o-plenty", en: "Biomes O' Plenty", zh: "超多生物群系", aliases: ["BOP", "生物群系"] },
  { slug: "terralith", en: "Terralith", zh: "Terralith 地形生成", aliases: ["地形生成"] },
  { slug: "tectonic", en: "Tectonic", zh: "构造地形" },
  { slug: "alexs-mobs", en: "Alex's Mobs", zh: "Alex 的生物", aliases: ["Alex 生物"] },
  { slug: "aether", en: "The Aether", zh: "天境", aliases: ["以太", "天堂"] },
  { slug: "deeper-and-darker", en: "Deeper and Darker", zh: "更深与更暗", aliases: ["深暗"] },
  { slug: "the-undergarden", en: "The Undergarden", zh: "地下花园" },
  { slug: "the-bumblezone", en: "The Bumblezone", zh: "蜜蜂领域", aliases: ["蜂巢世界"] },
  { slug: "yungs-api", en: "YUNG's API", zh: "YUNG 的前置库", aliases: ["YUNG 前置"] },
  { slug: "yungs-better-mineshafts", en: "YUNG's Better Mineshafts", zh: "YUNG 的更好的矿井", aliases: ["更好的矿井"] },
  { slug: "yungs-better-dungeons", en: "YUNG's Better Dungeons", zh: "YUNG 的更好的地牢", aliases: ["更好的地牢"] },
  { slug: "yungs-better-desert-temples", en: "YUNG's Better Desert Temples", zh: "YUNG 的更好的沙漠神殿", aliases: ["更好的沙漠神殿"] },
  { slug: "yungs-better-strongholds", en: "YUNG's Better Strongholds", zh: "YUNG 的更好的要塞", aliases: ["更好的要塞"] },
  { slug: "yungs-better-witch-huts", en: "YUNG's Better Witch Huts", zh: "YUNG 的更好的女巫小屋", aliases: ["更好的女巫小屋"] },
  { slug: "yungs-better-ocean-monuments", en: "YUNG's Better Ocean Monuments", zh: "YUNG 的更好的海底神殿", aliases: ["更好的海底神殿"] },
  { slug: "yungs-better-nether-fortresses", en: "YUNG's Better Nether Fortresses", zh: "YUNG 的更好的下界要塞", aliases: ["更好的下界要塞"] },
  { slug: "yungs-better-end-island", en: "YUNG's Better End Island", zh: "YUNG 的更好的末地岛", aliases: ["更好的末地岛"] },
  { slug: "yungs-better-jungle-temples", en: "YUNG's Better Jungle Temples", zh: "YUNG 的更好的丛林神庙", aliases: ["更好的丛林神庙"] },
  { slug: "yungs-cave-biomes", en: "YUNG's Cave Biomes", zh: "YUNG 的洞穴生物群系", aliases: ["洞穴生物群系"] },
  { slug: "yungs-bridges", en: "YUNG's Bridges", zh: "YUNG 的桥梁", aliases: ["自然桥梁"] },
  { slug: "when-dungeons-arise", en: "When Dungeons Arise", zh: "地牢浮现之时", aliases: ["地牢浮现"] },
  { slug: "dungeons-and-taverns", en: "Dungeons and Taverns", zh: "地牢与酒馆" },
  { slug: "towns-and-towers", en: "Towns and Towers", zh: "城镇与高塔" },
  { slug: "structory", en: "Structory", zh: "Structory 建筑" },
  { slug: "nullscape", en: "Nullscape", zh: "虚空之境" },
  { slug: "betterend", en: "Better End", zh: "更好的末地" },
  { slug: "betternether", en: "Better Nether", zh: "更好的下界" },
  { slug: "cinderscapes", en: "Cinderscapes", zh: "烬境" },
  { slug: "incendium", en: "Incendium", zh: "Incendium 下界扩展", aliases: ["下界扩展"] },
  { slug: "amplified-nether", en: "Amplified Nether", zh: "放大下界" },
  { slug: "alexs-caves", en: "Alex's Caves", zh: "Alex 的洞穴" },
  { slug: "mowzies-mobs", en: "Mowzie's Mobs", zh: "Mowzie 的生物" },
  { slug: "l_enders-cataclysm", en: "L_Ender's Cataclysm", zh: "灾变", aliases: ["Cataclysm"] },
  { slug: "bosses-of-mass-destruction", en: "Bosses of Mass Destruction", zh: "大规模毁灭首领", aliases: ["大型首领"] },
  { slug: "aquamirae", en: "Aquamirae", zh: "深海之谜" },
  { slug: "ice-and-fire-dragons", en: "Ice and Fire", zh: "冰与火之歌", aliases: ["冰火传说", "冰与火"] },
  { slug: "naturalist", en: "Naturalist", zh: "自然主义者" },
  { slug: "environmental", en: "Environmental", zh: "环境生物" },
  { slug: "regions-unexplored", en: "Regions Unexplored", zh: "未探索地区" },
  { slug: "biomesyougo", en: "Oh The Biomes You'll Go", zh: "你将去的生物群系", aliases: ["BYG"] },
  { slug: "geophilic", en: "Geophilic", zh: "Geophilic 地形增强", aliases: ["地形增强"] },
  { slug: "eternal-starlight", en: "Eternal Starlight", zh: "永恒星光" },
  { slug: "the-outer-end", en: "The Outer End", zh: "外末地" },
  { slug: "enderscape", en: "Enderscape", zh: "末境" },

  // Building, storage and quality of life
  { slug: "farmers-delight", en: "Farmer's Delight", zh: "农夫乐事", aliases: ["农夫"] },
  { slug: "storagedrawers", en: "Storage Drawers", zh: "储物抽屉" },
  { slug: "sophisticated-backpacks", en: "Sophisticated Backpacks", zh: "精妙背包", aliases: ["精致背包"] },
  { slug: "sophisticated-storage", en: "Sophisticated Storage", zh: "精妙存储" },
  { slug: "waystones", en: "Waystones", zh: "传送石碑", aliases: ["传送石"] },
  { slug: "supplementaries", en: "Supplementaries", zh: "装饰补充", aliases: ["补充装饰"] },
  { slug: "comforts", en: "Comforts", zh: "舒适睡眠", aliases: ["睡袋"] },
  { slug: "carry-on", en: "Carry On", zh: "搬箱", aliases: ["搬运"] },
  { slug: "travelersbackpack", en: "Traveler's Backpack", zh: "旅行者背包" },
  { slug: "small-ships", en: "Small Ships", zh: "小船" },
  { slug: "construction-wand", en: "Construction Wand", zh: "建筑魔杖" },
  { slug: "handcrafted", en: "Handcrafted", zh: "手工制作" },
  { slug: "another-furniture", en: "Another Furniture", zh: "另一种家具" },
  { slug: "macaws-furniture", en: "Macaw's Furniture", zh: "Macaw 的家具" },
  { slug: "chipped", en: "Chipped", zh: "方块凿刻" },
  { slug: "rechiseled", en: "Rechiseled", zh: "重凿" },
  { slug: "framedblocks", en: "FramedBlocks", zh: "框架方块" },
  { slug: "farmers-respite", en: "Farmer's Respite", zh: "农夫休憩" },
  { slug: "pams-harvestcraft", en: "Pam's HarvestCraft", zh: "潘马斯农场" },
  { slug: "serene-seasons", en: "Serene Seasons", zh: "静谧四季" },
  { slug: "tough-as-nails", en: "Tough As Nails", zh: "严酷环境" },
  { slug: "thirst-was-taken", en: "Thirst Was Taken", zh: "口渴机制", aliases: ["口渴"] },
  { slug: "diet", en: "Diet", zh: "饮食" },
  { slug: "firstaid", en: "First Aid", zh: "急救" },
  { slug: "corpse", en: "Corpse", zh: "尸体", aliases: ["死亡遗体"] },
  { slug: "yigd", en: "You're in Grave Danger", zh: "死亡墓碑", aliases: ["YIGD"] },
  { slug: "corail-tombstone", en: "Corail Tombstone", zh: "科拉尔墓碑", aliases: ["墓石"] },
  { slug: "simple-voice-chat", en: "Simple Voice Chat", zh: "简单语音聊天", aliases: ["语音聊天"] },
  { slug: "plasmo-voice", en: "Plasmo Voice", zh: "Plasmo 语音" },
  { slug: "emotecraft", en: "Emotecraft", zh: "表情动作" },
  { slug: "falling-leaves", en: "Falling Leaves", zh: "落叶" },
  { slug: "effective", en: "Effective", zh: "效果" },
  { slug: "wilder-wild", en: "Wilder Wild", zh: "狂野自然" },
  { slug: "visuality", en: "Visuality", zh: "视觉" },
  { slug: "particle-rain", en: "Particle Rain", zh: "粒子雨" },
  { slug: "immersive-melodies", en: "Immersive Melodies", zh: "沉浸旋律" },
  { slug: "ambientsounds", en: "Ambient Sounds", zh: "环境音效" },
  { slug: "presence-footsteps", en: "Presence Footsteps", zh: "脚步声" },
  { slug: "sound-physics-remastered", en: "Sound Physics Remastered", zh: "声音物理重制版" },
  { slug: "not-enough-animations", en: "Not Enough Animations", zh: "更多动画" },
  { slug: "first-person-model", en: "First Person Model", zh: "第一人称模型" },
  { slug: "shoulder-surfing-reloaded", en: "Shoulder Surfing Reloaded", zh: "越肩视角重制版" },
  { slug: "cameraoverhaul", en: "Camera Overhaul", zh: "摄像机大修" },
  { slug: "do-a-barrel-roll", en: "Do a Barrel Roll", zh: "翻滚", aliases: ["桶滚"] },
  { slug: "zoomify", en: "Zoomify", zh: "缩放" },
  { slug: "controlling", en: "Controlling", zh: "按键控制" },
  { slug: "betterdays", en: "BetterDays", zh: "更好的天数" },
  { slug: "eating-animation", en: "Eating Animation", zh: "进食动画" },
  { slug: "travelers-titles", en: "Traveler's Titles", zh: "旅行者称号" },
  { slug: "quark", en: "Quark", zh: "夸克" },
  { slug: "charm", en: "Charm", zh: "Charm 内容扩展" },
  { slug: "lets-do-vinery", en: "Vinery", zh: "葡萄园" },

  // Shared libraries
  { slug: "fabric-api", en: "Fabric API", zh: "Fabric 基础前置", aliases: ["Fabric API", "Fabric 前置", "FabricAPI"] },
  { slug: "architectury-api", en: "Architectury API", zh: "Architectury 跨平台前置", aliases: ["Architectury API", "Architectury", "跨平台前置"] },
  { slug: "cloth-config", en: "Cloth Config API", zh: "Cloth Config 配置库", aliases: ["Cloth Config", "Cloth Config API", "布质配置"] },
  { slug: "geckolib", en: "GeckoLib", zh: "GeckoLib 动画前置", aliases: ["GeckoLib", "壁虎库"] },
  { slug: "kotlin-for-forge", en: "Kotlin for Forge", zh: "Kotlin for Forge 前置", aliases: ["Kotlin for Forge", "Kotlin 前置"] },
  { slug: "forge-config-api-port", en: "Forge Config API Port", zh: "Forge 配置 API 移植" },
  { slug: "balm", en: "Balm", zh: "Balm 前置" },
  { slug: "bookshelf-lib", en: "Bookshelf", zh: "Bookshelf 前置" },
  { slug: "puzzles-lib", en: "Puzzles Lib", zh: "Puzzles Lib 前置" },
  { slug: "collective", en: "Collective", zh: "Collective 前置" },
  { slug: "cucumber", en: "Cucumber", zh: "Cucumber 前置" },
  { slug: "citadel", en: "Citadel", zh: "Citadel 前置" },
  { slug: "curios", en: "Curios API", zh: "Curios 饰品栏", aliases: ["饰品栏"] },
  { slug: "patchouli", en: "Patchouli", zh: "帕秋莉手册", aliases: ["指南书"] },
  { slug: "playeranimator", en: "Player Animator", zh: "玩家动画前置" },
  { slug: "midnightlib", en: "MidnightLib", zh: "MidnightLib 前置" },
  { slug: "owo-lib", en: "owo-lib", zh: "owo-lib 前置" },
  { slug: "cardinal-components-api", en: "Cardinal Components API", zh: "Cardinal 组件 API", aliases: ["Cardinal Components API"] },
  { slug: "polymer", en: "Polymer", zh: "Polymer 前置" },
  { slug: "trinkets", en: "Trinkets", zh: "Trinkets 饰品栏", aliases: ["饰品栏"] },
  { slug: "yacl", en: "Yet Another Config Lib", zh: "YACL 配置库", aliases: ["YACL"] },
  { slug: "flywheel", en: "Flywheel", zh: "Flywheel 渲染前置" },
  { slug: "terrablender", en: "TerraBlender", zh: "TerraBlender 前置" },
  { slug: "resourceful-lib", en: "Resourceful Lib", zh: "Resourceful Lib 前置" },
  { slug: "moonlight", en: "Moonlight Lib", zh: "Moonlight 前置" },
  { slug: "iceberg", en: "Iceberg", zh: "Iceberg 前置" },
  { slug: "prickle", en: "Prickle", zh: "Prickle 前置" },
  { slug: "smartbrainlib", en: "SmartBrainLib", zh: "SmartBrainLib 前置" },
  { slug: "placebo", en: "Placebo", zh: "Placebo 前置" },
  { slug: "fabric-language-kotlin", en: "Fabric Language Kotlin", zh: "Fabric Kotlin 前置" },
  { slug: "libipn", en: "LibIPN", zh: "LibIPN 前置" },
  { slug: "lithostitched", en: "Lithostitched", zh: "Lithostitched 前置" },
  { slug: "konkrete", en: "Konkrete", zh: "Konkrete 前置" },
  { slug: "forgified-fabric-api", en: "Forgified Fabric API", zh: "Forgified Fabric 前置", aliases: ["Forgified Fabric API"] },
  { slug: "kubejs", en: "KubeJS", zh: "KubeJS 脚本" },
  { slug: "crafttweaker", en: "CraftTweaker", zh: "CraftTweaker 配方脚本" },
];

const CHINESE = /[㐀-䶿一-鿿豈-﫿]/;

export function hasChinese(text: string): boolean {
  return CHINESE.test(text);
}

/** Comparison key for the catalog: case and all whitespace are irrelevant. */
function key(value: string): string {
  return value.toLowerCase().replace(/\s+/g, "");
}

interface Hit {
  mod: ModName;
  exact: boolean;
  /** Length of the shorter side of the containment match. */
  length: number;
  order: number;
}

/** Chinese-only search over the display name and aliases; exact names win,
    then longer containment matches. English-only queries return no hits. */
export function findChineseMods(query: string): ModName[] {
  if (!hasChinese(query)) return [];
  const needle = key(query);
  if (!needle) return [];

  const hits: Hit[] = [];
  MOD_NAMES.forEach((mod, order) => {
    let best: Hit | undefined;
    for (const name of [mod.zh, ...(mod.aliases ?? [])]) {
      const candidate = key(name);
      if (!candidate) continue;
      const exact = candidate === needle;
      if (!exact && !candidate.includes(needle) && !needle.includes(candidate)) continue;
      const length = Math.min(candidate.length, needle.length);
      if (!best || (exact && !best.exact) || (exact === best.exact && length > best.length)) {
        best = { mod, exact, length, order };
      }
    }
    if (best) hits.push(best);
  });

  hits.sort((a, b) => Number(b.exact) - Number(a.exact) || b.length - a.length || a.order - b.order);
  return hits.map((hit) => hit.mod);
}

/** Look up a catalog entry by Modrinth slug first, then by a project title or
    one of its names. Matching uses the same case/space-insensitive key. */
export function findModName(slug?: string | null, title?: string | null): ModName | undefined {
  const lookup = (value: string): ModName | undefined => {
    const needle = key(value);
    if (!needle) return undefined;
    return MOD_NAMES.find((mod) => [mod.slug, mod.en, mod.zh, ...(mod.aliases ?? [])].some((name) => key(name) === needle));
  };

  return (slug ? lookup(slug) : undefined) ?? (title ? lookup(title) : undefined);
}

/** "sodium-fabric-0.6.5+mc1.21.4.jar" → "sodium": the words before the
    first version-like or loader part, which is usually the project slug. */
export function slugOfFile(file: string): string {
  const base = file.replace(/\.disabled$/i, "").replace(/\.jar$/i, "");
  const parts = base.split(/[-_ ]+/);
  const kept: string[] = [];
  for (const [i, part] of parts.entries()) {
    const p = part.toLowerCase();
    if (/^\d/.test(p) || p.startsWith("mc") || p.startsWith("v1") || (i > 0 && /^(fabric|forge|neoforge|quilt)$/.test(p))) break;
    kept.push(p);
  }
  return kept.join("-") || base.toLowerCase();
}
