import { useEffect, useState } from "react";
import { Link, useParams } from "@tanstack/react-router";
import { ArrowLeft, Copy, ExternalLink, FolderOpen, FolderPlus, Pencil, Plus, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { ProjectForm } from "@/components/project-form";
import { copyText, openFolder, pickFolder, revealFolder } from "@/lib/api";
import { useProjectsStore } from "@/stores/projects";
import { toInput, type ProjectDir } from "@/types/project";

function Info({ label, value }: { label: string; value: string | null }) {
  return (
    <div className="grid gap-0.5">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="text-sm">{value || "—"}</dd>
    </div>
  );
}

export function ProjectDetailPage() {
  const { projectId } = useParams({ from: "/projects/$projectId" });
  const {
    items,
    statuses,
    loading,
    saving,
    error,
    refresh,
    save,
    setRoot,
    addEntry,
    removeDir,
  } = useProjectsStore();

  const project = items.find((p) => p.id === projectId);
  const [editing, setEditing] = useState(false);
  const [manualPath, setManualPath] = useState("");
  const [manualLabel, setManualLabel] = useState("");
  const [manualKind, setManualKind] = useState<"root" | "entry">("entry");
  const [notice, setNotice] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => {
    if (items.length === 0 && !loading) void refresh();
  }, [items.length, loading, refresh]);

  if (loading) return <p className="px-6 py-8 text-sm text-muted-foreground">读取项目档案…</p>;
  if (!project)
    return (
      <div className="px-6 py-8 text-sm text-muted-foreground">
        找不到该项目档案，可能已被删除。
        <Link to="/projects" className="ml-2 underline">
          返回项目列表
        </Link>
      </div>
    );

  const root = project.dirs.find((d) => d.kind === "root");
  const entries = project.dirs.filter((d) => d.kind === "entry");

  const guard = async (fn: () => Promise<void>) => {
    try {
      await fn();
      setNotice(null);
    } catch (e) {
      setNotice(`操作失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const chooseRoot = () =>
    guard(async () => {
      const path = await pickFolder();
      if (path) await setRoot(project.id, path);
    });

  const chooseEntry = () =>
    guard(async () => {
      const path = await pickFolder();
      if (path) await addEntry(project.id, path);
    });

  const addManual = async () => {
    if (!manualPath.trim()) return;
    const path = manualPath.trim();
    const label = manualLabel.trim() || undefined;
    const ok =
      manualKind === "root"
        ? await setRoot(project.id, path, label)
        : await addEntry(project.id, path, label);
    if (ok) {
      setManualPath("");
      setManualLabel("");
    }
  };

  const act = (dir: ProjectDir, kind: "open" | "reveal" | "copy") =>
    guard(async () => {
      if (kind === "open") await openFolder(dir.path);
      else if (kind === "reveal") await revealFolder(dir.path);
      else {
        await copyText(dir.path);
        setCopied(dir.id);
        setTimeout(() => setCopied((cur) => (cur === dir.id ? null : cur)), 1500);
      }
    });

  return (
    <div className="mx-auto w-full max-w-5xl px-6 py-8">
      <div className="mb-4 flex items-center gap-2">
        <Link to="/projects" className="text-muted-foreground hover:text-foreground">
          <ArrowLeft />
        </Link>
        <h1 className="text-lg font-semibold">{project.name}</h1>
        <Badge variant="secondary">{project.status}</Badge>
        {project.tags.map((t) => (
          <Badge key={t} variant="outline">
            {t}
          </Badge>
        ))}
        <Button size="sm" variant="ghost" className="ml-auto" onClick={() => setEditing(true)}>
          <Pencil /> 编辑
        </Button>
      </div>

      {error && (
        <div className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}
      {notice && <div className="mb-3 text-xs text-destructive">{notice}</div>}

      <section className="mb-4 rounded-xl border bg-background p-4">
        <h2 className="mb-3 text-sm font-medium">基础信息</h2>
        <dl className="grid grid-cols-2 gap-x-6 gap-y-3 md:grid-cols-4">
          <Info label="项目编号" value={project.code} />
          <Info label="项目经理" value={project.manager} />
          <Info label="客户名称" value={project.customer} />
          <Info label="对接联系人" value={project.contactName} />
          <Info label="联系方式" value={project.contactPhone} />
          <Info label="合同编号" value={project.contractNo} />
          <Info label="合同周期" value={project.contractPeriod} />
          <Info label="交付截止" value={project.deliveryDeadline} />
        </dl>
      </section>

      <section className="rounded-xl border bg-background p-4">
        <div className="mb-1 flex items-center justify-between">
          <h2 className="text-sm font-medium">本地目录映射</h2>
          <span className="text-xs text-muted-foreground">只登记路径，不改磁盘</span>
        </div>

        <div className="mt-3 flex items-center gap-2">
          <Badge variant="outline">主根目录</Badge>
          {root ? (
            <>
              <span className="min-w-0 flex-1 truncate font-mono text-xs" title={root.path}>
                {root.path}
              </span>
              {!root.exists && (
                <Badge variant="destructive" title="路径当前不可访问，登记仍保留">
                  不可达
                </Badge>
              )}
              <DirButtons dir={root} copied={copied} onAct={act} onRemove={() => void removeDir(project.id, root.id)} />
            </>
          ) : (
            <span className="flex-1 text-xs text-muted-foreground">尚未登记主根目录</span>
          )}
          <Button size="sm" variant="outline" disabled={saving} onClick={chooseRoot}>
            <FolderPlus /> {root ? "更换" : "选择目录"}
          </Button>
        </div>

        <Separator className="my-4" />

        <div className="mb-2 flex items-center gap-2">
          <Badge variant="outline">快捷入口</Badge>
          <span className="text-xs text-muted-foreground">需求文档、会议纪要等子目录或文件</span>
          <Button size="sm" variant="ghost" className="ml-auto" disabled={saving} onClick={chooseEntry}>
            <FolderPlus /> 选择目录
          </Button>
        </div>

        {entries.length === 0 ? (
          <p className="mb-3 text-xs text-muted-foreground">还没有快捷入口。</p>
        ) : (
          <ul className="mb-3 grid gap-1.5">
            {entries.map((d) => (
              <li key={d.id} className="flex items-center gap-2">
                <span className="w-28 shrink-0 truncate text-xs text-muted-foreground" title={d.label ?? undefined}>
                  {d.label || "未命名"}
                </span>
                <span
                  className={`min-w-0 flex-1 truncate font-mono text-xs ${d.exists ? "" : "text-destructive"}`}
                  title={d.exists ? d.path : `${d.path}（当前不可达）`}
                >
                  {d.path}
                </span>
                <DirButtons dir={d} copied={copied} onAct={act} onRemove={() => void removeDir(project.id, d.id)} />
              </li>
            ))}
          </ul>
        )}

        <div className="flex items-center gap-2">
          <Select
            value={manualKind}
            onValueChange={(v) => setManualKind(String(v) === "root" ? "root" : "entry")}
          >
            <SelectTrigger className="w-28">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="entry">快捷入口</SelectItem>
              <SelectItem value="root">主根目录</SelectItem>
            </SelectContent>
          </Select>
          <Input
            value={manualLabel}
            onChange={(e) => setManualLabel(e.target.value)}
            placeholder="入口名称，如 需求文档"
            className="w-40"
          />
          <Input
            value={manualPath}
            onChange={(e) => setManualPath(e.target.value)}
            placeholder="绝对路径，如 D:\项目\客户A\需求（可登记暂不可达的路径）"
            className="flex-1 font-mono text-xs"
            onKeyDown={(e) => e.key === "Enter" && void addManual()}
          />
          <Button size="sm" variant="outline" disabled={saving || !manualPath.trim()} onClick={() => void addManual()}>
            <Plus /> 登记
          </Button>
        </div>
      </section>

      <Dialog open={editing} onOpenChange={setEditing}>
        <DialogContent className="max-h-[90vh] overflow-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>编辑项目</DialogTitle>
            <DialogDescription>修改后即刻写回本地库，不影响磁盘文件。</DialogDescription>
          </DialogHeader>
          <ProjectForm
            initial={toInput(project)}
            statuses={statuses}
            saving={saving}
            onCancel={() => setEditing(false)}
            onSubmit={async (input) => {
              const ok = await save(project.id, input);
              if (ok) setEditing(false);
              return ok;
            }}
          />
        </DialogContent>
      </Dialog>
    </div>
  );
}

function DirButtons({
  dir,
  copied,
  onAct,
  onRemove,
}: {
  dir: ProjectDir;
  copied: string | null;
  onAct: (dir: ProjectDir, kind: "open" | "reveal" | "copy") => Promise<void>;
  onRemove: () => void;
}) {
  return (
    <div className="flex items-center gap-0.5">
      <Button size="icon" variant="ghost" title="打开" onClick={() => void onAct(dir, "open")}>
        <FolderOpen />
      </Button>
      <Button size="icon" variant="ghost" title="在目录中显示" onClick={() => void onAct(dir, "reveal")}>
        <ExternalLink />
      </Button>
      <Button
        size="icon"
        variant="ghost"
        title={copied === dir.id ? "已复制" : "复制路径"}
        onClick={() => void onAct(dir, "copy")}
      >
        <Copy />
      </Button>
      <Button size="icon" variant="ghost" title="移除登记" onClick={onRemove}>
        <X />
      </Button>
    </div>
  );
}
