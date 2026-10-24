import { useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { deleteGuard, nameHint } from "@/lib/workspace-view";
import { useWorkspacesStore } from "@/stores/workspaces";
import type { Workspace } from "@/types/workspace";

export function WorkspaceManager({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { items, activeId, loading, saving, error, refresh, create, rename, remove } =
    useWorkspacesStore();
  const [draft, setDraft] = useState("");
  const [editing, setEditing] = useState<{ id: string; name: string } | null>(null);
  const [confirming, setConfirming] = useState<Workspace | null>(null);
  const draftHint = draft.trim() ? nameHint(draft) : null;

  useEffect(() => {
    if (open) void refresh();
  }, [open, refresh]);

  const close = () => {
    setEditing(null);
    setConfirming(null);
    setDraft("");
    onOpenChange(false);
  };

  return (
    <>
      <Dialog open={open} onOpenChange={(o) => (o ? onOpenChange(true) : close())}>
        <DialogContent className="max-h-[90vh] overflow-auto sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>管理工作区</DialogTitle>
            <DialogDescription>
              工作区把项目按客户或类型分开；切换工作区等于换整个视野。这是逻辑分类，
              不是访问控制 —— 数据都在同一个本地库里。
            </DialogDescription>
          </DialogHeader>

          {error && (
            <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              <div>{error.message}</div>
              {error.hint && <div className="mt-0.5 opacity-80">建议：{error.hint}</div>}
            </div>
          )}

          <div className="flex items-end gap-2">
            <div className="grid flex-1 gap-1.5">
              <Input
                value={draft}
                placeholder="新工作区名称，例：客户乙"
                onChange={(e) => setDraft(e.target.value)}
              />
              {draftHint && <p className="text-xs text-destructive">{draftHint}</p>}
            </div>
            <Button
              size="sm"
              disabled={saving || draftHint !== null || !draft.trim()}
              onClick={async () => {
                if (await create(draft)) setDraft("");
              }}
            >
              <Plus /> 新建
            </Button>
          </div>

          {loading ? (
            <p className="text-sm text-muted-foreground">读取工作区…</p>
          ) : items.length === 0 ? (
            <div className="rounded-xl border bg-background p-8 text-center text-sm text-muted-foreground">
              还没有工作区。
            </div>
          ) : (
            <ul className="grid gap-2">
              {items.map((w) => {
                const reason = deleteGuard(w, activeId);
                return (
                  <li key={w.id} className="flex items-center gap-2 rounded-xl border bg-background px-3 py-2">
                    <div className="min-w-0 flex-1">
                      {editing?.id === w.id ? (
                        <>
                          <Input
                            value={editing.name}
                            autoFocus
                            onChange={(e) => setEditing({ id: w.id, name: e.target.value })}
                            onKeyDown={async (e) => {
                              if (e.key === "Enter" && !nameHint(editing.name)) {
                                if (await rename(w.id, editing.name)) setEditing(null);
                              } else if (e.key === "Escape") {
                                setEditing(null);
                              }
                            }}
                          />
                          {nameHint(editing.name) && (
                            <p className="mt-1 text-xs text-destructive">{nameHint(editing.name)}</p>
                          )}
                        </>
                      ) : (
                        <div className="flex items-center gap-2">
                          <span className="truncate text-sm font-medium">{w.name}</span>
                          {w.isActive && <Badge variant="secondary">当前</Badge>}
                          <span className="text-xs text-muted-foreground">{w.projectCount} 个项目</span>
                        </div>
                      )}
                    </div>
                    <Button size="sm" variant="ghost" disabled={saving} onClick={() => setEditing({ id: w.id, name: w.name })}>
                      改名
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      title={reason ?? "删除"}
                      disabled={loading || reason !== null}
                      onClick={() => setConfirming(w)}
                    >
                      <Trash2 />
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}
        </DialogContent>
      </Dialog>

      <Dialog open={confirming !== null} onOpenChange={(o) => !o && setConfirming(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>删除工作区</DialogTitle>
            <DialogDescription>
              只删除空的「{confirming?.name}」；里面已经有项目时删不掉，项目本身也不会被删。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setConfirming(null)}>
              取消
            </Button>
            <Button
              variant="destructive"
              disabled={saving}
              onClick={async () => {
                if (!confirming) return;
                if (await remove(confirming.id)) setConfirming(null);
              }}
            >
              确认删除
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
