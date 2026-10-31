import { useEffect, useState } from "react";

import { call, toError } from "../api/backend";
import type { Cape, Snapshot } from "../api/types";
import { Modal } from "../components/overlay";
import { PlayerFace } from "../components/PlayerFace";
import { SkinModel } from "../components/SkinModel";
import { Button, Field, Input, Segmented } from "../components/ui";
import { usePick, useStore } from "../store";
import { t } from "../lib/i18n";

/** Well-known servers, so most players need not look up an address. */
const SERVERS = [
  { label: "LittleSkin", address: "littleskin.cn" },
  { label: t("Blessing Skin 自建", "Self-hosted Blessing Skin"), address: "" },
];

/** Signing in to a third-party (Yggdrasil) server, played through
    authlib-injector. The password goes to that server only. */
export function AddYggdrasil() {
  const s = usePick((s) => ({ closeModal: s.closeModal, toast: s.toast }));
  const [server, setServer] = useState("littleskin.cn");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [profiles, setProfiles] = useState<{ id: string; name: string }[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const login = async (profile?: string) => {
    if (!server.trim() || !username.trim() || !password) return;
    setBusy(true);
    setError(null);
    try {
      const result = await call<{ profiles: { id: string; name: string }[] | null; snapshot: Snapshot }>("login_yggdrasil", {
        server: server.trim(),
        username: username.trim(),
        password,
        profile: profile ?? null,
      });
      if (result.profiles) {
        setProfiles(result.profiles);
      } else {
        useStore.getState().applySnapshot(result.snapshot);
        s.toast(t("登录成功", "Signed in"), "success");
        s.closeModal();
      }
    } catch (e) {
      setError(toError(e).message);
    }
    setBusy(false);
  };

  return (
    <Modal
      title={t("外置登录", "Third-party sign-in")}
      subtitle={t("LittleSkin 等皮肤站的账号；游戏通过 authlib-injector 使用它们的皮肤和正版验证", "Accounts on skin sites like LittleSkin; the game uses their skins and verification through authlib-injector")}
      onClose={s.closeModal}
      footer={
        <>
          <Button variant="ghost" onClick={s.closeModal}>
            {t("取消", "Cancel")}
          </Button>
          {!profiles && (
            <Button variant="primary" icon="check" busy={busy} disabled={!server.trim() || !username.trim() || !password} onClick={() => void login()}>
              {t("登录", "Sign in")}
            </Button>
          )}
        </>
      }
    >
      <form
        className="col"
        style={{ gap: 14 }}
        onSubmit={(e) => {
          e.preventDefault();
          void login();
        }}
      >
        <Field label={t("认证服务器", "Authentication server")} hint={t("皮肤站首页或 API 地址都可以，例如 littleskin.cn", "The skin site's home page or API address, e.g. littleskin.cn")}>
          <div className="row" style={{ flexWrap: "wrap" }}>
            <div style={{ flex: 1, minWidth: 220 }}>
              <Input mono value={server} disabled={busy || profiles != null} placeholder="https://skin.example.com" onChange={(e) => setServer(e.target.value)} />
            </div>
            {SERVERS.filter((item) => item.address).map((item) => (
              <button key={item.label} type="button" className="filter-chip" aria-pressed={server === item.address} onClick={() => setServer(item.address)}>
                {item.label}
              </button>
            ))}
          </div>
        </Field>
        <Field label={t("邮箱或用户名", "Email or username")}>
          <Input autoFocus value={username} disabled={busy || profiles != null} autoComplete="username" onChange={(e) => setUsername(e.target.value)} />
        </Field>
        <Field label={t("密码", "Password")} hint={t("只发给这个认证服务器，启动器不保存密码，只保存登录凭据", "Sent only to this server; the launcher keeps the sign-in token, never the password")}>
          <Input type="password" value={password} disabled={busy || profiles != null} autoComplete="current-password" onChange={(e) => setPassword(e.target.value)} />
        </Field>
        {/* Submitting with Enter. */}
        <button type="submit" hidden />
        {profiles && (
          <Field label={t("选择角色", "Choose a character")} hint={t("这个账号有多个角色，选一个用来进入游戏", "This account has several characters; choose one to play as")}>
            <div className="list">
              {profiles.map((profile) => (
                <div key={profile.id} className="list-row">
                  <PlayerFace name={profile.name} size={28} />
                  <span style={{ flex: 1, fontWeight: 600 }}>{profile.name}</span>
                  <Button size="sm" variant="go" busy={busy} onClick={() => void login(profile.id)}>
                    {t("使用", "Use")}
                  </Button>
                </div>
              ))}
            </div>
          </Field>
        )}
        {error && (
          <span className="field-hint bad">
            {error}
          </span>
        )}
      </form>
    </Modal>
  );
}

/** Changing the skin (Microsoft and third-party accounts) and the cape
    (Microsoft accounts own capes). */
export function SkinDialog({ accountId }: { accountId: string }) {
  const s = usePick((s) => ({
    account: s.accounts.find((a) => a.id === accountId) ?? null,
    closeModal: s.closeModal,
    toast: s.toast,
    chooseFiles: s.chooseFiles,
  }));
  const [slim, setSlim] = useState(s.account?.skinSlim ?? false);
  const [capes, setCapes] = useState<Cape[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const microsoft = s.account?.provider === "microsoft";

  useEffect(() => {
    if (!microsoft) return;
    call<Cape[]>("account_capes", { id: accountId }).then(setCapes, () => setCapes([]));
  }, [accountId, microsoft]);

  if (!s.account) return null;
  const run = async (what: string, task: () => Promise<unknown>, done: string) => {
    setBusy(what);
    try {
      await task();
      s.toast(done, "success");
    } catch (e) {
      s.toast(toError(e).message, "error");
    }
    setBusy(null);
  };
  const changeSkin = (path: string | null) =>
    run(path ? "upload" : "reset", async () => useStore.getState().applySnapshot(await call<Snapshot>("change_skin", { id: accountId, path, slim })), path ? t("皮肤已更换，进游戏后生效", "Skin changed; it shows next time you play") : t("已恢复默认皮肤", "Default skin restored"));
  const setCape = (cape: string | null) =>
    run(`cape:${cape ?? ""}`, async () => {
      await call("set_cape", { id: accountId, cape });
      setCapes((list) => list?.map((c) => ({ ...c, state: c.id === cape ? "ACTIVE" : "INACTIVE" })) ?? null);
    }, cape ? t("披风已更换", "Cape changed") : t("已隐藏披风", "Cape hidden"));

  return (
    <Modal title={t("皮肤与披风", "Skin & cape")} subtitle={s.account.username} onClose={s.closeModal}>
      <div className="skin-dialog">
        <div className="skin-preview">{s.account.skinUrl ? <SkinModel skinUrl={s.account.skinUrl} slim={slim} capeUrl={capes?.find((c) => c.state === "ACTIVE")?.url} /> : <PlayerFace account={s.account} size={80} />}</div>
        <div className="col" style={{ gap: 14, flex: 1, minWidth: 0 }}>
          <Field label={t("手臂粗细", "Arm width")} hint={t("Alex 式的细手臂是 3 像素，Steve 式是 4 像素；要和皮肤图一致", "Alex-style slim arms are 3 pixels, Steve-style are 4; match your skin image")}>
            <div>
              <Segmented
                value={slim ? "slim" : "classic"}
                options={[
                  { value: "classic", label: t("标准（4 像素）", "Classic (4 px)") },
                  { value: "slim", label: t("纤细（3 像素）", "Slim (3 px)") },
                ]}
                onChange={(v) => setSlim(v === "slim")}
              />
            </div>
          </Field>
          <div className="row" style={{ flexWrap: "wrap" }}>
            <Button
              variant="primary"
              icon="image"
              busy={busy === "upload"}
              disabled={busy != null}
              onClick={async () => {
                const [path] = await s.chooseFiles("skin");
                if (path) void changeSkin(path);
              }}
            >
              {t("选择皮肤图片…", "Choose skin image…")}
            </Button>
            <Button busy={busy === "reset"} disabled={busy != null} onClick={() => void changeSkin(null)}>
              {t("恢复默认", "Reset to default")}
            </Button>
          </div>
          <span className="field-hint">{t("64×64（或旧版 64×32）的 PNG。", "A 64×64 (or old 64×32) PNG.")}{s.account.provider === "yggdrasil" ? t(`会上传到 ${s.account.serverName ?? "认证服务器"}。`, `It's uploaded to ${s.account.serverName ?? "the authentication server"}.`) : ""}</span>
          {microsoft && (
            <Field label={t("披风", "Cape")}>
              {capes == null ? (
                <span className="muted">
                  <span className="spinner" />{t(" 正在读取…", " Loading…")}
                </span>
              ) : capes.length === 0 ? (
                <span className="muted">{t("这个账号没有披风。", "This account has no capes.")}</span>
              ) : (
                <div className="capes">
                  <button type="button" className="cape" aria-pressed={!capes.some((c) => c.state === "ACTIVE")} disabled={busy != null} onClick={() => void setCape(null)}>
                    <span className="cape-art none" />
                    {t("不显示", "None")}
                  </button>
                  {capes.map((cape) => (
                    <button key={cape.id} type="button" className="cape" aria-pressed={cape.state === "ACTIVE"} disabled={busy != null} onClick={() => void setCape(cape.id)}>
                      <span className="cape-art" style={{ backgroundImage: `url(${cape.url.replace(/^http:/, "https:")})` }} />
                      {cape.alias || t("披风", "Cape")}
                    </button>
                  ))}
                </div>
              )}
            </Field>
          )}
        </div>
      </div>
    </Modal>
  );
}
