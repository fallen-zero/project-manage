# M15 多工作区分类隔离 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在同一份 SQLite 库内把工作区（按客户 / 项目类型）做成后端强制的逻辑分区：顶栏一个切换器决定当前视野，项目及其关联数据（待办 / 里程碑 / 到期 / 日报 / 台账 / 正文索引 / 操作日志）按区隔离，根项目换区时整棵子树随迁。

**Architecture:** V11 加 `workspaces` 表 + `projects.workspace_id`（`NOT NULL DEFAULT 'default'`，存量行由 `ADD COLUMN` 语义自动回填）+ `settings.active_workspace_id` 一个当前区。新增 `src-tauri/src/workspace.rs` 作为「谁是活动区」的唯一权威：读路径在自己的 SQL 里带区谓词，写路径在入口调 `ensure_active_*`，前端只是切换器的触发者。换区跟随 —— 移动子树时同一事务里把活动区写成目标区，保证命令返回值仍在当前视野内可读（spec §六.4）。

**Tech Stack:** Rust / rusqlite 0.40.2（bundled；`uuid`、`serde_json` 已在树里）/ Tauri 2 同步命令 / React 19 + TypeScript / zustand / shadcn-radix / `node --test`。

**Spec:** `docs/superpowers/specs/2026-10-23-m15-workspaces-design.md`（本计划从它论证，两份一起读）

## Global Constraints

- 零新依赖：`src-tauri/Cargo.lock` 的 `[[package]]` 数保持 **616**；`git diff --stat master..HEAD -- src-tauri/Cargo.lock src-tauri/Cargo.toml package.json` 必须空输出。
- 结构门终态：`generate_handler!` **75**（现 70，+5）、`tauri::command(async)` **保持 7**（五条新命令一律同步）、`allow(dead_code)` **保持 1**（`db.rs`）、`hits.append(&mut collect(` **保持 9**、`TARGET_VERSION` **11**（现 10）。
- 错误码只用既有的 `invalid_input` / `not_found` / `db_failed`：**不新增码名**。跨区与「不存在」一律回 `not_found`（spec §四）。
- SQL 一律参数绑定。唯一允许的字符串拼接是 `&'static str` 常量表名（与 `ledger.rs:212 soft_delete` 同形）和 `IN (?,?,…)` 占位符。
- 注释、用户文案、commit message 一律中文；错误码字面量与 SQL 除外。
- **`allow(dead_code)` 门 = 1（`db.rs`）的临时破例与回收路径**（Task 2 评审抓到，控制方裁定）：Task 2 要把 `workspace.rs` 四个读侧函数与 `project.rs` 两个新字段先落地（后五格都靠它们），而它们在 Task 3–6 之前没有**非测试**调用点，clippy `-D warnings` 就过不去 ⇒ 本分支的豁免数临时从 1 升到 7。裁定为「保留临时豁免 + 逐格回收」，不做「把每个函数挪到它的消费格」的重构：重构要改五格计划文本、新缺陷概率高于收益，而这个数由 Task 9 Step 1 的门自动兜住。硬要求：
  1. **Task 5 Step 4** 落地 `ledger::list`/`ledger::reveal` 的调用点后删掉 `ensure_active_project` 的豁免（Task 3 报告实测：该函数在 Task 3 内没有生产调用点，当场删掉撞 clippy `--lib -D warnings` exit 101；裁定只改回收格、不在 Task 3 里造调用点）；**Task 3 Step 2** 之后 `Project.workspace_id` 被 SELECT 真读，删掉它的豁免；
  2. **Task 4 Step 4** 落地 `name_of` 时**不带**豁免（同格就被 `workspace_name` 消费）、并在本格删掉 `ProjectInput.workspace_id` 的豁免（本格起读它）；
  3. **Task 5 Step 1** 起 `ensure_active_row` 有调用点，删它的豁免；**Task 6 Step 4** 起 `active_project_ids` 有调用点，删它的豁免；
  4. 到 Task 9 Step 1 现跑 `grep -rn "allow(dead_code)" src-tauri/src/ | wc -l` 必须回到 **1**；没回收干净就是那一格的红，不要去改门的期望值。
  另：注释与文案里**不许出现** `allow(dead_code)` 这个方括号字面量（`grep -rn` 会把注释也数进去，与 async 门那条同款坑）—— 要指它时写「dead_code 豁免」。
- 基线（2026-10-23 在本分支实测，非引用）：`cargo test -j 1 --lib` **372 passed / exit=0**、仓库根 `node --test "tests/**/*.test.ts"` **tests 71 / pass 71 / fail 0**、命令 **70** / async **7** / `TARGET_VERSION 10` / 豁免 **1** / `Cargo.lock` **616** / collect **9**。
- 每个 Task 结尾先跑本 Task 覆盖的 target，再跑全量 `cargo test -j 1 --lib`（前端 Task 跑 `npm run build`），**拿到真实退出码后**才提交。`cargo test` 只接一个筛选词。
- 子代理**一律不跑** `npm run tauri dev` / `npm run dev`；真机格与变异取证由控制方跑。真实数据目录 `%APPDATA%\dev.zero.pfm\` 的 `ledger.db`/`-wal`/`-shm` 是只读红线，真机走一次性 compile-time identifier 沙盒。
- 同一时刻只允许一个会提交的处理者（同 worktree 串行）；只读评审可并行。
- 新建文件（含 `.md`）后跑 `unix2dos`：本仓工作树是 CRLF、索引是 LF（`core.autocrlf=true`）。
- 不新建 Skill、不改 `AGENTS.md`、不 stage `Python/`。

## 文件结构（本轮全部落点）

**新建**

| 文件 | 单一职责 |
|------|---------|
| `src-tauri/src/workspace.rs` | 活动区的唯一权威 + 工作区 CRUD + 跨区守卫（`active_id`/`ensure_active_*`/`name_of`/`active_project_ids`） |
| `src/types/workspace.ts` | 前端 `Workspace` 线格式（与 Rust camelCase 逐字段对齐） |
| `src/lib/workspace-view.ts` | 前端可测的三件纯函数：`deleteGuard` / `nameHint` / `activeWorkspaceId` |
| `src/stores/workspaces.ts` | 区列表与活动区 id 的唯一状态源（zustand） |
| `src/components/workspace-manager.tsx` | 「管理工作区」弹窗（新建 / 改名 / 删除） |
| `tests/workspace-view.test.ts` | 上述纯函数的 `node --test` 守护 |

**修改（按 Task 归属）**

- Task 1：`src-tauri/src/db.rs`（`TARGET_VERSION`、迁移 match、`V11`、四条测试、两条既有断言）、`src-tauri/src/backup.rs`（告警句、两处 schema 字面断言、一条新测试）、`src-tauri/src/lib.rs`（bootstrap 测试注释 `:1087`）
- Task 2：`src-tauri/src/workspace.rs`、`src-tauri/src/lib.rs`（`mod workspace;` + 五条命令 + `generate_handler!`）
- Task 3：`src-tauri/src/project.rs`（`Project`/`ProjectInput` 字段、`list_projects`/`get_project` 谓词、`create_project` 归属、`validate_parent` 同区）、五处 `ProjectInput` 测试字面量（`project.rs:546`、`project_audit.rs:305`、`ledger.rs:690`、`ledger_export.rs:440`、`search.rs:406`）
- Task 4：`src-tauri/src/project.rs`（`update_project` 换区 + `collect_subtree` + 跟随）、`src-tauri/src/project_audit.rs`（`workspaceId` 进差异、`name_of` 引用、`list`/`project_options` 区过滤）
- Task 5：`todo.rs`/`milestone.rs`/`expiry.rs`/`daily_report.rs`（`project_exists`、`list`/`get`、mutator 守卫、`load_projects`/`existing_keys`）、`ledger.rs`（`project_exists`/`soft_delete`/五个 `update_*`/`list`/`reveal`）、`project.rs`（三个目录 mutator）
- Task 6：`search.rs`（`collect` 签名 + 九段 SQL）、`index_store.rs`（`SELECT_SQL`、`indexed_project_count`、`list_docs`）、`doc_preview.rs`（`row_for_preview`）、`lib.rs`（`index_overview`、`ledger_export`）
- Task 7：`src/types/project.ts`、`src/lib/api.ts`、`src/stores/workspaces.ts`、`src/lib/workspace-view.ts`、`tests/workspace-view.test.ts`
- Task 8：`src/components/app-shell.tsx`、`src/components/workspace-switcher.tsx`（新建）、`src/components/workspace-manager.tsx`、`src/components/project-form.tsx`、`src/pages/projects.tsx`、`src/pages/project-detail.tsx`、`src/stores/projects.ts`（只读注释，不改逻辑）
- Task 9：无代码改动（门禁 / 变异 / 真机 / 映射表，控制方跑）
- Task 10：`docs/技术方案.md`、`docs/开发进度.md`、`docs/HANDOFF.md`

**测试条数账（逐 target 数实数，不是凭印象累加）**

| Task | 新增 cargo | 累计 cargo | 新增 node | 累计 node |
|------|-----------|-----------|----------|----------|
| 基线 | — | 372 | — | 71 |
| 1 | +5（`db.rs` 4 + `backup.rs` 1） | 377 | — | 71 |
| 2 | +7（`workspace.rs`） | 384 | — | 71 |
| 3 | +3（`project.rs`） | 387 | — | 71 |
| 4 | +7（`project.rs` 5 + `project_audit.rs` 2；其中 M3/M6 两条由 Task 4 评审补上） | 394 | — | 71 |
| 5 | +7（四模块 4 + `ledger.rs` 1 + 目录 1 + 日报导入 1） | 401 | — | 71 |
| 6 | +3（`search.rs` 1 + `index_store.rs` 1 + 命令层 1） | 404 | — | 71 |
| 7 | — | 404 | +4 | 75 |
| 8 | — | 404 | 0（UI 无测试先例） | 75 |

改名不增数：Task 1 把既有 `v10_schema_meta_reaches_target_version_ten` 改名为 `v11_..._eleven`，条数不变。**如果实现者在本格数出的实数与上表不符，以运行为准并把差异写进账本，不要顺着表改测试。**

---

## Task 1: V11 迁移 + 备份 schema/告警

**Files:**
- Modify: `src-tauri/src/db.rs:98`（`TARGET_VERSION`）、`:118-130`（match 链）、`:461` 之后（新增 `V11` 常量）、`:730-738`（既有 `v10_schema_meta...`）、`:794-806`（既有 `v9_projects_parent_id_and_index_exist` 的索引向量）、测试模块末尾（新增四条）
- Modify: `src-tauri/src/backup.rs:23`（`warn_plaintext_columns` 那句）、`:868`、`:895`（两处 `assert_eq!(sum.schema_version, 10)`）、`:665` 那条告警测试、`:900` 附近新增一条测试
- Modify: `src-tauri/src/lib.rs:1087`（bootstrap 测试注释「链头已随 M14 V10 走到 10」）

**Interfaces:**
- Consumes: 无（本 Task 是链头）
- Produces: `workspaces(id TEXT PK, name TEXT NOT NULL UNIQUE, created_at TEXT)`、`projects.workspace_id TEXT NOT NULL DEFAULT 'default'`、`settings` 里恒有一行 `active_workspace_id`、`db::TARGET_VERSION == 11`、`crate::db::V11` 常量。Task 2/3 之后所有区谓词的前提。

- [ ] **Step 1: 写 V11 的失败测试（表 + 列）**

在 `db.rs` 的 `mod tests` 里、紧跟 `v10_backfills_nothing`（`:772-778`）之后加：

```rust
    /// V11 必须建出 `workspaces`（三列、顺序固定）并给 `projects` 加 `workspace_id` 列。
    /// 列顺序变了要同步 `workspace.rs` 的 SELECT；本仓的形制是用这条断言把线格式钉住。
    #[test]
    fn v11_workspaces_table_and_projects_column_exist() {
        let conn = open_in_memory().unwrap();
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('workspaces') ORDER BY cid")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(cols.join(","), "id,name,created_at", "V11 的 workspaces 列集合/顺序不对：{cols:?}");

        let pcols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('projects')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert!(
            pcols.iter().any(|c| c == "workspace_id"),
            "V11 必须给 projects 加 workspace_id 列：{pcols:?}"
        );
    }
```

- [ ] **Step 2: 跑测试确认它红在「没有 workspaces 表」**

Run: `cd src-tauri && cargo test -j 1 --lib v11_workspaces_table_and_projects_column_exist`
Expected: FAIL，panic 文案是 `V11 的 workspaces 列集合/顺序不对：[]`（`pragma_table_info` 对不存在的表返回空集，不是报错）。**如果它红成别的样子（比如 SQL 报错），说明测试写错了，先修测试再往下。**

- [ ] **Step 3: 写 V11 的 DDL 与链头**

在 `db.rs:461`（`V10` 结束的那个 `";` 行）之后插入：

```rust
/// v11：多工作区（M15 spec §三）。一张 `workspaces` + `projects.workspace_id` + 一个当前区设置行。
/// 顺序是刻意的：先建表并种默认区，再给 projects 加带 DEFAULT 的列 —— 这样「存量项目回填到默认区」
/// 由 SQLite 的 `ADD COLUMN` 语义天然完成，迁移里不写一句 UPDATE。
/// `settings` 用 `INSERT OR IGNORE`：它是 V1 就有的表，键位可能已被备份导入带进来，冲突必须静默跳过。
/// `workspaces` 用裸 `INSERT`：表刚建出来必然为空，写 `OR IGNORE` 反而会把「默认区被人为删过」吞掉。
const V11: &str = "
CREATE TABLE workspaces (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO workspaces (id, name) VALUES ('default', '默认工作区');

ALTER TABLE projects ADD COLUMN workspace_id TEXT NOT NULL DEFAULT 'default';

CREATE INDEX ix_project_workspace ON projects(workspace_id);

INSERT OR IGNORE INTO settings (key, value) VALUES ('active_workspace_id', 'default');
";
```

并把 `:98` 与 match 链各改一处：

```rust
pub(crate) const TARGET_VERSION: i64 = 11;
```

```rust
            10 => V10,
            11 => V11,
            _ => "",
```

- [ ] **Step 4: 跑 Step 1 的测试到绿，并当场收两处既有断言**

Run: `cd src-tauri && cargo test -j 1 --lib v11_`
Expected: `v11_workspaces_table_and_projects_column_exist` PASS；同时 `v9_projects_parent_id_and_index_exist` 与 `v10_schema_meta_reaches_target_version_ten` **必须转红** —— 它们是本轮已知的连带断言，红的方向就是下面两处要改的地方。跑 `cargo test -j 1 --lib` 全量确认红的只有这两条：

- `db.rs:731` 那条测试连名字一起改（名字里的数字会骗人）：

```rust
    #[test]
    fn v11_schema_meta_reaches_target_version_eleven() {
        let conn = open_in_memory().unwrap();
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 11, "迁移必须跑到 11");
        assert_eq!(TARGET_VERSION, 11, "= 上一条，两处别各写各的");
    }
```

- `db.rs:805` 那条索引向量加第二项（`ORDER BY name` 下 `idx_…` 排在 `ix_…` 前）：

```rust
        assert_eq!(
            idx,
            vec!["idx_projects_parent_id", "ix_project_workspace"],
            "projects 上的显式索引只有这两条：{idx:?}"
        );
```

- [ ] **Step 5: 回填测试（存量项目自动落默认区，业务列不动）**

```rust
    /// V11 的回填不是 UPDATE：先把库手工建到 V10、落一个带标签的存量项目，再跑 `migrate`。
    /// 存量行必须由 `ADD COLUMN … DEFAULT 'default'` 自动落进默认区，其余列一个字节都不许变
    /// （spec §三 第 1 条 + D2「升级后打开应用看到的必须和升级前一模一样」）。
    /// 这一条同时是「旧备份包导入」那半段的判据：`backup.rs:556` 导入收尾调的就是这个 `migrate`。
    #[test]
    fn v11_backfills_existing_projects_into_default() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_meta (version INTEGER NOT NULL, applied_at TEXT NOT NULL);",
        )
        .unwrap();
        for (target, ddl) in [
            (1, V1),
            (2, V2),
            (3, V3),
            (4, V4),
            (5, V5),
            (6, V6),
            (7, V7),
            (8, V8),
            (9, V9),
            (10, V10),
        ] {
            apply_version(&conn, target, ddl).unwrap();
        }
        conn.execute(
            "INSERT INTO projects (id, name, customer) VALUES ('p-old', '存量项目', '甲客户')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_tags (id, project_id, tag) VALUES ('t-old', 'p-old', '政务')",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap();

        let ws: String = conn
            .query_row("SELECT workspace_id FROM projects WHERE id = 'p-old'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ws, "default", "存量项目必须回填进默认区");
        let bits: String = conn
            .query_row(
                "SELECT name || '/' || customer FROM projects WHERE id = 'p-old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bits, "存量项目/甲客户", "V11 只许动 workspace_id 一列");
        let tag: String = conn
            .query_row("SELECT tag FROM project_tags WHERE id = 't-old'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tag, "政务", "标签行不该被迁移碰");
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, TARGET_VERSION);
    }
```

Run: `cd src-tauri && cargo test -j 1 --lib v11_backfills_existing_projects_into_default`
Expected: PASS（Step 3 已做完，这条是补断言不是补功能）。

- [ ] **Step 6: 种子测试（默认区一行 + 当前区设置行）**

```rust
    /// V11 必须种出「有且仅有一行默认区」+「`active_workspace_id = default`」。
    /// 这两行是 §四 `active_id` 与 §五 三道删除守卫的地基：默认区被种成别的名字，
    /// 顶栏就会多出一个空区；设置行没种上，首屏就走进 `active_id` 的兜底分支（那是兜底不是主路径）。
    #[test]
    fn v11_seeds_default_workspace_and_active_setting() {
        let conn = open_in_memory().unwrap();
        let rows: Vec<(String, String)> = conn
            .prepare("SELECT id, name FROM workspaces ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![("default".to_owned(), "默认工作区".to_owned())],
            "V11 只该种默认区：{rows:?}"
        );
        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'active_workspace_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(value, "default", "当前区必须种成默认区");
    }
```

- [ ] **Step 7: 原子性测试（V11 半途失败整段回滚）**

```rust
    /// V11 一句 DDL 失败时，前面已经建好的 `workspaces` 与加好的列必须一起退回去，
    /// `schema_meta` 也不许登记 11（否则下次开机跳过 V11，留下半拉库起不来 —— `db.rs:136-144`
    /// 那段注释写的就是这个坑）。判据形状沿用 `apply_version` 那条 dup-CREATE 探针。
    #[test]
    fn v11_is_atomic_when_a_later_statement_fails() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_meta (version INTEGER NOT NULL, applied_at TEXT NOT NULL);",
        )
        .unwrap();
        for (target, ddl) in [
            (1, V1),
            (2, V2),
            (3, V3),
            (4, V4),
            (5, V5),
            (6, V6),
            (7, V7),
            (8, V8),
            (9, V9),
            (10, V10),
        ] {
            apply_version(&conn, target, ddl).unwrap();
        }
        let broken = format!("{V11}\nCREATE TABLE v11_probe(x);\nCREATE TABLE v11_probe(x);");

        assert!(apply_version(&conn, 11, &broken).is_err(), "重复建表必须报错");

        let tables: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'workspaces'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tables, 0, "V11 半途失败必须连已经建好的 workspaces 一起回滚");
        let cols: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('projects') WHERE name = 'workspace_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cols, 0, "加到一半的列也必须退回去");
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 10, "失败的版本一律不许登记 schema_meta");
    }
```

- [ ] **Step 8: 备份侧 —— 告警句、两处 schema 字面数、一条新包测试**

`backup.rs:23` 那句在末尾「项目修改日志的字段旧值/新值（含联系人、联系方式）」之前插入「工作区名称、」，改完整句是：

```rust
    "包内非 *_cipher 列为明文：项目名/编号/负责人/客户/联系人/合同号（含项目模板的同类字段）、工作区名称、服务器 ip 与堡垒机、各类 url、各表 note、待办内容、里程碑备注、日报内容、到期事项名称与备注、项目修改日志的字段旧值/新值（含联系人、联系方式）。"
```

`backup.rs:665` 那条 `plaintext_warning_names_every_plain_text_table_since_m9` 里，紧跟既有的 `assert!(warn.contains("里程碑备注"), …)` 之后加：

```rust
        // M15 的第四处明文命名：区名是按客户起的，「客户乙」这种就是业务命名。
        assert!(warn.contains("工作区名称"), "漏了工作区名称的明文：{warn}");
```

`backup.rs:868` 与 `backup.rs:895` 两处 `assert_eq!(sum.schema_version, 10, …)` 改成 11（前者的文案一起改）：

```rust
        assert_eq!(sum.schema_version, 11, "M15 起包内 schema 必须是 11");
```

```rust
        assert_eq!(sum.schema_version, 11);
```

在 `backup_package_carries_project_audit_logs_without_counting_them` 之后加一条（形制照它）：

```rust
    /// M15 的备份半边：工作区行随整库快照进包，但 Counts 仍是七张表（沿用 M12/M13/M14 的裁定）。
    #[test]
    fn backup_package_carries_workspaces_without_counting_them() {
        let live = crate::db::open_in_memory().unwrap();
        live.execute("INSERT INTO workspaces (id, name) VALUES ('ws-2', '客户乙')", [])
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("workspaces.zip");
        let sum = export(&live, &dst).unwrap();
        assert_eq!(sum.schema_version, crate::db::TARGET_VERSION);
        assert_eq!(sum.schema_version, 11, "M15 起包内 schema 必须是 11");

        let (_unpacked, got) = unpack_db(&dst);
        let names: Vec<String> = got
            .prepare("SELECT name FROM workspaces ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(names, vec!["默认工作区", "客户乙"], "区行必须随整库快照进包：{names:?}");
        let keys = serde_json::to_value(&sum.counts).unwrap();
        assert_eq!(
            keys.as_object().unwrap().len(),
            7,
            "workspaces 是新表，不进 Counts：{keys:?}"
        );
    }
```

- [ ] **Step 9: `lib.rs:1087` 的注释跟着链头改字**

```rust
        assert_eq!(ver, crate::db::TARGET_VERSION, "= db.rs 的 TARGET_VERSION，链头已随 M15 V11 走到 11");
```

- [ ] **Step 10: 全量 cargo + 结构门读数**

Run: `cd src-tauri && cargo test -j 1 --lib`
Expected: `test result: ok. 377 passed; 0 failed`（基线 372 + 本 Task 新增 5；改名那条不增减）。
Run: `cd src-tauri && grep -c "TARGET_VERSION: i64 = 11" src/db.rs`
Expected: `1`。

- [ ] **Step 11: 提交**

```bash
git add src-tauri/src/db.rs src-tauri/src/backup.rs src-tauri/src/lib.rs
git commit -m "feat(m15): V11 工作区表 + projects.workspace_id 默认区回填 + 包内 schema 11"
```

---

## Task 2: `workspace.rs` 模块 + 五条命令

**Files:**
- Create: `src-tauri/src/workspace.rs`
- Modify: `src-tauri/src/lib.rs:24` 之后（`mod workspace;`）、`audit_list`（`:161-171`）之后（五条命令体）、`generate_handler!` 末尾（`audit_list` → `audit_list, workspace_list, workspace_create, workspace_rename, workspace_delete, workspace_switch`）

**Interfaces:**
- Consumes: Task 1 的 `workspaces` 表、`projects.workspace_id`、`settings.active_workspace_id`；既有 `crate::error::{AppError, AppResult}`；`uuid::Uuid::new_v4()`（同 `project.rs:173`）。
- Produces（Task 3–8 全部靠这组签名）：

```rust
pub const DEFAULT_ID: &str;                       // "default"
pub const ACTIVE_KEY: &str;                       // "active_workspace_id"
pub struct Workspace { pub id: String, pub name: String, pub project_count: i64, pub is_active: bool }
pub fn active_id(conn: &Connection) -> AppResult<String>;
pub fn set_active(conn: &Connection, id: &str) -> AppResult<()>;
pub fn exists(conn: &Connection, id: &str) -> AppResult<bool>;
pub fn name_of(conn: &Connection, id: &str) -> AppResult<String>;
pub fn list(conn: &Connection) -> AppResult<Vec<Workspace>>;
pub fn create(conn: &Connection, name: &str) -> AppResult<Workspace>;
pub fn rename(conn: &Connection, id: &str, name: &str) -> AppResult<Workspace>;
pub fn delete(conn: &Connection, id: &str) -> AppResult<()>;
pub fn switch(conn: &Connection, id: &str) -> AppResult<String>;
pub fn ensure_active_project(conn: &Connection, project_id: &str) -> AppResult<()>;
pub fn ensure_active_row(conn: &Connection, table: &'static str, id: &str) -> AppResult<()>;
pub fn active_project_ids(conn: &Connection) -> AppResult<std::collections::HashSet<String>>;
```

`set_active` 与 `active_project_ids` 是给 Task 4（换区跟随）和 Task 6（`index_overview` 过滤）用的；`name_of` 是给 Task 4 的日志字段用的。

- [ ] **Step 1: 建模块（先只放读侧三件 + 类型，测试才有对象）**

新建 `src-tauri/src/workspace.rs`：

```rust
//! 工作区（M15）。一张 `workspaces` + `settings.active_workspace_id` 一个当前区。
//! **隔离是逻辑分区，不是访问控制**：同库同进程，拿到 `ledger.db` 的人看得到全部区
//! （spec §十二.6 要求这句写在模块头，别让「隔离」两个字被读成加密）。

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashSet;

use crate::error::{AppError, AppResult};

/// 默认区的 id 与显示名：V11 种的就是这两行，`projects.workspace_id` 的列 DEFAULT 也是这个 id。
pub const DEFAULT_ID: &str = "default";
pub const ACTIVE_KEY: &str = "active_workspace_id";
/// 区名要进顶栏那个下拉，超长会把 header 撑破。只在后端判一次（本项目只在边界校验一次）。
const NAME_MAX_CHARS: usize = 40;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// 只数未软删的项目：管理弹窗里那个数字是「删不删得掉」的判据之一，
    /// 而 §五 的删除守卫**含软删行**，两者口径不同是刻意的（软删行仍占着区，不能让区先删掉）。
    pub project_count: i64,
    pub is_active: bool,
}

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查表单必填项与取值范围"))
}

/// 跨区与不存在共用这一条：一个 crafted id 得到的答复和 id 打错完全一样（spec §四）。
/// hint 里点名工作区，因为界面最常见的真成因就是「刚切过区」。
fn not_found(what: &str, id: &str) -> AppError {
    AppError::new(
        "not_found",
        &format!("{what}不存在：{id}"),
        Some("刷新后重试；如果刚切换过工作区，这条数据在别的工作区里"),
    )
}

/// 当前活动区 id。
pub fn active_id(conn: &Connection) -> AppResult<String> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [ACTIVE_KEY],
            |r| r.get(0),
        )
        .optional()?;
    // 读不到只可能是有人手工 `DELETE FROM settings`（V11 恒种这一行）。这里报错会让每条命令
    // 都失败、整个界面打不开；回默认区至少还看得见数据（spec §4.3，T-5 钉这一支）。
    Ok(value.unwrap_or_else(|| DEFAULT_ID.to_owned()))
}

/// 把当前区写成 `id`。**不校验存在性**：调用方分两类 —— `switch` 自己先查过（§五），
/// Task 4 的换区跟随传的是它刚用 `exists` 查过的目标区。这一层是写，不是门。
pub fn set_active(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![ACTIVE_KEY, id],
    )?;
    Ok(())
}

pub fn exists(conn: &Connection, id: &str) -> AppResult<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM workspaces WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}

/// 区名，给操作日志的「工作区」字段用（spec §七：日志里存显示名，不存裸 id）。
pub fn name_of(conn: &Connection, id: &str) -> AppResult<String> {
    let name: Option<String> = conn
        .query_row("SELECT name FROM workspaces WHERE id = ?1", [id], |r| r.get(0))
        .optional()?;
    // 有项目的区删不掉（§五 守卫③），所以这一支在命令路径上实际不可达；写它是为了让
    // 日志的 changes 永远不含裸 id。形制照 `project_audit.rs:72 parent_name` 的「已删除项目」。
    Ok(name.unwrap_or_else(|| "已删除工作区".to_owned()))
}

/// 断言项目属于活动区且未软删。所有「按 project_id 挂新数据」的 create 路径靠它（spec §4.2）。
pub fn ensure_active_project(conn: &Connection, project_id: &str) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects
          WHERE id = ?1 AND deleted_at IS NULL AND workspace_id = ?2",
        params![project_id, active_id(conn)?],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(not_found("项目", project_id));
    }
    Ok(())
}

/// 断言某一行的宿主项目属于活动区。`table` 只接编译期常量（与 `ledger.rs:212 soft_delete`
/// 同一种「表名由调用方以常量传入」的写法），九张表都有 `project_id` 与 `deleted_at` 两列。
/// **不加 `p.deleted_at IS NULL`**：spec §四 的 SQL 就是这个形状，软删项目下的行仍可靠地
/// 属于那个区，多半个谓词只会让读路径与写路径口径分叉。
pub fn ensure_active_row(conn: &Connection, table: &'static str, id: &str) -> AppResult<()> {
    let n: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM {table} t
               JOIN projects p ON p.id = t.project_id
              WHERE t.id = ?1 AND t.deleted_at IS NULL AND p.workspace_id = ?2"
        ),
        params![id, active_id(conn)?],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(not_found("记录", id));
    }
    Ok(())
}

/// 活动区里**含软删**的项目 id 集合。给 `index_overview` 过滤用（spec §4.1：
/// worker 照旧全库扫，只有呈现层按区收窄）。
pub fn active_project_ids(conn: &Connection) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM projects WHERE workspace_id = ?1")?;
    let ids = stmt
        .query_map(params![active_id(conn)?], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}
```

- [ ] **Step 2: 写第一条失败测试（活动区兜底）**

在 `workspace.rs` 末尾加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, project};
    use rusqlite::Connection;

    /// 活动区兜底 + 守卫能不能真的挡：这一支同时钉 §4.3（读不到设置行回 default）
    /// 与 §4.2（跨区项目读不到）。设置行被删掉时如果 `active_id` 报错，整条命令链全挂。
    #[test]
    fn active_id_defaults_when_the_setting_row_is_gone() {
        let conn = db::open_in_memory().unwrap();
        assert_eq!(active_id(&conn).unwrap(), DEFAULT_ID, "V11 种好后直接读要得 default");

        conn.execute("DELETE FROM settings WHERE key = ?1", [ACTIVE_KEY])
            .unwrap();
        assert_eq!(active_id(&conn).unwrap(), DEFAULT_ID, "设置行被手工删掉时必须兜回默认区");

        let p = project::create_project(&conn, &project_input("兜底项目")).unwrap();
        assert_eq!(p.workspace_id, DEFAULT_ID);
        ensure_active_project(&conn, &p.id).unwrap();
    }

    fn project_input(name: &str) -> project::ProjectInput {
        project::ProjectInput {
            name: name.into(),
            code: None,
            manager: None,
            customer: None,
            contact_name: None,
            contact_phone: None,
            contract_no: None,
            contract_period: None,
            delivery_deadline: None,
            status: "立项".into(),
            parent_id: None,
            workspace_id: None,
            tags: vec![],
        }
    }
}
```

Run: `cd src-tauri && cargo test -j 1 --lib workspace::tests::active_id_defaults`
Expected: **编译失败**（`project::ProjectInput` 还没有 `workspace_id` 字段、`Project` 也没有 `workspace_id`）—— 这正是本 Task 与 Task 3 的接缝。为了让本 Task 独立可跑，**在本 Step 顺手把 Task 3 要加的两个字段先加上**（`Project` 加 `pub workspace_id: String`、`ProjectInput` 加 `#[serde(default)] pub workspace_id: Option<String>`），并把 `project.rs:272`、`project.rs:368` 两处 `Project { … }` 字面量与 5 处测试字面量补上字段（值先取 `"default"`、`None`），使编译通过；Task 3 再把读/写语义接上。**这是刻意的跨 Task 前置，账本里记一句「Task 2 落了字段声明，Task 3 落语义」。**

Expected（补字段后）: PASS。

- [ ] **Step 3: CRUD 四条测试 + 实现（逐个 RED→GREEN，不一次写完）**

测试（全部加在 `mod tests` 里，紧接 Step 2 那条之后）：

```rust
    #[test]
    fn create_rejects_blank_duplicate_and_overlong_names() {
        let conn = db::open_in_memory().unwrap();
        assert_eq!(create(&conn, "   ").unwrap_err().code, "invalid_input", "空名要应用层码，不是 db_failed");
        assert_eq!(create(&conn, "客户乙").unwrap().name, "客户乙");
        // 显示值要去空白：区名进顶栏下拉，前后空格会让人以为是两个区。
        assert_eq!(create(&conn, "  客户丙  ").unwrap().name, "客户丙");
        assert_eq!(create(&conn, "客户乙").unwrap_err().code, "invalid_input", "重名要回人话，不是 UNIQUE 撞出来的 db_failed");
        let long = "区".repeat(NAME_MAX_CHARS + 1);
        assert_eq!(create(&conn, &long).unwrap_err().code, "invalid_input");
        assert!(create(&conn, &"区".repeat(NAME_MAX_CHARS)).is_ok(), "{NAME_MAX_CHARS} 字是闭区间");
    }

    #[test]
    fn rename_and_switch_and_delete_on_unknown_id_are_not_found() {
        let conn = db::open_in_memory().unwrap();
        for err in [
            rename(&conn, "ws-ghost", "新名字").unwrap_err(),
            switch(&conn, "ws-ghost").unwrap_err(),
            delete(&conn, "ws-ghost").unwrap_err(),
        ] {
            assert_eq!(err.code, "not_found", "未知 id 一律 not_found：{:?}", err.code);
        }
    }

    #[test]
    fn switch_writes_and_reads_back_and_is_idempotent() {
        let conn = db::open_in_memory().unwrap();
        let ws = create(&conn, "客户乙").unwrap();
        assert_eq!(switch(&conn, &ws.id).unwrap(), ws.id);
        assert_eq!(active_id(&conn).unwrap(), ws.id, "切换要真的落到 settings 并被读回");
        assert_eq!(switch(&conn, &ws.id).unwrap(), ws.id, "重复切同一个区幂等成功");
        assert_eq!(switch(&conn, DEFAULT_ID).unwrap(), DEFAULT_ID);
        assert_eq!(active_id(&conn).unwrap(), DEFAULT_ID);

        // `create`/`rename` 的返回值走 `row()`，那一支的 is_active 与 `list()` 是两处 SQL，各钉一次
        // （Task 2 评审点名 `row()` 的 is_active 无测试）。
        assert!(!create(&conn, "新建未激活区").unwrap().is_active, "新建区不该是活动区（row() 那一支）");
        let cur = create(&conn, "当前改名区").unwrap();
        switch(&conn, &cur.id).unwrap();
        assert!(
            rename(&conn, &cur.id, "改名后仍活动").unwrap().is_active,
            "改名要读回当前区标记（row() 那一支）"
        );
    }

    #[test]
    fn delete_refuses_default_active_and_non_empty_workspaces() {
        let conn = db::open_in_memory().unwrap();
        // ①默认区：`projects.workspace_id` 的列 DEFAULT 指向它，删掉等于给孤儿列找不着家。
        assert_eq!(delete(&conn, DEFAULT_ID).unwrap_err().code, "invalid_input");
        assert!(exists(&conn, DEFAULT_ID).unwrap());

        // ③非空区：守卫含软删行 —— 软删项目仍占着 workspace_id，先删区会让它的日志从所有视野消失。
        let ws = create(&conn, "有项目的区").unwrap();
        let p = project::create_project(&conn, &project_input("区内项目")).unwrap();
        conn.execute(
            "UPDATE projects SET workspace_id = ?1 WHERE id = ?2",
            params![ws.id, p.id],
        )
        .unwrap();
        assert_eq!(delete(&conn, &ws.id).unwrap_err().code, "invalid_input");
        // 软删走裸 SQL，不用 `project::delete_project`：Task 3 之后 `get_project` 按活动区过滤，
        // 这个项目的区是「有项目的区」而活动区还是默认区，走命令路径会先 not_found，守卫③那一支就测不到。
        conn.execute(
            "UPDATE projects SET deleted_at = datetime('now') WHERE id = ?1",
            [p.id],
        )
        .unwrap();
        assert_eq!(delete(&conn, &ws.id).unwrap_err().code, "invalid_input", "软删行也算非空");

        // ②当前区：切走才让删。
        let empty = create(&conn, "空区").unwrap();
        switch(&conn, &empty.id).unwrap();
        assert_eq!(delete(&conn, &empty.id).unwrap_err().code, "invalid_input");
        switch(&conn, DEFAULT_ID).unwrap();
        delete(&conn, &empty.id).unwrap();
        assert!(!exists(&conn, &empty.id).unwrap(), "空区删除是硬删 workspaces 行");
        assert!(exists(&conn, &ws.id).unwrap(), "别的区不受影响");
    }

    #[test]
    fn list_reports_project_count_and_marks_the_active_row() {
        let conn = db::open_in_memory().unwrap();
        // 把默认区的 `created_at` 钉死在早于本次的时间：`datetime('now')` 只到秒，
        // 不钉住的话它与新建区同秒 ⇒ 排序落到第二键 `id`，而 uuid v4 恒大于 'default'，
        // 「default 恒在最前」这条会变成 4/5 概率红（Task 2 实跑抓到，Task 9 会再撞一次）。
        conn.execute(
            "UPDATE workspaces SET created_at = '2000-01-01 00:00:00' WHERE id = ?1",
            [DEFAULT_ID],
        )
        .unwrap();
        let b = create(&conn, "客户乙").unwrap();
        let rows = list(&conn).unwrap();
        assert_eq!(rows[0].id, DEFAULT_ID, "default 恒在最前（按 created_at, id 定序）");
        assert_eq!(rows.len(), 2, "新建的区要出现在列表里，且**不自动跳过去**（spec §五）");
        assert!(!rows.iter().find(|w| w.id == b.id).unwrap().is_active);
        assert!(rows.iter().find(|w| w.id == DEFAULT_ID).unwrap().is_active);

        let p = project::create_project(&conn, &project_input("区内项目")).unwrap();
        conn.execute(
            "UPDATE projects SET workspace_id = ?1 WHERE id = ?2",
            params![b.id, p.id],
        )
        .unwrap();
        project::create_project(&conn, &project_input("区内项目乙")).unwrap();
        project::delete_project(&conn, &p.id).unwrap();
        let rows = list(&conn).unwrap();
        assert_eq!(rows.iter().find(|w| w.id == b.id).unwrap().project_count, 0, "project_count 只数未软删");
        assert_eq!(rows.iter().find(|w| w.id == DEFAULT_ID).unwrap().project_count, 1);
    }

    #[test]
    fn ensure_active_row_rejects_a_foreign_workspace_row() {
        let conn = db::open_in_memory().unwrap();
        let p = project::create_project(&conn, &project_input("默认区项目")).unwrap();
        let t = crate::todo::create(
            &conn,
            &crate::todo::TodoInput {
                project_id: p.id.clone(),
                title: "写点东西".to_owned(),
                due_date: None,
            },
        )
        .unwrap();
        // 先在活动区内验一次「放行」，否则下面「跨区被拒」可能因为 id 根本不存在而假绿。
        ensure_active_row(&conn, "todos", &t.id).unwrap();

        // 裸 SQL 把宿主项目整体挪走，等价于「这条行属于别区」（此刻行还在库里）。
        conn.execute(
            "UPDATE projects SET workspace_id = 'other' WHERE id = ?1",
            [p.id.clone()],
        )
        .unwrap();
        assert_eq!(
            ensure_active_row(&conn, "todos", &t.id).unwrap_err().code,
            "not_found",
            "跨区行必须与不存在的行同码（spec §四）"
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM todos WHERE id = ?1", [&t.id], |r| r.get::<_, i64>(0))
                .unwrap(),
            1,
            "守卫只挡可见性，不许顺手删行"
        );
    }
```

> 上面用到 `crate::todo` 时，若 `todo::TodoInput` 的字段名与 `todo.rs` 实际不一致，**以 `todo.rs` 为准改测试**，不要改生产代码；本 Task 只借它造一条挂在项目下的行。

实现（补在 Step 1 那个模块里，放在 `name_of` 之后）：

```rust
/// 名字判据：trim 非空、≤ 40 字符。返回 trim 后的显示值。
fn check_name(name: &str) -> AppResult<String> {
    let trimmed = name.trim().to_owned();
    if trimmed.is_empty() {
        return Err(invalid("工作区名称不能为空"));
    }
    if trimmed.chars().count() > NAME_MAX_CHARS {
        return Err(invalid(&format!(
            "工作区名称不能超过 {} 个字符",
            NAME_MAX_CHARS
        )));
    }
    Ok(trimmed)
}

/// 重名先查再插：`workspaces.name` 上有 UNIQUE 兜底，但撞它得到的是 `db_failed`，
/// 用户看不出是自己打重了名（与 `project.rs` 把状态 CHECK 提到应用层判是同一条理由）。
/// `exclude_id` 给改名用：允许把区改成它自己现在就叫的名字。
fn name_taken(conn: &Connection, name: &str, exclude_id: Option<&str>) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM workspaces WHERE name = ?1 AND (?2 IS NULL OR id <> ?2)",
        params![name, exclude_id],
        |r| r.get(0),
    )?;
    if n > 0 {
        return Err(invalid("工作区名称已存在"));
    }
    Ok(())
}

/// 单行读回。给 `create`/`rename` 的返回值用：不自己拼 `Workspace`，
/// 免得「新建的区到底是当前区吗」这类问题出现两个真值。
fn row(conn: &Connection, id: &str) -> AppResult<Workspace> {
    let active = active_id(conn)?;
    conn.query_row(
        "SELECT w.id, w.name,
                (SELECT COUNT(*) FROM projects p
                  WHERE p.workspace_id = w.id AND p.deleted_at IS NULL),
                w.id = ?2
           FROM workspaces w WHERE w.id = ?1",
        params![id, active],
        |r| {
            Ok(Workspace {
                id: r.get(0)?,
                name: r.get(1)?,
                project_count: r.get(2)?,
                is_active: r.get(3)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| not_found("工作区", id))
}

pub fn list(conn: &Connection) -> AppResult<Vec<Workspace>> {
    let active = active_id(conn)?;
    let mut stmt = conn.prepare(
        "SELECT w.id, w.name,
                (SELECT COUNT(*) FROM projects p
                  WHERE p.workspace_id = w.id AND p.deleted_at IS NULL)
           FROM workspaces w ORDER BY w.created_at, w.id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Workspace {
                id: r.get(0)?,
                name: r.get(1)?,
                project_count: r.get(2)?,
                is_active: false,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    Ok(rows
        .into_iter()
        .map(|w| Workspace { is_active: w.id == active, ..w })
        .collect())
}

/// 新建区。**不自动切过去**（spec §五：先看列表，切不切由人决定）。
pub fn create(conn: &Connection, name: &str) -> AppResult<Workspace> {
    let name = check_name(name)?;
    name_taken(conn, &name, None)?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute("INSERT INTO workspaces (id, name) VALUES (?1, ?2)", params![id, name])?;
    row(conn, &id)
}

/// 改名。默认区可以改名，只是不可删（D3）。
pub fn rename(conn: &Connection, id: &str, name: &str) -> AppResult<Workspace> {
    if !exists(conn, id)? {
        return Err(not_found("工作区", id));
    }
    let name = check_name(name)?;
    name_taken(conn, &name, Some(id))?;
    conn.execute("UPDATE workspaces SET name = ?2 WHERE id = ?1", params![id, name])?;
    row(conn, id)
}

/// 删除。三道守卫的顺序就是文案的顺序（spec §五）：默认区 → 当前区 → 非空。
pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    if !exists(conn, id)? {
        return Err(not_found("工作区", id));
    }
    if id == DEFAULT_ID {
        return Err(invalid("默认工作区不可删除"));
    }
    if id == active_id(conn)? {
        return Err(invalid("当前工作区不可删除，请先切换到其他工作区"));
    }
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE workspace_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    if n > 0 {
        return Err(invalid(&format!(
            "该工作区下还有 {n} 个项目（含已删除项目），不能删除"
        )));
    }
    conn.execute("DELETE FROM workspaces WHERE id = ?1", [id])?;
    Ok(())
}

/// 切换。只认存在的区，配合 `delete` 的「当前区不可删」，`settings` 里的指针永不悬空（§4.3）。
pub fn switch(conn: &Connection, id: &str) -> AppResult<String> {
    if !exists(conn, id)? {
        return Err(not_found("工作区", id));
    }
    set_active(conn, id)?;
    Ok(id.to_owned())
}
```

- [ ] **Step 4: 跑 workspace 全模块测试**

Run: `cd src-tauri && cargo test -j 1 --lib workspace::`
Expected: `7 passed; 0 failed`（Step 2 那条 + 本 Step 六条）。逐条红就按红的顺序修实现，**不要**为了让断言过而改测试期望值 —— 每条期望值都是 spec §五/§4.3 的字面口径。

- [ ] **Step 5: 接五条命令（同步命令，函数体不写 await 点）**

`lib.rs:24`（`mod watch;`）之后加 `mod workspace;`。在 `audit_list`（`:161-171`）之后加：

```rust
/// 工作区五条命令（M15 spec §五）：一律同步。`db(&state)?` 取回的 `MutexGuard` 不是 `Send`，
/// 套上 async 才是多余的门槛 —— 本轮五条都不加 async 标记，所以那条「async 计数」结构门保持 7 条不动。
/// **这句注释里不许出现 `tauri::command` + `(async)` 的连续字面量**：Step 6 的门就是 `grep -c` 那一串，
/// 注释里写全它会把门读数从 7 变成 8（Task 2 实跑抓到过）。
#[tauri::command]
fn workspace_list(state: State<'_, AppState>) -> AppResult<Vec<workspace::Workspace>> {
    let conn = db(&state)?;
    workspace::list(&conn)
}

#[tauri::command]
fn workspace_create(
    state: State<'_, AppState>,
    name: String,
) -> AppResult<workspace::Workspace> {
    let conn = db(&state)?;
    workspace::create(&conn, &name)
}

#[tauri::command]
fn workspace_rename(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> AppResult<workspace::Workspace> {
    let conn = db(&state)?;
    workspace::rename(&conn, &id, &name)
}

#[tauri::command]
fn workspace_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    workspace::delete(&conn, &id)
}

#[tauri::command]
fn workspace_switch(state: State<'_, AppState>, id: String) -> AppResult<String> {
    let conn = db(&state)?;
    workspace::switch(&conn, &id)
}
```

并在 `generate_handler!` 里把 `audit_list` 改成：

```rust
            audit_list,
            workspace_list,
            workspace_create,
            workspace_rename,
            workspace_delete,
            workspace_switch
```

- [ ] **Step 6: 结构门读数**

Run: `cd src-tauri && awk '/generate_handler!\[/,/\]\)/' src/lib.rs | grep -cE '^            [a-z_]+,?$'`
Expected: **75**。控制方 2026-10-23 在改前的树上现跑过这条，读数是 **70**（末条注册项没有逗号，正则里的 `,?` 已覆盖，所以不用 `+1` 补救）。
Run: `cd src-tauri && grep -c "tauri::command(async)" src/lib.rs`
Expected: **7**。这条门是 `grep -c` 数 `src/lib.rs` 里的行数 —— 命令与注释里都不许出现那个方括号字面量，注释写进去一次读数就变 8（Task 2 实跑抓到过，Step 5 的注释文本已按此改写；计划文本自己写没事，它不在 grep 的目录里）。
Run: `cd src-tauri && grep -rn "allow(dead_code)" src-tauri/src/ | wc -l`
Expected: **7** = 基线 `db.rs` 的 1 处 + 本格 6 处临时豁免（回收路径见 Global Constraints）。注释里再出现那个字面量会把读数抬到 8 —— 那是要修的，不是要接受的。

- [ ] **Step 7: 全量 cargo + clippy**

Run: `cd src-tauri && cargo test -j 1 --lib`
Expected: `384 passed; 0 failed`（377 + 7）。
Run: `cd src-tauri && cargo clippy -j 1 --lib -- -D warnings`
Expected: `exit=0`。

- [ ] **Step 8: 提交**

```bash
git add src-tauri/src/workspace.rs src-tauri/src/lib.rs src-tauri/src/project.rs
git commit -m "feat(m15): workspace 模块（活动区权威 + 跨区守卫）与五条同步命令，命令 70→75"
```

---

## Task 3: 项目读路径按区过滤 + create 归属 + 父项同区

**Files:**
- Modify: `src-tauri/src/project.rs:93-141`（`validate_parent`）、`:168-197`（`create_project`）、`:264-333`（`list_projects`）、`:335-385`（`get_project`）、测试字面量 `:546`
- Modify: `src-tauri/src/project_audit.rs:305`、`src-tauri/src/ledger.rs:690`、`src-tauri/src/ledger_export.rs:440`、`src-tauri/src/search.rs:406` 四处 `ProjectInput` 字面量补字段

**Interfaces:**
- Consumes: Task 2 的 `workspace::active_id`、`workspace::exists`；Task 2 Step 2 已落地的 `Project.workspace_id` / `ProjectInput.workspace_id` 字段声明。
- Produces: `validate_parent(conn, self_id: Option<&str>, parent_id: Option<&str>, workspace: &str) -> AppResult<()>`（**签名多一个实参**，Task 4 的换区流程按 `effective_workspace` 传它）；`list_projects`/`get_project` 只回活动区（Task 5–8 的所有「跨区不可见」判据靠这两个）；`create_project` 恒落活动区。

- [ ] **Step 1: 写三条失败测试**

加在 `project.rs` 的 `mod tests` 末尾（`input(name)` 那个夹具已存在，见 `:545`）：

```rust
    /// create 必须写活动区，**忽略客户端塞进来的区 id**（spec §六 第 1 条）：
    /// 接受它就能用一条 IPC 把项目造进当前视野看不见的区，变成哪一区都找不到的孤儿。
    #[test]
    fn create_project_lands_in_the_active_workspace_not_the_input_one() {
        let conn = db::open_in_memory().unwrap();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        let mut input = input("孤儿防御");
        input.workspace_id = Some(b.id.clone());
        input.parent_id = None;

        let p = create_project(&conn, &input).unwrap();
        assert_eq!(p.workspace_id, crate::workspace::DEFAULT_ID, "create 恒落活动区，不吃 input");

        // 它只能从活动区的视野里被读到：切去乙区之后列表必须是空的。
        crate::workspace::switch(&conn, &b.id).unwrap();
        assert!(list_projects(&conn).unwrap().is_empty(), "被塞进别区的项目不能出现在乙区列表里");
        assert_eq!(get_project(&conn, &p.id).unwrap_err().code, "not_found");
    }

    /// 读路径的两个入口都要收窄：列表按区过滤，详情跨区读不到（`/project/$id` 深链因此天然翻不过去）。
    #[test]
    fn list_and_get_are_scoped_to_active_workspace() {
        let conn = db::open_in_memory().unwrap();
        let a = create_project(&conn, &input("默认区项目")).unwrap();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        crate::workspace::switch(&conn, &b.id).unwrap();
        let mut b_input = input("乙区项目");
        b_input.parent_id = None;
        let b_project = create_project(&conn, &b_input).unwrap();

        let in_b: Vec<String> = list_projects(&conn).unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(in_b, vec![b_project.id.clone()], "列表只出活动区：{in_b:?}");
        assert_eq!(get_project(&conn, &b_project.id).unwrap().workspace_id, b.id);
        assert_eq!(get_project(&conn, &a.id).unwrap_err().code, "not_found", "跨区 id 与不存在的 id 同码");

        crate::workspace::switch(&conn, crate::workspace::DEFAULT_ID).unwrap();
        let in_default: Vec<String> = list_projects(&conn).unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(in_default, vec![a.id.clone()], "切回来又是另一份列表：{in_default:?}");
    }

    /// 跨区父子边必须造不出来：父项存在性那条 SQL 加区谓词（spec §六）。
    /// 用裸 SQL 把一个项目挪到别区，模拟「父在乙、子在丙」这个不该存在的形状。
    #[test]
    fn parent_must_live_in_the_active_workspace() {
        let conn = db::open_in_memory().unwrap();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        let parent = create_project(&conn, &input("总包甲")).unwrap();
        conn.execute(
            "UPDATE projects SET workspace_id = ?1 WHERE id = ?2",
            rusqlite::params![b.id, parent.id],
        )
        .unwrap();

        let mut child = input("子项目乙");
        child.parent_id = Some(parent.id.clone());
        let err = create_project(&conn, &child).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(
            err.message.contains("所属总包不存在或已删除"),
            "文案要指名归属问题：{}",
            err.message
        );
        assert_eq!(list_projects(&conn).unwrap().len(), 0, "默认区里此刻没有项目：父项已被挪走");
    }
```

Run: `cd src-tauri && cargo test -j 1 --lib project::tests::create_project_lands_in`
Expected: FAIL —— `assertion failed: p.workspace_id == DEFAULT_ID` 一侧不对（`workspace_id` 还是 `"default"` 字面量时它会假过，所以**先看后两条是否同时红**；如果三条全绿说明 Step 2 还没开始写实现，正常，按顺序往下做）。
实际口径：本 Step 先跑第 2、3 条（`list_and_get_are_scoped…`、`parent_must_live…`），它们**必须红**（现在没有任何区过滤）。第 1 条因为 `create_project` 还没写列（走 DEFAULT 落 default）会**绿** —— 它是钉「不许信任 input」的反值断言，红绿都由 Step 3 的实现决定，不许为它改断言。

- [ ] **Step 2: 读路径加区谓词**

`list_projects`（`project.rs:264`）改成（只列改动，其余行不动）：

```rust
pub fn list_projects(conn: &Connection) -> AppResult<Vec<Project>> {
    let workspace = crate::workspace::active_id(conn)?;
    let mut rows = conn.prepare(
        "SELECT id, name, code, manager, customer, contact_name, contact_phone,
                contract_no, contract_period, delivery_deadline, status, parent_id, workspace_id
         FROM projects WHERE deleted_at IS NULL AND workspace_id = ?1 ORDER BY updated_at DESC",
    )?;
```

`rows.query_map` 的第一个实参从 `[]` 改成 `params![workspace]`，闭包里 `parent_id: r.get(11)?` 之后加 `workspace_id: r.get(12)?,`。

`get_project`（`:335`）的 SELECT 同样加 `workspace_id`（第 12 列），取元组时补 `r.get::<_, String>(11)?`，`query_row` 实参改 `params![id, crate::workspace::active_id(conn)?]`，构造 `Project { … }` 时带上 `workspace_id: base.11`。

> 标签与目录那三条批量 SQL（`:294`、`:303`）不动：它们的输入已经是被过滤过的项目集合。

Run: `cd src-tauri && cargo test -j 1 --lib project::tests::list_and_get_are_scoped`
Expected: PASS。

- [ ] **Step 3: create 写活动区**

`create_project`（`:168`）在 `validate(input)?;` 之后加一行取活动区，并把列与实参各加一位：

```rust
pub fn create_project(conn: &Connection, input: &ProjectInput) -> AppResult<Project> {
    validate(input)?;
    // create 恒落活动区，**不看 `input.workspace_id`**（spec §六 第 1 条）：接受客户端给的区
    // 就等于开一条「把项目造进当前视野外」的通路。改归属只有一条合法路径 —— update。
    let workspace = crate::workspace::active_id(conn)?;
    let parent_id = normalize(input.parent_id.clone());
    let tx = write_tx(conn)?;
    validate_parent(&tx, None, parent_id.as_deref(), &workspace)?;
    let id = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO projects (id, name, code, manager, customer, contact_name, contact_phone,
                               contract_no, contract_period, delivery_deadline, status, parent_id,
                               workspace_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![
            id,
            input.name.trim(),
            normalize(input.code.clone()),
            normalize(input.manager.clone()),
            normalize(input.customer.clone()),
            normalize(input.contact_name.clone()),
            normalize(input.contact_phone.clone()),
            normalize(input.contract_no.clone()),
            normalize(input.contract_period.clone()),
            normalize(input.delivery_deadline.clone()),
            input.status,
            parent_id,
            workspace,
        ],
    )?;
```

（`replace_tags` → `record_create` → `commit` → `get_project` 四行照旧；Task 4 会给 `record_create` 多一个实参。）

- [ ] **Step 4: `validate_parent` 加同区断言**

```rust
fn validate_parent(
    conn: &Connection,
    self_id: Option<&str>,
    parent_id: Option<&str>,
    workspace: &str,
) -> AppResult<()> {
    let Some(pid) = parent_id else {
        return Ok(());
    };
    if self_id == Some(pid) {
        return Err(invalid("项目不能把自己设为所属总包"));
    }
    let exists = conn
        .query_row(
            "SELECT 1 FROM projects WHERE id = ?1 AND deleted_at IS NULL AND workspace_id = ?2",
            params![pid, workspace],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
```

后面循环检测那一段不动（环是在同一个区内部才能成环）。`update_project` 里那处调用暂时先传 `old.workspace_id.as_str()`（Task 4 改成 effective），`ProjectInput` 四处测试字面量各补 `workspace_id: None,`。

Run: `cd src-tauri && cargo test -j 1 --lib project::tests::parent_must_live`
Expected: PASS。

- [ ] **Step 5: 全量回归（这是本轮最可能出现既有测试红的一格）**

Run: `cd src-tauri && cargo test -j 1 --lib`
Expected: `387 passed; 0 failed`（384 + 3）。
**如果别的模块的既有测试红**（`ledger.rs`/`search.rs`/`daily_report.rs`/`index_job.rs` 等），先判成因：它们 seed 项目一律走 `project::create_project` 或直接 INSERT。直接 INSERT 的那类不写 `workspace_id`，靠列 DEFAULT 落 `default`，与 `active_id` 的 `default` 一致，不该红；真红了**不要**在测试里补区参数，先把成因写进报告（这是 Task 5/6 的守卫还没落地还是本 Task 的谓词写错）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/project.rs src-tauri/src/project_audit.rs src-tauri/src/ledger.rs src-tauri/src/ledger_export.rs src-tauri/src/search.rs
git commit -m "feat(m15): 项目列表/详情按活动区过滤，create 恒落活动区，父项必须同区"
```

---

## Task 4: 换区与子树随迁 + 活动区跟随 + 操作日志联动

**Files:**
- Modify: `src-tauri/src/project.rs:199-232`（`update_project`）；Step 0 探针 OK 时用一条 `WITH RECURSIVE … UPDATE`，**不新增** `collect_subtree`（本次实测 3.50.4 / bundled 3.53.2 均 OK）
- Modify: `src-tauri/src/project_audit.rs:92-186`（三处 changes）、`:210-226`（`record_create` 签名）、`:228-297`（`list` 与 `project_options` 的区过滤）
- Modify: `src-tauri/src/project.rs:194`（`record_create` 调用点）
- Modify: `src-tauri/src/workspace.rs:72-80`（删 `name_of` 的 dead_code 豁免 —— Global Constraints 第 2 条要求本格回收，Files 原清单漏了本行，执行后追认）

**Interfaces:**
- Consumes: Task 2 的 `workspace::exists`/`set_active`/`name_of`/`active_id`；Task 3 的 `validate_parent(…, workspace)` 四参签名、`Project.workspace_id`。
- Produces: `fn collect_subtree(conn: &Connection, root_id: &str) -> AppResult<Vec<String>>`（私有）；`project_audit::record_create(conn, project_id, input, workspace_id: &str)`（**多一个实参**）；日志 `changes` 里 `field = "workspaceId"`、`label = "工作区"`、值是**区名**。

- [ ] **Step 0: 可行性探针 —— `WITH RECURSIVE` 能不能前缀 `UPDATE`（先跑，再决定 Step 3 写哪个形状）**

spec §六.3 留了这个岔口。当场跑一次，别留给实现者猜：

```bash
cd src-tauri && python - <<'PY'
import sqlite3
c = sqlite3.connect(":memory:")
c.executescript("""
CREATE TABLE projects(id TEXT PRIMARY KEY, parent_id TEXT, workspace_id TEXT);
INSERT INTO projects VALUES('r',NULL,'a'),('c1','r','a'),('g1','c1','a'),('x',NULL,'b');
""")
try:
    c.execute("""
      WITH RECURSIVE sub(id) AS (
        SELECT ?1 UNION ALL SELECT p.id FROM projects p JOIN sub s ON p.parent_id = s.id
      )
      UPDATE projects SET workspace_id = ?2 WHERE id IN (SELECT id FROM sub)
    """, ("r", "b"))
    print("CTE-UPDATE: OK", c.execute("SELECT id, workspace_id FROM projects ORDER BY id").fetchall())
except Exception as e:
    print("CTE-UPDATE: FAIL", type(e).__name__, e)
print("sqlite version:", sqlite3.sqlite_version)
PY
```

Expected: 读数落进账本。**两条分支只走一条**：
- 打印 `CTE-UPDATE: OK` ⇒ Step 3 用 `WITH RECURSIVE … UPDATE`（一条语句）。
- 打印 `FAIL` ⇒ Step 3 用下面给的 `collect_subtree` + `IN (…)`。
> 注意这跑的是系统 python 的 sqlite3，只当可行性风向标；bundled rusqlite 的 SQLite 版本以 `Cargo.lock` 里 `libsqlite3-sys` 为准，若两者结论冲突，**取保守的 `collect_subtree` 分支**并在账本记一句冲突。

- [ ] **Step 1: 写换区的失败测试（三条）**

加在 `project.rs` 的 `mod tests` 末尾。夹具用现成的 `input(name)`（`:545`）：

```rust
    /// 三层树随根换区，并且**换区后活动区跟随到目标区**，返回值仍在当前视野里读得到
    /// （spec §六.4：不跟随的话 `update_project` 末尾那次 `get_project` 会 not_found，
    /// 于是这次保存「其实成功了但界面报失败」）。
    #[test]
    fn move_subtree_follows_root_and_keeps_return_visible() {
        let conn = db::open_in_memory().unwrap();
        let root = create_project(&conn, &input("总包甲")).unwrap();
        let mut child_in = input("子项目乙");
        child_in.parent_id = Some(root.id.clone());
        let child = create_project(&conn, &child_in).unwrap();
        let mut grand_in = input("孙项目丙");
        grand_in.parent_id = Some(child.id.clone());
        let grand = create_project(&conn, &grand_in).unwrap();

        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        let mut move_in = input("总包甲");
        move_in.parent_id = None;
        move_in.workspace_id = Some(b.id.clone());
        let moved = update_project(&conn, &root.id, &move_in).unwrap();
        assert_eq!(moved.workspace_id, b.id, "返回值要能读得到，区也确实是目标区");

        assert_eq!(
            crate::workspace::active_id(&conn).unwrap(),
            b.id,
            "换区后活动区跟随（spec §六.4 的返回契约前提）"
        );
        // 不断顺序：根项目本次主 UPDATE 动了 `updated_at`，反而排到最前，子与孙同秒序不可靠。
        // 本 Task 的判据是「三层齐、且都在乙区」。
        let mut ids: Vec<String> = list_projects(&conn).unwrap().into_iter().map(|p| p.id).collect();
        ids.sort();
        let mut want = vec![root.id.clone(), child.id.clone(), grand.id.clone()];
        want.sort();
        assert_eq!(ids, want, "整棵子树三层都要在乙区：{ids:?}");

        crate::workspace::switch(&conn, crate::workspace::DEFAULT_ID).unwrap();
        assert!(list_projects(&conn).unwrap().is_empty(), "旧区必须一台不剩");
    }

    /// 有父的项目不许单独换区（D7）。两个方向都要拒：库里已有父、或本次提交里刚指定了父。
    #[test]
    fn child_project_cannot_move_alone() {
        let conn = db::open_in_memory().unwrap();
        let parent = create_project(&conn, &input("总包甲")).unwrap();
        let mut child_in = input("子项目乙");
        child_in.parent_id = Some(parent.id.clone());
        let child = create_project(&conn, &child_in).unwrap();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();

        let mut move_in = input("子项目乙");
        move_in.parent_id = Some(parent.id.clone());
        move_in.workspace_id = Some(b.id.clone());
        let err = update_project(&conn, &child.id, &move_in).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("子项目不能单独换区"), "文案要点名随迁：{}", err.message);

        // 摘父 + 换区同一次保存也要拒（否则会绕过子树前提）。
        let mut detach_and_move = input("子项目乙");
        detach_and_move.parent_id = None;
        detach_and_move.workspace_id = Some(b.id.clone());
        assert_eq!(update_project(&conn, &child.id, &detach_and_move).unwrap_err().code, "invalid_input");

        // 数据一个字节没变：区、父、活动区都还在原位。
        assert_eq!(get_project(&conn, &child.id).unwrap().workspace_id, crate::workspace::DEFAULT_ID);
        assert_eq!(
            get_project(&conn, &child.id).unwrap().parent_id.as_deref(),
            Some(parent.id.as_str())
        );
        assert_eq!(crate::workspace::active_id(&conn).unwrap(), crate::workspace::DEFAULT_ID);
    }

    /// 半途失败不许留下「父在本区、子在别区、当前区已跟过去」。
    /// 造失败的那一刀选在**子树 UPDATE 与 set_active 之后**：把日志表 DROP 掉，
    /// `record_update` 就必失败（与 M14 的 `audit_insert_failure_rolls_back_project_write` 同一手法，
    /// 不用给生产代码开测试注入点）。这样断的才是「三步同事务」，而不是「早退得早」。
    #[test]
    fn move_is_atomic_when_the_log_write_fails() {
        let conn = db::open_in_memory().unwrap();
        let root = create_project(&conn, &input("总包甲")).unwrap();
        let mut child_in = input("子项目乙");
        child_in.parent_id = Some(root.id.clone());
        let child = create_project(&conn, &child_in).unwrap();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();

        conn.execute("DROP TABLE project_audit_logs", []).unwrap();

        let mut move_in = input("总包甲");
        move_in.parent_id = None;
        move_in.workspace_id = Some(b.id.clone());
        let err = update_project(&conn, &root.id, &move_in).unwrap_err();
        assert_eq!(err.code, "db_failed", "日志写不进 ⇒ 整个项目写回滚：{:?}", err.code);

        // get_project 现在按活动区过滤，所以这里全部走裸 SQL，读库里的真实形状。
        let of = |id: &str| conn
            .query_row("SELECT workspace_id FROM projects WHERE id = ?1", [id], |r| r.get::<_, String>(0))
            .unwrap();
        assert_eq!(of(&root.id), crate::workspace::DEFAULT_ID, "根没动才算回滚干净");
        assert_eq!(of(&child.id), crate::workspace::DEFAULT_ID, "子树不能留在目标区");
        assert_eq!(
            crate::workspace::active_id(&conn).unwrap(),
            crate::workspace::DEFAULT_ID,
            "跟随那一句也必须一起回滚，否则界面停在空区"
        );
    }
```

Run: `cd src-tauri && cargo test -j 1 --lib project::tests::move_subtree_follows_root`
Expected: FAIL —— `assert_eq!` 左边 `moved.workspace_id == "default"`（`update_project` 还没看 `input.workspace_id`）。

- [ ] **Step 2: 写 `collect_subtree`（保守分支；Step 0 若判 CTE 可用，把它换成一条 CTE UPDATE 并删掉本函数）**

放在 `write_tx` 之后：

```rust
/// 根项目连同全部后代 id。带 `visited` 去重：库里若被人手改成环（外键不存在、`validate_parent`
/// 只在命令路径上判），无visited 的逐层收集会永远走不到尽头 —— 去重之后最多多跑一轮空集，不会挂死。
/// 软删行也收：一个区的软删项目仍占着 `workspace_id`（§五 删除守卫含软删就是这个理由），
/// 随迁时把它们一起带走才不会在旧区留下无主行。
fn collect_subtree(conn: &rusqlite::Transaction<'_>, root_id: &str) -> AppResult<Vec<String>> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut ids = Vec::new();
    let mut frontier = vec![root_id.to_owned()];
    seen.insert(root_id.to_owned());
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for parent in &frontier {
            let mut stmt = conn.prepare("SELECT id FROM projects WHERE parent_id = ?1")?;
            let kids = stmt
                .query_map([parent], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);
            for kid in kids {
                if seen.insert(kid.clone()) {
                    next.push(kid);
                }
            }
        }
        ids.extend(next.iter().cloned());
        frontier = next;
    }
    ids.insert(0, root_id.to_owned());
    Ok(ids)
}
```

- [ ] **Step 3: `update_project` 接换区流程**

整函数替换成（`write_tx` / `get_project` / `replace_tags` / `commit` 的既有形状不动）：

```rust
pub fn update_project(conn: &Connection, id: &str, input: &ProjectInput) -> AppResult<Project> {
    validate(input)?;
    let parent_id = normalize(input.parent_id.clone());
    let tx = write_tx(conn)?;
    let old = get_project(&tx, id)?;
    // 目标区：`None` 或与旧值相同 = 不换区。`normalize` 顺带把空串当「不改」处理，
    // 前端新建表单的种子值因此不会造出一个「迁到 id 为空串的区」。
    let target = normalize(input.workspace_id.clone()).filter(|w| w != &old.workspace_id);
    let workspace = target.as_deref().unwrap_or(old.workspace_id.as_str());

    // D7 先判：有父（库里的或本次提交的）就不许单独换区。这句必须排在 `validate_parent`
    // 之前 —— 换区时 `validate_parent` 用的是目标区，库里的父在旧区，会先报「所属总包不存在
    // 或已删除」，把「子项目不能单独换区」这个真因盖掉。
    if let Some(target) = &target {
        if old.parent_id.is_some() || parent_id.is_some() {
            return Err(invalid(
                "子项目不能单独换区，请编辑它的父项目（整棵子树一起随迁）",
            ));
        }
        if !crate::workspace::exists(&tx, target)? {
            return Err(invalid("目标工作区不存在"));
        }
    }
    validate_parent(&tx, Some(id), parent_id.as_deref(), workspace)?;

    if let Some(target) = &target {
        let subtree = collect_subtree(&tx, id)?;
        let placeholders = subtree.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let args: Vec<&str> = std::iter::once(target.as_str())
            .chain(subtree.iter().map(String::as_str))
            .collect();
        // 后代**不动 updated_at**：列表按 updated_at 倒序，随迁不该把整棵子树挤到列表顶上。
        tx.execute(
            &format!(
                "UPDATE projects SET workspace_id = ?1 WHERE id IN ({placeholders})"
            ),
            rusqlite::params_from_iter(args.iter().copied()),
        )?;
        // 换区跟随（spec §六.4）：活动区跟着子树走，`get_project` 才读得到本次返回的项目。
        crate::workspace::set_active(&tx, target)?;
    }

    let changed = tx.execute(
        "UPDATE projects SET name=?2, code=?3, manager=?4, customer=?5, contact_name=?6,
                contact_phone=?7, contract_no=?8, contract_period=?9, delivery_deadline=?10,
                status=?11, parent_id=?12, updated_at=datetime('now')
         WHERE id=?1 AND deleted_at IS NULL",
        params![
            id,
            input.name.trim(),
            normalize(input.code.clone()),
            normalize(input.manager.clone()),
            normalize(input.customer.clone()),
            normalize(input.contact_name.clone()),
            normalize(input.contact_phone.clone()),
            normalize(input.contract_no.clone()),
            normalize(input.contract_period.clone()),
            normalize(input.delivery_deadline.clone()),
            input.status,
            parent_id,
        ],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    replace_tags(&tx, id, &input.tags)?;
    project_audit::record_update(&tx, &old, input)?;
    tx.commit()?;
    get_project(conn, id)
}
```

> `validate_parent(…, workspace)` 传的是 **effective 区**（换区时是目标区）：先判父再写，父必须在子即将落座的那个区里。

Run: `cd src-tauri && cargo test -j 1 --lib project::tests::`
Expected: 三条新测试都 PASS（`cargo test` 只接一个筛选词，别把两条测试名并列）。其中 `move_is_atomic_when_the_log_write_fails` 是真的在验三步同事务（DROP 掉日志表 ⇒ 子树 UPDATE 与 `set_active` 都写完之后才失败）；它必须**红在 Step 3 之前** —— 如果它在只做 Step 2 的读过滤时就绿了，说明 `update_project` 里的三步还没进同一个 `tx`，回到 Step 3 检查是不是有人把 `tx.execute` 写成了 `conn.execute`。

- [ ] **Step 4: 日志字段（`project_audit.rs`）**

> **与 spec §七 文本的一处偏离（已在此点名，不是偷偷改）**：spec 写的是「`input_fields`（`:92`）/`project_fields`（`:108`）各加一格 `workspaceId`」。实际按 `parentId` 的既有形制**加在那两张列表之外**（`create_changes`/`update_changes`/`delete_changes` 各推一条），理由是这两张表里存的是 `Option<String>` 的**原值**、拿到的是 `&Connection` 之外的数据，而区/父项都要用 `conn` 把 id 解析成显示名 —— 混进列表会让 `update_changes` 的 `debug_assert_eq!(old_fields.len(), new_fields.len())` 与 zip 比较整条变形。行为等价（日志里仍是「工作区」一格存两区名），故不回头改 spec 文本。

`parent_name` 旁边加同构的一件（放 `parent_name` 之后）：

```rust
/// 区名。日志里存显示名而不是裸 id（与 `parent_name` 同一条理由）。
/// 有项目的区删不掉（M15 spec §五 守卫③），所以「已删除工作区」这一支实际不可达。
fn workspace_name(conn: &Connection, id: &str) -> AppResult<String> {
    crate::workspace::name_of(conn, id)
}
```

`create_changes` 多一个实参并补一条（放在 `parentId` 那条之前，与表单里「工作区」排在「所属总包」上方一致）：

```rust
fn create_changes(
    conn: &Connection,
    input: &ProjectInput,
    workspace_id: &str,
) -> AppResult<Vec<AuditChange>> {
    let mut out: Vec<AuditChange> = input_fields(input)
        .into_iter()
        .filter_map(|f| f.value.map(|v| change(f.field, f.label, None, Some(v))))
        .collect();
    out.push(change(
        "workspaceId",
        "工作区",
        None,
        Some(workspace_name(conn, workspace_id)?),
    ));
    if let Some(pid) = normalize_opt(&input.parent_id) {
```

`update_changes` 在 `parentId` 那段之前插：

```rust
    // 换区只在**被编辑的那个根项目**下记一条；随迁的后代不各自记（M15 spec §七：
    // 否则一次换区产 N 条噪声行，而它们的区变化完全由父这一条解释）。
    let target_ws = normalize_opt(&input.workspace_id)
        .unwrap_or_else(|| old.workspace_id.clone());
    if target_ws != old.workspace_id {
        out.push(change(
            "workspaceId",
            "工作区",
            Some(workspace_name(conn, &old.workspace_id)?),
            Some(workspace_name(conn, &target_ws)?),
        ));
    }
```

`delete_changes` 在 `parentId` 那段之前插：

```rust
    out.push(change(
        "workspaceId",
        "工作区",
        Some(workspace_name(conn, &old.workspace_id)?),
        None,
    ));
```

`record_create` 签名与调用点各改一处：

```rust
pub fn record_create(
    conn: &Connection,
    project_id: &str,
    input: &ProjectInput,
    workspace_id: &str,
) -> AppResult<()> {
    let changes = create_changes(conn, input, workspace_id)?;
    insert(conn, project_id, input.name.trim(), "create", &changes)
}
```

```rust
    project_audit::record_create(&tx, &id, input, &workspace)?;
```

- [ ] **Step 5: `audit_list` / `project_options` 按区过滤**

`list`（`project_audit.rs:249`）的 SQL 与实参换成：

```rust
    let workspace = crate::workspace::active_id(conn)?;
    let mut stmt = conn.prepare(
        "SELECT a.id, a.project_id, a.project_name, a.action, a.changes, a.created_at
           FROM project_audit_logs a
           JOIN projects p ON p.id = a.project_id
          WHERE (?1 IS NULL OR a.project_id = ?1) AND (?2 IS NULL OR a.action = ?2)
            AND p.workspace_id = ?5
          ORDER BY a.created_at DESC, a.rowid DESC
          LIMIT ?3 OFFSET ?4",
    )?;
    let raw = stmt
        .query_map(params![project_filter, action, limit + 1, offset, workspace], |r| {
```

**故意不加 `p.deleted_at IS NULL`**：软删项目的日志必须还出现在所属区（M14 的「永久保留」+ 删除动作本身就产生一条日志）。`project_options` 同样 JOIN 后加 `AND p.workspace_id = ?1`：

```rust
    let workspace = crate::workspace::active_id(conn)?;
    let mut stmt = conn.prepare(
        "SELECT a.project_id, a.project_name
           FROM project_audit_logs a
           JOIN projects p ON p.id = a.project_id AND p.workspace_id = ?1
           JOIN (SELECT project_id, MAX(rowid) AS max_rowid FROM project_audit_logs GROUP BY project_id) b
             ON a.rowid = b.max_rowid
          ORDER BY a.project_name, a.project_id",
    )?;
    let out = stmt
        .query_map([&workspace], |r| {
```

- [ ] **Step 6: 两条日志侧测试**

`project_audit.rs` 的 `mod tests` 末尾：

```rust
    /// 根换区 ⇒ 恰一条 update 日志、「工作区」那条存两区中文名、随迁的后代不各自出日志。
    #[test]
    fn workspace_change_is_logged_once_for_root() {
        let conn = db::open_in_memory().unwrap();
        let root = crate::project::create_project(&conn, &input("总包甲")).unwrap();
        let mut child_in = input("子项目乙");
        child_in.parent_id = Some(root.id.clone());
        let child = crate::project::create_project(&conn, &child_in).unwrap();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();

        let mut move_in = input("总包甲");
        move_in.workspace_id = Some(b.id.clone());
        crate::project::update_project(&conn, &root.id, &move_in).unwrap();

        let page = list(&conn, None, Some("update"), None, None).unwrap();
        let moves: Vec<&AuditEntry> = page
            .items
            .iter()
            .filter(|e| {
                e.changes
                    .iter()
                    .any(|c| c.field == "workspaceId")
            })
            .collect();
        assert_eq!(moves.len(), 1, "一次换区只能有一条带工作区差异的日志：{}", page.items.len());
        assert_eq!(moves[0].project_id, root.id, "日志挂在被编辑的根项目下");
        let c = &moves[0].changes[0];
        assert_eq!((c.old_value.as_deref(), c.new_value.as_deref()), (Some("默认工作区"), Some("客户乙")), "存显示名不是裸 id：{:?}", c);

        let child_logs = list(&conn, Some(&child.id), Some("update"), None, None).unwrap();
        assert!(child_logs.items.is_empty(), "随迁的后代不许各自出日志：{}", child_logs.items.len());
    }

    /// 日志页与项目下拉都只出活动区。被硬删的项目行（这里用裸 SQL 造）它的日志从所有区视野消失
    /// —— M14 的「永久保留」针对行不针对可见性（spec §七 已点名，不藏）。
    #[test]
    fn audit_list_and_options_are_scoped_to_active_workspace() {
        let conn = db::open_in_memory().unwrap();
        let a = crate::project::create_project(&conn, &input("默认区项目")).unwrap();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        // 乙区的项目只能靠「切过去再建」得到：create 恒落活动区，input 里的区号会被忽略（§六.1）。
        crate::workspace::switch(&conn, &b.id).unwrap();
        let bp = crate::project::create_project(&conn, &input("乙区项目")).unwrap();
        crate::workspace::switch(&conn, crate::workspace::DEFAULT_ID).unwrap();

        let page = list(&conn, None, None, None, None).unwrap();
        let ids: Vec<&str> = page.items.iter().map(|e| e.project_id.as_str()).collect();
        assert!(ids.contains(&a.id.as_str()), "默认区日志要在：{ids:?}");
        assert!(!ids.contains(&bp.id.as_str()), "别区日志不能漏进来：{ids:?}");
        let opts: Vec<&str> = page.projects.iter().map(|p| p.project_id.as_str()).collect();
        assert_eq!(opts, vec![a.id.as_str()], "项目下拉只出活动区有日志的项目：{opts:?}");

        conn.execute("DELETE FROM projects WHERE id = ?1", [&bp.id]).unwrap();
        let after = list(&conn, None, None, None, None).unwrap();
        assert!(!after.items.iter().any(|e| e.project_id == bp.id));
    }
```

Run: `cd src-tauri && cargo test -j 1 --lib project_audit::`
Expected: 新增两条 PASS，既有 audit 测试全绿（`input(name)` 补 `workspace_id: None` 后编译过；create 日志多一条「工作区」行，如果既有断言写了 `changes.len()`，**按新条数改断言**并在报告里点名是哪条）。

- [ ] **Step 7: 全量回归 + 提交**

Run: `cd src-tauri && cargo test -j 1 --lib`
Expected: `394 passed; 0 failed`（387 + 7；评审 M3/M6 各补一条守卫）。

```bash
git add src-tauri/src/project.rs src-tauri/src/project_audit.rs
git commit -m "feat(m15): 根项目换区整棵子树随迁 + 活动区跟随，工作区进字段级日志"
```

---

## Task 5: 关联表的读写守卫（待办 / 里程碑 / 到期 / 日报 / 台账 / 目录）

**Files:**
- Modify: `src-tauri/src/todo.rs:122-210`、`src-tauri/src/milestone.rs:75-168`、`src-tauri/src/expiry.rs:83-181`、`src-tauri/src/daily_report.rs:96-180` + `:529` + `:542`、`src-tauri/src/ledger.rs:194-235` + 五个 `update_*`、`src-tauri/src/project.rs:440/470/500`、`src-tauri/src/workspace.rs`（删 `ensure_active_row` 与 `ensure_active_project` 两处 dead_code 豁免）

**Interfaces:**
- Consumes: Task 2 的 `workspace::active_id`/`ensure_active_project`/`ensure_active_row`。
- Produces: 无新签名（本 Task 全是往既有函数入口加谓词/守卫）。**注意 `ledger::soft_delete` 的表名参数从 `&str` 收紧成 `&'static str`**，五个调用点（`:399/484/566/623/665`）传的都是字面量，不用改。

**这一格是机械格，但一格的评审面很大（跨 6 个模块 30 余处）**：按「一个模块一批改完 + 该模块测试」的顺序做，别把六个模块的半截改放在一起。

- [ ] **Step 1: `todo.rs` —— 四处**

`project_exists`（`:122`）：

```rust
fn project_exists(conn: &Connection, project_id: &str) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects
          WHERE id = ?1 AND deleted_at IS NULL AND workspace_id = ?2",
        params![project_id, crate::workspace::active_id(conn)?],
        |r| r.get(0),
    )?;
```

`list`（`:139`）在 `let mut stmt` 前加 `let workspace = crate::workspace::active_id(conn)?;`，JOIN 那行加区谓词、`query_map` 实参改两条：

```rust
               JOIN projects p ON p.id = t.project_id AND p.deleted_at IS NULL
                                  AND p.workspace_id = ?2
              WHERE t.deleted_at IS NULL AND (?1 IS NULL OR t.project_id = ?1)
              ORDER BY t.created_at, t.id"
        ),
    )?;
    let rows = stmt.query_map(params![project_id, workspace], map_todo)?;
```

`get` / `update` / `delete` 三个函数体首行各加一句（**只加这一句，SQL 不动**）：

```rust
    crate::workspace::ensure_active_row(conn, "todos", id)?;
```

Run: `cd src-tauri && cargo test -j 1 --lib todo::`
Expected: 既有 todo 测试全绿（它们的项目都在默认区 = 活动区，守卫不该挡）；红的就是改错了。

> 本步落地后 `ensure_active_row` 有了生产调用点：删掉 `src-tauri/src/workspace.rs` 里它的 dead_code 豁免（Global Constraints 第 3 条），豁免读数 4→3。

- [ ] **Step 2: `milestone.rs` / `expiry.rs` —— 同形状**

`milestone.rs`：`project_exists`（`:75`）、`list`（`:92`，JOIN 加 `AND p.workspace_id = ?2`）同 Step 1；`get`（`:110`）、`update`（`:145`）、`delete`（`:160`）首行加 `ensure_active_row(conn, "milestones", id)?;`。

`expiry.rs`：`project_exists`（`:83`）、`list`（`:98`）同上；`get`（`:116`）**已有 JOIN**，只在 SQL 里加 `AND p.workspace_id = ?2` 并把实参改 `params![id, crate::workspace::active_id(conn)?]`（这条不走 `ensure_active_row`，避免同一次读做两遍同样的判断）；`update`（`:157`）、`delete`（`:171`）首行加 `ensure_active_row(conn, "project_expiries", id)?;`。

- [ ] **Step 3: `daily_report.rs` —— 五处 + 导入两处**

CRUD 四个函数同 Step 1（表名 `"daily_reports"`）；`update` 里 `project_exists(conn, &edit.project_id)?` 保留（它管「不能把日报挂到别区项目下」），守卫加在它**之前**（先管这行属不属于本区，再管新宿主）。

导入侧两处（spec §4.1 的「日报导入」那行）：

```rust
/// 匹配用的项目清单：软删项目**不在**清单里（挂不上新行，与 `create` 同一判据）。
/// **也只认活动区**（M15 spec §4.1）：换区即换视野，表里指向别区项目的行走既有那条
/// 「找不到项目」的拒绝理由，而不是悄悄写到看不见的地方去。
fn load_projects(conn: &Connection) -> AppResult<Vec<ProjectRow>> {
    let workspace = crate::workspace::active_id(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, code, name FROM projects WHERE deleted_at IS NULL AND workspace_id = ?1",
    )?;
    let rows = stmt.query_map(params![workspace], |r| {
```

```rust
fn existing_keys(conn: &Connection) -> AppResult<HashSet<(String, String, String, i64)>> {
    let workspace = crate::workspace::active_id(conn)?;
    let mut stmt = conn.prepare(
        "SELECT d.report_date, d.project_id, d.content, d.hours
           FROM daily_reports d
           JOIN projects p ON p.id = d.project_id
          WHERE d.deleted_at IS NULL AND p.workspace_id = ?1",
    )?;
    let rows = stmt.query_map(params![workspace], |r| {
```

- [ ] **Step 4: `ledger.rs` —— 九处**

`project_exists`（`:223`）加 `AND workspace_id = ?2`（同 Step 1 形状）—— 它同时覆盖五个 `create_*`。
`soft_delete`（`:212`）签名收紧 + 首行守卫：

```rust
fn soft_delete(conn: &Connection, table: &'static str, id: &str) -> AppResult<()> {
    crate::workspace::ensure_active_row(conn, table, id)?;
```

五个 `update_*`（`update_env:376`、`update_credential:464`、`update_server:546`、`update_link:601`、`update_note:649`）函数体首行各加一句，表名用各自的字面量：

```rust
    crate::workspace::ensure_active_row(conn, "ledger_envs", id)?;
```

`list`（`:235`）入口一句 `ensure_active_project`；`reveal`（`:194`）**没有 `project_id` 参数**，入口用 `ensure_active_row`（`table` 由 `secret_column` 给出）：

```rust
    crate::workspace::ensure_active_project(conn, project_id)?;
```

```rust
    let (table, col_name, cipher_col) = secret_column(kind, field)?;
    crate::workspace::ensure_active_row(conn, table, id)?;
```

> **本步收掉 `ensure_active_project` 的 dead_code 豁免**（Global Constraints 第 1 条）：`ledger::list`（以及 Step 5 的三个目录 mutator）是本分支里它的首批生产调用点（Task 3 报告已实测 Task 3 内无调用点）。删完现跑 `cargo clippy -j 1 --lib -- -D warnings` 必须 exit 0；本步收工后豁免读数应为 **2**（`db.rs` + `active_project_ids`）。
>
> `secret_column` 回的是 `(&'static str, &'static str, &'static str)`（`ledger.rs:156`），直接能喂给守卫。

- [ ] **Step 5: `project.rs` 三个目录 mutator**

`set_root_dir`（`:440`）、`add_entry_dir`（`:470`）、`remove_dir`（`:500`）函数体首行各加：

```rust
    crate::workspace::ensure_active_project(conn, project_id)?;
```

- [ ] **Step 6: 七条守卫测试（每个模块族一条）**

各模块 `mod tests` 末尾加。形状统一：用该模块**已有的** `seed(conn, id)` 夹具（`todo.rs:219`、`milestone.rs:177`、`expiry.rs:188`、`daily_report.rs:664` 都是 `fn seed(conn: &Connection, id: &str)`，返回 `()`，裸 INSERT 靠列 DEFAULT 落默认区）造一个区内项目 → 建关联行 → 裸 SQL 把**项目**挪到别区（等价于「关联行属于别区」）→ 断读不到、改不了、删不掉、挂不上。以 `todo.rs` 为完整样板，其余四条**同形状但必须各自写全**（不写「参照上面」）：

```rust
    /// 待办的读与写都只认活动区（M15 spec §4.1 / §4.2）。
    #[test]
    fn todo_list_and_mutators_are_scoped() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p-scope");
        let t = create(
            &conn,
            &TodoInput {
                project_id: "p-scope".to_owned(),
                title: "写点东西".to_owned(),
                due_date: None,
            },
        )
        .unwrap();
        assert_eq!(list(&conn, None).unwrap().len(), 1, "活动区里的待办先要读得到");

        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        conn.execute(
            "UPDATE projects SET workspace_id = ?1 WHERE id = ?2",
            rusqlite::params![b.id, "p-scope"],
        )
        .unwrap();

        assert!(list(&conn, None).unwrap().is_empty(), "别区待办不许漏进当前区");
        assert!(list(&conn, Some("p-scope")).unwrap().is_empty());
        assert_eq!(get(&conn, &t.id).unwrap_err().code, "not_found");
        assert_eq!(
            update(
                &conn,
                &t.id,
                &TodoEdit { title: "改一下".to_owned(), due_date: None, done: false }
            )
            .unwrap_err()
            .code,
            "not_found"
        );
        assert_eq!(delete(&conn, &t.id).unwrap_err().code, "not_found");
        // create 的守卫：跨区 project_id 挂不上新行。
        assert_eq!(
            create(
                &conn,
                &TodoInput {
                    project_id: "p-scope".to_owned(),
                    title: "别区挂待办".to_owned(),
                    due_date: None,
                }
            )
            .unwrap_err()
            .code,
            "not_found"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM todos WHERE title = '别区挂待办'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0,
            "被拒的那条一条都不许多"
        );
    }
```

另六条**同形状、各自写全**（下表给出每条的表名、入参结构体与要落的断言；结构体字段已按各模块实际定义写好，`Option<String>` 字段一律用 `Some(…to_owned())`）：

| 测试名（落在哪个文件） | 关联行入参 | 必须落的断言 |
|---|---|---|
| `milestone_list_and_mutators_are_scoped`（`milestone.rs`） | `MilestoneInput { project_id: "p-scope".to_owned(), stage: "立项".to_owned(), plan_date: Some("2026-11-30".to_owned()), note: None }`；`MilestoneEdit { stage: "开发".to_owned(), plan_date: None, note: None, done: false }` | 挪区后：`list(&conn, None)` 空、`list(&conn, Some("p-scope"))` 空、`get` 回 `not_found`、`update` 回 `not_found`、`delete` 回 `not_found`、`create` 回 `not_found` 且 `SELECT COUNT(*) FROM milestones WHERE stage='开发'` 为 0 |
| `expiry_list_and_mutators_are_scoped`（`expiry.rs`） | `ExpiryInput { project_id: "p-scope".to_owned(), kind: "域名".to_owned(), label: "oa.gov.cn".to_owned(), expire_date: Some("2027-01-31".to_owned()), note: None }`；`ExpiryUpdate { expire_date: Some("2028-01-31".to_owned()), note: None }` | 同上六件，表名 `project_expiries`；`get` 那条走的是 `expiry::get` 自己的 JOIN（Step 2 里加的就是那条 SQL），所以它红的形状是「返回到行」而不是「守卫报错」—— 断言仍写 `.unwrap_err().code == "not_found"` |
| `daily_report_list_and_mutators_are_scoped`（`daily_report.rs`） | `DailyReportInput { project_id: "p-scope".to_owned(), report_date: Some("2026-10-23".to_owned()), content: "推进一期".to_owned(), hours: 1.0 }`；`DailyReportEdit { project_id: "p-scope".to_owned(), report_date: Some("2026-10-24".to_owned()), content: "推进二期".to_owned(), hours: 2.0 }` | 同上六件，表名 `daily_reports`；`update` 那条要断**行没被改**（`SELECT content FROM daily_reports WHERE id=?1` 仍是「推进一期」），因为 `update` 里有两道门（宿主项目守卫 + 本行守卫），只断错误码不足以证明库里没落 |
| `ledger_read_and_write_paths_are_scoped`（`ledger.rs`） | `ledger::EnvInput { env: "prod".to_owned(), name: "生产门户".to_owned(), url: None, port: None, note: None }`；`update_env` 复用同一个 `EnvInput` | 用该模块已有的 `seed_project(conn, name) -> String`（`:687`）造项目 + `create_env` 拿行 id；挪区后：`list` 回 `not_found`、`update_env` 回 `not_found`、`delete_env` 回 `not_found`、再 `create_env` 回 `not_found`；并断 `SELECT COUNT(*) FROM ledger_envs` 不增 |
| `directory_mutators_reject_a_foreign_project`（`project.rs`） | `DirInput { path: "C:/m15-scope/root".to_owned(), label: None, sort: None }` | `create_project` 拿项目 → `workspace::create` + 裸 SQL 挪区 → `set_root_dir`/`add_entry_dir`/`remove_dir` 三个各断 `not_found`；`remove_dir` 的 `dir_id` 传任意串（守卫排在前面，跨区时先回 `not_found`）；末断 `SELECT COUNT(*) FROM project_dirs` 为 0 |
| `import_matches_only_active_workspace_projects`（`daily_report.rs`） | 用 `write_xlsx` 现造一份两行表：一行的项目编号落在默认区、另一行落在别区（**两个项目要各自 `switch` 到对应区再 `create_project`**，跨区 create 会被守卫拒；断言阶段活动区是默认区） | `preview` 的 `ok == 1`、`rejected` 里出现别区那行的 `line`；`apply` 回 `invalid_input` 且 `SELECT COUNT(*) FROM daily_reports` 不增。表体沿用该模块既有 `.xlsx` 夹具写法（`export_row`（`:974`）与 `write_xlsx` 都在树里），**不为这条新造导出/导入往返工具** |

Run: `cd src-tauri && cargo test -j 1 --lib`（`cargo test` 只接一个筛选词；要按模块看就分六次跑，别把六个词并列）
Expected: 全绿，`401 passed`；七条新名各自出现。

- [ ] **Step 7: 全量 + 提交**

Run: `cd src-tauri && cargo test -j 1 --lib`
Expected: `401 passed; 0 failed`（394 + 7）。

```bash
git add src-tauri/src/todo.rs src-tauri/src/milestone.rs src-tauri/src/expiry.rs src-tauri/src/daily_report.rs src-tauri/src/ledger.rs src-tauri/src/project.rs src-tauri/src/workspace.rs
git commit -m "feat(m15): 待办/里程碑/到期/日报/台账/目录读写全部按活动区强制守卫"
```

---

## Task 6: 检索与索引呈现层按区过滤

**Files:**
- Modify: `src-tauri/src/search.rs:125-141`（`collect`）、`:159-330`（九段 SQL 与 `field_hits` 头部）
- Modify: `src-tauri/src/index_store.rs:232`（`list_docs`）、`:207`（`indexed_project_count`）、`:283`（`SELECT_SQL`）与 `run_select`（`:322`）
- Modify: `src-tauri/src/doc_preview.rs:72-84`（`row_for_preview`）
- Modify: `src-tauri/src/lib.rs:501`（`index_overview` 持锁段）、`:818`（`ledger_export`）

**Interfaces:**
- Consumes: Task 2 的 `workspace::active_id`/`ensure_active_project`/`active_project_ids`。
- Produces: `collect(source: &'static str, sql: &str, conn: &Connection, pattern: &str, workspace: &str) -> AppResult<Vec<FieldHit>>`（**多一个实参，九处调用点都传**）。

- [ ] **Step 1: 写失败测试（九段同口径 + 正文同口径）**

`search.rs` 的 `mod tests` 末尾：

```rust
    /// 九段检索**每一段**都要带区谓词。同一串关键词「串码」埋进活动区与别区的**各**数据类里：
    /// 只改项目段、或漏掉任一条 JOIN 段，都会让另一边的命中漏进当前区 —— 变异 M⑥ 打的就是这一格。
    #[test]
    fn field_hits_are_scoped_in_every_section() {
        let (conn, a, _mk) = fixture();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        crate::workspace::switch(&conn, &b.id).unwrap();
        let bp = seed_project(&conn, "政务云迁移乙");
        crate::workspace::switch(&conn, crate::workspace::DEFAULT_ID).unwrap();

        // 关联行必须在**它宿主项目所属的那一区**里创建：Task 5 之后 `todo::create` 一类的 create 守卫
        // 会拒跨区（`project_exists` 已带区谓词）。所以两段各切一次区，而不是在活动区=默认区的循环里
        // 给别区的项目建行（那会当场 panic 在 `.unwrap()`）。
        for (pid, ws) in [(&a, crate::workspace::DEFAULT_ID.to_owned()), (&bp, b.id.clone())] {
            crate::workspace::switch(&conn, &ws).unwrap();
            todo::create(
                &conn,
                &todo::TodoInput {
                    project_id: pid.to_owned(),
                    title: "串码待办".to_owned(),
                    due_date: None,
                },
            )
            .unwrap();
            daily_report::create(
                &conn,
                &daily_report::DailyReportInput {
                    project_id: pid.to_owned(),
                    report_date: Some("2026-10-23".to_owned()),
                    content: "串码日报".to_owned(),
                    hours: 1.0,
                },
            )
            .unwrap();
            expiry::create(
                &conn,
                &expiry::ExpiryInput {
                    project_id: pid.to_owned(),
                    kind: "域名".to_owned(),
                    label: "串码域名".to_owned(),
                    expire_date: Some("2027-01-31".to_owned()),
                    note: None,
                },
            )
            .unwrap();
            ledger::create_env(
                &conn,
                pid,
                &ledger::EnvInput {
                    env: "prod".to_owned(),
                    name: "串码环境".to_owned(),
                    url: None,
                    port: None,
                    note: None,
                },
            )
            .unwrap();
            ledger::create_note(
                &conn,
                pid,
                &ledger::NoteInput {
                    title: "串码备注".to_owned(),
                    body: Some("串码正文".to_owned()),
                    tag: None,
                },
            )
            .unwrap();
        }
        crate::workspace::switch(&conn, crate::workspace::DEFAULT_ID).unwrap();

        let hits = field_hits(&conn, "串码").unwrap();
        assert!(
            hits.iter().all(|h| h.project_id == a),
            "有别的区的行漏进当前区：{:?}",
            hits.iter().map(|h| (h.source, h.project_id.clone())).collect::<Vec<_>>()
        );
        // 每段都要**有**命中，否则「全在同一区」会因为某段压根没数据而假绿。
        assert_eq!(
            sources(&hits),
            vec!["env", "note", "daily", "todo", "expiry"],
            "九段里这五段各该留一条活动区命中：{:?}",
            sources(&hits)
        );

        // 项目段单独验一次（`seed_project` 的 code 两边同为 `P-2026-009`）：
        let by_code = field_hits(&conn, "P-2026-009").unwrap();
        assert_eq!(by_code.len(), 1, "两个区各一条同码项目，过滤后只能剩活动区那一条");
        assert!(by_code.iter().all(|h| h.project_id == a), "项目段没带区谓词：{:?}", sources(&by_code));
    }
```

> `fixture()`（`search.rs:395`）、`seed_project()`（`:403`）、`sources()`（`:511`）都是该模块**已有**夹具，不新造。`sources(&hits)` 的顺序等于 `field_hits` 里九段的排列顺序（项目段最先，`unified_bundle` 才靠 `partition` 把它单独拎出来）。

`index_store.rs` 的 `mod tests` 末尾：

```rust
    /// 正文命中与首屏空态谓词**必须同口径**（M3 起就在的那句注释：少半个谓词就会出现
    /// 「文案说正文有索引、正文段却是空的」）。两个区各写一条 ok 正文，
    /// 断 `doc_hits` 与 `indexed_project_count` 同时只认活动区。
    #[test]
    fn doc_hits_and_indexed_project_count_use_the_same_scope() {
        let c = conn();
        seed_project(&c, "p-a");
        let b = crate::workspace::create(&c, "客户乙").unwrap();
        crate::workspace::switch(&c, &b.id).unwrap();
        seed_project(&c, "p-b");
        crate::workspace::switch(&c, crate::workspace::DEFAULT_ID).unwrap();

        for pid in ["p-a", "p-b"] {
            write_doc(&c, pid, &file(&format!("C:/{pid}.docx")), DocOutcome::Ok("付款通知".into()))
                .unwrap();
        }

        assert_eq!(indexed_project_count(&c).unwrap(), 1, "只数活动区的可搜项目");
        let hits = doc_hits(&c, "付款", 50).unwrap();
        assert_eq!(hits.len(), 1, "当前区的正文命中只能有一条：{}", hits.len());
        assert_eq!(hits[0].project_id, "p-a");

        crate::workspace::switch(&c, &b.id).unwrap();
        let hits = doc_hits(&c, "付款", 50).unwrap();
        assert_eq!(hits[0].project_id, "p-b", "切过去要换另一半");
        assert_eq!(indexed_project_count(&c).unwrap(), 1, "谓词同口径：切换后计数仍是 1");
    }
```

> `conn()` / `seed_project()` / `file()`（`index_store.rs:347-375`）是已有夹具；`seed_project` 走裸 INSERT，`workspace_id` 由列 DEFAULT 落 `default`，正好等于首屏的活动区。

- [ ] **Step 2: `search.rs` —— 一处签名 + 九段谓词**

```rust
fn collect(
    source: &'static str,
    sql: &str,
    conn: &Connection,
    pattern: &str,
    workspace: &str,
) -> AppResult<Vec<FieldHit>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![pattern, PER_GROUP_LIMIT, workspace], map_row)?;
```

`field_hits` 在 `let p = like_pattern(q);` 之后加 `let workspace = crate::workspace::active_id(conn)?;`，九个 `hits.append(&mut collect(` 调用的最后一个实参从 `&p,` 改成 `&p,\n        &workspace,`，并在每条 SQL 里加 `?3` 谓词：项目段（`:162`）在 `WHERE p.deleted_at IS NULL` 之后加 `AND p.workspace_id = ?3`；其余八段（`:181/:198/:214/:230/:246/:263/:279/:298`）在各自的 `JOIN projects p ON … AND p.deleted_at IS NULL` 里追加 `AND p.workspace_id = ?3`。

> **工作区名不进检索**：不加第十段 `collect`，`hits.append(&mut collect(` 的个数必须仍是 **9**（spec §九；Task 9 Step 1 的结构门会抓）。九段全改完后当场跑一次 `grep -c` 自证。
Run: `cd src-tauri && cargo test -j 1 --lib search::tests::field_hits_are_scoped_in_every_section`
Expected: PASS（红过之后）。九段全改完才可能绿 —— 少改一段，`sources` 那条断言就红；段数加了，`collect` 门就红。


- [ ] **Step 3: `index_store.rs` 三处**

`SELECT_SQL`（`:283`）：`AND p.deleted_at IS NULL` 之后加一行 `   AND p.workspace_id = ?3`；`run_select` 的 `params![expr, limit]` 改 `params![expr, limit, crate::workspace::active_id(conn)?]`（`run_select` 已经收 `conn`）。
`indexed_project_count`（`:207`）：SQL 加 `AND p.workspace_id = ?1`，`query_row` 实参改 `params![crate::workspace::active_id(conn)?]`。
`list_docs`（`:232`）：首行加 `crate::workspace::ensure_active_project(conn, project_id)?;`。`status_counts` **不加**（它的调用方 `index_overview` 已按区过滤，加了就是每项目多一次查询）。

Run: `cd src-tauri && cargo test -j 1 --lib index_store::tests::doc_hits_and_indexed_project_count_use_the_same_scope`
Expected: PASS。

- [ ] **Step 4: `doc_preview.rs` + `lib.rs` 两条命令**

`row_for_preview`（`:72`）：SQL 加 `AND p.workspace_id = ?2`，实参改 `params![doc_id, crate::workspace::active_id(conn)?]`（跨区 `doc_id` 走它现有的那条 `unavailable` 文案，**不新增错误码**）。

`index_overview`（`lib.rs:501`）的持锁段改成：

```rust
    let (opts, targets, counts, has_fts) = {
        let conn = db(&state)?;
        let opts = index_scan::ScanOptions::load(&conn)?;
        // 呈现层按区收窄，worker 不动：`targets` 全库取回来后用一次集合过滤（spec §4.1），
        // 过滤排在 status_counts 之前，别为看不见的项目白跑一轮 per-项目查询。
        let active = workspace::active_project_ids(&conn)?;
        let targets = index_job::targets(&conn, None)?
            .into_iter()
            .filter(|t| active.contains(&t.project_id))
            .collect::<Vec<_>>();
        let mut counts = Vec::with_capacity(targets.len());
        for t in &targets {
            counts.push(index_store::status_counts(&conn, &t.project_id)?);
        }
        (opts, targets, counts, fts5_available(&conn))
    };
```

`ledger_export`（`lib.rs:818`）在 `let conn = db(&state)?;` 之后、`unlocked_mk` 之前加：

```rust
    // 区守卫排在取 mk 之前：跨区项目不许导出（也不该为它弹解锁）。
    workspace::ensure_active_project(&conn, &input.project_id)?;
```

- [ ] **Step 5: 命令层接线测试（`lib.rs` 已有的 `include_str!` 形制，不是伪代码）**

命令体要 `State<'_, AppState>`，本仓在单测里合成不了；**判据本体已经在 `workspace.rs`（Task 2 Step 3）与模块层测过**，这一格钉的是「命令层确实接上了、并且接在正确的位置」，形制照 `lib.rs:1206 ledger_export_wires_the_guard_before_any_write`。加在 `lib.rs` 的 `mod tests` 里：

```rust
    /// M15：`ledger_export` 的区守卫必须**恰好一次**且早于取数；`index_overview` 的区过滤必须
    /// 排在 `status_counts` 循环之前。needle 一律 `concat!` 拼 —— 写成字面量会把本测试自己数进去
    /// （坑 33，与上面 `backup_export` / `ledger_export` 两条同款）。
    #[test]
    fn workspace_guards_are_wired_before_the_work_in_command_bodies() {
        let src = include_str!("lib.rs");

        let lstart = src
            .find(concat!("fn ledger_", "export("))
            .expect("ledger_export 必须还在 lib.rs 里");
        let lbody = &src[lstart..lstart + src[lstart..].find("\n}").expect("函数体收尾")];
        let lguard = concat!("workspace::ensure_active_", "project(");
        assert_eq!(lbody.matches(lguard).count(), 1, "命令层必须恰好调用一次区守卫");
        assert!(
            lbody
                .find(lguard)
                .unwrap()
                < lbody.find(concat!("ledger_export::", "collect(")).unwrap(),
            "区守卫必须早于取数：跨区项目连导出的门都不该进"
        );

        let ostart = src
            .find(concat!("fn index_", "overview("))
            .expect("index_overview 必须还在 lib.rs 里");
        let obody = &src[ostart..ostart + src[ostart..].find("\n}").expect("函数体收尾")];
        let ofilter = concat!("workspace::active_", "project_ids(");
        let counts = concat!("index_store::status_", "counts(");
        assert_eq!(obody.matches(ofilter).count(), 1, "总览的候选集合只该取一次活动区");
        assert!(
            obody.find(ofilter).unwrap() < obody.find(counts).unwrap(),
            "区过滤要排在 status_counts 之前：别区的项目不该白跑一轮 per-项目查询"
        );
    }
```

Run: `cd src-tauri && cargo test -j 1 --lib lib::tests::workspace_guards_are_wired`
Expected: PASS。如果 `find("\n}")` 截到的函数体比预期短（`index_overview` 里有嵌套块），**先确认截到的确实覆盖到 `status_counts` 那一行**再改 needle 位置，不要靠放宽断言蒙过去。
命令体行为的完整证据走真机格 C-11（`/index` 只剩当前区项目）与 C-12（备份往返），在账本里记一句「命令体不可单测注入 State，走真机」——这是有名缺口，不是漏测。

- [ ] **Step 6: 全量回归**

Run: `cd src-tauri && cargo test -j 1 --lib`
Expected: `404 passed; 0 failed`（401 + 3）。**这一格最容易红既有测试**：`search.rs` 有四十来条既有测试走 `field_hits`，`index_store.rs` 也是。它们的项目全在默认区 = 活动区，所以红 = 谓词写错位置（例如把 `?3` 绑到 `LIMIT` 之前、或 `run_select` 少传一个实参导致 `params!` 索引错位）。按红的位置改实现，**不许**为了让既有测试绿而放宽谓词。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/search.rs src-tauri/src/index_store.rs src-tauri/src/doc_preview.rs src-tauri/src/lib.rs
git commit -m "feat(m15): 检索九段、正文命中与首屏谓词、索引总览与台账导出全部按活动区过滤"
```

---

## Task 7: 前端线格式、API、store 与三个纯函数

**Files:**
- Create: `src/types/workspace.ts`、`src/lib/workspace-view.ts`、`src/stores/workspaces.ts`、`tests/workspace-view.test.ts`
- Modify: `src/lib/api.ts`（M14 `listAudit` 之后加五条）、`src/types/project.ts:12-27`（`Project`）、`:29-42`（`ProjectInput`）、`:44-57`（`EMPTY_INPUT`）、`:59-74`（`toInput`）

**Interfaces:**
- Consumes: Task 2 的五条命令名（`workspace_list` / `workspace_create` / `workspace_rename` / `workspace_delete` / `workspace_switch`）与 `Workspace` 的 camelCase 四键。
- Produces（Task 8 全靠这些名字）：
  - `Workspace { id: string; name: string; projectCount: number; isActive: boolean }`
  - `listWorkspaces()` / `createWorkspace(name)` / `renameWorkspace(id, name)` / `deleteWorkspace(id)` / `switchWorkspace(id)`
  - `useWorkspacesStore` → `{ items, activeId, loading, saving, error, refresh, create, rename, remove, switchTo, clearError }`
  - `nameHint(name): string | null`、`deleteGuard(ws, activeId): string | null`、`activeWorkspaceId(items): string | null`、`WORKSPACE_NAME_MAX = 40`、`DEFAULT_WORKSPACE_ID = "default"`
  - `Project.workspaceId: string`、`ProjectInput.workspaceId?: string | null`

- [ ] **Step 1: `src/types/workspace.ts`**

```ts
// 与 Rust 侧 #[serde(rename_all = "camelCase")] 的字段一一对应（`workspace.rs` 的 `Workspace`）。
// `Option<String>` 序列化成 null，所以这里可空字段一律写 `| null`，不写可选属性；
// `workspace.ts` 四件都是非空的，所以没有 `?`。
export interface Workspace {
  id: string;
  name: string;
  /** 只数未软删的项目（管理弹窗里的显示值）。后端的「非空不许删」守卫**含软删行**，
   *  两个口径不同是刻意的，别把它当删除可行性判据的唯一来源（判据在 `deleteGuard` + 后端）。 */
  projectCount: number;
  /** 这一行是不是当前区。不为它单开第 6 条命令（spec D6 拍的是 5 条）。 */
  isActive: boolean;
}
```

- [ ] **Step 2: `src/types/project.ts` 四处**

`Project` 在 `parentId: string | null;` 之后加：

```ts
  /** V11 起 NOT NULL，读出来恒有值。 */
  workspaceId: string;
```

`ProjectInput` 在 `parentId: string | null;` 之后加：

```ts
  /** 缺省（`undefined`/`null`）= **不改归属**；`create` 一律忽略这一格（后端恒落活动区，
   *  spec §六 第 1 条）。写成可选属性而不是 `string | null`，是为了不破坏
   *  `apply-template.ts` / `template-manager.tsx:31` 那几处已有的 `ProjectInput` 字面量。 */
  workspaceId?: string | null;
```

`EMPTY_INPUT` **不加**这一格（新建 = 不改区 = 后端用活动区），只在其上方补一行注释：

```ts
// 新建表单的初值不含 workspaceId：create 恒落活动区（M15 spec §六），传了也会被后端忽略。
export const EMPTY_INPUT: ProjectInput = {
```

`toInput` 在 `parentId: p.parentId ?? null,` 之后加 `workspaceId: p.workspaceId,`。

- [ ] **Step 3: `src/lib/api.ts` 五条包装**

紧跟既有 `listAudit`（M14）之后：

```ts
// —— M15 工作区（五条同步命令；切换器与「管理工作区」弹窗的唯一出口）——
export const listWorkspaces = () => invoke<Workspace[]>("workspace_list");
export const createWorkspace = (name: string) => invoke<Workspace>("workspace_create", { name });
export const renameWorkspace = (id: string, name: string) =>
  invoke<Workspace>("workspace_rename", { id, name });
export const deleteWorkspace = (id: string) => invoke<void>("workspace_delete", { id });
export const switchWorkspace = (id: string) => invoke<string>("workspace_switch", { id });
```

并在文件顶部的 type import 组里加 `import type { Workspace } from "@/types/workspace";`。

- [ ] **Step 4: 先写失败的纯函数测试（四条，`node --test`）**

新建 `tests/workspace-view.test.ts`（形制照 `tests/audit-view.test.ts`：`node:test` + `node:assert/strict` + `.ts` 相对路径 import + 一个 `const ws = (patch) => …` 夹具）：

```ts
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
```

Run: `node --test tests/workspace-view.test.ts`
Expected: FAIL —— `Cannot find module '.../src/lib/workspace-view.ts'`。

- [ ] **Step 5: 写 `src/lib/workspace-view.ts`**

```ts
import type { Workspace } from "@/types/workspace";

/// 与后端 `workspace.rs` 的 `NAME_MAX_CHARS` 同一个数：改了这边没改那边，
/// 结果就是「前端放行、后端当场拒」，用户看见的是一条莫名其妙的错误。
export const WORKSPACE_NAME_MAX = 40;
export const DEFAULT_WORKSPACE_ID = "default";

/** 区名的可见判据。后端是唯一的真门（本项目只在边界校验一次），
 *  这一格只为弹窗提前给一句人话，不是第二道校验。 */
export function nameHint(name: string): string | null {
  const trimmed = name.trim();
  if (!trimmed) return "工作区名称不能为空";
  if ([...trimmed].length > WORKSPACE_NAME_MAX) {
    return `工作区名称不能超过 ${WORKSPACE_NAME_MAX} 个字符`;
  }
  return null;
}

/** 删除按钮的禁用原因；`null` = 可删。
 *  三条原因的顺序与文案与后端 `workspace::delete` 的三道守卫逐字对应（M15 spec §五）。 */
export function deleteGuard(ws: Workspace, activeId: string | null): string | null {
  if (ws.id === DEFAULT_WORKSPACE_ID) return "默认工作区不可删除";
  if (activeId !== null && ws.id === activeId) return "当前工作区不可删除，请先切换到其他工作区";
  if (ws.projectCount > 0) return "该工作区下还有项目（含已删除项目），不能删除";
  return null;
}

/** 当前区 id 的唯一取法：`items` 里 `isActive` 那格。store 不另存一份真值。 */
export function activeWorkspaceId(items: Workspace[]): string | null {
  return items.find((w) => w.isActive)?.id ?? null;
}
```

> `deleteGuard` 的第二条用 `activeId !== null` 而不是 `&&` 短路成 falsy：`activeId` 还没加载完（`null`）时**不能**把某一行误判成「当前区」。

Run: `node --test tests/workspace-view.test.ts`
Expected: `pass 4 / fail 0`。
Run: `node --test "tests/**/*.test.ts"`
Expected: `tests 75 / pass 75 / fail 0`（基线 71 + 4）。

- [ ] **Step 6: `src/stores/workspaces.ts`**

```ts
import { create } from "zustand";
import * as api from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { activeWorkspaceId } from "@/lib/workspace-view";
import type { Workspace } from "@/types/workspace";

// 后端的每个写命令都回整行 Workspace，直接覆盖本地行，不在前端重算 projectCount。
function upsert(items: Workspace[], w: Workspace): Workspace[] {
  const i = items.findIndex((x) => x.id === w.id);
  if (i === -1) return [...items, w];
  const next = items.slice();
  next[i] = w;
  return next;
}

interface WorkspacesState {
  items: Workspace[];
  activeId: string | null;
  loading: boolean;
  saving: boolean;
  error: AppErrorShape | null;
  refresh: () => Promise<void>;
  create: (name: string) => Promise<boolean>;
  rename: (id: string, name: string) => Promise<boolean>;
  remove: (id: string) => Promise<boolean>;
  switchTo: (id: string) => Promise<boolean>;
  clearError: () => void;
}

export const useWorkspacesStore = create<WorkspacesState>((set, get) => {
  const run = async (fn: () => Promise<Workspace | void>): Promise<boolean> => {
    set({ saving: true, error: null });
    try {
      const w = await fn();
      if (w) set({ items: upsert(get().items, w) });
      return true;
    } catch (e) {
      set({ error: toAppError(e) });
      return false;
    } finally {
      set({ saving: false });
    }
  };

  return {
    items: [],
    activeId: null,
    loading: true,
    saving: false,
    error: null,

    refresh: async () => {
      set({ loading: true, error: null });
      try {
        const items = await api.listWorkspaces();
        set({ items, activeId: activeWorkspaceId(items) });
      } catch (e) {
        set({ error: toAppError(e) });
      } finally {
        set({ loading: false });
      }
    },

    // 新建**不自动切过去**（spec §五）：run() 里只 upsert 一行，activeId 原样不动。
    create: (name) => run(() => api.createWorkspace(name)),

    rename: (id, name) => run(() => api.renameWorkspace(id, name)),

    remove: (id) =>
      run(async () => {
        await api.deleteWorkspace(id);
        set({ items: get().items.filter((w) => w.id !== id) });
      }),

    // 切换成功后整份重取：区名没变，但 isActive 与各页可见数据全变了。
    // 在前端手改标志位等于造第二个真值（spec §8.2）。顶栏靠 activeId 变化触发 <Outlet> 重挂载。
    switchTo: async (id) => {
      set({ saving: true, error: null });
      try {
        await api.switchWorkspace(id);
      } catch (e) {
        set({ error: toAppError(e) });
        return false;
      } finally {
        set({ saving: false });
      }
      await get().refresh();
      return true;
    },

    clearError: () => set({ error: null }),
  };
});
```

- [ ] **Step 7: 类型门 + 提交**

`Project.workspaceId` 是**必填**，所以 `src/pages/projects.tsx:295` 的 `emptySeed(): Project` 字面量必须在本格一起补上（否则 `tsc` 当场红）：在 `parentId: null,` 之后加

```ts
    // 新建不参与区归属：create 恒落活动区（M15 spec §六）。这里给空串只为满足类型，
    // toInput 之后它会被 normalize 成「不改」，后端也忽略。
    workspaceId: "",
```

> `tsconfig.json` 的 `include` 只有 `["src"]`（控制方 2026-10-23 现读），所以 `tests/project-tree.test.ts:10` 那个 `: Project` 工厂**不进 tsc**（`node --test` 只擦类型不校验），本格不动它。

Run: `npm run build 2>&1 | grep -c "error TS"`
Expected: `0`。**如果 tsc 报出别的 `Project` 字面量缺 `workspaceId`，就地补齐并在报告里点名补了哪几处** —— 那是本格的活，不留给 Task 8。（`noUnusedLocals: true` 是开着的：本 Step 新建文件里未使用的本地变量会直接红。）

```bash
git add src/types/workspace.ts src/types/project.ts src/lib/api.ts src/lib/workspace-view.ts src/stores/workspaces.ts tests/workspace-view.test.ts
git commit -m "feat(m15): 前端工作区线格式/API/store 与三个纯函数（node 71→75）"
```

---

## Task 8: 顶栏切换器、管理弹窗、换区重挂载与项目表单的「工作区」一行

**Files:**
- Create: `src/components/workspace-switcher.tsx`、`src/components/workspace-manager.tsx`
- Modify: `src/components/app-shell.tsx:22-24`（mount effect）、`:41-43`（header 右侧）、`:46-48`（`<main>`）
- Modify: `src/components/project-form.tsx:31-51`（props）、`:74-91`（值与提交归一化）、`:127-149`（渲染，紧挨「所属总包」）
- Modify: `src/pages/projects.tsx:105`（`emptySeed`）、`:208`（`parentOptions` 那行旁边加 `workspaceOptions`）、`:295-311`（`emptySeed` 体）
- Modify: `src/pages/project-detail.tsx:340`

**Interfaces:**
- Consumes: Task 7 的 `useWorkspacesStore`、`Workspace`、`nameHint`、`deleteGuard`、`Project.workspaceId`。
- Produces: 无（这是最外层接线）。

- [ ] **Step 1: `src/components/workspace-manager.tsx`**

形制照 `src/components/template-manager.tsx`（同一套 `Dialog` + `error` 条 + 空态 + `<li>` 行 + 二次确认弹窗），差别是这里没有子表单，改名用一行就地输入：

```tsx
import { useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { deleteGuard, nameHint } from "@/lib/workspace-view";
import { useWorkspacesStore } from "@/stores/workspaces";
import type { Workspace } from "@/types/workspace";

export function WorkspaceManager({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { items, activeId, loading, saving, error, refresh, create, rename, remove } =
    useWorkspacesStore();
  const [draft, setDraft] = useState("");
  const [editing, setEditing] = useState<{ id: string; name: string } | null>(null);
  const [confirming, setConfirming] = useState<Workspace | null>(null);
  const draftHint = draft.trim() ? nameHint(draft) : null;

  useEffect(() => {
    if (open) void refresh();
  }, [open, refresh]);

  const close = () => {
    setEditing(null);
    setConfirming(null);
    setDraft("");
    onOpenChange(false);
  };

  return (
    <>
      <Dialog open={open} onOpenChange={(o) => (o ? onOpenChange(true) : close())}>
        <DialogContent className="max-h-[90vh] overflow-auto sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>管理工作区</DialogTitle>
            <DialogDescription>
              工作区把项目按客户或类型分开；切换工作区等于换整个视野。这是逻辑分类，
              不是访问控制 —— 数据都在同一个本地库里。
            </DialogDescription>
          </DialogHeader>

          {error && (
            <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              <div>{error.message}</div>
              {error.hint && <div className="mt-0.5 opacity-80">建议：{error.hint}</div>}
            </div>
          )}

          <div className="flex items-end gap-2">
            <div className="grid flex-1 gap-1.5">
              <Input
                value={draft}
                placeholder="新工作区名称，例：客户乙"
                onChange={(e) => setDraft(e.target.value)}
              />
              {draftHint && <p className="text-xs text-destructive">{draftHint}</p>}
            </div>
            <Button
              size="sm"
              disabled={saving || draftHint !== null || !draft.trim()}
              onClick={async () => {
                if (await create(draft)) setDraft("");
              }}
            >
              <Plus /> 新建
            </Button>
          </div>

          {loading ? (
            <p className="text-sm text-muted-foreground">读取工作区…</p>
          ) : (
            <ul className="grid gap-2">
              {items.map((w) => {
                const reason = deleteGuard(w, activeId);
                return (
                  <li key={w.id} className="flex items-center gap-2 rounded-xl border bg-background px-3 py-2">
                    <div className="min-w-0 flex-1">
                      {editing?.id === w.id ? (
                        <Input
                          value={editing.name}
                          autoFocus
                          onChange={(e) => setEditing({ id: w.id, name: e.target.value })}
                          onKeyDown={async (e) => {
                            if (e.key === "Enter" && !nameHint(editing.name)) {
                              if (await rename(w.id, editing.name)) setEditing(null);
                            } else if (e.key === "Escape") {
                              setEditing(null);
                            }
                          }}
                        />
                      ) : (
                        <div className="flex items-center gap-2">
                          <span className="truncate text-sm font-medium">{w.name}</span>
                          {w.isActive && <Badge variant="secondary">当前</Badge>}
                          <span className="text-xs text-muted-foreground">{w.projectCount} 个项目</span>
                        </div>
                      )}
                    </div>
                    <Button size="sm" variant="ghost" disabled={saving} onClick={() => setEditing({ id: w.id, name: w.name })}>
                      改名
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      title={reason ?? "删除"}
                      disabled={loading || reason !== null}
                      onClick={() => setConfirming(w)}
                    >
                      <Trash2 />
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}
        </DialogContent>
      </Dialog>

      <Dialog open={confirming !== null} onOpenChange={(o) => !o && setConfirming(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>删除工作区</DialogTitle>
            <DialogDescription>
              只删除空的「{confirming?.name}」；里面已经有项目时删不掉，项目本身也不会被删。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setConfirming(null)}>
              取消
            </Button>
            <Button
              variant="destructive"
              disabled={saving}
              onClick={async () => {
                if (!confirming) return;
                if (await remove(confirming.id)) setConfirming(null);
              }}
            >
              确认删除
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
```

- [ ] **Step 2: `src/components/workspace-switcher.tsx`**

```tsx
import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { WorkspaceManager } from "@/components/workspace-manager";
import { useWorkspacesStore } from "@/stores/workspaces";

/// 顶栏的全局工作区切换器 + 「管理工作区」入口（spec §8.3）。
/// 值取 store 的 activeId（也就是 `items` 里 isActive 那一格），本地不另存一份。
export function WorkspaceSwitcher() {
  const { items, activeId, loading, saving, switchTo } = useWorkspacesStore();
  const [managerOpen, setManagerOpen] = useState(false);

  return (
    <div className="flex items-center gap-1">
      <Select
        value={activeId ?? undefined}
        onValueChange={(v) => void switchTo(String(v))}
        disabled={loading || items.length === 0 || saving}
      >
        <SelectTrigger className="h-8 w-44" title="当前工作区">
          <SelectValue placeholder="工作区" />
        </SelectTrigger>
        <SelectContent>
          {items.map((w) => (
            <SelectItem key={w.id} value={w.id}>
              {w.name}（{w.projectCount}）
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Button size="sm" variant="ghost" onClick={() => setManagerOpen(true)}>
        管理
      </Button>
      <WorkspaceManager open={managerOpen} onOpenChange={setManagerOpen} />
    </div>
  );
}
```

- [ ] **Step 3: `app-shell.tsx` 三处接线**

import 组加 `import { WorkspaceSwitcher } from "@/components/workspace-switcher";` 与 `import { useWorkspacesStore } from "@/stores/workspaces";`；组件体里：

```tsx
  const refreshWorkspaces = useWorkspacesStore((s) => s.refresh);
  const activeWorkspaceId = useWorkspacesStore((s) => s.activeId);

  useEffect(() => {
    void refresh();
    void refreshVault();
    void refreshWorkspaces();
  }, [refresh, refreshVault, refreshWorkspaces]);
```

header 右侧那一格（现 `<div className="ml-auto"><VaultControl /></div>`）换成：

```tsx
        <div className="ml-auto flex items-center gap-2">
          <WorkspaceSwitcher />
          <VaultControl />
        </div>
```

`<main>` 里给 `<Outlet />` 套一层 key（换区即换整个视野，见 spec §8.3；`activeId` 还没取到时用 `"boot"`，后端已按活动区过滤，早到的请求不会串数据）：

```tsx
      <main className="flex-1 overflow-auto">
        <div key={activeWorkspaceId ?? "boot"}>
          <Outlet />
        </div>
      </main>
```

- [ ] **Step 4: `project-form.tsx` 的「工作区」一行**

props 加一项（放在 `parentOptions` 之后）：

```tsx
  workspaceOptions?: Workspace[];
```
并在 import 组加 `import type { Workspace } from "@/types/workspace";`；解构里加 `workspaceOptions`。

值与提交归一化（紧挨既有的 `currentParentId` / `parentValue` 那段）：

```tsx
  // 换区只对「根项目」开放：有父（库里的或本次提交的）就随父走（spec §六.2）。
  const hasParent = form.parentId !== null;
  const workspaceValue =
    form.workspaceId ?? workspaceOptions?.find((w) => w.isActive)?.id ?? "";
  const workspaceHint = hasParent ? "子项目随父项目一起换区，请编辑它的父项目" : null;
```

`submit()` 里在 `parentId` 归一化之后补一行（**有父时强制提交 `null` = 不改区**，与后端的拒绝同向）：

```tsx
    const workspaceId = hasParent ? null : (form.workspaceId ?? null);
    const ok = await onSubmit({ ...form, parentId, workspaceId, tags: parseTags(tagsText) });
```

渲染（放在「所属总包」那一格之后）：

```tsx
      {workspaceOptions && (
        <div className="grid gap-2">
          <Label>工作区</Label>
          <Select
            value={workspaceValue}
            onValueChange={(v) => patch("workspaceId", String(v))}
            disabled={hasParent || workspaceOptions.length === 0}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {workspaceOptions.map((w) => (
                <SelectItem key={w.id} value={w.id}>
                  {w.name}（{w.projectCount}）
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {workspaceHint && <p className="text-xs text-muted-foreground">{workspaceHint}</p>}
        </div>
      )}
```

> `patch("workspaceId", …)` 复用既有的 `patch`；它的第二个实参是 `string`，`ProjectInput.workspaceId` 收 `string | null | undefined` 兼容。`workspaceOptions` **只在编辑根项目/普通编辑时传**（新建不传 ⇒ 整行不渲染，与 spec §8.4 一致；`TemplateManager` 用的那个 `ProjectForm` 也不传 ⇒ 模板表单里不会出现这一行）。
> 注意 `workspaceValue` 不能是空串（Radix `Select` 的 `value=""` 会当未选中处理）—— `workspaceOptions` 非空时它必然命中 `form.workspaceId` 或 `isActive` 那格；两者都取不到时 `Select` 显示 placeholder，用户选一次即写入。

- [ ] **Step 5: 两个页面的接线**

`src/pages/projects.tsx`：
1. import 组加 `import { useWorkspacesStore } from "@/stores/workspaces";`。
2. 组件里取 `const workspaces = useWorkspacesStore((s) => s.items);` 与 `const refreshWorkspaces = useWorkspacesStore((s) => s.refresh);`，并把后者挂进既有 `useEffect`（`:45-48` 那个，实参数组同步加 `refreshWorkspaces`）：`void refreshWorkspaces();`。
3. `ProjectForm` 那一处（`:203-215`）加一行：`workspaceOptions={editing.id === null ? undefined : workspaces}`。
4. （`emptySeed()` 的 `workspaceId: ""` 已在 Task 7 Step 7 补过 —— 那是 tsc 门的要求，本格不重复、也不改它。）

`src/pages/project-detail.tsx`：同一件 —— import `useWorkspacesStore`，取 `const workspaces = useWorkspacesStore((s) => s.items);`（该页已有 `useEffect` 里的 `refresh` 队列，把 `refreshWorkspaces` 一起挂上），在 `ProjectForm`（`:336-347`）加一行 `workspaceOptions={workspaces}`（详情页恒是编辑态，直接传）。

- [ ] **Step 6: 类型门 + 手测口径**

Run: `npm run build 2>&1 | grep -c "error TS"`
Expected: `0`。
Run: `node --test "tests/**/*.test.ts"`
Expected: `75 / 75 / 0`（本 Task 不加测试）。
UI 的完整证据走 Task 9 的真机格（C-1…C-12）；本 Task **不**在开发机上跑 `npm run dev`/`tauri dev`（红线）。在报告里如实写「未做浏览器手测，UI 判据由 Task 9 真机格承担」，不要声称看过界面。

- [ ] **Step 7: 提交**

```bash
git add src/components/workspace-switcher.tsx src/components/workspace-manager.tsx src/components/app-shell.tsx src/components/project-form.tsx src/pages/projects.tsx src/pages/project-detail.tsx
git commit -m "feat(m15): 顶栏工作区切换器与管理弹窗、换区重挂载整棵页面子树、项目表单工作区一行"
```

---

## Task 9: 全量门禁 + 变异取证 + 真机十二格 + spec↔计划映射表

**这一格由控制方跑，不派实现者**（变异要改生产代码、真机要跑 `tauri dev`，两条都在子代理的红线之外）。

**Files:**
- Create（证据，全部落在 git-ignored 的账本目录里，**收口前不要 `git clean -fdx`**）：
  `.superpowers/sdd/2026-10-23-m15-workspaces/{mutation-evidence.txt,real-machine-readings.md,spec-plan-mapping.md,m15-cdp.mjs,m15-cdp.stdout.txt,m15-backup.zip}`
- Modify: 无生产代码（变异下靶后必须逐条还原并 `git diff --exit-code` 验 0）

- [ ] **Step 1: 整套门禁（终态树现跑，逐个真实退出码）**

```bash
cd src-tauri && cargo test -j 1 --lib; echo "exit=$?"
cd src-tauri && cargo clippy -j 1 --lib -- -D warnings; echo "exit=$?"
cd src-tauri && cargo clippy -j 1 --lib --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo build -j 1 --lib; echo "exit=$?"
node --test "tests/**/*.test.ts" 2>&1 | tail -8
npm run build 2>&1 | tail -5
```
+ 结构门（仓库根）：

```bash
grep -c "TARGET_VERSION: i64 = 11" src-tauri/src/db.rs
awk '/generate_handler!\[/,/\]\)/' src-tauri/src/lib.rs | grep -cE '^            [a-z_]+,?$'   # 应为 75
grep -c "tauri::command(async)" src-tauri/src/lib.rs
grep -rc "allow(dead_code)" src-tauri/src/ | grep -v ":0"
grep -c '^\[\[package\]\]' src-tauri/Cargo.lock
grep -c "hits.append(&mut collect(" src-tauri/src/search.rs
git diff --stat master..HEAD -- src-tauri/Cargo.lock src-tauri/Cargo.toml package.json   # 必须空输出
git rev-list --merges --count --all
```

Expected 读数（**期望值不是断言，跑出来不符就报差异并查成因**）：cargo `404 passed`、node `tests 75 / pass 75 / fail 0`、`npm run build` `error TS` 0、`TARGET_VERSION 11`、命令 **75**、async **7**、豁免 **1**（`db.rs`）、`Cargo.lock` **616**、collect **9**、依赖 diff **空**、merge commit **0**。
`grep -c ... | grep -v ":0"` 这类管道会把 `$?` 吞掉：**退出码一律单独 `echo "exit=$?"` 取**（本仓前科：管道 tail 吞掉退出码，把红报成绿）。

- [ ] **Step 2: 十一处变异，逐处下靶 → 跑指定测试 → 记退出码 → 还原**

每处记录格式（写进 `mutation-evidence.txt`）：靶名、改动的文件:行、原语句 → 改后语句、`command`、`exit=`、`expected_nonzero=True`、失败测试名、`restore_cmp=identical`、`git_diff_after_restore=clean`。
命令一律 `cd src-tauri && cargo test -j 1 --lib <单个筛选词>`（`cargo test` 只接一个筛选词）。

| # | 靶 | 怎么改 | 必须红的测试 |
|---|----|--------|-------------|
| M① | 项目列表不加区谓词 | `project.rs:268` 的 `AND workspace_id = ?1` 摘掉（实参留着） | `project::tests::list_and_get_are_scoped_to_active_workspace` |
| M② | 父项不判同区 | `project.rs` `validate_parent` 里 `AND workspace_id = ?2` 摘掉 | `project::tests::parent_must_live_in_the_active_workspace` |
| M③ | 子项目可单独换区 | `update_project` 的 `if old.parent_id.is_some() \|\| parent_id.is_some()` 改成 `if false` | `project::tests::child_project_cannot_move_alone` |
| M④ | create 信任入参区 | `let workspace = crate::workspace::active_id(conn)?;` 改成 `input.workspace_id.clone().filter(\|s\| !s.trim().is_empty()).unwrap_or_else(…active_id…)` | `project::tests::create_project_lands_in_the_active_workspace_not_the_input_one` |
| M⑤ | 跟随缺失 | `update_project` 里 `crate::workspace::set_active(&tx, target)?;` 注释掉 | `project::tests::move_subtree_follows_root_and_keeps_return_visible` |
| M⑥ | 子树 UPDATE 不在同一事务 | `update_project` 里子树那句 `tx.execute(…workspace_id = ?1 WHERE id IN…)` 改成 `conn.execute(…)`（自动提交，跨出事务边界） | `project::tests::move_is_atomic_when_the_log_write_fails`（日志失败时子树已经落在目标区 ⇒ 三条区号断言必红）；同靶下 `move_subtree_follows_root_and_keeps_return_visible` 仍绿是正常的，它验的是成功路径，不是回滚 |
| M⑦ | 检索只改项目段 | 九段里只给项目段加 `?3`，其余八段的 `AND p.workspace_id = ?3` 摘掉（`collect` 多绑一个实参不会报错，SQLite 允许多余参数） | `search::tests::field_hits_are_scoped_in_every_section` |
| M⑧ | 首屏谓词与正文不同步 | `index_store.rs:207` `indexed_project_count` 不加区谓词 | `index_store::tests::doc_hits_and_indexed_project_count_use_the_same_scope` |
| M⑨ | 台账写守卫缺失 | `ledger.rs:212` `soft_delete` 首行的 `ensure_active_row` 摘掉 | `ledger::tests::ledger_read_and_write_paths_are_scoped` |
| M⑩ | 非空区守卫缺失 | `workspace::delete` 的第③道（`n > 0` 那段）摘掉 | `workspace::tests::delete_refuses_default_active_and_non_empty_workspaces` |
| M⑪ | 活动区兜底改成报错 | `active_id` 的 `Ok(value.unwrap_or_else(\|\| DEFAULT_ID.to_owned()))` 改成 `value.ok_or_else(\|\| …)?`（返回 `AppResult` 签名不变） | `workspace::tests::active_id_defaults_when_the_setting_row_is_gone` |
| M⑫ | 日志按区过滤缺失 | `project_audit::list` 的 `AND p.workspace_id = ?5` 摘掉 | `project_audit::tests::audit_list_and_options_are_scoped_to_active_workspace` |

> 表里有 12 行（M①–M⑫），spec §十三 承诺「至少十处」，多出的两处（M⑥/M⑫）是执行期发现「同口径谓词」和「日志区过滤」这两条不变量各值一个靶。
> 每条靶还原后必须 `git diff --exit-code src-tauri/src/...` 验 0，并把 `sha256sum` 前后对上一次（本仓前科：`sed -i` 还原漏半个字符）。

- [ ] **Step 3: 真机十二格（CDP，一次性 identifier 沙盒）**

隔离口径（M13 事故后定的，HANDOFF 坑 51）：

1. 先对真实数据目录取三项读数并落盘：`ls -l --time-style=full-iso %APPDATA%\dev.zero.pfm\ledger.db{,-wal,-shm}` + `sha256sum`（记下 `sha256` 前 12 位）。
2. 临时改 `src-tauri/tauri.conf.json` 的 `identifier` → `dev.zero.pfm.m15test`（**只有 compile-time identifier 才改得动 `app.path().app_data_dir()`**；`-c identifier` 与 `APPDATA`/`LOCALAPPDATA` 环境变量都不生效，这是 M13 实测过的）。
3. `git diff --exit-code src-tauri/tauri.conf.json` 在改动期必然非 0，取完证据后按**原字节**还原（`git checkout -- src-tauri/tauri.conf.json`）并再验 0。
4. `npm run tauri dev` 由**控制方**起，走受管后台任务（`cmd &` 会随工具调用结束被回收，导致 CDP 代理全挂）；端口用 9333（9222 被 Qoder 自己占着）。
5. 进程只按自己记下的 PID 精确回收（`taskkill //PID <pid> //T //F`），**绝不按映像名通杀**（不 `taskkill //IM node.exe` / `msedgewebview2.exe`）。
6. 跑完删净沙盒目录，并**再取一次真实目录三项读数**，与第 1 步逐字一致才算过；不一致立刻停手报告，不许接着改代码。

`m15-cdp.mjs` 的取数口径：只回布尔/条数/字段名/长度/错误码名，不回项目正文、不回凭据。`waitFor` 必须 try/catch 包 `document.body` 为 null 的瞬间（M14 前科：reload 期表达式抛错让整格挂掉）。

| 格 | 动作 | 必须落盘的读数形状 |
|----|------|-------------------|
| C-1 | 沙盒首启（先建两个默认区项目） | `activeName` 文案含「默认工作区」、`projectRows=2` |
| C-2 | 弹窗新建「客户乙」 | `rows=2`、`newRowProjectCount=0`、`activeStillDefault=true`（**不自动跳**） |
| C-3 | 切到「客户乙」 | `projectRows=0`、`searchDefaultNameHits=0`、`todosRows=0`、`dailyRows=0`、`auditRows=0` |
| C-4 | 乙区新建项目 | `createdWorkspaceId=乙区 id`；切回默认区 `gone=true`；再切回 `back=true` |
| C-5 | 编辑根项目（带 2 层子）改区 | 保存后 `activeFollows=true`、`treeRowsInTarget=3`、`indents=[16px,36px,56px]` 形状同 M13 c3、切回旧区 `rows=0` |
| C-6 | 编辑子项目 | `workspaceSelectDisabled=true`、提示文案含「子项目随父项目一起换区」、保存后 `workspaceUnchanged=true` |
| C-7 | 父项候选里找别区项目 | `crossWorkspaceParentCandidates=0`（列表已过滤 ⇒ 无法构造）；后端侧证据引 M② |
| C-8 | `/audit` | `rows` 只含当前区；「工作区」差异行显示两区**中文名**（断 `oldName`/`newName` 字符串，不断 id） |
| C-9 | 删非空区 / 删当前区 / 删默认区 | 三条错误码名 `invalid_input` + 三条文案各命中一次；`workspacesUnchanged=true` |
| C-10 | 删空区 | `gone=true`、`rows` 数 -1 |
| C-11 | 切区后 `/index` | `overviewProjects` 只含当前区；`workerTotalsUnchanged=true`（总量口径不动，spec §4.1） |
| C-12 | 备份导出 → 切区 → 导入 | footer `schema=v11`、导入后 `activeId` 与区列表原样、`searchAlive=true`；`m15-backup.zip` 留档 |

Expected: `M15-RESULT: ALL-PASS`，12 格逐条读数写进 `real-machine-readings.md`（含真实目录前后 `sha256` 一致那句）。

- [ ] **Step 4: spec↔计划双向映射表**

`spec-plan-mapping.md`：spec 每一节（§一…§十三）→ 计划哪一 Task 的哪一 Step → 测试名 / 真机格 / 变异靶。**没有落点的 spec 句子必须显式写成「本轮不做」或「有名缺口」，不许留空。**至少要覆盖：§4.1 的 11 行读过滤、§4.2 的 5 行写守卫、§五 的 5 条命令、§六 的 5 条子项（含 §六.4 跟随）、§七 的 4 条日志联动、§八 的 5 个子节、§十 T-1…T-18、§十一 C-1…C-12、§十二 的 9 条不做。

- [ ] **Step 5: 把账本与证据提交（账本进 git，证据目录不进）**

`.superpowers/` 已在 `.gitignore` 里（M9 起如此），所以本 Step 只提交账本目录之外的东西 —— 也就是没有东西要提交；把「证据目录路径 + 关键读数」写进 Task 10 的活文档即可。**不要**为了留证据而 `git add -f .superpowers/`（M13/M14 的取证都在同一条约定下存活，`git clean -fdx` 会清掉 —— 收口时别清）。

---

## Task 10: 活文档回写

**Files:**
- Modify: `docs/技术方案.md`（数据模型段、里程碑表、架构口径两句）
- Modify: `docs/开发进度.md`（M15 验收证据小节 + changelog）
- Modify: `docs/HANDOFF.md`（§3 快照、§4-A 门禁期望串、坑清单若有新增）

**Interfaces:**
- Consumes: Task 9 的全部真实读数（**数字一律取实测，不许写期望值**）。
- Produces: 交付后的三份活文档。历史规格（M9–M14 的 spec）**不 retro-edit**；只在 `docs/思维导图.md` 的对应格子加一行「M15 已交付」指针（若该文件被当作活清单）。

- [ ] **Step 1: `docs/技术方案.md`**

1. 数据模型段（M14 的 `project_audit_logs` 那两行之后）加：

```text
workspaces 工作区（M15 新增，V11）：id（TEXT PK）/ name（UNIQUE）/ created_at；`default`（默认工作区）由 V11 种，不可删。
projects.workspace_id（M15 新增，V11，NOT NULL DEFAULT 'default'）：逻辑分区归属；存量行由 ADD COLUMN 语义自动回填，迁移零 UPDATE。
settings.active_workspace_id（M15 新增）：当前活动区，后端记住；读不到兜回 `default`（兜底不是主路径）。
```

2. 里程碑表加 M15 一行（状态列写「代码 + 变异 + 真机全完，**已 fast-forward 合入 `master`（合并点 `<现取 sha>`）**」—— 合并点等批准后回填，未合并时写「待合并」并给取数命令，不写死「同指」）。

3. 架构口径加两句（这是本轮最容易被误读的两条，必须落在活文档里）：

```text
**工作区是逻辑分区，不是访问控制**：同库同进程，任何拿到 `ledger.db` 的人都能读到全部工作区的数据。隔离的兑现点是「后端按当前区过滤读写」，不是加密、不是权限。（M15 spec §十二.6）
**索引抓取不按区开关**：后台 worker 照旧扫所有未删项目的根目录（`index_job.rs:98 targets()`），按区收窄只发生在呈现与写入这一层。理由是重扫代价与增量机制（mtime）跟 UI 视野无关。（M15 spec §4.1 / §十二.8）
```

- [ ] **Step 2: `docs/开发进度.md`**

加一节 `## M15 验收证据（多工作区分类隔离，<状态 + 合并点>；cargo <实测> / node 75）`，逐格列：门禁串、12 处变异靶的原始读数摘要、真机 12 格 ALL-PASS、spec↔映射表路径、真实目录前后 `sha256` 一致那句（沙盒纪律的证据）、有名缺口清单（命令层注入不了 State ⇒ 走真机；UI 无 jsdom 自动化 ⇒ 走真机；其余按执行期实际入账）。

- [ ] **Step 3: `docs/HANDOFF.md`**

1. §3 快照那行改成「**M15 多工作区已 fast-forward 合入 `master`（合并点 `<现取>`）**：终审 <结论> → <修复情况>；合并后在 `master` 树上复跑整套门全绿。Task 1–10 complete；cargo **<实测>** / node **75** / 命令 **75** / async 7 / `TARGET_VERSION` **11** / 豁免 1 / `Cargo.lock` 616 / collect 9；十二处变异全红并还原；真机 12 格 ALL-PASS。下一格候选：`docs/思维导图.md` §四 的候选已清空，需与需求方重新取题。sha 与读数一律现取（`git rev-parse --short master` / `git ls-remote --heads origin` / §4-A 那串命令），本文不写死「同指」。证据在 `.superpowers/sdd/2026-10-23-m15-workspaces/{progress.md,mutation-evidence.txt,real-machine-readings.md,spec-plan-mapping.md,m15-cdp.mjs,m15-cdp.stdout.txt,m15-backup.zip,final-review.md,final-fix-report.md}`（git-ignored，收口前别清）。」
2. §4-A 的门禁期望串改成：`命令 75 / async 7 / TARGET_VERSION 11 / 豁免 1 / Cargo.lock 616 / collect 9 / cargo <实测> / node 75`。
3. 若执行期出过新事故（尤其是数据目录相关），按坑编号追加一条，写成因与已定的处置口径；没有就不加（不写瞬时状态句）。

- [ ] **Step 4: 校验与提交**

```bash
unix2dos docs/技术方案.md docs/开发进度.md docs/HANDOFF.md
git diff --check            # 必须空输出（无行尾空白）
git add docs/技术方案.md docs/开发进度.md docs/HANDOFF.md
git commit -m "docs(m15): 活文档回写 —— 技术方案/开发进度/HANDOFF（工作区是逻辑分区不是权限、索引抓取不按区开关）"
git push origin feat/m15-workspaces
```

---

## Execution Notes（控制方在执行期遵守）

1. **接缝提醒**：Task 2 Step 2 会把 `Project.workspace_id` / `ProjectInput.workspace_id` 的**字段声明**先落下去（为了让 workspace 的测试能编译），Task 3 才落**语义**。账本里点名这条跨 Task 前置，别把它读成 Task 3 的活没干完。
2. **计划文本自身有错时的处置**：走 `sdd-plan-revision-fix-loop`（裁定 → 改计划本体 → `task-brief` 重生成该任务简报 → 账本与计划一起提交 → 明确 `FIX_BASE` 派发 fix round → `review-package` 限定范围复审）。注释级、零行为回归的文本修正不重开派发轮。
3. **派发前必做的数字闸门**：每个 Task 的「Expected N passed」在派发实现者之前由控制方当场逐 target 数出实数（本计划的累计表是计划期读数，执行期以运行为准）。
4. **合并 `master` 需要需求方当次批准**；推送 `master` 每次单独指示；常驻推送授权只覆盖功能分支。
5. **每条对第三方库内部行为的判断**（例如 `params_from_iter` 的绑定语义、`WITH RECURSIVE` 能否前缀 UPDATE、`ADD COLUMN DEFAULT` 对存量行的填充）一律到本机 vendored 源码或当场探针取证据，写成具名风险再落测试，不凭记忆断言。

