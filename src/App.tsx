import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { CommandPalette } from "./components/CommandPalette";
import { NavRail, type View } from "./components/NavRail";
import { TopBar } from "./components/TopBar";
import { Button, EmptyState, ErrorBox, Loading, Notice } from "./components/ui";
import { BillingPage } from "./features/billing/BillingPage";
import { JobsPage } from "./features/jobs/JobsPage";
import { RecommendationsPage } from "./features/recommendations/RecommendationsPage";
import { ServiceDetailPane } from "./features/service-detail/ServiceDetail";
import { Sidebar } from "./features/service-list/Sidebar";
import { StatisticsPage } from "./features/statistics/StatisticsPage";
import { UnlockScreen } from "./features/vault/UnlockScreen";
import { useT } from "./lib/i18n";
import { api, apiV2 } from "./lib/ipc";
import {
  keys,
  useAuth,
  useCapabilities,
  useProjectLoad,
  useServices,
  useSettings,
} from "./lib/queries";
import type { CmdError, EnvLabel, ServiceSummary, VaultStatus } from "./lib/types";

type Theme = "light" | "dark" | "system";

function readTheme(): Theme {
  try {
    const v = window.localStorage.getItem("crc.theme");
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    // WebView có thể bị chặn storage; theo hệ điều hành là mặc định hợp lý.
    return "system";
  }
}

/** Bản TS của `config::suggest_label` — nhánh dev cố tình hẹp, xem chú thích bên Rust. */
function inferLabel(projectId: string): EnvLabel {
  const id = projectId.toLowerCase();
  if (["prod", "production", "master", "live", "main"].some((k) => id.includes(k))) return "prod";
  if (["stg", "staging", "stage", "uat", "preprod"].some((k) => id.includes(k))) return "staging";
  if (["dev", "develop", "sandbox", "test", "local", "demo"].some((k) => id.includes(k))) return "dev";
  return "unknown";
}

export default function App() {
  const t = useT();
  const qc = useQueryClient();
  const auth = useAuth();
  const settingsQ = useSettings();

  const [project, setProject] = useState<string | null>(null);
  const [selected, setSelected] = useState<{ region: string; name: string } | null>(null);
  const [view, setView] = useState<View>("services");
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [metricsMinutes, setMetricsMinutes] = useState(60);
  const [skipVault, setSkipVault] = useState(false);

  // Trạng thái vault: quyết định có chặn ở màn mở khoá hay không.
  const vaultQ = useQuery<VaultStatus, CmdError>({
    queryKey: ["vaultStatus"],
    queryFn: () => apiV2.vaultStatus(),
    staleTime: 30_000,
    retry: false,
  });

  // Áp theme lên <html> để CSS custom property đổi theo.
  useEffect(() => {
    const root = document.documentElement;
    if (theme === "system") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", theme);
    try {
      if (theme === "system") window.localStorage.removeItem("crc.theme");
      else window.localStorage.setItem("crc.theme", theme);
    } catch {
      // Không lưu được thì thôi, không phải lỗi đáng báo.
    }
  }, [theme]);

  const settings = settingsQ.data;

  // Chọn project ban đầu: ưu tiên project đang chọn lần trước, rồi project mặc định
  // của gcloud. Không tự đoán thêm — chọn sai project là chọn sai môi trường.
  useEffect(() => {
    if (project || !settings) return;
    const initial = settings.currentProject ?? auth.data?.defaultProject ?? null;
    if (initial) {
      setProject(initial);
      void api.selectProject(initial).then((s) => qc.setQueryData(keys.settings, s));
    }
  }, [project, settings, auth.data?.defaultProject, qc]);

  useEffect(() => {
    if (settings) setMetricsMinutes(settings.metricsWindowMinutes);
  }, [settings?.metricsWindowMinutes, settings]);

  const autoRefreshMs = (settings?.autoRefreshSeconds ?? 30) * 1000;
  const servicesQ = useServices(project, autoRefreshMs);
  const loadQ = useProjectLoad(project, autoRefreshMs);
  const capsQ = useCapabilities(project);

  const services = servicesQ.data?.services ?? [];

  // Service đang chọn không còn tồn tại (bị xoá, hoặc vừa đổi project) → bỏ chọn.
  useEffect(() => {
    if (!selected || services.length === 0) return;
    const still = services.some((s) => s.name === selected.name && s.region === selected.region);
    if (!still) setSelected(null);
  }, [services, selected]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const label: EnvLabel = useMemo(() => {
    if (!project || !settings) return "unknown";
    return settings.projectLabels[project] ?? inferLabel(project);
  }, [project, settings]);

  const requiresTypedConfirm = label === "prod" || label === "unknown";

  const pick = (s: ServiceSummary) => setSelected({ region: s.region, name: s.name });

  const openService = (sel: { region: string; name: string }) => {
    setSelected(sel);
    setView("services");
  };

  const onProjectChange = async (p: string) => {
    setProject(p);
    setSelected(null);
    qc.setQueryData(keys.settings, await api.selectProject(p));
  };

  const refreshAll = async () => {
    await api.clearCache();
    if (project) {
      await qc.invalidateQueries({ queryKey: keys.services(project) });
      await qc.invalidateQueries({ queryKey: keys.load(project) });
      // Các màn v2 dùng query riêng — làm mới luôn để nút Reload ở TopBar có tác dụng ở mọi màn.
      await qc.invalidateQueries({ queryKey: ["jobs", project] });
      await qc.invalidateQueries({ queryKey: ["cost", project] });
      await qc.invalidateQueries({ queryKey: ["recommendations", project] });
      if (selected) {
        await qc.invalidateQueries({
          queryKey: keys.service(project, selected.region, selected.name),
        });
      }
    }
  };

  if (settingsQ.isLoading) {
    return <Loading label={t("Đang khởi động…")} />;
  }

  if (!settings) {
    return (
      <div className="p-6">
        <ErrorBox error={settingsQ.error} onRetry={() => void settingsQ.refetch()} />
      </div>
    );
  }

  // Chặn ở màn mở khoá: đã có vault nhưng chưa mở, và người dùng chưa chọn bỏ qua.
  const vault = vaultQ.data;
  if (vault?.exists && !vault.unlocked && !skipVault) {
    return (
      <UnlockScreen
        status={vault}
        onUnlocked={(s) => qc.setQueryData(["vaultStatus"], s)}
        onUseGcloud={() => setSkipVault(true)}
      />
    );
  }

  return (
    <div className="flex h-full flex-col">
      <TopBar
        settings={settings}
        project={project}
        onProjectChange={(p) => void onProjectChange(p)}
        dataAgeSeconds={servicesQ.data?.ageSeconds ?? null}
        refreshing={servicesQ.isFetching || loadQ.isFetching}
        onRefresh={() => void refreshAll()}
        onOpenPalette={() => setPaletteOpen(true)}
        theme={theme}
        onThemeToggle={() =>
          setTheme((t) => (t === "system" ? "light" : t === "light" ? "dark" : "system"))
        }
      />

      {auth.isError && (
        <div className="p-4">
          <ErrorBox error={auth.error} onRetry={() => void auth.refetch()} />
        </div>
      )}

      {!project ? (
        <div className="flex flex-1 items-center justify-center">
          <EmptyState
            icon="☁"
            title={t("Chọn một GCP project để bắt đầu")}
            hint={
              auth.data
                ? t("Đang đăng nhập với {account}. Chọn project ở thanh trên.", {
                    account: auth.data.impersonating ?? auth.data.account,
                  })
                : t("Đang lấy thông tin xác thực từ gcloud…")
            }
          />
        </div>
      ) : (
        <div className="flex min-h-0 flex-1">
          <NavRail view={view} onChange={setView} />

          {view === "services" ? (
            <div className="flex min-h-0 flex-1">
              <Sidebar
                services={services}
                load={loadQ.data}
                loading={servicesQ.isLoading}
                error={servicesQ.error}
                selected={selected}
                onSelect={pick}
              />

              <main className="flex min-h-0 min-w-0 flex-1 flex-col">
                {selected ? (
                  <ServiceDetailPane
                    project={project}
                    region={selected.region}
                    service={selected.name}
                    load={loadQ.data}
                    caps={capsQ.data}
                    readOnly={settings.readOnly}
                    requiresTypedConfirm={requiresTypedConfirm}
                    metricsMinutes={metricsMinutes}
                    onMetricsMinutesChange={setMetricsMinutes}
                    logPollSeconds={settings.logPollSeconds}
                    autoRefreshMs={autoRefreshMs}
                  />
                ) : (
                  <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6">
                    <EmptyState
                      icon="◧"
                      title={
                        services.length > 0
                          ? t("{count} service trong {project}", {
                              count: services.length,
                              project,
                            })
                          : t("Không có Cloud Run service nào trong {project}", { project })
                      }
                      hint={
                        services.length > 0
                          ? t("Chọn một service ở cột bên trái, hoặc bấm Ctrl+K để nhảy nhanh.")
                          : t(
                              "Kiểm tra lại project — hoặc region của service có thể chưa được Cloud Run Admin API trả về.",
                            )
                      }
                    />
                    {services.length > 0 && (
                      <Button onClick={() => setPaletteOpen(true)}>
                        {t("Ctrl+K — nhảy tới service")}
                      </Button>
                    )}
                    {label === "unknown" && (
                      <div className="max-w-xl">
                        <Notice tone="warning" icon="?">
                          {t("Project")} <strong>{project}</strong>{" "}
                          {t(
                            "chưa được gắn nhãn môi trường. App đang xử lý như production: mọi thao tác ghi sẽ yêu cầu gõ đúng tên service. Gắn nhãn ở thanh trên để bỏ bước đó trên project dev.",
                          )}
                        </Notice>
                      </div>
                    )}
                  </div>
                )}
              </main>
            </div>
          ) : view === "statistics" ? (
            <StatisticsPage project={project} autoRefreshMs={autoRefreshMs} onOpenService={openService} />
          ) : view === "jobs" ? (
            <JobsPage project={project} readOnly={settings.readOnly} requiresTypedConfirm={requiresTypedConfirm} />
          ) : view === "billing" ? (
            <BillingPage project={project} />
          ) : (
            <RecommendationsPage project={project} readOnly={settings.readOnly} />
          )}
        </div>
      )}

      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        services={services}
        onPick={pick}
      />
    </div>
  );
}
