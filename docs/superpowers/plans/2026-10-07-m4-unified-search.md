# M4 首屏统一检索 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 M3 留下的两条互不相认的检索通路（台账明文列 LIKE、正文 FTS5）合成首屏的一次 IPC 统一结果，三段渲染、段内按项目聚簇，并能点开正文命中看原文高亮。

**Architecture:** 合并与聚合全在 Rust 侧：`search::unified_bundle` 同时调 `search::field_hits` 与 `index_store::doc_hits`，按「项目档案 / 台账条目 / 文件正文」三段产出带三级截断计数的 `SearchBundle`；新增 `doc_preview.rs` 按 `index_docs` 行 id 从磁盘重抽原文、在原文上做大小写不敏感子串高亮并按 UTF-16 码元交回 range。前端只做三段渲染与预览对话框，不碰文件系统、不算聚合口径。

**Tech Stack:** Rust 2021 + Tauri 2 + rusqlite(bundled/FTS5) + jieba-rs + serde(rename_all = camelCase)；React 19 + zustand + Tailwind 工具类 + `@tauri-apps/plugin-{opener,dialog,clipboard-manager}`；前端断言用仓库自带 Node 的 `node --test`（不新增依赖）。

**Spec:** `docs/superpowers/specs/2026-10-07-m4-unified-search-design.md`（决策 D1–D7、契约、排序与截断口径、预览通路、错误码表、测试策略、真机验收与性能实测都在那份里；本计划是它的任务化，冲突时以 spec 为准，spec 与 `docs/技术方案.md` 冲突时以 `docs/技术方案.md` 为准并回写）。

## Global Constraints

每个任务的验收都隐含这一节，逐条照抄 spec 与 `docs/开发进度.md`：

- **工具绝不动被索引的原始文件**：只登记路径、只读文件。索引与检索对磁盘的唯一动作是「读」，这条红线的实现只需要 grep `src-tauri/src/extract.rs` 的 `File::open`（今天恰好两处：`:44` 与 `:130`）。新增代码**不许**在任何地方打开被索引文件之外的写句柄。
- **夹具只能在 `tempfile::tempdir()` 与内存库 `db::open_in_memory()` 里造**；真实数据目录 `%APPDATA%\dev.zero.pfm\`（库文件名 `ledger.db`，见 `db.rs:8`）是只读红线，真机验证前拍 size+纳秒 mtime 快照、验证后逐文件对上。
- **真机验证一律用一次性 identifier** `dev.zero.pfm.m4test`，跑完删净；绝不为验证改 `src-tauri/tauri.conf.json`，`-c '{...}'` 那段 JSON 被 git-bash→npm→Windows 三层引号咬碎时改用仓库外的一次性 override。
- **子代理一律不许跑 `tauri dev` / `npm run dev`**（要跑由控制方明确授予）。因此「UI 实际渲染未验证」必须在报告里如实写。
- **入库与查询同一个分词器**：查询侧的切词只从 `tokenize::query_terms` 出，预览高亮也用它。`tokenize.rs` 内 `.cut(` 恰好出现 1 次是这条的结构守卫。
- **密文列不参与任何检索**：任何新增 SQL 里不许出现 `*_cipher` / `*_nonce`。
- **每条测试都要能被它所守护的表达式打红**：写测试前先在被守护表达式上做反值心算；恒绿断言（复制实现照样绿、断言对象压根不存在）算缺陷不算守护。
- 门禁四条，全部要实跑并取真退出码（管道 `tail` 会吞掉 `$?`，用 `; echo exit=$?`）：
  - `cd src-tauri && cargo test --lib` → 期望条数见各任务，`0 failed`
  - `cd src-tauri && cargo clippy --lib -- -D warnings` → exit 0 且输出里 0 条 warning
  - `cd src-tauri && cargo clippy --lib --all-targets -- -D warnings` → 同上（这条把测试代码也扫了）
  - `npm run build`（= `tsc && vite build`）→ exit 0
- 本机物理内存 16 GB，`rustc` 会因内存压力报**假失败**（`os error 1455` / 内部编译错误）：先重跑一次再判定是不是真错。跑 `tauri dev` 时不要同时跑 `cargo test`/`clippy`（抢同一个 `target/`）。
- 输出语言一律中文（注释、文案、commit message、报告）；错误码、SQL 字面量、JSON key 保持英文。
- 前端样式只用 Tailwind 工具类；确实需要独立样式文件时用 `.scss`，不新增裸 `.css`（唯一既有例外是 `src/styles.css`）。
- Git：仓库无远端，**只本地提交，绝不 push、绝不 amend、不 `--no-verify`、不 `git add -A`**；按文件 name 逐个 stage。
- 验证脚本**只回断言结果**（布尔 / 条数 / `matchedBy` / 文案前缀 / 尺寸），绝不打印抽出的正文或任何解密内容。

---

## 已实测事实（写计划时逐条跑过，不是推测）

这些数字是各任务 Expected 的来源，实现者不必复证，但改动让它们失效时要照实报。

| 事实 | 实测值 | 来源 |
|---|---|---|
| `cargo test --lib` 起点 | **104 passed / 0 failed** | 本轮 `git rev-parse HEAD` = `34a48e1`，与 `docs/开发进度.md:134` 一致 |
| 各模块条数 | db 8 / extract 14 / index_job 13 / index_scan 10 / index_store 14 / ledger 12 / project 10 / search 9 / tokenize 5 / vault 9 | 同上，`cargo test --lib` 按模块名筛选逐条数出 |
| `generate_handler!` 条数 | **39** | `src-tauri/src/lib.rs:562-602` |
| `AppError::io` 的错误码 | **`fs_failed`**（`error.rs:21-23`），`From<rusqlite::Error>` 是 `db_failed`（`error.rs:45`） | 实读 |
| jieba 查询侧切词 | `cut("验收")=["验收"]`、`cut("验收，")=["验收","，"]`、`cut("，。！ ")=["，","。","！"," "]`、`cut("合同 报价")=["合同"," ","报价"]`、`cut("付款条件")=["付款条件"]` | 仓库外探针（`rustc --extern jieba_rs=target/debug/deps/libjieba_rs-079f0c496641c0db.rlib`） |
| `query_terms` 的实际输出（Task 5 的用例向量来源） | `payment→["payment"]`、`Payment→["Payment"]`、`PAYMENT→["PAYMENT"]`、`验收，→["验收"]`、`合同 报价→["合同","报价"]`、`payment terms→["payment","terms"]`、`abc123→["abc123"]`、`维保→["维保"]`、`付款条件→["付款条件"]` | 同一探针，按 Task 1 的 `query_terms` 定义（`cut(q,true)` + trim + `any(is_alphanumeric)`）逐字复刻后实测 |
| jieba 入库侧切词 | `cut_for_search("里面只有付款条件与验收流程")` = `里面 只有 付款 条件 付款条件 与 验收 流程`；`cut_for_search("报价单 …")` 给出**重叠**子词 `报价 价单 报价单` | 同一探针 |
| 复合词重复的真实形态 | `clean_snippet` 后读成「里面只有付款条件**付款条件**与验收流程」，即 `index_store.rs:210-213` 契约注释里那条探针结果 | 探针 + `index_store.rs` 注释 |
| `node --test` 能直跑 `.ts` | 本机 PATH 上 node = **v26.3.0**（harness 报的 v24.18.0 不是仓库用的那个），类型剥离免 flag，`2 pass / exit 0` 已实测 | 本轮探针目录 `%TEMP%\m4-node-ts-probe` |
| `node --test` 的静默空跑 | glob 没匹配到文件时 **exit 0 且 tests 0**；`node --test tests`（目录实参）exit 1；`--test-match` 组合 exit 9 | 同一探针 |
| `@types/node` | devDependencies 里**没有**（`package.json`），而 `tsconfig.json` 的 `include` 只有 `["src"]` → 测试文件必须放 `src/` 之外 | 实读 |
| 库文件名 | `ledger.db`（不是 pm.db） | `src-tauri/src/db.rs:8` |
| 真机 CDP 手法 | `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" npm run tauri dev` + Node 全局 WebSocket 发 `Runtime.evaluate`；React 受控输入要用原型 `value` setter 再派发 `input` 事件 | `docs/开发进度.md:31` |
| `write_doc` 可直接在别的模块的测试里调用 | `pub fn write_doc(conn, project_id: &str, file: &ScannedFile, outcome: DocOutcome) -> AppResult<i64>`，`ScannedFile` 五个字段全 `pub` | `index_store.rs:27`、`index_scan.rs:47-53` |

**条数链（计划承诺的唯一口径，与各任务正文里写死的 Expected 逐条对齐）**：Task 1 → 106（tokenize +2），Task 2 → 106（extract +1、index_job −1，总数不变），Task 3 → 108（tokenize +2），Task 4 → **120**（search +11、index_store +1），Task 5 → 134（新增 doc_preview 模块 14 条），Task 6 → 134（0 条新 Rust 测试，handler 39 → 41），Task 7 → Rust 侧仍 134、`node --test` **pass 8**，Task 8/9 不新增单测。终态分模块：tokenize 9 / extract 15 / index_job 12 / index_store 15 / search 20 / doc_preview 14，加不变的 db 8 / index_scan 10 / ledger 12 / project 10 / vault 9 = **134**。

> 这条链在 Task 4 首轮评审后从 119/133 上调过一处：台账段（三段的三分之一）在原本 10 条测试里只被断言过「为空」，`partition` 谓词的变异因此是等价变异、无人捕获，所以补了第 11 条 `ledger_section_clusters_its_own_hits_instead_of_leaking_into_projects`。凡引用旧数（119/133/search 19）的下游正文都要一起改。

## 文件结构（谁负责什么）

```
src-tauri/src/
  tokenize.rs      [改] MAX_QUERY_CHARS 的家、query_terms()（查询侧唯一切词）、clean_snippet 的三连折叠
  extract.rs       [改] 收 pub(crate) fn panic_to_err（从 index_job 搬来）+ 它的直测
  index_job.rs     [改] 删私有 panic_to_err，改引 extract::panic_to_err；删那条直测
  index_store.rs   [改] indexed_project_count()；摘要串契约注释更正；一条折叠的集成断言
  search.rs        [改] Cluster<T> / ClusterSection<C> / SearchBundle + unified_bundle()
  doc_preview.rs   [新] 预览：查库 → 抽原文（包 panic 边界）→ 定位 → 窗口化 → UTF-16 range
  lib.rs           [改] `mod doc_preview;`（归 Task 5，否则本文件的测试根本不参与编译）+ 两条命令 + generate_handler（39 → 41，归 Task 6）

src/
  types/search.ts               [改] SearchBundle / Cluster<T> / ClusterSection<T> / DocPreview
  lib/api.ts                    [改] searchAll / docPreview 两个封装
  lib/search-order.ts           [新] 纯逻辑：seq 守卫、bundle → 渲染清单与截断说明、空态判据（node --test 的对象）
  stores/local-search.ts        [改] hits: FieldHit[] → bundle: SearchBundle | null，保留 seq 守卫
  pages/search.tsx              [改] 只留输入框 + debounce + store 接线 + 三种空态
  components/search-bundle.tsx  [新] 三段 + 簇 + 「还有 N 条」+ 段级说明
  components/doc-preview-dialog.tsx [新] 原文窗口 + 高亮 + 三个动作（打开文件 / 打开所在目录 / 复制路径）
tests/                          [新目录] search-order.test.ts（在 src/ 之外，不进 tsc 的 program）
package.json                    [改] scripts.test = "node --test \"tests/**/*.test.ts\""
```

切分依据：`unified_bundle` 与 `doc_preview` 是两个可独立评审的收口（前者纯库内聚合、后者跨磁盘与偏移算术），各自的测试面也不同；IPC 接线单独一个任务是因为它的正确性只能由真机点验兜（事件名与命令名两侧都是字面量，tsc 抓不到拼错）。

### Task 1: 查询侧唯一切词入口 + 共享长度上限

**Files:**
- Modify: `src-tauri/src/tokenize.rs`（`:42` `query_expression` 拆出 `query_terms`；新增 `MAX_QUERY_CHARS`；tests mod 追加 2 条）
- Modify: `src-tauri/src/search.rs:87`（字面量 128 → 引常量）
- Modify: `src-tauri/src/index_store.rs:262`（字面量 128 → 引常量）

**Interfaces:**
- Consumes: 无（本任务是链条起点）
- Produces:
  - `pub const MAX_QUERY_CHARS: usize = 128;`（`tokenize.rs`）
  - `pub fn query_terms(query: &str) -> Vec<String>`（`tokenize.rs`）—— Task 5 的预览高亮用它拿词
  - `pub fn query_expression(query: &str, prefix: bool) -> Option<String>` 签名不变、行为不变

- [ ] **Step 1: 写两条红测试**

追加到 `src-tauri/src/tokenize.rs` 的 `mod tests` 末尾（`:192` 之后、闭合 `}` 之前）：

```rust
    /// 查询侧唯一切词入口的三条行为：丢空白词、丢纯标点词、保持 `cut` 的原序。
    /// 期望向量不是想当然：本轮在仓库外用同一版 jieba-rs 实测过
    /// `cut("合同 报价")` 确实吐出一枚独立的 `" "` 词（被 `trim` + `is_empty` 丢掉），
    /// `cut("验收，")` = `["验收", "，"]`。
    #[test]
    fn query_terms_drops_blanks_and_punctuation_only_words() {
        assert_eq!(query_terms("验收"), vec!["验收"]);
        assert_eq!(
            query_terms("验收，"),
            vec!["验收"],
            "纯标点词进不了索引，留在查询里只会把整条 AND 变成 0 命中"
        );
        assert_eq!(query_terms("合同 报价"), vec!["合同", "报价"], "丢掉空白词后，词序就是 cut 的返回序");
        assert!(query_terms("，。！ ").is_empty(), "全标点时一个词都不产出");
        assert!(query_terms("").is_empty());
        assert!(query_terms("   ").is_empty());
    }

    /// 「入库与查询同一个分词器」这条红线的**结构**守卫：查询侧只允许有一处 `cut`。
    /// 为什么不写成行为断言：把 `query_expression` 里那份切词复制一份出来，两边结果照样相等，
    /// 任何行为断言都绿 —— 那条断言不可证伪。数源码里的出现次数才守得住「别再加第二套词表」。
    /// `index_text` 用的是 `.cut_for_search(`，与本 needle 不重叠（needle 要求左括号紧跟 `cut`）。
    /// needle 必须用 `concat!` 拼：写成字面量会数到它自己。
    #[test]
    fn query_side_has_exactly_one_cut() {
        let src = include_str!("tokenize.rs");
        let n = src.match_indices(concat!(".cut", "(")).count();
        assert_eq!(n, 1, "查询侧的切词只能有 `query_terms` 里那一处，实测 {n} 次");
    }
```

Run: `cd src-tauri && cargo test --lib tokenize; echo exit=$?`
Expected: 编译失败，`cannot find function query_terms in this scope`（未定义，不是断言失败）。

- [ ] **Step 2: 实现 `MAX_QUERY_CHARS` 与 `query_terms`**

在 `tokenize.rs` 的 `use crate::...` 位置（本文件目前只 `use std::sync::OnceLock; use jieba_rs::Jieba;`，`:8-10`）之后插入常量：

```rust
/// 查询串长度上限（按 char 计）。两条检索通路的边界守卫都引这一个数：
/// 之前 `search.rs:87` 与 `index_store.rs:262` 各写了一遍字面量 128，改一处就会让两边口径分家。
/// 家放在这里而不是 `search.rs`：`index_store` 本来就 `use crate::tokenize::...`，
/// 反过来引 `search.rs` 会新造一条 `index_store → search` 的反向依赖，只为搬一个数。
pub const MAX_QUERY_CHARS: usize = 128;
```

把 `query_expression`（`:42-64`）整段替换为下面两个函数——切词部分原样搬进 `query_terms`，注释跟着搬：

```rust
/// 查询侧唯一的切词入口：`cut` + 去空白 + 丢纯标点词，返回 `cut` 的原序。
/// 两个消费方：`query_expression` 拼 FTS5 语法，`doc_preview` 拿它决定原文里高亮哪些词。
/// 必须是同一个入口——预览若另起一次切词，两处词表会随词典版本各自漂移，
/// 表现为「列表里摘要标出了这个词、点开原文却没有标记」。
pub fn query_terms(query: &str) -> Vec<String> {
    jieba()
        .cut(query, true)
        .into_iter()
        .map(|x| x.word.trim().to_owned())
        // 纯标点词永不进索引，留在查询里只会把整条 AND 变成 0 命中（实测「验收，」即如此）。
        // 这里用 is_alphanumeric 而不是「非空白」，是因为它和索引侧几乎同口径：unicode61 默认只把
        // `L* N* Co` 当词字符，标点 P* 与符号 S*（™ € ± ★）都是分隔符、永不进索引，
        // 所以丢弃「整词无字母数字」没有信息损失。唯一比索引侧窄的是私有使用区 Co——它是词字符
        // 而 is_alphanumeric 回 false，于是全由 Co 组成的查询词会被丢掉、返回无结果而不是错结果。
        // 本项目索引的是中/英/数字文本，故判据保持不变，只在此登记这一处口径差。
        .filter(|t| !t.is_empty() && t.chars().any(|c| c.is_alphanumeric()))
        .collect()
}

/// 查询侧唯一出口。每个词都包成 `"词"`，FTS5 的操作符（`NEAR`/`*`/`-`/`|`/引号）
/// 就只能是普通字符，查询串无法注入语法；串内的 `"` 按 FTS5 规则翻倍成 `""`。
///
/// 前缀查询的关键约束：`*` 必须写在引号外面。`"维保"*` 能命中索引里的「维保期」，
/// `"维保*"` 里的 `*` 只是词内一个字面字符，永远命不中。
pub fn query_expression(query: &str, prefix: bool) -> Option<String> {
    let terms: Vec<String> = query_terms(query)
        .into_iter()
        .map(|t| {
            let quoted = format!("\"{}\"", t.replace('"', "\"\""));
            if prefix {
                format!("{quoted}*")
            } else {
                quoted
            }
        })
        .collect();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}
```

- [ ] **Step 3: 两处 128 引常量**

`src-tauri/src/search.rs:87` 的 `if q.chars().count() > 128 {` 改为 `if q.chars().count() > crate::tokenize::MAX_QUERY_CHARS {`，并在 `use crate::error::{AppError, AppResult};`（`:13`）下面加一行 `use crate::tokenize::MAX_QUERY_CHARS;`，那行判断随即简化为 `if q.chars().count() > MAX_QUERY_CHARS {`。

`src-tauri/src/index_store.rs:262` 同样改成 `if q.chars().count() > crate::tokenize::MAX_QUERY_CHARS {`。本文件已经在 `:17` 附近 `use crate::tokenize::{index_text, query_expression};`，把它改成 `use crate::tokenize::{index_text, query_expression, MAX_QUERY_CHARS};`，判断写成 `> MAX_QUERY_CHARS`。

Run: `cd src-tauri && cargo test --lib tokenize; echo exit=$?`
Expected: `7 passed; 0 failed`（tokenize 原 5 条 + 本任务 2 条）。

Run: `cd src-tauri && cargo test --lib; echo exit=$?`
Expected: `106 passed; 0 failed`（基线 104 + 2）。

- [ ] **Step 4: 结构门禁实跑（不是口头承诺）**

Run:
```bash
cd src-tauri && grep -rn "chars().count() > 128" src/ ; echo "exit=$?"
grep -n "\.cut(" src/tokenize.rs | cat; echo "cut_hits=$(grep -o '\.cut(' src/tokenize.rs | wc -l)"
```
Expected: 第一条 `exit=1`（0 命中 = 两处字面量都搬走了；`index_store.rs:642` 测试文案里的「超 128 字」不算，所以只 grep 比较式而不 grep 裸 128）。第二条只列出 `query_terms` 内那一行，`cut_hits=1`。

- [ ] **Step 5: 变异取证（证明两条新测试都守得住）**

1. 把 `query_expression` 改回自带一份 `jieba().cut(query, true) + filter`（即复制而不是引 `query_terms`）：`cargo test --lib tokenize` 必须让 `query_side_has_exactly_one_cut` 红（`cut_hits=2`），而其余 6 条**全绿**——这正是「行为断言守不住同源」的现场证据。改回来。
2. 把 `query_terms` 的 `filter` 判据换成 `!t.is_empty()`（只丢空白、不丢纯标点）：`query_terms_drops_blanks_and_punctuation_only_words` 必须红。**不许**只留第 1 条就交。

Run: `cd src-tauri && cargo test --lib tokenize; echo exit=$?`
Expected: 变异回滚后 `7 passed; 0 failed`。

- [ ] **Step 6: clippy 两道 + 提交**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings; echo exit=$?
cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
```
Expected: 两条都是 `exit=0` 且输出 0 条 warning。

```bash
cd E:/zero/demo/2026/project-files-manage && git add src-tauri/src/tokenize.rs src-tauri/src/search.rs src-tauri/src/index_store.rs
git commit -m "refactor: M4 查询侧唯一切词入口 query_terms，128 长度上限提成共享常量"
```

---

### Task 2: 把 `panic_to_err` 从作业层提到抽取层

预览要在 IPC 线程上抽盘，而 `index_job.rs:146` 的 `panic_to_err` 是私有的、只守索引那一条路。本任务把它提到 `extract.rs` 做成 `pub(crate)`，让索引与预览共用一个实现——这是一条跨模块不变量：**任何按 path 调用抽取器的地方都必须包 panic 边界**。

**Files:**
- Modify: `src-tauri/src/extract.rs`（新增 `pub(crate) fn panic_to_err`；tests mod 收那条直测；`:244` 与 `:580` 两处注释指针改名）
- Modify: `src-tauri/src/index_job.rs`（删 `:138-158` 的私有实现与注释、删 `:785-806` 的直测、`:18` 的 import 加一项、`:224-227` 调用点注释改名）

**Interfaces:**
- Consumes: `extract_text`（签名不变）
- Produces: `pub(crate) fn panic_to_err<T, F: FnOnce() -> T>(f: F, what: &str) -> AppResult<T>`（`extract.rs`）—— Task 5 直接调它；错误码仍是 `extract_failed`

- [ ] **Step 1: 把函数与它的直测搬到 `extract.rs`**

在 `src-tauri/src/extract.rs` 的 `pub fn extract_text`（`:267` 附近）**之前**插入（函数体逐字取自 `index_job.rs:146-158`，只改可见性与注释里的归属）：

```rust
/// 把「可能 panic 的调用」统一转成 `AppResult`：panic → `Err(extract_failed)`。
/// 索引（`index_job::run_pass`）与预览（`doc_preview`）两条路共用这一个实现，
/// 于是「一个坏文件只废它自己」这条红线只有一条路径可以写错。
/// 为什么必须有这一层：`技术方案.md` 2.2 把 `pdf-extract` **和 zip+XML 解析**并列列为 panic 源，
/// 而本文件里只有 `pdf_text` 自己包了 `catch_unwind`；docx/pptx/xlsx/txt 四条路径的 panic
/// 原本会一路炸穿调用方。M4 之前这个助手在 `index_job.rs` 里是私有的，预览那条路一道边界都没有。
/// 前提：release 保持 unwind（见本文件 `release_profile_does_not_abort_so_panic_guards_work`），
/// 否则这里抓不到任何东西。
///
/// **跨模块不变量：任何按 path 调用抽取器的地方都必须包这一层。** 新增读盘通路的人不必再读
/// `index_job.rs` 才能发现这一点——它写在抽取层，就在抽取层的门口。
pub(crate) fn panic_to_err<T, F: FnOnce() -> T>(f: F, what: &str) -> AppResult<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => Ok(v),
        Err(_) => Err(AppError::new(
            "extract_failed",
            &format!("抽取器 panic，已兜住并跳过：{what}"),
            Some("该文件本轮不索引；这类文件可换工具另存一份再登记"),
        )),
    }
}
```

把 `index_job.rs:785-806` 那整条测试（`/// panic 边界的直测（终审 I1）` 注释 + `#[test] fn panic_to_err_turns_a_panic_into_an_extract_failure`）**逐字搬**进 `extract.rs` 的 `mod tests`（`:283` 之后），注释里那句「那是 `开发进度.md` 的缺口 1」保留，另外补一行归属说明：

```rust
    /// M4 起这条直测跟着 `panic_to_err` 住在抽取层：被守护的表达式在哪，测试就在哪。
```

- [ ] **Step 2: `index_job.rs` 改引用、删私有实现**

- 删掉 `index_job.rs:138-158`（那段 `/// 把「可能 panic 的调用」…` 注释 + `fn panic_to_err`）。
- `:18` 的 `use crate::extract::extract_text;` 改为 `use crate::extract::{extract_text, panic_to_err};`。
- `:224-227` 的调用点注释里两处 `panic_to_err` 说法保留，只把「抽取层里只有 `pdf_text` 自己包了 `catch_unwind`」那句后面补成：`…（边界助手本身自 M4 起住在 extract.rs，索引与预览共用）`。调用表达式 `panic_to_err(|| extract_text(...), &file.path).and_then(|r| r)` **一个字符都不改**。
- 删掉搬走的测试（`:785-806`）。

Run: `cd src-tauri && cargo test --lib; echo exit=$?`
Expected: `106 passed; 0 failed`（总数不变：`extract` 模块 15 条、`index_job` 模块 12 条）。

Run: `cd src-tauri && cargo test --lib extract; echo exit=$?`
Expected: `15 passed; 0 failed`。

- [ ] **Step 3: 注释指针改名（这两处是文档级引用，指向不存在的东西比没注释更坏）**

`extract.rs:244` 附近 `pdf_text` 的 doc 注释里：把「外层 `index_job::panic_to_err` 还兜着 docx/pptx/xlsx/txt 那三条本文件没包的路径」改成「外层 `panic_to_err`（本文件，M4 从 `index_job` 提上来）还兜着 docx/pptx/xlsx/txt 那三条本文件没包的路径」，并把「也不许改成去调 `index_job` 的私有助手」这句删掉（它已经不存在了），换成「也不许因为『反正外面有兜底』把这两道叠出的边界折成一道：`extract_text` 还有不走 `panic_to_err` 的调用方」。

`extract.rs:580` 附近那条 `assert_ne!` 的 message 里 `index_job::panic_to_err` 改为 `crate::panic_to_err`。

Run: `cd src-tauri && grep -rn "index_job::panic_to_err" src/; echo exit=$?`
Expected: `exit=1`（0 命中）。

- [ ] **Step 4: 变异取证**

把 `extract.rs` 里 `panic_to_err` 的 `catch_unwind` 换成直接 `Ok(f())`：`cargo test --lib extract` 必须让 `panic_to_err_turns_a_panic_into_an_extract_failure` 红（进程会 abort，测试以「测试线程 panic」形式报，也算红，但要在报告里写明是哪种）。改回来。

- [ ] **Step 5: clippy 两道 + 提交**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings; echo exit=$?
cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
git add src-tauri/src/extract.rs src-tauri/src/index_job.rs
git commit -m "refactor: M4 panic 边界从作业层提到抽取层，索引与预览共用一个实现"
```
Expected: 两条门 `exit=0`；提交只含这两个文件。

---

### Task 3: `clean_snippet` 的 `A B AB` 三连折叠

`index_text` 用 `cut_for_search`，复合词会连同子词一起写进那一列，实测 `cut_for_search("里面只有付款条件与验收流程")` = `里面 只有 付款 条件 付款条件 与 验收 流程`，`snippet()` 取的就是这一列，于是摘要读成「里面只有付款条件**付款条件**与验收流程」。本任务在展示清洗层折掉它，**不碰索引**（spec D5：加 UNINDEXED 正文列要付全量重扫的代价）。

**Files:**
- Modify: `src-tauri/src/tokenize.rs:76-93`（`clean_snippet` 重构 + 折叠助手 + 2 条新测试；另有 Step 5 的三处注释保鲜，落点在 `MAX_QUERY_CHARS` 头注释、`query_terms` 头注释与结构守卫的注释）
- Modify: `src-tauri/src/index_store.rs:209-213`（`DocHit.snippet` 的契约注释：重复词已被折叠）与 `:516-520` 附近那条测试的探针注释 + 断言升级为逐字相等

**Interfaces:**
- Consumes: `is_cjk`（`:66`，不变）
- Produces: `pub fn clean_snippet(raw: &str) -> String`（签名不变，`index_store.rs:241` 的调用点不动）

- [ ] **Step 1: 写两条红测试**

追加到 `tokenize.rs` 的 `mod tests`：

```rust
    /// 折叠规则：只有「前面若干个词恰好铺满后面那个复合词」才折，铺不满就不折。
    /// 正例两枚都来自实测的 `cut_for_search` 输出：`付款 条件 付款条件` 是**无重叠铺满**，
    /// `报价 价单 报价单` 是**有重叠铺满**（报价=0..2、价单=1..3），后者证明规则不要求子词互不相交。
    #[test]
    fn clean_snippet_folds_the_indexed_compound_back_into_one_word() {
        assert_eq!(
            clean_snippet("里面 只有 付款 条件 付款条件 与 验收 流程"),
            "里面只有付款条件与验收流程"
        );
        assert_eq!(clean_snippet("报价 价单 报价单 一份"), "报价单一份", "重叠子词同样被铺满判据收下");
        // 用户真写了两遍的情形：两个词等长，谁都不是对方的真子串，一个字都不许动。
        assert_eq!(clean_snippet("数据 数据 库"), "数据数据库", "相邻同词是原文真重复，折了就是篡改正文");
        assert_eq!(clean_snippet("甲方 甲方要求"), "甲方甲方要求", "单枚前缀铺不满复合词，不许折");
    }

    /// `[` / `]` 是 `技术方案.md` 6.1 要求暴露给前端的高亮标记，折叠不许把它吃掉；
    /// 摘要里那枚省略号 `⋯` 会粘在首个词上（`⋯付款` 不是 `付款条件` 的子串），
    /// 于是被截断的窗口首格不折叠——这是**已知取舍**，宁可不折也不折错。
    #[test]
    fn clean_snippet_keeps_the_highlight_markers_and_leaves_truncated_runs_alone() {
        assert_eq!(clean_snippet("[付款] 条件 付款条件"), "[付款]条件付款条件", "带标记就不折，标记是界面契约");
        assert_eq!(clean_snippet("付款 [条件] 付款条件"), "付款[条件]付款条件");
        assert_eq!(
            clean_snippet("⋯付款 条件 付款条件 与 验收"),
            "⋯付款条件付款条件与验收",
            "省略号粘住首词时放弃折叠（观感问题不该吃掉了召回证据）"
        );
    }
```

Run: `cd src-tauri && cargo test --lib clean_snippet; echo exit=$?`
Expected: FAIL —— `clean_snippet_folds_the_indexed_compound_back_into_one_word` 断言不符（现返回 `里面只有付款条件付款条件与验收流程`）。**若「新正例反而通过、旧测试红」说明实现把 `数据 数据` 也折了，那是篡改正文，不许留下。**

- [ ] **Step 2: 实现折叠**

把 `tokenize.rs:76-93` 的 `clean_snippet` 整段替换为：

```rust
/// `snippet()` 回的是我们插入空格连接起来的词条串，直接展示会读成「甲 方 要 求」。
/// 两步：① 折掉 `cut_for_search` 造成的复合词重复；② 空格相邻任一侧是 CJK 就删掉。
/// 第二步的规则一字不变（Latin 之间的空格保留、数字与中文之间也删、`⋯` 两侧不带空格故不受影响）。
pub fn clean_snippet(raw: &str) -> String {
    // 按单个空格切再拼回，是无损往返（`split(' ')` 保留空段），所以下面第二步看到的串与入参同形。
    let folded = fold_duplicated_compound(&raw.split(' ').collect::<Vec<_>>());
    let joined = folded.join(" ");
    let chars: Vec<char> = joined.chars().collect();
    let mut out = String::with_capacity(joined.len());
    for (i, c) in chars.iter().enumerate() {
        if *c == ' ' {
            let near_cjk = (i > 0 && is_cjk(chars[i - 1])) || chars.get(i + 1).is_some_and(|n| is_cjk(*n));
            if near_cjk {
                continue;
            }
        }
        out.push(*c);
    }
    out
}

/// 折叠对象只有一种：紧跟在复合词 W 左侧的那一串词，每一个都是 W 的**真子串**，
/// 且它们在 W 里首次出现的区间并起来**恰好铺满 W**。
/// 「铺满」这一步就是它不篡改正文的原因：单枚真子串必然比 W 短、铺不满，所以
/// 「用户真的把某个词写了两遍」（`数据 数据`、`甲方 甲方要求`）永远落在判据之外；
/// 而 `cut_for_search` 吐出的子词组（实测 `付款 条件 付款条件`、`报价 价单 报价单`）恰好铺满。
/// 带 `[` / `]` 高亮标记的词一律不参与：标记是 `技术方案.md` 6.1 要求暴露给前端的信息，
/// 为了观感吃掉它，代价比重复词大。
/// `[` `]` `⋯` 这三个字面量来自 `index_store::SELECT_SQL` 里 `snippet()` 的实参，改那边要同步改这里。
/// 签名里只给内层 `&str` 命名生命周期：返回值只从词里抄 `&str`，不带外层切片的借用。
fn fold_duplicated_compound<'a>(tokens: &[&'a str]) -> Vec<&'a str> {
    let mut drop = vec![false; tokens.len()];
    for idx in 0..tokens.len() {
        let run = foldable_run(tokens, idx);
        for k in 1..=run {
            drop[idx - k] = true;
        }
    }
    tokens
        .iter()
        .enumerate()
        .filter(|(i, _)| !drop[*i])
        .map(|(_, t)| *t)
        .collect()
}

/// 返回 `tokens[w_idx]` 左侧应丢掉的词数（0 = 不折）。左侧任何一处不满足条件就整组放弃。
fn foldable_run(tokens: &[&str], w_idx: usize) -> usize {
    let whole: Vec<char> = tokens[w_idx].chars().collect();
    if whole.len() < 2 || tokens[w_idx].contains('[') || tokens[w_idx].contains(']') {
        return 0;
    }
    let mut covered = vec![false; whole.len()];
    let mut run = 0usize;
    let mut i = w_idx;
    while i > 0 {
        i -= 1;
        let part = tokens[i];
        let part_chars: Vec<char> = part.chars().collect();
        // 空段（连续空格）算「不是真子串」→ 整组放弃，保守。
        if part_chars.is_empty()
            || part_chars.len() >= whole.len()
            || part.contains('[')
            || part.contains(']')
        {
            return 0;
        }
        // 子串判据就落在这里：`find` 回 None 就是「不是 W 的子串」→ 整组放弃。
        // 用首次出现的区间：稳定，且不会为凑铺满去找最宽松的落点。
        // 写计划时这里是「先 `contains` 再 `find(...).unwrap_or(0)`」两处并写，评审后收成一条：
        // `str::contains` 按定义就是 `find(..).is_some()`，两条并写会让 `unwrap_or(0)` 那个分支
        // 永不可达（于是它既不能被测试证伪，又把「守卫被删」的回归吞成静默的错区间覆盖），
        // 而下面那条 `⋯付款` 用例正是靠这一格返回 None 才不折的。
        let Some(start_byte) = tokens[w_idx].find(part) else {
            return 0;
        };
        let start_chars = tokens[w_idx][..start_byte].chars().count();
        for slot in &mut covered[start_chars..start_chars + part_chars.len()] {
            *slot = true;
        }
        run += 1;
        if covered.iter().all(|c| *c) {
            return run;
        }
    }
    0
}
```

关于签名里的 `<'a>`（写计划时漏了，实现者按编译器要求补上，评审已独立复现）：`fn f(tokens: &[&str]) -> Vec<&str>` 会 `error[E0106] missing lifetime specifier`——`&[&str]` 有两个输入生命周期位置且无 `self`，输出侧无法省略；而 rustc 提示的 `&'a [&'a str]` 在本函数唯一调用点（`&raw.split(' ').collect::<Vec<_>>()`，一个临时 `Vec`）会 `error[E0716] temporary value dropped while borrowed`。**只命名内层**是唯一能配上该调用点的最小形式。

关于 `foldable_run` 开头那三处「蕴含关系上冗余」的守卫（`whole.len() < 2`、W 侧两枚括号 `contains`）：**按本计划有意保留**，它们与 part 侧括号守卫同属「把 D5 界面契约写成代码」的显式声明，行为由子串判据 + 长度判据 + 铺满判据承载。评审若再点名「不可证伪」，答案是既定的（见 Step 4 第 2 条），不必为此改代码。

Run: `cd src-tauri && cargo test --lib tokenize; echo exit=$?`
Expected: `9 passed; 0 failed`（原 5 + Task 1 的 2 + 本任务 2）。旧测试 `clean_snippet_only_collapses_cjk_boundaries` 必须**仍然绿**——它的三组输入按上面的判据都落不进折叠，若它红了说明折叠规则越界改了不该改的东西。

- [ ] **Step 3: 集成面收口（`index_store.rs`）**

`index_store.rs:505-531` 那条 `chinese_word_hits_with_a_readable_body_snippet` 里，把探针注释与断言升级为逐字相等（原注释写着实测值 `"里面只有付款条件付款条件与验收流程"`）：

```rust
        // 摘要只取自正文列（snippet 的第 2 个实参固定为 1）：只靠文件名命中的结果，摘要不带 [ ]。
        // 这条是 M4 的界面契约。正文入库串是 `里面 只有 付款 条件 付款条件 与 验收 流程`
        // （cut_for_search 把复合词连同子词一起写进索引列），M4 的三连折叠把它折回一个词，
        // 所以下面期望的是**逐字相等**而不是「不含空格」这种弱断言。
        let only_name = hits(&c, "报价单");
        assert_eq!(only_name.len(), 1);
        assert_eq!(only_name[0].snippet, "里面只有付款条件与验收流程");
```

写计划时这个代码块里还有一条 `assert!(!only_name[0].snippet.contains('['), "正文没这个词、只靠文件名命中时摘要不带标记");`，Task 3 评审判定它是**恒绿断言**：紧邻的逐字相等断言的那个串本身不含 `[`，前一条通过时它在逻辑上不可能红；且按计划文本去掉 `{}` 实参后失败也打不出任何诊断。故删。
「摘要要标出命中词」的正向契约由同一测试里 `hit.snippet.contains('[') && hit.snippet.contains(']')` 那条守着（可被「把 SELECT_SQL 的标记换成别的」打红），「只靠文件名命中不带标记」由上面那句逐字相等守着。

同时把 `DocHit.snippet` 的契约注释（`index_store.rs:209-213`）里「`cut_for_search` 的复合词会连着出现两次（实测「里面只有付款条件付款条件与验收流程」）」这句改成「`cut_for_search` 的复合词原本会连着出现两次，M4 起由 `clean_snippet` 的三连折叠收口（见 `tokenize.rs:fold_duplicated_compound`）；两类残留不折、重复词仍连着出现两次：窗口首格被 `⋯` 粘住，以及命中词自己带着 `[ ]` 标记。折叠只作用在这条展示串上，不影响召回（原注释末尾那句「不影响召回，只影响观感」要保留下来）」。

Run: `cd src-tauri && cargo test --lib index_store; echo exit=$?`
Expected: `14 passed; 0 failed`（条数不变，本任务在 index_store 只改注释与加强一条既有断言）。

Run: `cd src-tauri && cargo test --lib; echo exit=$?`
Expected: `108 passed; 0 failed`（106 + 2）。

- [ ] **Step 4: 变异取证（本任务最重要的一条，因为它守护的是「不篡改用户正文」）**

四条变异都要实跑并把**真实红的那条测试名**贴进报告，不许凭推断写期望。「改回来」之后一律 `diff -q <备份> <工作文件>` 验字节一致（本仓 `core.autocrlf=true`、工作区 CRLF，`git show` 吐的是 LF 裸 blob，**不要用 `git show > 文件` 还原**）。

1. 把 `foldable_run` 的判据从「铺满」放宽成「只要是真子串就折」（删掉 `if covered.iter().all(...)` 的 return，走到头返回 run）。
   实测红在 `clean_snippet_folds_the_indexed_compound_back_into_one_word`（写计划时这条误写成 `clean_snippet_keeps_the_highlight_markers_...`：「甲方 甲方要求」那条断言其实在 **folds** 那条测试里、排第 4，而第 1 条 `assert_eq!` 先 panic，所以看不到它）。要拿到「篡改正文」的直接证据，就临时加一枚只跑一次的探针测试（只断言 `clean_snippet("甲方 甲方要求")`），跑完删除。
2. 删掉 `part.contains('[')` / `part.contains(']')` 两处守卫：**本条变异预期不红**（Task 3 评审独立证明，实现者也用定向探针复现：删守卫后全量 108 绿、三组带标记输入仍不折）。原因是「带标记就不折」已被别的判据蕴含——W 侧 `:140` 先拒绝带标记的 W；带标记的 part 要么不是无标记 W 的子串（`find` 回 None），要么 `[付款]`/`[条件]` 这种 4 char 撞上 `part_chars.len() >= whole.len()`；`[` `]` 这两格本身永远铺不满。
   处置：**四处括号守卫全部保留**（它们是把 D5 界面契约写在代码里的显式声明，成本为零、语义自解释），**不许为了凑红去删守卫或改夹具**。这条「括号守卫当前没有独立守护者」作为有名缺口登记进账本，交 M4 终审统一裁定。
3. 把 `fold_duplicated_compound` 整个函数换成 `tokens.to_vec()`（不折）：新正例与 `index_store` 那条逐字相等都必须红。改回来。（附带确认：`clean_snippet_keeps_the_highlight_markers_...` 在这条下保持绿是**正确**的——它守的是「不许多折」。）
4. 把子串判据那格的 `else { return 0; }` 改成 `else { 0 }` 等价的 `unwrap_or(0)` 形态（即 `find` 失败时落回区间 0）：`clean_snippet_keeps_the_highlight_markers_and_leaves_truncated_runs_alone` 里 `⋯付款 条件 付款条件 与 验收` 那条**必须红**（`⋯付款` 3 char、`付款条件` 4 char，长度守卫挡不住它，只有 `find` 回 None 才不折；落回 0 会把它当成 `付款` 铺满 → 折成 `付款条件与验收`，吃掉省略号那格）。这条是子串判据的独立守护者，也是本任务把 `contains` + `unwrap_or(0)` 收成一条 `find` 之后新增的变异。

- [ ] **Step 5: 三处注释保鲜（Task 1 评审登记的 Minor，随本任务一起做）**

本任务已经在改 `tokenize.rs`，这三处是注释级、零行为回归的修正，**不单独开派发轮**；除下面三处外不许动同文件里的任何代码。

1. `MAX_QUERY_CHARS` 的头注释（`:12-13`）里那句「之前 `search.rs:87` 与 `index_store.rs:262` 各写了一遍字面量 128」改用**函数名**定位，因为行号每改一次就漂：
   `/// 之前 \`search::field_hits\` 与 \`index_store::doc_hits\` 的守卫各写了一遍字面量 128，改一处就会让两边口径分家。`
2. `query_terms` 的头注释里「`doc_preview` 拿它决定原文里高亮哪些词」指向的是**本里程碑后面任务**才存在的模块，加一个去向标记，别让它读起来像已经存在：
   `/// 两个消费方：\`query_expression\` 拼 FTS5 语法，\`doc_preview\`（M4 Task 5 落地）拿它决定原文里高亮哪些词。`
3. 结构守卫 `query_side_has_exactly_one_cut` 的注释末尾补一条**给下一个人的规矩**（它数的是整份源文本，注释和测试里的同款字面量一样会命中）：
   `/// 因此本文件里不许出现第二处带点的 \`cut\` 字面量——注释与测试里也算。要在文档里提这个调用，写成 \`cut(...)\` 或用反引号把点断开，否则这条守卫会在没有任何代码回归的情况下红。`

Run: `cd src-tauri && cargo test --lib tokenize; echo exit=$?`
Expected: `9 passed; 0 failed`（条数不变；第 3 处补的注释里**不能**出现带点的 cut 字面量，否则这条守卫自己会红）。

- [ ] **Step 6: clippy 两道 + 提交**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings; echo exit=$?
cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
git add src-tauri/src/tokenize.rs src-tauri/src/index_store.rs
git commit -m "fix: M4 摘要里的复合词重复：clean_snippet 按「恰好铺满」折叠，带高亮标记不折"
```

---

### Task 4: `search::unified_bundle` —— 三段聚合 + 段内按项目聚簇 + 三个截断计数

**Files:**
- Modify: `src-tauri/src/search.rs`（顶部常量区 `:15` 之后加四个常量；`:29` 之后加三个 DTO；`field_hits` 之后加 `cluster_by_project` 与 `unified_bundle`；tests 模块加 11 条）
- Modify: `src-tauri/src/index_store.rs`（`status_counts`（`:149`）之后加 `indexed_project_count`；tests 模块加 1 条；顺带更正 `doc_hits` 头注释里那条过期指针，见 Step 3 末尾）

**Interfaces:**
- Consumes：`search::field_hits`（既有）、`index_store::doc_hits`（既有，签名 `pub fn doc_hits(conn: &Connection, query: &str, limit: i64) -> AppResult<Vec<DocHit>>`）、Task 1 的 `MAX_QUERY_CHARS`（本任务不直接引，两处守卫仍在原处）。
- Produces：
  - `pub fn unified_bundle(conn: &Connection, query: &str) -> AppResult<SearchBundle>` —— Task 6 的 `search_all` 命令体只做「取锁 + 调这个函数」。
  - `pub fn indexed_project_count(conn: &Connection) -> AppResult<usize>`（`index_store`）。
  - `pub struct SearchBundle { query, projects, projects_hidden, ledger, docs, relaxed, indexed_projects }` / `pub struct ClusterSection<C> { clusters, hidden_clusters }` / `pub struct Cluster<T> { project_id, project_name, items, hidden }`，全部 `#[derive(Debug, Serialize)] #[serde(rename_all = "camelCase")]` —— 前端 `src/types/search.ts`（Task 7）逐字段对应，线格式为 `projectId` / `projectName` / `hiddenClusters` / `projectsHidden` / `indexedProjects`。

**为什么这些形状是定死的**（spec §三、§四、D3/D6/D7）：簇间序只取「命中条数降序 → 项目名升序 → 项目 id 升序」，**不跨段合并、不把 bm25 与 LIKE 归一化**（§3.4 禁假归一化）；三个截断各自有承载（簇内 → `Cluster.hidden`，整簇 → `ClusterSection.hidden_clusters`，平铺 → `projects_hidden`），只显示不展开；`relaxed` 在**截断前**的 docs 上算，否则放宽命中全被截掉时段级说明会凭空消失。

- [ ] **Step 1: 先写 11 条红测试（`search.rs` 的 `mod tests`）**

测试模块的 `use` 行改为（`DocHit` 是 DTO 字段类型，必须在**文件顶部**引，见 Step 3；这里只引测试要直接点名的东西）：

```rust
    use super::*;
    use crate::{db, index_scan::ScannedFile, index_store, ledger, project, vault};
    use rusqlite::Connection;
```

新增两个夹具（放在既有 `seed_ledger` 之后）。正文行不像台账那样有 builder，直接走写入侧唯一入口；`write_doc` 是 `pub`、`ScannedFile` 五个字段全 `pub`（`index_scan.rs:47-53`），所以不需要把 `index_store` 的测试夹具提出来共享：

```rust
    /// 一条可搜的正文行。`path` 在同一项目内唯一（`db.rs:297` `index_docs` 的 `UNIQUE (project_id, path)`）。
    fn seed_doc(conn: &Connection, pid: &str, path: &str, body: &str) {
        index_store::write_doc(
            conn,
            pid,
            &ScannedFile {
                path: path.to_owned(),
                file_name: path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned(),
                ext: "docx".to_owned(),
                size: 1024,
                mtime: 1_700_000_000,
            },
            index_store::DocOutcome::Ok(body.to_owned()),
        )
        .unwrap();
    }

    fn cluster_names(section: &ClusterSection<Cluster<DocHit>>) -> Vec<String> {
        section.clusters.iter().map(|c| c.project_name.clone()).collect()
    }
```

十一条测试（向量全部沿用 `index_store.rs` 已实测的事实 5/6：正文含「验收」的行查「验收」是 `exact`；正文「维保期为十二个月」+ 文件名「维保期说明.docx」查「维保」是 `prefix`。**不要**换新词去赌 jieba 词典）：

```rust
    #[test]
    fn bundle_splits_three_sections_without_letting_one_borrow_another() {
        let (c, pid, mk) = fixture();
        seed_ledger(&c, &pid, &mk); // 五类台账各一条，逐条读过：没有一条含「验收」
        seed_project(&c, "验收流程梳理"); // 「项目」段的命中只可能来自这里
        seed_doc(&c, &pid, "C:/x/合同验收.docx", "甲方要求验收指标见合同附件");
        let b = unified_bundle(&c, "验收").unwrap();
        assert_eq!(b.query, "验收");
        assert_eq!(b.projects.len(), 1, "项目名命中只进「项目」段");
        assert_eq!(b.projects[0].title, "验收流程梳理");
        assert_eq!(b.projects[0].source, "project");
        assert!(b.ledger.clusters.is_empty(), "台账没有「验收」，正文命中不该被塞进台账段");
        assert_eq!(b.docs.clusters.len(), 1);
        assert_eq!(b.docs.clusters[0].items.len(), 1);
        assert_eq!(b.docs.clusters[0].project_name, "政务云迁移");
        assert_eq!(b.indexed_projects, 1, "只有一个项目有 ok 正文行");
    }

    /// 台账段必须有**非空**证据。上一条只断过 `b.ledger.clusters.is_empty()`，
    /// 于是 `partition` 的谓词怎么写都得空簇 —— Task 4 首轮评审据此判定「三段的三分之一零守护」，
    /// 且简报原来的变异 6 是等价变异（实测全量 119 条都绿）。这条补上台账段的聚簇与两段不互相漏。
    #[test]
    fn ledger_section_clusters_its_own_hits_instead_of_leaking_into_projects() {
        let (c, pid, mk) = fixture();
        seed_ledger(&c, &pid, &mk); // 「生产门户」只在 env 表里，逐条读过
        seed_doc(&c, &pid, "C:/x/合同验收.docx", "甲方要求验收指标见合同附件");
        let b = unified_bundle(&c, "生产门户").unwrap();
        assert!(b.projects.is_empty(), "「生产门户」不是项目名，平铺段必须空");
        assert_eq!(b.ledger.clusters.len(), 1, "五条台账只有 env 命中，应聚成一簇");
        assert_eq!(b.ledger.hidden_clusters, 0);
        let cluster = &b.ledger.clusters[0];
        assert_eq!(cluster.project_id, pid);
        assert_eq!(cluster.project_name, "政务云迁移");
        assert_eq!(sources(&cluster.items), ["env"]);
        assert!(b.docs.clusters.is_empty(), "正文里没有「生产门户」，不该凭空多出正文簇");
    }

    #[test]
    fn doc_clusters_sort_by_hit_count_then_project_name() {
        let (c, _, _) = fixture();
        // 先建 Beta（2 条）再建 Alpha（2 条）：并列时若按插入序排会得 Beta,Alpha，
        // 这条阳性对照保证守的是「项目名升序」。Gamma 5 条，条数降序排第一。
        let beta = seed_project(&c, "Beta 项目");
        let alpha = seed_project(&c, "Alpha 项目");
        let gamma = seed_project(&c, "Gamma 项目");
        for i in 0..2 {
            seed_doc(&c, &beta, &format!("C:/b/{i}.docx"), "甲方要求验收指标");
            seed_doc(&c, &alpha, &format!("C:/a/{i}.docx"), "甲方要求验收指标");
        }
        for i in 0..5 {
            seed_doc(&c, &gamma, &format!("C:/g/{i}.docx"), "甲方要求验收指标");
        }
        let b = unified_bundle(&c, "验收").unwrap();
        assert_eq!(cluster_names(&b.docs), ["Gamma 项目", "Alpha 项目", "Beta 项目"]);
    }

    #[test]
    fn items_keep_the_bm25_order_that_doc_hits_returns() {
        let (c, pid, _) = fixture();
        for name in ["C:/x/甲.docx", "C:/x/乙.docx", "C:/x/丙.docx"] {
            seed_doc(&c, &pid, name, "甲方要求验收指标见合同附件");
        }
        let b = unified_bundle(&c, "验收").unwrap();
        let cluster = &b.docs.clusters[0];
        assert_eq!(cluster.items.len(), 3, "对照：三条命中都在同一簇");
        let in_bundle: Vec<String> = cluster.items.iter().map(|h| h.doc_id.clone()).collect();
        // 这里**刻意引常量而不是写字面量 200**：一是 Step 5 的 grep 门禁要过，二是把
        // 「上限改成 1」这种变异交给上一条 `items.len() == 3` 去打红，而不是靠两条硬编码数字互证。
        let from_store: Vec<String> = index_store::doc_hits(&c, "验收", DOCS_FETCH_LIMIT)
            .unwrap()
            .into_iter()
            .map(|h| h.doc_id)
            .collect();
        assert!(from_store.len() == 3 && from_store.windows(2).all(|w| w[0] != w[1]));
        assert_eq!(in_bundle, from_store, "M4 不许重排 bm25 序（§3.4：只做聚簇，不做再排序）");
    }

    #[test]
    fn section_drops_the_21st_cluster_and_reports_it() {
        let (c, _, _) = fixture();
        for i in 0..21 {
            let pid = seed_project(&c, &format!("p{i:02}"));
            seed_doc(&c, &pid, "C:/x/合同.docx", "甲方要求验收指标");
        }
        let b = unified_bundle(&c, "验收").unwrap();
        assert_eq!(b.docs.clusters.len(), 20);
        assert_eq!(b.docs.hidden_clusters, 1, "整簇被扔掉的簇数要有承载，否则等于静默丢弃");
        assert!(
            !cluster_names(&b.docs).iter().any(|n| n == "p20"),
            "名序最大者被截：{:?}",
            cluster_names(&b.docs)
        );
    }

    #[test]
    fn cluster_keeps_10_items_and_reports_the_other_2() {
        let (c, pid, _) = fixture();
        for i in 0..12 {
            seed_doc(&c, &pid, &format!("C:/x/{i:02}.docx"), "甲方要求验收指标");
        }
        let b = unified_bundle(&c, "验收").unwrap();
        assert_eq!(b.docs.clusters.len(), 1);
        assert_eq!(b.docs.clusters[0].items.len(), 10);
        assert_eq!(b.docs.clusters[0].hidden, 2);
        assert_eq!(b.docs.hidden_clusters, 0);
    }

    #[test]
    fn flat_project_section_truncates_into_projects_hidden() {
        let (c, _, _) = fixture();
        for i in 0..15 {
            seed_project(&c, &format!("验收{i:02}"));
        }
        let b = unified_bundle(&c, "验收").unwrap();
        assert_eq!(b.projects.len(), 10);
        assert_eq!(b.projects_hidden, 5, "平铺段没有簇可挂计数，只能单独给一个数");
        assert!(b.docs.clusters.is_empty());
        assert_eq!(b.indexed_projects, 0, "有命中 ≠ 有正文索引，两个数不许互相推出来");
    }

    #[test]
    fn empty_query_yields_an_empty_bundle_instead_of_an_error() {
        let (c, pid, _) = fixture();
        seed_doc(&c, &pid, "C:/x/合同.docx", "甲方要求验收指标");
        let b = unified_bundle(&c, "   ").unwrap();
        assert!(b.projects.is_empty() && b.ledger.clusters.is_empty() && b.docs.clusters.is_empty());
        assert!(!b.relaxed);
        // 空查询不许留下幽灵计数：把三处 `saturating_sub` 写死成任何非零数都会在这里红。
        assert_eq!((b.projects_hidden, b.ledger.hidden_clusters, b.docs.hidden_clusters), (0, 0, 0));
    }

    #[test]
    fn over_long_query_is_rejected_by_the_existing_guard() {
        let (c, _, _) = fixture();
        let err = unified_bundle(&c, &"验".repeat(129)).unwrap_err();
        assert_eq!(err.code, "invalid_input", "聚合层不许再加第三道长度校验");
    }

    #[test]
    fn relaxed_is_the_prefix_stage_and_nothing_else() {
        let (c, pid, _) = fixture();
        seed_doc(&c, &pid, "C:/x/维保期说明.docx", "维保期为十二个月");
        seed_doc(&c, &pid, "C:/x/合同验收.docx", "甲方要求验收指标见合同附件");
        let loose = unified_bundle(&c, "维保").unwrap();
        assert!(loose.relaxed, "库里只有「维保期」，落到前缀段要说得出口");
        assert_eq!(loose.docs.clusters[0].items[0].matched_by, "prefix");
        let tight = unified_bundle(&c, "验收").unwrap();
        assert!(!tight.relaxed, "精确段命中了就不该出段级说明");
    }

    #[test]
    fn bundle_wire_format_is_camel_case() {
        let (c, pid, _) = fixture();
        seed_doc(&c, &pid, "C:/x/合同.docx", "甲方要求验收指标");
        let v = serde_json::to_value(unified_bundle(&c, "验收").unwrap()).unwrap();
        let obj = v.as_object().unwrap();
        for key in ["query", "projects", "projectsHidden", "ledger", "docs", "relaxed", "indexedProjects"] {
            assert!(obj.contains_key(key), "缺 {key}：{:?}", obj.keys().collect::<Vec<_>>());
        }
        assert!(!obj.contains_key("projects_hidden"));
        let cluster = &v["docs"]["clusters"][0];
        for key in ["projectId", "projectName", "items", "hidden"] {
            assert!(cluster.as_object().unwrap().contains_key(key), "簇缺 {key}");
        }
        assert!(v["ledger"].as_object().unwrap().contains_key("hiddenClusters"));
    }
```

Run: `cd src-tauri && cargo test --lib search; echo exit=$?`
Expected: 编译失败 —— `cannot find function unified_bundle in this scope` / `cannot find type SearchBundle`（未定义，不是断言失败）。

- [ ] **Step 2: 加 `indexed_project_count`（`index_store.rs`）——先只写测试跑红，再落实现跑绿**

放在 `status_counts`（`:149`）之后：

```rust
/// 有几个项目已经有可搜的正文了。首屏空态要在「正文还没建索引」和「建了但没命中」之间分开
/// （M3 的 m8：文案对「根不存在」和「0 个可索引文件」说了同一句话），所以这里的谓词
/// **必须与 `SELECT_SQL` 同口径**：少半个谓词就会出现「文案说正文有索引、正文段却是空的」。
/// 取数走原生 `i64` 再转 `usize`：rusqlite 0.40.2 的 `from_sql_integral!(usize)`（`src/types/from_sql.rs:146`）
/// 挂在 `#[cfg(feature = "fallible_uint")]`（`:145`）后面，而本项目 `Cargo.toml:25` 只开了 `bundled`，
/// 所以 `r.get::<_, usize>(0)` 编译不过（Task 4 实测 `E0277`：`usize: FromSql<'_>` 不满足）。
/// 控制方写计划时只查到「有 `from_sql_integral!(usize)`」没查它上面那行 `cfg`，这是那条论断的更正。
/// `count(DISTINCT …)` 恒非负，转换不丢信息。
pub fn indexed_project_count(conn: &Connection) -> AppResult<usize> {
    let n = conn.query_row(
        "SELECT count(DISTINCT d.project_id)
           FROM index_docs d
           JOIN projects p ON p.id = d.project_id
          WHERE d.index_status = 'ok' AND p.deleted_at IS NULL",
        [],
        |r| r.get::<_, i64>(0),
    )?;
    Ok(n as usize)
}
```

它的测试放在 `index_store.rs` 的 `mod tests`（三个排除理由各占一个项目，这样缺哪个谓词都会红）：

```rust
    /// 与 `SELECT_SQL` 同口径是这条的唯一价值：`ok` 之外不算、项目软删了不算。
    #[test]
    fn indexed_project_count_matches_the_search_predicate() {
        let c = conn();
        seed_project(&c, "p_ok");
        write_doc(&c, "p_ok", &file("C:/x/合同.docx"), DocOutcome::Ok("甲方要求验收指标".into())).unwrap();
        seed_project(&c, "p_skipped");
        write_doc(&c, "p_skipped", &file("C:/x/大图.png"), DocOutcome::Skipped("超出大小上限")).unwrap();
        seed_project(&c, "p_deleted");
        write_doc(&c, "p_deleted", &file("C:/x/验收.docx"), DocOutcome::Ok("验收指标".into())).unwrap();
        c.execute("UPDATE projects SET deleted_at = datetime('now') WHERE id = 'p_deleted'", []).unwrap();
        assert_eq!(indexed_project_count(&c).unwrap(), 1, "只有 p_ok 既在库里又没被软删");
    }
```

顺序是「测试先落地、实现后落地」：先只贴下面那段测试跑一次 `cargo test --lib index_store`，红在 `error[E0425]: cannot find function indexed_project_count in this scope`（测试引用了还不存在的函数，编译不过就是本任务的 RED，Task 4 实测如此）；再落上面那段实现。

Run: `cd src-tauri && cargo test --lib index_store; echo exit=$?`
Expected: `15 passed; 0 failed`（14 + 1）。

- [ ] **Step 3: 实现常量、三个 DTO、`cluster_by_project`、`unified_bundle`（`search.rs`）**

文件顶部 `use` 增加（`DocHit` 是 DTO 的字段类型，必须在模块作用域引）：

```rust
use crate::index_store::{self, DocHit};
```

`PER_GROUP_LIMIT`（`:15`）之后追加四个常量。注释里**不许出现** `unwrap_or(50)` 或 `, 200)` 这类字面量文本——Step 5 那条 grep 门禁数的是整份源文件，注释与测试也算（计划原文在这里写的是 `limit.unwrap_or(50).clamp(1, 200)`，会把自家的门直接弄红；同 Task 1 那条「结构门禁用 `include_str!` 数文本时，注释里的同款字面量会被无回归地数进去」的坑）。指针只写函数名加行号：

```rust
/// 正文段一次向库取多少条。这是**取数**上限，不是展示上限；展示截断走下面三个数。
/// 顶对齐 IPC 侧 `search_docs` 的 limit clamp（`lib.rs:542`，那里是边界唯一一次校验），
/// 让首屏与 `/index` 的「试搜正文」试验台在同一条 SQL 上取数。
///
/// 下面三个展示计数只**相对本次取到的样本**：库里命中超过这条取数上限时它们会低报
/// （排在第 201 位的正文根本没进内存，不计进任何 `hidden`）。M4 的线格式里没有「结果被截过」的位
/// （spec §三 定死那七个字段，M4 不擅自加），所以诚实性落在两处：本注释 + Task 8 的文案措辞 ——
/// 要说「本次结果里另有 N 条未展开」，不能说「库里还有 N 条」。
const DOCS_FETCH_LIMIT: i64 = 200;
/// 段级：一个段最多展示多少个簇。整簇被扔掉的簇数进 `ClusterSection::hidden_clusters`。
const MAX_CLUSTERS_PER_SECTION: usize = 20;
/// 簇级：一个簇最多展示多少条，被截掉的条数进该簇 `hidden`。
const MAX_ITEMS_PER_CLUSTER: usize = 10;
/// 「项目」段是平铺列表，没有簇可挂计数，截掉的条数只能进 `projects_hidden`。
const MAX_PROJECT_HITS: usize = 10;
```

`FieldHit`（`:18-29`）之后追加三个 DTO（字段与注释逐字照 spec §三；`Debug` 是给测试的失败信息用的）：

```rust
/// 一个项目在一个段里的命中。两段的簇形状除 `items` 元素类型外逐字段相同，所以只写一个泛型结构；
/// 泛型不动线格式：serde 按 `T` 的具体实例化生成序列化，`rename_all` 照样生效。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cluster<T> {
    pub project_id: String,
    pub project_name: String,
    pub items: Vec<T>,
    /// 簇内被 `MAX_ITEMS_PER_CLUSTER` 截掉的条数。只显示、不做展开（§十）。
    /// 只相对本次取到的样本，见 `DOCS_FETCH_LIMIT` 的头注释。
    pub hidden: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSection<C> {
    pub clusters: Vec<C>,
    /// 被 `MAX_CLUSTERS_PER_SECTION` 整簇扔掉的簇数。
    /// 只相对本次取到的样本，见 `DOCS_FETCH_LIMIT` 的头注释。
    pub hidden_clusters: usize,
}

/// 首屏一次 IPC 的全部返回。三段之间**不做分数比较**：bm25 与 LIKE 不可比（§3.4）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBundle {
    pub query: String,
    /// `source == "project"` 的命中，平铺，最多 `MAX_PROJECT_HITS` 条。
    pub projects: Vec<FieldHit>,
    /// 平铺段被截掉的条数。台账段一样只相对本次取到的样本，
    /// 而且它上面还压着 `PER_GROUP_LIMIT`（每组取数上限），所以低报的方向与正文段相同。
    pub projects_hidden: usize,
    pub ledger: ClusterSection<Cluster<FieldHit>>,
    pub docs: ClusterSection<Cluster<DocHit>>,
    /// 正文段存在 `matchedBy == "prefix"`。段级说明只出一次（D7）。
    /// 在截断前的 docs 上算：否则放宽命中全被截掉时，这个说明会凭空消失。
    pub relaxed: bool,
    /// `index_store::indexed_project_count`：空态文案用它区分「还没建索引」与「没命中」。
    pub indexed_projects: usize,
}
```

`field_hits` 之后追加两个 pub 函数（`unified_bundle`）与一个私有助手（`cluster_by_project`）：

```rust
/// 段内按项目聚簇。簇间序 = 命中条数降序 → 项目名升序 → 项目 id 升序。
/// 第三级兜底不是因为库里会有两个同项目（`projects.id` 是主键），而是为了「簇序可复现」：
/// 前两级在库里**不唯一**（name 无 UNIQUE，条数更是常并列）。
///
/// 三键的守护现状（Task 4 实测，别把它读成「三级都有测试」）：
/// - 第一级条数：`doc_clusters_sort_by_hit_count_then_project_name` 里 Gamma 那 5 条守住；
/// - 第二级项目名：确定性红在 `section_drops_the_21st_cluster_and_reports_it`（21 个等条数簇，
///   断的是「名序最大者被扔掉」）；`doc_clusters_sort_by_hit_count_then_project_name` 只能概率性红 ——
///   `project::create_project` 的 id 是随机 UUID v4（`project.rs:110`），去掉名字键后 Alpha/Beta
///   谁在前就是抛硬币（首轮实测 10 次里只红 2 次）；
/// - 第三级 id：**当前没有任何测试能把它打红**。要它生效得有两个同名且同条数的簇，而测试没法把
///   随机 id 的「插入序 vs id 序」摆成固定先后，`sort_by` 又是稳定排序。有名缺口，交 M5
///   （补法要么给 `projects.id` 开一个测试注入点，要么在测试里用裸 SQL 固定 id）。
fn cluster_by_project<T>(
    items: Vec<T>,
    project_of: impl Fn(&T) -> (String, String),
) -> ClusterSection<Cluster<T>> {
    let mut grouped: Vec<(String, String, Vec<T>)> = Vec::new();
    for item in items {
        let (project_id, project_name) = project_of(&item);
        match grouped.iter_mut().find(|(pid, _, _)| *pid == project_id) {
            Some(entry) => entry.2.push(item),
            None => grouped.push((project_id, project_name, vec![item])),
        }
    }
    grouped.sort_by(|a, b| {
        b.2.len()
            .cmp(&a.2.len())
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.0.cmp(&b.0))
    });
    let hidden_clusters = grouped.len().saturating_sub(MAX_CLUSTERS_PER_SECTION);
    let clusters = grouped
        .into_iter()
        .take(MAX_CLUSTERS_PER_SECTION)
        .map(|(project_id, project_name, mut items)| {
            let hidden = items.len().saturating_sub(MAX_ITEMS_PER_CLUSTER);
            items.truncate(MAX_ITEMS_PER_CLUSTER);
            Cluster { project_id, project_name, items, hidden }
        })
        .collect();
    ClusterSection { clusters, hidden_clusters }
}

/// 首屏唯一入口：一次调用带回三段与三个截断计数。
/// 空判与长度守卫都不在这里做（§八：`field_hits` 与 `doc_hits` 各自负责，本项目只在边界校验一次），
/// 所以 129 字的查询由 `field_hits` 回 `invalid_input`，空白查询回一个空 bundle。
pub fn unified_bundle(conn: &Connection, query: &str) -> AppResult<SearchBundle> {
    let fields = field_hits(conn, query)?;
    let docs = index_store::doc_hits(conn, query, DOCS_FETCH_LIMIT)?;
    // 「项目」段平铺、其余五段聚簇：思维导图把这两类分开画，前端渲染也不同款。
    let (mut projects, ledger_rows): (Vec<FieldHit>, Vec<FieldHit>) =
        fields.into_iter().partition(|h| h.source == "project");
    let projects_hidden = projects.len().saturating_sub(MAX_PROJECT_HITS);
    projects.truncate(MAX_PROJECT_HITS);
    // relaxed 取自**截断前**的完整 docs 集合，簇内截断不影响它。
    let relaxed = docs.iter().any(|h| h.matched_by == "prefix");
    let indexed_projects = index_store::indexed_project_count(conn)?;
    Ok(SearchBundle {
        query: query.trim().to_owned(),
        projects,
        projects_hidden,
        ledger: cluster_by_project(ledger_rows, |h| (h.project_id.clone(), h.project_name.clone())),
        docs: cluster_by_project(docs, |h| (h.project_id.clone(), h.project_name.clone())),
        relaxed,
        indexed_projects,
    })
}
```

`index_store.rs` 里 `doc_hits` 的头注释有一条 M3 时期写下的过期指针（现在读作「Task 10 的 IPC：`limit.unwrap_or(50).clamp(1, 200)`」，而本计划里做接线的是 Task 6），本任务把它换成现名。**只动那两行注释，同一函数体的代码一个字都不许碰**（`src/search.rs` 才是 Step 5 门禁扫的文件，这段落在 `index_store.rs`，但指针过期同样会误导读者）：

```rust
/// `limit` 原样进 SQL，这里不校验也不补默认值：SQLite 里负数 LIMIT = 不限行、0 = 无行，
/// clamp 属于调用方的系统边界（`lib.rs:542` 的 `search_docs` 是唯一做过 clamp 的入口，
/// 首屏那条走 `search::DOCS_FETCH_LIMIT`）。
/// 刻意不做第二道校验 —— 本项目只在边界校验一次，两道 clamp 会漂成两个数。
```

Run: `cd src-tauri && cargo test --lib search; echo exit=$?`
Expected: `20 passed; 0 failed`（原 9 + 本任务 11）。

Run: `cd src-tauri && cargo test --lib; echo exit=$?`
Expected: `120 passed; 0 failed`（108 + 11 + 1）。

- [ ] **Step 4: 变异取证（每个截断计数与每条排序都要被单独打红）**

1. 三个计数各写死成 `0`（`hidden_clusters`、`hidden`、`projects_hidden` 三处分别改）：`section_drops_the_21st_cluster_and_reports_it`、`cluster_keeps_10_items_and_reports_the_other_2`、`flat_project_section_truncates_into_projects_hidden` 各红一条。这三项是「不静默丢弃」的唯一证据。
2. **反转** `.then_with(|| a.1.cmp(&b.1))` 的比较方向（改成 `b.1.cmp(&a.1)`），不要用「删掉这一行」的形态：删掉后并列簇落到随机的 UUID id 序上，`doc_clusters_sort_by_hit_count_then_project_name` 只有约 2/10 概率红（Task 4 首轮实测正是 2/10，报的是「捕获到了但红点与预测不同」）。反转方向才能让两个捕获者都确定性红：`section_drops_the_21st_cluster_and_reports_it`（p20 不再被扔、混进前 20 个簇）与 `doc_clusters_sort_by_hit_count_then_project_name`（变成 Gamma, Beta, Alpha）。两条都要在报告里贴出红点。
3. `cluster_by_project` 里 `items.truncate(...)` 之前加一句 `items.reverse()`：`items_keep_the_bm25_order_that_doc_hits_returns` 必须红（3 条 id 互不相同，奇数长度 reverse 不可能等于原序，所以这条不存在「恰好对称而假绿」）。
4. 把 `relaxed` 改成在截断后的 docs 簇上算（`bundle.docs.clusters.iter().any(...)`）：`relaxed_is_the_prefix_stage_and_nothing_else` 的第一条断言在 12 条截断夹具下仍然绿，**所以要另加一步**：把 `MAX_ITEMS_PER_CLUSTER` 临时改成 `0` 跑一次该测试，红则说明截断前/后确实有差异；然后改回 `10`。把两次结果都贴进报告。
5. `unified_bundle` 里补一道 `if query.trim().is_empty() { return Err(AppError::new("invalid_input", ..)) }`：`empty_query_yields_an_empty_bundle_instead_of_an_error` 必须红。
6. `partition` 的谓词从 `== "project"` 改成 `!= "note"`：`ledger_section_clusters_its_own_hits_instead_of_leaking_into_projects` 必须红（env 命中溜进平铺段，`b.projects.is_empty()` 与 `b.ledger.clusters.len() == 1` 一起倒）。**这条在补第 11 条测试之前是等价变异**：旧夹具的台账段只有空命中，谓词怎么写都得空簇，首轮实测「单条测试 exit=0、全量 `119 passed`」就是这个原因，所以变异 6 的红点必须在第 11 条上，报告要指名它。
7. （**预期不红**的取证，必须实跑并把绿贴进报告）去掉第三级 `.then_with(|| a.0.cmp(&b.0))`：预期全量 `120 passed / 0 failed` 不变。理由写在 `cluster_by_project` 的头注释里 —— `sort_by` 稳定、夹具项目名互异，走不到第三级。不许为凑红改夹具、不许为凑红在测试里裸 SQL 注 id；这条是登记在注释里的有名缺口，不是待办。

Expected: 变异回滚后 `20 passed; 0 failed`（全量 `120 passed`）。

- [ ] **Step 5: 一条 grep 门禁（防「数值又漂回字面量」）**

```bash
cd src-tauri && grep -rn "take(20)\|truncate(10)\|, 200)\|unwrap_or(50)" src/search.rs; echo exit=$?
```

Expected: `exit=1`（0 命中）。本任务把四个数写成常量后，`search.rs` 里不该再出现任何裸字面量形态的同款上限；`doc_hits(conn, query, DOCS_FETCH_LIMIT)` 走的是名字。

- [ ] **Step 6: clippy 两道 + 提交**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings; echo exit=$?
cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
git add src-tauri/src/search.rs src-tauri/src/index_store.rs
git commit -m "feat: M4 统一检索聚合：三段 bundle、段内按项目聚簇与三个截断计数"
```

Expected: 两条 `exit=0` 且 0 条 warning；提交只含这两个文件。

---

### Task 5: `doc_preview.rs` —— 重抽原文 + UTF-16 码元高亮区间

**Files:**
- Create: `src-tauri/src/doc_preview.rs`（两条命令归 Task 6，本任务只交付可单测的库层函数）
- Modify: `src-tauri/src/lib.rs`（`mod` 区 `:1-11` 按字母序加一行 `mod doc_preview;`，落在 `mod db;` 与 `mod error;` 之间。**这一行必须在本任务落地**：没有它 `doc_preview.rs` 不属于这个 crate，Step 2 期望的那条编译错误、Step 3 的 `14 passed` 与 `134 passed` 全都不可达，`cargo test --lib` 会一声不响地停在 120）
- Modify: `src-tauri/Cargo.toml:46`（注释里的 `index_job::panic_to_err` 改成 `extract::panic_to_err`，并把预览这条新通路写进「哪些路径受 panic 边界保护」那句）
- Modify: `src-tauri/src/extract.rs:577` 附近（`release_profile_does_not_abort_so_panic_guards_work` 的 doc 里「以及 `index_job` 的两道边界」改成指认搬迁后的真实归属）

`Cargo.toml` 与 `extract.rs` 这两处 Modify 是 **Task 2 评审登记的 plan-mandated 范围遗漏**（`f72a5c8` 把边界助手搬到 `extract.rs` 后，`Cargo.toml:46` 的注释字面悬空指向一个不存在的符号，而 Task 2 的 grep 门只扫 `src/`、提交范围只有两个文件，够不着它）。落在本任务而不是回头补 Task 2，是因为本任务正是**第三条**按 path 调用抽取器的通路：那两条注释说的「哪些路径有边界兜着」必须在本任务之后才为真，改在别处都还要再改一次。两处都是纯注释、零行为回归，**不许**顺手动同文件里的任何代码。

`lib.rs` 那一行是**控制方派发前预检补进来的计划修订**（原计划把模块声明整条划给 Task 6，与本任务自己的 RED 期望和条数门禁冲突）。它的连带后果是 `cargo clippy --lib -- -D warnings`：该命令不带 `--tests`，即 `cfg(test)` 关闭，此时 `#[cfg(test)] mod tests` 不参与编译，本格里每个 `pub` 项（含 `PreviewRow`/`DocPreview` 的字段）在 lib 目标里都没有 caller，rustc 会逐条报 `dead_code`（控制方用 `rustc --crate-type lib --deny warnings` 在最小夹具上实测过）。所以 Step 1 的文件头要带一行**具名回收点**的模块级豁免，形态与 Task 4 给 `search::unified_bundle` 开的那行同形，由 Task 6 的 Step 3 grep 门回收。

**Interfaces:**
- Consumes：Task 1 的 `tokenize::query_terms`（**预览的词只从这一处拿**，spec §五.4）、Task 2 的 `extract::panic_to_err`（`pub(crate) fn panic_to_err<T, F: FnOnce() -> T>(f: F, what: &str) -> AppResult<T>`）、既有 `extract::extract_text`（`pub fn extract_text(path: &Path) -> AppResult<String>`）、`index_store::write_doc` + `DocOutcome` + `ScannedFile`（只用于造库里的行）。
- Produces（Task 6 的 `doc_preview` 命令按这个顺序串起来）：
  - `pub struct PreviewRow { pub path: String, pub project_name: String }`
  - `pub fn row_for_preview(conn: &Connection, doc_id: &str) -> AppResult<PreviewRow>` —— **只能在持锁时调**，它只读库。
  - `pub fn render_preview(doc_id: &str, row: &PreviewRow, query: &str) -> AppResult<DocPreview>` —— **必须在锁外调**，它要读盘。
  - `pub struct DocPreview { doc_id, path, project_name, text, ranges: Vec<(usize, usize)>, truncated }`，`#[derive(Debug, Serialize)] #[serde(rename_all = "camelCase")]`，线格式 `docId` / `projectName` / `truncated` / `ranges`（JSON 数组的数组，前端类型 `[number, number][]`）。
  - 错误码：`preview_unavailable`、`preview_file_missing`、`extract_failed`（后两者由 panic 边界与 io 翻译给出）。

**两条算术不变量**（spec §三、§五.5）：`ranges` 用 **UTF-16 码元**下标（前端 `text.slice` 零换算），所以必须有「不是字节偏移」和「BMP 外字符记 2 码元」两条测试各自钉住；窗口分隔串 `PREVIEW_GAP` 留在 `text` 里，`ranges` 按构造不跨它。

- [ ] **Step 1: 登记模块，并写文件头与常量、DTO（先让红测试能编译到函数名）**

`src-tauri/src/lib.rs` 的 `mod` 区加一行（按字母序落在 `mod db;` 与 `mod error;` 之间）：

```rust
mod doc_preview;
```

`src-tauri/src/doc_preview.rs` 的文件头：

```rust
//! 点开正文命中后的原文预览：查库拿路径 → 锁外重抽原文 → 在原文上定位 → 窗口化 → 码元区间。
//!
//! 两条红线在这一格里：
//! 1. **本模块不自己开文件读盘**，读盘只经 `extract::extract_text`（同一条扩展名分派表、
//!    同一套 chardetng + encoding_rs）。这样「索引与检索对磁盘的唯一动作是读」仍只需 grep
//!    `extract.rs` 一处（今天只有 `:44` 与 `:130`）。
//! 2. **抽取必须包在 panic 边界里**（`extract::panic_to_err`）。预览跑在 IPC 命令线程上，
//!    没有边界的话，一份畸形 docx 被用户在首屏点开就当场终止应用进程。

// 临时豁免，Task 6 落地 `doc_preview` IPC 时必须删掉这一段（Step 3 有 grep 门守着，与
// `search::unified_bundle` 在 Task 4 的同形豁免一起回收）：本模块的第一个 caller 在 Task 6，
// 而 `cargo clippy --lib` 不带 `--tests`，cfg(test) 关闭时下面每个 pub 项都没有 caller，
// rustc 会逐条判 dead_code。
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::extract::{panic_to_err, extract_text};
use crate::tokenize::query_terms;

/// 一扇窗的**半宽**（按 char 计），所以**没被并过**的单扇窗最长
/// `2 * PREVIEW_WINDOW_CHARS + 命中自身长度` 个 char（两端各扩一个半宽，中间还得放下命中本身
/// —— Task 5 复审指出原句少了后半截）。链式合并会让窗更长：`MAX_PREVIEW_WINDOWS` 只封顶**窗数**、
/// 不封顶窗长，命中每隔不到两个半宽出现一次时会并成一扇，长文里这扇可以接近整篇，由 `truncated` 说实话。
/// （Task 5 复审 Minor-1：并窗判据从「命中起点」换成「新窗左沿」后合并带翻倍，旧句的「单窗最长」成了假话。）
/// 不要求落在词或行边界上：预览是给人核对原文的，切在字中间比多加一层对齐逻辑更好读。
const PREVIEW_WINDOW_CHARS: usize = 4_000;
/// 一次预览最多拼几扇窗；多出来的命中直接丢弃，由 `truncated` 说实话。
const MAX_PREVIEW_WINDOWS: usize = 3;
/// 窗与窗之间的分隔串，**留在** `text` 里（3 个字符、各 1 个码元）。
/// 它不参与命中匹配，所以 `ranges` 按构造永不跨窗——跨了前端就会把上窗尾部涂到下窗头上。
const PREVIEW_GAP: &str = "\n⋯\n";

/// `index_docs` 一行里预览要用的两列。
pub struct PreviewRow {
    pub path: String,
    pub project_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocPreview {
    pub doc_id: String,
    pub path: String,
    pub project_name: String,
    /// 窗口拼接后的原文，不是整篇。
    pub text: String,
    /// **UTF-16 码元**偏移（既不是字符偏移也不是字节偏移），后端算好。
    pub ranges: Vec<(usize, usize)>,
    /// `text` 不等于整篇原文：任一窗口切断了原文，或有命中没能进窗口。
    pub truncated: bool,
}
```

这一格里两处新东西各有讲究：`#![allow(dead_code)]` 是带具名回收点的临时豁免（Task 6 Step 3 的 grep 门负责回收，形态与 Task 4 给 `search::unified_bundle` 开的那行同形）；而豁免那 4 行注释里**不许**写 Step 5 门要扫的那几个字面量形态 —— 本文件被 `File::open` 那条门扫，把它写进注释就把自己弄红（M4 已登记的坑第 49 条的同一个形状）。同理，文件头第 1 条红线写「本模块不自己开文件读盘」是刻意避开那个字面量的写法，不是漏写，语义与原句一致。

- [ ] **Step 2: 写 14 条红测试**

tests 模块的夹具与助手（`render_preview_with` 收抽取器参数，所以偏移算术与并窗算术一条磁盘都不碰；只有那两条「真走 `extract_text`」的用例例外，它们用**注定不存在的路径**（`C:/definitely/not/here.txt` / `.png`）来分别触发 io 翻译与不支持两种码 —— 本任务**不需要 `tempfile`**，在磁盘上造夹具反而更靠近红线）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, index_scan::ScannedFile, index_store, index_store::DocOutcome};
    use rusqlite::params;

    /// 只回固定正文的「抽取器」：偏移与窗口算术的用都不该需要磁盘。
    fn body(text: String) -> impl Fn(&Path) -> AppResult<String> {
        move |_| Ok(text.clone())
    }

    fn row_for(path: &str) -> PreviewRow {
        PreviewRow { path: path.to_owned(), project_name: "政务云迁移".to_owned() }
    }

    /// 按拼出来的 `text` 取一段码元切片：前端用的就是 `String.prototype.slice`，
    /// 测试必须用同一把尺子，否则「码元契约」只是注释。
    fn utf16_slice(text: &str, r: &(usize, usize)) -> String {
        text.encode_utf16()
            .skip(r.0)
            .take(r.1 - r.0)
            .flat_map(|u| char::decode_utf16([u]).map(|c| c.unwrap_or('\u{FFFD}')))
            .collect()
    }

    fn conn() -> Connection {
        let c = db::open_in_memory().unwrap();
        c.execute("INSERT INTO projects (id, name, status) VALUES ('p1', '政务云迁移', '立项')", [])
            .unwrap();
        c
    }

    /// 造一行 `index_docs` 并回它的 `id`（`write_doc` 回的是 doc_rowid，预览按 id 查）。
    fn seed_row(conn: &Connection, path: &str, outcome: DocOutcome) -> String {
        let scanned = ScannedFile {
            path: path.to_owned(),
            file_name: path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned(),
            ext: path.rsplit('.').next().unwrap_or("").to_owned(),
            size: 1024,
            mtime: 1_700_000_000,
        };
        let rowid = index_store::write_doc(conn, "p1", &scanned, outcome).unwrap();
        conn.query_row("SELECT id FROM index_docs WHERE doc_rowid = ?1", params![rowid], |r| r.get(0))
            .unwrap()
    }

    /// 与 Task 6 命令体同一条链：锁内查库、锁外抽盘。
    fn preview(conn: &Connection, doc_id: &str, q: &str) -> AppResult<DocPreview> {
        render_preview(doc_id, &row_for_preview(conn, doc_id)?, q)
    }
```

14 条测试（前 9 条纯算术，后 5 条真通路）：

```rust
    #[test]
    fn preview_highlights_every_hit_in_the_stitched_text() {
        let p = render_preview_with("d1", &row_for("C:/x/合同.docx"), "验收", body("甲方要求验收指标见合同附件".to_owned())).unwrap();
        assert_eq!(p.ranges.len(), 1);
        assert_eq!(utf16_slice(&p.text, &p.ranges[0]), "验收");
        assert_eq!(p.doc_id, "d1");
        assert!(!p.truncated, "正文短到一扇窗装得下时不该说自己是截断的");
    }

    /// 钉「不是字节偏移」：中文一处就翻 3 倍，前端 slice 会整段错位。
    #[test]
    fn ranges_are_utf16_units_not_byte_offsets() {
        let p = render_preview_with("d1", &row_for("C:/x/a.docx"), "验收", body("甲方验收".to_owned())).unwrap();
        assert_eq!(p.ranges, vec![(2, 4)], "字节偏移会是 (4, 8)");
    }

    /// 钉「不是 char 偏移」：命中前有一个 BMP 外字符，码元比 char 多 1。
    /// 这条是前端 `text.slice` 能对齐的唯一证据。
    #[test]
    fn a_bmp_outside_char_before_the_hit_costs_two_units() {
        let p = render_preview_with("d1", &row_for("C:/x/a.docx"), "验收", body("🎉甲方验收".to_owned())).unwrap();
        assert_eq!(p.ranges, vec![(4, 6)], "char 偏移会是 (3, 5)");
        assert_eq!(utf16_slice(&p.text, &p.ranges[0]), "验收");
    }

    #[test]
    fn matching_is_case_insensitive_and_keeps_the_original_spelling() {
        let p = render_preview_with(
            "d1",
            &row_for("C:/x/a.docx"),
            "payment",
            body("Payment terms and payment notes and PAYMENT due".to_owned()),
        )
        .unwrap();
        let got: Vec<String> = p.ranges.iter().map(|r| utf16_slice(&p.text, r)).collect();
        assert_eq!(got, ["Payment", "payment", "PAYMENT"], "高亮要涂原文写法，不是小写串");
    }

    /// §五.4 的「检索命中的词与预览高亮的词同一套切词」在这一条上可观测：
    /// 用户敲「验收，」，FTS 侧丢掉「，」命中正文，预览也必须照样涂出来。
    #[test]
    fn punctuation_in_the_query_is_dropped_by_the_shared_tokenizer() {
        let p = render_preview_with("d1", &row_for("C:/x/a.docx"), "验收，", body("甲方要求验收指标".to_owned())).unwrap();
        assert_eq!(p.ranges.len(), 1, "针若取 query 原文（含「，」）就会 0 命中");
    }

    #[test]
    fn multi_word_query_highlights_both_terms() {
        let p = render_preview_with("d1", &row_for("C:/x/a.docx"), "合同 报价", body("合同报价单里写了验收".to_owned())).unwrap();
        assert_eq!(p.ranges, vec![(0, 2), (2, 4)], "相邻但不重叠的两处保持分开，前端各涂一格");
    }

    #[test]
    fn near_hits_share_one_window_so_no_gap_is_emitted() {
        // 夹具是按半宽算出来的，不是随手一个短串：HALF = `PREVIEW_WINDOW_CHARS` = 4000，
        // 第一处命中 (0,2) 的窗是 `[0, 4002]`；要让第二处命中的**窗左沿**恰等于 4002（相接），
        // 它的 `start` 就得落在 `4002 + HALF = 8002`。
        // 于是并窗判据从 `<=` 翻成 `<` 时两扇窗不再相接 → `stitch` 多插一个 `PREVIEW_GAP` → 本条红；
        // 而 `<=` 下是一扇覆盖全文的窗 → `p.text` 就是原文、不含分隔串。
        // （原夹具 `"甲验收乙"` 只有一处命中，并窗分支根本不执行，Step 4 变异 3 因此在给定夹具下
        // 逻辑不可满足 —— Task 5 首轮评审 Important-2，名不符实的假守护。）
        let text = format!("验收{}验收", "字".repeat(8_000));
        let p = render_preview_with("d1", &row_for("C:/x/a.docx"), "验收", body(text.clone())).unwrap();
        assert_eq!(text.chars().count(), 8_004, "间距算错了这条就退化成单命中夹具");
        assert_eq!(p.ranges, vec![(0, 2), (8_002, 8_004)], "两处命中进同一扇窗，各自涂一格");
        assert!(!p.truncated, "相接的两处命中该并成一扇，窗数没超上限不该截断");
        assert!(!p.text.contains(PREVIEW_GAP), "两窗相接还分开拼就会多出一个假省略号");
        assert_eq!(p.text, text);
    }

    #[test]
    fn four_spaced_hits_give_three_windows_and_the_fourth_is_dropped() {
        let mut text = String::new();
        for _ in 0..4 {
            text.push_str("验收");
            text.push_str(&"字".repeat(10_000));
        }
        let p = render_preview_with("d1", &row_for("C:/x/a.docx"), "验收", body(text)).unwrap();
        assert_eq!(p.ranges.len(), MAX_PREVIEW_WINDOWS, "第 4 处超出窗数上限就不该出现");
        assert!(p.truncated);
        assert_eq!(p.text.match_indices(PREVIEW_GAP).count(), MAX_PREVIEW_WINDOWS - 1);
        for r in &p.ranges {
            assert!(!utf16_slice(&p.text, r).contains('⋯'), "range 跨过分隔串就会把上窗尾部涂进下窗");
        }
    }

    /// §五.6：纯文件名命中时原文里一处都匹配不上，`ranges` 为空是合法结果。
    #[test]
    fn a_file_name_only_match_yields_no_ranges_but_still_shows_a_head_window() {
        let text = "数".repeat(9_000);
        let p = render_preview_with("d1", &row_for("C:/x/报价单.docx"), "报价单", body(text.clone())).unwrap();
        assert!(p.ranges.is_empty());
        assert_eq!(p.text.chars().count(), 2 * PREVIEW_WINDOW_CHARS, "无命中时给开头一扇全长窗");
        assert!(p.truncated);
    }

    #[test]
    fn a_missing_row_is_preview_unavailable() {
        let c = conn();
        let e = preview(&c, "no-such-doc", "验收").unwrap_err();
        assert_eq!(e.code, "preview_unavailable");
    }

    #[test]
    fn a_skipped_row_is_preview_unavailable() {
        let c = conn();
        let id = seed_row(&c, "C:/x/大图.png", DocOutcome::Skipped("超出大小上限"));
        let e = preview(&c, &id, "验收").unwrap_err();
        assert_eq!(e.code, "preview_unavailable", "skipped 行没有正文可抽，不能假装能读");
    }

    #[test]
    fn a_soft_deleted_projects_row_is_preview_unavailable() {
        let c = conn();
        let id = seed_row(&c, "C:/x/合同.docx", DocOutcome::Ok("甲方要求验收指标".into()));
        c.execute("UPDATE projects SET deleted_at = datetime('now') WHERE id = 'p1'", []).unwrap();
        assert_eq!(preview(&c, &id, "验收").unwrap_err().code, "preview_unavailable");
    }

    /// io 码要翻译，其余码不许改写（前端只认 `preview_*` 两个码）。
    #[test]
    fn io_errors_are_translated_and_other_codes_pass_through() {
        let missing = render_preview_with("d1", &row_for("C:/definitely/not/here.txt"), "验收", extract_text).unwrap_err();
        assert_eq!(missing.code, "preview_file_missing", "fs_failed 不能原样给前端");
        let png = render_preview_with("d1", &row_for("C:/definitely/not/here.png"), "验收", extract_text).unwrap_err();
        assert_eq!(png.code, "extract_unsupported", "非 io 码保持原样");
    }

    /// M3 的 I2 教训：中间层没有测试等于没守。这一条钉住 panic 边界真的接到了预览这条路上。
    #[test]
    fn a_panicking_extractor_is_caught_not_fatal() {
        let boom: fn(&Path) -> AppResult<String> = |_| panic!("模拟解析器在畸形文件上炸");
        let e = render_preview_with("d1", &row_for("C:/x/a.docx"), "验收", boom).unwrap_err();
        assert_eq!(e.code, "extract_failed");
    }
```

`body` 的口径就是上面那一条：收 `String`、闭包捕获并 clone。所有调用点已经写成 `body("…".to_owned())` 或 `body(text)`，**不要**改成 `&'static str` 或加 `impl Into<String> + 'static` 之类的生命周期花招（长文本那两条需要 move，短字面量那七条也统一 move，读起来只有一种形状）。

Run: `cd src-tauri && cargo test --lib doc_preview; echo exit=$?`
Expected: 编译失败 —— `cannot find function render_preview_with in this scope`（Step 1 的 DTO 已在，函数还没有）。

- [ ] **Step 3: 实现查库、翻译、算术与拼接**

错误与查库：

```rust
fn unavailable() -> AppError {
    AppError::new(
        "preview_unavailable",
        "这条索引记录不能预览",
        Some("记录可能来自已注销的目录，或索引重建前的旧结果；到 /index 重跑一轮索引"),
    )
}

/// 只在持锁时调用；返回后**释放锁**再抽盘。握着 `state.conn` 去读盘会把整个应用卡在一次慢 IO 上
/// （索引作业那条路之所以自开连接，就是同一个理由）。
/// 行不存在 / 项目已软删 / `index_status != 'ok'` 三种都归 `preview_unavailable`：
/// skipped 与 failed 的行本来就没有正文，不能假装能读（spec §五.1）。
pub fn row_for_preview(conn: &Connection, doc_id: &str) -> AppResult<PreviewRow> {
    let row = conn
        .query_row(
            "SELECT d.path, p.name
               FROM index_docs d
               JOIN projects p ON p.id = d.project_id AND p.deleted_at IS NULL
              WHERE d.id = ?1 AND d.index_status = 'ok'",
            params![doc_id],
            |r| Ok(PreviewRow { path: r.get(0)?, project_name: r.get(1)? }),
        )
        .optional()?;
    row.ok_or_else(unavailable)
}

/// 抽取层的 io 码（`AppError::io` 的 `fs_failed`）在预览这里翻译成 `preview_file_missing`：
/// 文件改名/删除是预览独有的失效形态，不该让前端去认识 `extract` 内部的 io 码名（spec §六）。
/// **其余码原样透出**（`extract_unsupported`、`extract_failed`、`db_failed`）。
fn map_io_error(e: AppError) -> AppError {
    if e.code == "fs_failed" {
        AppError::new(
            "preview_file_missing",
            "文件已不可达",
            Some("文件可能已改名或被移走；真正的失效标记要到 M5 的核对轮才会写回记录"),
        )
    } else {
        e
    }
}

/// 生产入口：抽取器固定为 `extract::extract_text`。
pub fn render_preview(doc_id: &str, row: &PreviewRow, query: &str) -> AppResult<DocPreview> {
    render_preview_with(doc_id, row, query, extract_text)
}

/// 收抽取器参数不是测试专用注入缝：`extract_text` 会在畸形 docx/pdf 上 panic，
/// 而 panic 边界只能在这一层包，所以「边界在不在」必须能被一条 `|_| panic!()` 的用例打红（M3 的 I2）。
fn render_preview_with(
    doc_id: &str,
    row: &PreviewRow,
    query: &str,
    extract_with: impl Fn(&Path) -> AppResult<String>,
) -> AppResult<DocPreview> {
    let path = PathBuf::from(&row.path);
    let raw = panic_to_err(move || extract_with(&path), &row.path)
        .and_then(|r| r)
        .map_err(map_io_error)?;
    let chars: Vec<char> = raw.chars().collect();
    let hits = hit_ranges(&raw, query);
    let (windows, dropped) = take_windows(&hits, chars.len());
    // 无命中（纯文件名命中的那一类，§五.6）也要给东西看：取开头一扇全长窗。
    let shown = if windows.is_empty() {
        vec![(0usize, chars.len().min(2 * PREVIEW_WINDOW_CHARS))]
    } else {
        windows
    };
    let whole = shown.len() == 1 && shown[0] == (0, chars.len());
    let (text, ranges) = stitch(&chars, &shown, &hits);
    Ok(DocPreview {
        doc_id: doc_id.to_owned(),
        path: row.path.clone(),
        project_name: row.project_name.clone(),
        text,
        ranges,
        truncated: dropped || !whole,
    })
}
```

纯算术四件（`lower_first` / `hit_ranges` / `take_windows` / `stitch`）：

```rust
/// 逐 char 取小写**首字符**：char 数严格不变，这是「下标能对上原文」的前提。
/// ß→ss 这类展开成两字符的映射按首字符比，代价是那种词少认一次命中；
/// 换来的是偏移永不因折叠而漂——漂了前端就涂错色，比少涂一格严重（spec §五.4）。
fn lower_first(src: &str) -> String {
    src.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect()
}

/// 原文上的 **char** 区间（半开），升序且已合并重叠。
/// 词只从 `tokenize::query_terms` 拿：检索命中的词与预览高亮的词必须同一套切词，
/// 这里另起一次 `cut` 就会随词典版本漂成两处口径（spec §五.4）。
fn hit_ranges(text: &str, query: &str) -> Vec<(usize, usize)> {
    let hay: Vec<char> = lower_first(text).chars().collect();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for term in query_terms(query) {
        let needle: Vec<char> = lower_first(&term).chars().collect();
        if needle.is_empty() || needle.len() > hay.len() {
            continue;
        }
        let mut from = 0usize;
        while from + needle.len() <= hay.len() {
            let Some(off) = hay[from..].windows(needle.len()).position(|w| w == needle.as_slice()) else {
                break;
            };
            let at = from + off;
            out.push((at, at + needle.len()));
            from = at + needle.len(); // 同词的不重叠出现：跳过整段继续找
        }
    }
    out.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in out {
        match merged.last_mut() {
            // `<` 不是笔误：这里比的是**命中**，两段首尾相接（`s == prev.1`）在原文上是两格，
            // 前端要各涂一格，所以不并。真正必须分开的是「合同」「报价」这种相邻不重叠的命中
            // （`multi_word_query_highlights_both_terms` 断 `[(0,2),(2,4)]`）；写 `<=` 会把它们
            // 并成一格，与本任务自己的用例冲突。Task 5 首轮评审裁定：断言是规格，实现文本写错了。
            Some(prev) if s < prev.1 => prev.1 = prev.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

/// 以命中为中心扩半宽，**左沿相接或越过上一扇右沿**的窗并成一扇；超出 `MAX_PREVIEW_WINDOWS` 的命中丢弃并置 `truncated`。
/// 只与**最后一扇**比就够：命中升序，窗也升序。
fn take_windows(hits: &[(usize, usize)], text_len: usize) -> (Vec<(usize, usize)>, bool) {
    let mut windows: Vec<(usize, usize)> = Vec::new();
    let mut dropped = false;
    for &(start, end) in hits {
        let hi = (end + PREVIEW_WINDOW_CHARS).min(text_len);
        // 比的是新窗的**左沿** `lo`，不是命中起点 `start`：窗是 `[start - 半宽, end + 半宽]`，
        // 拿 `start` 去比，两处命中相距落在「一个半宽到两个半宽」这条带里时既不并窗、`lo` 又越过
        // 上一扇的右沿 —— `stitch` 会把中间那段正文**吐两遍**，还在它中间插一个代表「此处有省略」的
        // `PREVIEW_GAP`，而那段正文根本没被省略（Task 5 首轮评审 Important-1）。
        let lo = start.saturating_sub(PREVIEW_WINDOW_CHARS);
        // 这里刻意不用 `match windows.last_mut()` 里再写 `windows.len()` / `windows.push()`：
        // `last_mut()` 的可变借用横跨所有分支，那两个调用会撞 E0502（计划原文就是这个形状，
        // Task 5 实现者实测编不过）。借用在 `merged` 出作用域前结束，所以先算出布尔再分支。
        let merged = match windows.last_mut() {
            Some(prev) if lo <= prev.1 => {
                prev.1 = prev.1.max(hi);
                true
            }
            _ => false,
        };
        if merged {
            continue;
        }
        if windows.len() >= MAX_PREVIEW_WINDOWS {
            dropped = true;
        } else {
            windows.push((lo, hi));
        }
    }
    (windows, dropped)
}

fn utf16_len(chars: &[char]) -> usize {
    chars.iter().map(|c| c.len_utf16()).sum()
}

/// 按窗口切原文、中间插 `PREVIEW_GAP`，同时把命中从「原文 char 坐标」换算到
/// 「拼出来的 text 的码元坐标」。换算只做一次，就在写入每一扇窗的时候。
fn stitch(chars: &[char], windows: &[(usize, usize)], hits: &[(usize, usize)]) -> (String, Vec<(usize, usize)>) {
    let mut out = String::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut base = 0usize; // 已写入 out 的码元数
    for (i, &(ws, we)) in windows.iter().enumerate() {
        if i > 0 {
            out.push_str(PREVIEW_GAP);
            // 分隔串的码元数就地算，不写死 3（Task 5 复审 Minor(b)：老写法每扇窗白做一次 collect，
            // 且计划原文那句 `collect::<Vec<char>` 的尖括号根本没闭合，编译不过）。
            base += PREVIEW_GAP.encode_utf16().count();
        }
        out.extend(chars[ws..we].iter());
        for &(hs, he) in hits {
            if hs >= ws && he <= we {
                let start = base + utf16_len(&chars[ws..hs]);
                ranges.push((start, start + utf16_len(&chars[hs..he])));
            }
        }
        base += utf16_len(&chars[ws..we]);
    }
    (out, ranges)
}
```

两处写法要点：`PREVIEW_GAP` 的码元数就是它的 char 数（3），但代码里仍走 `utf16_len` 而**不写死 3**——将来换成别的分隔串时不会悄悄错；`out.extend(chars[ws..we].iter())` 依赖 `Extend<char> for String` 对 `&char` 成立（`impl Extend<&'a char> for String`），若编译器不接就写 `.iter().copied()`，两种都允许，以编译过为准。

Run: `cd src-tauri && cargo test --lib doc_preview; echo exit=$?`
Expected: `14 passed; 0 failed`。

Run: `cd src-tauri && cargo test --lib; echo exit=$?`
Expected: `134 passed; 0 failed`（120 + 14）。

- [ ] **Step 4: 变异取证**

1. `hit_ranges` 的 needle 改成 `query.trim()`（不经 `query_terms`）：`punctuation_in_the_query_is_dropped_by_the_shared_tokenizer` 必须红。
2. `lower_first` 改成 `src.to_lowercase()`（整串折叠）：`a_bmp_outside_char_before_the_hit_costs_two_units` 或长度不变式用例红（`ß` 那种展开会漂偏移；本仓夹具没有 ß，所以这条**预期是假绿**，按已知缺口登记，不要为了让它红而篡改断言）。
3. `take_windows` 的并窗判据从 `lo <= prev.1` 改成 `lo < prev.1`（相接不并）：`near_hits_share_one_window_so_no_gap_is_emitted` 必须红。它的夹具是按半宽算的（第二处命中 `start = 8002`，左沿 `8002 - 4000 = 4002` 恰等于第一扇窗右沿），所以翻成 `<` 就变成两扇相接的窗 → `stitch` 多插一个 3 码元的 `PREVIEW_GAP` → 第二处命中的起点被推到 8005。**首个红点是 `assert_eq!(p.ranges, vec![(0, 2), (8_002, 8_004)])` 这一条**（`assert_eq!` 失败即中止本用例，后面的 `!p.truncated`、`!p.text.contains(PREVIEW_GAP)`、`p.text == text` 在同一次运行里不会执行到，但它们在这个变异下各自也成立地失败 —— 取证时照实写「红点一处、其后三条被短路挡住」，**不许**为了凑「两条都红」去调断言顺序或拆条）。**并且这条夹具还兼任第二格取证**：把判据里的 `lo` 换回命中起点 `start`（就是 Important-1 那个缺陷形态，`8002 <= 4002` 为假 → 两扇**重叠**窗、正文吐两遍）也必须红，红点同上。这一格的可信度论证（Task 5 复审具名风险，已 SETTLED）：只要多出一扇窗，`stitch` 的 `base` 就多算一次分隔串，其后每个命中的起点位移至少 3 码元，所以 `ranges` 是「多插分隔串（进而重复正文）」的忠实代理，不存在「ranges 逐字相同而 `text` 变脏」的变异。还原后 `cargo test --lib doc_preview` 恢复 `14 passed`。**这条在旧夹具（`"甲验收乙"`，单命中）下逻辑上不可能满足** —— 并窗分支根本没执行，改判据全量照绿，那是等价变异而不是「这条变异不成立」。
4. `stitch` 里删掉 `if i > 0 { push_str(PREVIEW_GAP) }`：`four_spaced_hits_give_three_windows_and_the_fourth_is_dropped` 的 gap 计数断言必须红。
5. `render_preview_with` 去掉 `panic_to_err(...).and_then(|r| r)` 直接调 `extract_with(&path)`：`a_panicking_extractor_is_caught_not_fatal` 必须红。**红的形态**（Task 5 首轮评审核实过，原句「让整个测试进程崩掉」在本机不成立）：dev profile 是 `panic = "unwind"`，而 libtest 给每条测试线程自带 `catch_unwind`，所以现象是**该用例 FAILED + `cargo test` 以 101 退出，测试进程本身不崩**；报告要把形态照实写。生产语义仍然成立 —— IPC 命令线程没有 harness，同一处 panic 就是真终止整个应用，那正是这道边界存在的全部理由。改回来后恢复 `14 passed`。
6. `truncated` 写成 `false`：`a_file_name_only_match...` 与 `four_spaced_hits...` 都必须红。

Expected: 变异回滚后 `14 passed; 0 failed`。

- [ ] **Step 5: 两条 grep 门禁（红线与「不重造读盘」）**

```bash
cd src-tauri && grep -rn "File::open\|std::fs::read\|read_to_string" src/doc_preview.rs; echo exit=$?
grep -rn "\.cut(\|cut_for_search" src/doc_preview.rs; echo exit=$?
```

Expected: 两条都 `exit=1`（0 命中）。第一条守「预览不含自己的读盘路径，红线仍只需 grep `extract.rs`」；第二条守「词只从 `query_terms` 拿」。第一条命中的话先看文件头 —— 注释里出现了那个字面量就是注释写错（Step 1 给的是避开它的写法），**不许**为了让门变绿而把门改成只扫代码行。

- [ ] **Step 6: 两处悬空注释指针更正 + clippy 两道 + 提交**

`src-tauri/Cargo.toml:45-48` 那段（`[profile.release]` 上方的注释）整段替换为：

```toml
# 不许在这里写 panic = "abort"：abort 下 panic 不展开，`extract::pdf_text` 与
# `extract::panic_to_err`（M4 从 index_job 提上来，索引与预览两条路共用）/ worker 线程体里的
# catch_unwind 在发布包里一个都抓不到，一个畸形文件就直接终止整个进程。由
# `extract::tests::release_profile_does_not_abort_so_panic_guards_work` 钉住（终审 C1）。
```

`src-tauri/src/extract.rs` 里 `release_profile_does_not_abort_so_panic_guards_work` 的 doc 注释，把
`/// \`catch_unwind\`（以及 \`index_job\` 的两道边界）在发布包里**抓不到任何东西**，`
改为
`/// \`catch_unwind\`（以及 \`panic_to_err\` 那道边界与 worker 线程体的最外层 \`catch_unwind\`）在发布包里**抓不到任何东西**，`
—— 搬完之后再写「`index_job` 的两道」就把读者指到一个已经没有边界助手的文件里去了。同一句里
`/// 一个畸形文件直接终止整个应用 —— 而 96 条单测全绿` 的 **`96`** 是 M3 收口时的快照数，本任务之后就是 134，
任何后续里程碑都会再漂一次，所以把数字换成不随条数漂移的说法：`—— 而全仓单测都跑在 dev profile 上，一条都不会红`。

这三处都是**纯注释**，不许顺手动同一函数体内的任何代码；改完 `cargo test --lib` 必须仍是 `134 passed`。

```bash
cd src-tauri && cargo clippy --lib -- -D warnings; echo exit=$?
cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
grep -rn "index_job::panic_to_err" . --include=*.rs --include=*.toml; echo exit=$?
git add src-tauri/src/doc_preview.rs src-tauri/src/lib.rs src-tauri/Cargo.toml src-tauri/src/extract.rs
git commit -m "feat: M4 原文预览：锁外重抽 + panic 边界 + UTF-16 码元高亮区间"
```

Expected: 两条 clippy `exit=0` 且 0 条 warning（`PathBuf` 若最终没用就删掉那一行 import，别留 `unused_imports`）；第三条 grep 是全仓（含 `*.toml`）扫悬空符号，`exit=1`（0 命中）；提交含这四个文件，不多不少。

---

### Task 6: 两条 IPC 命令接线（`search_all` / `doc_preview`）

**Files:**
- Modify: `src-tauri/src/lib.rs`（`search_local`（`:255-259`）之后加 `search_all`；`search_docs`（`:536-543`）之后加 `doc_preview`；`generate_handler!`（`:562-602`）末尾加两条。`mod doc_preview;` 那一行**已由 Task 5 落地**，本任务只核它在位，不要重复加。**`doc_preview` 那一条的属性必须是 `#[tauri::command(async)]`**，`search_all` 保持无参数的 `#[tauri::command]` —— 为什么只给前者、代价是什么、取证口径划到哪一格，全在 Step 1 下面那四段，不要自己重述也不要改成第二种写法。**另外一处同文件注释**：`index_overview` 的出锁段那句「此时别的 IPC 进得来」按 Step 1 里「顺带把一句既有的假话收口」那段换成两句实话 —— 它在本任务的 Files 范围内（`lib.rs`），不是越界）
- Modify: `src-tauri/src/doc_preview.rs`（两件事，都只碰注释与豁免，不碰任何表达式。① **删 Task 5 为「caller 还不存在」开的那块临时豁免**：以 `// 临时豁免，Task 6 落地` 开头的 4 行注释，加上紧跟它的那行 `#![allow(dead_code)]`，共 5 行；**删完后的形态是「文件头注释 → 单个空行 → `use std::path::{Path, PathBuf};`」**（照本计划 Task 5 Step 1 那块模板：`#![allow(dead_code)]` 与它下面那行 `use` 之间只有 1 个空行，豁免块上方也只有 1 个）。Task 6 首轮实现按「前后各留一个空行」逐字执行，结果留下了 2 个连续空行（`doc_preview.rs:9-10`）—— 那句话是本计划自己的措辞缺陷，已在此更正，别再照旧句执行。② **同步 `PREVIEW_WINDOW_CHARS` 的 doc 注释**（Task 5 落地时是 3 行，行号会漂，按「紧贴 `const PREVIEW_WINDOW_CHARS: usize` 的那段 `///`」定位）：并窗判据在 fix round 1 从「命中起点」换成了「新窗左沿」，合并带随之翻倍，旧句「单窗最长 `2 * PREVIEW_WINDOW_CHARS + 命中自身长度`」变成了假话（链式合并能让一扇窗接近整篇）。整段换成本计划 Task 5 Step 1 里那份 6 行的写法，逐字照抄，不要自己重述。除这两处之外的任何字节都不许变，尤其不许动文件头那条红线注释的语义。）
- Modify: `src-tauri/src/search.rs`（**删掉 Task 4 为「caller 还不存在」开的那行临时豁免**：`unified_bundle` 上方的 `#[allow(dead_code)]`（Task 4 落地时在 `:290`，行号会漂，按「紧贴 `pub fn unified_bundle` 的那行 `#[allow(dead_code)]`」定位）。它的注释里写死了「第一个 caller 在 Task 6 的 `search_all` IPC，该任务落地时必须删掉本行」——本任务就是那个落地点，不删就永久化（M3 的豁免账教训：临时豁免必须有具名回收点与一条 grep 门）。除删这行和 Step 4 点名的**注释/断言**（Step 4 共六条，落在本文件的是第 1、2、3、4、6 条；第 6 条是断言不是注释）之外，不许动 `search.rs` 的任何表达式。）
- Modify: `src-tauri/src/index_store.rs`（**只改注释**：`query_guards_and_limit_pass_straight_through` 的头注释里那条「Task 10 的 IPC 写 `limit.unwrap_or(50).clamp(1, 200)`」旧指针，落点在 `:671` 附近，按注释文本定位；见 Step 4）
- Modify: `src-tauri/src/extract.rs`（**只改一条断言消息字符串**，第 595-605 行那段 `release_profile_does_not_abort_so_panic_guards_work` 里的 `assert_ne!` 尾句：`见本文件 \`pdf_text\` 的注释与 \`crate::panic_to_err\`（终审 C1）。` 里的 `crate::panic_to_err` 是 `f72a5c8` 把边界助手从 `index_job` 搬进 `extract` 之后悬空的第三个指针（Task 5 已收掉 `Cargo.toml:46` 与同函数 doc 那两处，这一条是 Task 5 复审登记的越界残留——本任务被禁止碰 `extract.rs` 除 `:577` 之外的字节，所以落在拥有 `extract.rs` 的这里）。改为 `见本文件 \`pdf_text\` 的注释与 \`panic_to_err\`（终审 C1）。`。**纯字符串**，不许动同一函数体里的任何表达式，也不许动 `:577` 附近 Task 5 已改好的那句。）

M4 在这一格只有两个 `#[allow(dead_code)]`（`search.rs` 一处、`doc_preview.rs` 一处），两块豁免都由本任务回收，Step 3 的门一次扫两个文件 —— 少收一个就是永久豁免。

**Interfaces:**
- Consumes：Task 4 的 `search::unified_bundle`、Task 5 的 `doc_preview::{row_for_preview, render_preview, DocPreview}`、既有 `db(&state)`（`lib.rs` 内取锁的助手，毒锁回 `db_poisoned`）。
- Produces：前端可调用的命令名 `search_all`（入参 `{ query }`）与 `doc_preview`（入参 `{ docId, query }`，camelCase → snake_case 由 Tauri 映射，`removeDir`/`ledgerList` 已是先例）。

- [ ] **Step 1: 加两条命令**

先确认 `src-tauri/src/lib.rs` 的 `mod` 区里 `mod doc_preview;` 已在位（Task 5 Step 1 落的地），本任务不重复加它。以下这一格才是本步的产出：

```rust
/// 首屏统一检索：一次 IPC 带回三段与三个截断计数。这一格只持锁查库，不碰磁盘。
#[tauri::command]
fn search_all(state: State<'_, AppState>, query: String) -> AppResult<search::SearchBundle> {
    let conn = db(&state)?;
    search::unified_bundle(&conn, &query)
}

/// 原文预览：**锁内查库、锁外抽盘**，所以必须分两步。
/// 握着 `state.conn` 去读盘会让别的命令等一次慢 IO（网络盘上一个大文件就是秒级卡顿，而首屏正是
/// 最容易连点的地方），所以这里刻意不写成 `let conn = db(&state)?; preview(&conn, ..)`。
/// 光出锁还不够：`async` 那一行把命令体挪出 IPC 所在线程，见下面那段「为什么必须有 async」。
#[tauri::command(async)]
fn doc_preview(
    state: State<'_, AppState>,
    doc_id: String,
    query: String,
) -> AppResult<doc_preview::DocPreview> {
    let row = {
        let conn = db(&state)?;
        doc_preview::row_for_preview(&conn, &doc_id)?
    };
    doc_preview::render_preview(&doc_id, &row, &query)
}
```

**为什么 `doc_preview` 必须有 `async` 而 `search_all` 本轮没有（Task 6 首轮复审的 Important-1，三条机制断言都由控制方到 vendored 源码逐行复核过，不是转述评审）**：

1. `tauri-macros-2.7.0/src/command/wrapper.rs:50` —— `execution_context` 的默认值是 `Blocking`；`:116`/`:159` 只有写了 `async` 才变 `Async`。`body_blocking`（`:398-431`）生成的代码是 `let result = $path(...); kind.block(result, resolver); return true;` —— **命令体就地在那个线程里跑完**，没有换线程。
2. `wry-0.57.0/src/webview2/mod.rs:944-978` —— Windows 上 `attach_ipc_handler` 把回调注册进 `add_WebMessageReceived`，`Ok(request) => ipc_handler(request)` 是**在 COM 事件回调里同步调用**的；`tauri-runtime-wry-2.12.0/src/lib.rs:5289-5307` 的 `create_ipc_handler` 也只是把 request 直接转给 handler，不起线程。所以 sync 形态的 `doc_preview` 占的是消息泵所在线程，期间**没有任何别的 `invoke` 进得来** —— 出锁出得干干净净，别的命令也一样等着，「锁外抽盘」买到的东西被 sync 形态抵消掉。
3. 加了 `async` 且函数本身不是 `async fn` 时，`wrapper.rs:258` 的 `kind` 是 `"sync_threadpool"`，`body_async`（定义在 `:355`，not-tracing 分支的生成体在 `:382-389`，`$path` 那一行是 `:384`）生成 `resolver.respond_async_serialized(async move { let result = $path(...); … })` —— **命令体写在 `async move` 块里面**，不在 `return true` 之前就地执行；而 `tauri-2.12.0/src/ipc/mod.rs:371` 的 `respond_async_serialized_inner` 在 `:375` 就是 `crate::async_runtime::spawn(...)` —— 于是命令体落到 async runtime 的工作线程，IPC 所在线程立刻回去泵下一条消息。对照第 1 条：sync 形态的 `$path(...)` 在 `wrapper.rs:425` 就地调用、`:427` 直接 `kind.block(result, resolver)`，同一句宏展开在两种形态下落点完全不同。

为什么只给 `doc_preview`：它是本仓**第一条把无上限的磁盘 IO + pdf/docx 解析放进命令体内**的通路（既有重活一律走后台线程，`index_job.rs:383-387` 自己 `thread::Builder::spawn`）。本任务落地后 `generate_handler!` 共 **41 条**（既有 39 条 + `search_all` + `doc_preview`），其中 **40 条 sync、`doc_preview` 是唯一的 async** —— `vault_unlock` 的 Argon2id（百毫秒级）也一样占着那个线程，那是那 40 条成规模的既有口径，不在本轮顺手改。`search_all` 的取数有上限（200 行）且只做 SQL，**本轮保持 sync**，是否也要 `async` 交给 T9 的 5 万行实测说话（见 Task 9 Step 3 新增的那条），照本计划一贯的「拆不拆由实测说话，不由推测说话」。

**本任务查证到的一条机制事实，它决定了「出锁」这件事什么时候才承重**：`AppState.conn` 那把 `Mutex<Connection>` 在全仓只有 `lib.rs:37` 的 `db()` 一处取锁点、38 个调用点全在 `lib.rs` 的 IPC 命令里；唯一的非 IPC 线程 `index_job::start` **自己开一条连接**（`index_job.rs:402`，它的理由写在 `:365-366`），不抢这把锁。所以在给 `doc_preview` 加 `async` 之前，这把锁实际上**无人竞争** —— 慢 IO 挡的是消息泵那一个线程，跟锁没关系，「锁外抽盘」对界面延迟的真实收益是 0。加了 `async` 之后它才第一次有牙齿：命令体落到 async runtime 的工作线程，预览读盘期间真会有第二条 `invoke` 来抢这把锁。这也是 Important-1 选「加属性」而不是「只降级注释」的核心理由 —— 出锁与换线程是同一个契约的两半，只做一半等于没做，而两句注释各自都会变成半句假话。

**顺带把一句既有的假话收口（Important-1 的同源事实，零行为改动）**：`lib.rs:481` 那一整行写的是「出锁段：`is_dir()` 可以慢，但此时别的 IPC 进得来。」（去掉行首缩进与 `//`，逐字如此），它写在 M3，而 `index_overview` 是 sync 命令 —— sync 形态下这段时间占的就是消息泵所在线程，别的 IPC **进不来**，那句「进得来」按上面的机制根本不成立。本任务不改 `index_overview` 的形态（它是既有 39 条 sync 命令里的一条，本任务只把 `doc_preview` 这一条换成 async，其余一律不动 —— 时间锚写在上一段，不靠某个「共 N 条」的静态数字），但既然本任务把机制查清了，它就得说实话。整行换为下面这两行，逐字照抄（`index_overview` 的函数体、`{ let conn = … }` 那一块，以及它上面那段以「卡住整条 IPC（Task 10 评审的 Important 1，代价实测过口径，不是推测）」收尾的旧任务号注释都不许动 —— 后者在终审清单里，行号会随本任务增长而漂，所以按文本定位）：

```rust
    // 出锁段：`is_dir()` 可以慢，但已不再排 `Mutex<Connection>`；命令本身仍是 sync，
    // 所以这段时间消息泵还被它占着，别的 IPC 进不来（Task 6 复审 Important-1 核到的口径）。
```

`async` 形态的代价要写明白：**同一类命令的两次调用现在可能乱序返回**（sync 形态下由消息泵天然串行，所以「乱序」这件事以前不可能发生）。首屏的 `seq` 守卫（Task 7 的 `search-order.ts::isNewest`、Task 8 的 store）因此从「防御性写法」变成**承重**的，那两个任务不许把它当成可选优化删掉。`doc_preview` 本身是点一次开一个对话框，乱序不可见。

`mod doc_preview;` 与 `fn doc_preview()` 同名不冲突：模块在类型命名空间、函数在值命名空间，控制方已用 `rustc --edition 2021` 单独验过这种形状能编过。命令名保持 spec §三 定的 `doc_preview`，前端 `invoke("doc_preview")` 与之一致。

**这条属性改动的取证口径**：编译本身就是证据链的一节 —— `State<'_, AppState>` 落进 `spawn` 出去的 future 要求它 `Send`，形状不对就编不过，所以 `cargo test --lib` 绿 = 「这个签名在 async 形态下成立」。但**运行时兑现（预览大文件期间别的 IPC 是否真进得来）在单测里证明不了**，本任务不许声称已验证；它由 T9 真机那条新增断言负责，报告里要在「没能自动验证」清单上点名这一格。

- [ ] **Step 2: `generate_handler!` 加两条（39 → 41）**

在 `search_docs` 之后：

```rust
            search_docs,
            search_all,
            doc_preview
```

- [ ] **Step 3: 编译与六道门禁**

```bash
cd src-tauri && cargo test --lib; echo exit=$?
cargo clippy --lib -- -D warnings; echo exit=$?
cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
grep -rn "allow(dead_code)" src/search.rs src/doc_preview.rs; echo exit=$?
grep -rn "crate::panic_to_err\|index_job::panic_to_err" . --include=*.rs --include=*.toml; echo exit=$?
grep -n "tauri::command(async)" src/lib.rs; echo exit=$?
```

Expected: `134 passed; 0 failed`（本任务 0 条新单测：命令体只是转发，`unified_bundle` 与 `doc_preview::*` 的逻辑已各自守住）；两条 clippy `exit=0`；第四条 **`exit=1`（0 命中）**——它就是「Task 4 与 Task 5 那两块临时豁免都被本任务回收」的证据，接线之后 `unified_bundle` 与 `doc_preview::*` 都有了真 caller，豁免留着就永远不会有人发现它过期了。两个文件一起扫：只要还剩一处命中，输出里会点名是哪个文件，别把它读成「另一个也快了」。这条门的覆盖面**只有这两个文件**，不是全 crate —— 全仓另有一处 `#[allow(dead_code)]` 在 `src/db.rs:30`（M1 给 `open_in_memory` 开的，它的调用点至今全在 `mod tests` 里，`cargo clippy --lib` 不带 `--tests` 时确实没有非测试 caller，所以今天仍然必需）。它**不是本任务的账、也不许本任务顺手删**，归属在账本与 `docs/HANDOFF.md` 的 M4 豁免账里点名记为「永久豁免，或将 `open_in_memory` 开非测试入口时回收」（Task 6 首轮复审 Minor-3：不写明就会有人把「一条 grep 门 = 豁免账收干净」读成全仓成立）。第五条扫**悬空符号指针**（`f72a5c8` 把边界助手搬进 `extract.rs` 后，任何写成 `crate::panic_to_err` 或 `index_job::panic_to_err` 的注释/字符串都指到一个不存在的路径上；正确写法在 `extract.rs` 内部是裸 `panic_to_err`，跨文件是 `crate::extract::panic_to_err`），`exit=1`；Task 5 的门只扫 `index_job::panic_to_err` 这一种形态，所以漏掉了 `extract.rs:603` 那条 `crate::` 前缀的，本任务把两种形态一起封住。第六条是「钉住裁定范围」的门：期望**恰好 1 行**且 `exit=0`，那一行必须紧贴 `fn doc_preview`（`grep -n` 的行号与 `fn doc_preview` 只差一行）。它同时守住两个方向 —— 属性没落 = 0 行 = `exit=1` = 红；给 `search_all`（或任何既有命令）也顺手加上 = 2 行 = 红，因为「本轮只动 `doc_preview`，`search_all` 留给 T9 实测」是裁定而不是随手选择，而这条边界编译器和 clippy 都不会替你看着。

- [ ] **Step 4: Task 4 定向复审留下的六处残留收口（六处全零行为，本任务不新增测试）**

Task 4 的 fix round 1 复审判了「5 条 finding 全 ADDRESSED、无新 Critical/Important」，但留了一批文本级 open 项，本任务收其中**六条**：4 条注释文本（下面 1–4）+ 1 条旧任务号指针（5）+ 1 条测试强度（6，键集合等值断言 —— 它是「M4 不加线格式字段」那条裁定唯一有牙齿的闸）。落点分布：六条里 5 条在本文件、第 5 条在 `index_store.rs`。其余 3 条已在账本里登记为有名缺口（台账侧 `MAX_CLUSTERS_PER_SECTION` 无独立用例、第三级 id 键无守护、M3 遗留的其余旧任务号注释）。逐字替换，**不许顺手改任何表达式或断言逻辑**：

1. `search.rs` 的 `Cluster::hidden` 注释第二行（现为「只相对本次取到的样本，见 `DOCS_FETCH_LIMIT` 的头注释。」）改为点名两个上游：

```rust
    /// 簇内被 `MAX_ITEMS_PER_CLUSTER` 截掉的条数。只显示、不做展开（§十）。
    /// 只相对本次取到的样本：正文段的上游是 `DOCS_FETCH_LIMIT`，台账段的上游是 `PER_GROUP_LIMIT`。
```

2. 同文件 `ClusterSection::hidden_clusters` 注释第二行同样改为那句两个上游的写法（首行「被 `MAX_CLUSTERS_PER_SECTION` 整簇扔掉的簇数。」不动）。

3. 同文件 `SearchBundle::projects_hidden` 的注释（现两行主语横跨平铺段与台账段）改为只说本段、并给出它自己的上游：

```rust
    /// 平铺段被截掉的条数。它与台账段一样只相对本次取到的样本，
    /// 上游是 `PER_GROUP_LIMIT`（每组取数上限），所以低报的方向与正文段相同。
    pub projects_hidden: usize,
```

4. 同文件 `cluster_by_project` 的头注释里「第二级项目名」那两句**必须标出变异形态**（复审指出：「确定性红」只对反转方向成立，删行形态下连 `section_drops...` 也只是约 20/21 概率红）。整段替换为：

```rust
/// 段内按项目聚簇。簇间序 = 命中条数降序 → 项目名升序 → 项目 id 升序。
/// 第三级兜底不是因为库里会有两个同项目（`projects.id` 是主键），而是为了「簇序可复现」：
/// 前两级在库里**不唯一**（name 无 UNIQUE，条数更是常并列）。
///
/// 三键的守护现状（Task 4 两轮实测，别把它读成「三级都有测试」）：
/// - 第一级条数：`doc_clusters_sort_by_hit_count_then_project_name` 里 Gamma 那 5 条守住；
/// - 第二级项目名：**反转比较方向**时两条测试都确定性红 —— `section_drops_the_21st_cluster_and_reports_it`
///   （21 个等条数簇，断「名序最大者被扔掉」）与 `doc_clusters_sort_by_hit_count_then_project_name`
///   （并列对翻反）。**删掉这一行**则两条都只剩概率性红：`project::create_project` 的 id 是随机
///   UUID v4（`project.rs:110`），并列簇改按 id 升序，首轮实测 10 次只红 2 次 —— 所以本仓的排序键
///   变异取证一律用反转形态；
/// - 第三级 id：**当前没有任何测试能把它打红**（删掉它实测 120 条全绿）。要它生效得有两个同名且
///   同条数的簇，而测试没法把随机 id 的「插入序 vs id 序」摆成固定先后，`sort_by` 又是稳定排序。
///   有名缺口，交 M5（补法要么给 `projects.id` 开测试注入点，要么在测试里用裸 SQL 固定 id）。
```

5. `src-tauri/src/index_store.rs` 里那条测试注释（上面 Files 点名的 `:671` 附近）把旧任务号换成现名，顺带把它里面那两个裸字面量形态去掉（本任务的 Step 3 门只扫 `src/search.rs`，但留着它们，将来谁把门面扩到 `index_store.rs` 就会无预警红）：

```rust
    /// limit 那三条钉的是**现状**不是意图：SQLite 里负数 LIMIT = 不限行、0 = 无行，
    /// `doc_hits` 不做二次校验，clamp 归调用方边界（`lib.rs:542` 的 `search_docs`，
    /// 首屏那条走 `search::DOCS_FETCH_LIMIT`）。
```

6. 同文件 `bundle_wire_format_is_camel_case` 里，紧跟 `assert!(!obj.contains_key("projects_hidden"));` 之后补一句键集合等值断言（复审指出这条测试是 presence-only，加第八个 camelCase 字段不会红；而 I-2 的裁定恰恰是不加，所以得有个东西真锁住集合大小）：

```rust
        assert_eq!(obj.len(), 7, "bundle 的键集合就是 spec §三 那七个，多一个都要红");
```

补完之后本任务**仍然**是 `134 passed; 0 failed`、`search::tests` 仍 20 条（加的是断言不是测试）。证伪取证一条即可：临时往 `SearchBundle` 加一个 `pub docs_capped: bool`（值随便给 `false`），这条必须红，然后**删干净**——这条同时是给「M4 不加线格式字段」这个裁定上的一道闸。

- [ ] **Step 5: 两侧字面量对账（tsc 与 rustc 都抓不到拼错的那一类）**

```bash
cd src-tauri && grep -n "search_all,\|doc_preview$" src/lib.rs | tail -4; echo exit=$?
grep -rn "\"search_all\"\|\"doc_preview\"" ../src/lib/api.ts; echo exit=$?
```

Expected: 第一条列出 handler 里的两行（`mod doc_preview;` 与 `fn doc_preview` 也在文件里，所以用 `tail -4` 只看接线段）；第二条在 Task 7 落地前**必然 `exit=1`**（api.ts 还没有这两个名字），Task 7 之后必须 `exit=0`。这一条 Task 7 要重跑并写进它的 Expected。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/lib.rs src-tauri/src/search.rs src-tauri/src/doc_preview.rs src-tauri/src/index_store.rs src-tauri/src/extract.rs
git commit -m "feat: M4 索引接线：search_all 与 doc_preview 两条 IPC 命令（后者锁内查库、锁外抽盘）"
```

Expected: 提交含这**五**个文件，不多不少（`extract.rs` 是 Step 文件清单里那条断言消息字符串，只有 1 行；报告里 `git show --stat` 要能看出它只有注释级改动）。**fix round 1 是这一次之外的第二个提交，它只含 `lib.rs` 与 `doc_preview.rs` 两个文件**（`async` 属性 + `index_overview` 那两句注释 + 删掉一个多余空行，全部落在本任务 Files 已点名的文件里）；别为了「凑齐五个文件」去动首轮已经落地的另外三个 —— 它们这轮没有待办，改动本身就是越界。

---

### Task 7: 前端类型、api 封装与可测的纯逻辑层

**Files:**
- Modify: `src/types/search.ts`（追加 4 个类型与段标题常量；`SOURCE_LABELS`/`SOURCE_ORDER` 保留，`field_hits` 仍按 source 分组）
- Modify: `src/lib/api.ts`（`searchLocal` 之后加 `searchAll` 与 `docPreview`）
- Create: `src/lib/search-order.ts`
- Create: `tests/search-order.test.ts`（仓库根 `tests/`，**在 `src/` 之外**）
- Modify: `package.json`（加 `test` 脚本）

**Interfaces:**
- Consumes：Task 6 的命令名；既有 `@/types/index` 的 `DocHit`（`docId`/`projectId`/`projectName`/`path`/`snippet`/`matchedBy`/`score`，与 Rust 侧 camelCase 对齐）、`@/types/search` 的 `FieldHit`、`@/lib/ipc` 的 `AppErrorShape`。
- Produces（Task 8 直接引）：
  - `src/types/search.ts`：`Cluster<T>`、`ClusterSection<T>`、`SearchBundle`、`DocPreview`
  - `src/lib/api.ts`：`searchAll(query): Promise<SearchBundle>`、`docPreview(docId, query): Promise<DocPreview>`
  - `src/lib/search-order.ts`：`isNewest`、`SECTION_TITLES`、`SectionView`、`bundleToSections`、`clusterNote`、`emptyStateKind`

- [ ] **Step 1: 先写 `tests/search-order.test.ts`（8 条，跑红）**

> 计数订正（Task 7 派发前预检）：这一节原本写「7 条」，而下面的代码块实际有 8 个 `test(...)`（第 8 个是末尾那条 value-import 门禁），Step 4 的期望也是 `pass 8` —— 差的那一条就是门禁本身，标题把它漏掉了。本仓没有 lint 会替你对这个数，所以标题、块内条数、Step 4 期望三处必须一起是 8（同形前科 = Task 6 的「四处文本残留」编号到 6，坑 63）。

```ts
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
```

Run: `node --test "tests/**/*.test.ts"; echo exit=$?`
Expected: `exit=1`（模块 `../src/lib/search-order.ts` 还不存在）。**不许只看退出码**：glob 空匹配时 node 回的是 `exit=0 / tests 0`，那等于没有门禁。

- [ ] **Step 2: 类型与 api 封装**

`src/types/search.ts` 末尾追加（`SOURCE_LABELS` / `SOURCE_ORDER` 原样保留）：

```ts
// —— M4 统一检索的线格式，与 Rust 侧 search.rs / doc_preview.rs 逐字段对应（serde camelCase）。
import type { DocHit } from "@/types/index";   // 实际写在文件顶部，这里只是标注出处

export interface Cluster<T> {
  projectId: string;
  projectName: string;
  items: T[];
  /** 簇内被 `MAX_ITEMS_PER_CLUSTER` 截掉的条数：只显示不展开。 */
  hidden: number;
}

export interface ClusterSection<T> {
  clusters: T[];
  /** 整簇被段级上限扔掉的簇数。 */
  hiddenClusters: number;
}

export interface SearchBundle {
  query: string;
  projects: FieldHit[];
  projectsHidden: number;
  ledger: ClusterSection<Cluster<FieldHit>>;
  docs: ClusterSection<Cluster<DocHit>>;
  /** 正文段存在 matchedBy === "prefix"：段级说明只出一行（D7）。 */
  relaxed: boolean;
  /** 有 ok 正文行的项目数：空态文案靠它区分「还没建索引」与「没命中」。 */
  indexedProjects: number;
}

export interface DocPreview {
  docId: string;
  path: string;
  projectName: string;
  /** 窗口拼接后的原文，不是整篇；窗之间以 `⋯` 行分隔。 */
  text: string;
  /** UTF-16 码元区间，后端算好，`text.slice()` 直接可用。 */
  ranges: [number, number][];
  truncated: boolean;
}
```

`src/lib/api.ts` 在 `searchLocal` 之后：

```ts
// 首屏统一检索：三段一次带回。页面切到这个入口是 Task 8 的事，`searchLocal` 在迁移完成前仍被旧调用方用着。
export const searchAll = (query: string) => invoke<SearchBundle>("search_all", { query });
// 点开正文命中：后端锁内查库、锁外重抽原文，返回窗口文本与码元区间。
export const docPreview = (docId: string, query: string) =>
  invoke<DocPreview>("doc_preview", { docId, query });
```

并把 `SearchBundle` / `DocPreview` 加进那行 `import type { FieldHit } from "@/types/search";`。

- [ ] **Step 3: `src/lib/search-order.ts`**

**这一格只 import `SearchBundle`，不许顺手把 `FieldHit` 也带上**（Task 7 派发前预检抓到，控制方用本仓自己的 `npx tsc` 在隔离探针里复现过）：`tsconfig.json` 开着 `noUnusedLocals: true` 且 `include` 只有 `["src"]`，而 `search-order.ts` 的函数签名里没有一处直接写 `FieldHit`（`bundleToSections(bundle: SearchBundle)` 用的是 bundle 上的字段类型，不需要点名 `FieldHit`）—— 一旦带上就是 `error TS6196: 'FieldHit' is declared but never used.`，**Step 4 的 `npm run build` 当场红**。原文这里写的是 `import type { FieldHit, SearchBundle }`，那是计划自己的错，不是实现者该照抄的规格；`src/types/search.ts` 那一格相反，它必须 import `DocHit`（`docs: ClusterSection<Cluster<DocHit>>` 真的用到）。

```ts
// 首屏结果页的纯逻辑：段的存在性、三个截断计数的文案、空态判据、seq 守卫。
// 这一格刻意不 import 任何运行时值（`import type` 会被擦除），这样它能被 `node --test` 直跑，
// 不必给仓库添 jsdom / @testing-library。
import type { SearchBundle } from "@/types/search";

export type SectionKind = "projects" | "ledger" | "docs";

export const SECTION_TITLES: Record<SectionKind, string> = {
  projects: "项目档案",
  ledger: "信息台账",
  docs: "文档正文",
};

export interface SectionView {
  kind: SectionKind;
  title: string;
  /** 该段的截断/放宽说明；没有要说的就 null。 */
  note: string | null;
}

/** 连打两个字会发两次请求，后回来的旧的不能盖新的：带一个自增序号比较就够了。 */
export function isNewest(mine: number, latest: number): boolean {
  return mine === latest;
}

/** 簇内那一行的「还有 N 条」；后端已经截过，这里只负责说实话。 */
export function clusterNote(hidden: number): string | null {
  return hidden > 0 ? `还有 ${hidden} 条未显示` : null;
}

/** 三段的存在性与说明。顺序固定为 项目 → 台账 → 正文；**空段不产出**，但两段的「空」判据不同形：
 *  平铺段看 `projects.length`（有没有行），聚簇段看 `clusters.length`（有没有簇）—— 簇里 `items` 为空
 *  也照样产出，因为那一行的截断说明与放宽说明仍然要说。
 *  三个截断计数各有自己的说法：平铺段用「还有」，聚簇段用「另有」，段级说明里 relaxed 只追加一次。 */
export function bundleToSections(bundle: SearchBundle): SectionView[] {
  const out: SectionView[] = [];
  if (bundle.projects.length > 0) {
    out.push({
      kind: "projects",
      title: SECTION_TITLES.projects,
      note: bundle.projectsHidden > 0 ? `还有 ${bundle.projectsHidden} 个项目未显示` : null,
    });
  }
  if (bundle.ledger.clusters.length > 0) {
    out.push({
      kind: "ledger",
      title: SECTION_TITLES.ledger,
      note:
        bundle.ledger.hiddenClusters > 0 ? `另有 ${bundle.ledger.hiddenClusters} 个项目未显示` : null,
    });
  }
  if (bundle.docs.clusters.length > 0) {
    const bits: string[] = [];
    if (bundle.docs.hiddenClusters > 0) bits.push(`另有 ${bundle.docs.hiddenClusters} 个项目未显示`);
    if (bundle.relaxed) bits.push("含前缀放宽匹配");
    out.push({ kind: "docs", title: SECTION_TITLES.docs, note: bits.length ? bits.join("；") : null });
  }
  return out;
}

export type EmptyState = "results" | "not-indexed" | "no-hit";

/** 三段全空时要说哪一句。`indexedProjects === 0` 与「有索引但没命中」不能同一句话回答
 *  （M3 的 m8 教训：同一句文案盖两种失效，用户就分不清是还没建索引还是搜错了）。 */
export function emptyStateKind(bundle: SearchBundle): EmptyState {
  if (bundleToSections(bundle).length > 0) return "results";
  return bundle.indexedProjects === 0 ? "not-indexed" : "no-hit";
}
```


- [ ] **Step 4: 跑绿并补 `test` 脚本**

`package.json` 的 scripts 加一行（放 `build` 之后）：

```json
    "test": "node --test \"tests/**/*.test.ts\"",
```

```bash
node --test "tests/**/*.test.ts"; echo exit=$?
npm test; echo exit=$?
npm run build; echo exit=$?
grep -rn "\"search_all\"\|\"doc_preview\"" src/lib/api.ts; echo exit=$?
```

Expected: `npm test` 输出 **`pass 8`**、`fail 0`、`exit=0`；`npm run build` `exit=0` —— 但**别把这条门禁读成它守不住的东西**（Task 7 首轮评审 M-1 订正：原文写「tsc 段守类型，`ranges: [number, number][]` 与 Rust 的 `Vec<(usize, usize)>` 线格式对不上就会在这里红」，那是假的 —— `invoke<T>` 是类型断言不是校验，前后端之间没有任何编译期耦合，Rust 侧改字段名时 tsc 一个字都不会报。它真正保证的只有 TS 侧自洽）。跨侧对账靠两件事，都要在报告里逐字段落出来：① Rust 侧既有的 `bundle_wire_format_is_camel_case`（`search.rs` 的测试，`serde_json::to_value` 后断言 bundle 的键集合恰好 7 个）；② 本任务落地后由评审读 `search.rs` / `doc_preview.rs` 的 `#[serde(rename_all = "camelCase")]` 结构体逐字段对 TS 类型。**已知缺口登记**：`DocPreview` 那一侧没有对应的 wire 测试（`doc_preview.rs` 里没有 `serde_json::to_value`），当场 grep 过；给它补一条要把 Rust 条数从 134 抬到 135 并动 Task 5 的模块数与整条链，故不在本任务补，登记为推迟到终审统一处置的有名缺口，中间由 Task 9 真机 `invoke("doc_preview")` 的读数兜住。最后一条 grep `exit=0`（Task 6 Step 4 的两侧字面量对账在这里闭环）。

- [ ] **Step 5: 变异取证**

1. `isNewest` 改成 `mine >= latest`：**这一条的预期是「不红」，不是「必须红」**（Task 7 派发前预检订正：原文这里先写「第一条测试必须红」、同一个括号里又推出「因此这条变异不会红」，两处自相矛盾，实现者照前一句读就会去给测试喂假数据 —— 而那正是本条末尾禁止的事）。为什么不红：`isNewest(3, 3)` 在 `>=` 下仍 `true`，`isNewest(2, 3)` 仍 `false`，而真实调用形态里 `mine` 永远 ≤ `latest`，`>=` 与 `==` 只在 `mine > latest` 才不同 —— 这是**等价变异**，客观上说明「旧请求号大于新号」这一格零守护。可观测的变异是另一个：把 `isNewest` 整个函数体写成 `return true;`，此时 `isNewest(2, 3)` 回 `true`，第一条测试**必须红**。把两个读数都原样记进报告，并把 `>=` 那格按有名缺口登记（T8 的 store 守卫此刻还不存在，`bundleToSections` 的用例守不到它 —— 这条已在 `docs/HANDOFF.md` 的「没能自动验证」清单里）。**不要**为了让 `>=` 红而喂假数据或改断言。
2. `clusterNote(0)` 从 `null` 改成回「还有 0 条未显示」：那条必须红。
3. docs 段说明里删掉 `relaxed` 分支：`relaxed 只在正文段追加一次` 必须红。
4. `emptyStateKind` 的 `indexedProjects === 0` 改成 `indexedProjects < 0`：三态那条必须红。
5. 往 `search-order.ts` 加一行 `import { toAppError } from "@/lib/ipc";`：value import 门禁必须红，然后删掉。
6. **段序交换（I-1 整改的自证，必须做）**：把 `bundleToSections` 里 ledger 与 docs 两个 `if` 块整体换到对方前面。第 3 条**必须红**，且失败点要指名「有内容的段按 项目 / 台账 / 正文 顺序产出；平铺段的存在性看 projects.length」。为什么单列这一条：整改前控制方对**同一处**交换得到的读数是 `pass 8 / fail 0 / exit=0`（全绿），也就是说测试名承诺的段序当时零守护 —— 补上三段同现夹具后必须变成可证伪，否则整改没落地。改回来。

Expected: 变异回滚后 `pass 8 / fail 0`。

- [ ] **Step 6: 提交**

```bash
git add src/types/search.ts src/lib/api.ts src/lib/search-order.ts tests/search-order.test.ts package.json
git commit -m "feat: M4 前端类型与纯逻辑层：bundle 分段、截断说明、空态判据 + node --test"
```

Expected: 提交只含这五个文件（`package-lock.json` 不该动，因为没装新依赖；动了就 `git restore --staged` 它并在报告里说明）。

---

### Task 8: 首屏渲染 —— 三段结果、簇、原文预览对话框

**Files:**
- Modify: `src/stores/local-search.ts`（`hits: FieldHit[]` → `bundle: SearchBundle | null`，保留 `seq` 守卫并改走 `isNewest`）
- Modify: `src/lib/api.ts`（store 迁完之后 `searchLocal` 就零调用方了，本任务把那个 wrapper 删掉；见 Step 1 末）
- Modify: `src/pages/search.tsx`（只留输入框 + debounce + 三种空态 + 组合）
- Create: `src/components/search-bundle.tsx`
- Create: `src/components/doc-preview-dialog.tsx`
- Modify: `src/pages/index-status.tsx`（`:607-672` 的「试搜正文」试验台加一句定位说明）

**Interfaces:**
- Consumes：Task 7 的 `api.searchAll` / `api.docPreview` / `search-order` 全部导出；既有 `@/components/ui/dialog`（`Dialog`/`DialogContent`/`DialogHeader`/`DialogTitle`/`DialogDescription`/`DialogFooter`/`DialogClose`）、`@/components/ui/badge`、`@/lib/api` 的 `revealFolder`（`revealItemInDir`）与 `copyText`（原生剪贴板）、`toAppError`。
- Produces：无下游任务依赖（收口任务）。

**本任务不新增单测**：JSX 渲染不在这张网里（spec §八），由 `npm run build` 的 tsc 段与 Task 9 真机点验负责；因此本任务的报告必须逐条列出「哪些交互没能自动验证」。

- [ ] **Step 1: store 换 bundle**

```ts
import { create } from "zustand";
import * as api from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { isNewest } from "@/lib/search-order";
import type { SearchBundle } from "@/types/search";

interface LocalSearchState {
  bundle: SearchBundle | null;
  searched: boolean;
  busy: boolean;
  error: AppErrorShape | null;
  run: (query: string) => Promise<void>;
  clear: () => void;
}

// 连打两个字会发两次请求，后回来的旧结果不能盖掉新结果，所以带一个自增序号。
let seq = 0;

export const useLocalSearchStore = create<LocalSearchState>((set) => ({
  bundle: null,
  searched: false,
  busy: false,
  error: null,

  run: async (query) => {
    const mine = ++seq;
    set({ busy: true, error: null });
    if (!query.trim()) {
      set({ bundle: null, searched: false, busy: false });
      return;
    }
    try {
      const bundle = await api.searchAll(query);
      if (isNewest(mine, seq)) set({ bundle, searched: true });
    } catch (e) {
      if (isNewest(mine, seq)) set({ error: toAppError(e), bundle: null, searched: true });
    } finally {
      if (isNewest(mine, seq)) set({ busy: false });
    }
  },

  clear: () => {
    seq++;
    set({ bundle: null, searched: false, busy: false, error: null });
  },
}));
```

**（`LocalSearchState.query` 是首轮评审 M-6：写进 store 但全仓没有读取方（页面自己的输入框是 `search.tsx` 里的 `text`，预览对话框用的是 `bundle.query`），所以这一格从定稿里删掉。留着它 tsc 不报、测试不抓，是 D-5 那一形的「写了没人读」版本。）**

**Step 1 还要顺手删掉 `api.ts` 里那个已经零调用方的 wrapper**（裁定见账本 `## Task 8 派发前预检`）：删 `searchLocal` 那一整行（`export const searchLocal = (query: string) => invoke<FieldHit[]>("search_local", { query });`），**并删 `searchAll` 之上那句注释**（「首屏统一检索：三段一次带回。页面切到这个入口是 Task 8 的事…」—— 本任务一落地它就变成假话）。**删完不给 `searchAll` 补新注释**：那一行 `invoke<SearchBundle>("search_all", { query })` 自己说得清，而非-obvious 的那半（锁内查库、锁外抽盘）已经写在紧邻的 `docPreview` 注释里，两处都写就是重复。`api.ts:7` 的类型 import 里 `FieldHit` 只有这一处在用，**必须同时删掉**（留着就是 `noUnusedLocals` 的 `TS6196`，本任务自己的 `npm run build` 当场红 —— 与 Task 7 预检抓到的那处同形）。改完那一行是 `import type { DocPreview, SearchBundle } from "@/types/search";`。**Rust 侧的 `search_local` 命令与它的 handler 注册一个字都不动**（41 条不变，M3 的测试也不碰前端），Task 9 要做两侧对照时直接用 CDP `invoke("search_local", …)`，不需要 TS wrapper。

- [ ] **Step 2: `src/components/doc-preview-dialog.tsx`**

三个动作刻意**不含「用外部程序打开文件」**：`openPath` 会把文件交给外部应用，而那个应用随时可以保存回写——这条工具的立身边界是「只读源文件」，所以只给 `revealItemInDir`（在资源管理器里选中它，不打开）+ 复制路径 + 跳项目详情页。这个取舍写进注释，别让下一个人以为是漏了功能。

```tsx
import { useEffect, useState, type ReactNode } from "react";
import { Link } from "@tanstack/react-router";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import * as api from "@/lib/api";
import { toAppError } from "@/lib/ipc";
import type { DocPreview } from "@/types/search";

interface Props {
  docId: string | null;
  // 簇上的项目 id 由调用方传入：DocPreview 线格式里没有 projectId（Rust 侧只有
  // docId/path/projectName），为了一个跳转按钮去加字段会改掉 Task 4/5 已定稿的线格式
  // 和它们的 10+14 条测试。
  projectId: string | null;
  query: string;
  onOpenChange: (open: boolean) => void;
}

/** 按后端给的码元区间切片上色：`ranges` 就是 `String.prototype.slice` 的下标，零换算。 */
function Highlighted({ text, ranges }: { text: string; ranges: [number, number][] }) {
  const parts: ReactNode[] = [];
  let cursor = 0;
  ranges.forEach(([from, to], i) => {
    if (from > cursor) parts.push(text.slice(cursor, from));
    parts.push(<mark key={i}>{text.slice(from, to)}</mark>);
    cursor = Math.max(cursor, to);
  });
  if (cursor < text.length) parts.push(text.slice(cursor));
  return <pre className="max-h-[55vh] overflow-auto whitespace-pre-wrap break-words rounded-md bg-muted/40 p-3 font-mono text-xs leading-relaxed">{parts}</pre>;
}

export function DocPreviewDialog({ docId, projectId, query, onOpenChange }: Props) {
  // 没有 busy 这一格：定稿的 JSX 里没有任何读取方，加载中由 DialogDescription 的
  // `data === null` 分支说出去。留着就是 noUnusedLocals 的 TS6133，按「谁没被用就删谁」删掉。
  const [data, setData] = useState<DocPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** footer 那两个动作的失败回执，与 `error`（预览本体失败）分开：预览失败会整段不渲染正文，
   *  而「复制路径失败」必须留着正文让用户再试一次。 */
  const [notice, setNotice] = useState<string | null>(null);

  /** 复用 `project-detail.tsx::guard` 的形态：失败要说出来。`copyText` 在窗口失焦时抛（`api.ts`
   *  那句注释自己点名的），`revealFolder` 在文件已被移走时抛（后端专造了 `preview_file_missing`），
   *  两者都不是不可能的边角，静默吞掉就是让用户以为成功。 */
  const guard = async (fn: () => Promise<void>) => {
    try {
      await fn();
      setNotice(null);
    } catch (e) {
      setNotice(`操作失败：${toAppError(e).message}`);
    }
  };

  useEffect(() => {
    if (!docId) {
      setData(null);
      setError(null);
      return;
    }
    let alive = true;
    // 换一份预览前先清空上一份：否则新文件的正文没回来时，界面上是拿旧文档的正文配新标题。
    setData(null);
    setError(null);
    api
      .docPreview(docId, query)
      .then((d) => alive && setData(d))
      // 预览失败只让对话框变红，绝不让整段结果消失（spec §七）。
      .catch((e) => alive && setError(toAppError(e).message));
    return () => {
      alive = false;
    };
  }, [docId, query]);

  return (
    <Dialog open={docId !== null} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle className="font-mono text-sm">{data?.path ?? "原文预览"}</DialogTitle>
          <DialogDescription className="text-xs">
            {data ? `${data.projectName} · 窗口拼接的原文，不是整篇` : "正在从磁盘重抽原文…"}
          </DialogDescription>
        </DialogHeader>
        {error && <p className="text-sm text-destructive">{error}</p>}
        {!error && data && <Highlighted text={data.text} ranges={data.ranges} />}
        {!error && data && (
          <p className="text-xs text-muted-foreground">
            {data.ranges.length === 0
              ? "正文里没有这个词，命中的是文件名。"
              : data.truncated
                ? "只显示命中附近的窗口，中间以 ⋯ 分隔。"
                : "整篇已在上面。"}
          </p>
        )}
        {notice && <p className="text-xs text-destructive">{notice}</p>}
        <DialogFooter className="flex-wrap gap-x-4">
          {data && (
            <>
              <button className="text-xs underline" onClick={() => void guard(() => api.revealFolder(data.path))}>
                在资源管理器中选中
              </button>
              <button className="text-xs underline" onClick={() => void guard(() => api.copyText(data.path))}>
                复制完整路径
              </button>
              {projectId && (
                <Link className="text-xs underline" to="/projects/$projectId" params={{ projectId }}>
                  打开项目
                </Link>
              )}
            </>
          )}
          <button className="text-xs text-muted-foreground" onClick={() => onOpenChange(false)}>
            关闭
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

**为什么 `projectId` 是 prop 而不是 `data` 上的字段**：`DocPreview` 线格式里没有项目 id（Rust 侧只有 `docId`/`path`/`projectName`），为一个跳转按钮去加字段会改掉 Task 4/5 已定稿的线格式和它们的 10+14 条测试；而调用方 `search-bundle.tsx` 本来就站在簇上，`cluster.projectId` 是现成的。`Link` 的写法照 `src/pages/search.tsx:87-93`（本项目路由不传 `search`）。

- [ ] **Step 3: `src/components/search-bundle.tsx`**

```tsx
import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { Badge } from "@/components/ui/badge";
import { DocPreviewDialog } from "@/components/doc-preview-dialog";
import { bundleToSections, clusterNote } from "@/lib/search-order";
import { SOURCE_LABELS, type FieldHit, type SearchBundle } from "@/types/search";
import type { DocHit } from "@/types/index";

function FieldRow({ hit }: { hit: FieldHit }) {
  return (
    <div className="flex items-center gap-2 rounded-md border bg-background px-3 py-2 text-sm">
      <div className="min-w-0 flex-1">
        <p className="truncate">{hit.title}</p>
        {hit.detail && (
          <p className="truncate font-mono text-xs text-muted-foreground" title={hit.detail}>
            {hit.detail}
          </p>
        )}
      </div>
      <Badge variant="outline" className="shrink-0">{SOURCE_LABELS[hit.source]}</Badge>
      <Badge variant="outline" className="shrink-0">{hit.projectName}</Badge>
      <Link
        to="/projects/$projectId"
        params={{ projectId: hit.projectId }}
        className="shrink-0 text-xs underline text-muted-foreground hover:text-foreground"
      >
        打开项目
      </Link>
    </div>
  );
}

/** 正文段的一行：点开才去磁盘重抽原文，所以这里只有路径与摘要。 */
function DocRow({ hit, onOpen }: { hit: DocHit; onOpen: (docId: string) => void }) {
  return (
    <button
      className="rounded-md border bg-background px-3 py-2 text-left text-sm hover:bg-muted/50"
      onClick={() => onOpen(hit.docId)}
    >
      <p className="truncate font-mono text-xs">{hit.path}</p>
      <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">{hit.snippet}</p>
      {hit.matchedBy === "prefix" && <Badge variant="outline" className="mt-1">放宽匹配</Badge>}
    </button>
  );
}

/** 簇头那一行：项目名 + 簇内截断说明。两段的簇头同形，所以共用这一个小组件。 */
function ClusterHead({ name, hidden }: { name: string; hidden: number }) {
  const note = clusterNote(hidden);
  return (
    <p className="px-1 text-xs text-muted-foreground">
      {name}
      {note && <span className="ml-2">{note}</span>}
    </p>
  );
}

export function SearchBundleView({ bundle }: { bundle: SearchBundle }) {
  const [open, setOpen] = useState<{ docId: string; projectId: string } | null>(null);
  const sections = bundleToSections(bundle);

  return (
    <div className="grid gap-4">
      {sections.map((s) => (
        <section key={s.kind} className="grid gap-1.5">
          <h2 className="text-xs font-medium text-muted-foreground">
            {s.title}
            {s.note && <span className="ml-2 font-normal">{s.note}</span>}
          </h2>
          {s.kind === "projects" && bundle.projects.map((h) => <FieldRow key={h.id} hit={h} />)}
          {s.kind === "ledger" &&
            bundle.ledger.clusters.map((c) => (
              <div key={c.projectId} className="grid gap-1">
                <ClusterHead name={c.projectName} hidden={c.hidden} />
                {c.items.map((h) => (
                  <FieldRow key={h.id} hit={h} />
                ))}
              </div>
            ))}
          {s.kind === "docs" &&
            bundle.docs.clusters.map((c) => (
              <div key={c.projectId} className="grid gap-1">
                <ClusterHead name={c.projectName} hidden={c.hidden} />
                {c.items.map((h) => (
                  <DocRow
                    key={h.docId}
                    hit={h}
                    onOpen={(docId) => setOpen({ docId, projectId: c.projectId })}
                  />
                ))}
              </div>
            ))}
        </section>
      ))}
      <DocPreviewDialog
        docId={open?.docId ?? null}
        projectId={open?.projectId ?? null}
        query={bundle.query}
        onOpenChange={(next) => !next && setOpen(null)}
      />
    </div>
  );
}
```

两个聚簇段各自直接读 `bundle.ledger.clusters` 与 `bundle.docs.clusters`，所以 `c.items` 在每个分支里本来就是具体类型（`FieldHit[]` / `DocHit[]`）：**不需要运行时判别、不需要 `as` 转换、也就没有联合类型要收窄**。原先那版把两段收进一个 `ledgerOf(kind)` 里，联合是在那一格造出来的，然后又要靠 `"snippet" in c.items[0]` 拆回去 —— 同一个表达式先制造问题再解决问题，还会把「`items[0]` 会不会越界」变成需要额外论证的东西（簇必然由至少一条命中产生，`cluster_by_project` 按构造保证，界面上到不了空簇；但按分支写就不必论证）。**文案纪律**：段级说明只有 `s.note`（平铺段是「还有 N 个项目未显示」，两个聚簇段是「另有 N 个项目未显示」，正文段还可能是「另有…；含前缀放宽匹配」这种并起来的形态），簇级只有 `clusterNote(c.hidden)`（「还有 N 条未显示」）；两者不许互换，也不许在簇头再补一次段级文案。

- [ ] **Step 4: `src/pages/search.tsx` 收成薄壳**

保留 `DEBOUNCE_MS = 250`、输入框、error 行与 `useEffect(() => clear, [clear])`；把 `group(hits)` 与整段渲染换成 `<SearchBundleView bundle={bundle} />`，空态按 `emptyStateKind` 三句：

```tsx
const { bundle, searched, busy, error, run, clear } = useLocalSearchStore();

{!text.trim() ? (
  <p className="text-sm text-muted-foreground">
    一次搜三处：项目档案、信息台账的明文列，以及已建索引的文档正文。账号密码这些加密列不参与检索，也不出现在结果里。
  </p>
) : busy ? (
  <p className="text-sm text-muted-foreground">检索中…</p>
) : searched && bundle && emptyStateKind(bundle) === "not-indexed" ? (
  <p className="text-sm text-muted-foreground">
    正文还没建索引，去 <Link to="/index" className="underline">/index</Link> 启动一轮；项目档案与台账的明文检索不受影响。
  </p>
) : searched && bundle && emptyStateKind(bundle) === "no-hit" ? (
  <p className="text-sm text-muted-foreground">
    没有命中「{bundle.query}」。只登记过路径的项目不会凭空出现在正文里。
  </p>
) : bundle ? (
  <SearchBundleView bundle={bundle} />
) : null}
```

（`SOURCE_LABELS`/`SOURCE_ORDER`/`FieldHit` 若在本页不再被引，就从这页的 import 里删掉——`SOURCE_ORDER` 现在只有 `search-order.ts` 的分段顺序取代，但 `types/search.ts` 里仍留着它给 `ledger` 段内小徽章用；谁没被用就删谁，别留 `noUnusedLocals` 的红。）

Run: `npm run build; echo exit=$?`
Expected: `exit=0`。若 tsc 在 `search-order.ts` 报未用变量或联合类型错，按上面两段说明收口，不许用 `@ts-ignore` 绕过。

- [ ] **Step 5: `/index` 试验台的定位说明（`src/pages/index-status.tsx:607-672`）**

在那块「试搜正文」的标题下加一行：

```tsx
<p className="text-xs text-muted-foreground">
  这里验的是索引侧召回（两段式与放宽匹配本身）；正式搜索面在首屏「搜索」，走 search_all。
</p>
```

Run: `npm run build; echo exit=$?` 与 `node --test "tests/**/*.test.ts"; echo exit=$?`
Expected: 两条 `exit=0`，node 侧仍 `pass 8 / fail 0`（本任务没动纯逻辑层）。第三条门是 Step 1 那次删除的回收证据：`grep -rn "searchLocal" src/`（注意别用 `tail`/管道包它，读的是它自己的退出码）→ 期望 **exit=1（0 命中）**；还能搜到就说明 store 没迁干净或 wrapper 只删了一半。

- [ ] **Step 6: 提交**

```bash
git add src/stores/local-search.ts src/lib/api.ts src/pages/search.tsx src/components/search-bundle.tsx src/components/doc-preview-dialog.tsx src/pages/index-status.tsx
git commit -m "feat: M4 首屏渲染：三段结果、簇内计数与原文预览对话框"
```

Expected: `git status --porcelain` 之前先看 `git diff --cached --name-only`，**恰好这六个文件**（`package-lock.json` 不该动，因为没装新依赖）。Rust 侧一个字都没改，所以 `cargo test --lib` 仍 134 passed 是**无回归检查**而不是本任务的产出。

---

### Task 9: 真机验收 + D6 的实测决定 + 文档收口

**Files:**
- Modify: `docs/开发进度.md`（M4 段：测试条数、真机结论、没能自动验证的格子）
- 可能 Modify: `src-tauri/src/search.rs` 与 `src/types/search.ts` + `src/lib/search-order.ts`（**仅当**实测推翻了 D6 的 20/10/10，见 Step 3）

**红线（逐字来自项目约定，违反即任务失败）**：
- 真实数据目录 `%APPDATA%\dev.zero.pfm\`（库文件名 `ledger.db`）**只读**。验证前 `stat` 拍 size+mtime 基线，验证后逐文件对上；对不上立刻停手上报。
- **绝不跑 `npm run dev`（纯浏览器里没有 `__TAURI_INTERNALS__`，`invoke` 必失败，那不能证明任何东西），也绝不为验证改 `src-tauri/tauri.conf.json`。**
- 真机一律用一次性 identifier `dev.zero.pfm.m4test`（数据目录由它决定 ⇒ 沙盒库落在 `%APPDATA%\dev.zero.pfm.m4test\`，见 Step 2），**被索引的文件夹具**放 `%TEMP%`；跑完两处都删净。`-c '{...}'` 那段 JSON 要穿 git-bash→npm→Windows 三层引号，碎了就改用仓库外的一次性 override 文件。
- 原生对话框（`revealItemInDir` 触发的资源管理器、`openDialog`）这一类**点不通**（M2 已实测），照实登记为「由人点验」；读/查/改类命令可用 CDP 全程驱动：`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"` + Node 全局 WebSocket 发 `Runtime.evaluate`；React 受控输入要用原型 `value` setter 再派发 `input` 事件。
- 脚本**只回断言结果**（布尔、条数、`matchedBy`、文案前缀、码元区间长度），**绝不打印抽出的正文或任何解密内容**。
- 断言失败是 **finding**，不许绕；数字变了照实报，绝不为凑数改测试。

- [ ] **Step 1: 基线快照（真实目录只读证明）**

```bash
cd "$APPDATA" && ls -la dev.zero.pfm 2>/dev/null && stat -c '%n %s %Y' dev.zero.pfm/* 2>/dev/null
```

把输出的三行（文件名/字节数/mtime）记进报告。若目录不存在（本机从没真跑过），就如实登记「真实库不存在，只读红线本轮无从违反」。

- [ ] **Step 2: 种子一个沙盒库并跑通 6 条断言 + 2 格实测**

（这一节标题原本写「5 条断言」而下面编号到 8 —— 坑 63 在本文件自己身上第四次应验。实际形状是 **6 条断言 + 2 格实测**：第 7、8 条不给通过/失败判据，只给数字与写死的裁定口径，所以不能混进「必须回绿」的清单里。）

用 `verify-tauri-ipc-via-seeded-sandbox-db` 那一套：**沙盒的库不在 `%TEMP%`，而在 `%APPDATA%\dev.zero.pfm.m4test\`** —— 代码里数据目录只来自 `app.path().app_data_dir()`（`lib.rs:578`），而它由 identifier 决定，所以「一次性 identifier `dev.zero.pfm.m4test`」本身就是隔离手段，**不需要也没有**一个「指到 %TEMP% 的数据目录」开关（真要去指就得改 `tauri.conf.json`，那是红线）。`%TEMP%` 放的是**被索引的文件夹具**。步骤：%TEMP% 下建一次性夹具目录并造 3 个项目各自的正文文件（只读，跑完删）→ 启 `npm run tauri dev` 带一次性 identifier 与 `--remote-debugging-port=9222` → 让应用自建 `%APPDATA%\dev.zero.pfm.m4test\ledger.db`，再由种子脚本往里灌 3 个项目、每个项目 1 条正文命中的 `index_docs` + FTS 行（正文取自 `tokenize::index_text` 的口径，别手拼空格串；灌库用 python `sqlite3` 或 node 26 自带的 `node:sqlite` 都行，**别为此加依赖**）→ CDP 发 `invoke("search_all", {query:"验收"})`。

必须回绿的断言（只回布尔与条数）：
1. `search_all` 回的对象有 `projectsHidden`/`indexedProjects`/`relaxed` 三个键（线格式在真机侧闭环，补 Task 4 那条 serde 断言只能守 Rust 单测的缺口）。
2. 三段中至少「正文段」非空，且 `docs.clusters[0].items[0].snippet` 里**没有**分词空格残留（守 Task 3 的折叠在真数据上同样生效）。
3. `invoke("doc_preview", {docId: <上面那条的 docId>, query: "验收"})` 回 `Ok`，`ranges.length >= 1`，且 `text.slice(...ranges[0])` 在页面里等于「验收」（守码元契约——这一条是整条链上唯一能证明前端 `slice` 对齐的证据）。
4. 预览一个 `index_status='skipped'` 的行回 `code === "preview_unavailable"`。
5. 预览一个文件已被删除的行回 `code === "preview_file_missing"`（在沙盒 tempdir 里造的文件，删掉它，不碰真实目录）。
6. 连打两个字（先「验」再「验收」）后页面显示的是后一次的结果（守 seq 守卫在真 UI 上闭环；用 CDP 逐字符派发 input 事件）。
7. **预览载荷长度（Task 5 复审 Minor-1 的有名缺口，实测而非断言）**：在沙盒 tempdir 里造一份长文（≥ 4 万 char，命中每隔约 8000 char 一个，间距刻意落在合并带 `(4000, 8002]` 内），`invoke("doc_preview", …)` 后**只回三个数字**：`text` 的码元长度、`ranges.len()`、`truncated`。并窗判据在 fix round 1 换成了「新窗左沿」，合并带随之翻倍，`MAX_PREVIEW_WINDOWS` 只封顶窗数不封顶窗长，所以链式合并会把一扇窗拉到接近整篇 —— 这是裁定过并接受的行为（要断开就得让窗相接，那正是 Important-1 的重复段落失效形态），但对话框是否还读得动只有真机看得见。**判据写死**：若单次预览的 `text` 超过 6 万码元且页面渲染明显卡顿或滚动失效，把它登记为终审的独立 finding（讨论「给合并加长度上限 + 上限处宁可多插一个 `⋯`」这条备选路），不在本任务里顺手改算术；不卡就在 `docs/开发进度.md` 记数字收口。
8. **`async` 的运行时兑现（Task 6 复审 Important-1 裁的那条，全链条上唯一的证据就在这一格）**：光有第 3 条只证明预览能回，不证明它**不占消息泵**。做法：沙盒 tempdir 里造一份足以让抽取慢到肉眼可辨的文件（几百 MB 级纯文本，或页数很多的 PDF，反正只读、跑完删），在页面里**同一时刻**并发发两条：`invoke("doc_preview", {慢的那份})` 与一条便宜的 `invoke("db_status")`，只回两个数字 —— 各自的墙钟毫秒与「`db_status` 是否先返回」。**判据写死**：`db_status` 先回 ⇒ `async` 生效（sync 形态下它必然排在慢命令后面，因为整条 IPC 都在消息泵那一个线程上）；`db_status` 等到 `doc_preview` 之后才回 ⇒ 属性没起作用，登记为终审的独立 finding 并附两个毫秒数。**第三种读数同样是结论，必须记下来，不许当成「这一格没跑出来」跳过**：任何一条**始终不返回**（超时也不回）⇒ 那说明命令体在 spawn 出去的 future 里 panic 或挂死了，而 async 形态下这**不会**终止进程、也**不会**给界面任何提示（`ipc/mod.rs:375-388` 的 `return_result` 排在 `task.await` 之后，panic 时走不到它；本仓没有 `panic::set_hook`）—— 这一格正是 `doc_preview.rs` 那句 panic 边界注释唯一的真机反证，遇到就按终审独立 finding 登记并写清是哪一条没回。这条同时是 `lib.rs` 里 `doc_preview` 那段注释（「锁外抽盘」+「`async` 那一行把命令体挪出 IPC 所在线程」）唯一的真机对账，跑不通就不许在收口文档里说它成立。

- [ ] **Step 3: D6 的实测决定（20/10/10 是不是合适）**

沙盒里灌一份**贴近真实规模**的数据（5 万行 `index_docs`，正文列填 1～2 KB 的随机 CJK 串，全在 `%TEMP%`），然后量两件事，只回数字：
- `search_all` 一次往返的墙钟毫秒（首屏的体验线）。
- 命中分布：有多少个簇的 `items.len()` 触到 10、多少个段触到 20 簇。

裁定口径写死在这里，不用再问人：**若 5 万行下一次往返 > 800 ms，把 `DOCS_FETCH_LIMIT` 降到 100 并在 `docs/开发进度.md` 登记「取数上限由 200 降到 100，因为 X ms」；若「触顶簇」占比 > 30%，说明 `MAX_ITEMS_PER_CLUSTER = 10` 太小，把三个数一起上调（20/10/10 → 30/15/15）并同步改 Task 4 的三条截断测试期望值与 spec §四。** 两个都没触到就维持原值，只在文档里记数字。**不许**为了「看起来更快」而偷偷放宽 `truncated`/`hidden` 的语义。

顺带记第三个数字（Task 6 的 Important-1 把这条列成了 `search_all` 的待裁定项，本轮它保持 sync）：**`search_all` 往返期间并发那条便宜的 `db_status`，它是否被占住**（口径同 Step 2 第 8 条）。若 `search_all` 本身已 > 800 ms 且 `db_status` 被推到它后面，那说明首屏每次键入都在冻消息泵，`search_all` 也该加 `#[tauri::command(async)]` —— 但**不在本任务改代码**：把两个毫秒数与结论写进 `docs/开发进度.md`，作为终审那轮的独立 finding 落地（终审只有一次 fix 派发，正好把它和别的整改一起做）。

- [ ] **Step 4: 清场 + 文档收口**

```bash
# 1) 关掉 dev 进程；2) 删一次性沙盒：`%APPDATA%\dev.zero.pfm.m4test\`（沙盒库，Step 2 说的就是它）
#    + %TEMP% 下本轮建的夹具目录与 override 文件；3) 复查真实目录逐文件与 Step 1 的基线一致
cd "$APPDATA" && stat -c '%n %s %Y' dev.zero.pfm/* 2>/dev/null
git status --porcelain   # 必须只剩 M4 的代码与文档，没有沙盒残留、没有新增依赖
```

`docs/开发进度.md` 的 M4 段落按既有格式写：终态测试条数（**cargo 134 / node pass 8**）、两条命令名、三个上限的实测裁定结果、以及这张「没能自动验证」清单（逐条点名，不许省略）：
- JSX 渲染正确性（只有 tsc + 真机点验，`node --test` 不覆盖）
- 原生对话框/资源管理器那一步（`revealItemInDir` 由人点验）
- `isNewest` 的 `>=` 变异假绿（Task 7 Step 5 第 1 条登记的有名缺口）
- `lower_first` 折叠展开型映射（ß/İ）导致少认一次命中的分支（本机无夹具）
- 纯静默拒绝 WAL 那条分支仍是无测试守护的 deferred minor（M0 起就记着）
- 5 万行下的检索延迟只有沙盒数字，不代表用户真实目录
- `#[tauri::command(async)]` 的运行时兑现（Task 6 只在编译侧证明了这个签名在 async 形态下成立；「预览期间别的 IPC 进得来」唯一证据是上面 Step 2 第 8 条，134 条单测一条都不覆盖它）

```bash
git add docs/开发进度.md
git commit -m "docs: M4 收口：真机断言、上限实测裁定与未能自动验证的格子"
```

---

## 完成判据（可核对）

- `cd src-tauri && cargo test --lib` → `134 passed; 0 failed`（分模块：tokenize 9 / extract 15 / index_job 12 / index_store 15 / search 20 / doc_preview 14 / db 8 / index_scan 10 / ledger 12 / project 10 / vault 9）
- `node --test "tests/**/*.test.ts"` → `pass 8 / fail 0 / exit=0`
- `npm test`、`npm run build` 两条 `exit=0`
- `cargo clippy --lib -- -D warnings` 与 `cargo clippy --lib --all-targets -- -D warnings` 四条全部 `exit=0`
- 结构门禁**一律以各任务 Step 里写的那几条为准，不在这里重抄一份**（重抄必然漂 —— 本轮实测：这一度写着单前缀 `index_job::panic_to_err`，而 Task 6 Step 3 早已扩成两形态，还少了三条）。当场逐条跑的清单是**八条**，出处分别标在括号里：
  `grep -rn "chars().count() > 128" src/` → exit=1（T1）；`grep -rn "crate::panic_to_err\|index_job::panic_to_err" . --include=*.rs --include=*.toml` → exit=1（T6 Step 3 第五条，**两种前缀都要扫**）；`grep -rn "\.cut(" src/tokenize.rs` → 恰好 1 行（T1）；`grep -rn "File::open" src/doc_preview.rs` → exit=1（T5）；`grep -rn "\.cut(\|cut_for_search" src/doc_preview.rs` → exit=1（T5）；`grep -rn "allow(dead_code)" src/search.rs src/doc_preview.rs` → exit=1（T6 Step 3，**只扫这两个文件**，全仓还剩 `db.rs:30` 那处永久豁免）；`grep -n "tauri::command(async)" src/lib.rs` → 恰好 1 行且 exit=0（T6 钉裁定范围）；再加 T8 的删除门 `grep -rn "searchLocal" src/` → exit=1。
- `generate_handler!` 条数 = **41**（39 + `search_all` + `doc_preview`；`sed -n '589,629p' src-tauri/src/lib.rs | grep -cP '^\s*[a-z_]+(,|$)'` 现数，别背）
- **每任务 1–2 个提交**（走过 fix round 的 T3/T4/T5/T6/T7/T8 各两个），每个只含该任务 Files 里点名的文件；`git status --porcelain` 干净
- `%APPDATA%\dev.zero.pfm\` 逐文件与真机基线快照一致；`%TEMP%` 无本轮残留

---

## 本轮不做（留给后续里程碑，逐条有名）

- 结果分页/展开：三个截断计数只显示不展开（D6/D7）。要展开得再发一次带 offset 的 IPC。
- 失效核对（reconcile）：`preview_file_missing` 只翻译码，不写回 `missing` 状态 ⇒ M5。
- `/index` 页 `scanning` 文案对「根不存在」与「0 个可索引文件」同一句话（M3 的 m8）：本轮只在**首屏**空态用 `indexedProjects` 解决同类问题，`/index` 那一处不动。
- 路径归一化（M3 的 m5）：D4 把 `path` 从预览入参拿掉之后，它不再是安全前提，退回「同一目录的另一种写法会落成两行」的观感问题，仍归 M5。
- 图片 OCR（`extract_unsupported` 的 `.png` 那一类）：M6。


