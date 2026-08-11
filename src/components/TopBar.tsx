import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { CredentialPanel } from "../features/vault/CredentialPanel";
import { agoSeconds } from "../lib/format";
import { LANGUAGE_NAMES, useI18n, useT } from "../lib/i18n";
import { api, apiV2 } from "../lib/ipc";
import { keys, useAuditTail, useAuth, useCapabilities, useProjects } from "../lib/queries";
import type { EnvLabel, Language, Settings } from "../lib/types";
import {
  Badge,
  Button,
  Dialog,
  ErrorBox,
  Field,
  Input,
  KeyValue,
  Kbd,
  Loading,
  Notice,
  Select,
  Toggle,
  useToast,
} from "./ui";

const LABEL_TEXT: Record<EnvLabel, string> = {
  dev: "DEV",
  staging: "STAGING",
  prod: "PRODUCTION",
  unknown: "CHƯA GẮN NHÃN",
};

export function EnvLabelBadge({ label }: { label: EnvLabel }) {
  const t = useT();
  if (label === "prod") {
    return (
      <Badge
        tone="critical"
        icon="●"
        title={t("Project production — mọi thao tác ghi cần gõ tên service")}
      >
        {t(LABEL_TEXT.prod)}
      </Badge>
    );
  }
  if (label === "unknown") {
    return (
      <Badge
        tone="warning"
        icon="?"
        title={t("Chưa gắn nhãn — app xử lý như production cho an toàn")}
      >
        {t(LABEL_TEXT.unknown)}
      </Badge>
    );
  }
  if (label === "staging") {
    return <Badge tone="serious">{t(LABEL_TEXT.staging)}</Badge>;
  }
  return <Badge tone="good">{t(LABEL_TEXT.dev)}</Badge>;
}

export function TopBar({
  settings,
  project,
  onProjectChange,
  dataAgeSeconds,
  refreshing,
  onRefresh,
  onOpenPalette,
  theme,
  onThemeToggle,
}: {
  settings: Settings;
  project: string | null;
  onProjectChange: (p: string) => void;
  dataAgeSeconds: number | null;
  refreshing: boolean;
  onRefresh: () => void;
  onOpenPalette: () => void;
  theme: "light" | "dark" | "system";
  onThemeToggle: () => void;
}) {
  const t = useT();
  const qc = useQueryClient();
  const toast = useToast();
  const auth = useAuth();
  const projects = useProjects();
  const caps = useCapabilities(project);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [auditOpen, setAuditOpen] = useState(false);

  const label: EnvLabel = project ? (settings.projectLabels[project] ?? inferLabel(project)) : "unknown";
  const isProd = label === "prod";

  const patchSettings = (s: Settings) => qc.setQueryData(keys.settings, s);

  return (
    <>
      <header
        className="flex shrink-0 items-center gap-2 border-b px-3 py-2"
        style={{
          background: isProd ? "color-mix(in oklab, var(--status-critical) 12%, var(--surface-1))" : "var(--surface-1)",
          // Viền đỏ dày trên project production: dấu hiệu ngoại vi, thấy được mà không
          // cần đọc chữ nào.
          borderTop: isProd ? "3px solid var(--status-critical)" : "3px solid transparent",
        }}
      >
        <span className="text-[13px] font-semibold">Cloud Run Cockpit</span>

        <Select
          value={project ?? ""}
          onChange={(e) => onProjectChange(e.target.value)}
          className="max-w-[280px]"
          aria-label="GCP project"
        >
          <option value="" disabled>
            {projects.isLoading ? t("Đang tải project…") : t("Chọn project…")}
          </option>
          {settings.recentProjects.length > 0 && (
            <optgroup label={t("Gần đây")}>
              {settings.recentProjects.map((p) => (
                <option key={`r-${p}`} value={p}>
                  {p}
                </option>
              ))}
            </optgroup>
          )}
          <optgroup label={t("Tất cả project")}>
            {(projects.data ?? []).map((p) => (
              <option key={p.projectId} value={p.projectId}>
                {p.displayName === p.projectId ? p.projectId : `${p.displayName} — ${p.projectId}`}
              </option>
            ))}
          </optgroup>
        </Select>

        {project && <EnvLabelBadge label={label} />}

        {project && label === "unknown" && (
          <Select
            aria-label={t("Gắn nhãn môi trường")}
            value=""
            className="h-7 text-[11px]"
            onChange={async (e) => {
              const v = e.target.value as EnvLabel;
              if (!v) return;
              patchSettings(await api.setProjectLabel(project, v));
              toast({
                tone: "good",
                title: t("Đã gắn nhãn {label} cho {project}", {
                  label: t(LABEL_TEXT[v]),
                  project: project ?? "",
                }),
                body:
                  v === "dev" || v === "staging"
                    ? t("Từ giờ thao tác ghi trên project này không cần gõ tên service.")
                    : t("Thao tác ghi trên project này sẽ luôn cần gõ tên service."),
              });
            }}
          >
            <option value="">{t("Gắn nhãn…")}</option>
            <option value="dev">Dev</option>
            <option value="staging">Staging</option>
            <option value="prod">Production</option>
          </Select>
        )}

        <div className="ml-auto flex items-center gap-2">
          {dataAgeSeconds !== null && (
            <span
              className="tnum text-[11px] text-[var(--ink-muted)]"
              title={t("Độ tươi của dữ liệu đang hiển thị")}
            >
              {t("dữ liệu {ago}", { ago: agoSeconds(dataAgeSeconds) })}
            </span>
          )}

          <Button
            size="sm"
            variant="ghost"
            onClick={onRefresh}
            loading={refreshing}
            title={t("Bỏ cache và lấy lại")}
          >
            ⟳ Reload
          </Button>

          <Toggle
            checked={settings.readOnly}
            tone="warn"
            hint={
              settings.readOnly
                ? t("Đang chỉ đọc — app không gửi thay đổi nào lên GCP")
                : t("ĐANG CHO GHI — thao tác sẽ tạo revision mới trên GCP")
            }
            label={settings.readOnly ? t("🔒 Chỉ đọc") : t("✎ Cho ghi")}
            onChange={async (v) => {
              patchSettings(await api.setReadOnly(v));
              if (!v) {
                toast({
                  tone: isProd ? "critical" : "warning",
                  title: t("Đã bật chế độ ghi"),
                  body: isProd
                    ? t(
                        "{project} được gắn nhãn PRODUCTION. Mỗi thao tác ghi vẫn sẽ yêu cầu gõ đúng tên service.",
                        { project: project ?? "" },
                      )
                    : t("Thao tác sửa env/scaling từ giờ sẽ tạo revision mới thật trên GCP."),
                });
              }
            }}
          />

          <Button size="sm" variant="ghost" onClick={onOpenPalette} title={t("Nhảy tới service")}>
            🔍 <Kbd>Ctrl K</Kbd>
          </Button>

          <Button size="sm" variant="ghost" onClick={onThemeToggle} title={`Theme: ${theme}`}>
            {theme === "dark" ? "🌙" : theme === "light" ? "☀" : "◐"}
          </Button>

          <Button
            size="sm"
            variant="ghost"
            onClick={() => setAuditOpen(true)}
            title={t("Lịch sử thao tác")}
          >
            📜
          </Button>

          <Button
            size="sm"
            variant="ghost"
            onClick={() => setSettingsOpen(true)}
            title={t("Cài đặt")}
          >
            ⚙
          </Button>
        </div>
      </header>

      {/* Thanh phụ: danh tính hiệu lực + cảnh báo quyền. Chỉ hiện khi có gì cần nói. */}
      {(auth.data?.impersonating || auth.isError || (caps.data && caps.data.missing.length > 0)) && (
        <div
          className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 border-b px-3 py-1 text-[11px]"
          style={{ background: "var(--surface-2)" }}
        >
          {auth.data?.impersonating && (
            <span className="flex items-center gap-1.5">
              <Badge tone="serious" icon="⇄">
                Impersonating
              </Badge>
              <span className="mono">{auth.data.impersonating}</span>
              <span className="text-[var(--ink-muted)]">
                {t("(đăng nhập: {account})", { account: auth.data.account })}
              </span>
            </span>
          )}
          {caps.data && caps.data.missing.length > 0 && (
            <details>
              <summary className="cursor-pointer text-[var(--ink-secondary)]">
                {t("Thiếu {n} nhóm quyền trên project này", { n: caps.data.missing.length })}
              </summary>
              <ul className="mt-1 list-disc pl-5">
                {caps.data.missing.map((m) => (
                  <li key={m} className="selectable">
                    {m}
                  </li>
                ))}
              </ul>
            </details>
          )}
          {auth.isError && <span style={{ color: "var(--status-critical)" }}>{auth.error?.message}</span>}
        </div>
      )}

      <SettingsDialog
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        settings={settings}
        project={project}
        onSaved={patchSettings}
      />
      <AuditDialog open={auditOpen} onClose={() => setAuditOpen(false)} />
    </>
  );
}

/** Bản TS của `suggest_label` trong Rust, chỉ để hiện nhãn ngay khi chưa gọi backend. */
function inferLabel(projectId: string): EnvLabel {
  const id = projectId.toLowerCase();
  if (["prod", "production", "master", "live", "main"].some((k) => id.includes(k))) return "prod";
  if (["stg", "staging", "stage", "uat", "preprod"].some((k) => id.includes(k))) return "staging";
  if (["dev", "develop", "sandbox", "test", "local", "demo"].some((k) => id.includes(k))) return "dev";
  return "unknown";
}

function SettingsDialog({
  open,
  onClose,
  settings,
  project,
  onSaved,
}: {
  open: boolean;
  onClose: () => void;
  settings: Settings;
  project: string | null;
  onSaved: (s: Settings) => void;
}) {
  const t = useT();
  const { lang } = useI18n();
  const toast = useToast();
  const [auto, setAuto] = useState(String(settings.autoRefreshSeconds));
  const [poll, setPoll] = useState(String(settings.logPollSeconds));
  const [reveal, setReveal] = useState(String(settings.revealTimeoutSeconds));
  const [verifying, setVerifying] = useState(false);
  const [auditFile, setAuditFile] = useState<string | null>(null);
  const [allowedText, setAllowedText] = useState(settings.allowedProjects.join(", "));
  const [savingAllow, setSavingAllow] = useState(false);

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={t("Cài đặt")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("Đóng")}
          </Button>
          <Button
            variant="primary"
            onClick={async () => {
              onSaved(
                await api.setPreferences({
                  autoRefreshSeconds: Number(auto) || 0,
                  logPollSeconds: Number(poll) || 3,
                  revealTimeoutSeconds: Number(reveal) || 30,
                }),
              );
              toast({ tone: "good", title: t("Đã lưu cài đặt") });
              onClose();
            }}
          >
            {t("Lưu")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        {/* Ngôn ngữ đứng đầu: người không đọc được tiếng Việt phải tìm thấy nó trước
            khi phải hiểu bất kỳ mục nào khác trong màn này. Lưu ngay khi chọn, không
            đợi bấm Lưu — đổi ngôn ngữ mà phải đọc tiếp tiếng Việt để tìm nút Lưu thì
            hỏng mất mục đích. */}
        <div>
          <h3 className="mb-1.5 text-[12px] font-semibold">{t("Ngôn ngữ")}</h3>
          <div className="flex items-center gap-2">
            <Select
              value={lang}
              aria-label={t("Ngôn ngữ")}
              onChange={async (e) => {
                const next = e.target.value as Language;
                onSaved(await api.setPreferences({ language: next }));
              }}
            >
              {(Object.keys(LANGUAGE_NAMES) as Language[]).map((l) => (
                <option key={l} value={l}>
                  {LANGUAGE_NAMES[l]}
                </option>
              ))}
            </Select>
            <span className="text-[11px] text-[var(--ink-muted)]">
              {t("Message lỗi từ GCP vẫn hiện bằng tiếng Việt.")}
            </span>
          </div>
        </div>

        <div>
          <h3 className="mb-1.5 text-[12px] font-semibold">{t("Xác thực (Service Account)")}</h3>
          <CredentialPanel allowedProjects={settings.allowedProjects} />
        </div>

        <div>
          <h3 className="mb-1.5 text-[12px] font-semibold">{t("Project được phép thao tác")}</h3>
          <p className="mb-2 text-[11px] leading-relaxed text-[var(--ink-muted)]">
            {t(
              "Khi khoá, app chỉ cho phép thao tác trên đúng những project trong danh sách — chặn ở tầng Rust, không chỉ ẩn dropdown. Đây là lớp bảo vệ để app không đụng nhầm project production hay staging. Nhiều project cách nhau bằng dấu phẩy.",
            )}
          </p>
          <div className="flex items-center gap-2">
            <Input
              value={allowedText}
              onChange={(e) => setAllowedText(e.target.value)}
              className="flex-1"
              placeholder="example-project"
            />
            <Toggle
              checked={settings.projectLock}
              tone="warn"
              hint={
                settings.projectLock
                  ? t("Đang khoá — chỉ project trong danh sách")
                  : t("Đang mở — mọi project")
              }
              label={settings.projectLock ? t("🔒 Đang khoá") : t("🔓 Đang mở")}
              onChange={async (v) => {
                const projects = allowedText.split(",").map((s) => s.trim()).filter(Boolean);
                onSaved(await apiV2.setAllowedProjects(projects, v));
                toast({
                  tone: v ? "good" : "warning",
                  title: v ? t("Đã khoá vào danh sách project") : t("Đã mở khoá project"),
                  body: v
                    ? t("Chỉ còn thao tác được trên: {list}", { list: projects.join(", ") })
                    : t("App có thể thao tác trên mọi project — cẩn thận với prod."),
                });
              }}
            />
            <Button
              size="sm"
              loading={savingAllow}
              onClick={async () => {
                setSavingAllow(true);
                try {
                  const projects = allowedText.split(",").map((s) => s.trim()).filter(Boolean);
                  onSaved(await apiV2.setAllowedProjects(projects, settings.projectLock));
                  toast({ tone: "good", title: t("Đã lưu danh sách project được phép") });
                } catch (e) {
                  const err = e as { message?: string };
                  toast({ tone: "critical", title: t("Không lưu được"), body: err.message });
                } finally {
                  setSavingAllow(false);
                }
              }}
            >
              {t("Lưu danh sách")}
            </Button>
          </div>
        </div>

        <div className="grid grid-cols-3 gap-3">
          <Field
            label={t("Auto refresh (giây)")}
            hint={t("0 = tắt. Dưới 10s sẽ bị kẹp lên 10s.")}
          >
            <Input value={auto} onChange={(e) => setAuto(e.target.value)} inputMode="numeric" />
          </Field>
          <Field
            label={t("Nhịp poll log (giây)")}
            hint={t("REST không có streaming thật, log lấy bằng polling.")}
          >
            <Input value={poll} onChange={(e) => setPoll(e.target.value)} inputMode="numeric" />
          </Field>
          <Field label={t("Tự ẩn secret sau (giây)")}>
            <Input value={reveal} onChange={(e) => setReveal(e.target.value)} inputMode="numeric" />
          </Field>
        </div>

        <div>
          <h3 className="mb-1.5 text-[12px] font-semibold">{t("Nhãn môi trường")}</h3>
          <p className="mb-2 text-[11px] text-[var(--ink-muted)]">
            {t(
              "Project nhãn Production hoặc chưa gắn nhãn yêu cầu gõ đúng tên service trước khi ghi. Gắn nhãn Dev cho project thử nghiệm để khỏi phải gõ mỗi lần.",
            )}
          </p>
          <div className="flex flex-col gap-1">
            {Object.entries(settings.projectLabels).length === 0 && (
              <p className="text-[11px] text-[var(--ink-muted)]">{t("Chưa gắn nhãn project nào.")}</p>
            )}
            {Object.entries(settings.projectLabels).map(([p, l]) => (
              <div key={p} className="flex items-center gap-2">
                <span className="mono flex-1 truncate text-[12px]">{p}</span>
                <Select
                  className="h-7 text-[11px]"
                  value={l}
                  onChange={async (e) => onSaved(await api.setProjectLabel(p, e.target.value as EnvLabel))}
                >
                  <option value="dev">{t("Dev")}</option>
                  <option value="staging">{t("Staging")}</option>
                  <option value="prod">{t("Production")}</option>
                  <option value="unknown">{t("Chưa gắn nhãn")}</option>
                </Select>
              </div>
            ))}
          </div>
        </div>

        <div>
          <h3 className="mb-1.5 text-[12px] font-semibold">{t("Kiểm tra tên metric")}</h3>
          <p className="mb-2 text-[11px] leading-relaxed text-[var(--ink-muted)]">
            {t(
              "Monitoring API không báo lỗi khi tên metric sai — nó trả về series rỗng. Chart phẳng ở 0 khi đó sẽ bị đọc thành “service không có tải”. Chạy kiểm tra này khi thêm project mới.",
            )}
          </p>
          <Button
            size="sm"
            loading={verifying}
            disabled={!project}
            onClick={async () => {
              if (!project) return;
              setVerifying(true);
              try {
                const res = await api.verifyMetrics(project);
                const missing = res.filter((m) => !m.exists);
                toast({
                  tone: missing.length ? "warning" : "good",
                  title: missing.length
                    ? t("{missing}/{total} metric không tìm thấy", {
                        missing: missing.length,
                        total: res.length,
                      })
                    : t("Cả {total} metric đều tồn tại trên project này", { total: res.length }),
                  body: missing.length ? missing.map((m) => m.metric).join("\n") : undefined,
                });
              } catch (e) {
                const err = e as { message?: string };
                toast({ tone: "critical", title: t("Không kiểm tra được"), body: err.message });
              } finally {
                setVerifying(false);
              }
            }}
          >
            {t("Đối chiếu với metricDescriptors")}
          </Button>
        </div>

        <div>
          <h3 className="mb-1.5 text-[12px] font-semibold">Audit log</h3>
          <p className="mb-2 text-[11px] text-[var(--ink-muted)]">
            {t(
              "Mọi thao tác ghi và mọi lần xem giá trị secret được ghi vào file JSONL trên máy (không chứa giá trị secret).",
            )}
          </p>
          <div className="flex items-center gap-2">
            <Button size="sm" onClick={async () => setAuditFile(await api.auditPath())}>
              {t("Hiện đường dẫn file")}
            </Button>
            {auditFile && <code className="selectable mono text-[11px]">{auditFile}</code>}
          </div>
        </div>
      </div>
    </Dialog>
  );
}

const ACTION_TEXT: Record<string, string> = {
  updateEnv: "Sửa env",
  updateScaling: "Sửa scaling",
  validateOnly: "Kiểm tra trước",
  revealSecret: "Xem secret",
  toggleReadOnly: "Đổi chế độ ghi",
};

function AuditDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const t = useT();
  const q = useAuditTail(open);

  return (
    <Dialog open={open} onClose={onClose} title={t("Lịch sử thao tác trên máy này")} width={860}>
      {q.isLoading && <Loading />}
      <ErrorBox error={q.error} />
      {q.data && q.data.length === 0 && (
        <Notice tone="info">{t("Chưa có thao tác nào được ghi lại.")}</Notice>
      )}
      {q.data && q.data.length > 0 && (
        <div className="flex flex-col gap-1.5">
          {(q.data as Array<Record<string, unknown>>).map((r, i) => {
            const outcome = String(r["outcome"] ?? "");
            const tone =
              outcome === "error" ? "critical" : outcome === "pending" ? "warning" : "good";
            return (
              <div key={i} className="rounded border px-2.5 py-2 text-[11px]">
                <div className="flex flex-wrap items-center gap-2">
                  <Badge tone={tone}>
                    {t(ACTION_TEXT[String(r["action"])] ?? String(r["action"]))}
                  </Badge>
                  <span className="mono">{String(r["project"] ?? "")}</span>
                  {r["service"] ? <span className="mono">/{String(r["service"])}</span> : null}
                  <span className="ml-auto text-[var(--ink-muted)]">{String(r["ts"] ?? "")}</span>
                </div>
                {Array.isArray(r["changes"]) && (r["changes"] as unknown[]).length > 0 && (
                  <ul className="selectable mono mt-1 list-disc pl-5">
                    {(r["changes"] as string[]).map((c, j) => (
                      <li key={j}>{c}</li>
                    ))}
                  </ul>
                )}
                <p className="selectable mt-1 whitespace-pre-wrap text-[var(--ink-secondary)]">
                  {String(r["message"] ?? "")}
                </p>
                <p className="mt-0.5 text-[var(--ink-muted)]">
                  {String(r["effectiveIdentity"] ?? r["account"] ?? "")}
                </p>
              </div>
            );
          })}
        </div>
      )}
      <div className="mt-3">
        <KeyValue
          items={[
            [
              t("Lưu ý"),
              t(
                "Đây là log cục bộ để tra nhanh. Nguồn chuẩn cho audit vẫn là Cloud Audit Logs trên GCP.",
              ),
            ],
          ]}
        />
      </div>
    </Dialog>
  );
}
