// Development-only end-to-end check of the command/event contract with the
// real Rust backend. Enabled by VITE_SELFTEST=1 in a dev server; results go to
// the launcher log under target "web". Run it against a scratch data
// directory (MC_LAUNCHER_DATA_DIR): it creates and removes an instance and an
// offline account, but downloads no game files.

import { backend, call, toError } from "../api/backend";
import type { Instance, LauncherEvent, Snapshot } from "../api/types";

let pass = 0;
let fail = 0;

const log = (message: string) => void call("frontend_log", { level: "info", message: `[selftest] ${message}` }).catch(() => {});

function waitFor<T extends LauncherEvent["type"]>(
  type: T,
  match: (e: Extract<LauncherEvent, { type: T }>) => boolean = () => true,
  timeout = 30_000,
): Promise<Extract<LauncherEvent, { type: T }>> {
  return new Promise((resolve, reject) => {
    let stop: (() => void) | null = null;
    const timer = window.setTimeout(() => {
      stop?.();
      reject(new Error(`timed out waiting for ${type}`));
    }, timeout);
    void backend()
      .subscribe((event) => {
        if (event.type === type && match(event as Extract<LauncherEvent, { type: T }>)) {
          window.clearTimeout(timer);
          stop?.();
          resolve(event as Extract<LauncherEvent, { type: T }>);
        }
      })
      .then((unlisten) => (stop = unlisten));
  });
}

async function step(name: string, body: () => Promise<string>) {
  try {
    const detail = await body();
    pass += 1;
    log(`PASS ${name}${detail ? ` — ${detail}` : ""}`);
  } catch (error) {
    fail += 1;
    log(`FAIL ${name} — ${toError(error).message}`);
  }
}

function expect(condition: unknown, message: string) {
  if (!condition) throw new Error(message);
}

export async function run() {
  log("start");
  const initial = await call<Snapshot>("bootstrap");
  const originalActive = initial.settings.activeInstanceId;
  let created: Instance | null = null;

  await step("bootstrap", async () => `${initial.instances.length} instances, ${initial.accounts.length} accounts, ${initial.appInfo.platform}-${initial.appInfo.arch}`);
  await step("login_configured", async () => String(await call<boolean>("login_configured")));

  await step("load_java_runtimes → javaRuntimes", async () => {
    const event = waitFor("javaRuntimes");
    await call("load_java_runtimes");
    const e = await event;
    return `${e.data.length} runtimes${e.data[0] ? `, first java${e.data[0].major} ${e.data[0].arch}` : ""}`;
  });

  await step("load_versions → versions", async () => {
    const event = waitFor("versions");
    await call("load_versions", { force: false });
    const e = await event;
    expect("Ok" in e.data, "Err" in e.data ? e.data.Err.message : "");
    const list = (e.data as { Ok: { versions: unknown[]; latest: { release: string }; fromCache: boolean } }).Ok;
    return `${list.versions.length} versions, latest ${list.latest.release}, fromCache=${list.fromCache}`;
  });

  await step("load_loader_versions → loaderVersions", async () => {
    const key = "st-loader";
    const event = waitFor("loaderVersions", (e) => e.data.key === key);
    await call("load_loader_versions", { key, loader: "fabric", minecraft: "1.21.4" });
    const r = (await event).data.result;
    expect("Ok" in r, "Err" in r ? r.Err.message : "");
    return `${(r as { Ok: unknown[] }).Ok.length} fabric versions`;
  });

  await step("add_offline_account / remove_account", async () => {
    // Offline accounts need an owning Microsoft account; without one the
    // core refuses, which is what to check then.
    if (!initial.accounts.some((a) => a.provider === "microsoft" && a.ownership === true)) {
      const refused = await call<Snapshot>("add_offline_account", { username: "SelfTest" }).then(() => null, (error: { code?: string }) => error.code);
      expect(refused === "owner_required", `expected owner_required, got ${refused}`);
      return "refused without an owner";
    }
    const added = await call<Snapshot>("add_offline_account", { username: "SelfTest" });
    const account = added.accounts.find((a) => a.username === "SelfTest");
    expect(account, "account not added");
    expect(added.settings.activeAccountId === account!.id, "new account not active");
    const removed = await call<Snapshot>("remove_account", { id: account!.id });
    expect(!removed.accounts.some((a) => a.id === account!.id), "account not removed");
    if (initial.settings.activeAccountId) await call("set_active_account", { id: initial.settings.activeAccountId });
    return `uuid ${account!.uuid}`;
  });

  await step("create_instance (fabric)", async () => {
    const snap = await call<Snapshot>("create_instance", { name: "自测实例", versionId: "1.21.4", versionType: "release", loader: "fabric", loaderVersion: null });
    created = snap.instances.find((i) => i.name === "自测实例") ?? null;
    expect(created, "instance missing");
    expect(created!.loader === "fabric", `loader ${created!.loader}`);
    expect(snap.settings.activeInstanceId === created!.id, "not active");
    return created!.id;
  });

  if (created) {
    const id = (created as Instance).id;
    await step("configure_instance", async () => {
      const snap = await call<Snapshot>("configure_instance", { id, name: "自测实例 2", memoryMb: 3072, javaPath: "auto", loader: "fabric", loaderVersion: null });
      const i = snap.instances.find((x) => x.id === id)!;
      expect(i.name === "自测实例 2" && i.memoryMb === 3072 && i.javaPath === "auto", JSON.stringify(i));
      return "name, memory, java saved";
    });
    await step("instance_busy", async () => {
      const busy = await call<boolean>("instance_busy", { id });
      expect(busy === false, "busy");
      return "idle";
    });
    await step("search_mods → modSearch", async () => {
      const key = "st-search";
      const event = waitFor("modSearch", (e) => e.data.key === key);
      await call("search_mods", { id, key, query: "sodium", offset: 0 });
      const r = (await event).data.result;
      expect("Ok" in r, "Err" in r ? r.Err.message : "");
      const hits = (r as { Ok: { hits: { project_id: string; title: string }[]; total_hits: number } }).Ok;
      expect(hits.hits.length > 0, "no hits");
      const project = hits.hits[0].project_id;
      const vkey = "st-versions";
      const versions = waitFor("modVersions", (e) => e.data.key === vkey);
      await call("load_mod_versions", { id, key: vkey, project });
      const v = (await versions).data.result;
      expect("Ok" in v, "Err" in v ? v.Err.message : "");
      return `${hits.total_hits} hits, first "${hits.hits[0].title}" with ${(v as { Ok: unknown[] }).Ok.length} compatible versions`;
    });
    await step("list_mods / read_log", async () => {
      const mods = await call<unknown[]>("list_mods", { id });
      const lines = await call<string[]>("read_log", { id });
      return `${mods.length} mods, ${lines.length} log lines`;
    });
    await step("file_operation backup → operationComplete", async () => {
      const event = waitFor("operationComplete");
      await call("file_operation", { id, operation: { kind: "backup" } });
      const e = await event;
      expect("Ok" in e.data, "Err" in e.data ? e.data.Err.message : "");
      return (e.data as { Ok: string }).Ok;
    });
    await step("delete_instance / recycled / restoreDeleted", async () => {
      const snap = await call<Snapshot>("delete_instance", { id });
      expect(!snap.instances.some((i) => i.id === id), "still listed");
      const bin = await call<Instance[]>("recycled_instances");
      expect(bin.some((i) => i.id === id), "not in recycle bin");
      const event = waitFor("operationComplete");
      await call("file_operation", { id, operation: { kind: "restoreDeleted" } });
      const e = await event;
      expect("Ok" in e.data, "Err" in e.data ? e.data.Err.message : "");
      const back = await call<Snapshot>("bootstrap");
      expect(back.instances.some((i) => i.id === id), "not restored");
      await call("delete_instance", { id });
      return "moved to recycle bin, restored, removed again";
    });
  }

  await step("set_active_instance (restore)", async () => {
    const snap = await call<Snapshot>("set_active_instance", { id: originalActive });
    expect(snap.settings.activeInstanceId === originalActive, "not restored");
    return String(originalActive);
  });

  await step("start_login without Client ID", async () => {
    try {
      const loginId = await call<string>("start_login", { mode: "device" });
      await call("cancel_login", { loginId });
      return "configured: login started and cancelled";
    } catch (error) {
      const e = toError(error);
      expect(e.code === "client_id", `${e.code}: ${e.message}`);
      return `rejected with ${e.code}`;
    }
  });

  await step("open_url rejects non-web links", async () => {
    try {
      await call("open_url", { url: "file:///etc/hosts" });
    } catch (error) {
      return toError(error).message;
    }
    throw new Error("was accepted");
  });

  log(`done: ${pass} passed, ${fail} failed`);
}
