import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { bundleToSections, clusterNote, emptyStateKind, isNewest } from "../src/lib/search-order.ts";
import type { SearchBundle } from "../src/types/search.ts";

const emptyCluster = { clusters: [], hiddenClusters: 0 };

function bundle(over: Partial<SearchBundle> = {}): SearchBundle {
  return {
    query: "验收",
    projects: [],
    projectsHidden: 0,
    ledger: emptyCluster,
    docs: emptyCluster,
    relaxed: false,
    indexedProjects: 0,
    ...over,
  };
}

test("seq 守卫只认最新那一次", () => {
  assert.equal(isNewest(3, 3), true);
  assert.equal(isNewest(2, 3), false, "慢回来的旧请求不许盖掉新 bundle");
});

test("三段全空时不产出任何段", () => {
  assert.deepEqual(bundleToSections(bundle()), []);
});

test("有内容的段按 项目 / 台账 / 正文 顺序产出；平铺段的存在性看 projects.length", () => {
  // 存在性判据两段不同形（Task 7 派发前预检订正）：平铺段看 `projects.length`，聚簇段看 `clusters.length`
  // —— 原本这条的名字写「段里没有簇也能产出」，那是实现里**没有**的行为（`clusters.length === 0` 就不产出，
  // 第 2 条与第 4 条的第一句断言正是钉这个的），照旧名读会诱导出「把空段也产出」的反向整改。
  // 「有簇但簇里 `items` 为空」照样产出这一格，由第 4、5 条用 `items: []` 的夹具守着，不在本条。
  // 段序守卫（Task 7 首轮评审 I-1 整改）：原本这条只喂单段夹具，名字里的「按 项目 / 台账 / 正文 顺序」
  // 压根没有被断言 —— 控制方实测过：把 `bundleToSections` 里 ledger 与 docs 两个 `if` 块整个交换，
  // 8 条照样全绿（`pass 8 / fail 0 / exit=0`）。所以必须有一个**三段同时非空**的夹具来钉住 `out` 的顺序；
  // 顺手把 `relaxed` 也塞进这个夹具，让「放宽只落在正文段」拿到平铺段与台账段的反面断言（评审 M-2）。
  const hit = { source: "project", id: "p1", projectId: "p1", projectName: "甲", title: "甲", detail: "" };
  const sections = bundleToSections(bundle({ projects: [hit] }));
  assert.deepEqual(sections.map((s) => s.kind), ["projects"]);
  assert.equal(sections[0].title, "项目档案");
  const cluster = { projectId: "p1", projectName: "甲", items: [], hidden: 0 };
  const all = bundleToSections(
    bundle({
      projects: [hit],
      ledger: { clusters: [cluster], hiddenClusters: 0 },
      docs: { clusters: [cluster], hiddenClusters: 0 },
      relaxed: true,
    }),
  );
  assert.deepEqual(all.map((s) => s.kind), ["projects", "ledger", "docs"]);
  assert.equal(all[0].note, null, "放宽属于正文段，平铺段不该跟着说");
  assert.equal(all[1].note, null, "放宽属于正文段，台账段不该跟着说");
  assert.equal(all[2].note, "含前缀放宽匹配");
});

test("三个截断计数各自产出一行说明，互不借用", () => {
  assert.equal(bundleToSections(bundle({ projectsHidden: 5, projects: [] })).length, 0, "只有计数没有行时该段仍不产出");
  const withRows = bundle({
    projects: [{ source: "project", id: "p1", projectId: "p1", projectName: "甲", title: "甲", detail: "" }],
    projectsHidden: 5,
  });
  assert.equal(bundleToSections(withRows)[0].note, "还有 5 个项目未显示");
  const ledger = bundle({ ledger: { clusters: [{ projectId: "p1", projectName: "甲", items: [], hidden: 0 }], hiddenClusters: 3 } });
  assert.equal(bundleToSections(ledger)[0].note, "另有 3 个项目未显示");
  const docs = bundle({ docs: { clusters: [{ projectId: "p1", projectName: "甲", items: [], hidden: 0 }], hiddenClusters: 1 } });
  assert.equal(bundleToSections(docs)[0].note, "另有 1 个项目未显示");
});

test("relaxed 只在正文段追加一次放宽说明", () => {
  const docs = bundle({ relaxed: true, docs: { clusters: [{ projectId: "p1", projectName: "甲", items: [], hidden: 0 }], hiddenClusters: 2 } });
  assert.equal(bundleToSections(docs)[0].note, "另有 2 个项目未显示；含前缀放宽匹配");
  const ledger = bundle({ relaxed: true, ledger: { clusters: [{ projectId: "p1", projectName: "甲", items: [], hidden: 0 }], hiddenClusters: 0 } });
  assert.equal(bundleToSections(ledger)[0].note, null, "放宽属于正文段，台账段不该跟着说");
});

test("簇内 hidden 的行内说明", () => {
  assert.equal(clusterNote(0), null);
  assert.equal(clusterNote(2), "还有 2 条未显示");
});

test("空态分「还没建索引」与「有索引但没命中」两句", () => {
  assert.equal(emptyStateKind(bundle()), "not-indexed");
  assert.equal(emptyStateKind(bundle({ indexedProjects: 3 })), "no-hit");
  assert.equal(emptyStateKind(bundle({ projects: [{ source: "project", id: "p1", projectId: "p1", projectName: "甲", title: "甲", detail: "" }] })), "results");
});

// 运行时无 value import 是「node --test 能直跑这个文件」的前提：`import type` 会被类型剥离擦掉，
// 而任何 value import（例如 `@/lib/api`）都会把 @tauri-apps 拉进来，在纯 Node 里当场炸。
// 这条不守就等于「以后有人往 search-order.ts 加了个 value import，测试文件在 node 下跑不动，
// 而 tsc 完全看不出问题」。
test("search-order.ts 运行时无 value import", () => {
  const src = readFileSync(new URL("../src/lib/search-order.ts", import.meta.url), "utf8");
  for (const line of src.split(/\r?\n/)) {
    if (/^import\b/.test(line) && !/^import\s+type\b/.test(line)) {
      assert.fail(`search-order.ts 出现 value import：${line}`);
    }
  }
});
