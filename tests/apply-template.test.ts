import test from "node:test";
import assert from "node:assert/strict";
import { applyTemplate } from "../src/lib/apply-template.ts";
import type { ProjectInput } from "../src/types/project.ts";
import type { ProjectTemplate } from "../src/types/project-template.ts";

const base: ProjectInput = {
  name: "手工项目名",
  code: "P-1",
  manager: "原经理",
  customer: "原客户",
  contactName: "原联系人",
  contactPhone: "13800000000",
  contractNo: "HT-1",
  contractPeriod: "2026-01 ~ 2026-12",
  deliveryDeadline: "2026-12-31",
  status: "立项",
  parentId: "parent-1",
  tags: ["原标签"],
};

const tpl = (patch: Partial<ProjectTemplate> = {}): ProjectTemplate => ({
  id: "t1",
  name: "模板名",
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
  ...patch,
});

test("项目名永远保留，模板名不会变成项目名", () => {
  const got = applyTemplate(base, tpl({ name: "模板名" }));
  assert.equal(got.name, "手工项目名");
});

test("模板非空字段覆盖当前值", () => {
  const got = applyTemplate(base, tpl({ manager: "模板经理", customer: "模板客户", code: "TPL-1" }));
  assert.equal(got.manager, "模板经理");
  assert.equal(got.customer, "模板客户");
  assert.equal(got.code, "TPL-1");
});

test("模板 null 或空白字段不清空当前值", () => {
  const got = applyTemplate(base, tpl({ code: "", customer: null, manager: "   " }));
  assert.equal(got.code, "P-1");
  assert.equal(got.customer, "原客户");
  assert.equal(got.manager, "原经理");
});

test("status 总覆盖；tags 非空才覆盖，空 tags 保留原值", () => {
  const withTags = applyTemplate(base, tpl({ tags: ["模板标签"] }));
  assert.equal(withTags.status, "开发");
  assert.deepEqual(withTags.tags, ["模板标签"]);
  const noTags = applyTemplate(base, tpl({ tags: [] }));
  assert.deepEqual(noTags.tags, ["原标签"]);
});

test("返回新对象，不修改入参", () => {
  const before = JSON.stringify(base);
  const got = applyTemplate(base, tpl({ manager: "模板经理", tags: ["模板标签"] }));
  assert.notEqual(got, base);
  assert.equal(JSON.stringify(base), before);
  assert.notEqual(got.tags, base.tags);
});

test("套用模板保留当前 parentId", () => {
  const got = applyTemplate(base, tpl({ manager: "模板经理" }));
  assert.equal(got.parentId, "parent-1");
  const empty = applyTemplate({ ...base, parentId: null }, tpl());
  assert.equal(empty.parentId, null);
});
