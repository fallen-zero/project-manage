import test from "node:test";
import assert from "node:assert/strict";
import { buildDueWarnings, dueStateOf, localToday, scopeDueInput, DUE_HORIZON_DAYS } from "../src/lib/due-warning.ts";
import type { Todo } from "../src/types/todo.ts";
import type { Milestone } from "../src/types/milestone.ts";
import type { Project } from "../src/types/project.ts";

// today 一律喂常量，函数不读机器时钟：档位边界才是可证伪的。
const TODAY = "2026-10-14";
const iso = (offsetDays: number) => {
  const ms = Date.UTC(2026, 9, 14) + offsetDays * 86_400_000;
  return new Date(ms).toISOString().slice(0, 10);
};

const project = (over: Partial<Project> = {}): Project => ({
  id: "p1",
  name: "甲项目",
  code: "P-01",
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

const todo = (over: Partial<Todo> = {}): Todo => ({
  id: "t1",
  projectId: "p1",
  title: "待办一",
  dueDate: null,
  done: false,
  createdAt: "2026-10-01 00:00:00",
  updatedAt: "2026-10-01 00:00:00",
  ...over,
});

const milestone = (over: Partial<Milestone> = {}): Milestone => ({
  id: "m1",
  projectId: "p1",
  stage: "上线",
  planDate: null,
  note: "",
  done: false,
  createdAt: "2026-10-01 00:00:00",
  updatedAt: "2026-10-01 00:00:00",
  ...over,
});

const empty = { todos: [] as Todo[], milestones: [] as Milestone[], projects: [] as Project[] };

test("昨天到期 ⇒ overdue 且 days 为负", () => {
  const r = buildDueWarnings({ ...empty, todos: [todo({ dueDate: iso(-1) })], projects: [project()] }, TODAY);
  assert.equal(r.items.length, 1);
  assert.equal(r.items[0].state, "overdue");
  assert.equal(r.items[0].days, -1);
  assert.equal(r.items[0].projectLabel, "甲项目（P-01）");
});

test("今天到期 ⇒ soon（不是 overdue），days 为 0", () => {
  const r = buildDueWarnings({ ...empty, todos: [todo({ dueDate: TODAY })], projects: [project()] }, TODAY);
  assert.equal(r.items[0].state, "soon");
  assert.equal(r.items[0].days, 0);
});

test("第 7 天 ⇒ 仍进黄档（边界含）", () => {
  const r = buildDueWarnings({ ...empty, todos: [todo({ dueDate: iso(DUE_HORIZON_DAYS) })], projects: [project()] }, TODAY);
  assert.equal(r.items.length, 1);
  assert.equal(r.items[0].state, "soon");
});

test("第 8 天 ⇒ 不进预警", () => {
  const r = buildDueWarnings({ ...empty, todos: [todo({ dueDate: iso(DUE_HORIZON_DAYS + 1) })], projects: [project()] }, TODAY);
  assert.deepEqual(r.items, []);
});

test("勾掉的待办不算（判据③）", () => {
  const r = buildDueWarnings({ ...empty, todos: [todo({ dueDate: iso(-3), done: true })], projects: [project()] }, TODAY);
  assert.deepEqual(r.items, []);
});

test("勾掉的里程碑不算 —— D5 加那一列就是为了这一条", () => {
  const r = buildDueWarnings(
    { ...empty, milestones: [milestone({ planDate: iso(-30), done: true })], projects: [project()] },
    TODAY,
  );
  assert.deepEqual(r.items, [], "立项/评审的计划日期天然在过去，没 done 就会刷一屏误红");
});

test("没日期的行整个跳过，且不进 unparsed", () => {
  const r = buildDueWarnings(
    {
      todos: [todo({ id: "t-null", dueDate: null }), todo({ id: "t-empty", dueDate: "" })],
      milestones: [milestone({ id: "m-null", planDate: null })],
      projects: [project()],
    },
    TODAY,
  );
  assert.deepEqual(r.items, []);
  assert.equal(r.unparsed, 0, "空 = 没填，不是填错");
});

test("非 ISO 的交付截止 ⇒ 不亮但计入 unparsed（判据②，对 deadline 类它是唯一防线）", () => {
  const r = buildDueWarnings(
    { ...empty, projects: [project({ deliveryDeadline: "10月底前" })] },
    TODAY,
  );
  assert.deepEqual(r.items, []);
  assert.equal(r.unparsed, 1, "解析不了要响亮地说，不能静默少亮一条");
});

test("归档项目的交付截止不进预警，也不计 unparsed（判据⑤）", () => {
  const r = buildDueWarnings(
    { ...empty, projects: [project({ status: "归档", deliveryDeadline: iso(-5) })] },
    TODAY,
  );
  assert.deepEqual(r.items, []);
  assert.equal(r.unparsed, 0);
});

test("三来源汇总 + 排序：overdue 在前、再按日期、再按项目名", () => {
  const pA = project({ id: "pa", name: "乙项目", code: null, deliveryDeadline: iso(2) });
  const pB = project({ id: "pb", name: "丙项目", deliveryDeadline: iso(-2) });
  const r = buildDueWarnings(
    {
      todos: [todo({ id: "ta", projectId: "pa", dueDate: iso(1), title: "待办甲" })],
      milestones: [milestone({ id: "ma", projectId: "pb", stage: "上线", planDate: iso(-1) })],
      projects: [pA, pB],
    },
    TODAY,
  );
  assert.deepEqual(
    r.items.map((i) => [i.kind, i.date, i.state]),
    [
      ["deadline", iso(-2), "overdue"],
      ["milestone", iso(-1), "overdue"],
      ["todo", iso(1), "soon"],
      ["deadline", iso(2), "soon"],
    ],
    `实际顺序不对：${JSON.stringify(r.items)}`,
  );
  assert.equal(r.items[1].label, "上线", "里程碑那一格的 label 是阶段名");
  assert.equal(r.items[2].label, "待办甲");
  assert.equal(r.items[3].label, "交付截止");
});

test("horizonDays 传 0 ⇒ 只算今天与已逾期", () => {
  const r = buildDueWarnings(
    { ...empty, todos: [todo({ id: "t0", dueDate: TODAY }), todo({ id: "t1", dueDate: iso(1) })], projects: [project()] },
    TODAY,
    0,
  );
  assert.deepEqual(r.items.map((i) => i.id), ["t0"]);
});

// ── 终审 N1：上面那条排序用例四个日期全不同 ⇒ 永远走不到「再按项目名」那一级。
// 甲 U+7532 / 乙 U+4E59：码点序是「乙先」，拼音序（`localeCompare(…, "zh")`）是「甲先」⇒ 两条相反，
// 所以这一格的断言是有牙的（把 sort 里那行换回 `<` 码元比较就红）。
test("同一天两条 ⇒ 第三级键走项目名的拼音序，不是码点序", () => {
  assert.ok("乙".charCodeAt(0) < "甲".charCodeAt(0), "码点序里乙在前：这条用例的存在理由就是它与拼音序相反");
  const r = buildDueWarnings(
    {
      todos: [
        todo({ id: "t-yi", projectId: "py", dueDate: iso(2), title: "乙项目那条" }),
        todo({ id: "t-jia", projectId: "pj", dueDate: iso(2), title: "甲项目那条" }),
      ],
      milestones: [],
      projects: [project({ id: "py", name: "乙项目", code: null }), project({ id: "pj", name: "甲项目", code: null })],
    },
    TODAY,
  );
  assert.deepEqual(
    r.items.map((i) => `${i.projectLabel}/${i.label}`),
    ["甲项目/甲项目那条", "乙项目/乙项目那条"],
    `同日期同档位时要按项目名（拼音序）排，实际：${JSON.stringify(r.items.map((i) => i.projectLabel))}`,
  );
});

// ── 终审 I2：面板那个徽标与预警块**共用**下面这份单行判据（`milestone-panel.tsx` 的 `stateOf` 就是转调它）。
// 抽出来之前面板自己写第三份比较（字符串 `<` + 自己的 shiftDate），把 `<` 改成 `<=` 现有测试全绿
// ⇒ T7 变异 M⑥「打不红」；共用之后 M⑥ 有了可红处，读数见 `.superpowers/sdd/…/mutation-evidence.txt`。
test("dueStateOf 单行判据：昨天 overdue、今天 soon、第 7 天含边界、第 8 天不命中", () => {
  assert.deepEqual(dueStateOf(iso(-1), TODAY), { state: "overdue", days: -1 });
  // 今天算 soon 不算 overdue —— 这一行就是 M④/M⑥ 共用的靶点（`days < 0` 写成 `days <= 0` 当场红）
  assert.deepEqual(dueStateOf(TODAY, TODAY), { state: "soon", days: 0 });
  assert.deepEqual(dueStateOf(iso(DUE_HORIZON_DAYS), TODAY), { state: "soon", days: 7 });
  assert.equal(dueStateOf(iso(DUE_HORIZON_DAYS + 1), TODAY), null);
  assert.deepEqual(dueStateOf(TODAY, TODAY, 0), { state: "soon", days: 0 }, "horizon 传 0 时今天仍命中、明天不命中");
  assert.equal(dueStateOf(iso(1), TODAY, 0), null);
});

test("dueStateOf 对形状不对的值返回 null ⇒ 面板不许给它亮徽标", () => {
  for (const bad of ["", "2026.10.14", "2026-1-4", "10月底前", "2026-10-14 ", "2026/10/14"]) {
    assert.equal(dueStateOf(bad, TODAY), null, `「${bad}」不是 YYYY-MM-DD，不该有档位`);
  }
});

test("dueStateOf 把「形状对、历法错」的日期滚进下一档（I2 之后页面与面板同一读数）", () => {
  // `2026-09-45` 过 ISO_DATE 的形状检查，`Date.UTC(2026,8,45)` 把它滚成 2026-10-15 ⇒ days=1、soon。
  // 修面板之前这里是真分叉：面板的字符串比较判 overdue（红）、本函数判 soon（黄）。
  // 账本 Task 6 那条「/todos 显黄、面板无徽标」的旧描述方向反了，按这句实测改准（终审 triage #14）。
  assert.deepEqual(dueStateOf("2026-09-45", TODAY), { state: "soon", days: 1 });
  // 经 `<input type="date"> 录入不可达该值：浏览器只发 YYYY-MM-DD，且后端 norm_date 还挡形状。
});

// ── 终审 I1：预警块与计数原先吃的是未筛选的全量（只有列表行走过 scoped）。
// 页面本身没有组件测试网（门只有 `tsc` + `vite build`），所以把「三份一起缩」抽成纯函数来守这一格。
test("scopeDueInput：三份一起缩 ⇒ 别项目的行既不出现、也不计进 unparsed", () => {
  const input = {
    todos: [
      todo({ id: "t-jia", projectId: "pj", dueDate: iso(-1) }),
      todo({ id: "t-yi", projectId: "py", dueDate: iso(-1) }),
    ],
    milestones: [milestone({ id: "m-yi", projectId: "py", planDate: iso(-1) })],
    projects: [
      project({ id: "pj", name: "甲项目", code: null }),
      project({ id: "py", name: "乙项目", code: null, deliveryDeadline: "10月底前" }),
    ],
  };
  const all = buildDueWarnings(input, TODAY);
  assert.equal(all.items.length, 3, "不筛时三行都该在（下面那半条的对照读数）");
  assert.equal(all.unparsed, 1, "乙项目那个「10月底前」在全量口径里计 unparsed");

  const scoped = buildDueWarnings(scopeDueInput(input, "pj"), TODAY);
  assert.deepEqual(scoped.items.map((i) => [i.kind, i.id]), [["todo", "t-jia"]], "筛到甲项目 ⇒ 预警只剩甲的行");
  assert.equal(scoped.unparsed, 0, "别的项目的坏日期不该记进当前筛选的 unparsed 数");
  assert.equal(scopeDueInput(input, null), input, "null = 全部项目，原样返回同一份引用");
});

test("localToday 回本机日历日的 ISO 形状", () => {
  assert.match(localToday(), /^\d{4}-\d{2}-\d{2}$/);
});

// 阈值必须钉**值**：第7天/第8天那两条是按符号引用的，常量漂成 14 它们照样绿
// （T4 评审带回，T7 变异 M⑦ 打的就是这条 —— 没有这条断言，M⑦ 无处可红）。
test("DUE_HORIZON_DAYS 钉死为 7", () => {
  assert.equal(DUE_HORIZON_DAYS, 7);
});
