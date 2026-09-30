import { useEffect, useState } from "react";
import { Link } from "@tanstack/react-router";
import { Copy, FolderOpen, Pencil, Plus, Trash2 } from "lucide-react";
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
import { Separator } from "@/components/ui/separator";
import { copyText, openFolder } from "@/lib/api";
import { useProjectsStore } from "@/stores/projects";
import { toInput, type Project, type ProjectInput } from "@/types/project";

export function ProjectsPage() {
  const { items, statuses, loading, saving, error, refresh, save, remove } = useProjectsStore();
  const [editing, setEditing] = useState<{ id: string | null; input: ProjectInput } | null>(null);
  const [confirming, setConfirming] = useState<Project | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const openRoot = async (p: Project) => {
    const root = p.dirs.find((d) => d.kind === "root");
    if (!root) return;
    try {
      await openFolder(root.path);
    } catch (e) {
      setNotice(`打开失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const copyRoot = async (p: Project) => {
    const root = p.dirs.find((d) => d.kind === "root");
    if (!root) return;
    await copyText(root.path);
    setCopiedId(p.id);
    setTimeout(() => setCopiedId((cur) => (cur === p.id ? null : cur)), 1500);
  };

  return (
    <div className="mx-auto w-full max-w-6xl px-6 py-8">
      <div className="mb-4 flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold">项目档案</h1>
          <p className="text-xs text-muted-foreground">
            仅登记本地路径，软件不会移动、修改或复制磁盘上的原始文件。
          </p>
        </div>
        <Button size="sm" onClick={() => setEditing({ id: null, input: toInput(emptySeed()) })}>
          <Plus /> 新建项目
        </Button>
      </div>

      {error && (
        <div className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          <div>{error.message}</div>
          {error.hint && <div className="mt-0.5 opacity-80">建议：{error.hint}</div>}
        </div>
      )}
      {notice && <div className="mb-3 text-xs text-destructive">{notice}</div>}

      {loading ? (
        <p className="text-sm text-muted-foreground">读取项目台账…</p>
      ) : items.length === 0 ? (
        <div className="rounded-xl border bg-background p-10 text-center text-sm text-muted-foreground">
          还没有项目档案，点右上角「新建项目」开始登记。
        </div>
      ) : (
        <ul className="grid gap-2">
          {items.map((p) => {
            const root = p.dirs.find((d) => d.kind === "root");
            return (
              <li
                key={p.id}
                className="grid grid-cols-[minmax(0,1.4fr)_auto] items-center gap-3 rounded-xl border bg-background px-4 py-3"
              >
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <Link
                      to="/projects/$projectId"
                      params={{ projectId: p.id }}
                      className="truncate text-sm font-medium hover:underline"
                    >
                      {p.name}
                    </Link>
                    <Badge variant="secondary">{p.status}</Badge>
                    {p.tags.map((t) => (
                      <Badge key={t} variant="outline">
                        {t}
                      </Badge>
                    ))}
                  </div>
                  <div className="mt-1 flex items-center gap-2 text-xs text-muted-foreground">
                    <span>{[p.code, p.customer, p.manager].filter(Boolean).join(" · ") || "—"}</span>
                    {root && (
                      <span
                        className={`max-w-[46ch] truncate font-mono ${root.exists ? "" : "text-destructive"}`}
                        title={root.exists ? root.path : `${root.path}（当前不可达）`}
                      >
                        {root.path}
                      </span>
                    )}
                  </div>
                </div>

                <div className="flex items-center gap-1">
                  <Button
                    size="icon"
                    variant="ghost"
                    disabled={!root}
                    title="打开主根目录"
                    onClick={() => void openRoot(p)}
                  >
                    <FolderOpen />
                  </Button>
                  <Button
                    size="icon"
                    variant="ghost"
                    disabled={!root}
                    title={copiedId === p.id ? "已复制" : "复制路径"}
                    onClick={() => void copyRoot(p)}
                  >
                    <Copy />
                  </Button>
                  <Button
                    size="icon"
                    variant="ghost"
                    title="编辑"
                    onClick={() => setEditing({ id: p.id, input: toInput(p) })}
                  >
                    <Pencil />
                  </Button>
                  <Button
                    size="icon"
                    variant="ghost"
                    title="删除"
                    onClick={() => setConfirming(p)}
                  >
                    <Trash2 />
                  </Button>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      <Dialog open={editing !== null} onOpenChange={(o) => !o && setEditing(null)}>
        <DialogContent className="max-h-[90vh] overflow-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{editing?.id ? "编辑项目" : "新建项目"}</DialogTitle>
            <DialogDescription>基础信息会存进本地库，敏感凭据在信息台账里单独加密存储。</DialogDescription>
          </DialogHeader>
          {editing && (
            <ProjectForm
              initial={editing.input}
              statuses={statuses}
              saving={saving}
              onCancel={() => setEditing(null)}
              onSubmit={async (input) => {
                const ok = await save(editing.id, input);
                if (ok) setEditing(null);
                return ok;
              }}
            />
          )}
        </DialogContent>
      </Dialog>

      <Dialog open={confirming !== null} onOpenChange={(o) => !o && setConfirming(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>删除项目档案</DialogTitle>
            <DialogDescription>
              只删除「{confirming?.name}」在软件里的登记信息，磁盘上的文件与目录不会被动到。
            </DialogDescription>
          </DialogHeader>
          <Separator />
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
    </div>
  );
}

// 新建时表单的初值：状态默认取枚举首项，避免把空串塞进 select
function emptySeed(): Project {
  return {
    id: "",
    name: "",
    code: null,
    manager: null,
    customer: null,
    contactName: null,
    contactPhone: null,
    contractNo: null,
    contractPeriod: null,
    deliveryDeadline: null,
    status: "立项",
    tags: [],
    dirs: [],
  };
}
