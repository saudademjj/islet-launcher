import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { call } from "../api/backend";
import type { Instance, LocalMod, ServerProject, Snapshot } from "../api/types";
import { Button, CardHeader, Chip, Field, Identicon, Input, Segmented, Switch } from "../components/ui";
import { Toolbar } from "../layout/Shell";
import { loaderLabel } from "../lib/format";
import { t } from "../lib/i18n";
import { normal } from "../lib/motion";
import { MOD_KEY } from "../lib/platform";
import { MemoryField, memoryValid } from "../overlays/RemoteServer";
import { usePick, useStore } from "../store";
import { LeaveQuestion } from "./SettingsPage";

/** The kinds of server, grouped the way people ask for them. */
const KINDS: { group: string; items: { value: string; note: string }[] }[] = [
  { group: t("原版", "Vanilla"), items: [{ value: "vanilla", note: t("官方服务端，不加模组", "The official server, no mods") }] },
  {
    group: t("模组服", "Modded"),
    items: [
      { value: "fabric", note: t("轻量，新版本跟得快", "Light, quick to update") },
      { value: "quilt", note: t("兼容 Fabric 模组", "Runs Fabric mods too") },
      { value: "forge", note: t("老牌大型模组", "The classic big mods") },
      { value: "neoforge", note: t("新版 Forge 模组", "Forge mods, newer versions") },
    ],
  },
  {
    group: t("插件服", "Plugins"),
    items: [
      { value: "paper", note: t("性能好，插件最多", "Fast, the most plugins") },
      { value: "purpur", note: t("Paper 加更多可调玩法", "Paper with more to tweak") },
    ],
  },
];

const isPlugin = (loader: string) => loader === "paper" || loader === "purpur";

/** Release versions a kind of server can run: Paper and Purpur list
 *  their own, the rest follow Mojang's list. */
const pluginVersions = new Map<string, string[]>();
function useServerVersions(loader: string): { versions: string[]; loading: boolean } {
  const s = usePick((s) => ({ versions: s.versions, loaded: s.versionsLoaded, loadVersions: s.loadVersions }));
  const [own, setOwn] = useState<string[] | null>(() => pluginVersions.get(loader) ?? null);
  useEffect(() => {
    if (!isPlugin(loader)) {
      if (!s.loaded) s.loadVersions(false);
      return;
    }
    const known = pluginVersions.get(loader);
    setOwn(known ?? null);
    if (known) return;
    let live = true;
    call<string[]>("server_versions", { loader })
      .then((list) => {
        pluginVersions.set(loader, list);
        if (live) setOwn(list);
      })
      .catch(() => live && setOwn([]));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loader]);
  const mojang = useMemo(() => s.versions.filter((v) => v.versionType === "release").map((v) => v.id), [s.versions]);
  if (isPlugin(loader)) return { versions: own ?? [], loading: own === null };
  return { versions: mojang, loading: !s.loaded };
}

function KindPicker({ value, onChange }: { value: string; onChange: (loader: string) => void }) {
  return (
    <div className="server-kinds">
      {KINDS.map((group) => (
        <div key={group.group} className="col gap-6">
          <span className="field-label">{group.group}</span>
          <div className="server-kind-row">
            {group.items.map((kind) => (
              <button key={kind.value} type="button" className="quick-version" aria-pressed={value === kind.value} onClick={() => onChange(kind.value)}>
                <span className="quick-id">{loaderLabel(kind.value)}</span>
                <span className="quick-note">{kind.note}</span>
              </button>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

function VersionPicker({ loader, value, onChange }: { loader: string; value: string; onChange: (version: string) => void }) {
  const { versions, loading } = useServerVersions(loader);
  const [all, setAll] = useState(false);
  // The chosen version stays in view even when it is far down the list.
  const shown = all ? versions : versions.slice(0, 12);
  const list = value && versions.includes(value) && !shown.includes(value) ? [...shown, value] : shown;
  if (loading) return <span className="field-hint"><span className="spinner" />{t(" 正在获取版本列表…", " Loading versions…")}</span>;
  if (versions.length === 0) return <span className="field-hint bad">{t("版本列表加载失败，请检查网络后重试", "Couldn't load versions; check the network and try again")}</span>;
  return (
    <div className="col gap-8">
      <div className="server-versions">
        {list.map((version) => (
          <button key={version} type="button" className="server-version mono" aria-pressed={value === version} onClick={() => onChange(version)}>
            {version}
          </button>
        ))}
      </div>
      {!value || versions.includes(value) ? null : <span className="field-hint bad">{t(`${loaderLabel(loader)} 没有 ${value} 的服务端，换一个版本`, `${loaderLabel(loader)} has no server for ${value}; pick another`)}</span>}
      {versions.length > 12 && (
        <Button size="sm" variant="ghost" onClick={() => setAll(!all)}>
          {all ? t("收起", "Fewer") : t(`全部 ${versions.length} 个版本`, `All ${versions.length} versions`)}
        </Button>
      )}
    </div>
  );
}

export function ServerPage() {
  const s = usePick((s) => ({ projects: s.serverProjects, selected: s.serverProject, pageGuard: s.pageGuard }));
  const [creating, setCreating] = useState(false);
  const current = s.projects.find((p) => p.id === s.selected) ?? s.projects[0] ?? null;
  const select = (id: string) => {
    if (useStore.getState().pageGuard) return useStore.getState().toast(t("先保存或还原这台服务器的更改", "Save or revert this server's changes first"), "info");
    setCreating(false);
    useStore.setState({ serverProject: id });
  };
  const create = () => {
    if (useStore.getState().pageGuard) return useStore.getState().toast(t("先保存或还原这台服务器的更改", "Save or revert this server's changes first"), "info");
    setCreating(true);
  };
  const showCreate = creating || s.projects.length === 0;
  // The unsaved bar sits over the page, outside the scrolling content.
  const [bar, setBar] = useState<HTMLDivElement | null>(null);
  // Another server, or the new one, starts at the top.
  const scroller = useRef<HTMLDivElement>(null);
  useEffect(() => {
    scroller.current?.scrollTo({ top: 0 });
  }, [current?.id, showCreate]);

  return (
    <>
      <Toolbar title={t("开服", "Server")} meta={t("选好服务端和模组或插件，一键导出或部署到云服务器", "Pick a server, its mods or plugins, then export or deploy it in one go")}>
        <Button variant="primary" icon="plus" disabled={showCreate} onClick={create}>
          {t("新建服务器", "New server")}
        </Button>
      </Toolbar>
      <div className="content" ref={scroller}>
        <div className={`page server-layout${s.projects.length === 0 ? " single" : ""}`}>
          {s.projects.length > 0 && (
            <nav className="card server-list" aria-label={t("服务器", "Servers")}>
              {s.projects.map((project) => {
                const active = !showCreate && project.id === current?.id;
                return (
                  <button key={project.id} type="button" className={`list-row clickable${active ? " selected" : ""}`} aria-current={active ? "true" : undefined} onClick={() => select(project.id)}>
                    {active && <span className="selected-bar" />}
                    <Identicon seed={project.id} size={30} />
                    <span className="col" style={{ minWidth: 0, alignItems: "flex-start" }}>
                      <span className="server-list-name">{project.name}</span>
                      <span className="field-hint mono">
                        {loaderLabel(project.loader)} · {project.versionId}
                      </span>
                    </span>
                  </button>
                );
              })}
            </nav>
          )}
          <div className="col gap-16">
            {showCreate ? (
              <CreateServer
                first={s.projects.length === 0}
                onCancel={() => setCreating(false)}
                onCreated={(id) => {
                  setCreating(false);
                  useStore.setState({ serverProject: id });
                }}
              />
            ) : (
              current && <ProjectView key={current.id} project={current} bar={bar} />
            )}
          </div>
        </div>
      </div>
      <div ref={setBar} />
    </>
  );
}

function CreateServer({ first, onCancel, onCreated }: { first: boolean; onCancel: () => void; onCreated: (id: string) => void }) {
  const [name, setName] = useState(t("我的服务器", "My server"));
  const [loader, setLoader] = useState("paper");
  const [version, setVersion] = useState("");
  const { versions } = useServerVersions(loader);
  const [busy, setBusy] = useState(false);
  // The newest version of the chosen kind, until one is picked.
  useEffect(() => {
    if (versions.length && !versions.includes(version)) setVersion(versions[0]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [versions]);
  const ok = name.trim().length > 0 && versions.includes(version);
  return (
    <section className="card">
      <CardHeader icon="rack" title={first ? t("开一台自己的服务器", "Run your own server") : t("新建服务器", "New server")} />
      <div className="card-body col gap-20">
        {first && (
          <span className="field-hint">
            {t(
              "选一种服务端和游戏版本，再添加想要的模组或插件。完成后导出成文件夹，或者填上云服务器的 SSH 一键部署；内存、JVM 参数和视距会按那台机器的配置自动设好。",
              "Choose a kind of server and a game version, then add the mods or plugins you want. Export it as a folder or deploy it over SSH; memory, JVM flags and view distance fit the machine it runs on.",
            )}
          </span>
        )}
        <Field label={t("名称", "Name")}>
          <Input value={name} maxLength={64} onChange={(e) => setName(e.target.value)} />
        </Field>
        <Field label={t("服务端类型", "Kind of server")}>
          <KindPicker value={loader} onChange={setLoader} />
        </Field>
        <Field label={t("游戏版本", "Game version")}>
          <VersionPicker loader={loader} value={version} onChange={setVersion} />
        </Field>
        <div className="row gap-8 justify-end">
          {!first && (
            <Button variant="ghost" onClick={onCancel}>
              {t("取消", "Cancel")}
            </Button>
          )}
          <Button
            variant="primary"
            icon="check"
            busy={busy}
            disabled={!ok || busy}
            onClick={async () => {
              setBusy(true);
              const created = await useStore.getState().run<[Snapshot, string]>("create_server_project", { name: name.trim(), versionId: version, loader });
              setBusy(false);
              if (!created) return;
              useStore.getState().applySnapshot(created[0]);
              onCreated(created[1]);
            }}
          >
            {t("创建", "Create")}
          </Button>
        </div>
      </div>
    </section>
  );
}

type Form = Omit<ServerProject, "maxPlayers" | "port"> & { name: string; loader: string; versionId: string; maxPlayers: string; port: string; memory: string };

const toForm = (p: Instance): Form => {
  const project = p.server!;
  return { ...project, name: p.name, loader: p.loader, versionId: p.versionId, maxPlayers: String(project.maxPlayers), port: String(project.port), memory: String(project.memoryMb) };
};

const same = (a: Form, b: Form) => (Object.keys(a) as (keyof Form)[]).every((key) => a[key] === b[key]);

function ProjectView({ project, bar }: { project: Instance; bar: HTMLDivElement | null }) {
  const s = usePick((s) => ({ openModal: s.openModal, openManage: s.openManage, modalOpen: s.modal !== null, leaving: s.leavingPage }));
  const saved = useMemo(() => toForm(project), [project]);
  const [form, setForm] = useState(saved);
  const [mods, setMods] = useState<LocalMod[] | null>(null);
  const set = <K extends keyof Form>(key: K, value: Form[K]) => setForm((f) => ({ ...f, [key]: value }));
  const plugins = isPlugin(project.loader);
  const what = plugins ? t("插件", "Plugins") : t("模组", "Mods");

  // The list follows edits made in the manage dialog.
  useEffect(() => {
    if (s.modalOpen) return;
    let live = true;
    if (project.loader === "vanilla") setMods([]);
    else call<LocalMod[]>("list_mods", { id: project.id }).then((list) => live && setMods(list)).catch(() => live && setMods([]));
    return () => {
      live = false;
    };
  }, [project.id, project.loader, s.modalOpen]);

  const maxPlayers = Number(form.maxPlayers);
  const port = Number(form.port);
  const valid = form.name.trim().length > 0 && form.motd.length <= 120 && maxPlayers >= 1 && maxPlayers <= 1000 && port > 0 && port < 65536 && memoryValid(form.memory) && form.versionId.length > 0;
  const dirty = !same(form, saved);

  const save = async () => {
    if (!valid) {
      useStore.getState().toast(t("有设置没填对，先改一下", "Some settings aren't right yet"), "error");
      return false;
    }
    const next: ServerProject = {
      motd: form.motd,
      maxPlayers,
      difficulty: form.difficulty,
      gamemode: form.gamemode,
      pvp: form.pvp,
      whitelist: form.whitelist,
      onlineMode: form.onlineMode,
      port,
      memoryMb: Number(form.memory),
      seed: form.seed.trim(),
    };
    return useStore.getState().mutate("configure_server_project", { id: project.id, name: form.name.trim(), versionId: form.versionId, loader: form.loader, project: next }, t("服务器设置已保存", "Server settings saved"));
  };
  // A saved project comes back as new props; the form follows. Snapshots
  // for other changes bring an equal project and keep the edits.
  const savedKey = JSON.stringify(saved);
  useEffect(() => {
    setForm(saved);
  }, [savedKey]); // eslint-disable-line react-hooks/exhaustive-deps
  const saveLatest = useRef(save);
  saveLatest.current = save;
  useEffect(() => {
    useStore.getState().setPageGuard(dirty ? { save: () => saveLatest.current(), discard: () => setForm(saved) } : null);
  }, [dirty, saved]);
  useEffect(() => () => useStore.getState().setPageGuard(null), []);
  useEffect(() => {
    if (!dirty) return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void saveLatest.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dirty]);
  const changes = (Object.keys(saved) as (keyof Form)[]).filter((key) => form[key] !== saved[key]).length;
  const kindChanged = form.loader !== saved.loader && (mods?.length ?? 0) > 0;
  // Exports and deploys build what was saved.
  const blocked = dirty ? t("先保存更改", "Save your changes first") : undefined;

  return (
    <>
      <section className="card server-head">
        <Identicon seed={project.id} size={46} />
        <div className="col gap-2">
          <span className="server-head-name">{project.name}</span>
          <span className="field-hint mono">
            {loaderLabel(project.loader)}
            {project.loaderVersion ? ` ${project.loaderVersion}` : ""} · Minecraft {project.versionId}
          </span>
        </div>
        <div className="spacer" />
        <div className="row gap-8 wrap justify-end">
          <Button icon="folder" disabled={dirty} title={blocked} onClick={() => s.openModal({ kind: "exportServer", id: project.id })}>
            {t("导出到文件夹", "Export to folder")}
          </Button>
          <Button variant="primary" icon="server" disabled={dirty} title={blocked} onClick={() => s.openModal({ kind: "remote", id: project.id })}>
            {t("部署到云服务器", "Deploy to a server")}
          </Button>
          <Button variant="danger-ghost" icon="trash" title={t("删除", "Delete")} aria-label={t("删除", "Delete")} onClick={() => s.openModal({ kind: "deleteInstance", instance: project })} />
        </div>
      </section>

      {project.loader !== "vanilla" && (
        <section className="card">
          <CardHeader icon={plugins ? "pack" : "mod"} title={what} meta={mods ? mods.length : "…"}>
            <Button size="sm" onClick={() => s.openManage(project.id, "mods")}>
              {t("管理", "Manage")}
            </Button>
            <Button size="sm" variant="primary" icon="download" onClick={() => s.openManage(project.id, "browse")}>
              {plugins ? t("下载插件", "Get plugins") : t("下载模组", "Get mods")}
            </Button>
          </CardHeader>
          <div className="card-body">
            {mods && mods.length > 0 ? (
              <div className="row gap-6 wrap">
                {mods.slice(0, 24).map((mod) => (
                  <Chip key={mod.name}>{mod.name.replace(/\.jar(\.disabled)?$/, "")}</Chip>
                ))}
                {mods.length > 24 && <span className="field-hint">{t(`还有 ${mods.length - 24} 个`, `${mods.length - 24} more`)}</span>}
              </div>
            ) : (
              <span className="field-hint">
                {plugins
                  ? t("还没有插件。点「下载插件」从 Modrinth 搜，比如 EssentialsX、LuckPerms；也可以把 .jar 拖进管理窗口。", "No plugins yet. Get them from Modrinth, like EssentialsX or LuckPerms, or drop .jar files into the manage window.")
                  : t("还没有模组。点「下载模组」只会列出能在服务器上运行的模组；只在客户端用的会在导出时去掉。", "No mods yet. Get mods lists only ones that run on a server; client-only ones are left out when exporting.")}
              </span>
            )}
          </div>
        </section>
      )}
      {project.loader === "vanilla" && <span className="field-hint">{t("原版服务端不加载模组和插件；想装插件，把类型换成 Paper。", "A vanilla server runs no mods or plugins; switch to Paper for plugins.")}</span>}

      <section className="card">
        <CardHeader icon="sliders" title={t("服务器设置", "Server settings")} text={t("内存、JVM 参数和视距在开服时按机器配置自动调整", "Memory, JVM flags and view distance fit the machine at start")} />
        <div className="card-body col gap-20">
          <Field label={t("名称", "Name")} tone={form.name.trim() ? undefined : "bad"}>
            <Input value={form.name} maxLength={64} invalid={!form.name.trim()} onChange={(e) => set("name", e.target.value)} />
          </Field>
          <Field label={t("服务端类型", "Kind of server")} tone={kindChanged ? "bad" : undefined} hint={kindChanged ? t(`换类型后，已添加的${what}可能用不了，保存后去「管理」里检查`, `Changing the kind may leave the ${what.toLowerCase()} you added unusable; check them under Manage after saving`) : undefined}>
            <KindPicker value={form.loader} onChange={(loader) => set("loader", loader)} />
          </Field>
          <Field label={t("游戏版本", "Game version")}>
            <VersionPicker loader={form.loader} value={form.versionId} onChange={(version) => set("versionId", version)} />
          </Field>
          <Field label={t("服务器介绍（MOTD）", "Message of the day")} hint={t("显示在多人游戏列表里；不填就用名称", "Shown in the multiplayer list; the name when left empty")}>
            <Input value={form.motd} maxLength={120} placeholder={form.name} onChange={(e) => set("motd", e.target.value.replace(/[\r\n]/g, ""))} />
          </Field>
          <div className="row gap-16 items-start wrap">
            <Field label={t("最多玩家", "Max players")} tone={maxPlayers >= 1 && maxPlayers <= 1000 ? undefined : "bad"}>
              <Input mono inputMode="numeric" value={form.maxPlayers} style={{ width: 100 }} invalid={!(maxPlayers >= 1 && maxPlayers <= 1000)} onChange={(e) => set("maxPlayers", e.target.value.replace(/\D/g, "").slice(0, 4))} />
            </Field>
            <Field label={t("端口", "Port")} tone={port > 0 && port < 65536 ? undefined : "bad"} hint={t("默认 25565；记得在安全组放行", "25565 by default; open it in the security group")}>
              <Input mono inputMode="numeric" value={form.port} style={{ width: 110 }} invalid={!(port > 0 && port < 65536)} onChange={(e) => set("port", e.target.value.replace(/\D/g, "").slice(0, 5))} />
            </Field>
            <Field label={t("种子（可不填）", "Seed (optional)")}>
              <Input mono value={form.seed} maxLength={64} style={{ width: 200 }} onChange={(e) => set("seed", e.target.value)} />
            </Field>
          </div>
          <MemoryField value={form.memory} onChange={(memory) => set("memory", memory)} />
          <Field label={t("难度", "Difficulty")}>
            <Segmented
              value={String(form.difficulty)}
              options={[
                { value: "0", label: t("和平", "Peaceful") },
                { value: "1", label: t("简单", "Easy") },
                { value: "2", label: t("普通", "Normal") },
                { value: "3", label: t("困难", "Hard") },
              ]}
              onChange={(v) => set("difficulty", Number(v))}
            />
          </Field>
          <Field label={t("默认游戏模式", "Default game mode")}>
            <Segmented
              value={String(form.gamemode)}
              options={[
                { value: "0", label: t("生存", "Survival") },
                { value: "1", label: t("创造", "Creative") },
                { value: "2", label: t("冒险", "Adventure") },
                { value: "3", label: t("旁观", "Spectator") },
              ]}
              onChange={(v) => set("gamemode", Number(v))}
            />
          </Field>
          <div className="setting-row">
            <div className="col">
              <span className="setting-name">{t("玩家互相伤害（PVP）", "Player vs player")}</span>
            </div>
            <Switch label="PVP" checked={form.pvp} onChange={(v) => set("pvp", v)} />
          </div>
          <div className="setting-row">
            <div className="col">
              <span className="setting-name">{t("白名单", "Whitelist")}</span>
              <span className="field-hint">{t("只让名单上的人进；开服后用 whitelist add 名字 添加", "Only listed players can join; add them with whitelist add <name> once it runs")}</span>
            </div>
            <Switch label={t("白名单", "Whitelist")} checked={form.whitelist} onChange={(v) => set("whitelist", v)} />
          </div>
          <div className="setting-row">
            <div className="col">
              <span className="setting-name">{t("正版验证", "Online mode")}</span>
              <span className="field-hint">{t("只让微软正版账号进入；关掉后离线账号也能进，但谁都能冒用别人的名字", "Only Microsoft accounts can join; turned off, offline accounts can too, but anyone can use any name")}</span>
            </div>
            <Switch label={t("正版验证", "Online mode")} checked={form.onlineMode} onChange={(v) => set("onlineMode", v)} />
          </div>
        </div>
      </section>
      <div style={{ height: 60 }} />

      {bar && createPortal(
      <AnimatePresence>
        {dirty && (
          <motion.div
            className="save-bar"
            role="region"
            aria-label={t("未保存的更改", "Unsaved changes")}
            initial={{ y: 70, opacity: 0 }}
            animate={{ y: 0, opacity: 1, transition: normal }}
            exit={{ y: 70, opacity: 0, transition: { duration: 0.16 } }}
          >
            {s.leaving ? (
              <LeaveQuestion count={changes} />
            ) : (
              <>
                <span className="save-bar-mark" />
                <span className="save-bar-text">{changes}{t(" 项更改未保存", " unsaved changes")}</span>
                <span className="kbd">{MOD_KEY}S</span>
                <div className="spacer" />
                <Button variant="hero" size="sm" onClick={() => setForm(saved)}>
                  {t("还原", "Revert")}
                </Button>
                <Button variant="primary" size="sm" icon="check" disabled={!valid} onClick={() => void save()}>
                  {t("保存", "Save")}
                </Button>
              </>
            )}
          </motion.div>
        )}
      </AnimatePresence>,
      bar,
      )}
    </>
  );
}
