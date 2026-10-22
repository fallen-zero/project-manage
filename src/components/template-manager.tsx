import { useEffect, useState } from "react";
import { Pencil, Plus, Trash2 } from "lucide-react";
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
import { ProjectForm } from "@/components/project-form";
import { useTemplatesStore } from "@/stores/templates";
import { EMPTY_TEMPLATE_INPUT, type ProjectTemplate } from "@/types/project-template";
import type { ProjectInput } from "@/types/project";

// 模板名放进 ProjectInput.name 这一格；applyTemplate 从不读 template.name 写项目名。
function toForm(t: ProjectTemplate): ProjectInput {
  return {
    name: t.name,
    code: t.code ?? "",
    manager: t.manager ?? "",
    customer: t.customer ?? "",
    contactName: t.contactName ?? "",
    contactPhone: t.contactPhone ?? "",
    contractNo: t.contractNo ?? "",
    contractPeriod: t.contractPeriod ?? "",
    deliveryDeadline: t.deliveryDeadline ?? "",
    status: t.status,
    tags: t.tags,
  };
}

export function TemplateManager({
  open,
  onOpenChange,
  statuses,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  statuses: string[];
}) {
  const { items, loading, saving, error, refresh, save, remove } = useTemplatesStore();
  const [editing, setEditing] = useState<{ id: string | null; input: ProjectInput } | null>(null);
  const [confirming, setConfirming] = useState<ProjectTemplate | null>(null);

  useEffect(() => {
    if (open) void refresh();
  }, [open, refresh]);

  const close = () => {
    setEditing(null);
    setConfirming(null);
    onOpenChange(false);
  };

  return (
    <>
      <Dialog open={open} onOpenChange={(o) => (o ? onOpenChange(true) : close())}>
        <DialogContent className="max-h-[90vh] overflow-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{editing ? (editing.id ? "编辑模板" : "新建模板") : "项目模板"}</DialogTitle>
            <DialogDescription>
              模板只保存项目基础信息，新建项目时可以一键填充；不包含项目名、台账或目录。
            </DialogDescription>
          </DialogHeader>

          {error && (
            <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              <div>{error.message}</div>
              {error.hint && <div className="mt-0.5 opacity-80">建议：{error.hint}</div>}
            </div>
          )}

          {editing ? (
            <ProjectForm
              initial={editing.input}
              statuses={statuses}
              saving={saving}
              nameLabel="模板名称"
              nameRequiredMessage="模板名称不能为空"
              onCancel={() => setEditing(null)}
              onSubmit={async (input) => {
                const ok = await save(editing.id, input);
                if (ok) setEditing(null);
                return ok;
              }}
            />
          ) : (
            <div className="space-y-2">
              <div className="flex justify-end">
                <Button size="sm" onClick={() => setEditing({ id: null, input: { ...EMPTY_TEMPLATE_INPUT, tags: [] } })}>
                  <Plus /> 新建模板
                </Button>
              </div>
              {loading ? (
                <p className="text-sm text-muted-foreground">读取模板…</p>
              ) : items.length === 0 ? (
                <div className="rounded-xl border bg-background p-8 text-center text-sm text-muted-foreground">
                  还没有模板，点「新建模板」开始。
                </div>
              ) : (
                <ul className="grid gap-2">
                  {items.map((t) => (
                    <li key={t.id} className="flex items-center gap-2 rounded-xl border bg-background px-3 py-2">
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <span className="truncate text-sm font-medium">{t.name}</span>
                          <Badge variant="secondary">{t.status}</Badge>
                          {t.tags.map((tag) => (
                            <Badge key={tag} variant="outline">
                              {tag}
                            </Badge>
                          ))}
                        </div>
                        <p className="mt-1 truncate text-xs text-muted-foreground">
                          {[t.code, t.manager, t.customer, t.contractNo].filter(Boolean).join(" · ") || "—"}
                        </p>
                      </div>
                      <Button size="icon" variant="ghost" title="编辑" onClick={() => setEditing({ id: t.id, input: toForm(t) })}>
                        <Pencil />
                      </Button>
                      <Button size="icon" variant="ghost" title="删除" onClick={() => setConfirming(t)}>
                        <Trash2 />
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </DialogContent>
      </Dialog>

      <Dialog open={confirming !== null} onOpenChange={(o) => !o && setConfirming(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>删除模板</DialogTitle>
            <DialogDescription>只删除模板「{confirming?.name}」，不会影响已创建的项目。</DialogDescription>
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
