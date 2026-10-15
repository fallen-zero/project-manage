// 待办清单：跨项目统一管理（需求 思维导图.md:38-45）。顶部是到期预警块（spec §六）。
// 数据一次拉全（listTodos(null) / listMilestones(null) / listProjects()）：几十个项目、
// 几百条待办的量级，视图排序与预警都在前端，比按视图来回打后端简单也更省往返。
import { useCallback, useEffect, useMemo, useState } from "react";
import { Link } from "@tanstack/react-router";
import { Plus, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import {
  createTodo,
  deleteTodo,
  listMilestones,
  listProjects,
  listTodos,
  updateTodo,
} from "@/lib/api";
import { buildDueWarnings, localToday, projectLabel, type DueItem } from "@/lib/due-warning";
import { groupByProject, sortTodos, type TodoView } from "@/lib/todo-view";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { dueOrNull, type Todo } from "@/types/todo";
import type { Milestone } from "@/types/milestone";
import type { Project } from "@/types/project";

const VIEWS: { key: TodoView; label: string }[] = [
  { key: "all", label: "全部" },
  { key: "byProject", label: "按项目分组" },
  { key: "byDue", label: "按截止排序" },
];

const ALL_PROJECTS = "__all__";

function Row({
  todo,
  projectLabel,
  editing,
  draftTitle,
  draftDue,
  busy,
  onStartEdit,
  onDraft,
  onSave,
  onCancel,
  onToggle,
  onDelete,
}: {
  todo: Todo;
  projectLabel: string;
  editing: boolean;
  draftTitle: string;
  draftDue: string;
  busy: boolean;
  onStartEdit: (t: Todo) => void;
  onDraft: (key: "title" | "due", value: string) => void;
  onSave: () => void;
  onCancel: () => void;
  onToggle: (t: Todo) => void;
  onDelete: (t: Todo) => void;
}) {
  return (
    <div className="flex items-center gap-2 border-b px-1 py-2 last:border-b-0">
      <input
        type="checkbox"
        className="size-4 accent-primary"
        checked={todo.done}
        disabled={busy}
        onChange={() => onToggle(todo)}
        aria-label={todo.done ? "标记为未完成" : "标记为已完成"}
      />
      {editing ? (
        <>
          <Input
            value={draftTitle}
            onChange={(e) => onDraft("title", e.target.value)}
            className="flex-1"
            autoFocus
          />
          <Input
            type="date"
            value={draftDue}
            onChange={(e) => onDraft("due", e.target.value)}
            className="w-44"
          />
          <Button size="sm" disabled={busy || draftTitle.trim() === ""} onClick={onSave}>
            保存
          </Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={onCancel}>
            取消
          </Button>
        </>
      ) : (
        <>
          <span className={`min-w-0 flex-1 truncate text-sm ${todo.done ? "text-muted-foreground line-through" : ""}`}>
            {todo.title}
          </span>
          <Link to="/projects/$projectId" params={{ projectId: todo.projectId }} className="shrink-0">
            <Badge variant="outline">{projectLabel}</Badge>
          </Link>
          <span className="w-24 shrink-0 text-right font-mono text-xs text-muted-foreground">
            {todo.dueDate ?? "无截止"}
          </span>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => onStartEdit(todo)}>
            编辑
          </Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => onDelete(todo)}>
            <Trash2 />
          </Button>
        </>
      )}
    </div>
  );
}

export function TodosPage() {
  const [todos, setTodos] = useState<Todo[]>([]);
  const [milestones, setMilestones] = useState<Milestone[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [view, setView] = useState<TodoView>("all");
  const [filter, setFilter] = useState<string>(ALL_PROJECTS);
  const [newTitle, setNewTitle] = useState("");
  const [newDue, setNewDue] = useState("");
  const [newProject, setNewProject] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftTitle, setDraftTitle] = useState("");
  const [draftDue, setDraftDue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);

  const reload = useCallback(async () => {
    try {
      const [t, m, p] = await Promise.all([listTodos(null), listMilestones(null), listProjects()]);
      setTodos(t);
      setMilestones(m);
      setProjects(p);
      setError(null);
    } catch (e) {
      setError(toAppError(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const labelOf = useMemo(() => {
    // 项目名的显示形状只有一份（due-warning 的 projectLabel），这里不重拼一遍，免得两处漂
    const byId = new Map(projects.map((p) => [p.id, projectLabel(p)]));
    return (id: string) => byId.get(id) ?? "（项目缺失）";
  }, [projects]);

  const due = useMemo(
    () => buildDueWarnings({ todos, milestones, projects }, localToday()),
    [todos, milestones, projects],
  );

  const visible = useMemo(() => {
    const scoped = filter === ALL_PROJECTS ? todos : todos.filter((t) => t.projectId === filter);
    return view === "byProject" ? groupByProject(scoped, projects, view) : null;
  }, [todos, projects, filter, view]);

  const flat = useMemo(() => {
    const scoped = filter === ALL_PROJECTS ? todos : todos.filter((t) => t.projectId === filter);
    return sortTodos(scoped, view);
  }, [todos, filter, view]);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
      await reload();
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-4">
      <section className="rounded-xl border bg-background p-4">
        <h2 className="text-sm font-medium">到期预警</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          三处一起算：未完成的待办截止、未勾选的里程碑计划日期、未归档项目的交付截止。红=已逾期，黄=今天起 7 天内。
        </p>
        {due.unparsed > 0 && (
          <p className="mt-2 text-xs text-destructive">
            {due.unparsed} 条日期无法解析，没有参与预警（要写成 YYYY-MM-DD）
          </p>
        )}
        {due.items.length === 0 ? (
          <p className="mt-3 text-sm text-muted-foreground">眼下没有到期项。</p>
        ) : (
          <ul className="mt-3 space-y-1">
            {due.items.map((i: DueItem) => (
              <li key={`${i.kind}-${i.id}`} className="flex items-center gap-2 text-sm">
                <Badge variant={i.state === "overdue" ? "destructive" : "outline"}>
                  {i.state === "overdue" ? `逾期 ${-i.days} 天` : i.days === 0 ? "今天" : `${i.days} 天后`}
                </Badge>
                <span className="min-w-0 flex-1 truncate">
                  {i.projectLabel} · {i.label}
                </span>
                <span className="font-mono text-xs text-muted-foreground">{i.date}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      {error && (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}

      <section className="rounded-xl border bg-background p-4">
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <h2 className="text-sm font-medium">待办</h2>
          <span className="text-xs text-muted-foreground">共 {todos.length} 条未完成 {todos.filter((t) => !t.done).length}</span>
          <div className="ml-auto flex items-center gap-2">
            {VIEWS.map((v) => (
              <Button
                key={v.key}
                size="sm"
                variant={view === v.key ? "default" : "outline"}
                onClick={() => setView(v.key)}
              >
                {v.label}
              </Button>
            ))}
            <Select value={filter} onValueChange={setFilter}>
              <SelectTrigger className="w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL_PROJECTS}>全部项目</SelectItem>
                {projects.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {labelOf(p.id)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>

        <div className="mb-3 flex flex-wrap items-center gap-2">
          <Input
            value={newTitle}
            onChange={(e) => setNewTitle(e.target.value)}
            placeholder="待办内容"
            className="min-w-48 flex-1"
            onKeyDown={(e) => e.key === "Enter" && void add()}
          />
          <Input type="date" value={newDue} onChange={(e) => setNewDue(e.target.value)} className="w-44" />
          <Select value={newProject} onValueChange={setNewProject}>
            <SelectTrigger className="w-56">
              <SelectValue placeholder="选择项目" />
            </SelectTrigger>
            <SelectContent>
              {projects.map((p) => (
                <SelectItem key={p.id} value={p.id}>
                  {labelOf(p.id)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button size="sm" disabled={busy || newTitle.trim() === "" || newProject === ""} onClick={() => void add()}>
            <Plus /> 新增
          </Button>
        </div>

        <Separator className="mb-2" />

        {todos.length === 0 && <p className="py-6 text-center text-sm text-muted-foreground">还没有待办。</p>}

        {view === "byProject" && visible
          ? visible.map((g) => (
              <div key={g.projectId} className="mb-3">
                <div className="mb-1 text-xs font-medium text-muted-foreground">{g.projectLabel}</div>
                {g.todos.map((t) => row(t))}
              </div>
            ))
          : flat.map((t) => row(t))}
      </section>
    </div>
  );

  function add() {
    return run(() => createTodo({ projectId: newProject, title: newTitle.trim(), dueDate: dueOrNull(newDue) })).then(
      () => {
        setNewTitle("");
        setNewDue("");
      },
    );
  }

  function row(t: Todo) {
    const editing = editingId === t.id;
    return (
      <Row
        key={t.id}
        todo={t}
        projectLabel={labelOf(t.projectId)}
        editing={editing}
        draftTitle={editing ? draftTitle : ""}
        draftDue={editing ? draftDue : ""}
        busy={busy}
        onStartEdit={(x) => {
          setEditingId(x.id);
          setDraftTitle(x.title);
          setDraftDue(x.dueDate ?? "");
        }}
        onDraft={(k, v) => (k === "title" ? setDraftTitle(v) : setDraftDue(v))}
        onSave={() =>
          void run(() =>
            updateTodo(t.id, { title: draftTitle.trim(), dueDate: dueOrNull(draftDue), done: t.done }),
          ).then(() => setEditingId(null))
        }
        onCancel={() => setEditingId(null)}
        onToggle={(x) => void run(() => updateTodo(x.id, { title: x.title, dueDate: x.dueDate, done: !x.done }))}
        onDelete={(x) => void run(() => deleteTodo(x.id))}
      />
    );
  }
}
