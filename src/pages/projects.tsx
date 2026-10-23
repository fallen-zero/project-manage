import { useEffect, useState } from "react";
import { Plus } from "lucide-react";
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
import { ProjectRow } from "@/components/project-row";
import { TemplateManager } from "@/components/template-manager";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { applyTemplate } from "@/lib/apply-template";
import {
  buildProjectForest,
  flattenProjectTree,
  parentCandidates,
  type ProjectTreeNode,
} from "@/lib/project-tree";
import { copyText, openFolder } from "@/lib/api";
import { useProjectsStore } from "@/stores/projects";
import { useTemplatesStore } from "@/stores/templates";
import { toInput, type Project, type ProjectInput } from "@/types/project";

export function ProjectsPage() {
  const { items, statuses, loading, saving, error, refresh, save, remove } = useProjectsStore();
  const [editing, setEditing] = useState<{ id: string | null; input: ProjectInput } | null>(null);
  const [confirming, setConfirming] = useState<Project | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [templatesOpen, setTemplatesOpen] = useState(false);
  const { items: templates, loading: templatesLoading, refresh: refreshTemplates } = useTemplatesStore();
  const [formKey, setFormKey] = useState(0);
  const [selectedTemplate, setSelectedTemplate] = useState("none");
  const [view, setView] = useState<"all" | "tree">("all");

  useEffect(() => {
    void refresh();
    void refreshTemplates();
  }, [refresh, refreshTemplates]);

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
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1 rounded-lg border bg-background p-0.5">
            <Button
              size="sm"
              variant={view === "all" ? "secondary" : "ghost"}
              onClick={() => setView("all")}
            >
              全部
            </Button>
            <Button
              size="sm"
              variant={view === "tree" ? "secondary" : "ghost"}
              onClick={() => setView("tree")}
            >
              按总包
            </Button>
          </div>
          <Button size="sm" variant="outline" onClick={() => setTemplatesOpen(true)}>
            模板
          </Button>
          <Button
            size="sm"
            onClick={() => {
              setSelectedTemplate("none");
              setFormKey((k) => k + 1);
              setEditing({ id: null, input: toInput(emptySeed()) });
            }}
          >
            <Plus /> 新建项目
          </Button>
        </div>
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
        view === "all" ? (
          <ul className="grid gap-2">
            {items.map((p) => (
              <ProjectRow
                key={p.id}
                project={p}
                depth={0}
                copiedId={copiedId}
                onOpenRoot={(x) => void openRoot(x)}
                onCopyRoot={(x) => void copyRoot(x)}
                onEdit={(x) => setEditing({ id: x.id, input: toInput(x) })}
                onDelete={(x) => setConfirming(x)}
              />
            ))}
          </ul>
        ) : (
          <div className="grid gap-3">
            {buildProjectForest(items).map((node) => (
              <TreeGroup
                key={node.project.id}
                node={node}
                copiedId={copiedId}
                onOpenRoot={(x) => void openRoot(x)}
                onCopyRoot={(x) => void copyRoot(x)}
                onEdit={(x) => setEditing({ id: x.id, input: toInput(x) })}
                onDelete={(x) => setConfirming(x)}
              />
            ))}
          </div>
        )
      )}

      <Dialog open={editing !== null} onOpenChange={(o) => !o && setEditing(null)}>
        <DialogContent className="max-h-[90vh] overflow-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{editing?.id ? "编辑项目" : "新建项目"}</DialogTitle>
            <DialogDescription>基础信息会存进本地库，敏感凭据在信息台账里单独加密存储。</DialogDescription>
          </DialogHeader>
          {editing && (
            <div className="grid gap-3">
              {editing.id === null && (
                <div className="grid gap-2">
                  <Label className="text-xs text-muted-foreground">从模板填充</Label>
                  <Select
                    value={selectedTemplate}
                    onValueChange={(v) => {
                      if (v === "none") {
                        setSelectedTemplate("none");
                        return;
                      }
                      const t = templates.find((x) => x.id === v);
                      if (!t) return;
                      setEditing((prev) => (prev ? { ...prev, input: applyTemplate(prev.input, t) } : prev));
                      setFormKey((k) => k + 1);
                      setSelectedTemplate("none");
                    }}
                    disabled={templatesLoading || templates.length === 0}
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue placeholder="选择模板" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">不使用模板</SelectItem>
                      {templates.map((t) => (
                        <SelectItem key={t.id} value={t.id}>
                          {t.name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  {!templatesLoading && templates.length === 0 && (
                    <p className="text-xs text-muted-foreground">暂无模板，先在「模板」里新建。</p>
                  )}
                </div>
              )}
              <ProjectForm
                key={formKey}
                initial={editing.input}
                statuses={statuses}
                saving={saving}
                parentOptions={parentCandidates(items, editing.id)}
                onChange={(input) => setEditing((prev) => (prev ? { ...prev, input } : prev))}
                onCancel={() => setEditing(null)}
                onSubmit={async (input) => {
                  const ok = await save(editing.id, input);
                  if (ok) setEditing(null);
                  return ok;
                }}
              />
            </div>
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

      <TemplateManager open={templatesOpen} onOpenChange={setTemplatesOpen} statuses={statuses} />
    </div>
  );
}

function TreeGroup({
  node,
  copiedId,
  onOpenRoot,
  onCopyRoot,
  onEdit,
  onDelete,
}: {
  node: ProjectTreeNode;
  copiedId: string | null;
  onOpenRoot: (p: Project) => void;
  onCopyRoot: (p: Project) => void;
  onEdit: (p: Project) => void;
  onDelete: (p: Project) => void;
}) {
  const nodes = flattenProjectTree([node]);
  return (
    <section className="rounded-xl border bg-background p-3">
      <div className="mb-2 flex items-center gap-2 px-1 text-xs text-muted-foreground">
        <span>总包</span>
        <span>共 {nodes.length} 项</span>
      </div>
      <ul className="grid gap-2">
        {nodes.map((n) => (
          <ProjectRow
            key={n.project.id}
            project={n.project}
            depth={n.depth}
            copiedId={copiedId}
            onOpenRoot={onOpenRoot}
            onCopyRoot={onCopyRoot}
            onEdit={onEdit}
            onDelete={onDelete}
          />
        ))}
      </ul>
    </section>
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
    parentId: null,
    tags: [],
    dirs: [],
  };
}
