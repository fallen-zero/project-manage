# M9 待办清单 + 项目里程碑 + 到期预警 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让待办成为独立一级栏目（跨项目新增/编辑/删除/勾选完成/按项目筛选/三视图），新建项目里程碑表，并把「到期预警」以红（已逾期）/黄（7 天内）两档做在待办页顶部。

**Architecture:** V5 迁移追加两张同形制的业务表（`todos` / `milestones`，都是 uuid 主键 + `project_id` 外键 + `done` 勾选 + 软删三件套），Rust 侧两个新模块各带四条 CRUD 命令（共八条新 IPC，函数体全同步）。**预警不落在 SQL**：前端纯函数 `src/lib/due-warning.ts` 吃「全部待办 + 全部里程碑 + 项目列表」三份已过滤数据与传入的 `today` 算红黄；排序与三视图也在前端（`src/lib/todo-view.ts`），能被 `node --test` 直跑。

**Tech Stack:** Rust 2021 / rusqlite 0.40.2 bundled / serde camelCase / Tauri 2；前端 React 19 + TS + Vite + Tailwind 4 + TanStack Router（hash history）+ shadcn/ui；测试 `cargo test -j 1 --lib` 与 `node --test "tests/**/*.test.ts"`。**零新 crate、零新 npm 依赖。**

**Spec:** `docs/superpowers/specs/2026-10-14-m9-todos-milestones-design.md`（决策 D1–D12、数据模型 §三、命令契约 §四、失败面 §五、预警判据 §六、真机十格 §十、不在本轮 §十一、风险 §十二、回写 §十三；**分歧以 spec 为准**）。需求原文在 `docs/思维导图.md:33-36`（里程碑）、`:38-45`（待办）、`:73`（到期预警）—— 不在技术方案的里程碑表里（HANDOFF 坑 34）。

## 本计划的完成度（暂停时刻的实态，2026-10-14）

**这份计划当前只落笔到 Task 2，不是可以直接派发的成品。** 续写时按下面的编号走（原「文件结构」段末那条依赖序里写的 T4-T9 是旧编号，已按本节订正）：

- **Task 1 V5 迁移**（已写，含 `backup.rs` 明文告警那句）
- **Task 2 `todo.rs` + 待办四条命令接线**（已写；命令并入本格是**必须的**，见该格 Step 3 的 `dead_code` 账）
- **Task 3 `milestone.rs` + 里程碑四条命令接线**（**未写**）：与 T2 同形，多一条 `STAGES` 五词白名单（照 `ledger.rs:341-346` 的 `ENVS.contains` 那一形），日期判据**复用** `todo::norm_date` 不再写第二份；本格终态命令 **53**。测试约 9 条，含 `milestone_line_format_has_exactly_eight_keys`（八键：id/projectId/stage/planDate/note/done/createdAt/updatedAt）。
- **Task 4 前端契约层**（**未写**）：`src/types/todo.ts`、`src/types/milestone.ts`（含 `MILESTONE_STAGES` 五词常量）、`src/lib/api.ts` 八个封装、`src/lib/due-warning.ts`、`src/lib/todo-view.ts`，加 `tests/due-warning.test.ts` 与 `tests/todo-view.test.ts`（node 基线 10 ⇒ 约 23），并在此格补那条 Rust↔TS 词表一致性文本门（`include_str!("../../src/types/milestone.ts")`，needle 一律 `concat!` 拼，坑 33）。
- **Task 5 `/todos` 页 + nav 第 5 格 + `todosRoute`**（**未写**）
- **Task 6 `milestone-panel.tsx` + 接进 `project-detail.tsx`**（**未写**，照 `ledger-panel.tsx` 的现成形制）
- **Task 7 全量门禁 + 五处变异取证 + 真机十格**（**未写**；控制方执行，子代理禁跑 dev）
- **Task 8 文档回写**（**未写**；清单在 spec §十三）

另有两处**只有实跑才能定**的账，续写时不许凭印象填：每格 Expected 条数（坑 5）、以及 T3/T4 的 `dead_code` 与 clippy 读数。

## Global Constraints

- **基线**（控制方 2026-10-14 在合并 M8.1 后的 `master` 树上现跑；功能分支 `feat/m9-todos-milestones` 与该树代码逐字节相同）：`cargo test -j 1 --lib` = **225 passed / 0 failed / exit=0**；两条 clippy（`--lib -- -D warnings`、`--lib --all-targets -- -D warnings`）**exit=0 且 0 warning**；`cargo build --lib` **exit=0**；`node --test "tests/**/*.test.ts"`（**在仓库根**跑）**tests 10 / pass 10 / fail 0**；`npm run build` **exit=0 且 `error TS`=0**；结构门 `generate_handler!` **45**、`tauri::command(async)` **4**、`TARGET_VERSION` **4**、全仓 `dead_code` 豁免 **1**（`db.rs:30`）、`Cargo.lock` `[[package]]` **616**。
- **每个 Task 的 Expected 条数必须实跑数出来**（看 `cargo test -j 1 --lib` 的 `test result` 行 + `grep -c '^    fn '` 数本格具名测试），**不许累加**（HANDOFF 坑 5）。下文写的数是本计划按逐条列名数出来的；**派发每格前用 task-brief 重数一次，实跑与列名不符就按实跑订正**。
- **命令显式锚目录**（cargo/clippy 前要 `cd src-tauri`，`node --test` 与 `npm run build` 要在**仓库根**）；而 **cwd 在多次调用之间会残留**（M8.1 合并那轮就因 `cd: src-tauri: No such file or directory` 让一条 clippy 根本没跑、`exit=1` 是 `cd` 的退出码 —— 坑 4）。退出码一律 `cmd > /tmp/x.log 2>&1; echo "exit=$?"; grep … /tmp/x.log`，**绝不** `cmd | tail; echo $?`。
- **16 G 机的提交内存**：`cargo test` 报「could not compile」却一行 `error[Exxxx]` 都没有 ⇒ 是 `os error 1455` 抖动，**先重跑同一命令再判**，别改代码（坑 32）。所有 cargo 命令带 `-j 1`。
- **零新依赖**（硬门）：`src-tauri/Cargo.toml` 与 `Cargo.lock` **一行不动**；`grep -c '^\[\[package\]\]' src-tauri/Cargo.lock` 在 T9 终态仍须 **616**。前端 `package.json` 不动（`node --test` 直跑 TS 靠 Node 24 的原生类型擦除，仓库里已有两条这样的测试）。
- **日期不用 `regex` crate**（它会破上一条硬门）：Rust 侧按字节判形状（T2），前端用字面量正则 `/^\d{4}-\d{2}-\d{2}$/`。两边各一份实现，**由 T4 那条文本门钉住逐字一致**。
- **零新错误码**（D7）：只用现成的 `invalid_input`（形制 `ledger.rs:15`）与 `not_found`（`ledger.rs:19`）。不许出现 `todo_*` / `milestone_*` 这两个新前缀。
- **结构门终态**（T9 逐条现取）：`generate_handler!` **53**（D11 第三次批准破例；计数正则**逗号可选** `^ +[a-z_]+,?$`）、`tauri::command(async)` 仍 **4**（八条新命令一律同步）、`TARGET_VERSION` **5**、`dead_code` 豁免仍 **1**（`db.rs:30`）、`Cargo.lock` `[[package]]` **616** 不变。
- **红线（每条派发词带全）**：① 绝不动被索引的源文件；② 写盘落点白名单只有「用户当场选中的包路径 + 数据目录里的 `pre-restore-*.zip`」两处且只新增（M8.1 之后还多一道 `guard_destination` 硬拒）——**M9 一次盘也不该写**，落盘只在真机沙盒的库里；③ 真实数据目录 `%APPDATA%\dev.zero.pfm\` 只读，真机一律一次性 identifier `dev.zero.pfm.m9test` 跑完删净，绝不为验证改 `tauri.conf.json`；④ **子代理一律不跑 `tauri dev`/`npm run dev`**（T8 由控制方执行）；⑤ 同一个 worktree 只允许**一个会提交的实现者**，只读评审可并行；⑥ 输出中文（错误码、SQL、JSON key 除外）；⑦ 未跟踪的 `Python/` 绝不 stage、绝不删。
- **测试断言必须可证伪**（坑 10）：本计划每条新判据都在 T8 的**五处变异取证**里点名怎么把它打红。写测试前先做反值心算，别写恒绿断言。

## 本轮明确不做（实现者不许顺手加，评审按这一格打红）

- **维保/域名/证书三类到期字段**一个也不建（需求原文 `:73` 括号里那三类）。本轮预警盯的是里程碑计划日期、项目交付截止、未完成待办截止 —— 这是**缩量兑现**，T9 回写时要在技术方案 M11 行点名。
- 待办/里程碑**不进** `backup::Counts`（D9），不改包格式、不改 `formatVersion`、不动 `strip_index`。
- 待办正文**不进任何检索通路**：不进 `search_local`，不进 `index_docs`/FTS。M9 之后「待办内容能不能被搜到」的答案仍是**不能**。
- 不改 `project-form.tsx:27` 那格「交付截止」的控件（本轮只在预警侧「解析不了就跳过并计数」）。
- update 的 input **不含 `project_id`**：不能改归属（照 `EnvInput` 的先例）。
- 不加 `milestone_stage_options` 命令（加了就是 54 条，而批准的是 53）；词表用前端常量 + T4 的文本门钉一致性。
- 不做：待办重复/提醒推送/通知、父子项目关联、多工作区、操作日志、导出、多人协作、`schema_meta` 回滚。
- 不给 `milestones` 与 `projects.status` 统一词表（D4：两套词各有各的语义）。

## 已核签名（实读于本仓，写代码前照这段，别凭印象）

| 事项 | 真值与位置 |
|---|---|
| 迁移驱动 | `pub(crate) const TARGET_VERSION: i64 = 4;`（`db.rs:98`）；`for target in (current + 1)..=TARGET_VERSION { let ddl = match target { 1 => V1, … 4 => V4, _ => "" }; apply_version(conn, target, ddl)?; }`（`db.rs:117-126`）；`fn apply_version` = 一个 `unchecked_transaction` 里 `execute_batch(ddl)` + `INSERT INTO schema_meta` + `commit`（`db.rs:139-149`） |
| V5 该照哪一版 | V3（`db.rs:198` 起）与 V4（`db.rs:295` 起）：`const Vn: &str = "` 开头、`CREATE TABLE` **不带** `IF NOT EXISTS`、结尾一串 `CREATE INDEX`（`db.rs:283-287` 是 V3 的五个索引） |
| 子表形制（逐字照抄这个形状） | `ledger_notes`：`id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, … created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now')), deleted_at TEXT`（`db.rs:272-281`） |
| FK 是否真生效 | `pragma(conn, "foreign_keys", "ON")`（`db.rs:56`）；`db.rs:336-337` 的测试注释写明「PRAGMA foreign_keys=ON，所以必须先落一行父项目」 |
| 项目删除是软删 | `delete_project` 是 `UPDATE projects SET deleted_at = datetime('now'), updated_at = … WHERE id = ?1 AND deleted_at IS NULL`（`project.rs:163-173`）⇒ 表上的 `ON DELETE CASCADE` **对现有五张台账表从未触发**（D8） |
| 跨项目读要挡软删项目 | `… JOIN projects p ON p.id = d.project_id AND p.deleted_at IS NULL`（`doc_preview.rs:77`）、`WHERE p.deleted_at IS NULL`（`index_job.rs:129`）、`AND p.deleted_at IS NULL`（`index_store.rs:212`） |
| helper 三件套 | `fn invalid(msg) -> AppError`（码 `invalid_input`、hint「检查表单必填项」，`ledger.rs:15-17`）、`fn not_found(id)`（`ledger.rs:19-21`）、`fn norm(value: Option<String>) -> Option<String>`（trim 后空串折成 `None`，`ledger.rs:23-25`）、`fn project_exists(conn, project_id)`（`SELECT COUNT(*) FROM projects WHERE id = ?1 AND deleted_at IS NULL`，命中 0 ⇒ `not_found`，`ledger.rs:218-228`）、`fn soft_delete(conn, table, id)`（`ledger.rs:207-216`） |
| 白名单枚举的写法 | `if !ENVS.contains(&input.env.as_str()) { return Err(invalid(&format!("环境「{}」不在 dev/test/pre/prod 之内", input.env))); }`（`ledger.rs:341-346`）⇒ `STAGES` 就照这一形 |
| 出参线格式 | `#[derive(Debug, Serialize)] #[serde(rename_all = "camelCase")]`（`ledger.rs:92` 的 `Env`）；入参多一行 `Deserialize` 且同样 `rename_all = "camelCase"`（`project.rs:45-59` 的 `ProjectInput`） |
| bool 上线的先例 | `pub username_set: bool`（`ledger.rs:47`）→ JSON `usernameSet`；`IndexOverview.ocr_available: bool` → `ocrAvailable` |
| create 之后怎么回行 | `create_env` 用 `list(conn, project_id)?.envs.into_iter().find(|e| e.id == id).expect("刚插入的行一定读得到")`（`ledger.rs:364-368`）—— 本轮改用按 id 直读（见 T2），**别学 `expect` 那条** |
| 主键 | `uuid::Uuid::new_v4().to_string()`（`ledger.rs:350`、`project.rs:110`）；`uuid` 已在依赖树，零新增 |
| 命令层 | `struct AppState { conn: Mutex<rusqlite::Connection>, data_dir: PathBuf, mk: Mutex<Option<MasterKey>>, index: Arc<index_job::IndexShared>, preview: … }`（`lib.rs:32-40`）；`fn db(state: &AppState) -> AppResult<MutexGuard<'_, rusqlite::Connection>>`（`lib.rs:43-51`，锁中毒报 `db_poisoned`）；命令形如 `#[tauri::command] fn ledger_env_create(state: State<'_, AppState>, project_id: String, input: EnvInput) -> AppResult<ledger::Env> { let conn = db(&state)?; ledger::create_env(&conn, &project_id, &input) }`（`lib.rs:289-296`） |
| 模块声明 | `lib.rs:1-17` 是 `mod backup; mod db; … mod watch;`，**字母序** ⇒ `mod milestone;` 插在 `mod ledger;` 之后、`mod ocr;` 之前；`mod todo;` 插在 `mod tokenize;` 之后、`mod vault;` 之前（坑 30：M8 T1 就是被这条字母序例面文字坑过一次） |
| `include_str!` 文本门形制 | `backup_export_wires_the_guard_before_any_write`（`lib.rs:903-922`）：`let src = include_str!("lib.rs");` + **needle 全用 `concat!` 拼**（否则会把测试自己的源码数进去，坑 33），断「恰好一次」+ 字节先后 |
| 线格式钉键数 | `manifest_serializes_to_the_documented_camel_case_keys`（`backup.rs:629-650`）用 `serde_json::to_value` 后取 `.as_object().unwrap().keys()` 比 `Vec<&str>` |
| 内存库夹具 | `crate::db::open_in_memory().unwrap()`（`backup.rs:667` 在用）；父项目要自己 seed：`conn.execute("INSERT INTO projects (id, name, status) VALUES ('p1','甲项目','立项')", [])`（`backup.rs:670-673`） |
| 明文列告警那句 | `pub fn warn_plaintext_columns() -> &'static str`（`backup.rs:22-24`）：「包内非 `*_cipher` 列为明文：…」—— 一句文本两处出（manifest 与界面），**加表就要改这句** |
| 前端调用层 | `src/lib/api.ts`：`const { invoke } = …` 之后一行一个 `export const listProjects = () => invoke<Project[]>("project_list");`、`export const updateEnv = (id: string, input: EnvInput) => invoke<void>("ledger_env_update", { id, input });`（`api.ts:21-123`）；JS 侧参数与 input 字段一律 camelCase |
| 路由 | `src/router.tsx`：`const backupRoute = createRoute({ getParentRoute: () => rootRoute, path: "/backup", component: BackupPage });` + `rootRoute.addChildren([searchRoute, projectsRoute, projectDetailRoute, indexRoute, backupRoute])`（`router.tsx:36-42`） |
| nav | `src/components/app-shell.tsx:8-13`：`const nav = [{ to: "/", label: "搜索" }, { to: "/projects", label: "项目" }, { to: "/index", label: "索引" }, { to: "/backup", label: "备份" }];` |
| 页面形制 | `project-detail.tsx`：`<section className="rounded-xl border bg-background p-4">` + `<h2 className="text-sm font-medium">`；错误条 `{error && <div className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">…}`；可用 ui 组件只有 `badge button card dialog input label select separator`（`src/components/ui/`），**没有 checkbox / table** |
| 子面板先例 | `src/components/ledger-panel.tsx` 是被 `project-detail.tsx:25` import 的独立面板 ⇒ 里程碑区照它做成 `milestone-panel.tsx`（spec §七 写「改 project-detail.tsx」，本计划按现成形制落成一个新组件，见 T7） |
| node 测试 | `import test from "node:test"; import assert from "node:assert/strict"; import { backupFileName } from "../src/lib/backup-name.ts";`（`tests/backup-name.test.ts:1-3`）—— **相对路径 + 带 `.ts` 后缀**，不是 `@/` 别名；被测模块内部只准 `import type`（`search-order.ts:1-3` 的注释写明：`import type` 会被擦除，所以能被 node 直跑） |

---

## 文件结构

| 文件 | 动作 | 职责 |
|---|---|---|
| `src-tauri/src/db.rs` | 改 | `TARGET_VERSION` 4→5、match 加 `5 => V5`、`const V5`（两张表 + 两个索引）；迁移层测试 |
| `src-tauri/src/backup.rs` | 改 | **只改一句文本** `warn_plaintext_columns()` 把两张新表的明文列念进去 + 一条断言 |
| `src-tauri/src/todo.rs` | **新建** | 待办线类型 + 四操作 + 日期形状判据 `is_iso_date`/`norm_date`（`pub(crate)`，供 milestone.rs 共用） |
| `src-tauri/src/milestone.rs` | **新建** | 里程碑线类型 + 四操作 + `STAGES` 五词白名单；日期判据消费 `todo::norm_date` |
| `src-tauri/src/lib.rs` | 改 | `mod milestone; mod todo;`；八条 `#[tauri::command] fn`（同步）；`generate_handler!` 45→53；两条文本门测试 |
| `src/types/todo.ts` `src/types/milestone.ts` | **新建** | 与 Rust 出参逐字段对齐的 camelCase 线类型 + 入参类型 + `MILESTONE_STAGES` 常量 |
| `src/lib/due-warning.ts` | **新建** | 预警纯函数：判据①-⑤、红黄两档、`unparsed` 计数、`localToday()`、`DUE_HORIZON_DAYS` |
| `src/lib/todo-view.ts` | **新建** | 三视图的排序/分组纯函数（空日期落最后、分组按项目名、组内保序） |
| `src/lib/api.ts` | 改 | 八个封装 |
| `src/pages/todos.tsx` | **新建** | 待办页：预警块 + 三视图 + 项目筛选 + 新增/编辑/勾选/删除 |
| `src/components/milestone-panel.tsx` | **新建** | 项目详情的里程碑区（增删改、勾选、命中预警的行标色） |
| `src/router.tsx` `src/components/app-shell.tsx` | 改 | `todosRoute` + nav 第 5 格 |
| `tests/due-warning.test.ts` `tests/todo-view.test.ts` | **新建** | `node --test` 覆盖预警判据与视图排序 |

依赖序：**T1（V5）→ T2（`todo.rs` + 待办四条命令，45→49）→ T3（`milestone.rs` + 里程碑四条命令，49→53）→ T4（前端契约层：types + api + `due-warning.ts` + `todo-view.ts` + 两条 node 测试 + 词表一致性文本门）→ T5（`/todos` 页 + nav + 路由）→ T6（`milestone-panel.tsx`）→ T7（全量门禁 + 五处变异取证 + 真机十格）→ T8（文档回写）**。T2 与 T3 可并行评审但**必须串行提交**（同 worktree 一个提交者）。

---

## Task 1: V5 迁移（两张表 + 两个索引）+ 明文告警那句改准

**Files:**
- Modify: `src-tauri/src/db.rs`（`TARGET_VERSION` 常量 `:98`、match 分支 `:119-123`、`V5` 常量追加在 `V4` 之后约 `:328`、测试模块 `:329` 起）
- Modify: `src-tauri/src/backup.rs:22-24`（`warn_plaintext_columns()` 一句文本）+ 其测试模块加一条断言
- Test: 两处测试都写在各自文件的 `mod tests` 里（本仓没有 `src-tauri/tests/` 集成测试目录，别新建）

**Interfaces:**
- Consumes: 无（这是第一格）
- Produces: 库里存在 `todos(id, project_id, title, due_date, done, created_at, updated_at, deleted_at)` 与 `milestones(id, project_id, stage, plan_date, note, done, created_at, updated_at, deleted_at)`；`db::TARGET_VERSION == 5`；`crate::db::open_in_memory()` 返回的库上 FK 与 CHECK 都生效（T2/T3 的测试全靠这一点）

- [ ] **Step 1: 写失败的迁移测试（六条，放 `db.rs` 的 `mod tests` 末尾）**

注意三条现成事实，测试要照着写才跑得通：`open_in_memory()` 走 `after_open(conn, false)`，里面**已经** `PRAGMA foreign_keys=ON` 并且已经 `migrate`（`db.rs:44-59`）；`todos.project_id` 有外键，所以插子表前必须先有父项目（复用本模块已有的私有 `fn seed_project(conn, id)`，`db.rs:339-345`）；`db.rs` 的测试体靠 `use super::*` 拿符号，`open_in_memory()` / `TARGET_VERSION` / `seed_project` 都不带模块前缀（照 `db.rs:349-351` 那一格现成的写法）。

```rust
    /// V5 必须真把两张表和两个索引建出来（`CREATE TABLE` 没写 `IF NOT EXISTS`，
    /// 所以「对象建了一半、schema_meta 停在旧版本」这个失败形状在事务保护下不会发生，
    /// 但建了表漏了索引是看得见的，这里一起钉）。
    #[test]
    fn v5_creates_todo_and_milestone_tables_and_indexes() {
        let conn = open_in_memory().unwrap();
        let names: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master
                  WHERE name IN ('todos','milestones','ix_todo_project','ix_milestone_project')
                  ORDER BY name",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            names,
            vec![
                "ix_milestone_project",
                "ix_todo_project",
                "milestones",
                "todos"
            ],
            "V5 的四个对象必须全在：{names:?}"
        );
    }

    #[test]
    fn v5_schema_meta_reaches_target_version_five() {
        let conn = open_in_memory().unwrap();
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 5, "迁移必须跑到 5");
        assert_eq!(TARGET_VERSION, 5, "= 上一条，两处别各写各的");
    }

    /// D4：`stage` 是 CHECK 枚举。这五个词是需求原文那五个（思维导图.md:35-36），
    /// 词表外的值必须被挡；报错文本要确认是 CHECK 而不是别的约束。
    #[test]
    fn v5_milestone_stage_check_refuses_words_outside_the_five() {
        let conn = open_in_memory().unwrap();
        seed_project(&conn, "p-stage");
        for stage in ["立项", "评审", "联调", "上线", "验收"] {
            conn.execute(
                "INSERT INTO milestones (id, project_id, stage) VALUES (?1, 'p-stage', ?2)",
                params![format!("m-{stage}"), stage],
            )
            .unwrap_or_else(|e| panic!("五段词表里的「{stage}」必须插得进去：{e}"));
        }
        let err = conn
            .execute(
                "INSERT INTO milestones (id, project_id, stage) VALUES ('m-bad', 'p-stage', '初验')",
                [],
            )
            .expect_err("词表外的阶段名必须被 CHECK 拒掉");
        assert!(
            err.to_string().contains("CHECK constraint failed"),
            "要的是 CHECK 那条错，实际：{err}"
        );
    }

    /// D5：`done` 只吃 0/1。写 2 必须报错 —— 否则「布尔」只是约定。
    #[test]
    fn v5_done_column_check_refuses_values_other_than_zero_and_one() {
        let conn = open_in_memory().unwrap();
        seed_project(&conn, "p-done");
        for table in ["todos", "milestones"] {
            let err = conn
                .execute(
                    &format!("INSERT INTO {table} (id, project_id, title, stage, done) \
                               VALUES ('x', 'p-done', 't', '立项', 2)"),
                    [],
                )
                .expect_err("两张表的 done 都不许收 2");
            assert!(
                err.to_string().contains("CHECK constraint failed"),
                "{table} 的 done 要的是 CHECK，实际：{err}"
            );
        }
    }

    /// D8 的前提事实：`PRAGMA foreign_keys=ON` 真的在挡外键。
    /// 这条不是装饰 —— 它是「软删项目不会被 CASCADE 删掉子行」的另一半证明
    /// （另一半在 T2/T3 的 cross_project 测试里：行还在、只是读不到）。
    #[test]
    fn v5_both_new_tables_enforce_the_project_foreign_key() {
        let conn = open_in_memory().unwrap();
        let err = conn
            .execute(
                "INSERT INTO todos (id, project_id, title) VALUES ('t-x', 'no-such-project', '甲')",
                [],
            )
            .expect_err("不存在的项目必须挡住");
        assert!(
            err.to_string().contains("FOREIGN KEY constraint failed"),
            "要的是 FK 那条错，实际：{err}"
        );
        let err2 = conn
            .execute(
                "INSERT INTO milestones (id, project_id, stage) VALUES ('m-x', 'no-such-project', '立项')",
                [],
            )
            .expect_err("milestones 同理");
        assert!(
            err2.to_string().contains("FOREIGN KEY constraint failed"),
            "要的是 FK 那条错，实际：{err2}"
        );
    }

    /// 默认值是线格式的一部分：日期列可空、done 默认 0、note 默认空串。
    #[test]
    fn v5_dates_default_to_null_and_done_defaults_to_zero() {
        let conn = open_in_memory().unwrap();
        seed_project(&conn, "p-def");
        conn.execute(
            "INSERT INTO todos (id, project_id, title) VALUES ('t-1', 'p-def', '只填标题')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO milestones (id, project_id, stage) VALUES ('m-1', 'p-def', '立项')",
            [],
        )
        .unwrap();
        let (due, done): (Option<String>, i64) = conn
            .query_row(
                "SELECT due_date, done FROM todos WHERE id = 't-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(due, None, "due_date 不填就是 NULL，不是空串");
        assert_eq!(done, 0, "新行默认未完成");
        let (plan, note, mdone): (Option<String>, String, i64) = conn
            .query_row(
                "SELECT plan_date, note, done FROM milestones WHERE id = 'm-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(plan, None, "plan_date 不填就是 NULL");
        assert_eq!(note, "", "里程碑 note 默认空串（需求原文把它当可选项，不是必填）");
        assert_eq!(mdone, 0);
    }
```

上面用到的 `seed_project(&conn, id)` 是 `db.rs:339-345` 现成的私有测试 helper（就插一行 `INSERT INTO projects (id, name)`），同一个 `mod tests` 里直接叫它；`params!` 已在 `db.rs:336` import。**不要**为了测试把私有 helper 提可见性，也**不要**再造第二个 seed（M8 那轮 `backup.rs:667-673` 因为跨模块拿不到 `index_job::tests::seed_project` 才手写了 INSERT，同模块没有这个障碍）。测试体里 `open_in_memory()` / `TARGET_VERSION` 都不带 `db::` 前缀 —— 本模块的 `use super::*` 已经把它们带进来了（照 `v4_creates_index_tables_and_fts_is_writable`，`db.rs:349-351`）。

- [ ] **Step 2: 跑一遍，确认它红在「表不存在」**

Run: `cd src-tauri && cargo test -j 1 --lib v5_ > /tmp/v5.log 2>&1; echo "exit=$?"; grep -E "no such table|test result" /tmp/v5.log`
Expected：`exit=101`，报错里有 `no such table: todos` / `no such table: milestones`（若报的是 `could not compile` 且**没有任何 `error[Exxxx]`**，那是 1455 抖动，重跑同一命令再判 —— 坑 32）。

- [ ] **Step 3: 写 V5 的 DDL**

`db.rs` 里，紧跟在 `const V4: &str = "…";` 之后加（**逐字照 V3/V4 的形制**：不带 `IF NOT EXISTS`，索引跟在同一支 DDL 字符串里，靠 `apply_version` 的事务原子性）：

```rust
/// v5：待办清单 + 项目里程碑（M9，spec §三）。
/// 三处是「照需求写」而不是「照方便写」，别顺手改：
/// 1. `stage` 的五个词是 `思维导图.md:35-36` 原文（立项/评审/联调/上线/验收）。它与
///    `projects.status` 那八段**是两套词**、共有「立项」「上线」两个同形词而语义不同（spec D4）：
///    status 是「项目当前在哪」，stage 是「过程节点」。别统一，别让一个下拉喂两个字段。
/// 2. `done` 用 INTEGER + CHECK 而不是裸布尔约定：SQLite 没有布尔列，而 CHECK 挡住「直写库的人」，
///    serde 侧仍然出 `bool`（`rusqlite` 的 `bool` ToSql/FromSql 走 0/1）。
/// 3. 日期列**只由写入侧保证形状**（`todo::is_iso_date`），SQL 里不做任何日期解释：
///    `strftime('now')` 是 UTC，用它比日期会把本机「今天到期」算错一天（spec D1）。
const V5: &str = "
CREATE TABLE todos (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    due_date   TEXT,
    done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0,1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at TEXT
);

CREATE TABLE milestones (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    stage      TEXT NOT NULL CHECK (stage IN ('立项','评审','联调','上线','验收')),
    plan_date  TEXT,
    note       TEXT NOT NULL DEFAULT '',
    done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0,1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at TEXT
);

CREATE INDEX ix_todo_project ON todos(project_id);
CREATE INDEX ix_milestone_project ON milestones(project_id);
";
```

同文件改两处：

```rust
pub(crate) const TARGET_VERSION: i64 = 5;
```

```rust
            1 => V1,
            2 => V2,
            3 => V3,
            4 => V4,
            5 => V5,
            _ => "",
```

- [ ] **Step 4: 跑 V5 测试确认绿**

Run: `cd src-tauri && cargo test -j 1 --lib v5_ > /tmp/v5.log 2>&1; echo "exit=$?"; grep "test result" /tmp/v5.log`
Expected：`exit=0`、`test result: ok. 6 passed; 0 failed`。**Expected 以实跑为准**（本格具名测试 6 条）。

- [ ] **Step 5: 把明文告警那句改准（`backup.rs`）**

`warn_plaintext_columns()` 现在念的是台账那一层的列（`backup.rs:23`）。M9 之后包里有 `todos.title` 与 `milestones.note`，而 `strip_index` 只剥两张索引表、**新表原样进包** ⇒ 那句话不改就成了「包里的明文列清单少报两档」。整句替换（保持一句文本两处出的形制，别在前端另抄一遍）：

```rust
pub fn warn_plaintext_columns() -> &'static str {
    "包内非 *_cipher 列为明文：项目名/编号/负责人/客户/联系人/合同号、服务器 ip 与堡垒机、各类 url、各表 note、待办内容、里程碑备注。"
}
```

紧挨着现有那条 `manifest_serializes_to_the_documented_camel_case_keys`（`backup.rs:629`）之后加一条断言句子的测试：

```rust
    /// 包里躺着什么明文，这句必须念全 —— M9 加了两张明文表，漏念就是「用户以为没泄露」。
    #[test]
    fn plaintext_warning_names_the_two_m9_tables() {
        let warn = warn_plaintext_columns();
        assert!(warn.contains("待办内容"), "漏了待办的明文列：{warn}");
        assert!(warn.contains("里程碑备注"), "漏了里程碑的明文列：{warn}");
    }
```

- [ ] **Step 6: 跑全量 + 两条 clippy，确认零回归**

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/all.log 2>&1; echo "exit=$?"; grep "test result" /tmp/all.log; cargo clippy --lib -- -D warnings > /tmp/c1.log 2>&1; echo "clippy1=$?"; cargo clippy --lib --all-targets -- -D warnings > /tmp/c2.log 2>&1; echo "clippy2=$?"`
Expected：`exit=0`、**232 passed**（基线 225 + 本格的 6 + 1；**这个数要实跑核对，不许直接照抄**）、两条 clippy `exit=0`。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/db.rs src-tauri/src/backup.rs
git commit -m "feat(m9): V5 建 todos 与 milestones 两张表（stage CHECK 五段、两张表都带 done、日期列只存形状不解释）+ 明文告警那句补进两档新明文列"
```

---

## Task 2: `todo.rs` —— 待办的判据与四操作

**Files:**
- Create: `src-tauri/src/todo.rs`
- Test: `src-tauri/src/todo.rs` 的 `mod tests`（本仓单测与被测同文件，别新建 `tests/` 目录）

**Interfaces:**
- Consumes: T1 的 `todos` 表（列名见 T1 Step 3）
- Produces: `todo::Todo`（出参）、`todo::TodoInput { project_id, title, due_date }`、`todo::TodoEdit { title, due_date, done }`、`todo::list(&conn, Option<&str>) -> AppResult<Vec<Todo>>`、`todo::get(&conn, &str) -> AppResult<Todo>`、`todo::create(&conn, &TodoInput) -> AppResult<Todo>`、`todo::update(&conn, &str, &TodoEdit) -> AppResult<()>`、`todo::delete(&conn, &str) -> AppResult<()>`；**`pub(crate) fn is_iso_date(&str) -> bool` 与 `pub(crate) fn norm_date(&str, Option<String>) -> AppResult<Option<String>>` 是给 T3 共用的**（D7：一条判据只写一处，复制两份一定漂）

- [ ] **Step 1: 写十二行失败测试（模块还不存在，编译就过不了 —— 这就是本格的 RED）**

```rust
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查表单必填项"))
}

fn not_found(id: &str) -> AppError {
    AppError::new("not_found", &format!("记录不存在：{id}"), Some("刷新后重试"))
}

fn norm(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// 日期形状判据（spec §五 第 4 道）。**为什么写成吃参数的纯函数**：M8 终审 C1 的教训③
/// —— 判据只有收在纯函数里，命令层才有可测的落点。
/// **为什么不用 regex**：`regex` 不在依赖树里，引它就是破「M9 零新依赖」这条硬门，按字节判足够。
/// 只判形状、不判历法（`2026-13-45` 过得去，见 spec §十二 风险 5）。
pub(crate) fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[0].is_ascii_digit()
        && b[1].is_ascii_digit()
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
        && b[4] == b'-'
        && b[5].is_ascii_digit()
        && b[6].is_ascii_digit()
        && b[7] == b'-'
        && b[8].is_ascii_digit()
        && b[9].is_ascii_digit()
}

/// `column` 进错误文本：同一条判据两张表共用，用户要能看出是哪一格填错了。
pub(crate) fn norm_date(column: &str, value: Option<String>) -> AppResult<Option<String>> {
    match norm(value) {
        None => Ok(None),
        Some(s) if is_iso_date(&s) => Ok(Some(s)),
        Some(s) => Err(invalid(&format!(
            "「{column}」要写成 YYYY-MM-DD（如 2026-06-30），收到「{s}」"
        ))),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub due_date: Option<String>,
    pub done: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoInput {
    pub project_id: String,
    pub title: String,
    pub due_date: Option<String>,
}

/// update 的入参**没有** `project_id`：改归属不在本轮（spec §十一）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoEdit {
    pub title: String,
    pub due_date: Option<String>,
    pub done: bool,
}

const TODO_COLUMNS: &str = "t.id, t.project_id, t.title, t.due_date, t.done, t.created_at, t.updated_at";

fn map_todo(row: &rusqlite::Row<'_>) -> rusqlite::Result<Todo> {
    Ok(Todo {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        due_date: row.get(3)?,
        done: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn project_exists(conn: &Connection, project_id: &str) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE id = ?1 AND deleted_at IS NULL",
        [project_id],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(not_found(project_id));
    }
    Ok(())
}

/// `?1 IS NULL OR t.project_id = ?1` 这一形是故意的：**只有一条 SQL**。
/// 拆成两条字符串再 `format!` 拼接，就会有人改了一条忘另一条（spec §十二 风险 3 的成因形状）。
/// `JOIN projects … AND p.deleted_at IS NULL` 是 D8 的兑现点：软删项目的待办读不到，但行留着。
pub fn list(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<Todo>> {
    let mut stmt = conn.prepare(
        &format!(
            "SELECT {TODO_COLUMNS}
               FROM todos t
               JOIN projects p ON p.id = t.project_id AND p.deleted_at IS NULL
              WHERE t.deleted_at IS NULL AND (?1 IS NULL OR t.project_id = ?1)
              ORDER BY t.created_at"
        ),
    )?;
    let rows = stmt.query_map(params![project_id], map_todo)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Todo> {
    let sql = format!(
        "SELECT {TODO_COLUMNS} FROM todos t WHERE t.id = ?1 AND t.deleted_at IS NULL"
    );
    conn.query_row(&sql, [id], map_todo)
        .optional()?
        .ok_or_else(|| not_found(id))
}

pub fn create(conn: &Connection, input: &TodoInput) -> AppResult<Todo> {
    project_exists(conn, &input.project_id)?;
    let title = input.title.trim().to_owned();
    if title.is_empty() {
        return Err(invalid("待办内容不能为空"));
    }
    let due_date = norm_date("截止日期", input.due_date.clone())?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO todos (id, project_id, title, due_date) VALUES (?1,?2,?3,?4)",
        params![id, input.project_id, title, due_date],
    )?;
    // 按 id 直读回来，不学 `ledger.rs:364` 那条 `list().find().expect(...)`：
    // 那条要重扫一遍该项目全部行，而这里手里就有主键。
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, edit: &TodoEdit) -> AppResult<()> {
    let title = edit.title.trim().to_owned();
    if title.is_empty() {
        return Err(invalid("待办内容不能为空"));
    }
    let due_date = norm_date("截止日期", edit.due_date.clone())?;
    let changed = conn.execute(
        "UPDATE todos SET title=?2, due_date=?3, done=?4, updated_at=datetime('now')
          WHERE id=?1 AND deleted_at IS NULL",
        params![id, title, due_date, edit.done],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE todos SET deleted_at = datetime('now'), updated_at = datetime('now')
          WHERE id = ?1 AND deleted_at IS NULL",
        [id],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    /// 跨模块拿不到 `db.rs` 那个私有 seed（M8 的 `backup.rs:667-673` 是同一条先例：
    /// 不为测试提可见性、不造新缝，就地手写 INSERT）。
    fn seed(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, status) VALUES (?1, '待办测试项目', '立项')",
            [id],
        )
        .unwrap();
    }

    fn soft_delete_project(conn: &Connection, id: &str) {
        conn.execute(
            "UPDATE projects SET deleted_at = datetime('now') WHERE id = ?1",
            [id],
        )
        .unwrap();
    }

    fn input(project_id: &str, title: &str, due: Option<&str>) -> TodoInput {
        TodoInput {
            project_id: project_id.to_owned(),
            title: title.to_owned(),
            due_date: due.map(|s| s.to_owned()),
        }
    }

    #[test]
    fn is_iso_date_accepts_the_shape_and_rejects_everything_else() {
        assert!(is_iso_date("2026-06-30"));
        // 只判形状、不判历法：月份 13 这种形状对、历法错的值必须收（spec §十二 风险 5）。
        // 把它写进拒的那一列就是判据与实现不符；写成收才挡得住有人以后顺手加历法校验。
        assert!(is_iso_date("2026-13-45"), "历法有效性不在本轮，这一格不许顺手加");
        // 下面每一档各自单独都能把这条打红：点分隔、空格分隔、日月年倒序、月日不补零、
        // 带尾空格、无分隔、长度超、含非数字（恒绿断言就是这么来的）。
        for bad in [
            "",
            "2026.06.30",
            "2026 06 30",
            "30.06.2026",
            "2026-6-30",
            "2026-06-30 ",
            "20260630",
            "2026-06-300",
            "20a6-06-30",
        ] {
            assert!(!is_iso_date(bad), "「{bad}」不该被当成 YYYY-MM-DD");
        }
    }

    #[test]
    fn norm_date_maps_blank_and_absent_to_null() {
        for v in [None, Some("".to_owned()), Some("   ".to_owned())] {
            assert_eq!(norm_date("截止日期", v.clone()).unwrap(), None, "{v:?} 要折成 NULL");
        }
    }

    #[test]
    fn norm_date_refuses_a_non_iso_value_naming_the_column() {
        let err = norm_date("计划日期", Some("2026.06.30".to_owned())).unwrap_err();
        assert_eq!(err.code, "invalid_input", "D7：不许出现新错误码");
        assert!(err.message.contains("计划日期"), "错误文本要点名是哪一列：{}", err.message);
        assert_eq!(norm_date("截止日期", Some("2026-06-30".to_owned())).unwrap().as_deref(), Some("2026-06-30"));
    }

    #[test]
    fn create_rejects_empty_title() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let err = create(&conn, &input("p1", "   ", None)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM todos", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "被拒的输入不许留下行");
    }

    #[test]
    fn create_requires_an_existing_undeleted_project() {
        let conn = db::open_in_memory().unwrap();
        let err = create(&conn, &input("nope", "甲", None)).unwrap_err();
        assert_eq!(err.code, "not_found", "项目不存在按 not_found(项目 id) 报，与 ledger 同形");
        seed(&conn, "p-gone");
        soft_delete_project(&conn, "p-gone");
        let err2 = create(&conn, &input("p-gone", "乙", None)).unwrap_err();
        assert_eq!(err2.code, "not_found", "软删项目也不能挂新待办");
    }

    #[test]
    fn create_rejects_a_non_iso_due_date() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let err = create(&conn, &input("p1", "甲", Some("2026.06.30"))).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        let t = create(&conn, &input("p1", "乙", Some("2026-06-30"))).unwrap();
        assert_eq!(t.due_date.as_deref(), Some("2026-06-30"));
    }

    #[test]
    fn create_then_read_roundtrip() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let t = create(&conn, &input("p1", "  补齐验收材料  ", Some("2026-11-02"))).unwrap();
        assert_eq!(t.title, "补齐验收材料", "写入要 trim");
        assert_eq!(t.done, false, "新行默认未完成");
        assert_eq!(get(&conn, &t.id).unwrap(), t, "create 回来的行必须与再读的一致");
        assert_eq!(list(&conn, Some("p1")).unwrap().len(), 1);
        assert_eq!(list(&conn, None).unwrap().len(), 1, "None = 跨项目全部");
    }

    #[test]
    fn list_hides_soft_deleted_todos() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let t = create(&conn, &input("p1", "甲", None)).unwrap();
        delete(&conn, &t.id).unwrap();
        assert!(list(&conn, None).unwrap().is_empty());
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM todos WHERE deleted_at IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "delete 是软删：行要在库里（D8）");
        assert_eq!(get(&conn, &t.id).unwrap_err().code, "not_found");
    }

    /// D8 的兑现点，也是 T8 变异 M① 的靶子：**把 `list` 那句
    /// `AND p.deleted_at IS NULL` 摘掉，这条当场红**。它是跨项目视图唯一一处这条过滤。
    #[test]
    fn cross_project_list_hides_todos_of_a_soft_deleted_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p-alive");
        seed(&conn, "p-dead");
        create(&conn, &input("p-alive", "活项目的待办", None)).unwrap();
        let dead = create(&conn, &input("p-dead", "已删项目的待办", None)).unwrap();
        soft_delete_project(&conn, "p-dead");

        let titles: Vec<String> = list(&conn, None)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(titles, vec!["活项目的待办"], "软删项目的待办不能出现在跨项目列表里");
        // 行没被删：这是「靠过滤隐藏、不靠删行」，不是级联删除。
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM todos WHERE id = ?1", [&dead.id], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "项目软删不许动待办的行（恢复项目即原样回来）");
        assert!(list(&conn, Some("p-dead")).unwrap().is_empty(), "指定已删项目也读不到");
    }

    #[test]
    fn update_changes_title_due_date_and_done_but_never_the_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        seed(&conn, "p2");
        let t = create(&conn, &input("p1", "甲", Some("2026-06-30"))).unwrap();
        update(
            &conn,
            &t.id,
            &TodoEdit { title: "乙".to_owned(), due_date: None, done: true },
        )
        .unwrap();
        let after = get(&conn, &t.id).unwrap();
        assert_eq!(after.title, "乙");
        assert_eq!(after.due_date, None, "update 要能把截止日期清空回 NULL");
        assert!(after.done, "勾选完成走 update（D12：不单开命令）");
        assert_eq!(after.project_id, "p1", "update 改不了归属");
        let err = update(
            &conn,
            &t.id,
            &TodoEdit { title: "  ".to_owned(), due_date: None, done: false },
        )
        .unwrap_err();
        assert_eq!(err.code, "invalid_input");
        let err2 = update(
            &conn,
            "no-such-id",
            &TodoEdit { title: "丙".to_owned(), due_date: Some("2026-06-30".to_owned()), done: false },
        )
        .unwrap_err();
        assert_eq!(err2.code, "not_found");
    }

    #[test]
    fn delete_is_soft_and_a_second_delete_reports_not_found() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let t = create(&conn, &input("p1", "甲", None)).unwrap();
        delete(&conn, &t.id).unwrap();
        assert_eq!(delete(&conn, &t.id).unwrap_err().code, "not_found", "二次删要响亮，不能静默成功");
    }

    /// 线格式钉键数（本仓形制：`backup.rs:629` 的 manifest 八键、`search.rs:774` 的七键）。
    /// 少一个键 = 前端读到 undefined，多一个键 = 白送（spec §四）。
    #[test]
    fn todo_line_format_has_exactly_seven_keys() {
        let t = Todo {
            id: "t1".to_owned(),
            project_id: "p1".to_owned(),
            title: "甲".to_owned(),
            due_date: Some("2026-06-30".to_owned()),
            done: false,
            created_at: "2026-10-14 00:00:00".to_owned(),
            updated_at: "2026-10-14 00:00:00".to_owned(),
        };
        let obj = serde_json::to_value(&t).unwrap();
        let mut keys: Vec<&str> = obj.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["createdAt", "done", "dueDate", "id", "projectId", "title", "updatedAt"],
            "Todo 的键集合变了就要同步 src/types/todo.ts"
        );
    }
}
```

本格有两条形状规则要说准，实现者照这两条写：① `norm_date` 是先 `norm`（trim、空串折成 NULL）再判形状，所以「带尾空格的合法日期」在 `is_iso_date` 层面是拒的、经 `norm_date` 会被 trim 后收下；② 月份 13 那种「形状对、历法错」的值本轮**收下**（spec §十二 风险 5）。两条各自有专属断言，不许合并成一条。

- [ ] **Step 2: 跑一遍确认 RED**

Run: `cd src-tauri && cargo test -j 1 --lib -- --nocapture todo > /tmp/t2.log 2>&1; echo "exit=$?"; grep -E "^error|cannot find|test result" /tmp/t2.log | head`
Expected：`exit=101`（或编译期 `error[E0432]`/`unresolved module`），因为 `mod todo;` 还没进 `lib.rs`。**这一步的红是编译层的红，不是断言层的红** —— 按本仓口径（坑 10 + 「简报里的测试必须可证伪」），编译层 RED 之后要在 Step 4 用**变异取证**补上「断言真的有牙」那格，不许拿「编不过」当 RED 就收工。

- [ ] **Step 3: 落 `todo.rs`，并在同一格里把待办四条命令接上线**

**命令必须与模块同格落地**，这不是图省事，是形状问题：只落模块不接线，`list`/`get`/`create`/`update`/`delete` 全是未使用代码，`dead_code` 会报出来；而本仓全仓只允许**一条**豁免（`db.rs:30`，结构门读数 1），为它加豁免就把那条门读成了 2，且「逐件挂豁免」本来就是错的账（坑 26：一个未接线的入口件只需要**一条**豁免）。

四步：

1. `lib.rs:1-17` 的模块表按**字母序**加 `mod todo;`（`mod tokenize;` 之后、`mod vault;` 之前）。
2. `lib.rs` 的 use 区加一行 `use crate::todo::{Todo, TodoEdit, TodoInput};`（照 `use crate::ledger::{…}` 那一族的位置）。
3. 四条命令，照 `ledger_env_create`（`lib.rs:289-296`）的形制：`state: State<'_, AppState>` 起头、`let conn = db(&state)?;` 取锁、函数体**零 `await`**（`MutexGuard` 不是 `Send`，M8 spec D11），所以 `tauri::command(async)` 的计数**不动**：

```rust
#[tauri::command]
fn todo_list(state: State<'_, AppState>, project_id: Option<String>) -> AppResult<Vec<todo::Todo>> {
    let conn = db(&state)?;
    todo::list(&conn, project_id.as_deref())
}

#[tauri::command]
fn todo_create(state: State<'_, AppState>, input: TodoInput) -> AppResult<Todo> {
    let conn = db(&state)?;
    todo::create(&conn, &input)
}

#[tauri::command]
fn todo_update(state: State<'_, AppState>, id: String, input: TodoEdit) -> AppResult<()> {
    let conn = db(&state)?;
    todo::update(&conn, &id, &input)
}

#[tauri::command]
fn todo_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    todo::delete(&conn, &id)
}
```

4. `generate_handler!` 列表**末尾**追加四条，并保持「只有最后一条不带尾逗号」的现有形状（即给 `backup_import` 补逗号、`todo_delete` 作末条）⇒ 本格终态 **49 条**，现取：`sed -n '/generate_handler/,/^      \]/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"`（逗号必须可选，写死 `,$` 会漏算末条 —— 坑 21）。

`project_id: Option<String>` 的 JS 侧要显式传 `null`（`api.ts` 里写 `{ projectId }`，调用方传 `null` 而不是 `undefined`），别赌缺键的行为。

- [ ] **Step 4: 跑测试确认绿**

Run: `cd src-tauri && cargo test -j 1 --lib todo > /tmp/t2b.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t2b.log`
Expected：`exit=0`、**12 passed / 0 failed**（本格具名测试 12 条；全量应到 **244 passed** = T1 后的 232 + 12，**实跑核对**）。

- [ ] **Step 5: 两条 clippy + 全量**

Run: `cd src-tauri && cargo clippy --lib -- -D warnings > /tmp/c1.log 2>&1; echo "c1=$?"; cargo clippy --lib --all-targets -- -D warnings > /tmp/c2.log 2>&1; echo "c2=$?"; cargo test -j 1 --lib > /tmp/all.log 2>&1; echo "test=$?"; grep "test result" /tmp/all.log`
Expected：三个都 `0`。**`list` 那个 `format!` 只包常量、且参数全走 `?1` 绑定** —— clippy 若对 `&format!(…)` 提 `needless_borrow`，就把 `&` 去掉按提示改，别为此加 `#[allow]`。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/todo.rs src-tauri/src/lib.rs
git commit -m "feat(m9): todo.rs 的日期形状判据与四操作（一条 SQL 同时管跨项目与按项目筛、软删项目靠 p.deleted_at IS NULL 隐藏而不删行）"
```

---

<!-- APPEND-MARKER-3 -->


