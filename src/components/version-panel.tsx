// 项目版本台账（M16-a）。本地 state，照 milestone-panel 的形制：不加 zustand store。
// 版本号与上线时间必填、内容可空；日期用 type="date"，空串折成 null 再发请求。
import { useCallback, useEffect, useState } from "react";
import { Pencil, Plus, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { createVersion, deleteVersion, listVersions, updateVersion } from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import type { Version } from "@/types/version";

const dateOrNull = (raw: string): string | null => (raw.trim() === "" ? null : raw.trim());

export function VersionPanel({ projectId }: { projectId: string }) {
  const [rows, setRows] = useState<Version[]>([]);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [version, setVersion] = useState("");
  const [releasedAt, setReleasedAt] = useState("");
  const [content, setContent] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setRows(await listVersions(projectId));
      setError(null);
    } catch (e) {
      setError(toAppError(e));
    }
  }, [projectId]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const reset = () => {
    setEditingId(null);
    setVersion("");
    setReleasedAt("");
    setContent("");
  };

  // run 回报成败（同 milestone-panel）：异常吞进 state，promise 永不 reject。
  const run = async (fn: () => Promise<unknown>, okText: string): Promise<boolean> => {
    setBusy(true);
    setNotice(null);
    try {
      await fn();
      await reload();
      setNotice(okText);
      return true;
    } catch (e) {
      setError(toAppError(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    const fields = {
      version,
      releasedAt: dateOrNull(releasedAt),
      content: content.trim() === "" ? null : content,
    };
    const ok = editingId
      ? await run(() => updateVersion(editingId, fields), "版本已更新")
      : await run(() => createVersion({ projectId, ...fields }), "版本已新增");
    if (ok) reset();
  };

  const startEdit = (r: Version) => {
    setEditingId(r.id);
    setVersion(r.version);
    setReleasedAt(r.releasedAt);
    setContent(r.content);
    setNotice(null);
  };

  return (
    <div>
      <div className="mb-1 flex items-center justify-between">
        <h2 className="text-sm font-medium">项目版本台账</h2>
        <span className="text-xs text-muted-foreground">版本号与上线时间必填；同项目版本号唯一（删除后可重建）</span>
      </div>

      {error && (
        <div className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}
      {notice && !error && <p className="mb-3 text-xs text-muted-foreground">{notice}</p>}

      <div className="mb-3 grid gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <Input
            value={version}
            onChange={(e) => setVersion(e.target.value)}
            placeholder="版本号，如 1.2.0"
            className="w-40"
          />
          <Input
            type="date"
            value={releasedAt}
            onChange={(e) => setReleasedAt(e.target.value)}
            className="w-44"
            aria-label="上线时间"
          />
          <Button size="sm" disabled={busy || !version.trim() || !releasedAt} onClick={() => void save()}>
            <Plus /> {editingId ? "保存" : "记一笔"}
          </Button>
          {editingId && (
            <Button size="sm" variant="ghost" disabled={busy} onClick={reset}>
              <X /> 取消
            </Button>
          )}
        </div>
        <textarea
          value={content}
          onChange={(e) => setContent(e.target.value)}
          placeholder="本次上线内容（可留空）"
          className="min-h-20 w-full rounded-md border bg-transparent px-3 py-2 text-sm outline-none focus-visible:border-ring"
        />
      </div>

      {rows.length === 0 && <p className="py-4 text-center text-sm text-muted-foreground">还没有版本记录。</p>}

      {rows.map((r) => (
        <div key={r.id} className="flex items-start gap-2 border-b px-1 py-2 last:border-b-0">
          <span className="w-28 shrink-0 font-mono text-xs">{r.version}</span>
          <span className="w-24 shrink-0 font-mono text-xs text-muted-foreground">{r.releasedAt}</span>
          <span className="min-w-0 flex-1 whitespace-pre-wrap text-sm">{r.content || "—"}</span>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => startEdit(r)}>
            <Pencil /> 编辑
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={() => void run(() => deleteVersion(r.id), "版本已删除")}
          >
            <Trash2 />
          </Button>
        </div>
      ))}
    </div>
  );
}
