import test from "node:test";
import assert from "node:assert/strict";
import {
  buildProjectForest,
  flattenProjectTree,
  parentCandidates,
} from "../src/lib/project-tree.ts";
import type { Project } from "../src/types/project.ts";

const project = (id: string, name: string, parentId: string | null = null): Project => ({
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
  status: "立项",
  tags: [],
  dirs: [],
  parentId,
});

const sample = () => [
  project("root", "总包"),
  project("a", "子A", "root"),
  project("a1", "孙A1", "a"),
  project("b", "子B", "root"),
];

test("多层森林保留输入同级顺序，并算出 depth", () => {
  const forest = buildProjectForest(sample());
  assert.deepEqual(forest.map((n) => n.project.id), ["root"]);
  assert.deepEqual(forest[0].children.map((n) => n.project.id), ["a", "b"]);
  assert.equal(forest[0].children[0].children[0].project.id, "a1");
  assert.equal(forest[0].children[0].children[0].depth, 2);
  assert.deepEqual(forest[0].children[1].children, []);
});

test("父项目缺失时按根处理，不丢项目", () => {
  const forest = buildProjectForest([project("x", "孤儿", "ghost")]);
  assert.deepEqual(forest.map((n) => n.project.id), ["x"]);
  assert.equal(forest[0].depth, 0);
  assert.deepEqual(forest[0].children, []);
});

test("自指项目按根处理且只出现一次", () => {
  const forest = buildProjectForest([project("x", "自指", "x")]);
  assert.deepEqual(forest.map((n) => n.project.id), ["x"]);
  assert.deepEqual(forest[0].children, []);
});

test("双节点环不递归死循环，两项都按根各出现一次", () => {
  const forest = buildProjectForest([
    project("c1", "环一", "c2"),
    project("c2", "环二", "c1"),
  ]);
  assert.deepEqual(forest.map((n) => n.project.id), ["c1", "c2"]);
  assert.deepEqual(forest.map((n) => n.children.length), [0, 0]);
  assert.deepEqual(flattenProjectTree(forest).map((n) => n.project.id), ["c1", "c2"]);
});

test("扁平化是先序，depth 与树一致", () => {
  const flat = flattenProjectTree(buildProjectForest(sample()));
  assert.deepEqual(flat.map((n) => n.project.id), ["root", "a", "a1", "b"]);
  assert.deepEqual(flat.map((n) => n.depth), [0, 1, 2, 1]);
});

test("新建项目的父项候选是全部项目的先序扁平表", () => {
  const options = parentCandidates(sample(), null);
  assert.deepEqual(options.map((o) => o.id), ["root", "a", "a1", "b"]);
  assert.deepEqual(options.map((o) => o.depth), [0, 1, 2, 1]);
  assert.equal(options[0].name, "总包");
  assert.equal(options[0].status, "立项");
});

test("编辑时排除自己与全部后代，但保留父/祖先/兄弟", () => {
  const options = parentCandidates(sample(), "a");
  assert.deepEqual(options.map((o) => o.id), ["root", "b"]);
  assert.deepEqual(options.map((o) => o.depth), [0, 1]);
  assert.deepEqual(parentCandidates(sample(), "root"), []);
});

test("构建森林不改动入参数组与对象", () => {
  const items = sample();
  const before = JSON.stringify(items);
  buildProjectForest(items);
  parentCandidates(items, "a");
  assert.equal(JSON.stringify(items), before);
});
