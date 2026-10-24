import type { AuditAction, AuditChange } from "@/types/project-audit";

const ACTION_LABELS: Record<AuditAction, string> = {
  create: "新建",
  update: "编辑",
  delete: "删除",
};

export function actionLabel(action: AuditAction): string {
  return ACTION_LABELS[action] ?? action;
}

export function formatChangeText(action: AuditAction, change: AuditChange): string {
  const oldValue = change.oldValue ?? "—";
  const newValue = change.newValue ?? "—";
  if (action === "create") return `${change.label}：${newValue}`;
  if (action === "delete") return `${change.label}：${oldValue}`;
  return `${change.label}：${oldValue} → ${newValue}`;
}

export function formatAuditTime(raw: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2}):(\d{2})$/.exec(raw.trim());
  if (!m) return raw;
  const date = new Date(
    Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3]), Number(m[4]), Number(m[5]), Number(m[6])),
  );
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
