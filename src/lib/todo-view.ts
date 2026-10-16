// 待办页三视图的排序/分组（需求：全部 / 按项目分组 / 按截止日期排序）。
// 后端 list 的 ORDER BY 只是稳定基线，视图序在这里（先例：search-order.ts 把首屏排序放前端）。
import type { Project } from "@/types/project";
import type { Todo } from "@/types/todo";
// 刻意用相对路径带 .ts 后缀而不是 `@/lib/due-warning`：todo-view 是运行时值导入，
// `@/` 别名只有 tsc/vite 认，node --test 直跑本文件时解析不到（due-warning 自己全走
// import type 被擦除，所以它可以用别名）。allowImportingTsExtensions 下这条路 tsc/vite 都通。
import { projectLabel } from "./due-warning.ts";

export type TodoView = "all" | "byProject" | "byDue";

export interface TodoGroup {
  projectId: string;
  projectLabel: string;
  todos: Todo[];
}

export function sortTodos(todos: Todo[], view: TodoView): Todo[] {
  const rows = [...todos]; // 绝不就地排调用方的数组
  if (view === "byDue") {
    rows.sort((a, b) => {
      if (a.done !== b.done) return a.done ? 1 : -1;
      const av = a.dueDate;
      const bv = b.dueDate;
      if (av === null && bv === null) return a.createdAt < b.createdAt ? -1 : a.createdAt > b.createdAt ? 1 : 0;
      if (av === null) return 1; // 空日期落最后（spec §六）
      if (bv === null) return -1;
      if (av !== bv) return av < bv ? -1 : 1;
      return a.createdAt < b.createdAt ? -1 : a.createdAt > b.createdAt ? 1 : 0;
    });
    return rows;
  }
  rows.sort((a, b) => (a.createdAt < b.createdAt ? -1 : a.createdAt > b.createdAt ? 1 : 0));
  return rows;
}

export function groupByProject(todos: Todo[], projects: Project[], view: TodoView): TodoGroup[] {
  const byId = new Map(projects.map((p) => [p.id, p]));
  const sorted = sortTodos(todos, view);
  const groups = new Map<string, TodoGroup>();
  for (const t of sorted) {
    const key = t.projectId;
    let group = groups.get(key);
    if (!group) {
      const p = byId.get(key);
      group = { projectId: key, projectLabel: p ? projectLabel(p) : "（项目缺失）", todos: [] };
      groups.set(key, group);
    }
    group.todos.push(t);
  }
  // 组按项目名排：用 localeCompare("zh") 而不是 `<` 码元比较 —— 码元序里「乙(U+4E59)」排在
  // 「甲(U+7532)」前面，与中文使用者预期的拼音序（甲先）相反；简报的测试钉的就是拼音序。
  // Node 13+/现代浏览器都带 full-icu，显式传 "zh" 让回退语言也确定，不随 host locale 漂。
  return [...groups.values()].sort((a, b) => a.projectLabel.localeCompare(b.projectLabel, "zh"));
}
