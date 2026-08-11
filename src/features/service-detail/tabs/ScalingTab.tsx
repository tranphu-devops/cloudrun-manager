import { useEffect, useState } from "react";

import { ApplyDialog } from "../../../components/ApplyDialog";
import { Badge, Button, Card, Field, Input, Notice, Select, useToast } from "../../../components/ui";
import { api, asCmdError } from "../../../lib/ipc";
import { useInvalidateService } from "../../../lib/queries";
import type {
  ApplyPreview,
  ApplyResult,
  CmdError,
  ScalingUpdate,
  ServiceDetail,
} from "../../../lib/types";

/** Giá trị CPU Cloud Run cho phép. */
const CPU_OPTIONS = ["0.08", "0.25", "0.5", "1", "2", "4", "6", "8"];
const MEM_OPTIONS = ["128Mi", "256Mi", "512Mi", "1Gi", "2Gi", "4Gi", "8Gi", "16Gi", "32Gi"];

export function ScalingTab({
  project,
  detail,
  containerIndex,
  readOnly,
  requiresTypedConfirm,
}: {
  project: string;
  detail: ServiceDetail;
  containerIndex: number;
  readOnly: boolean;
  requiresTypedConfirm: boolean;
}) {
  const toast = useToast();
  const invalidate = useInvalidateService();
  const c = detail.containers[containerIndex];
  const s = detail.summary;

  const [minI, setMinI] = useState(String(s.minInstances ?? 0));
  const [maxI, setMaxI] = useState(String(s.maxInstances ?? 100));
  const [cpu, setCpu] = useState(c?.cpu ?? "1");
  const [mem, setMem] = useState(c?.memory ?? "512Mi");
  const [conc, setConc] = useState(String(detail.concurrency ?? 80));
  const [timeout, setTimeoutVal] = useState(detail.timeout ?? "300s");
  const [cpuIdle, setCpuIdle] = useState(c?.cpuIdle ?? true);
  const [boost, setBoost] = useState(c?.startupCpuBoost ?? false);

  const [open, setOpen] = useState(false);
  const [preview, setPreview] = useState<ApplyPreview | null>(null);
  const [previewError, setPreviewError] = useState<CmdError | null>(null);
  const [applyError, setApplyError] = useState<CmdError | null>(null);
  const [result, setResult] = useState<ApplyResult | null>(null);
  const [busy, setBusy] = useState(false);

  // Bám lại theo dữ liệu thật khi service/etag đổi.
  useEffect(() => {
    setMinI(String(s.minInstances ?? 0));
    setMaxI(String(s.maxInstances ?? 100));
    setCpu(c?.cpu ?? "1");
    setMem(c?.memory ?? "512Mi");
    setConc(String(detail.concurrency ?? 80));
    setTimeoutVal(detail.timeout ?? "300s");
    setCpuIdle(c?.cpuIdle ?? true);
    setBoost(c?.startupCpuBoost ?? false);
  }, [detail.etag, s.minInstances, s.maxInstances, detail.concurrency, detail.timeout, c]);

  const update: ScalingUpdate = {
    minInstances: Number(minI),
    maxInstances: Number(maxI),
    cpu,
    memory: mem,
    concurrency: Number(conc),
    timeout,
    cpuIdle,
    startupCpuBoost: boost,
  };

  const minNum = Number(minI);
  const maxNum = Number(maxI);
  const localInvalid =
    !Number.isFinite(minNum) ||
    !Number.isFinite(maxNum) ||
    minNum < 0 ||
    maxNum < 1 ||
    minNum > maxNum;

  const openDialog = async () => {
    setPreview(null);
    setPreviewError(null);
    setApplyError(null);
    setResult(null);
    setOpen(true);
    try {
      setPreview(
        await api.previewScaling({
          project,
          region: s.region,
          service: s.name,
          containerIndex,
          update,
        }),
      );
    } catch (e) {
      setPreviewError(asCmdError(e));
    }
  };

  const doApply = async (confirmText: string | null, validateOnly: boolean) => {
    setBusy(true);
    setApplyError(null);
    setResult(null);
    try {
      const r = await api.applyScaling({
        project,
        region: s.region,
        service: s.name,
        containerIndex,
        update,
        expectedEtag: detail.etag,
        confirmText,
        validateOnly,
      });
      setResult(r);
      if (!validateOnly) {
        invalidate(project, s.region, s.name);
        toast({ tone: "good", title: `Đã cập nhật scaling của ${s.name}`, body: r.outcome.message });
      }
    } catch (e) {
      setApplyError(asCmdError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-3">
      {minNum > 0 && (
        <Notice tone="info" icon="💰">
          Min instances = {minNum} nghĩa là luôn có {minNum} instance chạy và <strong>được tính tiền
          24/7</strong>, kể cả khi không có request. Đây là cách đổi tiền lấy việc loại bỏ cold start.
        </Notice>
      )}

      {!cpuIdle && (
        <Notice tone="warning" icon="💰">
          “CPU luôn được cấp” tính tiền CPU cho toàn bộ thời gian instance tồn tại, không chỉ lúc xử
          lý request. Chỉ cần bật khi app có việc chạy nền ngoài request (worker, cron trong process).
        </Notice>
      )}

      <div className="grid grid-cols-2 gap-3">
        <Card title="Scaling">
          <div className="grid grid-cols-2 gap-3">
            <Field
              label="Min instances"
              hint={minNum === 0 ? "0 = scale về 0 khi rảnh, có cold start" : "luôn chạy, tính tiền 24/7"}
            >
              <Input
                value={minI}
                inputMode="numeric"
                invalid={localInvalid}
                onChange={(e) => setMinI(e.target.value)}
              />
            </Field>
            <Field label="Max instances" hint="chặn trên để không bị hoá đơn bất ngờ">
              <Input
                value={maxI}
                inputMode="numeric"
                invalid={localInvalid}
                onChange={(e) => setMaxI(e.target.value)}
              />
            </Field>
            <Field label="Concurrency" hint="số request đồng thời mỗi instance (1–1000)">
              <Input value={conc} inputMode="numeric" onChange={(e) => setConc(e.target.value)} />
            </Field>
            <Field label="Request timeout" hint="ví dụ 300s, 5m — tối đa 3600s">
              <Input
                className="mono"
                value={timeout}
                onChange={(e) => setTimeoutVal(e.target.value)}
              />
            </Field>
          </div>
          {localInvalid && (
            <p className="mt-2 text-[11px]" style={{ color: "var(--status-critical)" }}>
              Min phải ≥ 0, max phải ≥ 1, và min không được lớn hơn max.
            </p>
          )}
        </Card>

        <Card title={`Resource — container ${c?.name ?? containerIndex + 1}`}>
          <div className="grid grid-cols-2 gap-3">
            <Field label="CPU">
              <Select value={cpu} onChange={(e) => setCpu(e.target.value)}>
                {[...new Set([cpu, ...CPU_OPTIONS])].map((v) => (
                  <option key={v} value={v}>
                    {v}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Memory">
              <Select value={mem} onChange={(e) => setMem(e.target.value)}>
                {[...new Set([mem, ...MEM_OPTIONS])].map((v) => (
                  <option key={v} value={v}>
                    {v}
                  </option>
                ))}
              </Select>
            </Field>
          </div>

          <div className="mt-3 flex flex-col gap-2">
            <label className="flex items-start gap-2 text-[12px]">
              <input
                type="checkbox"
                className="mt-0.5"
                checked={!cpuIdle}
                onChange={(e) => setCpuIdle(!e.target.checked)}
              />
              <span>
                CPU luôn được cấp
                <span className="block text-[11px] text-[var(--ink-muted)]">
                  Mặc định Cloud Run chỉ cấp CPU khi đang xử lý request.
                </span>
              </span>
            </label>
            <label className="flex items-start gap-2 text-[12px]">
              <input
                type="checkbox"
                className="mt-0.5"
                checked={boost}
                onChange={(e) => setBoost(e.target.checked)}
              />
              <span>
                Startup CPU boost
                <span className="block text-[11px] text-[var(--ink-muted)]">
                  Tăng CPU trong lúc khởi động để giảm cold start.
                </span>
              </span>
            </label>
          </div>

          <p className="mt-3 text-[11px] leading-relaxed text-[var(--ink-muted)]">
            Cloud Run có ràng buộc giữa CPU và memory (ví dụ CPU ≥ 4 cần memory ≥ 2Gi). App không đoán
            trước các ràng buộc này — bấm <strong>Kiểm tra trước</strong> để Cloud Run tự xác nhận mà
            không tạo revision.
          </p>
        </Card>
      </div>

      <div className="flex items-center gap-2">
        {detail.executionEnvironment && <Badge>{detail.executionEnvironment}</Badge>}
        <Button
          className="ml-auto"
          variant="primary"
          disabled={localInvalid}
          onClick={openDialog}
        >
          Xem thay đổi & áp dụng
        </Button>
      </div>

      <ApplyDialog
        open={open}
        onClose={() => setOpen(false)}
        title={`Áp dụng scaling / resource — ${s.name}`}
        serviceName={s.name}
        requiresTypedConfirm={requiresTypedConfirm}
        preview={preview}
        previewError={previewError}
        busy={busy}
        error={applyError}
        result={result}
        onApply={doApply}
        canWrite={!readOnly}
      />
    </div>
  );
}
