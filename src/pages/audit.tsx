import { useCallback, useEffect, useRef, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { listAudit } from "@/lib/api";
import { actionLabel, formatAuditTime, formatChangeText } from "@/lib/audit-view";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import type { AuditAction, AuditEntry, AuditProjectOption } from "@/types/project-audit";

const PAGE_SIZE = 200;
const ALL_PROJECTS = "__all__";
const ALL_ACTIONS = "all";
const ACTIONS: { value: AuditAction; label: string }[] = [
  { value: "create", label: "新建" },
  { value: "update", label: "编辑" },
  { value: "delete", label: "删除" },
];

export function AuditPage() {
  const [projectFilter, setProjectFilter] = useState(ALL_PROJECTS);
  const [actionFilter, setActionFilter] = useState(ALL_ACTIONS);
  const [items, setItems] = useState<AuditEntry[]>([]);
  const [projects, setProjects] = useState<AuditProjectOption[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<AppErrorShape | null>(null);
  const requestId = useRef(0);
  const offsetRef = useRef(0);

  const query = useCallback(
    (offset: number) =>
      listAudit({
        projectId: projectFilter === ALL_PROJECTS ? undefined : projectFilter,
        action: actionFilter === ALL_ACTIONS ? undefined : (actionFilter as AuditAction),
        limit: PAGE_SIZE,
        offset,
      }),
    [projectFilter, actionFilter],
  );

  const load = useCallback(
    async (reset: boolean) => {
      const token = ++requestId.current;
      setLoading(true);
      setError(null);
      if (reset) {
        setItems([]);
        setHasMore(false);
        offsetRef.current = 0;
      }
      try {
        const offset = reset ? 0 : offsetRef.current;
        const page = await query(offset);
        if (token !== requestId.current) return;
        setProjects(page.projects);
        setItems((prev) => (reset ? page.items : [...prev, ...page.items]));
        setHasMore(page.hasMore);
        offsetRef.current = offset + page.items.length;
      } catch (e) {
        if (token === requestId.current) setError(toAppError(e));
      } finally {
        if (token === requestId.current) setLoading(false);
      }
    },
    [query],
  );

  useEffect(() => {
    void load(true);
  }, [load]);

  return (
    <div className="mx-auto w-full max-w-5xl px-6 py-8">
      <div className="mb-4">
        <h1 className="text-xl font-semibold">操作日志</h1>
        <p className="text-xs text-muted-foreground">
          只记录项目基础信息的新建、编辑与删除，永久保留；其他模块的操作不在此记录。
        </p>
      </div>

      <div className="mb-4 flex flex-wrap items-end gap-3">
        <div className="grid gap-1.5">
          <Label className="text-xs text-muted-foreground">项目</Label>
          <Select value={projectFilter} onValueChange={setProjectFilter}>
            <SelectTrigger className="w-56">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL_PROJECTS}>全部项目</SelectItem>
              {projects.map((p) => (
                <SelectItem key={p.projectId} value={p.projectId}>
                  {p.projectName}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="grid gap-1.5">
          <Label className="text-xs text-muted-foreground">操作</Label>
          <Select value={actionFilter} onValueChange={setActionFilter}>
            <SelectTrigger className="w-40">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL_ACTIONS}>全部操作</SelectItem>
              {ACTIONS.map((a) => (
                <SelectItem key={a.value} value={a.value}>
                  {a.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {error && (
        <div className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          <div>{error.message}</div>
          {error.hint && <div className="mt-0.5 opacity-80">建议：{error.hint}</div>}
        </div>
      )}

      {loading && items.length === 0 ? (
        <p className="text-sm text-muted-foreground">读取日志…</p>
      ) : items.length === 0 ? (
        <div className="rounded-xl border bg-background p-10 text-center text-sm text-muted-foreground">
          还没有操作日志。
        </div>
      ) : (
        <ul className="grid gap-2">
          {items.map((entry) => (
            <li key={entry.id} className="rounded-xl border bg-background px-4 py-3">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-xs text-muted-foreground">{formatAuditTime(entry.createdAt)}</span>
                <span className="min-w-0 truncate text-sm font-medium">{entry.projectName}</span>
                <Badge
                  variant={
                    entry.action === "delete"
                      ? "destructive"
                      : entry.action === "create"
                        ? "outline"
                        : "secondary"
                  }
                >
                  {actionLabel(entry.action)}
                </Badge>
              </div>
              <ul className="mt-2 grid gap-1">
                {entry.changes.map((c, i) => (
                  <li key={`${c.field}-${i}`} className="text-xs text-muted-foreground">
                    {formatChangeText(entry.action, c)}
                  </li>
                ))}
              </ul>
            </li>
          ))}
        </ul>
      )}

      {hasMore && (
        <div className="mt-4 text-center">
          <Button variant="outline" size="sm" disabled={loading} onClick={() => void load(false)}>
            {loading ? "加载中…" : "加载更多"}
          </Button>
        </div>
      )}
    </div>
  );
}
