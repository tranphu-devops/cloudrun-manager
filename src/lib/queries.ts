/**
 * Hook TanStack Query cho mọi dữ liệu đọc.
 *
 * `staleTime` ở đây cố ý khớp với TTL cache bên Rust (`crates/gcp/src/lib.rs::ttl`).
 * Nếu để frontend refetch dày hơn cache backend thì mỗi lần đổi tab lại là một round-trip
 * IPC vô ích; nếu để thưa hơn thì người dùng thấy số cũ hơn thực tế mà không biết.
 */

import { useQuery, useQueryClient, type UseQueryOptions } from "@tanstack/react-query";

import { api } from "./ipc";
import type {
  CmdError,
  ProjectLoadSnapshot,
  RevisionInfo,
  SecretInfo,
  ServiceCharts,
  ServiceDetail,
  ServiceListResult,
} from "./types";

/**
 * Mọi lỗi đi qua IPC đều là `CmdError` (xem `asCmdError`), nên khai báo một lần ở đây
 * thay vì phải truyền type param cho từng `useQuery`. Thiếu khai báo này thì
 * `query.error` ra `Error` và `<ErrorBox>` không nhận.
 */
declare module "@tanstack/react-query" {
  interface Register {
    defaultError: CmdError;
  }
}

const SEC = 1000;

type Opt<T> = Omit<UseQueryOptions<T, CmdError>, "queryKey" | "queryFn">;

export const keys = {
  auth: ["auth"] as const,
  settings: ["settings"] as const,
  projects: ["projects"] as const,
  caps: (p: string) => ["caps", p] as const,
  services: (p: string) => ["services", p] as const,
  load: (p: string) => ["load", p] as const,
  service: (p: string, r: string, s: string) => ["service", p, r, s] as const,
  revisions: (p: string, r: string, s: string) => ["revisions", p, r, s] as const,
  charts: (p: string, r: string, s: string, m: number) => ["charts", p, r, s, m] as const,
  secrets: (p: string) => ["secrets", p] as const,
  secretVersions: (p: string, s: string) => ["secretVersions", p, s] as const,
  audit: ["audit"] as const,
};

export function useAuth() {
  return useQuery({
    queryKey: keys.auth,
    queryFn: api.authInfo,
    staleTime: 5 * 60 * SEC,
    retry: false,
  });
}

export function useSettings() {
  return useQuery({
    queryKey: keys.settings,
    queryFn: api.getSettings,
    staleTime: Infinity,
  });
}

export function useProjects() {
  return useQuery({
    queryKey: keys.projects,
    queryFn: api.listProjects,
    staleTime: 30 * 60 * SEC,
    retry: false,
  });
}

export function useCapabilities(project: string | null) {
  return useQuery({
    queryKey: keys.caps(project ?? ""),
    queryFn: () => api.checkPermissions(project as string),
    enabled: !!project,
    staleTime: 10 * 60 * SEC,
    retry: false,
  });
}

export function useServices(project: string | null, refreshMs: number) {
  return useQuery<ServiceListResult, CmdError>({
    queryKey: keys.services(project ?? ""),
    queryFn: () => api.listServices(project as string),
    enabled: !!project,
    staleTime: 30 * SEC,
    // 0 = tắt auto refresh. `false` chứ không phải 0 — react-query coi 0 là "liên tục".
    refetchInterval: refreshMs > 0 ? refreshMs : false,
    // Không poll khi cửa sổ mất focus: app này bị mở cả ngày, poll ngầm chỉ tốn quota.
    refetchIntervalInBackground: false,
    retry: false,
  });
}

export function useProjectLoad(project: string | null, refreshMs: number) {
  return useQuery<ProjectLoadSnapshot, CmdError>({
    queryKey: keys.load(project ?? ""),
    queryFn: () => api.projectLoad(project as string, 30),
    enabled: !!project,
    staleTime: 60 * SEC,
    refetchInterval: refreshMs > 0 ? Math.max(refreshMs, 60 * SEC) : false,
    refetchIntervalInBackground: false,
    retry: false,
  });
}

export function useService(
  project: string | null,
  region: string | null,
  service: string | null,
  opts?: Opt<ServiceDetail>,
) {
  return useQuery<ServiceDetail, CmdError>({
    queryKey: keys.service(project ?? "", region ?? "", service ?? ""),
    queryFn: () => api.getService(project as string, region as string, service as string),
    enabled: !!project && !!region && !!service,
    staleTime: 15 * SEC,
    retry: false,
    ...opts,
  });
}

export function useRevisions(
  project: string | null,
  region: string | null,
  service: string | null,
  enabled = true,
) {
  return useQuery<RevisionInfo[], CmdError>({
    queryKey: keys.revisions(project ?? "", region ?? "", service ?? ""),
    queryFn: () => api.listRevisions(project as string, region as string, service as string),
    enabled: enabled && !!project && !!region && !!service,
    staleTime: 30 * SEC,
    retry: false,
  });
}

export function useCharts(
  project: string | null,
  region: string | null,
  service: string | null,
  minutes: number,
  enabled: boolean,
  refreshMs: number,
) {
  return useQuery<ServiceCharts, CmdError>({
    queryKey: keys.charts(project ?? "", region ?? "", service ?? "", minutes),
    queryFn: () =>
      api.serviceCharts(project as string, region as string, service as string, minutes),
    enabled: enabled && !!project && !!region && !!service,
    staleTime: 60 * SEC,
    refetchInterval: refreshMs > 0 ? Math.max(refreshMs, 60 * SEC) : false,
    refetchIntervalInBackground: false,
    retry: false,
  });
}

export function useSecrets(project: string | null, enabled: boolean) {
  return useQuery<SecretInfo[], CmdError>({
    queryKey: keys.secrets(project ?? ""),
    queryFn: () => api.listSecrets(project as string),
    enabled: enabled && !!project,
    staleTime: 5 * 60 * SEC,
    retry: false,
  });
}

export function useSecretVersions(project: string | null, secret: string | null) {
  return useQuery({
    queryKey: keys.secretVersions(project ?? "", secret ?? ""),
    queryFn: () => api.listSecretVersions(project as string, secret as string),
    enabled: !!project && !!secret,
    staleTime: 5 * 60 * SEC,
    retry: false,
  });
}

export function useAuditTail(enabled: boolean) {
  return useQuery({
    queryKey: keys.audit,
    queryFn: () => api.auditTail(300),
    enabled,
    staleTime: 5 * SEC,
    retry: false,
  });
}

/**
 * Bỏ mọi query liên quan tới một service sau khi ghi thành công.
 *
 * Phải xoá cả danh sách service của project: sửa env đổi cả revision đang chạy nên
 * badge ở sidebar cũng không còn đúng.
 */
export function useInvalidateService() {
  const qc = useQueryClient();
  return (project: string, region: string, service: string) => {
    void qc.invalidateQueries({ queryKey: keys.service(project, region, service) });
    void qc.invalidateQueries({ queryKey: keys.revisions(project, region, service) });
    void qc.invalidateQueries({ queryKey: keys.services(project) });
    void qc.invalidateQueries({ queryKey: keys.audit });
  };
}
