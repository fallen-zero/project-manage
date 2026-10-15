import test from "node:test";
import assert from "node:assert/strict";
import { buildDueWarnings, localToday, DUE_HORIZON_DAYS } from "../src/lib/due-warning.ts";
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

test("localToday 回本机日历日的 ISO 形状", () => {
  assert.match(localToday(), /^\d{4}-\d{2}-\d{2}$/);
});
