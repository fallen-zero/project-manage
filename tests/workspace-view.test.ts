import test from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_WORKSPACE_ID,
  WORKSPACE_NAME_MAX,
  activeWorkspaceId,
  deleteGuard,
  nameHint,
} from "../src/lib/workspace-view.ts";
import type { Workspace } from "../src/types/workspace.ts";

const ws = (patch: Partial<Workspace> = {}): Workspace => ({
  id: "ws-2",
  name: "客户乙",
  projectCount: 0,
  isActive: false,
  ...patch,
});

test("区名判据与后端同一口径：trim 非空、上限闭区间", () => {
  assert.equal(nameHint("   "), "工作区名称不能为空");
  assert.equal(nameHint("客户乙"), null);
  assert.equal(nameHint("区".repeat(WORKSPACE_NAME_MAX)), null, "40 字是闭区间");
  assert.match(nameHint("区".repeat(WORKSPACE_NAME_MAX + 1)) ?? "", /不能超过 40 个字符/);
});

test("删除禁用判据的三种原因与可删回 null", () => {
  assert.equal(deleteGuard(ws({ id: DEFAULT_WORKSPACE_ID }), null), "默认工作区不可删除");
  assert.equal(deleteGuard(ws(), "ws-2"), "当前工作区不可删除，请先切换到其他工作区");
  assert.match(deleteGuard(ws({ projectCount: 3 }), null) ?? "", /该工作区下还有项目/);
  assert.equal(deleteGuard(ws(), null), null, "空的非默认非当前区可以删");
});

test("多条守卫同时成立时，原因取后端顺序的第一条", () => {
  // 既是当前区、底下又有项目 ⇒ 得先念「当前工作区不可删除」，因为用户下一步动作是切走，不是删项目。
  assert.equal(deleteGuard(ws({ id: "ws-2", projectCount: 2 }), "ws-2"), "当前工作区不可删除，请先切换到其他工作区");
  // 默认区就算被切成当前区，也要先念「默认工作区不可删除」（它切不走）。
  assert.equal(deleteGuard(ws({ id: DEFAULT_WORKSPACE_ID, projectCount: 9 }), DEFAULT_WORKSPACE_ID), "默认工作区不可删除");
});

test("activeWorkspaceId 在没有 isActive 行时回 null", () => {
  assert.equal(activeWorkspaceId([ws({ id: "a", isActive: false }), ws({ id: "b", isActive: true })]), "b");
  assert.equal(activeWorkspaceId([ws(), ws({ id: "b" })]), null);
  assert.equal(activeWorkspaceId([]), null);
});
