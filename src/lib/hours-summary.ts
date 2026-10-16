// 工时统计的纯逻辑（M10 spec §七 / D9）。聚合放前端是因为 SQL 不解释日期：
// `strftime`/`date` 按 UTC，周首也与 ISO 直觉不一致，M9 D7 就是为躲这个把判据搬过来的（沿用）。
// 本文件不读机器时钟 —— 今天由调用方从 `localToday()` 传进来，判据才能喂常量做断言。
import type { Project } from "@/types/project";
import type { DailyReport } from "@/types/daily-report";
// 唯一的 value import：相对路径 + .ts 后缀（`@/` 别名在 node --test 下解析不到，见 todo-view.ts:8 同条注释）。
import { projectLabel } from "./due-warning.ts";

export type HoursBucket = "day" | "week" | "month";

export interface HoursFilter {
  /** `null` = 全部项目。 */
  projectId: string | null;
  /** 闭区间的两端，`""` = 不限。形状不校验，交给比较本身（YYYY-MM-DD 串比就是日期序）。 */
  from: string;
  to: string;
  keyword: string;
}

export interface HoursRow {
  key: string;
  hours: number;
  /** 条目数：工时相同但条数不同的两格要看得出差别。 */
  count: number;
}

export interface ProjectHours {
  projectId: string;
  projectLabel: string;
  hours: number;
  count: number;
}

export interface HoursSummary {
  buckets: HoursRow[];
  byProject: ProjectHours[];
  total: number;
  count: number;
  /** 日期形状解析不了、被跳过的行数。界面必须念出来，别静默少算（沿用 due-warning 的 unparsed 形制）。 */
  unparsed: number;
}

const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;
const DAY_MS = 86_400_000;

export const dayKey = (date: string): string => date;
export const monthKey = (date: string): string => date.slice(0, 7);

/// ISO-8601 周键 `YYYY-Www`：周一起算，**包含周四的那一周属于该年**。
/// 算法就是「把日期挪到本周周四 → 取那一年 → 与当年第一周的周四相减」。
/// 不许改成 `getDay()` 直接算周序，也不许用 `strftime('%W')`：两者的周首与年首边界都不符 ISO。
export function isoWeekKey(date: string): string {
  const [y, m, d] = date.split("-").map(Number);
  const thursday = new Date(Date.UTC(y, m - 1, d));
  const dow = thursday.getUTCDay() || 7; // 周一=1 … 周日=7
  thursday.setUTCDate(thursday.getUTCDate() + 4 - dow);
  const isoYear = thursday.getUTCFullYear();
  const jan4 = new Date(Date.UTC(isoYear, 0, 4));
  jan4.setUTCDate(jan4.getUTCDate() + 4 - (jan4.getUTCDay() || 7));
  const week = Math.floor((thursday.getTime() - jan4.getTime()) / (7 * DAY_MS)) + 1;
  return `${isoYear}-W${String(week).padStart(2, "0")}`;
}

/// 求和一律整数化（spec §五.2）。`0.1 * 10` 直接累加得 `1.0000000000000002`，
/// 那条界面写出来就是「本周合计 1.0000000000000002 小时」。
export function sumHours(rows: DailyReport[]): number {
  let cents = 0;
  for (const r of rows) cents += Math.round(r.hours * 100);
  return cents / 100;
}

/// 三份条件一起缩：这是「聚合的入参必须是筛选后的行」的唯一实现处（M9 终审 I1 同族的教训）。
export function scopeDailyReports(rows: DailyReport[], f: HoursFilter): DailyReport[] {
  const keyword = f.keyword.trim().toLowerCase();
  return rows.filter((r) => {
    if (f.projectId !== null && r.projectId !== f.projectId) return false;
    if (f.from !== "" && r.reportDate < f.from) return false;
    if (f.to !== "" && r.reportDate > f.to) return false;
    if (keyword !== "" && !r.content.toLowerCase().includes(keyword)) return false;
    return true;
  });
}

/** 项目 id → 展示名；库里没有该项目时念「（项目缺失）」（同 todo-view.ts 的形制）。 */
function labelOf(projects: Project[], id: string): string {
  const p = projects.find((x) => x.id === id);
  return p ? projectLabel(p) : "（项目缺失）";
}

/// **页面的唯一入口**：内部先 scope 再聚合，所以「忘了缩」在结构上不可能发生（M⑥ 的靶就是这个函数改成吃全量）。
export function scopedHoursSummary(
  rows: DailyReport[],
  f: HoursFilter,
  bucket: HoursBucket,
  projects: Project[],
): HoursSummary {
  const scoped = scopeDailyReports(rows, f);
  const keyed = bucket === "day" ? dayKey : bucket === "week" ? isoWeekKey : monthKey;
  const buckets = new Map<string, DailyReport[]>();
  const byProject = new Map<string, DailyReport[]>();
  let unparsed = 0;

  for (const r of scoped) {
    if (!ISO_DATE.test(r.reportDate)) {
      unparsed += 1; // 形状不对：进不了任何桶，但要计数（判据②的形制）
      continue;
    }
    pushInto(buckets, keyed(r.reportDate), r);
    pushInto(byProject, r.projectId, r);
  }

  const valid = scoped.filter((r) => ISO_DATE.test(r.reportDate));
  return {
    buckets: [...buckets.entries()]
      .sort((a, b) => (a[0] < b[0] ? -1 : 1))
      .map(([key, rs]) => ({ key, hours: sumHours(rs), count: rs.length })),
    byProject: [...byProject.entries()]
      .map(([projectId, rs]) => ({
        projectId,
        projectLabel: labelOf(projects, projectId),
        hours: sumHours(rs),
        count: rs.length,
      }))
      // 组序按项目名的拼音序（`Intl.Collator("zh")`，同 todo-view.ts:54 的理由：码元序里「乙」在「甲」前）。
      .sort((a, b) => a.projectLabel.localeCompare(b.projectLabel, "zh")),
    total: sumHours(valid),
    count: valid.length,
    unparsed,
  };
}

function pushInto<T>(map: Map<string, T[]>, key: string, value: T): void {
  const list = map.get(key);
  if (list) list.push(value);
  else map.set(key, [value]);
}

/// 「复制上一天」的源日期：**所选日期之前最近一个有日报的那天**，不是机械的前一天
/// （D10：周一按前一天取会拿到周末的 0 条，等于按钮失灵）。返回 `null` = 之前一条都没有。
/// 只认形状合法的行：坏形状的日期不参与「哪天有日报」的判断。
export function previousReportDay(rows: DailyReport[], targetDate: string): string | null {
  let best: string | null = null;
  for (const r of rows) {
    if (!ISO_DATE.test(r.reportDate)) continue;
    if (r.reportDate >= targetDate) continue;
    if (best === null || r.reportDate > best) best = r.reportDate;
  }
  return best;
}
