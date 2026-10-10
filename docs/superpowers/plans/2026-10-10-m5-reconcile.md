# M5 磁盘追平 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让索引自己追平磁盘 —— 每轮把「库里有、本轮扫不到」的登记标成 `missing` 并点删它的 FTS 行，应用启动时自动跑一轮增量，并收掉预览通路的两格 M4 债务（尺寸门、并发单飞）。

**Architecture:** 不新增依赖、不新增 IPC 命令、不新增线格式字段、不新增迁移。差集回收是一个纯函数（`index_job::reclaim_missing`）加一个行级写入口（`index_store::mark_missing`），挂在 `run_pass` 每个项目队列循环之后；启动自动轮复用现成的 `index_job::start` 线程壳与现成的进度事件；前端只补 `missing` 的三格可见性。

**Tech Stack:** Rust 2021 / Tauri 2.12 / rusqlite 0.40.2（bundled，FTS5）/ jieba-rs 0.11 / React 19 + TS + Tailwind 4 + zustand 5；测试 `cargo test --lib`（内存库 + tempdir）与 `node --test`（既有 8 条，本轮不动）。

**Spec:** `docs/superpowers/specs/2026-10-10-m5-reconcile-design.md` —— 每条判据的理由与被否掉的备选都在那张决策表里（D1–D10），本计划只把它切成可照抄的步骤。派发时两份文件一起给实现者。

## Global Constraints

每个任务的隐含前提，逐条来自 spec，取值一字不改：

- **零新依赖**：`src-tauri/Cargo.toml` 与 `Cargo.lock` 一行不动。`notify` 归 M5.5。
- **零新 IPC 命令**：`generate_handler!` 恒 **41** 条。
- **零新线格式字段**：`SearchBundle` 的 `assert_eq!(obj.len(), 7)` 与 `DocPreview` 的 `assert_eq!(obj.len(), 6)` 两条既有断言必须照旧绿 —— 它们就是这条裁定的闸门。
- **零新迁移**：`TARGET_VERSION` 恒 **4**；`missing` 已在 `index_docs` 的 CHECK 里。
- **红线一**：工具绝不动被索引的原始文件 —— 只登记路径、只读文件。夹具只能落在 `tempfile::tempdir()` 与 `db::open_in_memory()`。
- **红线二**：真实数据目录 `%APPDATA%\dev.zero.pfm\`（`ledger.db`）只读；真机一律用一次性 identifier `dev.zero.pfm.m5test`，跑完删净；**绝不为验证修改 `src-tauri/tauri.conf.json`**（覆盖只走 `npm run tauri -- dev -c '{...}'`）。
- **红线三**：子代理一律不许跑 `tauri dev` / `npm run dev`；「UI 实际渲染未验证」必须如实写进报告。
- **红线四**：密文列不参与任何检索；`ledger_notes.body` 是**明文**列，别当密文列论证。
- **红线五**：验证脚本只回断言结果（布尔 / 条数 / 错误码 / 尺寸），绝不打印抽出的正文或任何解密内容。
- **红线六**：Git 按文件名逐个 `git add`，不 `-A`、不 amend、不 `--no-verify`；**不 push**（push 与 merge 每次都需需求方当次指示）。输出语言一律中文（错误码、SQL 字面量、JSON key 除外）。
- **ponytail**：最懒可行方案 —— 删除优先、复用仓库既有写法、不为假想需求加抽象、能一条断言守住就不开两条。
- 命令的 cwd：`cargo` 系列在 `src-tauri/`，`node --test` 与 `npm run build` 在**仓库根**。
- 退出码要实跑取，别接管道（`cmd; echo exit=$?`）；`cargo test --lib` 只接一个筛选词。
- **行号只作对照，取数只认命令**：本计划的 `文件:行号` 是 2026-10-10 在 `feat/m5-reconcile`（HEAD = spec 定稿那次提交）的读数。
- `skip_reason` 的取值域**不扩**：`missing` 行留 NULL（spec D4）。
- `MISSING_FUSE_MIN_ROWS` 只允许「定义 + 使用」各一次，别在任何地方再写 `20` 字面量。

## 已实测事实（本轮依据，勿重新发现）

| # | 事实 | 证据 | 取数命令 |
|---|---|---|---|
| 1 | 既有全量 134 passed / 11 个模块 | M4 收口（`docs/HANDOFF.md` 门禁基线一节） | `cd src-tauri && cargo test --lib` |
| 2 | `run_pass` 是纯函数（`&Connection` + `&mut dyn FnMut(Progress)`），内存库可全测 | `index_job.rs:140-147` | `grep -n "pub fn run_pass" src-tauri/src/index_job.rs` |
| 3 | 增量判据要求 `index_status = 'ok'` ⇒ `missing` 行天然匹配不上，文件回来必被重抽回同一行 | `index_store.rs:94-109` | `grep -n "fn current_rowid" src-tauri/src/index_store.rs` |
| 4 | `write_doc` 的冲突键是 `UNIQUE (project_id, path)`，BINARY 裸文本比较 | `db.rs:297`、`index_store.rs:52` | `grep -n "UNIQUE (project_id" src-tauri/src/db.rs` |
| 5 | `root_missing` 时 `run_pass` 手里是 `Default::default()` 的空 `outcome`，连 walkdir 都不发起 | `index_job.rs:160-167` | `grep -n "root_missing = " src-tauri/src/index_job.rs` |
| 6 | `capped` 是 `break` 出来的，所以扫到的集合只含配额内前 N 个 | `index_scan.rs:121-124` | `grep -n "out.capped = true" src-tauri/src/index_scan.rs` |
| 7 | `queue` 由 `outcome.files` / `outcome.over_size` 的 `.iter().cloned()` 拼成 ⇒ 队列循环跑完 `outcome` 仍在作用域内 | `index_job.rs:175-182` | `sed -n '175,182p' src-tauri/src/index_job.rs` |
| 8 | 轮末那句 `let _ = sweep_unrooted_projects(conn)?;` 是「丢弃计数」的既有形状，照抄它、别自创 | `index_job.rs:261` | `grep -n "let _ = sweep_unrooted_projects" src-tauri/src/index_job.rs` |
| 9 | `unchecked_transaction(&self)` 收 `&Connection` ⇒ `project.rs` 的公开签名不用改；其 Drop 是 `DropBehavior::Rollback` ⇒ 中途 Err 会 ROLLBACK | vendored `rusqlite-0.40.2/src/transaction.rs:243/:479`，`new_unchecked` 里 `drop_behavior: DropBehavior::Rollback`，`finish_()` → `rollback_()` | `grep -n "fn unchecked_transaction" ~/.cargo/registry/src/*/rusqlite-0.40.2/src/transaction.rs` |
| 10 | 任何 rusqlite 错误都映射成 `db_failed` | `error.rs:42-50` | `grep -n "db_failed" src-tauri/src/error.rs` |
| 11 | `project_dirs` 有 `UNIQUE (project_id, path)` ⇒ 同路径的 entry 行能让 root 的 INSERT 当场失败（Task 5 用它做回滚探针） | `db.rs:175` | `grep -n "UNIQUE (project_id" src-tauri/src/db.rs` |
| 12 | `row_for_preview` 与 `SELECT_SQL` 都是**正向** `= 'ok'` ⇒ 标 missing 后预览与召回自动退出，因此不为此新开测试 | `doc_preview.rs:79`、`index_store.rs:242-251` | `grep -rn "index_status = 'ok'" src-tauri/src/` |
| 13 | `fts_orphan_rows` 是 `#[cfg(test)] pub(crate)`，同文件测试可直接用；产品路径不许拿它当检查点（每轮全表扫） | `index_store.rs:131-140` | `grep -n "fn fts_orphan_rows" src-tauri/src/index_store.rs` |
| 14 | `render_preview_with` 被测试模块直接调用十余处 ⇒ 尺寸门放生产入口 `render_preview` 而不是它（否则一次签名改动波及十几处调用点，而那条路刻意不碰磁盘） | `doc_preview.rs:103-114`、`doc_preview.rs:293-299` | `grep -c "render_preview_with(" src-tauri/src/doc_preview.rs` |
| 15 | 库里那行的 `path` 与本轮扫到的串**同源**（都出自 `WalkDir::new(登记的 root 串)`）⇒ 差集比对用原样字符串，不做折叠 | `index_scan.rs:66`、`project.rs:91-102` | `grep -n "entry.path().display()" src-tauri/src/index_scan.rs` |
| 16 | `index_status` 的 CHECK 已含 `missing`；`pending` 全仓无生产写入点 | `db.rs:290-291` | `grep -n "CHECK (index_status" src-tauri/src/db.rs` |
| 17 | `index_overview` 已 `pick("missing")`（值恒 0），前端 `ProjectState` 已有该字段 ⇒ 后端一旦写入，类型零改动 | `lib.rs:440`/`:501`、`src/types/index.ts:5-16` | `grep -n "missing" src-tauri/src/lib.rs src/types/index.ts` |
| 18 | worker 自开连接 ⇒ 自动轮不占 `AppState.conn` 那把锁、不阻塞 IPC | `index_job.rs:365-367`、`:402-409` | `grep -n "db::open" src-tauri/src/index_job.rs` |
| 19 | `PROGRESS_EVERY` 所在的那个常量区就是 `MISSING_FUSE_MIN_ROWS` 该住的家 | `index_job.rs:22-25` | `sed -n '20,26p' src-tauri/src/index_job.rs` |
| 20 | 测试夹具里用假路径字符串（`scanned("C:/已注销/遗留说明.txt")`）是本仓 blessed 的做法，红线只约束「夹具文件落盘的位置」 | `index_job.rs` 测试模块里 `scanned` 助手的头注释（**指针写符号**：Task 4 落地时这一处的原行号 `:853-854` 已漂到别的函数上，是坑 65/67/73 在本轮的又一次应验） | `grep -n "fn scanned" -A 3 src-tauri/src/index_job.rs` |

**分模块条数（改动前后）**

| 模块 | 基线 | M5 终态 | 差 |
|---|---|---|---|
| tokenize | 9 | 9 | 0 |
| extract | 15 | 15 | 0 |
| index_job | 12 | **19** | +7（T2 +2、T3 +4、T4 +1） |
| index_store | 15 | **17** | +2（T1） |
| search | 20 | 20 | 0 |
| doc_preview | 14 | **17** | +3（T6 +1、T7 +2） |
| db | 8 | 8 | 0 |
| index_scan | 10 | 10 | 0 |
| ledger | 12 | 12 | 0 |
| project | 10 | **11** | +1（T5） |
| vault | 9 | 9 | 0 |
| **合计** | **134** | **147** | **+13** |

数模块条数一律从**全量输出**按 `^test <模块>::tests::` 数，别用 `cargo test --lib <模块>` 的聚合行（筛选走子串匹配会把同名巧合算进来）。

## 文件结构

| 文件 | 本轮职责 | 任务 |
|---|---|---|
| `src-tauri/src/index_store.rs` | 新增按项目取行的读入口 + `missing` 的唯一写入口 | T1 |
| `src-tauri/src/index_job.rs` | 新增差集回收纯函数与熔断常量；`run_pass` 里挂一处调用 | T2 / T3 / T4 |
| `src-tauri/src/project.rs` | `set_root_dir` 的 DELETE+INSERT 包事务 | T5 |
| `src-tauri/src/doc_preview.rs` | 生产入口加当前尺寸判门、`unavailable` 带上成因文案、新增单飞闸门类型与 `busy()`、改一句过期 hint | T6 / T7 |
| `src-tauri/src/lib.rs` | 命令里取 `ScanOptions` 并传上限、装 `PreviewShared`、setup 触发自动轮 | T6 / T7 / T8 |
| `src/pages/index-status.tsx` | `missing` 的三格可见性 | T9 |
| `docs/开发进度.md`、`docs/HANDOFF.md`、`docs/技术方案.md` | 收口与回写 | T10 |
| 全程不碰：`src-tauri/Cargo.toml`、`Cargo.lock`、`src-tauri/tauri.conf.json`、`src/types/index.ts`、`tests/**`、`.superpowers/sdd/2026-10-07-m4-unified-search/**` | — | — |

---

### Task 1: `index_store` 的失效读写两格

**Files:**
- Modify: `src-tauri/src/index_store.rs`（两个新函数插在 `clear_project` 之后；测试加进文件末尾 `mod tests`）
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: 既有 `write_doc`、`DocOutcome`、`fts_orphan_rows`（`#[cfg(test)]`）与测试助手 `conn()` / `seed_project()` / `file()`
- Produces: `pub fn doc_rows_for_project(conn: &Connection, project_id: &str) -> AppResult<Vec<(i64, String)>>`、`pub fn mark_missing(conn: &Connection, doc_rowid: i64) -> AppResult<()>`

- [ ] **Step 1: 写两条失败测试**

```rust
    /// 磁盘上没了的一行：主表标 missing、虚表点删，元数据行留着。
    /// 三条断言各有归属：status 是本轮唯一的新语义；`skip_reason` 留 NULL 是 D4（它的取值域
    /// 只有 skipped 那三个）；行数 = 1 是 D3「保留元数据行」的唯一证据 —— 双侧删行也照样能过
    /// 前两条。虚表侧的孤儿与总行数各一条：少了总行数那条，「死正文继续躺在虚表里污染 bm25
    /// 文档频率统计」这个 M3 终审 I3 否掉过的形态就会重新变成正确行为。
    #[test]
    fn mark_missing_flips_the_status_and_drops_only_the_fts_row() {
        let c = conn();
        seed_project(&c, "p1");
        let rowid = write_doc(&c, "p1", &file("C:/x/合同验收.docx"), DocOutcome::Ok("甲方要求验收指标".into())).unwrap();
        assert_eq!(fts_orphan_rows(&c), 0, "前提：写完两侧是对齐的");

        mark_missing(&c, rowid).unwrap();

        let (status, reason): (String, Option<String>) = c
            .query_row(
                "SELECT index_status, skip_reason FROM index_docs WHERE doc_rowid = ?1",
                params![rowid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "missing", "失效行必须改状态，否则它继续进召回");
        assert_eq!(reason, None, "missing 自己就是原因，skip_reason 不许被写新值（D4）");
        let kept: i64 = c
            .query_row("SELECT count(*) FROM index_docs WHERE doc_rowid = ?1", params![rowid], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, 1, "元数据行要留着：界面得能回答「曾被索引、现在磁盘上没了」");
        assert_eq!(fts_orphan_rows(&c), 0, "点删不许留下对不上主表的虚表行");
        let fts_total: i64 = c
            .query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts_total, 0, "死正文继续躺在虚表里就是继续污染 bm25 的文档频率统计");
    }

    /// 已标 missing 的行不再进候选集（幂等 + 熔断分母干净）。两个方向都断：不许含已失效那行，
    /// 也不许丢 skipped 那行 —— 把谓词写成 `= 'ok'` 会把 skipped/failed 漏掉，
    /// 「超限文件后来被删了」这类失效就永远没人标。
    #[test]
    fn doc_rows_for_project_skips_rows_already_marked_missing() {
        let c = conn();
        seed_project(&c, "p1");
        let a = write_doc(&c, "p1", &file("C:/x/甲.docx"), DocOutcome::Ok("甲 付款条件".into())).unwrap();
        let b = write_doc(&c, "p1", &file("C:/x/乙.docx"), DocOutcome::Ok("乙 付款条件".into())).unwrap();
        write_doc(&c, "p1", &file("C:/y/丙.docx"), DocOutcome::Skipped("too_large")).unwrap();
        mark_missing(&c, a).unwrap();

        let rows = doc_rows_for_project(&c, "p1").unwrap();
        assert_eq!(rows.len(), 2, "三行里只有已 missing 的那行该被排除：{rows:?}");
        assert!(rows.iter().any(|(id, _)| *id == b), "没失效的行不许被漏掉");
        assert!(!rows.iter().any(|(id, _)| *id == a), "变异：把 <> 'missing' 删掉 ⇒ 这条红");
        assert_eq!(doc_rows_for_project(&c, "p-不存在").unwrap().len(), 0, "别的项目的行不许串进来");
    }
```

- [ ] **Step 2: 跑红**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -20; echo exit=${PIPESTATUS[0]}`
Expected: 编译失败 `cannot find function `doc_rows_for_project` in this scope`。这是 RED 的正确形态，别当成自己写错。

- [ ] **Step 3: 最小实现**

```rust
/// 差集回收要的读入口：一个项目当前**尚未失效**的行。
/// 排除 `missing` 有两个理由：重复 UPDATE 是空转；而熔断的分母得是「还可能失效的行」，
/// 把历史 missing 行算进去会让第二轮比第一轮更容易触发熔断（那是假保险）。
pub fn doc_rows_for_project(conn: &Connection, project_id: &str) -> AppResult<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT doc_rowid, path FROM index_docs
          WHERE project_id = ?1 AND index_status <> 'missing'",
    )?;
    let rows = stmt
        .query_map(params![project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 「登记还在、文件没了」的唯一写入口（spec D3）。两句话一个事务，形状照 `write_doc`。
/// `skip_reason` / `error_msg` / `size` / `mtime` / `indexed_at` 一律不出现在 SET 子句里：
/// 那是这一行最近一次被抽时的读数，而回填自愈靠的正是 size/mtime 与新值不匹配（D4/D6）。
/// 召回与预览都不需要新代码：`SELECT_SQL` 与 `row_for_preview` 写的都是正向 `= 'ok'`。
pub fn mark_missing(conn: &Connection, doc_rowid: i64) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE index_docs SET index_status = 'missing', updated_at = datetime('now')
          WHERE doc_rowid = ?1",
        params![doc_rowid],
    )?;
    // 按 rowid 点删（本模块头注释已写明：按 doc_id 列删虚表是全表扫）。FTS5 无外键、不参与级联，
    // 所以删一侧就必须显式删另一侧。
    tx.execute("DELETE FROM index_docs_fts WHERE rowid = ?1", params![doc_rowid])?;
    tx.commit()?;
    Ok(())
}
```

> **这两行 `#[allow(dead_code)]` 是计划文本自身的缺陷造成的，2026-10-10 由 Task 1 实现者撞出来后在此定稿**（账本记为 Ruling）：`doc_rows_for_project` 与 `mark_missing` 的**生产**调用点在 Task 2/3 的 `reclaim_missing`，而 `cargo clippy --lib` 不带 `--tests` ⇒ `cfg(test)` 关闭时两个 `pub` 函数在 lib 目标里零调用点，每条会被判 `dead_code`，与本任务 Step 6 要求的「两条 clippy 全绿」互斥。这是 HANDOFF 坑 59 的第二层（「模块声明归下一个任务」的同族）。
> 定稿写法：紧贴两个函数各加一行 `#[allow(dead_code)]`，注释点名「生产调用点在 Task 2/3 的 `reclaim_missing`」，形状照仓内既有先例 `db.rs:30`（`open_in_memory` 同款处境）。**回收是硬要求**：Task 2 Step 3 必须删掉这两行，Task 2 Step 6 有一条 grep 门收口 —— 只写在注释里的「Task 2 记得删」等于没人负责（坑 48）。除这两行之外本任务不许有任何净新增。

- [ ] **Step 4: 跑绿**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -5; echo exit=${PIPESTATUS[0]}`
Expected: `136 passed; 0 failed`（基线 134 + 本任务 2），`index_store` 模块 **17** 条。

- [ ] **Step 5: 变异取证（三条各做一次，读数逐条记进报告）**

1. 删掉 `DELETE FROM index_docs_fts WHERE rowid = ?1` ⇒ Expected：第一条红在 `fts_orphan_rows` 或 `fts_total`。
2. 把 `index_status <> 'missing'` 改成 `index_status = 'ok'` ⇒ Expected：第二条红在 `rows.len()`（`left: 1, right: 2` —— `skipped` 那行被漏掉了）。
3. 在 UPDATE 的 SET 里加 `skip_reason = 'file_gone'` ⇒ Expected：第一条红在 `reason == None`（这是 D4 的专属守护，别省）。

每条还原后 `diff -q` 验字节一致并重跑全量；还原用的备份必须是**本轮编辑之后**的那份（拿开工前的备份回滚会把本轮编辑一起抹掉）。

- [ ] **Step 6: 门禁**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings; echo exit=$?
cd src-tauri && cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
```
Expected: 两条 `exit=0` 且输出里 0 条 warning。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/index_store.rs
git commit -m "feat(m5): index_docs 的失效读写两格（标 missing + 点删 FTS 行）"
```

---

### Task 2: `reclaim_missing` 的差集主路径

**Files:**
- Modify: `src-tauri/src/index_job.rs`（新函数放在 `sweep_unrooted_projects` 之后；测试与两个夹具助手加进 `mod tests`）
- Modify: `src-tauri/src/index_store.rs`（**Task 1 的三处收尾，见 Step 0**：删掉两行 `#[allow(dead_code)]`、并进一条断言、改三处不实注释）
- Test: `index_job.rs` 的 `mod tests`（新用例）+ `index_store.rs` 的 `mod tests`（并进既有用例，**条数不变**）

**Interfaces:**
- Consumes: T1 的 `doc_rows_for_project` / `mark_missing`；既有 `crate::index_scan::ScanOutcome`（字段 `files` / `over_size` / `walk_errors` / `capped`）、测试助手 `scanned(path)` / `doc_rows(conn, pid)` / `seeded_project`
- Produces: `pub fn reclaim_missing(conn: &Connection, project_id: &str, outcome: &ScanOutcome, root_missing: bool) -> AppResult<usize>`（本任务**不带熔断**，T3 再加）、测试助手 `outcome_with(...)` / `missing_rows(...)`

- [ ] **Step 0: Task 1 的三处收尾（由 Task 1 评审判定；注释级 + 断言级零行为回归，不单开返工轮 —— 坑 56）**

1. **并进一条断言，把 D4 的五根禁列一次守住。** 评审实测过：往 `mark_missing` 的 SET 里加 `indexed_at = NULL` 或 `error_msg = 'gone'`，Task 1 那两条新测试**全绿** —— 五列只有 `skip_reason` 有牙齿。修法不是新开一条测试（那会重开 136→137 的整条条数链），而是把 `index_store.rs` 第一条测试里的 `(status, reason)` 那次查询换成「写前 / 写后整行快照对比」：

```rust
        // D4 的五根禁列一次守住：只断 `skip_reason` 时，往 SET 里加 indexed_at = NULL 或
        // error_msg = 'gone' 都抓不到（Task 1 评审实测）。改成元组对比后五列各自承重。
        fn snapshot(c: &Connection, doc_rowid: i64) -> (Option<String>, Option<String>, i64, i64, Option<String>) {
            c.query_row(
                "SELECT skip_reason, error_msg, size, mtime, indexed_at FROM index_docs WHERE doc_rowid = ?1",
                params![doc_rowid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap()
        }
        let before = snapshot(&c, rowid);
        mark_missing(&c, rowid).unwrap();
        assert_eq!(snapshot(&c, rowid), before,
            "mark_missing 只许动 index_status 与 updated_at（spec D4）；变异：SET 里加 indexed_at = NULL ⇒ 这条红");
```

`snapshot` 写成测试模块里的**自由函数**（收 `&Connection` + `doc_rowid`），别写成闭包（借 `c` 之后同体还要用 `c` 会撞 borrow），也别为它开 `pub`。原来的 `assert_eq!(status, "missing", ...)` 那一条**保留不动** —— 它是新语义本身的断言，禁列快照替代不了；被替换掉的只有 `assert_eq!(reason, None, ...)` 那一条（`skip_reason` 已在元组第一位）。**条数一格都不变**：`index_store` 仍 17、全量仍 136（本仓先例：M4 终审批次那两处「并进既有测试体」，形状同为 N→N）。

2. **三处注释不实，只改文字、不动断言**：
   - `mark_missing` 里「本模块头注释已写明：按 `doc_id` 列删虚表是全表扫」—— 控制方已实读 `index_store.rs:1-9`，模块头**没有**这句话，唯一出处是 `db.rs:278`。改指 `db.rs:278`。
   - 第一条测试头注释里「双侧删行也照样能过前两条」不成立：行真被删时取那行的 `query_row(...).unwrap()` 先 panic 于 `QueryEmpty`，测试照红、只是红点前移。改成「双侧删行也照样红，只是先红在取行那一步」。
   - 第二条测试里「把 `<> 'missing'` 删掉 ⇒ 这条红」：实测先红在上一行的 `rows.len()`。改成「先红在 `rows.len()`」。
3. 删掉 Task 1 开在两个函数上方的 `#[allow(dead_code)]` **连同各自的注释行**（下一节的 Step 3 说明为什么此刻必须删）。

- [ ] **Step 1: 先加两个测试助手，再写两条失败测试**

助手加在 `mod tests` 里 `no_cancel()` 附近（T3/T4 继续用）：

```rust
    /// 不经磁盘、直接给 `reclaim_missing` 造本轮集合。`capped` 一并可设，
    /// 这样「配额触顶」那条闸门能被单独触发（T3）。两桶分开传，就是为了能单独验 over_size。
    fn outcome_with(files: &[&str], over_size: &[&str], capped: bool) -> crate::index_scan::ScanOutcome {
        crate::index_scan::ScanOutcome {
            files: files.iter().map(|p| scanned(p)).collect(),
            over_size: over_size.iter().map(|p| scanned(p)).collect(),
            walk_errors: vec![],
            capped,
        }
    }

    /// 库里该项目已标 missing 的行数。T3/T4 靠它断「一格都没动」。
    fn missing_rows(conn: &Connection, project_id: &str) -> i64 {
        conn.query_row(
            "SELECT count(*) FROM index_docs WHERE project_id = ?1 AND index_status = 'missing'",
            [project_id],
            |r| r.get(0),
        )
        .unwrap()
    }
```

测试：

```rust
    /// 本轮扫到 2 条、库里有 3 条 ⇒ 只有「没了.txt」进差集。
    /// 断言分工：`== 1` 断计数；按 rowid 断 status 断「动的是哪一行」；最后两条检索断
    /// 「删的是虚表侧那一半」—— 只数行数的话，`ok` 行还留在 FTS 里、照样搜得到，
    /// 而用户在首屏看到的失效（M3 当年 I3 的原话）就是这件事。
    #[test]
    fn reclaim_marks_rows_the_pass_did_not_see() {
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", Path::new("C:/根"));
        write_doc(&conn, "p1", &scanned("C:/根/留下.txt"), DocOutcome::Ok("甲方要求验收指标".into())).unwrap();
        let gone = write_doc(&conn, "p1", &scanned("C:/根/没了.txt"), DocOutcome::Ok("报价单里写着验收流程".into())).unwrap();
        write_doc(&conn, "p1", &scanned("C:/根/超限.txt"), DocOutcome::Skipped("too_large")).unwrap();

        assert_eq!(doc_rows(&conn, "p1"), 3, "前提：三行都真在库里");
        assert_eq!(index_store::doc_hits(&conn, "验收流程", 20).unwrap().len(), 1,
            "前提：那行正文此刻真搜得到，否则末尾「搜不到」什么都没证明");

        let o = outcome_with(&["C:/根/留下.txt", "C:/根/超限.txt"], &[], false);
        assert_eq!(reclaim_missing(&conn, "p1", &o, false).unwrap(), 1);

        let status: String = conn
            .query_row("SELECT index_status FROM index_docs WHERE doc_rowid = ?1", [gone], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "missing");
        assert_eq!(missing_rows(&conn, "p1"), 1, "只该动差集里那一行");
        assert!(index_store::doc_hits(&conn, "验收流程", 20).unwrap().is_empty(),
            "还搜得到就说明虚表侧没删干净");
        assert_eq!(index_store::doc_hits(&conn, "验收指标", 20).unwrap().len(), 1, "留下那行不许被牵连");
        assert_eq!(index_store::fts_orphan_rows(&conn), 0);
    }

    /// 超限文件本轮落在 `over_size` 桶里（它有一行 too_large）。集合漏掉这一桶，
    /// 就等于把每一个超限文件标成失效 —— 本轮最容易写错的一格，所以单独一条测试守。
    #[test]
    fn an_over_size_row_still_on_disk_is_not_marked_missing() {
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", Path::new("C:/根"));
        write_doc(&conn, "p1", &scanned("C:/根/留下.txt"), DocOutcome::Ok("甲方要求验收指标".into())).unwrap();
        let big = write_doc(&conn, "p1", &scanned("C:/根/大图.txt"), DocOutcome::Skipped("too_large")).unwrap();

        let o = outcome_with(&["C:/根/留下.txt"], &["C:/根/大图.txt"], false);
        assert_eq!(reclaim_missing(&conn, "p1", &o, false).unwrap(), 0, "两桶都算扫到了 ⇒ 差集为空");

        let status: String = conn
            .query_row("SELECT index_status FROM index_docs WHERE doc_rowid = ?1", [big], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "skipped", "变异：构集合时只取 outcome.files ⇒ 这行被标 missing，这条红");
        assert_eq!(missing_rows(&conn, "p1"), 0);
    }
```

- [ ] **Step 2: 跑红**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -20; echo exit=${PIPESTATUS[0]}`
Expected: 编译失败 `cannot find function reclaim_missing in this scope`。

- [ ] **Step 3: 最小实现**

先把 `doc_rows_for_project, mark_missing` 加进 `index_job.rs` 顶部**已有的**那条 `use crate::index_store::{...}`（别新开一行 use），再补 `use std::collections::HashSet;`（若已有则不重复）。**同时删掉 Task 1 开在 `index_store.rs` 的那两行 `#[allow(dead_code)]` 连同它们各自的注释行** —— 调用点从这一任务起真的存在了，豁免留在树上就是一句过期假话（Step 6 的门会抓）。函数放在 `sweep_unrooted_projects` 之后：

```rust
/// 差集回收：本轮**完整**扫过这个项目之后，把「库里有行、本轮扫不到」的登记标成失效。
/// 集合两侧同源（库里那行的 path 就是上一轮 `WalkDir::new(登记的 root 串)` 拼出来的），
/// 所以一律用原样字符串比对：不做大小写折叠、不做 canonicalize（spec D7 —— 折叠会让换写法
/// 变成「旧行永久谎标 missing」，因为 `UNIQUE (project_id, path)` 是 BINARY 比较）。
/// 返回标掉的行数；调用方刻意丢弃，形状与轮末那句 `let _ = sweep_unrooted_projects(conn)?;` 一致。
/// 临时放行：生产调用点在 Task 4 的 `run_pass`，本任务落地时 lib 目标内还没有人调它，
/// 而 `cargo clippy --lib` 不带 `--tests` 会把每个无调用点的 `pub` 项判成 `dead_code`。
/// Task 4 Step 3 删掉这一行连同上面两句注释，Step 6 的门改成 exit=1 收口。
#[allow(dead_code)]
pub fn reclaim_missing(
    conn: &Connection,
    project_id: &str,
    outcome: &ScanOutcome,
    root_missing: bool,
) -> AppResult<usize> {
    // 闸门一：盘没挂载 ≠ 文件全被删。root_missing 时 run_pass 手里是 Default::default() 的空集合，
    // 少了这一支就会把整个项目的行成片标失效 —— 而本工具的常态用法里有移动盘与未挂载盘。
    // 「本轮零扫到」不另立闸门：它与这一支是同一个条件（spec 第四节记着这次合并）。
    if root_missing {
        return Ok(0);
    }
    // 闸门二：capped 的集合是 break 出来的，只含配额内前 N 个；配额外的老行不是被删了，是没轮到被看见。
    if outcome.capped {
        return Ok(0);
    }
    // 两桶都要：超限文件照样有行（too_large），漏了它就是把每个超限文件误标失效。
    let seen: HashSet<&str> = outcome
        .files
        .iter()
        .chain(outcome.over_size.iter())
        .map(|f| f.path.as_str())
        .collect();
    let tracked = doc_rows_for_project(conn, project_id)?;
    let candidates: Vec<i64> = tracked
        .iter()
        .filter(|(_, path)| !seen.contains(path.as_str()))
        .map(|(rowid, _)| *rowid)
        .collect();
    for rowid in &candidates {
        mark_missing(conn, *rowid)?;
    }
    Ok(candidates.len())
}
```

- [ ] **Step 4: 跑绿**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -5; echo exit=${PIPESTATUS[0]}`
Expected: `138 passed; 0 failed`，`index_job` 模块 **14** 条。

- [ ] **Step 5: 变异取证**

1. `let seen` 只取 `outcome.files`（去掉 `.chain(outcome.over_size.iter())`）⇒ Expected：第二条红在 `== 0`（实际 1）。
2. `filter` 谓词取反（`seen.contains`）⇒ Expected：第一条红在 `== 1`（实际 2）。
3. 注释掉 `for` 循环体 ⇒ Expected：第一条红在 `status == "missing"`（掉进 `Ok(1)` 但库里没动）。
4. **Step 0 那条补强自己的取证**：往 `mark_missing` 的 UPDATE 的 SET 里加 `indexed_at = NULL` ⇒ Expected：`mark_missing_flips_the_status_and_drops_only_the_fts_row` 红在写后快照对比那条（`indexed_at: Some(..) -> None`）。这条变异在 Task 1 落地时是全绿通过的，正是评审报 Important-1 的依据；补强之后必须能红，红不了就说明快照写错了（比如漏了某一列或比的是同一份 before）。

- [ ] **Step 6: 门禁（三条，第 2、3 条是这次新增的回收门与自我豁免门）**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings; echo exit=$?
cd src-tauri && cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
grep -rn "allow(dead_code)" src-tauri/src/index_store.rs; echo exit=$?
grep -n "allow(dead_code)" src-tauri/src/index_job.rs; echo exit=$?
```
Expected：两条 clippy `exit=0` 且 0 warning；`index_store.rs` 那条 **exit=1**（Task 1 那两行豁免的回收证据 —— 注意本仓有两棵 `src/`，这条门必须写全 `src-tauri/src/`，按字面跑 `src/` 抓的是前端树，永远产不出它承诺的残留，坑 86）；`index_job.rs` 那条**恰好 1 行**。

**为什么本任务要再开一行豁免**（与 Task 1 同一条坑 59 的第二层，不是新增范围）：`reclaim_missing` 的生产调用点在 Task 4 的 `run_pass`，本任务落地时它在 lib 目标内零调用点，`cargo clippy --lib` 不带 `--tests` 会判 `dead_code`。定稿写法：紧贴 `reclaim_missing` 一行 `#[allow(dead_code)]`，注释点名「生产调用点在 Task 4 的 `run_pass`，本任务先落函数」。Task 4 Step 3 删掉它、Step 6 的门改成 `exit=1` 收口。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/index_job.rs src-tauri/src/index_store.rs
git commit -m "feat(m5): reclaim_missing 的差集主路径（两条硬闸门 + 两桶集合）"
```

---

### Task 3: 熔断与两条闸门的独立守护

**Files:**
- Modify: `src-tauri/src/index_job.rs`（`reclaim_missing` 内加熔断；常量加在 `PROGRESS_EVERY` 附近；**Step 0 的三处收尾两格也在这个文件**）
- Modify: `src-tauri/src/index_store.rs`（Step 0 第 3 项：一行注释）
- Test: `index_job.rs` 的 `mod tests`（4 条新用例 + Step 0 往既有用例里并进一条断言，**条数不因 Step 0 变化**）

**Interfaces:**
- Consumes: T2 的 `reclaim_missing` / `outcome_with` / `missing_rows` / `scanned` / `doc_rows`
- Produces: `const MISSING_FUSE_MIN_ROWS: usize = 20;`（模块私有，不外泄）

- [ ] **Step 0: Task 2 评审留下的三处收尾（1 Important + 2 Minor，全是注释级 / 断言级零行为回归 ⇒ 折进本任务，不开返工轮 —— 坑 56）**

1. **并进一条断言，把「`reclaim_missing` 真的走了 `mark_missing`」钉住**（Task 2 评审 I-1，控制方实读 `index_job.rs:1020-1047` 复现成立）：`reclaim_marks_rows_the_pass_did_not_see` 的头注释声称最后两条检索断言守的是「删的是虚表侧那一半」，实际两条都守不住 —— `doc_hits` 由 `SELECT_SQL` 的正向 `= 'ok'`（`index_store.rs:280`）决定，status 一翻转必空，与虚表删没删无关；`fts_orphan_rows` 的判据是 `rowid NOT IN (SELECT doc_rowid FROM index_docs)`（`index_store.rs:167`），D3 保留主表行 ⇒ 它在这里**恒 0**。后果：把 `mark_missing` 换成一句裸 `UPDATE index_docs SET index_status='missing'` 也能全绿，而那是 D3 明确否掉的「只压症状」形态。修法是在该测试末尾并进一条断言（**测试条数不变，`index_job` 仍从 14 起算**）：

```rust
        // 这条才是「点删真的发生了」的判据：虚表侧只剩「留下.txt」那一行。
        // 上面两条检索断言守的是另一件事（doc_hits 由 SELECT_SQL 的正向 `= 'ok'` 决定，
        // status 一翻转必空；fts_orphan_rows 因 D3 保留主表行而恒 0）—— 措辞按 Task 2 评审订正。
        let fts_total: i64 = conn
            .query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts_total, 1, "变异：把 reclaim_missing 里的 mark_missing 换成裸 UPDATE 只改状态 ⇒ 这里是 2，这条红");
```

   同时把头注释里「最后两条检索断『删的是虚表侧那一半』…只数行数的话 `ok` 行还留在 FTS 里、照样搜得到」那两行改成：「检索两条断的是这一行退出召回；虚表点删由末尾那条 `count(*) FROM index_docs_fts` 承重（`fts_orphan_rows` 在保留主表行的形态下恒 0，不承重，见 Task 2 评审 I-1）」。

2. **给「两侧同源」补上唯一的破坏序列出处**（Minor-1，`index_job.rs:136` 那句现在写得像无条件事实）：句后加一句 dated 附注 —— 稳态构造不出不同源（`targets()` 原样取 `project_dirs.path` → `WalkDir::new(root)` → `entry.path().display()`，库里侧 `write_doc` 落的就是同一个 `file.path`），唯一可破坏的序列是**作业运行中改指 / 注销根目录**（`project.rs:343-347` 与 `index_job.rs:110-119` 的旧快照写回窗口）：旧根的行被写回来、而 sweep 不收（该项目此时有新 root 行，`NOT EXISTS` 不成立）⇒ 下一轮旧串成片标 missing。要点明**那不是误删活 root 下的行**，它把 M3 I3 那种「永久可搜的陈旧行」收敛掉了；Task 5 的事务关的是两条语句之间的窗口，关不到这个。

3. **`updated_at` 那一格记成有名事实，不补断言**（Minor-2，落在 `index_store.rs:717-718` 的注释）：D4 说「只动 `index_status` 与 `updated_at`」，但五列快照刻意不含 `updated_at`（SQLite 的 `datetime('now')` 是秒级粒度，同秒内前后相等 ⇒ 断它会 flaky），而 `index_docs.updated_at` 全仓零消费者 —— 删掉 SET 里那句 `updated_at = datetime('now')`，138 条全绿。在本模块测试的 `snapshot` 上方写一行点名：「`updated_at` 不入快照（同秒必等，断它会 flaky）且全仓零消费者 ⇒ 这一格无守护，是裁过的有名缺口，别当漏写」。**只写注释，不动断言、不动实现。**

- [ ] **Step 1: 写四条测试**

两条硬闸门各一条 + 熔断一对（阳性对照必须有：只有前者时「把实现改成永远 `return Ok(0)`」也照样绿）。

```rust
    /// 盘没挂载时一格都不许动。库里刻意只留 **3** 行：3 个候选过不了熔断的地板行数量，
    /// 所以少了 root_missing 这一支就会被真的标掉 ⇒ 这条测试对那一支是专属守护。
    #[test]
    fn an_unmounted_root_marks_nothing() {
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", Path::new("Z:/没挂载/移动盘"));
        for name in ["甲.txt", "乙.txt", "丙.txt"] {
            write_doc(
                &conn,
                "p1",
                &scanned(&format!("Z:/没挂载/移动盘/{name}")),
                DocOutcome::Ok("甲方要求验收指标".into()),
            )
            .unwrap();
        }
        assert_eq!(doc_rows(&conn, "p1"), 3, "前提：三行在库里");

        let o = outcome_with(&[], &[], false); // 生产里 root_missing 一定配空 outcome
        assert_eq!(reclaim_missing(&conn, "p1", &o, true).unwrap(), 0, "变异：删掉 root_missing 那一支 ⇒ 3 被标掉");
        assert_eq!(missing_rows(&conn, "p1"), 0, "未挂载不等于被删除");
    }

    /// 配额触顶那轮不许比对。库 3 行 / 扫到 2 行 ⇒ 候选 1，熔断（`1 * 2 > 3` 为假）挡不住它，
    /// 只有 capped 这一支挡得住 ⇒ 专属守护。
    #[test]
    fn a_capped_scan_marks_nothing() {
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", Path::new("C:/根"));
        write_doc(&conn, "p1", &scanned("C:/根/留下.txt"), DocOutcome::Ok("甲方要求验收指标".into())).unwrap();
        write_doc(&conn, "p1", &scanned("C:/根/也留下.txt"), DocOutcome::Ok("甲方也要求验收指标".into())).unwrap();
        let extra = write_doc(&conn, "p1", &scanned("C:/根/配额外的旧行.txt"), DocOutcome::Ok("配额之外的 付款条件".into())).unwrap();

        let o = outcome_with(&["C:/根/留下.txt", "C:/根/也留下.txt"], &[], true);
        assert_eq!(reclaim_missing(&conn, "p1", &o, false).unwrap(), 0, "变异：删掉 capped 那一支 ⇒ 1 被标掉");
        let status: String = conn
            .query_row("SELECT index_status FROM index_docs WHERE doc_rowid = ?1", [extra], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "ok", "触顶那轮不许把配额外的老行判成被删除");
        assert_eq!(missing_rows(&conn, "p1"), 0);
    }

    /// 熔断：25 行里 24 行不见 ⇒ 判为「疑似盘只挂了一半」，本轮不动。
    /// 正文一律同一句，是为了让最后那条召回断言数得出确切值（数字混进查询词会让分词口径变成赌局）。
    #[test]
    fn the_fuse_refuses_a_mass_reclaim() {
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", Path::new("C:/根"));
        for i in 0..25 {
            write_doc(
                &conn,
                "p1",
                &scanned(&format!("C:/根/f{i:02}.txt")),
                DocOutcome::Ok("甲方要求验收指标".into()),
            )
            .unwrap();
        }
        let o = outcome_with(&["C:/根/f00.txt"], &[], false);
        assert_eq!(reclaim_missing(&conn, "p1", &o, false).unwrap(), 0, "24/25 过半且过地板 ⇒ 熔断");
        assert_eq!(missing_rows(&conn, "p1"), 0);
        assert_eq!(index_store::doc_hits(&conn, "验收指标", 50).unwrap().len(), 25, "熔断生效 ⇒ 二十四行正文都还搜得到（limit 取 50：doc_hits 原样穿透，20 会把结果截到 20）");
    }

    /// 熔断的阳性对照 + 地板行的专属守护：同样过半、但只有 10 行、缺 6 个 ⇒ 必须标掉。
    /// 这条不可省：只有上一条「25 缺 24」时，把 `MISSING_FUSE_MIN_ROWS` 改成 0、
    /// 或整支熔断写成恒跳过，测试照样全绿（`24 * 2 > 25` 与地板无关，前一条测不到地板那一半）。
    /// 6 个候选过得了「过半」（`6 * 2 > 10`）但过不了「> 20」⇒ 只有地板值那一半能被打红。
    #[test]
    fn the_fuse_floor_lets_a_small_majority_reclaim_through() {
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", Path::new("C:/根"));
        let paths: Vec<String> = (0..10).map(|i| format!("C:/根/f{i:02}.txt")).collect();
        for p in &paths {
            write_doc(&conn, "p1", &scanned(p), DocOutcome::Ok("甲方要求验收指标".into())).unwrap();
        }
        assert_eq!(doc_rows(&conn, "p1"), 10, "前提：十行都在库里");

        let seen: Vec<&str> = paths[..4].iter().map(String::as_str).collect(); // 前 4 个还在磁盘，后 6 个不见了
        assert_eq!(
            reclaim_missing(&conn, "p1", &outcome_with(&seen, &[], false), false).unwrap(),
            6,
            "6/10 过半但不超地板 ⇒ 正常回收；变异：把 MISSING_FUSE_MIN_ROWS 改成 0 ⇒ 这里变 0"
        );
        assert_eq!(missing_rows(&conn, "p1"), 6);
        assert_eq!(doc_rows(&conn, "p1"), 10, "回收只改状态，一行都不许删（D3）");
        assert_eq!(index_store::doc_hits(&conn, "验收指标", 20).unwrap().len(), 4, "被标的 6 行退出召回，留下的 4 行照旧");
    }
```

- [ ] **Step 2: 跑红**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -30; echo exit=${PIPESTATUS[0]}`
Expected: **只有 `the_fuse_refuses_a_mass_reclaim` FAIL**（`left: 24, right: 0`）；其余三条 PASS（`root_missing` / `capped` 两支 T2 已实现）。若四条全绿，说明熔断在 T2 就越界写了 —— 报告里点名，别删测试凑数。

- [ ] **Step 3: 实现熔断**

`index_job.rs` 顶部常量区（`PROGRESS_EVERY` 之后）：

```rust
/// 熔断地板：一次要标掉的行数**同时**过成「过半」和「过地板（> 20 行）」，才判为
/// 「疑似盘只挂了一半」而不是「用户真删了」。地板存在的意义是小项目不被它挡住 ——
/// 一个几十上百个文件的项目整目录被删是日常操作，那种回收是真的。阈值只写这一处。
const MISSING_FUSE_MIN_ROWS: usize = 20;
```

`reclaim_missing` 内、`candidates` 之后、`for` 之前插：

```rust
    // 熔断：一次要标掉过半、且超过地板行数，更像是盘只挂了一半 / walkdir 早期崩，不是真删。
    // 整数比较不用除法：`tracked.len() / 2` 有截断歧义，`candidates * 2 > tracked.len()` 没有。
    if candidates.len() * 2 > tracked.len() && candidates.len() > MISSING_FUSE_MIN_ROWS {
        return Ok(0);
    }
```

- [ ] **Step 4: 跑绿**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -5; echo exit=${PIPESTATUS[0]}`
Expected: `142 passed; 0 failed`，`index_job` 模块 **18** 条。

- [ ] **Step 5: 变异取证（四条，逐条记读数）**

1. 删掉 `if root_missing` ⇒ Expected：`an_unmounted_root_marks_nothing` 红（`left: 3, right: 0`）。
2. 删掉 `if outcome.capped` ⇒ Expected：`a_capped_scan_marks_nothing` 红（`left: 1, right: 0`）。
3. 把 `MISSING_FUSE_MIN_ROWS` 改成 `0` ⇒ Expected：`the_fuse_floor_lets_a_small_majority_reclaim_through` 红（`left: 0, right: 6` —— 六个候选被地板归零挡掉了）。
4. 删掉整个熔断 `if` ⇒ Expected：`the_fuse_refuses_a_mass_reclaim` 红（`left: 24, right: 0`）。第 3 条不受影响 —— 「过半」与「过地板」两个条件各有一条测试守着，这是 D5 那支熔断唯一的完整守护形态。
5. **Step 0 第 1 项自己的取证**：把 `reclaim_missing` 循环体里的 `mark_missing(conn, *rowid)?` 换成一句裸 `UPDATE index_docs SET index_status = 'missing' WHERE doc_rowid = ?1` ⇒ Expected：`reclaim_marks_rows_the_pass_did_not_see` 红在末尾那条 `fts_total == 1`（实际 2）。这条红不了就说明并进的位置不对（它在状态翻转之后跑，任何「只翻状态」的实现都躲不过）。

- [ ] **Step 6: 门禁（本条期望值由 Task 3 落地后订正 —— 原写「`grep -rn "MISSING_FUSE_MIN_ROWS"` 恰好 2 行」是一条**永远不可能成立**的门，因为 Step 1 的测试蓝本自己在注释与断言消息里各点名了该常量一次；同形前科是坑 49（示例注释含被门扫的字面量）与坑 63（计数词会说谎））**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings; echo exit=$?
cd src-tauri && cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
grep -c "const MISSING_FUSE_MIN_ROWS" src-tauri/src/index_job.rs
grep -rn "len() > 20\b\|> 20)" src-tauri/src/; echo exit=$?
grep -rn "MISSING_FUSE_MIN_ROWS" src-tauri/src/index_job.rs
```
Expected：两条 clippy `exit=0` 且 0 warning；常量定义**恰好 1** 处；裸字面量那条 **exit=1**（家规的实质是「阈值数字只出现在定义处」，不是「常量名只出现两次」）；常量名那条**实测 4 行** = 定义 `:31` + 使用 `:186` + 测试注释 `:1155` + 断言消息 `:1172`，四处都是**点名这个常量**而不是重写数字，属合规。把这条读数写进报告，别为了让门绿去删注释。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/index_job.rs src-tauri/src/index_store.rs
git commit -m "feat(m5): 差集回收的熔断（过半且过地板行则本轮不比对）"
```

---

### Task 4: 挂进 `run_pass`，并验回填自愈

**Files:**
- Modify: `src-tauri/src/index_job.rs`（`run_pass` 内、每个 target 的队列循环结束后、per-project 那句 `on_progress(progress_for(&target, &acc, "running", ""))` 之前）
- Test: 同文件 `mod tests`
- 不许改：既有夹具 `tree()` / `opts()`（既有用例共用，动它会波及别处断言）

**Interfaces:**
- Consumes: T2/T3 的 `reclaim_missing`、既有 `run_pass` 签名与 `targets()` / `scan_root` 主循环
- Produces: `run_pass` 每轮把被删文件的行为 `missing`；文件放回后同一 `doc_rowid` 回 `ok`

- [ ] **Step 1: 写失败测试**

```rust
    /// 端到端（真 tempdir + 真 walkdir）：删一个已索引文件 ⇒ 下一轮它自动变失效、退出召回；
    /// 放回 ⇒ **同一行**被重抽回 ok（spec D6：回填不新开设机制，靠的是 current_rowid
    /// 要求 index_status='ok' 这条既有谓词）。本轮唯一一条走完
    /// scan_root → 差集 → mark_missing → 召回 → 回填 全链的用例，所以断言覆盖全链。
    /// 删的文件是 tempdir 里自造的夹具，不是任何用户资料（红线一）。
    #[test]
    fn a_file_deleted_since_the_last_pass_is_marked_missing_and_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("留下.txt"), "甲方要求验收指标".as_bytes()).unwrap();
        let gone = dir.path().join("没了.txt");
        std::fs::write(&gone, "报价单里写着验收流程".as_bytes()).unwrap();
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        // 第二个项目刻意做成「有 root 行、但 root 指向不可能存在的目录」，专门钉住接线里
        // `root_missing` 那个实参传没传对：硬编码成 `false` 会让它的两行被成片标 missing。
        // 只钉 unit 级不够 —— Task 3 那四条 unit 用例是直接调 `reclaim_missing` 的，
        // 改调用点的实参它们一条都抓不到（「中间层没有测试等于没守」是 M3 的 I2 教训）。
        // 两行是假路径字符串键（本仓 blessed 的做法，出处是本测试模块里 `scanned` 助手的头注释
        // —— 指针一律写符号，行号会漂：Task 4 落地时原写的 `index_job.rs:853-854` 已经漂到别处），
        // 磁盘上不需要真的存在。软删与无 root 行的形态不归这里（归 sweep 那两条既有用例）。
        conn.execute("INSERT INTO projects (id, name, status) VALUES ('p2', '盘没挂载的项目', '立项')", []).unwrap();
        conn.execute("INSERT INTO project_dirs (id, project_id, kind, path) VALUES ('p2-d', 'p2', 'root', 'Z:/没挂载/移动盘')", []).unwrap();
        write_doc(&conn, "p2", &scanned("Z:/没挂载/移动盘/合同.txt"), DocOutcome::Ok("移动盘上的遗留合同".into())).unwrap();
        write_doc(&conn, "p2", &scanned("Z:/没挂载/移动盘/清单.txt"), DocOutcome::Ok("移动盘上的遗留清单".into())).unwrap();
        // ↑ p2 的正文刻意用**不与 p1 重叠**的词（`bef01f5` 原稿复用了「甲方要求验收指标」，被
        // Task 4 实现者实跑撞出：`doc_hits` 与 `SELECT_SQL` 都**不带项目过滤**，于是本用例末尾
        // 「留下那行不受牵连」那条断言的真读数是 3 而不是 1）。约束只有一条：正文不与本用例的
        // 检索用词（验收流程 / 验收指标）重叠即可，具体串怎么起不重要。p2 只参与行数与 missing
        // 计数断言，不参与任何检索断言 —— 这是它作为「未挂载夹具」唯一需要承担的角色。

        run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(doc_rows(&conn, "p1"), 2, "前提：首轮两行都进了库");
        assert_eq!(index_store::doc_hits(&conn, "验收流程", 20).unwrap().len(), 1,
            "前提：那行正文此刻真搜得到");

        std::fs::remove_file(&gone).unwrap();
        run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();

        assert_eq!(missing_rows(&conn, "p1"), 1, "第二轮回扫不到就该标失效");
        assert_eq!(doc_rows(&conn, "p1"), 2, "只标不删（D3）");
        assert_eq!(missing_rows(&conn, "p2"), 0, "盘没挂载那个项目一格都不许动 —— 这条钉的是接线处传的 `root_missing` 实参");
        assert_eq!(doc_rows(&conn, "p2"), 2, "p2 的两行都该原样留着（`targets()` 会列出它，但 root 不可达）");
        assert!(index_store::doc_hits(&conn, "验收流程", 20).unwrap().is_empty(),
            "还搜得到就说明虚表侧没删干净");
        assert_eq!(index_store::doc_hits(&conn, "验收指标", 20).unwrap().len(), 1, "留下那行不受牵连");
        assert_eq!(index_store::fts_orphan_rows(&conn), 0);

        std::fs::write(&gone, "报价单里写着验收流程".as_bytes()).unwrap();
        run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(missing_rows(&conn, "p1"), 0, "文件回来后同一行该被重抽回 ok");
        assert_eq!(doc_rows(&conn, "p1"), 2, "是更新同一行，不是新增一行");
        assert_eq!(index_store::doc_hits(&conn, "验收流程", 20).unwrap().len(), 1, "回填后又能搜到");
    }
```

- [ ] **Step 2: 跑红**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -30; echo exit=${PIPESTATUS[0]}`
Expected: 新用例 FAIL，红点在 `assert_eq!(missing_rows(&conn, "p1"), 1, ...)`（`left: 0, right: 1`）—— 回收函数有了，但没人调它。

- [ ] **Step 3: 接线（并回收 Task 2 的临时豁免）**

先删掉 Task 2 开在 `reclaim_missing` 上方那行 `#[allow(dead_code)]` 连同它上方那段临时注释（落地形态是**三句**，简报原写「两句」是计数词说谎，按同形先例坑 63 订正；整段一起删、不留半句） —— 生产调用点从本任务起真的存在了，豁免留着就是一句过期假话（Step 6 的门会抓）。再加这一句：

```rust
        // 差集回收（M5）：本轮完整扫过这个项目之后，库里那些「本轮没扫到」的行标成失效。
        // 两条硬闸门与熔断全在 reclaim_missing 里面，这里只负责把 root_missing 与本轮集合交出去。
        // 计数刻意丢弃，形状与轮末那句 `let _ = sweep_unrooted_projects(conn)?;` 一致：接住它就得给
        // RunSummary 加字段，而 missing 的行数本来就能从 status_counts 的 GROUP BY 免费拿到
        // （`index_overview` 的 pick("missing") 已在等）。代价照实记在 spec 风险 6：
        // 被熔断跳过时界面与不跳过时长得一样，没人能看出这一轮被挡下了。
        // 上面两条 cancel 早退路径（循环开头那条、循环内那条）都不经过这里 —— 一轮被取消时不做额外写库。
        let _ = reclaim_missing(conn, &target.project_id, &outcome, root_missing)?;
```

- [ ] **Step 4: 跑绿**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -5; echo exit=${PIPESTATUS[0]}`
Expected: `143 passed; 0 failed`，`index_job` 模块 **19** 条。**既有用例必须一条都不红**，尤其是 `run_pass_indexes_a_generated_tree_and_separates_outcomes`、`missing_root_is_flagged_without_killing_the_run`、`a_pass_reclaims_rows_left_by_an_unrooted_project`（它们库里都有真实 tempdir 行，接线若误标会当场红）。

Task 2 评审的两条结论对本任务有效，写在这里免得下一个人重新发现：① 「库里 path 与本轮 path 逐字同源」这件事**不是**无守护 —— 既有 `second_pass_skips_unchanged_files`（`index_job.rs:600-614`，末句 `unchanged == 1`）已经用真两轮 `run_pass` 加上 `current_rowid` 的 `path = ?2` 谓词钉住了写读两端逐字相等，所以本任务不是唯一证据、别把它当唯一证据来设计；② 但本任务里「**同一 `doc_rowid` 回 ok**」那一条是唯一的 —— 两侧不同源时第二轮是**插一条新行**而不是更新旧行，只有这一条能区分。它**不许被弱化**成「missing 计数归零」那种两读的断言。

- [ ] **Step 5: 变异取证**

1. 删掉接线那一句 ⇒ Expected：本任务用例红在 `missing_rows == 1`。
2. 把接线那句的 `root_missing` 实参硬编码成 `false` ⇒ Expected：本任务用例红在 `missing_rows(&conn, "p2") == 0`（p2 的两行被成片标掉，读数 `left: 2, right: 0`）。**控制方预先算过写在这里，免得实现者白跑**：Task 3 那四条 unit 用例抓不到这个变异（它们直接调 `reclaim_missing`，不经过调用点），而 p1 的 root 可达、真实值本来就是 `false` ⇒ 没有 p2 这一格，这条变异会「全量照绿 = 零守护」。所以处置是**补 p2 夹具**，不是登记有名缺口（坑 53 的规矩：全绿就得补测试，别宣布变异不成立）。
3. 把它挪到 `on_progress(...)` 之后 ⇒ Expected：全量仍 143 绿 ⇒ 记为「顺序等价变异」，登记即可，不新增断言。

- [ ] **Step 6: 门禁（含 Task 2 那块豁免的回收证据）**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings; echo exit=$?
cd src-tauri && cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
grep -rn "allow(dead_code)" src-tauri/src/index_job.rs; echo exit=$?
grep -rn "allow(dead_code)" src-tauri/src/index_store.rs; echo exit=$?
```
Expected：两条 clippy `exit=0` 且 0 warning；`index_job.rs` 那条 **exit=1**（Task 2 那块豁免已回收，全仓 `dead_code` 豁免至此只剩 `db.rs:30` 那处 M1 遗留的永久豁免，不归本轮、不许顺手删）；`index_store.rs` 那条仍 **exit=1**（Task 2 收掉的两行不会被本任务又写回来）。全量 **143 passed**。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/index_job.rs
git commit -m "feat(m5): run_pass 每轮按项目做差集回收；文件放回靠既有增量判据自动回填"
```

---

### Task 5: `set_root_dir` 的 DELETE+INSERT 包事务

**Files:**
- Modify: `src-tauri/src/project.rs`（`set_root_dir`）
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: 既有 `create_project` / `add_entry_dir` / `set_root_dir` / `input(name)` 助手、`DirInput { path: String, label: Option<String>, sort: Option<i64> }`、`project_dirs` 的 `UNIQUE (project_id, path)`
- Produces: 无新接口（签名与返回类型都不变，`lib.rs` 命令层与 M1 既有测试照旧吃它）

- [ ] **Step 1: 写失败测试**

```rust
    /// 残留 11：`set_root_dir` 原来是 DELETE + INSERT 两条独立的自动提交语句，作业轮末的
    /// `sweep_unrooted_projects` 恰好落在两条之间就会把该项目看成「无 root 行」⇒ 整项目索引行被清。
    /// 包进一个事务后这个窗口不存在。回滚探针用「同路径的 entry 行」：DELETE root 先成功，
    /// INSERT 必撞 `UNIQUE (project_id, path)`，于是能观测到 DELETE 有没有被一起回滚。
    /// 变异：把事务拆回两条 `conn.execute` ⇒ 第二条断言红（旧 root 行已自动提交掉了）。
    #[test]
    fn a_failed_root_replacement_keeps_the_old_root_row() {
        let c = db::open_in_memory().unwrap();
        let p = create_project(&c, &input("事务验证")).unwrap();
        add_entry_dir(&c, &p.id, &DirInput { path: "E:/同一路径".into(), label: None, sort: None }).unwrap();
        set_root_dir(&c, &p.id, &DirInput { path: "E:/旧根".into(), label: None, sort: None }).unwrap();

        let err = set_root_dir(&c, &p.id, &DirInput { path: "E:/同一路径".into(), label: None, sort: None })
            .expect_err("INSERT 该撞 UNIQUE(project_id, path)：entry 行已经占了那个路径");
        assert_eq!(err.code, "db_failed", "rusqlite 的约束错误走 error.rs 那条 From，不该被翻译成别的码");

        let root: Option<String> = c
            .query_row(
                "SELECT path FROM project_dirs WHERE project_id = ?1 AND kind = 'root'",
                [&p.id],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .unwrap();
        assert_eq!(root.as_deref(), Some("E:/旧根"), "INSERT 失败必须把 DELETE 一起回滚");
        let dirs: i64 = c
            .query_row("SELECT count(*) FROM project_dirs WHERE project_id = ?1", [&p.id], |r| r.get(0))
            .unwrap();
        assert_eq!(dirs, 2, "entry 与旧 root 都该还在：一次失败的改指不许留下半张表");
    }
```

`.optional()` 要 `rusqlite::OptionalExtension` 在作用域内：`project.rs` 主体已在用 `.optional()?`，测试模块若拿不到就在 `mod tests` 的 use 里补一行 `use rusqlite::OptionalExtension;`（别改主体那条）。

- [ ] **Step 2: 跑红**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -30; echo exit=${PIPESTATUS[0]}`
Expected: 新用例 FAIL，红在 `root.as_deref() == Some("E:/旧根")`（实际 `None`）。

- [ ] **Step 3: 实现事务**

把那两条 `conn.execute(...)` 包进事务，`clear_project` 留在事务外（`index_store` 的「每个写函数自开自收」约定不许破）：

```rust
    // 两条语句必须原子：轮末的 sweep 落在中间会把「刚删掉旧 root」误读成「这项目没 root 行」，
    // 连带清掉整项目索引行（M3 残留 11）。`unchecked_transaction` 收 &Connection ⇒ 签名不动；
    // 中途 Err 时它的 Drop 是 DropBehavior::Rollback（vendored transaction.rs 已核，见事实 9）。
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM project_dirs WHERE project_id = ?1 AND kind = 'root'", [project_id])?;
    tx.execute(
        "INSERT INTO project_dirs (id, project_id, kind, path, label, sort) VALUES (?1,?2,'root',?3,?4,0)",
        params![uuid::Uuid::new_v4().to_string(), project_id, path, normalize(input.label.clone())],
    )?;
    tx.commit()?;
```

`old` 那次查询、`if old.is_some_and(|old_path| old_path != path)` 里的 `clear_project`、末尾 `find_dir` **一字不动**（裸字符串比较判改指是 D7 的结论，本轮不改）。

**顺带把规格风险 8 的那条未验证项结掉**（不靠 fetch，靠形状）：规格写的是「DEFERRED 事务里把读升级为写失败时 SQLite 会立即回 `SQLITE_BUSY` 而不走 busy handler」，那条语义只在**事务内先读过、再写**时才成立。本任务的 `tx` 体是 `BEGIN DEFERRED` → **第一件事就是 DELETE**（写），事务内在它之前没有任何 SELECT：`old` 那次查询在 `let tx = ...` **之前**、属事务外的自动提交读。所以这一格不构成风险 8 描述的那个形态，无须改 `Immediate`（那要 `set_transaction_behavior(&mut)`、签名得动）。**但这是一条形状不变量**：谁以后为了「顺手」把 `old` 那次 SELECT 移进事务里，风险 8 立刻成立。请把这件事写进代码注释（一句：「`old` 的读必须留在事务外 —— 挪进来会让本事务变成先读后写的 DEFERRED 形态，触发 SQLite 的立即 `SQLITE_BUSY` 而不走 `busy_timeout`」），并在报告 §「与规格风险对照」那一格写明你的核对方式是形状核对而非文档核，规格风险 8 由此降级为「形状守卫 + 未做并发实测」的有名项。

- [ ] **Step 4: 跑绿**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -5; echo exit=${PIPESTATUS[0]}`
Expected: `144 passed; 0 failed`，`project` 模块 **11** 条；既有 `root_dir_is_replaced_not_duplicated` 与 `repointing_the_root_drops_rows_of_the_old_path` 照旧绿（成功路径行为未变正是这两条的职责）。

- [ ] **Step 5: 变异取证**：去掉 `let tx = ...` 与 `tx.commit()?`、退回两条 `conn.execute` ⇒ Expected：新用例红在 `root.as_deref()`。

- [ ] **Step 6: 门禁（本条期望值由 Task 5 落地订正 —— 原写「`grep -c "unchecked_transaction"` 期望 1」与 Step 3 蓝本互斥：蓝本注释自己点名了这个词，按蓝本落地恒为 2。同形前科是 Task 3 Step 6 那条常量门与坑 49/63 —— 写给别人的门，落笔前必须当场跑一遍**）

```bash
cd src-tauri && cargo clippy --lib -- -D warnings; echo exit=$?
cd src-tauri && cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
grep -c "conn.unchecked_transaction()" src-tauri/src/project.rs
grep -rn "allow(dead_code)" src-tauri/src/project.rs; echo exit=$?
```
Expected：两条 clippy `exit=0` 且 0 warning；调用点**恰好 1**（门扫的是调用形态而不是这个词本身，注释里再点名几次都算合规 —— 实现者**不许**为了凑门删注释）；`allow(dead_code)` 那条 **exit=1**（本任务没有任何新 `pub` 项，不该出现豁免）。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/project.rs
git commit -m "fix(m5): set_root_dir 的改指两条语句包进一个事务（M3 残留 11）"
```

---

### Task 6: 预览的当前尺寸门（I-4）+ 一句过期文案

**Files:**
- Modify: `src-tauri/src/doc_preview.rs`（`unavailable`、`row_for_preview` 末尾、`render_preview`、`map_io_error` 里的 M5 hint、测试模块的 `preview()` 助手）
- Modify: `src-tauri/src/lib.rs`（`doc_preview` 命令体）
- Test: `doc_preview.rs` 的 `mod tests`

**Interfaces:**
- Consumes: 既有 `index_scan::ScanOptions::load(&conn)`（键名 `index_max_file_bytes`）、`std::fs::metadata`
- Produces: `fn unavailable(why: &str) -> AppError`（私有，签名变了）、`pub fn render_preview(doc_id: &str, row: &PreviewRow, query: &str, max_file_bytes: u64) -> AppResult<DocPreview>`；`render_preview_with` **不变**

- [ ] **Step 1: 写失败测试**

```rust
    /// 终审 I-4：`index_docs.size` 是**登记时**的读数，文件在索引之后膨胀就不受任何上限约束
    /// （扫描器那道门只在扫描循环里判），而 `extract_text` 自己的 doc 注释写着「不做大小校验」。
    /// 修法：生产入口按**当前磁盘尺寸**再过一次同一道上限。
    /// 阳性对照必须有：上限放宽一格就该正常抽出，否则上面那条 expect_err 是恒红的。
    #[test]
    fn a_file_that_grew_past_the_limit_after_indexing_is_not_previewed() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("膨胀.txt");
        std::fs::write(&p, "甲方要求验收指标正文".as_bytes()).unwrap(); // 十个汉字 = 30 字节，刻意大于下面那个 10 的上限
        let c = conn();
        let id = seed_row(&c, &p.display().to_string(), DocOutcome::Ok("甲方要求验收指标".into()));
        let row = row_for_preview(&c, &id).unwrap();
        assert_eq!(row.path, p.display().to_string(), "前提：那一行的 path 就是磁盘上这个真文件");

        let e = render_preview(&id, &row, "验收", 10).expect_err("当前磁盘尺寸 30 > 上限 10 ⇒ 该拒");
        assert_eq!(e.code, "preview_unavailable", "不扩码表：三成因共用一条码，靠 hint 分（spec D10）");

        let ok = render_preview(&id, &row, "验收", 100_000).unwrap();
        assert_eq!(ok.ranges.len(), 1, "阳性对照：上限放宽就该正常抽出");
    }
```

- [ ] **Step 2: 跑红**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -20; echo exit=${PIPESTATUS[0]}`
Expected: 编译失败，`this function takes 3 arguments but 4 arguments were supplied`。

- [ ] **Step 3: 实现**

`doc_preview.rs`：

```rust
/// 三种成因共用 `preview_unavailable`（行不 ok / 当前尺寸超上限 / 已有一份在抽取），
/// 因为 M4 §七 把「前端只认 preview_* 两个码」定成了契约；成因一律写进 hint 由界面念出来。
/// 扩码表是终审议题（spec 风险 5），不许在本任务顺手加。
fn unavailable(why: &str) -> AppError {
    AppError::new("preview_unavailable", "这条索引记录不能预览", Some(why))
}
```

`row_for_preview` 末尾改成（原文案一字不变，只是搬进参数）：

```rust
    row.ok_or_else(|| unavailable("记录可能来自已注销的目录，或索引重建前的旧结果；到 /index 重跑一轮索引"))
```

生产入口加判门（**不动 `render_preview_with`**：它被测试模块直接调用十余处，而那条路刻意不碰磁盘）：

```rust
/// 生产入口：抽取器固定为 `extract::extract_text`。
/// `max_file_bytes` 是扫描器那道同一上限：`index_docs.size` 只是登记时的读数，
/// 文件在索引之后长大就不再有上限（终审 I-4），所以这里按**当前磁盘尺寸**再过一次。
/// stat 失败一律放行 —— 读不到元数据不代表超限，让抽取器自己出 `fs_failed`
/// （既有 `io_errors_are_translated…` 用假路径走的就是这一格，别把它改成拒绝）。
pub fn render_preview(
    doc_id: &str,
    row: &PreviewRow,
    query: &str,
    max_file_bytes: u64,
) -> AppResult<DocPreview> {
    if let Ok(meta) = std::fs::metadata(&row.path) {
        if meta.len() > max_file_bytes {
            return Err(unavailable("文件在索引之后长大了，已超过单文件上限；到 /index 重跑一轮把它降级成 skipped"));
        }
    }
    render_preview_with(doc_id, row, query, extract_text)
}
```

`map_io_error` 里那句「真正的失效标记要到 M5 的核对轮才会写回记录」在本轮之后成了过期文案，改成：

```rust
            Some("文件可能已改名或被移走；下一轮索引会把它标成失效，/index 的状态筛选能列出全部失效行"),
```

测试模块助手加一个「尺寸门不参与」的上限：

```rust
    /// 与 Task 6 命令体同一条链：锁内查库、锁外抽盘。`u64::MAX` = 尺寸门对本模块既有用例透明。
    fn preview(conn: &Connection, doc_id: &str, q: &str) -> AppResult<DocPreview> {
        render_preview(doc_id, &row_for_preview(conn, doc_id)?, q, u64::MAX)
    }
```

`lib.rs` 的 `doc_preview` 命令：锁内顺带取上限，锁外抽出。

```rust
    let (row, max_file_bytes) = {
        let conn = db(&state)?;
        (
            doc_preview::row_for_preview(&conn, &doc_id)?,
            index_scan::ScanOptions::load(&conn)?.max_file_bytes,
        )
    };
    doc_preview::render_preview(&doc_id, &row, &query, max_file_bytes)
```

- [ ] **Step 4: 跑绿**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -5; echo exit=${PIPESTATUS[0]}`
Expected: `145 passed; 0 failed`，`doc_preview` 模块 **15** 条；既有 14 条一条都不红（`u64::MAX` 让尺寸门对它们透明）。

- [ ] **Step 5: 变异取证**：删掉 `if let Ok(meta) = std::fs::metadata(...)` 整块 ⇒ Expected：本任务用例红在 `expect_err`（拿到了 Ok）。第二条（阳性对照）保证这条测试不会因为「恒拒」而假绿。

- [ ] **Step 6: 门禁**
```bash
cd src-tauri && cargo clippy --lib -- -D warnings; echo exit=$?
cd src-tauri && cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
grep -rn "File::open" src-tauri/src/doc_preview.rs; echo exit=$?
grep -rn "20971520" src-tauri/src/doc_preview.rs src-tauri/src/lib.rs; echo exit=$?
```
Expected：两条 clippy `exit=0` 且 0 warning；`File::open` 那条 **exit=1**（`std::fs::metadata` 不是 `File::open`，M4 那条既有门照旧成立、不许顺手改它）；上限那条 **exit=1**（预览侧的 hint 与注释都得从 `opts.max_file_bytes` 格式化，不许写死数字）。

**本条门的作用域由 Task 6 落地订正（第三次应验「门与仓库现实互斥」，前两次是 Task 3 的常量门与 Task 5 的事务门）**：原写 `grep -rn "20971520" src-tauri/src/` 期望 exit=1，实跑 **exit=0、命中 3 行**（`db.rs:311` 的 settings 种子、`db.rs:416` 那条「spec 定的单文件 20 MB」断言、`index_scan.rs:32` 的默认值兜底）—— 那三处是这道上限**本该在的地方**，全都是 M1 既有内容、不归本任务。所以门改成只扫本任务真正可能写死数字的两个文件（预览侧 + 命令侧）。**通用形态**：写「期望零命中」的门时，作用域必须收到「本轮新写的东西所在的那几格」，全仓扫法迟早被既有合规命中打死（坑 86 的另一种变体）。实现者当时**没有**为凑门删任何注释或文案，判为处置正确。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/doc_preview.rs src-tauri/src/lib.rs
git commit -m "fix(m5): 预览按当前磁盘尺寸再过一次上限门（终审 I-4）"
```

---

### Task 7: 预览的单飞闸门（I-5）

**Files:**
- Modify: `src-tauri/src/doc_preview.rs`（新增 `PreviewShared` / `PreviewGuard` / `busy()`；测试 +2）
- Modify: `src-tauri/src/lib.rs`（`AppState` 加字段、setup 构造里加一格、`doc_preview` 命令进出入）

**Interfaces:**
- Consumes: 既有 `RunningGuard` 的形状（`index_job.rs:315-327`）与它那条「panic 展开也复位」的测试先例
- Produces: `pub struct PreviewShared`、`PreviewShared::new()`、`PreviewShared::try_enter(shared: &Arc<PreviewShared>) -> Option<PreviewGuard>`、`pub fn busy() -> AppError`

- [ ] **Step 1: 写两条失败测试**

```rust
    /// 终审 I-5：命令体直接跑在 tokio **多线程** worker 上（`kind = "sync_threadpool"` 只是
    /// tracing 标签，见 HANDOFF 坑 87），所以连点 N 份慢文档就是 N 个 worker 被占死，
    /// 之后**所有** async 命令与插件异步任务一起排队。真机读数：单次慢预览 266,093 ms。
    /// 单飞闸门：占用中第二条直接拒。第三条断言（释放后能再进）不可省 ——
    /// 没有它，「永不放行」的实现也能过前两条。
    #[test]
    fn only_one_preview_is_in_flight_at_a_time() {
        let shared = std::sync::Arc::new(PreviewShared::new());
        let first = PreviewShared::try_enter(&shared);
        assert!(first.is_some(), "空闲时第一条该进");
        assert!(PreviewShared::try_enter(&shared).is_none(), "占用中第二条该被拒");
        drop(first);
        assert!(PreviewShared::try_enter(&shared).is_some(), "释放后该能再进");
    }

    /// 借 `index_job::running_flag_is_released_even_when_the_job_panics` 的形状。
    /// 少了 Drop 复位（比如改成命令末尾手写 `store(false)`），一次 panic 就**永久**锁死预览，
    /// 而界面只会一直念「已有一份预览正在抽取」，没有任何报错指向真正的原因。
    #[test]
    fn the_preview_gate_is_released_when_the_body_panics() {
        let shared = std::sync::Arc::new(PreviewShared::new());
        let outer = std::sync::Arc::clone(&shared);
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = PreviewShared::try_enter(&outer).expect("第一条该进");
            panic!("预览体内炸");
        }));
        assert!(ran.is_err(), "前提：闭包确实 panic 了，否则下面那条断言什么都没证明");
        assert!(PreviewShared::try_enter(&shared).is_some(), "panic 展开也必须把闸门放掉");
    }
```

- [ ] **Step 2: 跑红**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -20; echo exit=${PIPESTATUS[0]}`
Expected: 编译失败 `cannot find type PreviewShared in this scope`。

- [ ] **Step 3: 实现**

`doc_preview.rs` 的 `mod tests` 之前：

```rust
/// 预览的单飞闸门（终审 I-5）。住在 `doc_preview` 而不是塞进 `IndexShared`：那四格
/// （cancel/running/summary/last_error）语义全是索引作业的，混一格预览状态会让下一个读者
/// 重新发现一遍归属（spec D9）。
#[derive(Default)]
pub struct PreviewShared {
    busy: std::sync::atomic::AtomicBool,
}

/// 进预览的票。Drop 复位，所以 panic 展开也照样放行 —— 形状照 `index_job::RunningGuard`。
pub struct PreviewGuard {
    shared: std::sync::Arc<PreviewShared>,
}

impl Drop for PreviewGuard {
    fn drop(&mut self) {
        self.shared.busy.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl PreviewShared {
    pub fn new() -> Self {
        Self::default()
    }

    /// `swap` 而不是「先 read 再 store」：命令体跑在多线程 worker 上，
    /// 两步写会让两条都以为自己独占。
    pub fn try_enter(shared: &std::sync::Arc<Self>) -> Option<PreviewGuard> {
        if shared.busy.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return None;
        }
        Some(PreviewGuard { shared: std::sync::Arc::clone(shared) })
    }
}

/// 三成因里的第三种（D10：不扩码表，靠 hint 分）。
pub fn busy() -> AppError {
    AppError::new(
        "preview_unavailable",
        "这条索引记录不能预览",
        Some("已有一份预览正在抽取原文，等它结束再试；大文件的抽取可能要几十秒"),
    )
}
```

`lib.rs` 三处：`AppState` 加 `preview: std::sync::Arc<doc_preview::PreviewShared>,`；setup 的 `AppState { ... }` 构造里加 `preview: std::sync::Arc::new(doc_preview::PreviewShared::new()),`；命令体**最前面**：

```rust
    // 单飞闸门（I-5）：_guard 必须活到函数末尾，所以绑下划线**带名字**（`let _ =` 会当场释放）。
    let _guard = doc_preview::PreviewShared::try_enter(&state.preview).ok_or_else(doc_preview::busy)?;
```

放在取 row 之前：先占闸门，失败就不碰库、不碰磁盘。

- [ ] **Step 4: 跑绿**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -5; echo exit=${PIPESTATUS[0]}`
Expected: `147 passed; 0 failed`，`doc_preview` 模块 **17** 条。

- [ ] **Step 5: 变异取证（含一条如实登记的等价变异）**

1. `try_enter` 里的 `swap` 改成「先 `load` 判断再 `store`」⇒ 两条测试仍绿。这是**等价变异**（本机单线程测试构不出竞态）⇒ 报告里写成「闸门语义有守护，原子性无守护」并登记为有名缺口，**不许**声称变异红了。
2. 删掉 `impl Drop for PreviewGuard` 的复位语句 ⇒ Expected：第二条红在「panic 展开也必须把闸门放掉」。
3. `try_enter` 恒回 `None` ⇒ Expected：第一条红在「空闲时第一条该进」。

- [ ] **Step 6: 门禁**：两条 clippy `exit=0`；`grep -c "tauri::command(async)" src-tauri/src/lib.rs` 期望**恰好 1**（本轮没给别的命令加 async）。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/doc_preview.rs src-tauri/src/lib.rs
git commit -m "feat(m5): 预览加单飞闸门与 Drop 复位（终审 I-5）"
```

---

### Task 8: 启动自动追平一轮

**Files:**
- Modify: `src-tauri/src/lib.rs`（`setup` 闭包）
- Test: **无新单测** —— 要真实 `App` 实例。与 M3 缺口 2「`index_start` 的线程壳与 `app.emit` 连通性没有 Rust 单测，只有真机证据」同口径，写进「没能自动验证」清单，不许为了凑数造注入缝。

**Interfaces:**
- Consumes: 既有 `index_job::start(app, data_dir, shared, project_id, rebuild)`；T1–T7 的全部产出
- Produces: 应用启动即自动跑一轮 `rebuild = false` 的增量轮（内含差集回收）

- [ ] **Step 1: 改 setup**

```rust
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let conn = db::open(&data_dir)?;
            let index = Arc::new(index_job::IndexShared::new());
            app.manage(AppState {
                conn: Mutex::new(conn),
                data_dir: data_dir.clone(),
                mk: Mutex::new(None),
                index: Arc::clone(&index),
                preview: Arc::new(doc_preview::PreviewShared::new()),
            });
            // M5 D1：启动即追平一轮。rebuild=false 走既有增量判据（size + mtime + ok 相同 ⇒ 连文件都
            // 不打开），所以常态代价是一次目录遍历 + stat，不是重抽；root 不可达的项目连 walkdir 都不发起。
            // 与用户手动点撞车由 `start` 开头的 running.swap 挡：那一次收到既有 `index_running` 的 amber
            // 提示（M3 真机验过它的文案「上一轮还没结束」，本轮之后它触发得更频繁，但那句话仍是实话）。
            // 自动轮的失败不拦启动：`let _ =` 丢掉 AppResult —— 追平失败不该让应用打不开。
            // 头几秒前端还没装 index://progress 监听，那段进度事件会丢；用户进 /index 看到的是
            // 徽标「索引进行中」+ 既有那句「本轮已开始，正在等第一条进度事件」的空态文案，M3 已实测过这一档。
            let _ = index_job::start(app.handle().clone(), data_dir, index, None, false);
            Ok(())
        })
```

`data_dir` 那行从 `data_dir,` 改成 `data_dir: data_dir.clone(),`（自动轮要拿走原值），别改成传引用。**`Arc` 一律写裸名**：`lib.rs` 顶部已有 `use std::sync::{Arc, Mutex, MutexGuard};`，Task 7 落地时那两处 `std::sync::Arc` 已由控制方统一成裸名（同文件相邻两格两种写法是坑 60 那一族的「同一含义两种写法」），本任务新增的三处照裸名写，别把全限定形式带回来。

- [ ] **Step 2: 全量门禁**

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -5; echo exit=${PIPESTATUS[0]}
cd src-tauri && cargo clippy --lib -- -D warnings; echo exit=$?
cd src-tauri && cargo clippy --lib --all-targets -- -D warnings; echo exit=$?
npm run build; echo exit=$?
node --test "tests/**/*.test.ts"; echo exit=$?
```
Expected: `147 passed / 0 failed`；两条 clippy `exit=0` 0 warning；`npm run build` exit 0；node `ℹ tests 8 / pass 8 / fail 0`。
再数一次命令条数（本轮**不许**新增命令）：`sed -n '/generate_handler/,/])/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z0-9_]+,?$"` 期望 **41**。

**这条正则是 Task 8 落地时订正过的第四例「门与蓝本互斥」**：原写 `grep -cE "^ +[a-z_]+,$"`（要求尾逗号），而 `generate_handler!` 的**最后一条** `doc_preview` 按 Rust 宏写法不带逗号 ⇒ 实跑恒为 40，与它自己承诺的 41 差一。控制方三种写法都当场跑过：带逗号 `40`、`^ +[a-z_]+,?$`（逗号可选）`41`、`grep -oE` 取标识符 `41` ⇒ 取最懒的那条（逗号改成可选）。**通用形态同前三例**：写门时不只跑一遍期望值，还要跑到**边界元素**上（这里是列表最后一项），"看起来显然"的正则最容易在最后一格漏。**同轮评审又补一刀**：`[a-z_]` 不含数字 ⇒ 将来出现一条带数字的命令名（如 `xxx_v2`）会被静默少数 1 而无人报警，字符类已扩成 `[a-z0-9_]`（控制方实跑仍 **41**）。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(m5): 应用启动自动追平一轮增量（复用 index_job::start 与既有进度事件）"
```

---

### Task 9: `/index` 的 `missing` 三格可见性

**Files:**
- Modify: `src/pages/index-status.tsx`（`STATUS_FILTERS`、项目行计数那三格、文件行 Badge）
- Modify: `src/components/doc-preview-dialog.tsx`（Step 3b：让预览的红色回执把 `hint` 也带上 —— Task 7 评审查到 D10 的前提在这一格落空）
- Test: 无（JSX 不在 `node --test` 那张网里，M4 §八 已定；本轮不新增前端测试文件、不新增依赖）

**Interfaces:**
- Consumes: `index_overview` 已有的 `missing` 计数（T4 之后真的有数了）、`index_docs(projectId, status, limit, offset)` 走参数（`?2 IS NULL` 放开条件）；`src/lib/ipc.ts` 的 `AppErrorShape { code, message, hint }`（`hint` 一直在回线上，只是这一格从不渲染）
- Produces: 界面能列出失效行；预览失败时三条成因在屏幕上分得开。`src/types/index.ts` **不改**（`pending`/`missing` 字段已在）

- [ ] **Step 1: 筛选下拉加一项**

```tsx
const STATUS_FILTERS: { value: string; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "ok", label: "ok（已入索引）" },
  { value: "skipped", label: "skipped（跳过）" },
  { value: "failed", label: "failed（失败）" },
  { value: "missing", label: "missing（磁盘上已不存在）" },
];
```

- [ ] **Step 2: 项目行念出 missing**

`{p.ok} / {p.skipped} / {p.failed}` 三格之后加第四格。**`pending` 不加** —— 它至今无生产写入点，念出来是一格永远为 0 的谎：

```tsx
<span className={p.missing > 0 ? "text-destructive" : ""}>missing {p.missing}</span>
```

- [ ] **Step 3: 失效行的说明（不改配色，并点名与 spec 的偏差）**

读一眼既有渲染就知道：Badge 的**文案就是 `r.status` 本身**（`{r.status}`），所以 `missing` 与 `skipped` 靠文字已经分得开。给 `variant` 加分支只有两种落法，都不值当：把 `missing` 塞进 `destructive` 会和 `failed` 混成「出错了」（而失效不是错误，是磁盘上的事实），再造一个分支就是把 `outline` 换成另一个同样中性的色。所以本步只做一件事：把「为什么它不在结果里」挂成悬浮说明。

```tsx
<Badge
  variant={
    r.status === "ok" ? "secondary" : r.status === "failed" ? "destructive" : "outline"
  }
  title={
    r.status === "missing"
      ? "该文件在磁盘上已不存在，正文已退出检索；文件放回后重跑一轮索引会自动回填这一行"
      : undefined
  }
>
  {r.status}
</Badge>
```

`variant` 那三档**保持原样一字不动**。若 `src/components/ui/badge.tsx` 没把 `...props` 透传到根节点，`npm run build` 会报 `TS2322`：那时把 `title` 挂在这个 `<Badge>` 外面那层 `<li>`（它已有 `className`）上，**别为此改设计系统组件**。

**与 spec 第五节的偏差要点名**：规格里那句「文件行给 `missing` 一个配色分支」按实读判为零价值，落地改成 `title` 一句说明。这句话必须写进报告的「与简报/spec 的偏差」段交评审裁定，不许默默改（HANDOFF 坑 79 同形：改规格文本要走控制方，不是实现者临场发挥）。样式一律 Tailwind 工具类，本轮不新增 `.scss`，也不为 `missing` 新增 Badge variant。

- [ ] **Step 3b: 预览红字把 `hint` 带上（Task 7 评审查到 D10 的前提在这一格落空）**

规格第六节与 D10 的裁定是「不扩错误码，三条成因靠 `hint` 分」，但 `src/components/doc-preview-dialog.tsx` 的 catch 只做了一件事 —— `setError(toAppError(e).message)`，而渲染处也只印这个串。`hint` 在 `src/lib/ipc.ts` 的 `AppErrorShape` 里、在回线上、**从不显示** ⇒ 「行不 `ok`」「文件索引后长大超过上限」「已有一份预览正在抽取」这三条在屏幕上现在是**逐字同一句红字**，D10 那条裁定实际不落地（同仓 `src/components/ledger-panel.tsx:177` 是会渲染 hint 的，有先例可抄）。

改法就一行拼装，**不新增状态格、不动 `error` 的类型、不碰线格式**：

```tsx
      .catch((e) => {
        if (!alive) return;
        const ae = toAppError(e);
        // 三条 preview_unavailable 成因只能靠 hint 分（spec D10）：只印 message 的话它们在屏幕上
        // 逐字相同（Task 7 评审查到的正是这一格）。同仓先例：ledger-panel.tsx:177。
        setError(ae.hint ? `${ae.message}（${ae.hint}）` : ae.message);
      });
```

保持既有语义不动：`alive` 那个 effect 清理守卫不许丢（它是 M4 记过的「预览切档竞态」唯一的运行时守护），原来 `alive && setError(...)` 的短路含义要等价保留。若你更愿意挂成 `title` 悬浮而不是拼进串，二选一落地并在报告里写明选了哪种，**别同时做两份**。

连带影响要记进报告：Task 10 的真机断言 8 从此可以按「hint 首句」判闸门是否真拒（原来只能断 `code`，三条成因在界面上不可分）。

- [ ] **Step 4: 门禁**

```bash
npm run build; echo exit=$?
node --test "tests/**/*.test.ts"; echo exit=$?
grep -rn "_cipher\|_nonce" src/; echo exit=$?
```
Expected: build `exit=0`；node 仍 `ℹ tests 8 / pass 8 / fail 0`；密文列扫描 **exit=1**（作用域限在前端树 —— 这条门不许外推到 `src-tauri`，`db.rs` 的建表语句自己就定义那些列）。

- [ ] **Step 5: 提交**

```bash
git add src/pages/index-status.tsx src/components/doc-preview-dialog.tsx
git commit -m "feat(m5): 索引页显示失效行；预览红字带上 hint（三成因在界面上分得开）"
```

---

### Task 10: 真机验收 + 文档收口（控制方执行；子代理一律不许跑 dev）

**Files:**
- Modify: `docs/开发进度.md`（里程碑表 M5 行、新增「M5 验收证据」整节、缺口归属）
- Modify: `docs/HANDOFF.md`（第 12 版）
- Modify: `docs/技术方案.md`（§6.1 变更检测那一行、§八 M5 行、§九 监听开关那条加 dated 附注）
- 不碰：`.superpowers/sdd/2026-10-07-m4-unified-search/`（被已提交文档按路径引用的证据）

- [ ] **Step 1: 沙盒与红线**

```bash
# 两个必然失败的编译前探针（零成本验掉「JSON 被三层引号咬碎、应用悄悄落到真实 identifier」）
npm run tauri -- dev -c 不存在.json                    # 期望 failed to read configuration file ...(os error 2)
npm run tauri -- dev -c '{"__probe_unknown_key__":1}'   # 期望 Tauri config error: "__probe_unknown_key__" was unexpected
# 正式起沙盒（覆盖只走命令行 -c，绝不为验证改 tauri.conf.json）
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 \
  npm run tauri -- dev -c '{"identifier":"dev.zero.pfm.m5test"}'
```
动手前对真实目录 `%APPDATA%\dev.zero.pfm\` 三文件取 size + mtime + SHA-256 基线，收尾再取一次逐字对照。**别同时跑 `cargo test` / `clippy`**（抢 `target/` 会让 rustc 报内部编译错误，看着像内存问题）。CDP 探针**每步单独一次 evaluate**、只回布尔/条数/错误码名、超时给足，失败先怀疑探针而不是被测（M4 坑 80）；给 sync 命令计时必须在空队列上取（坑 81）。

- [ ] **Step 2: 十二条可判据断言**（读数各落一份 JSON 到 `.superpowers/sdd/2026-10-10-m5-reconcile/`）

夹具一律走应用自己的 IPC（`project_create` → `project_set_root` → `index_start`），**不手拼分词、不裸写 SQL**，文件全落 `%TEMP%`。

1. 启动自动轮跑了：新沙盒冷启动后 `index_overview().running === true`，或 `overview.projects[0].total` 从 0 变非 0。
2. 自动轮不拖慢 IPC：轮进行中并发一次 `db_status`，只回毫秒数。**并发侧还要各量一次「读命令」与「写命令」**（写侧例如 `project_update`）—— Task 8 评审算出最坏形态不是「读延迟量级」而是：主线程命令**持着 `Mutex<Connection>` 等 SQLite 让写锁**，理论上限是 `busy_timeout` 的 5 s。两条毫秒数都要落盘，超线才立 finding。
3. **主承诺**：删一个已索引文件 → 重启 → 该行 `index_status === "missing"`、`search_all` 正文段不命中它、`doc_preview` 回 `preview_unavailable`。
4. 改名 → 重启 → 旧行 missing + 新行 `ok` 各一（照实读数，不当缺陷）。
5. 不可达 root 零误标：某项目 root 指到 `Z:\未挂载\的移动盘` → 重启 → 该项目 `missing` 计数 0 且 `ok` 计数不变。
6. 熔断两侧：25 行项目缺 24 → 一格不标；改成只缺 3 → 标 3。
7. 自愈：把第 3 条删掉的文件放回 → 重启 → 同一 `id` 的行回 `ok` 且正文可搜。
8. I-5：连点两份慢文档 → 第二条**立即** `preview_unavailable`，且 hint 是「已有一份预览正在抽取」那一句。
9. I-4：把已索引 `.txt` 追加到超过上限 → 当场预览即拒（不等下一轮扫描降级）。
10. 顺手量自动轮进行中的一次 `search_all` 毫秒数（对照 M4 的 675–737 ms 静场读数，越线才立 finding）。
11. 撞车提示（Task 8 评审点名的可达窗口）：amber 那句「这不是索引出错，只是上一轮还没结束」**只在「`overview` 还没到货、页面以为空闲」那一个挂载窗口里可达** —— 主防线是两个 start 按钮的 `disabled={running}`。所以这一格要按窗口造前提（在自动轮进行中首次导航到 `/index`），采不到就照实记「采不到 + 当时的 `overview.running` 读数」，别改成「按钮灰了就等于提示过了」。
12. 起轮失败的可观测性（Task 8 评审 I 级的落地形态）：本轮改成「`start` 返回 Err 时写进 `last_error`」。本机造不出 `thread::spawn` 失败（那是 commit charge 耗尽时才出现的，M3/M4 记过 `os error 1455`），所以这一格**只验渲染链不验触发**——把库删到不可打开？不行（那是红线）。正解：只静态核 `index_overview` 透出的 `lastError` 字段在 /index 有整段渲染（`index-status.tsx:361-365`），并在报告里写「触发路径无实测证据，属有名缺口」。

取证顺序纪律：**先做完所有只读断言，再做会改库状态的步骤**（M4 坑 82）。失效行的可见性要真点一次 `/index` 的 `missing` 筛选，那一格只能由人眼或 CDP 读 DOM 确认。

- [ ] **Step 3: 清场**

删沙盒目录与 `%TEMP%` 夹具；按命令行特征回收孤儿进程（vite 1520、`msedgewebview2.exe` 9222）；撤掉 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`；真实目录三行 `stat` 与基线逐字一致；`cargo test --lib` 147 / node pass 8 / 两条 clippy / `npm run build` 全复跑一遍并贴原始退出码。

- [ ] **Step 4: 文档收口（写读数，不写「应该能用了」）**

`docs/开发进度.md`：里程碑表 M5 行改 ✅ 完成 + 门禁读数；新增「M5 验收证据」整节（门禁实跑 / 十二条断言读数 / **没能自动验证清单**：setup 那一行的启动触发无单测、几十 GB 真实语料的整轮耗时、无权限子树误标是否真会发生、`missing` 行长期累积的库体积、前端三格 JSX 渲染、三成因共用一码在界面上分不分得开、`swap` 那个等价变异本机无守护、自动轮进行中首屏检索延迟只量了一次 / **M5 留下的已知缺口**逐条带归属）；M4 那一节不动。
`docs/技术方案.md`：§6.1 变更检测那一行按交付改写（启动自动轮 + 差集回收已做，`notify` 延 M5.5 并写理由）；§八 M5 行内容描述改准；§九 监听开关那条加 dated 附注。**历史规格不 retro-edit**：M3 计划里「`delete_doc` 归 M5 补」「FTS 孤儿行由 M5 启动对账统一回收」两句不动，归属变化与「本轮只兑现一半」的缩量声明只写进 M5 spec 第九节与开发进度。
`docs/HANDOFF.md`：第 12 版（快照时间 / 分支与 HEAD / 门禁基线 / 一句话状态 / 缺口表 / 新坑），M4 那一版的指针表不被覆盖 —— 本文件是「下一个里程碑的恢复地图」，M4 的账要留在原处可查。

- [ ] **Step 5: 提交（逐个文件名）**

```bash
git add docs/开发进度.md docs/技术方案.md docs/HANDOFF.md
git commit -m "docs: M5 收口 —— 十二条真机读数、没能自动验证清单与既有文档回写"
```
不 push、不 merge（每次都需需求方当次指示）。

---

## 完成判据

- `cd src-tauri && cargo test --lib` → **147 passed / 0 failed**，分模块：tokenize 9 / extract 15 / **index_job 19** / **index_store 17** / search 20 / **doc_preview 17** / db 8 / index_scan 10 / ledger 12 / **project 11** / vault 9（数法：从**全量**输出按 `^test <模块>::tests::` 数）。
- `node --test "tests/**/*.test.ts"` → **`ℹ tests 8 / pass 8 / fail 0`**（本轮一条前端测试都没加，所以这条是无回归检查）。
- `cargo clippy --lib -- -D warnings` 与 `cargo clippy --lib --all-targets -- -D warnings` → 两条 **exit=0 / 0 warning**。
- `npm run build` → **exit=0**。
- **线格式没被动的证据**：`SearchBundle` 的 `len() == 7` 与 `DocPreview` 的 `len() == 6` 两条既有断言照旧绿；`generate_handler!` 条数仍 **41**（数法用逗号可选那条正则，见 Task 8 Step 2 的订正说明 —— 要求尾逗号的写法会把最后一项漏掉、恒读 40）。
- 结构门禁（期望 0 命中的那几条，正确结果是 **exit=1**，不是命令失败）：
  - `grep -c "const MISSING_FUSE_MIN_ROWS" src-tauri/src/index_job.rs` → **恰好 1**；`grep -rn "len() > 20" src-tauri/src/` → **exit=1**（当场实跑得出的读数，阈值数字只出现在定义处；常量名在测试注释与断言消息里另有两次点名，那是合规的，见 Task 3 Step 6 的订正说明）。
  - `grep -rn "20971520" src-tauri/src/doc_preview.rs src-tauri/src/lib.rs` → **exit=1**（预览侧与命令侧的上限一律来自 settings；`db.rs:311`/`:416` 与 `index_scan.rs:32` 那三处既有命中是这道上限本该在的地方，不归本门作用域 —— 见 Task 6 Step 6 的订正说明）。
  - `grep -rn "File::open" src-tauri/src/doc_preview.rs` → **exit=1**。
  - `grep -rn "notify" src-tauri/Cargo.toml` → **exit=1**（零新依赖的字面证据）。
  - `grep -c "tauri::command(async)" src-tauri/src/lib.rs` → **恰好 1**。
  - `grep -rn "file_gone" src-tauri/src/` → **exit=1**（D4：`skip_reason` 取值域不扩）。
  - `grep -rn "SET index_status = 'missing'" src-tauri/src/` → **恰好 1 行**（`missing` 的唯一写入点在 `mark_missing`；读侧那句 `<> 'missing'` 不匹配这个模式）。
- 每任务 1 个提交、只含该任务 Files 点名的文件；`git status --porcelain` 干净。
- `%APPDATA%\dev.zero.pfm\` 三文件与真机基线逐字一致；`%TEMP%` 无本轮残留；一次性沙盒 `dev.zero.pfm.m5test` 已删净。
