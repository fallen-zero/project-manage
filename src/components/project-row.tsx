import { Link } from "@tanstack/react-router";
import { Copy, FolderOpen, Pencil, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { Project } from "@/types/project";

export function ProjectRow({
  project,
  depth,
  copiedId,
  onOpenRoot,
  onCopyRoot,
  onEdit,
  onDelete,
}: {
  project: Project;
  depth: number;
  copiedId: string | null;
  onOpenRoot: (p: Project) => void;
  onCopyRoot: (p: Project) => void;
  onEdit: (p: Project) => void;
  onDelete: (p: Project) => void;
}) {
  const root = project.dirs.find((d) => d.kind === "root");
  return (
    <li
      className="grid grid-cols-[minmax(0,1.4fr)_auto] items-center gap-3 rounded-xl border bg-background py-3 pr-4"
      style={{ paddingLeft: 16 + depth * 20 }}
    >
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <Link
            to="/projects/$projectId"
            params={{ projectId: project.id }}
            className="truncate text-sm font-medium hover:underline"
          >
            {project.name}
          </Link>
          <Badge variant="secondary">{project.status}</Badge>
          {project.tags.map((t) => (
            <Badge key={t} variant="outline">
              {t}
            </Badge>
          ))}
        </div>
        <div className="mt-1 flex items-center gap-2 text-xs text-muted-foreground">
          <span>{[project.code, project.customer, project.manager].filter(Boolean).join(" · ") || "—"}</span>
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
          onClick={() => onOpenRoot(project)}
        >
          <FolderOpen />
        </Button>
        <Button
          size="icon"
          variant="ghost"
          disabled={!root}
          title={copiedId === project.id ? "已复制" : "复制路径"}
          onClick={() => onCopyRoot(project)}
        >
          <Copy />
        </Button>
        <Button size="icon" variant="ghost" title="编辑" onClick={() => onEdit(project)}>
          <Pencil />
        </Button>
        <Button size="icon" variant="ghost" title="删除" onClick={() => onDelete(project)}>
          <Trash2 />
        </Button>
      </div>
    </li>
  );
}
