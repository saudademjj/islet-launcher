import { useEffect, useState } from "react";

import { call, toError } from "../api/backend";
import type { ContentKind, ModProject, ProjectDetails } from "../api/types";
import { formatCount, formatRelative } from "../lib/format";
import { t } from "../lib/i18n";
import { usePick } from "../store";
import { Icon, type IconName } from "./Icon";
import { ModIcon } from "./ModIcon";
import { Button } from "./ui";

const LINK_LABEL: Record<string, string> = {
  page: t("项目主页", "Project page"),
  source: t("源代码", "Source"),
  wiki: "Wiki",
  issues: t("问题反馈", "Issues"),
  discord: "Discord",
};

/** Markdown or HTML from the project, rendered and then sanitized. The two
 *  libraries load only when a description is first opened. */
async function render(details: ProjectDetails): Promise<string> {
  const [{ marked }, { default: DOMPurify }] = await Promise.all([import("marked"), import("dompurify")]);
  const html = details.format === "markdown" ? (marked.parse(details.body, { async: false, gfm: true }) as string) : details.body;
  DOMPurify.removeAllHooks();
  DOMPurify.addHook("afterSanitizeAttributes", (node) => {
    if (node.tagName === "IMG") {
      node.setAttribute("loading", "lazy");
      node.setAttribute("referrerpolicy", "no-referrer");
    }
  });
  return DOMPurify.sanitize(html, {
    FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select", "iframe", "video", "audio"],
    FORBID_ATTR: ["style", "class", "id", "target"],
  });
}

/** A search result opened up: description, gallery and links, with the
 *  newest compatible version one click away. */
export function ProjectDetail({ project, kind, glyph, busy, onInstall, onBack, pick }: { project: ModProject; kind: ContentKind; glyph: IconName; busy: boolean; onInstall: (version: string) => void; onBack: () => void; /** Versions listed by the caller (modpacks), instead of the store's. */ pick?: { loading: boolean; newest: { id: string; version_number: string } | null } }) {
  const s = usePick((s) => ({ versions: s.modVersions, activeId: s.settings.activeInstanceId, manageId: s.modal?.kind === "manage" ? s.modal.id : null, installMod: s.installMod, openUrl: s.openUrl, fileBusy: s.fileBusy }));
  const [details, setDetails] = useState<ProjectDetails | null>(null);
  const [html, setHtml] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    call<ProjectDetails>("mod_details", { project: project.project_id })
      .then(async (found) => {
        const clean = await render(found);
        if (!live) return;
        setDetails(found);
        setHtml(clean);
      })
      .catch((e) => live && setError(toError(e).message));
    return () => {
      live = false;
    };
  }, [project.project_id]);

  const updated = details?.updated ? Date.parse(details.updated) : NaN;
  const versions = pick ? { loading: pick.loading } : s.versions.project === project.project_id ? s.versions : null;
  const newest = pick ? pick.newest : s.versions.project === project.project_id ? s.versions.versions[0] : undefined;
  const instanceId = pick ? "new" : (s.manageId ?? s.activeId);

  const [notes, setNotes] = useState("");
  useEffect(() => {
    let live = true; setNotes("");
    if (newest) call<string>("mod_changelog", { version: newest.id }).then(body => render({ body, format: project.project_id.startsWith("cf:") ? "html" : "markdown", gallery: [], links: [], license: null, updated: null })).then(html => { if (live) setNotes(html); }, () => {});
    return () => { live = false; };
  }, [newest?.id, project.project_id]);
  // Links in a description open in the browser, never inside the launcher.
  const follow = (event: React.MouseEvent) => {
    const anchor = (event.target as HTMLElement).closest("a");
    if (!anchor) return;
    event.preventDefault();
    const href = anchor.getAttribute("href") ?? "";
    if (/^https:\/\//i.test(href)) void s.openUrl(href);
  };

  return (
    <div className="project-detail">
      <div className="project-head">
        <Button size="sm" variant="ghost" icon="chevronLeft" onClick={onBack}>
          {t("返回结果", "Back to results")}
        </Button>
        <ModIcon src={project.icon_url} seed={project.project_id} size={48} glyph={glyph} />
        <div className="col fill gap-2">
          <span className="project-title truncate">{project.title}</span>
          <span className="faint truncate" style={{ fontSize: "var(--text-xs)" }}>
            <Icon name="download" /> {formatCount(project.downloads)}
            {details?.license && ` · ${details.license}`}
            {Number.isFinite(updated) && t(` · ${formatRelative(updated)}更新`, ` · updated ${formatRelative(updated)}`)}
          </span>
        </div>
        {instanceId && (
          <Button variant="primary" icon="download" disabled={busy || s.fileBusy || !newest} onClick={() => newest && onInstall(newest.id)}>
            {versions?.loading ? t("正在获取版本…", "Loading versions…") : newest ? t(`安装 ${newest.version_number}`, `Install ${newest.version_number}`) : t("没有兼容版本", "No compatible version")}
          </Button>
        )}
      </div>
      {notes && <details><summary>{t("更新日志", "Changelog")}</summary><div className="project-body selectable" onClick={follow} dangerouslySetInnerHTML={{ __html: notes }} /></details>}
      <p className="muted project-summary">{project.description}</p>
      {details && details.links.length > 0 && (
        <div className="project-links">
          {details.links.map(([kindOfLink, url]) => (
            <Button key={kindOfLink} size="sm" variant="ghost" icon="external" onClick={() => void s.openUrl(url)}>
              {LINK_LABEL[kindOfLink] ?? kindOfLink}
            </Button>
          ))}
        </div>
      )}
      {details && details.gallery.length > 0 && (
        <div className="project-gallery" aria-label={t("截图", "Gallery")}>
          {details.gallery.map((image) => (
            <button key={image.url} type="button" className="project-shot" title={image.title ?? undefined} onClick={() => void s.openUrl(image.url)}>
              <img src={image.url} alt={image.title ?? ""} loading="lazy" referrerPolicy="no-referrer" />
            </button>
          ))}
        </div>
      )}
      {error ? (
        <div className="faint" style={{ padding: "12px 0" }}>
          {t(`没能加载介绍：${error}`, `Couldn't load the description: ${error}`)}
        </div>
      ) : !details ? (
        <div className="faint row" style={{ padding: "12px 0" }}>
          <span className="spinner" />
          {t(" 正在加载介绍…", " Loading the description…")}
        </div>
      ) : html.trim() ? (
        <div className="project-body selectable" onClick={follow} dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <div className="faint" style={{ padding: "12px 0" }}>
          {kind === "mod" ? t("这个模组没有写介绍。", "This mod has no description.") : t("这个项目没有写介绍。", "This project has no description.")}
        </div>
      )}
    </div>
  );
}
