import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  dayKey,
  isoWeekKey,
  monthKey,
  previousReportDay,
  scopeDailyReports,
  scopedHoursSummary,
  sumHours,
  type HoursFilter,
} from "../src/lib/hours-summary.ts";
import type { DailyReport } from "../src/types/daily-report.ts";
import type { Project } from "../src/types/project.ts";

const row = (over: Partial<DailyReport> = {}): DailyReport => ({
  id: "d1",
  projectId: "p1",
  reportDate: "2026-10-16",
  content: "甲",
  hours: 1,
  createdAt: "2026-10-16 09:00:00",
  updatedAt: "2026-10-16 09:00:00",
  ...over,
});

const project = (over: Partial<Project> = {}): Project => ({
  id: "p1",
  name: "乙项目",
  // `code` 故意留 null：`projectLabel` 在有编号时返回「名（编号）」，
  // 下面按组序断言的是纯项目名，编号一进来就变成比较「乙项目（P-02）」这种串了。
  code: null,
  manager: null,
  customer: null,
  contactName: null,
  contactPhone: null,
  contractNo: null,
  contractPeriod: null,
  deliveryDeadline: null,
  status: "开发",
  tags: [],
  dirs: [],
  ...over,
});

// —— ISO 周：三条跨年边界是本轮最容易被写错、写错又不报错的一格 ——
test("isoWeekKey 钉住跨年周三条边界（周一为周首，含周四的那一周属于该年）", () => {
  assert.equal(isoWeekKey("2026-01-01"), "2026-W01", "2026-01-01 是周四 ⇒ 该周是 2026 年第 1 周");
  assert.equal(isoWeekKey("2026-12-31"), "2026-W53", "2026 是 53 周年，12-31 是周四");
  assert.equal(isoWeekKey("2027-01-01"), "2026-W53", "跨年的周五仍属 12-31 那一周，不许漂成 2027-W01");
  assert.equal(isoWeekKey("2027-01-04"), "2027-W01", "下一个周一起才换年");
});

test("isoWeekKey 同周不同日同键、周日归上一周", () => {
  assert.equal(isoWeekKey("2026-10-12"), isoWeekKey("2026-10-18"), "周一到周日同一周");
  assert.equal(isoWeekKey("2026-10-11"), "2026-W41", "2026-10-11 是周日，归 W41（周一为周首）");
});

test("日桶用原串、月桶取前七位", () => {
  assert.equal(dayKey("2026-10-16"), "2026-10-16");
  assert.equal(monthKey("2026-10-16"), "2026-10");
  assert.equal(monthKey("2026-01-05"), "2026-01");
});

test("sumHours 整数化求和：十条 0.1 得 1 而不是 1.0000000000000002", () => {
  const rows = Array.from({ length: 10 }, (_, i) => row({ id: `d${i}`, hours: 0.1 }));
  assert.equal(sumHours(rows), 1, "别改成 toBeCloseTo，那样恒绿");
  assert.equal(sumHours([]), 0, "空数组得 0，不是 NaN");
});

test("筛选三份条件一起缩：项目、日期闭区间、关键词（大小写与首尾空格都收）", () => {
  const rows = [
    row({ id: "a", projectId: "p1", reportDate: "2026-10-01", content: "联调甲" }),
    row({ id: "b", projectId: "p2", reportDate: "2026-10-16", content: "联调乙" }),
    row({ id: "c", projectId: "p1", reportDate: "2026-10-20", content: "验收甲" }),
  ];
  const f = (over: Partial<HoursFilter> = {}): HoursFilter => ({
    projectId: null,
    from: "",
    to: "",
    keyword: "",
    ...over,
  });
  assert.deepEqual(scopeDailyReports(rows, f()).map((r) => r.id), ["a", "b", "c"], "无条件 = 全部");
  assert.deepEqual(scopeDailyReports(rows, f({ projectId: "p1" })).map((r) => r.id), ["a", "c"]);
  assert.deepEqual(
    scopeDailyReports(rows, f({ from: "2026-10-01", to: "2026-10-01" })).map((r) => r.id),
    ["a"],
    "闭区间：两端都要含进来",
  );
  assert.deepEqual(scopeDailyReports(rows, f({ keyword: " 联调 " })).map((r) => r.id), ["a", "b"], "关键词两侧 trim");
  assert.deepEqual(scopeDailyReports(rows, f({ keyword: "验收" })).map((r) => r.id), ["c"]);
});

test("scopedHoursSummary 只算筛选后的行（M⑥ 靶：改成吃全量这条当场红）", () => {
  const rows = [
    row({ id: "a", projectId: "p1", reportDate: "2026-10-16", content: "甲", hours: 3 }),
    row({ id: "b", projectId: "p2", reportDate: "2026-10-16", content: "乙", hours: 5 }),
  ];
  const f = (over: Partial<HoursFilter> = {}): HoursFilter => ({ projectId: null, from: "", to: "", keyword: "", ...over });
  const onlyP1 = scopedHoursSummary(rows, f({ projectId: "p1" }), "day", [project({ id: "p1", name: "甲项目" }), project({ id: "p2", name: "乙项目" })]);
  assert.equal(onlyP1.total, 3, "筛到甲项目，总工时不能把乙项目的 5 也算进来");
  assert.equal(onlyP1.byProject.length, 1);
  assert.equal(onlyP1.buckets[0].count, 1);
  const all = scopedHoursSummary(rows, f(), "day", []);
  assert.equal(all.total, 8);
  assert.equal(all.count, 2);
});

test("按月分桶与组序：项目名按拼音、工时相同也要有确定次序", () => {
  const rows = [
    row({ id: "a", projectId: "p2", reportDate: "2026-10-16", hours: 2 }),
    row({ id: "b", projectId: "p1", reportDate: "2026-09-30", hours: 2 }),
    row({ id: "c", projectId: "p1", reportDate: "2026-09-01", hours: 1 }),
  ];
  const f: HoursFilter = { projectId: null, from: "", to: "", keyword: "" };
  const s = scopedHoursSummary(rows, f, "month", [project({ id: "p1", name: "乙项目" }), project({ id: "p2", name: "甲项目" })]);
  assert.deepEqual(s.buckets.map((b) => `${b.key}:${b.hours}`), ["2026-09:3", "2026-10:2"], "月桶按 key 升序");
  assert.deepEqual(s.byProject.map((p) => `${p.projectLabel}:${p.hours}`), ["甲项目:2", "乙项目:3"], "组序按拼音，不是码元序");
});

test("形状不对的行被跳过并计数，不许静默少算", () => {
  const rows = [
    row({ id: "a", reportDate: "2026-10-16" }),
    row({ id: "b", reportDate: "20261016" }),
    row({ id: "c", reportDate: "" }),
    row({ id: "d", reportDate: "2026-13-45" }),
  ];
  const s = scopedHoursSummary(rows, { projectId: null, from: "", to: "", keyword: "" }, "day", []);
  assert.equal(s.unparsed, 2, "无分隔与空串不是形状；「2026-13-45」形状对、历法错也算对（与后端 is_iso_date 同口径，引历法库就破零新依赖）");
  assert.equal(s.total, 2, "只有 a 与 d 进桶，各 1 小时");
  assert.equal(s.count, 2);
  assert.deepEqual(s.buckets.map((b) => b.key), ["2026-10-16", "2026-13-45"], "串比就是日期序，但历法错的值自己一格一桶");
});

test("previousReportDay 取最近一个有日报的那天，不是机械的前一天（D10）", () => {
  const rows = [
    row({ id: "f", reportDate: "2026-10-16" }),
    row({ id: "g", reportDate: "2026-10-15" }),
    row({ id: "m", reportDate: "2026-10-12" }),
  ];
  assert.equal(previousReportDay(rows, "2026-10-19"), "2026-10-16", "周一复制的是上周五，不是周日");
  assert.equal(previousReportDay(rows, "2026-10-16"), "2026-10-15", "目标日本身不算");
  assert.equal(previousReportDay(rows, "2026-10-01"), null, "之前一条都没有 ⇒ null，界面据此禁用按钮");
  assert.equal(previousReportDay([], "2026-10-19"), null);
});

// value import 走相对路径带 .ts 是「node --test 能直跑」的前提（先例 todo-view.ts:8 的注释）。
// `@/` 别名在纯 Node 下解析不到，而 tsc/vite 都看不出来 —— 这条守卫不写，以后有人顺手改成 `@/` 就炸在 CI 之外。
test("hours-summary.ts 的 value import 只走相对路径带 .ts", () => {
  const src = readFileSync(new URL("../src/lib/hours-summary.ts", import.meta.url), "utf8");
  for (const line of src.split(/\r?\n/)) {
    if (!/^import\b/.test(line) || /^import\s+type\b/.test(line)) continue;
    assert.match(line, /from "\.\/[^"]+\.ts"/, `value import 必须是相对路径带 .ts：${line}`);
  }
});
