import { AnimatePresence, motion } from "motion/react";

import type { Account } from "../api/types";
import { Icon, type IconName } from "../components/Icon";
import { Landscape, PX } from "../components/Pixel";
import { biomeStyle, useBiome } from "../lib/biome";
import { PlayerFace, SkinFront } from "../components/PlayerFace";
import { Button, Chip, Notice, SectionTitle, Status } from "../components/ui";
import { formatDate } from "../lib/format";
import { item, stagger } from "../lib/motion";
import { Toolbar } from "../layout/Shell";
import { useActiveAccount, useHasOwner, usePick, useStore } from "../store";
import { t } from "../lib/i18n";

export function AccountsPage() {
  const s = usePick((s) => ({
    accounts: s.accounts,
    activeId: s.settings.activeAccountId,
    loginConfigured: s.loginConfigured,
    authBusy: s.authBusy || s.auth !== null,
    openModal: s.openModal,
    startLogin: s.startLogin,
    setPage: s.setPage,
    openSettings: s.openSettings,
  }));
  const active = useActiveAccount();
  const owner = useHasOwner();

  const ready = s.loginConfigured;
  const microsoft = [
    {
      icon: "shield" as IconName,
      title: t("微软账号", "Microsoft account"),
      text: ready ? t("正版账号，可加入所有服务器", "Official account for all servers") : t("需要先在设置里填写 Client ID", "Needs a Client ID in Settings first"),
      primary: true,
      tag: ready ? undefined : t("需配置", "Setup needed"),
      disabled: s.authBusy,
      onClick: () => (ready ? void s.startLogin("browser") : s.openSettings("login")),
    },
    {
      icon: "monitor" as IconName,
      title: t("设备代码登录", "Device code"),
      text: ready ? t("在手机或其他设备上完成微软登录", "Sign in on a phone or another device") : t("同样需要先配置微软登录", "Also needs Microsoft sign-in set up first"),
      tag: ready ? undefined : t("需配置", "Setup needed"),
      disabled: s.authBusy,
      onClick: () => (ready ? void s.startLogin("device") : s.openSettings("login")),
    },
  ];
  const offline = {
    icon: "user" as IconName,
    title: t("离线账号", "Offline account"),
    text: owner ? t("单人游戏和局域网", "Singleplayer and LAN") : t("先登录一个拥有 Java 版的微软账号", "Sign in a Microsoft account that owns Java Edition first"),
    tag: owner ? undefined : t("需正版", "Needs ownership"),
    disabled: !owner,
    onClick: () => s.openModal({ kind: "addOffline" }),
  };
  const thirdParty = {
    icon: "globe" as IconName,
    title: t("外置登录", "Third-party sign-in"),
    text: owner ? t("LittleSkin 等皮肤站与统一通行证", "Skin sites like LittleSkin and Nide8") : t("先登录一个拥有 Java 版的微软账号", "Sign in a Microsoft account that owns Java Edition first"),
    tag: owner ? undefined : t("需正版", "Needs ownership"),
    disabled: !owner,
    onClick: () => s.openModal({ kind: "addYggdrasil" }),
  };
  const options: { icon: IconName; title: string; text: string; primary?: boolean; tag?: string; disabled?: boolean; onClick: () => void }[] = [...microsoft, offline, thirdParty];

  return (
    <>
      <Toolbar title={t("账号", "Accounts")} meta={active ? t(`${s.accounts.length} 个账号 · 正在使用 ${active.username}`, `${s.accounts.length} accounts · using ${active.username}`) : t(`${s.accounts.length} 个账号`, `${s.accounts.length} accounts`)} />
      <div className="content">
        <motion.div className="page" variants={stagger} initial="initial" animate="animate">
          <div className="page-split">
            <div className="page-main">
              {active && <Profile account={active} />}
              {!s.loginConfigured && (
                <motion.div variants={item}>
                  <Notice
                    tone="warn"
                    title={t("微软登录不可用", "Microsoft sign-in isn't available")}
                    actions={
                      <Button size="sm" onClick={() => s.openSettings("login")}>
                        {t("去设置", "Open settings")}
                      </Button>
                    }
                  >
                    {t("当前版本未内置微软登录。可在设置中填写自己的 Azure 应用 ID。", "Microsoft sign-in isn't built in. Add your own Azure Client ID in Settings.")}
                  </Notice>
                </motion.div>
              )}

              {s.accounts.length > 0 && (
                <motion.section variants={item}>
                  <SectionTitle>{t("已添加", "Added")}</SectionTitle>
                  <div className="col gap-12">
                    <AnimatePresence initial={false}>
                      {s.accounts.map((account) => (
                        <AccountCard key={account.id} account={account} active={account.id === s.activeId} />
                      ))}
                    </AnimatePresence>
                  </div>
                </motion.section>
              )}
            </div>

            {/* Adding an account is the side task here: a column of ways to sign in. */}
            <motion.aside className="page-aside" variants={item}>
              <section>
                <SectionTitle>{s.accounts.length ? t("添加账号", "Add account") : t("添加第一个账号", "Add your first account")}</SectionTitle>
                <div className="options">
                  {options.map((o) => (
                    <button key={o.title} type="button" className={`option${o.primary ? " primary" : ""}${o.tag ? " pending" : ""}`} disabled={o.disabled} onClick={o.onClick}>
                      <span className="option-icon">
                        <Icon name={o.icon} />
                      </span>
                      <span className="option-title">
                        {o.title}
                        {o.tag && <span className="option-tag">{o.tag}</span>}
                      </span>
                      <span className="option-text">{o.text}</span>
                      <Icon name="chevronRight" className="option-go" />
                    </button>
                  ))}
                </div>
                <p className="fine-print">
                  {t("微软登录在官方页面完成，密码不经过启动器。使用离线或外置账号前需先登录一个拥有 Java 版的微软账号。", "Sign-in happens on Microsoft's page; the launcher never sees your password. Offline and third-party accounts require a signed-in Java Edition account.")}
                </p>
              </section>
            </motion.aside>
          </div>
        </motion.div>
      </div>
    </>
  );
}

function providerLabel(account: Account): string {
  if (account.provider === "microsoft") return t("微软", "Microsoft");
  if (account.provider === "yggdrasil") return account.serverName ? t(`外置 · ${account.serverName}`, `Third-party · ${account.serverName}`) : t("外置", "Third-party");
  return t("离线", "Offline");
}

function AccountCard({ account, active }: { account: Account; active: boolean }) {
  const s = usePick((s) => ({ mutate: s.mutate, openModal: s.openModal, copy: s.copy }));
  const owner = useHasOwner();
  return (
    <motion.div
      layout="position"
      className={`account-card${active ? " active" : ""}`}
      data-drop-account={account.id}
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0, marginTop: -10, transition: { duration: 0.16 } }}
    >
      <PlayerFace account={account} size={48} className={active ? "accent" : undefined} />
      <div className="col fill gap-4">
        <div className="row">
          <span className="account-name">{account.username}</span>
          <Chip>{providerLabel(account)}</Chip>
          {account.ownership === false && <Status tone="warn">{t("未拥有 Java 版", "Doesn't own Java Edition")}</Status>}
          {account.provider !== "microsoft" && !owner && <Status tone="warn">{t("需先登录微软账号", "Needs a Microsoft sign-in")}</Status>}
        </div>
        {/* The UUID is for whitelists and support, a click away rather than on show. */}
        <span className="account-meta">
          {t("添加于 ", "Added ")}{formatDate(account.createdAt)}
          <button type="button" className="link account-uuid" title={account.uuid} onClick={() => void s.copy(account.uuid, "UUID ")}>
            <Icon name="copy" />
            {t("复制 UUID", "Copy UUID")}
          </button>
        </span>
      </div>
      {active ? (
        <span className="pill-accent">
          <Icon name="check" />{t(" 使用中", " In use")}
        </span>
      ) : (
        <Button size="sm" onClick={() => void s.mutate("set_active_account", { id: account.id })}>
          {t("使用", "Use")}
        </Button>
      )}
      <Button size="sm" variant="ghost" icon="trash" aria-label={t("删除账号", "Remove account")} title={t("删除账号", "Remove account")} onClick={() => s.openModal({ kind: "deleteAccount", account })} />
    </motion.div>
  );
}

/** The account in use, as a player card on its own patch of land. */
function Profile({ account }: { account: Account }) {
  const biome = useBiome(account.uuid);
  return (
    <motion.section className="profile" variants={item} style={biomeStyle(biome)}>
      <Landscape seed={account.uuid} unit={PX} cols={80} ground={[1, 3]} sky={8} treesFrom={40} treeChance={0.35} far={[4, 7]} biome={biome} />
      {/* The whole skin standing on the land when there is one; else the face. */}
      <span className="profile-figure">{account.skinUrl ? <SkinFront account={account} px={4} /> : null}</span>
      <PlayerFace key={account.uuid} account={account} size={72} className="profile-face" />
      <div className="col gap-6">
        <span className="profile-label">{t("正在使用", "Signed in as")}</span>
        <div className="row gap-12">
          <span className="profile-name truncate">{account.username}</span>
          <span className="profile-kind">{account.provider === "microsoft" ? t("微软账号", "Microsoft account") : account.provider === "yggdrasil" ? (account.serverName ?? t("外置账号", "Third-party account")) : t("离线账号", "Offline account")}</span>
        </div>
        {true && (
          <div className="row" style={{ marginTop: 6 }}>
            <Button size="sm" variant="hero" icon="image" onClick={() => useStore.getState().openModal({ kind: "skin", accountId: account.id })}>
              {account.provider === "microsoft" ? t("皮肤与披风", "Skin & cape") : t("更换皮肤", "Change skin")}
            </Button>
          </div>
        )}
      </div>
    </motion.section>
  );
}
