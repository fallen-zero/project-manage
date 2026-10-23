import test from "node:test";
import assert from "node:assert/strict";
import { actionLabel, formatAuditTime, formatChangeText } from "../src/lib/audit-view.ts";
import type { AuditChange } from "../src/types/project-audit.ts";

const change = (patch: Partial<AuditChange> = {}): AuditChange => ({
  field: "customer",
  label: "客户名称",
  oldValue: "甲客户",
  newValue: "乙客户",
  ...patch,
});

test("操作类型标签", () => {
  assert.equal(actionLabel("create"), "新建");
  assert.equal(actionLabel("update"), "编辑");
  assert.equal(actionLabel("delete"), "删除");
});

test("新建显示新值", () => {
  assert.equal(formatChangeText("create", change({ oldValue: null })), "客户名称：乙客户");
});

test("编辑显示旧值 → 新值，空值显示 —", () => {
  assert.equal(formatChangeText("update", change()), "客户名称：甲客户 → 乙客户");
  assert.equal(formatChangeText("update", change({ newValue: null })), "客户名称：甲客户 → —");
});

test("删除显示删除前值", () => {
  assert.equal(formatChangeText("delete", change({ newValue: null })), "客户名称：甲客户");
});

test("时间格式化：非法值原样，合法 UTC 输出本地形状", () => {
  assert.equal(formatAuditTime("not-a-time"), "not-a-time");
  assert.match(formatAuditTime("2026-10-23 02:00:00"), /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
});
