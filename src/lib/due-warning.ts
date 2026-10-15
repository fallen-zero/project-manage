// 到期预警的纯逻辑（spec §六 / D1）。这一格刻意只 import type（会被擦除），所以它能被
// `node --test` 直跑，不必给仓库添 jsdom（先例：search-order.ts、backup-name.ts）。
import type { Project } from "@/types/project";
import type { Milestone } from "@/types/milestone";
import type { Todo } from "@/types/todo";

export const DUE_HORIZON_DAYS = 7;

export type DueKind = "todo" | "milestone" | "deadline";
export type DueState = "overdue" | "soon";

export interface DueItem {
  kind: DueKind;
  /** todo/milestone 的行 id；deadline 那一类用项目 id。 */
  id: string;
  projectId: string;
  projectLabel: string;
  /** 待办标题 / 里程碑阶段名 / 「交付截止」。 */
  label: string;
  date: string;
  state: DueState;
  /** 距今天的日历日数：overdue 为负，含今天为 0。 */
  days: number;
}

export interface DueReport {
  items: DueItem[];
  /** 形状解析不了、被跳过的行数（界面必须念出来，别静默少亮 —— 判据②）。 */
  unparsed: number;
}

export interface DueInput {
  todos: Todo[];
  milestones: Milestone[];
  projects: Project[];
}

const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;
const DAY_MS = 86_400_000;

/** 折成「自 epoch 的天数」：用 Date.UTC 构造 ⇒ 与本机时区无关，比的是日历日不是时刻。
 *  调用方传进来的 today 已经过 ISO_DATE 形状保证（页面里 localToday 自己产，外部输入走后端校验）。 */
function toDayNumber(iso: string): number {
  const [y, m, d] = iso.split("-").map(Number);
  return Math.floor(Date.UTC(y, m - 1, d) / DAY_MS);
}

/** 页面用这一条读一次时钟；纯函数本体不读，才好测。 */
export function localToday(): string {
  const now = new Date();
  const pad = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${pad(now.getFullYear(), 4)}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

export function projectLabel(p: Project): string {
  return p.code ? `${p.name}（${p.code}）` : p.name;
}

export function buildDueWarnings(
  input: DueInput,
  today: string,
  horizonDays: number = DUE_HORIZON_DAYS,
): DueReport {
  const byId = new Map(input.projects.map((p) => [p.id, p]));
  const todayDay = toDayNumber(today);
  let unparsed = 0;
  const items: DueItem[] = [];

  const emit = (kind: DueKind, id: string, projectId: string, label: string, raw: string | null) => {
    if (raw === null || raw === "") return;
    if (!ISO_DATE.test(raw)) {
      unparsed += 1;
      return;
    }
    const days = toDayNumber(raw) - todayDay;
    if (days > horizonDays) return;
    const p = byId.get(projectId);
    items.push({
      kind,
      id,
      projectId,
      projectLabel: p ? projectLabel(p) : "（项目缺失）",
      label,
      date: raw,
      days,
      state: days < 0 ? "overdue" : "soon",
    });
  };

  for (const t of input.todos) {
    if (t.done) continue;
    emit("todo", t.id, t.projectId, t.title, t.dueDate);
  }
  for (const m of input.milestones) {
    if (m.done) continue;
    emit("milestone", m.id, m.projectId, m.stage, m.planDate);
  }
  for (const p of input.projects) {
    // 判据⑤：归档 = 不再催事。这一类没有 done 勾选态，用「未归档」当等价门。
    if (p.status === "归档") continue;
    emit("deadline", p.id, p.id, "交付截止", p.deliveryDeadline);
  }

  items.sort((a, b) => {
    if (a.state !== b.state) return a.state === "overdue" ? -1 : 1;
    if (a.date !== b.date) return a.date < b.date ? -1 : 1;
    // 「再按项目名」与 todo-view 的组序同一把尺：localeCompare("zh") 的拼音序，不是码元序。
    if (a.projectLabel !== b.projectLabel) return a.projectLabel.localeCompare(b.projectLabel, "zh");
    return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
  });
  return { items, unparsed };
}
