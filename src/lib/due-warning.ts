// 到期预警的纯逻辑（spec §六 / D1）。这一格刻意只 import type（会被擦除），所以它能被
// `node --test` 直跑，不必给仓库添 jsdom（先例：search-order.ts、backup-name.ts）。
import type { Project } from "@/types/project";
import type { Milestone } from "@/types/milestone";
import type { Todo } from "@/types/todo";
import type { Expiry } from "@/types/expiry";

export const DUE_HORIZON_DAYS = 7;

/** 到期事项单独一个更长的窗口（spec D3）：域名/证书续费要走合同与审批，7 天等于到期当天才亮红、那天已经晚了。 */
export const EXPIRY_HORIZON_DAYS = 30;

export type DueKind = "todo" | "milestone" | "deadline" | "expiry";
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
  /** 所属项目是否已归档（D4：归档项目的到期事项照常亮，但界面必须念出来，别让人以为录错）。 */
  archived: boolean;
}

export interface DueReport {
  items: DueItem[];
  /** 形状解析不了、被跳过的行数（界面必须念出来，别静默少亮 —— 判据②）。 */
  unparsed: number;
}

export interface DueInput {
  todos: Todo[];
  milestones: Milestone[];
  expiries: Expiry[];
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

/** 单行判据（spec §六 判据④）：形状对且落在 horizon 之内 ⇒ 返回档位与距今天的日历日数，否则 null。
 *  **这一份是 `buildDueWarnings` 与 `milestone-panel` 共用的**（终审 I2 从两处比较里抽出来的）：
 *  面板原先自己写第二份（字符串比较 + 自己的 `shiftDate`），把那里的 `<` 改成 `<=` 现有测试全绿
 *  ⇒ T7 变异 M⑥ 打不红、面板徽标没有任何自动化网。共用之后它落进 `node --test` 的射程。
 *  顺带了结一处真分叉：形状对但历法错的值（`2026-09-45`）过去是「页面判黄、面板判红」
 *  （`Date.UTC` 把它滚成 10-15 vs 字符串比较直接判小于今天），现在两处同判 soon。
 *  历法有效性仍**不在本轮**（spec §十二 风险 5：引历法库就破「零新依赖」）。 */
export function dueStateOf(
  date: string,
  today: string,
  horizonDays: number = DUE_HORIZON_DAYS,
): { state: DueState; days: number } | null {
  if (!ISO_DATE.test(date)) return null;
  const days = toDayNumber(date) - toDayNumber(today);
  if (days > horizonDays) return null;
  return { state: days < 0 ? "overdue" : "soon", days };
}

/** 窗口按类只在这一处选（M9 终审 I2 的同一条规矩：比较本体 `dueStateOf` 只有一份，谁也不许再写第二份日期比较）。
 *  把「到期事项用 30 天」写成两份（一处 emit、一处面板）就是下一次分叉的开始。 */
export function horizonOf(kind: DueKind): number {
  return kind === "expiry" ? EXPIRY_HORIZON_DAYS : DUE_HORIZON_DAYS;
}

/** 项目筛选把四份输入**一起**缩小（终审 I1；M11-a Task 4 起含到期事项）。抽成纯函数不是为了复用，是为了让那一格有可红的断言：
 *  页面本身没有组件测试网（门只有 `tsc` + `vite build`），原先预警块与「共 N 条」那行吃的是未筛选的全量
 *  ⇒ 用户筛到甲项目时仍看见乙/丙的红黄与全局 `unparsed` 数，而且没人守得住。
 *  `projectId === null` = 全部项目，原样返回。 */
export function scopeDueInput(input: DueInput, projectId: string | null): DueInput {
  if (projectId === null) return input;
  return {
    todos: input.todos.filter((t) => t.projectId === projectId),
    milestones: input.milestones.filter((m) => m.projectId === projectId),
    expiries: input.expiries.filter((e) => e.projectId === projectId),
    projects: input.projects.filter((p) => p.id === projectId),
  };
}

export function buildDueWarnings(
  input: DueInput,
  today: string,
  /** 给定时**所有类**都用它（既有的覆盖位，M9 T7 变异 M⑦ 那条常量的消费点）；
   *  省略时按类走 `horizonOf(kind)` —— 这才是 D3「到期事项单独 30 天」的唯一落点。 */
  horizonOverride?: number,
): DueReport {
  const byId = new Map(input.projects.map((p) => [p.id, p]));
  let unparsed = 0;
  const items: DueItem[] = [];

  const emit = (kind: DueKind, id: string, projectId: string, label: string, raw: string | null) => {
    if (raw === null || raw === "") return;
    if (!ISO_DATE.test(raw)) {
      unparsed += 1;
      return;
    }
    // `??` 而不是 `||`：0 是合法的覆盖值（既有测试「horizonDays 传 0」吃的就是它），写 `||` 那条会恒绿。
    const hit = dueStateOf(raw, today, horizonOverride ?? horizonOf(kind));
    if (hit === null) return;
    const p = byId.get(projectId);
    items.push({
      kind,
      id,
      projectId,
      projectLabel: p ? projectLabel(p) : "（项目缺失）",
      label,
      date: raw,
      state: hit.state,
      days: hit.days,
      archived: p?.status === "归档",
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
    // 判据⑤：**只有这一类**带归档门（交付截止不催已退役的项目）。到期事项不跟着这道门走 —— 见 D4。
    if (p.status === "归档") continue;
    emit("deadline", p.id, p.id, "交付截止", p.deliveryDeadline);
  }
  for (const e of input.expiries) {
    // D4：归档项目的到期事项照常亮（归档是管理上的退役，域名/证书/维保是还在计费的事实）。
    emit("expiry", e.id, e.projectId, `${e.kind} ${e.label}`, e.expireDate);
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
