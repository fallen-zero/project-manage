import test from "node:test";
import assert from "node:assert/strict";
import { groupByProject, sortTodos } from "../src/lib/todo-view.ts";
import type { Todo } from "../src/types/todo.ts";
import type { Project } from "../src/types/project.ts";

const t = (id: string, projectId: string, dueDate: string | null, createdAt: string, title = id, done = false): Todo => ({
  id,
  projectId,
  title,
  dueDate,
  done,
  createdAt,
  updatedAt: createdAt,
});

const project = (id: string, name: string): Project => ({
  id,
  name,
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
});

// 终审 N2：这条名字写着「未完成的在前」，但原来所有行都是 `done: false`
// ⇒ `sortTodos` 里那一条「done 沉底」的分支是**死代码**，谁也不会红。
// 补一行 done:true、并且给它**最早的截止日期**：分支被摘掉时它会排到最前，断言当场红。
// （裁定：行为保留、不改产品语义 —— 这条沉底本就是 spec 与需求都没要的超出项，已在 spec §十一 与计划里认账。）
test("按截止排序：未完成的在前、空日期落最后、同键按 createdAt 稳定", () => {
  const rows = [
    t("a", "p1", null, "2026-10-05 00:00:00"),
    t("b", "p1", "2026-10-20", "2026-10-01 00:00:00"),
    t("c", "p1", null, "2026-10-02 00:00:00"),
    t("d", "p1", "2026-10-10", "2026-10-01 00:00:00"),
    // 勾掉的：截止日期是全场最早的一条，但它必须沉到最后
    t("e", "p1", "2026-10-01", "2026-10-01 00:00:00", "已勾掉但日期最早", true),
  ];
  assert.deepEqual(
    sortTodos(rows, "byDue").map((x) => x.id),
    ["d", "b", "c", "a", "e"],
    "done:true 的那条必须排在最后（摘掉 `a.done !== b.done` 那一支它会因日期最早排到第一）",
  );
});

test("全部视图按创建时间，且不改动入参数组", () => {
  const rows = [t("a", "p1", null, "2026-10-03 00:00:00"), t("b", "p1", null, "2026-10-01 00:00:00")];
  const snapshot = rows.map((x) => x.id);
  assert.deepEqual(sortTodos(rows, "all").map((x) => x.id), ["b", "a"]);
  assert.deepEqual(rows.map((x) => x.id), snapshot, "sort 必须先复制，不许就地排用户的数组");
});

test("按项目分组：组按项目名排、组内保持视图序", () => {
  const rows = [
    t("a", "p2", "2026-10-30", "2026-10-01 00:00:00", "乙组一条"),
    t("b", "p1", "2026-10-10", "2026-10-02 00:00:00", "甲组一条"),
  ];
  const groups = groupByProject(rows, [project("p1", "甲项目"), project("p2", "乙项目")], "byDue");
  assert.deepEqual(groups.map((g) => g.projectLabel), ["甲项目", "乙项目"]);
  assert.deepEqual(groups[0].todos.map((x) => x.id), ["b"]);
  assert.equal(groups[1].todos[0].title, "乙组一条");
});
