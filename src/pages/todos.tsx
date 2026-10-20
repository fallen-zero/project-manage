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
  listExpiries,
  listMilestones,
  listProjects,
  listTodos,
  updateTodo,
} from "@/lib/api";
import {
  buildDueWarnings,
  DUE_HORIZON_DAYS,
  EXPIRY_HORIZON_DAYS,
  localToday,
  projectLabel,
  scopeDueInput,
  type DueItem,
} from "@/lib/due-warning";
import { groupByProject, sortTodos, type TodoView } from "@/lib/todo-view";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { dueOrNull, type Todo } from "@/types/todo";
import type { Milestone } from "@/types/milestone";
import type { Project } from "@/types/project";
import type { Expiry } from "@/types/expiry";

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
  const [expiries, setExpiries] = useState<Expiry[]>([]);
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
      const [t, m, x, p] = await Promise.all([
        listTodos(null),
        listMilestones(null),
        listExpiries(null),
        listProjects(),
      ]);
      setTodos(t);
      setMilestones(m);
      setExpiries(x);
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

  // 筛选只做一次，而且**四份数据一起缩**（终审 I1）：原先预警块与「共 N 条未完成」那行吃的是未筛选的全量，
  // 只有列表行走过 scoped ⇒ 用户筛到甲项目时仍看见乙/丙的红黄与全局的 unparsed 数。
  // 缩的动作抽成 `scopeDueInput`（纯函数），这一格才有可红的断言 —— 页面本身只有 `tsc` + `vite build` 两道门。
  const scoped = useMemo(
    () => scopeDueInput({ todos, milestones, expiries, projects }, filter === ALL_PROJECTS ? null : filter),
    [todos, milestones, expiries, projects, filter],
  );

  const due = useMemo(() => buildDueWarnings(scoped, localToday()), [scoped]);

  const visible = useMemo(
    () => (view === "byProject" ? groupByProject(scoped.todos, scoped.projects, view) : null),
    [scoped, view],
  );

  const flat = useMemo(() => sortTodos(scoped.todos, view), [scoped, view]);

  // run 必须回报成败：它把异常吞进 state、promise 永不 reject，
  // 所以任何 `.then(清理)` 都会无条件执行 —— T5 评审据此打回两条 Important（失败的新增照样清空输入、
  // 失败的保存退出编辑态看起来像成功）。调用方一律 `if (ok)` 再清理。
  const run = async (fn: () => Promise<unknown>): Promise<boolean> => {
    setBusy(true);
    try {
      await fn();
      await reload();
      return true;
    } catch (e) {
      setError(toAppError(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-4">
      <section className="rounded-xl border bg-background p-4">
        <h2 className="text-sm font-medium">到期预警</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          四处一起算：未完成的待办截止、未勾选的里程碑计划日期、未归档项目的交付截止、到期事项。红=已逾期，描边=今天起 {DUE_HORIZON_DAYS} 天内；到期事项单独看 {EXPIRY_HORIZON_DAYS} 天内（续费要走审批，窗口更长），归档项目的行照常提醒并在行尾标出「（归档项目）」—— 只有交付截止那一类不催已退役的项目。
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
                {/* 归档标注刻意放在 truncate 容器**之外**：Tailwind 的 `truncate` = overflow:hidden
                    + text-overflow:ellipsis + white-space:nowrap，裁的是**整个容器**。把标注塞进容器末尾
                    就等于让它跟着 `{项目名} · {标题}` 一起被省略号挤掉 —— 而这枚标注是 D4 那句
                    「归档项目的到期事项照常亮，但界面必须念出来，别让人以为录错」的唯一防线，
                    不能是一行里最先牺牲的那一段。容器本身补 `title` 兜全文（本仓先例：
                    expiry-panel.tsx、app-shell.tsx、ledger-panel.tsx、search-bundle.tsx）。
                    `shrink-0` 让标注永远保住自己的宽度，行名长了先被裁的是中间那段。 */}
                <span className="min-w-0 flex-1 truncate" title={`${i.projectLabel} · ${i.label}`}>
                  {i.projectLabel} · {i.label}
                </span>
                {i.archived && <span className="shrink-0 text-xs text-muted-foreground">（归档项目）</span>}
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
          <span className="text-xs text-muted-foreground">共 {scoped.todos.length} 条未完成 {scoped.todos.filter((t) => !t.done).length}</span>
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

        {scoped.todos.length === 0 && (
          <p className="py-6 text-center text-sm text-muted-foreground">
            {filter === ALL_PROJECTS ? "还没有待办。" : "这个项目下还没有待办。"}
          </p>
        )}

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
    if (busy || newTitle.trim() === "" || newProject === "") return; // Enter 键绕过按钮 disabled，这里补齐同一道门
    return run(() =>
      createTodo({ projectId: newProject, title: newTitle.trim(), dueDate: dueOrNull(newDue) }),
    ).then((ok) => {
      if (!ok) return; // 被后端拒了就留着用户刚敲的内容
      setNewTitle("");
      setNewDue("");
    });
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
          ).then((ok) => {
            if (ok) setEditingId(null); // 失败留在编辑态，草稿别丢
          })
        }
        onCancel={() => setEditingId(null)}
        onToggle={(x) => void run(() => updateTodo(x.id, { title: x.title, dueDate: x.dueDate, done: !x.done }))}
        onDelete={(x) => void run(() => deleteTodo(x.id))}
      />
    );
  }
}
