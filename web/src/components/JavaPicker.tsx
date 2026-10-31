import { sameArch } from "../lib/format";
import { usePick } from "../store";
import { Button, Status } from "./ui";
import { t } from "../lib/i18n";

/** Short distribution names: the vendor string is a company, players know
    the product. */
function vendorName(vendor: string): string {
  const v = vendor.toLowerCase();
  if (v.includes("adoptium") || v.includes("temurin")) return "Temurin";
  if (v.includes("azul") || v.includes("zulu")) return "Zulu";
  if (v.includes("microsoft")) return "Microsoft OpenJDK";
  if (v.includes("amazon") || v.includes("corretto")) return "Corretto";
  if (v.includes("graal")) return "GraalVM";
  if (v.includes("oracle")) return "Oracle";
  if (v.includes("homebrew")) return "OpenJDK";
  return vendor || "Java";
}

/** Where a runtime came from, read off its path; the full path is the row's tooltip. */
function javaSource(path: string): string {
  const p = path.toLowerCase().replace(/\\/g, "/");
  if (p.includes("mc-launcher") || p.includes("/runtime/") || p.includes("/runtimes/")) return t("启动器下载", "Downloaded by launcher");
  if (p.includes("homebrew") || p.includes("/usr/local/opt/") || p.includes("/opt/homebrew/")) return "Homebrew";
  if (p.includes("javavirtualmachines")) return t("系统安装", "System");
  if (p.includes("program files")) return t("系统安装", "System");
  if (p.includes("/.sdkman/")) return "SDKMAN";
  return t("本机", "This computer");
}

/** Detected runtimes; choosing one writes its path into the field above.
    Rows show version and source; hover for the full path. */
export function JavaPicker({ value, onChange, maxHeight = 150 }: { value: string; onChange: (path: string) => void; maxHeight?: number }) {
  const s = usePick((s) => ({
    runtimes: s.javaRuntimes,
    scanning: s.javaScanning,
    arch: s.appInfo.arch,
    scanJava: s.scanJava,
  }));
  return (
    <div className="col" style={{ gap: 6 }}>
      <div className="row">
        <span className="faint" style={{ fontSize: "var(--text-xs)" }}>
          {s.scanning ? t("正在检测本机 Java…", "Looking for Java…") : s.runtimes.length ? t(`检测到 ${s.runtimes.length} 个 Java`, `${s.runtimes.length} Java installations found`) : t("没有检测到系统 Java，安装世界时会自动准备。", "No system Java found; one is prepared when you install a world.")}
        </span>
        <div className="spacer" />
        <Button size="sm" variant="ghost" icon="refresh" busy={s.scanning} onClick={s.scanJava}>
          {s.scanning ? t("检测中…", "Scanning…") : t("重新检测", "Scan again")}
        </Button>
      </div>
      {s.runtimes.length > 0 && (
        <div className="picker" style={{ maxHeight }}>
          {s.runtimes.map((java) => {
            const chosen = value === java.path;
            const compatible = sameArch(java.arch, s.arch);
            return (
              <div key={java.path} className="picker-row" aria-selected={chosen} title={java.path}>
                <span className="java-major">Java {java.major}</span>
                <div className="col" style={{ flex: 1, minWidth: 0 }}>
                  <span className="java-name truncate">
                    {vendorName(java.vendor)} <span className="mono faint">{java.version}</span>
                  </span>
                  <span className="faint truncate" style={{ fontSize: "var(--text-xs)" }}>
                    {javaSource(java.path)} · {java.arch}
                  </span>
                </div>
                {chosen ? (
                  <Status tone="accent">{t("已选择", "Selected")}</Status>
                ) : !compatible ? (
                  <Status tone="danger">{t("架构不匹配", "Wrong architecture")}</Status>
                ) : (
                  <Button size="sm" onClick={() => onChange(java.path)}>
                    {t("使用", "Use")}
                  </Button>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
