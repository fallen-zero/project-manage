# M9 待办清单 + 项目里程碑 + 到期预警 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让待办成为独立一级栏目（跨项目新增/编辑/删除/勾选完成/按项目筛选/三视图），新建项目里程碑表，并把「到期预警」以红（已逾期）/黄（7 天内）两档做在待办页顶部。

**Architecture:** V5 迁移追加两张同形制的业务表（`todos` / `milestones`，都是 uuid 主键 + `project_id` 外键 + `done` 勾选 + 软删三件套），Rust 侧两个新模块各带四条 CRUD 命令（共八条新 IPC，函数体全同步）。**预警不落在 SQL**：前端纯函数 `src/lib/due-warning.ts` 吃「全部待办 + 全部里程碑 + 项目列表」三份已过滤数据与传入的 `today` 算红黄；排序与三视图也在前端（`src/lib/todo-view.ts`），能被 `node --test` 直跑。

**Tech Stack:** Rust 2021 / rusqlite 0.40.2 bundled / serde camelCase / Tauri 2；前端 React 19 + TS + Vite + Tailwind 4 + TanStack Router（hash history）+ shadcn/ui；测试 `cargo test -j 1 --lib` 与 `node --test "tests/**/*.test.ts"`。**零新 crate、零新 npm 依赖。**

**Spec:** `docs/superpowers/specs/2026-10-14-m9-todos-milestones-design.md`（决策 D1–D12、数据模型 §三、命令契约 §四、失败面 §五、预警判据 §六、真机十格 §十、不在本轮 §十一、风险 §十二、回写 §十三；**分歧以 spec 为准**）。需求原文在 `docs/思维导图.md:33-36`（里程碑）、`:38-45`（待办）、`:73`（到期预警）—— 不在技术方案的里程碑表里（HANDOFF 坑 34）。

## 任务总览与派发前必读（八格已写满）

- **Task 1** V5 迁移（`db.rs` 两张表 + 两个索引）+ `backup.rs` 明文告警那句改准
- **Task 2** `todo.rs` 判据与四操作 + **待办四条命令同格接线**（45→49；命令与模块同格是必须的：只落模块会逼出第二条 `dead_code` 豁免，把「全仓唯一豁免」那条结构门读成 2）
- **Task 3** `milestone.rs` 判据与四操作 + 里程碑四条命令（49→53）
- **Task 4** 前端契约层：两个线类型 + 八个封装 + `due-warning.ts` + `todo-view.ts` + 十五行 node 测试 + 五段阶段词的 Rust↔TS 文本门
- **Task 5** `/todos` 页（预警块 + 三视图 + 项目筛选 + 新增/编辑/勾选/删除）+ nav 第 5 格 + 路由
- **Task 6** `milestone-panel.tsx` + 接进项目详情页
- **Task 7** 全量门禁 + **六处变异取证** + 真机十格（**控制方执行，子代理禁跑 dev**）
- **Task 8** 活文档回写（技术方案 / 开发进度 / HANDOFF，清单在 spec §十三）

**期望数链**（由逐格列名数出；**派发每格前必须用实跑订正**，坑 5 + `verify-plan-test-count-expectations`）：cargo **225 → 232 → 244 → 253 → 254**；node **10 → 25**；命令 **45 → 49 → 53**；`TARGET_VERSION` **4 → 5**；`tauri::command(async)` 恒 **4**；`dead_code` 豁免恒 **1**；`Cargo.lock` `[[package]]` 恒 **616**。

**本计划与 spec 文本不同的三处，都是照仓内现成形制收紧、不是偏离**：① 里程碑区落成 `src/components/milestone-panel.tsx`（spec §七 只写「改 `project-detail.tsx`」，仓里有 `ledger-panel.tsx` 这个先例）；② 新增 `src/lib/todo-view.ts`（spec §六 说三视图在前端排，没定文件名）；③ 变异清单从 spec §九 的五条扩到六条（`milestone-panel` 的 `stateOf` 与 `due-warning` 是两份实现，各要独立打一次红，见 T7 表 M⑥ 与文末有名缺口第 1 条）。

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

const SELECT_COLUMNS: &str = "t.id, t.project_id, t.title, t.due_date, t.done, t.created_at, t.updated_at";

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
            "SELECT {SELECT_COLUMNS}
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
        "SELECT {SELECT_COLUMNS} FROM todos t WHERE t.id = ?1 AND t.deleted_at IS NULL"
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

## Task 3: `milestone.rs` —— 里程碑的判据与四操作（命令 49→53）

**Files:**
- Create: `src-tauri/src/milestone.rs`
- Modify: `src-tauri/src/lib.rs`（`mod milestone;` 按字母序在 `mod ledger;` 之后、`mod ocr;` 之前；四条命令；`generate_handler!` 末尾追加四条 ⇒ **53**）
- Test: `src-tauri/src/milestone.rs` 的 `mod tests`

**Interfaces:**
- Consumes: T1 的 `milestones` 表；**T2 的 `pub(crate) fn todo::norm_date(column: &str, value: Option<String>) -> AppResult<Option<String>>`**（同一条日期判据只写一处，D7）
- Produces: `milestone::Milestone`、`milestone::MilestoneInput { project_id, stage, plan_date, note }`、`milestone::MilestoneEdit { stage, plan_date, note, done }`、`pub(crate) const STAGES: [&str; 5]`（T4 那条词表一致性门要对它逐字比）、`list/get/create/update/delete` 同 T2 的形状

- [ ] **Step 1: 写九行失败测试**

```rust
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::todo::norm_date;

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查表单必填项"))
}

fn not_found(id: &str) -> AppError {
    AppError::new("not_found", &format!("记录不存在：{id}"), Some("刷新后重试"))
}

fn norm(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// 需求原文那五个词（`思维导图.md:35-36`）。与 `projects.status` 那八段是**两套词**（D4），
/// 别去复用 `project::STATUS_OPTIONS`，也别让一个下拉喂两个字段。
pub(crate) const STAGES: [&str; 5] = ["立项", "评审", "联调", "上线", "验收"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub id: String,
    pub project_id: String,
    pub stage: String,
    pub plan_date: Option<String>,
    pub note: String,
    pub done: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MilestoneInput {
    pub project_id: String,
    pub stage: String,
    pub plan_date: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MilestoneEdit {
    pub stage: String,
    pub plan_date: Option<String>,
    pub note: Option<String>,
    pub done: bool,
}

const SELECT_COLUMNS: &str =
    "m.id, m.project_id, m.stage, m.plan_date, m.note, m.done, m.created_at, m.updated_at";

fn map_milestone(row: &rusqlite::Row<'_>) -> rusqlite::Result<Milestone> {
    Ok(Milestone {
        id: row.get(0)?,
        project_id: row.get(1)?,
        stage: row.get(2)?,
        plan_date: row.get(3)?,
        note: row.get(4)?,
        done: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
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

/// 与 `todo::list` 同一条 SQL 形状：一条语句同时管「跨项目全部」与「按项目筛」，
/// 且 `p.deleted_at IS NULL` 在这张表上同样只有这一处（D8）。
pub fn list(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<Milestone>> {
    let mut stmt = conn.prepare(
        &format!(
            "SELECT {SELECT_COLUMNS}
               FROM milestones m
               JOIN projects p ON p.id = m.project_id AND p.deleted_at IS NULL
              WHERE m.deleted_at IS NULL AND (?1 IS NULL OR m.project_id = ?1)
              ORDER BY m.plan_date IS NULL, m.plan_date, m.created_at"
        ),
    )?;
    let rows = stmt.query_map(params![project_id], map_milestone)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Milestone> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM milestones m WHERE m.id = ?1 AND m.deleted_at IS NULL"
    );
    conn.query_row(&sql, [id], map_milestone)
        .optional()?
        .ok_or_else(|| not_found(id))
}

/// 白名单在**应用层**判：落到 SQLite 的 CHECK 会变成 `db_failed`，用户看不出是自己填错（spec §五 第 3 道）。
/// CHECK 仍然留着，它挡的是绕过命令直写库的那条路。
fn check_stage(stage: &str) -> AppResult<()> {
    if STAGES.contains(&stage) {
        Ok(())
    } else {
        Err(invalid(&format!(
            "阶段「{stage}」不在 {} 之内",
            STAGES.join("/")
        )))
    }
}

pub fn create(conn: &Connection, input: &MilestoneInput) -> AppResult<Milestone> {
    project_exists(conn, &input.project_id)?;
    check_stage(&input.stage)?;
    let plan_date = norm_date("计划日期", input.plan_date.clone())?;
    let note = norm(input.note.clone()).unwrap_or_default();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO milestones (id, project_id, stage, plan_date, note) VALUES (?1,?2,?3,?4,?5)",
        params![id, input.project_id, input.stage, plan_date, note],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, edit: &MilestoneEdit) -> AppResult<()> {
    check_stage(&edit.stage)?;
    let plan_date = norm_date("计划日期", edit.plan_date.clone())?;
    let note = norm(edit.note.clone()).unwrap_or_default();
    let changed = conn.execute(
        "UPDATE milestones SET stage=?2, plan_date=?3, note=?4, done=?5, updated_at=datetime('now')
          WHERE id=?1 AND deleted_at IS NULL",
        params![id, edit.stage, plan_date, note, edit.done],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE milestones SET deleted_at = datetime('now'), updated_at = datetime('now')
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

    fn seed(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, status) VALUES (?1, '里程碑测试项目', '立项')",
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

    fn input(project_id: &str, stage: &str, date: Option<&str>, note: Option<&str>) -> MilestoneInput {
        MilestoneInput {
            project_id: project_id.to_owned(),
            stage: stage.to_owned(),
            plan_date: date.map(str::to_owned),
            note: note.map(str::to_owned),
        }
    }

    fn edit(stage: &str, date: Option<&str>, note: Option<&str>, done: bool) -> MilestoneEdit {
        MilestoneEdit {
            stage: stage.to_owned(),
            plan_date: date.map(str::to_owned),
            note: note.map(str::to_owned),
            done,
        }
    }

    #[test]
    fn create_refuses_stage_words_outside_the_five_with_invalid_input() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for bad in ["初验", "需求", "开发", "归档", ""] {
            let err = create(&conn, &input("p1", bad, None, None)).unwrap_err();
            assert_eq!(err.code, "invalid_input", "「{bad}」要的是应用层那句，不是 db_failed");
        }
        for good in STAGES {
            create(&conn, &input("p1", good, None, None)).unwrap();
        }
        assert_eq!(list(&conn, Some("p1")).unwrap().len(), 5, "五段各建一行都要成");
    }

    #[test]
    fn create_rejects_non_iso_plan_date() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        // 左半条必须拿「形状就不对」的值去断：点分隔那种长度也是 10，但第 5、8 位不是 `-`，
        // 所以它测的是形状判据本身（拿合法日期断 unwrap_err 就是自己红，T2 踩过一次）。
        let err = create(&conn, &input("p1", "上线", Some("2026.10.14"), None)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("计划日期"), "错误文本要点名是哪一列：{}", err.message);
        let m = create(&conn, &input("p1", "上线", Some("2026-10-14"), None)).unwrap();
        assert_eq!(m.plan_date.as_deref(), Some("2026-10-14"));
    }

    #[test]
    fn create_rejects_project_that_does_not_exist_or_is_deleted() {
        let conn = db::open_in_memory().unwrap();
        assert_eq!(create(&conn, &input("nope", "立项", None, None)).unwrap_err().code, "not_found");
        seed(&conn, "p-gone");
        soft_delete_project(&conn, "p-gone");
        assert_eq!(create(&conn, &input("p-gone", "立项", None, None)).unwrap_err().code, "not_found");
    }

    #[test]
    fn create_then_list_roundtrip_keeps_note_and_done() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let m = create(&conn, &input("p1", "评审", Some("2026-10-14"), Some("  专家意见已回  ")))
            .unwrap();
        assert_eq!(m.note, "专家意见已回", "写入要 trim");
        assert_eq!(m.done, false);
        let bare = create(&conn, &input("p1", "验收", None, None)).unwrap();
        assert_eq!(bare.note, "", "不填 note 就是空串，不是 NULL（列本身 NOT NULL DEFAULT ''）");
        assert_eq!(get(&conn, &m.id).unwrap(), m);
        // 本模块的 list 默认按 plan_date 升序、空日期落最后（前端三视图还要各排各的，这条只是稳定基线）
        let ids: Vec<String> = list(&conn, Some("p1")).unwrap().into_iter().map(|x| x.id).collect();
        assert_eq!(ids, vec![m.id.clone(), bare.id.clone()]);
        assert_eq!(list(&conn, None).unwrap().len(), 2);
    }

    #[test]
    fn list_hides_soft_deleted_milestones() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let m = create(&conn, &input("p1", "联调", None, None)).unwrap();
        delete(&conn, &m.id).unwrap();
        assert!(list(&conn, None).unwrap().is_empty());
        assert_eq!(get(&conn, &m.id).unwrap_err().code, "not_found");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM milestones WHERE deleted_at IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "软删：行要在库里");
    }

    /// D8 的兑现点，T7 变异 M① 在这一格的靶子：**摘掉 `AND p.deleted_at IS NULL` 这条当场红**。
    #[test]
    fn cross_project_list_hides_milestones_of_a_soft_deleted_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p-alive");
        seed(&conn, "p-dead");
        create(&conn, &input("p-alive", "上线", None, None)).unwrap();
        let dead = create(&conn, &input("p-dead", "评审", None, None)).unwrap();
        soft_delete_project(&conn, "p-dead");

        let stages: Vec<String> = list(&conn, None)
            .unwrap()
            .into_iter()
            .map(|m| m.stage)
            .collect();
        assert_eq!(stages, vec!["上线"], "软删项目的里程碑不能进跨项目列表（预警就是这条查询）");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM milestones WHERE id = ?1", [&dead.id], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "行留着，恢复项目即原样回来");
    }

    #[test]
    fn update_sets_done_without_touching_the_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        seed(&conn, "p2");
        let m = create(&conn, &input("p1", "立项", Some("2026-10-14"), Some("备查"))).unwrap();
        update(&conn, &m.id, &edit("评审", None, None, true)).unwrap();
        let after = get(&conn, &m.id).unwrap();
        assert_eq!(after.stage, "评审");
        assert_eq!(after.plan_date, None, "计划日期要能清回 NULL");
        assert_eq!(after.note, "", "note 传 None 就是清成空串");
        assert!(after.done, "D5 的兑现点：勾掉的历史里程碑不再进预警");
        assert_eq!(after.project_id, "p1", "update 改不了归属");
        update(&conn, &m.id, &edit("验收", None, None, false)).unwrap();
        assert_eq!(get(&conn, &m.id).unwrap().stage, "验收");
        assert_eq!(update(&conn, "no-such", &edit("上线", None, None, false)).unwrap_err().code, "not_found");
    }

    #[test]
    fn delete_is_soft_and_a_second_delete_reports_not_found() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let m = create(&conn, &input("p1", "验收", None, None)).unwrap();
        delete(&conn, &m.id).unwrap();
        assert_eq!(delete(&conn, &m.id).unwrap_err().code, "not_found");
    }

    /// 线格式钉键数：八键（spec §四）。少一个键 = 前端读 undefined，多一个键 = 白送。
    #[test]
    fn milestone_line_format_has_exactly_eight_keys() {
        let m = Milestone {
            id: "m1".to_owned(),
            project_id: "p1".to_owned(),
            stage: "立项".to_owned(),
            plan_date: Some("2026-10-14".to_owned()),
            note: "备查".to_owned(),
            done: false,
            created_at: "2026-10-14 00:00:00".to_owned(),
            updated_at: "2026-10-14 00:00:00".to_owned(),
        };
        let obj = serde_json::to_value(&m).unwrap();
        let mut keys: Vec<&str> = obj.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "createdAt",
                "done",
                "id",
                "note",
                "planDate",
                "projectId",
                "stage",
                "updatedAt"
            ],
            "Milestone 的键集合变了就要同步 src/types/milestone.ts"
        );
    }
}
```

上面这批断言里有一条通用的自查，落盘后必须做：**每一条 `unwrap_err()` 用的值，形状一定要真的不合法**（T2 我就把两个合法日期写进了「该被拒」的那一列，那条测试会自己红）。本仓这一轮还遇到「工具把某些分隔符显示成 `-`」的显示层问题，所以别只靠眼看：
`python -c "import io;s=io.open('src-tauri/src/milestone.rs',encoding='utf-8',newline='').read();i=s.index('fn create_rejects_non_iso_plan_date');print([hex(ord(c)) for c in s[i:i+320]])"`
按码点确认那条被拒的值**不是**「长度 10 且第 5、8 位为 0x2d」的形状。T2 的 `is_iso_date` 那批值同查。

- [ ] **Step 2: 跑一遍确认 RED（编译层）**

Run: `cd src-tauri && cargo test -j 1 --lib milestone > /tmp/t3.log 2>&1; echo "exit=$?"; grep -E "^error|unresolved module|test result" /tmp/t3.log | head`
Expected：`exit=101`，报在 `unresolved module`/`couldn't read src/milestone.rs`。

- [ ] **Step 3: 落 `milestone.rs` 并接四条命令上线（同格必须，理由与 T2 Step 3 完全相同：只落模块会逼出第二条 `dead_code` 豁免）**

`lib.rs` 三处：

```rust
mod milestone;   // 字母序：mod ledger; 之后、mod ocr; 之前
```

```rust
#[tauri::command]
fn milestone_list(state: State<'_, AppState>, project_id: Option<String>) -> AppResult<Vec<milestone::Milestone>> {
    let conn = db(&state)?;
    milestone::list(&conn, project_id.as_deref())
}

#[tauri::command]
fn milestone_create(state: State<'_, AppState>, input: MilestoneInput) -> AppResult<Milestone> {
    let conn = db(&state)?;
    milestone::create(&conn, &input)
}

#[tauri::command]
fn milestone_update(state: State<'_, AppState>, id: String, input: MilestoneEdit) -> AppResult<()> {
    let conn = db(&state)?;
    milestone::update(&conn, &id, &input)
}

#[tauri::command]
fn milestone_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    milestone::delete(&conn, &id)
}
```

`generate_handler!` 末尾追加 `milestone_list, milestone_create, milestone_update, milestone_delete`，并保持「只有最后一条不带尾逗号」的形状 ⇒ 本格终态 **53**：
`sed -n '/generate_handler/,/^      \]/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"` → **53**。
同时 `grep -c 'tauri::command(async)' src-tauri/src/lib.rs` 仍须 **4**（八条新命令全同步；`MutexGuard` 不是 `Send`，M8 spec D11）。

- [ ] **Step 4: 全绿 + 全量 + 两条 clippy**

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/t3b.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t3b.log; cargo clippy --lib -- -D warnings > /tmp/c1.log 2>&1; echo "c1=$?"; cargo clippy --lib --all-targets -- -D warnings > /tmp/c2.log 2>&1; echo "c2=$?"`
Expected：`exit=0`、**253 passed**（244 + 本格 9 条；**实跑核对**）、两个 clippy `0`、`allow(dead_code)` 计数仍 **1**（`grep -rn "allow(dead_code)" src-tauri/src/ | wc -l`）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/milestone.rs src-tauri/src/lib.rs
git commit -m "feat(m9): milestone.rs 的阶段白名单与四操作（五段词与 projects.status 八段是两套、应用层先判免得落成 db_failed；日期判据复用 todo::norm_date 不写第二份）+ 命令 49→53"
```

---

## Task 4: 前端契约层（线类型 + 八个封装 + 预警与视图两个纯函数 + 词表一致性门）

**Files:**
- Create: `src/types/todo.ts`、`src/types/milestone.ts`、`src/lib/due-warning.ts`、`src/lib/todo-view.ts`、`tests/due-warning.test.ts`、`tests/todo-view.test.ts`
- Modify: `src/lib/api.ts`（八个封装）、`src-tauri/src/lib.rs`（一条词表一致性文本门测试）

**Interfaces:**
- Consumes: T2/T3 的线格式（`Todo` 七键、`Milestone` 八键，camelCase）、`Project`（`src/types/project.ts`，可空字段是 `string | null` 而不是可选属性）、`STAGES`（`milestone.rs`）
- Produces: `due-warning.ts` 的 `buildDueWarnings(input, today, horizonDays?) -> { items: DueItem[]; unparsed: number }`、`DUE_HORIZON_DAYS`、`localToday()`、`projectLabel(p)`；`todo-view.ts` 的 `sortTodos(todos, view)`、`groupByProject(todos, projects)`、`type TodoView`；`api.ts` 的 `listTodos/createTodo/updateTodo/deleteTodo/listMilestones/createMilestone/updateMilestone/deleteMilestone`

**tsconfig 的既成事实**（写代码前知道，别踩）：`strict: true` 但**没开** `noUncheckedIndexedAccess` ⇒ 数组下标与解构直接当 `T` 用；`allowImportingTsExtensions: true` ⇒ 测试里 `import … from "../src/lib/xxx.ts"` 带后缀是合法的；`include: ["src"]` ⇒ `tests/` 不在 `tsc` 的射程内（所以测试里的类型错误只有 `node --test` 跑得出来）。

- [ ] **Step 1: 两个线类型文件**

```ts
// src/types/todo.ts
// 与 src-tauri/src/todo.rs 的出参逐字段对齐（camelCase 来自 #[serde(rename_all = "camelCase")]）。
// 漏一个键就是运行时 undefined（同 src/types/backup.ts 开头那句注释的告诫）。
export interface Todo {
  id: string;
  projectId: string;
  title: string;
  dueDate: string | null;
  done: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface TodoInput {
  projectId: string;
  title: string;
  dueDate: string | null;
}

/// 与 Rust 的 TodoEdit 对齐：没有 projectId（改归属不在本轮，spec §十一）。
export interface TodoEdit {
  title: string;
  dueDate: string | null;
  done: boolean;
}

/** 表单里空串要折成 null，否则后端把 "" 当非法形状拒掉。 */
export const dueOrNull = (raw: string): string | null => (raw.trim() === "" ? null : raw.trim());
```

```ts
// src/types/milestone.ts
export interface Milestone {
  id: string;
  projectId: string;
  stage: string;
  planDate: string | null;
  note: string;
  done: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface MilestoneInput {
  projectId: string;
  stage: string;
  planDate: string | null;
  note: string | null;
}

export interface MilestoneEdit {
  stage: string;
  planDate: string | null;
  note: string | null;
  done: boolean;
}

// 需求原文那五个词（docs/思维导图.md:35-36）。与 projects.status 那八段是两套词（spec D4）：
// 这一份喂里程碑的下拉，statusOptions()（现成命令）喂项目状态的下拉，**不许互换**。
// lib.rs 的 stage_words_are_identical_in_rust_and_the_frontend_type 钉这三处逐字一致。
export const MILESTONE_STAGES = ["立项", "评审", "联调", "上线", "验收"] as const;
```

- [ ] **Step 2: `api.ts` 八个封装**（接在现有 `backupImport` 那一族之后，照同一形制）

```ts
export const listTodos = (projectId: string | null) =>
  invoke<Todo[]>("todo_list", { projectId });
export const createTodo = (input: TodoInput) => invoke<Todo>("todo_create", { input });
export const updateTodo = (id: string, input: TodoEdit) => invoke<void>("todo_update", { id, input });
export const deleteTodo = (id: string) => invoke<void>("todo_delete", { id });

export const listMilestones = (projectId: string | null) =>
  invoke<Milestone[]>("milestone_list", { projectId });
export const createMilestone = (input: MilestoneInput) =>
  invoke<Milestone>("milestone_create", { input });
export const updateMilestone = (id: string, input: MilestoneEdit) =>
  invoke<void>("milestone_update", { id, input });
export const deleteMilestone = (id: string) => invoke<void>("milestone_delete", { id });
```

对应要加进文件头的 `import type { Todo, TodoInput, TodoEdit } from "@/types/todo";` 与 `import type { Milestone, MilestoneInput, MilestoneEdit } from "@/types/milestone";`。
**`listTodos(null)` 传的是 `null` 而不是省键**（后端是 `Option<String>`；省键在本仓没有先例，别赌）。

- [ ] **Step 3: 写失败的 node 测试（十五行）**

```ts
// tests/due-warning.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { buildDueWarnings, localToday, DUE_HORIZON_DAYS } from "../src/lib/due-warning.ts";
import type { Todo } from "../src/types/todo.ts";
import type { Milestone } from "../src/types/milestone.ts";
import type { Project } from "../src/types/project.ts";

// today 一律喂常量，函数不读机器时钟：档位边界才是可证伪的。
const TODAY = "2026-10-14";
const iso = (offsetDays: number) => {
  const ms = Date.UTC(2026, 9, 14) + offsetDays * 86_400_000;
  return new Date(ms).toISOString().slice(0, 10);
};

const project = (over: Partial<Project> = {}): Project => ({
  id: "p1",
  name: "甲项目",
  code: "P-01",
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
  ...over,
});

const todo = (over: Partial<Todo> = {}): Todo => ({
  id: "t1",
  projectId: "p1",
  title: "待办一",
  dueDate: null,
  done: false,
  createdAt: "2026-10-01 00:00:00",
  updatedAt: "2026-10-01 00:00:00",
  ...over,
});

const milestone = (over: Partial<Milestone> = {}): Milestone => ({
  id: "m1",
  projectId: "p1",
  stage: "上线",
  planDate: null,
  note: "",
  done: false,
  createdAt: "2026-10-01 00:00:00",
  updatedAt: "2026-10-01 00:00:00",
  ...over,
});

const empty = { todos: [] as Todo[], milestones: [] as Milestone[], projects: [] as Project[] };

test("昨天到期 ⇒ overdue 且 days 为负", () => {
  const r = buildDueWarnings({ ...empty, todos: [todo({ dueDate: iso(-1) })], projects: [project()] }, TODAY);
  assert.equal(r.items.length, 1);
  assert.equal(r.items[0].state, "overdue");
  assert.equal(r.items[0].days, -1);
  assert.equal(r.items[0].projectLabel, "甲项目（P-01）");
});

test("今天到期 ⇒ soon（不是 overdue），days 为 0", () => {
  const r = buildDueWarnings({ ...empty, todos: [todo({ dueDate: TODAY })], projects: [project()] }, TODAY);
  assert.equal(r.items[0].state, "soon");
  assert.equal(r.items[0].days, 0);
});

test("第 7 天 ⇒ 仍进黄档（边界含）", () => {
  const r = buildDueWarnings({ ...empty, todos: [todo({ dueDate: iso(DUE_HORIZON_DAYS) })], projects: [project()] }, TODAY);
  assert.equal(r.items.length, 1);
  assert.equal(r.items[0].state, "soon");
});

test("第 8 天 ⇒ 不进预警", () => {
  const r = buildDueWarnings({ ...empty, todos: [todo({ dueDate: iso(DUE_HORIZON_DAYS + 1) })], projects: [project()] }, TODAY);
  assert.deepEqual(r.items, []);
});

test("勾掉的待办不算（判据③）", () => {
  const r = buildDueWarnings({ ...empty, todos: [todo({ dueDate: iso(-3), done: true })], projects: [project()] }, TODAY);
  assert.deepEqual(r.items, []);
});

test("勾掉的里程碑不算 —— D5 加那一列就是为了这一条", () => {
  const r = buildDueWarnings(
    { ...empty, milestones: [milestone({ planDate: iso(-30), done: true })], projects: [project()] },
    TODAY,
  );
  assert.deepEqual(r.items, [], "立项/评审的计划日期天然在过去，没 done 就会刷一屏误红");
});

test("没日期的行整个跳过，且不进 unparsed", () => {
  const r = buildDueWarnings(
    {
      todos: [todo({ id: "t-null", dueDate: null }), todo({ id: "t-empty", dueDate: "" })],
      milestones: [milestone({ id: "m-null", planDate: null })],
      projects: [project()],
    },
    TODAY,
  );
  assert.deepEqual(r.items, []);
  assert.equal(r.unparsed, 0, "空 = 没填，不是填错");
});

test("非 ISO 的交付截止 ⇒ 不亮但计入 unparsed（判据②，对 deadline 类它是唯一防线）", () => {
  const r = buildDueWarnings(
    { ...empty, projects: [project({ deliveryDeadline: "10月底前" })] },
    TODAY,
  );
  assert.deepEqual(r.items, []);
  assert.equal(r.unparsed, 1, "解析不了要响亮地说，不能静默少亮一条");
});

test("归档项目的交付截止不进预警，也不计 unparsed（判据⑤）", () => {
  const r = buildDueWarnings(
    { ...empty, projects: [project({ status: "归档", deliveryDeadline: iso(-5) })] },
    TODAY,
  );
  assert.deepEqual(r.items, []);
  assert.equal(r.unparsed, 0);
});

test("三来源汇总 + 排序：overdue 在前、再按日期、再按项目名", () => {
  const pA = project({ id: "pa", name: "乙项目", code: null, deliveryDeadline: iso(2) });
  const pB = project({ id: "pb", name: "丙项目", deliveryDeadline: iso(-2) });
  const r = buildDueWarnings(
    {
      todos: [todo({ id: "ta", projectId: "pa", dueDate: iso(1), title: "待办甲" })],
      milestones: [milestone({ id: "ma", projectId: "pb", stage: "上线", planDate: iso(-1) })],
      projects: [pA, pB],
    },
    TODAY,
  );
  assert.deepEqual(
    r.items.map((i) => [i.kind, i.date, i.state]),
    [
      ["deadline", iso(-2), "overdue"],
      ["milestone", iso(-1), "overdue"],
      ["todo", iso(1), "soon"],
      ["deadline", iso(2), "soon"],
    ],
    `实际顺序不对：${JSON.stringify(r.items)}`,
  );
  assert.equal(r.items[1].label, "上线", "里程碑那一格的 label 是阶段名");
  assert.equal(r.items[2].label, "待办甲");
  assert.equal(r.items[3].label, "交付截止");
});

test("horizonDays 传 0 ⇒ 只算今天与已逾期", () => {
  const r = buildDueWarnings(
    { ...empty, todos: [todo({ id: "t0", dueDate: TODAY }), todo({ id: "t1", dueDate: iso(1) })], projects: [project()] },
    TODAY,
    0,
  );
  assert.deepEqual(r.items.map((i) => i.id), ["t0"]);
});

test("localToday 回本机日历日的 ISO 形状", () => {
  assert.match(localToday(), /^\d{4}-\d{2}-\d{2}$/);
});
```

```ts
// tests/todo-view.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { groupByProject, sortTodos } from "../src/lib/todo-view.ts";
import type { Todo } from "../src/types/todo.ts";
import type { Project } from "../src/types/project.ts";

const t = (id: string, projectId: string, dueDate: string | null, createdAt: string, title = id): Todo => ({
  id,
  projectId,
  title,
  dueDate,
  done: false,
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

test("按截止排序：未完成的在前、空日期落最后、同键按 createdAt 稳定", () => {
  const rows = [
    t("a", "p1", null, "2026-10-05 00:00:00"),
    t("b", "p1", "2026-10-20", "2026-10-01 00:00:00"),
    t("c", "p1", null, "2026-10-02 00:00:00"),
    t("d", "p1", "2026-10-10", "2026-10-01 00:00:00"),
  ];
  assert.deepEqual(
    sortTodos(rows, "byDue").map((x) => x.id),
    ["d", "b", "c", "a"],
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
```

- [ ] **Step 4: 跑一遍确认它红在「模块不存在」**

Run: `node --test "tests/**/*.test.ts" > /tmp/n4.log 2>&1; echo "exit=$?"; grep -E "^# (tests|pass|fail)|Cannot find" /tmp/n4.log | head -6`
Expected：`exit=1`，两条新文件都报 `Cannot find module …due-warning`（基线那 10 条仍应 pass）。

- [ ] **Step 5: 写 `src/lib/due-warning.ts`**

```ts
// 到期预警的纯逻辑（spec §六 / D1）。这一格刻意只 import type（会被擦除），所以它能被
// `node --test` 直跑，不必给仓库添 jsdom（先例：search-order.ts、backup-name.ts）。
import type { Project } from "@/types/project";
import type { Milestone } from "@/types/milestone";
import type { Todo } from "@/types/todo";

export const DUE_HORIZON_DAYS = 7;

export type DueKind = "todo" | "milestone" | "deadline";
export type DueState = "overdue" | "soon";

export interface DueItem {
  kind: DueKind;
  /** todo/milestone 的行 id；deadline 那一类用项目 id。 */
  id: string;
  projectId: string;
  projectLabel: string;
  /** 待办标题 / 里程碑阶段名 / 「交付截止」。 */
  label: string;
  date: string;
  state: DueState;
  /** 距今天的日历日数：overdue 为负，含今天为 0。 */
  days: number;
}

export interface DueReport {
  items: DueItem[];
  /** 形状解析不了、被跳过的行数（界面必须念出来，别静默少亮 —— 判据②）。 */
  unparsed: number;
}

export interface DueInput {
  todos: Todo[];
  milestones: Milestone[];
  projects: Project[];
}

const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;
const DAY_MS = 86_400_000;

/** 折成「自 epoch 的天数」：用 Date.UTC 构造 ⇒ 与本机时区无关，比的是日历日不是时刻。
 *  调用方传进来的 today 已经过 ISO_DATE 形状保证（页面里 localToday 自己产，外部输入走后端校验）。 */
function toDayNumber(iso: string): number {
  const [y, m, d] = iso.split("-").map(Number);
  return Math.floor(Date.UTC(y, m - 1, d) / DAY_MS);
}

/** 页面用这一条读一次时钟；纯函数本体不读，才好测。 */
export function localToday(): string {
  const now = new Date();
  const pad = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${pad(now.getFullYear(), 4)}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

export function projectLabel(p: Project): string {
  return p.code ? `${p.name}（${p.code}）` : p.name;
}

export function buildDueWarnings(
  input: DueInput,
  today: string,
  horizonDays: number = DUE_HORIZON_DAYS,
): DueReport {
  const byId = new Map(input.projects.map((p) => [p.id, p]));
  const todayDay = toDayNumber(today);
  let unparsed = 0;
  const items: DueItem[] = [];

  const emit = (kind: DueKind, id: string, projectId: string, label: string, raw: string | null) => {
    if (raw === null || raw === "") return;
    if (!ISO_DATE.test(raw)) {
      unparsed += 1;
      return;
    }
    const days = toDayNumber(raw) - todayDay;
    if (days > horizonDays) return;
    const p = byId.get(projectId);
    items.push({
      kind,
      id,
      projectId,
      projectLabel: p ? projectLabel(p) : "（项目缺失）",
      label,
      date: raw,
      days,
      state: days < 0 ? "overdue" : "soon",
    });
  };

  for (const t of input.todos) {
    if (t.done) continue;
    emit("todo", t.id, t.projectId, t.title, t.dueDate);
  }
  for (const m of input.milestones) {
    if (m.done) continue;
    emit("milestone", m.id, m.projectId, m.stage, m.planDate);
  }
  for (const p of input.projects) {
    // 判据⑤：归档 = 不再催事。这一类没有 done 勾选态，用「未归档」当等价门。
    if (p.status === "归档") continue;
    emit("deadline", p.id, p.id, "交付截止", p.deliveryDeadline);
  }

  items.sort((a, b) => {
    if (a.state !== b.state) return a.state === "overdue" ? -1 : 1;
    if (a.date !== b.date) return a.date < b.date ? -1 : 1;
    if (a.projectLabel !== b.projectLabel) return a.projectLabel < b.projectLabel ? -1 : 1;
    return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
  });
  return { items, unparsed };
}
```

- [ ] **Step 6: 写 `src/lib/todo-view.ts`**

```ts
// 待办页三视图的排序/分组（需求：全部 / 按项目分组 / 按截止日期排序）。
// 后端 list 的 ORDER BY 只是稳定基线，视图序在这里（先例：search-order.ts 把首屏排序放前端）。
import type { Project } from "@/types/project";
import type { Todo } from "@/types/todo";
import { projectLabel } from "@/lib/due-warning";

export type TodoView = "all" | "byProject" | "byDue";

export interface TodoGroup {
  projectId: string;
  projectLabel: string;
  todos: Todo[];
}

export function sortTodos(todos: Todo[], view: TodoView): Todo[] {
  const rows = [...todos]; // 绝不就地排调用方的数组
  if (view === "byDue") {
    rows.sort((a, b) => {
      if (a.done !== b.done) return a.done ? 1 : -1;
      const av = a.dueDate;
      const bv = b.dueDate;
      if (av === null && bv === null) return a.createdAt < b.createdAt ? -1 : a.createdAt > b.createdAt ? 1 : 0;
      if (av === null) return 1; // 空日期落最后（spec §六）
      if (bv === null) return -1;
      if (av !== bv) return av < bv ? -1 : 1;
      return a.createdAt < b.createdAt ? -1 : a.createdAt > b.createdAt ? 1 : 0;
    });
    return rows;
  }
  rows.sort((a, b) => (a.createdAt < b.createdAt ? -1 : a.createdAt > b.createdAt ? 1 : 0));
  return rows;
}

export function groupByProject(todos: Todo[], projects: Project[], view: TodoView): TodoGroup[] {
  const byId = new Map(projects.map((p) => [p.id, p]));
  const sorted = sortTodos(todos, view);
  const groups = new Map<string, TodoGroup>();
  for (const t of sorted) {
    const p = byId.get(t.projectId);
    const key = t.projectId;
    if (!groups.has(key)) {
      groups.set(key, { projectId: key, projectLabel: p ? projectLabel(p) : "（项目缺失）", todos: [] });
    }
    groups.get(key).todos.push(t);
  }
  return [...groups.values()].sort((a, b) =>
    a.projectLabel < b.projectLabel ? -1 : a.projectLabel > b.projectLabel ? 1 : 0,
  );
}
```

- [ ] **Step 7: 跑测试确认绿 + 前端两条门**

Run: `node --test "tests/**/*.test.ts" > /tmp/n5.log 2>&1; echo "exit=$?"; grep -E "^ℹ (tests|pass|fail)" /tmp/n5.log; npm run build > /tmp/nb.log 2>&1; echo "build=$?"; grep -c "error TS" /tmp/nb.log`
Expected：`exit=0`、**tests 25 / pass 25 / fail 0**（基线 10 + 15；**实跑核对**）、`npm run build` **exit=0** 且 `error TS` 计数 **0**。

- [ ] **Step 8: 补那条 Rust↔TS 词表一致性文本门**（`lib.rs` 的 `mod tests`，接在 `backup_export_wires_the_guard_before_any_write` 之后）

五个阶段词在三个地方各存一份（`V5` 的 CHECK、`milestone.rs` 的 `STAGES`、`src/types/milestone.ts` 的 `MILESTONE_STAGES`）。CHECK 那条 T1 已按行为钉死；剩下两份是**文本**，最容易漂，漂一个字的后果不是编译错而是「前端能选、后端当场拒」。

```rust
    /// 阶段词表三处存份，靠这条把 Rust 与前端钉在一起（V5 的 CHECK 由
    /// `v5_milestone_stage_check_refuses_words_outside_the_five` 按行为钉）。
    /// 坑 33 说 include_str! 的 needle 要用 concat! 拼，那是针对「在同一文件里数出现次数」的写法；
    /// 这里是拿 lib.rs 的门去比**另一个文件**的字面量，数不到自己头上，所以用普通字面量。
    #[test]
    fn stage_words_are_identical_in_rust_and_the_frontend_type() {
        assert_eq!(
            crate::milestone::STAGES,
            ["立项", "评审", "联调", "上线", "验收"],
            "Rust 侧词表被改了：V5 的 CHECK 与前端常量要一起改"
        );
        let ts = include_str!("../../src/types/milestone.ts");
        assert!(
            ts.contains("立项\", \"评审\", \"联调\", \"上线\", \"验收"),
            "前端 MILESTONE_STAGES 那五个词的引号/顺序/字面与 Rust 侧不一致"
        );
    }
```

- [ ] **Step 9: Rust 门复跑 + 两条 clippy，提交**

Run: `cd src-tauri && cargo test -j 1 --lib stage_words > /tmp/t4.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t4.log; cargo clippy --lib -- -D warnings > /tmp/c1.log 2>&1; echo "c1=$?"; cargo clippy --lib --all-targets -- -D warnings > /tmp/c2.log 2>&1; echo "c2=$?"`
Expected：`exit=0`、**1 passed**；全量 **254 passed**（253 + 1；**实跑核对**）；两个 clippy `0`。

```bash
git add src/types/todo.ts src/types/milestone.ts src/lib/api.ts src/lib/due-warning.ts src/lib/todo-view.ts tests/due-warning.test.ts tests/todo-view.test.ts src-tauri/src/lib.rs
git commit -m "feat(m9): 前端契约层（todo/milestone 线类型 + 八个封装 + due-warning 与 todo-view 两个纯函数 + 13 条 node 测试）+ 五段阶段词的 Rust↔TS 文本门"
```

---

## Task 5: `/todos` 页（预警块 + 三视图 + 筛选 + CRUD）+ nav 第 5 格

**Files:**
- Create: `src/pages/todos.tsx`
- Modify: `src/components/app-shell.tsx:8-13`（`nav` 数组加第 5 格）、`src/router.tsx`（`todosRoute` + 进 `routeTree`）

**Interfaces:**
- Consumes: T4 的 `buildDueWarnings`/`localToday`/`projectLabel`、`sortTodos`/`groupByProject`、`api.ts` 的八个封装与 `listProjects`
- Produces: 路由 `/todos`（`AppShell` 的 nav 第 5 格）；无对外接口（终点格）

- [ ] **Step 1: 写页面本体**

```tsx
// 待办清单：跨项目统一管理（需求 思维导图.md:38-45）。顶部是到期预警块（spec §六）。
// 数据一次拉全（listTodos(null) / listMilestones(null) / listProjects()）：几十个项目、
// 几百条待办的量级，视图排序与预警都在前端，比按视图来回打后端简单也更省往返。
import { useCallback, useEffect, useMemo, useState } from "react";
import { Link } from "@tanstack/react-router";
import { Plus, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import {
  createTodo,
  deleteTodo,
  listMilestones,
  listProjects,
  listTodos,
  updateTodo,
} from "@/lib/api";
import { buildDueWarnings, localToday, projectLabel, type DueItem } from "@/lib/due-warning";
import { groupByProject, sortTodos, type TodoView } from "@/lib/todo-view";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { dueOrNull, type Todo } from "@/types/todo";
import type { Milestone } from "@/types/milestone";
import type { Project } from "@/types/project";

const VIEWS: { key: TodoView; label: string }[] = [
  { key: "all", label: "全部" },
  { key: "byProject", label: "按项目分组" },
  { key: "byDue", label: "按截止排序" },
];

const ALL_PROJECTS = "__all__";

function Row({
  todo,
  projectLabel,
  editing,
  draftTitle,
  draftDue,
  busy,
  onStartEdit,
  onDraft,
  onSave,
  onCancel,
  onToggle,
  onDelete,
}: {
  todo: Todo;
  projectLabel: string;
  editing: boolean;
  draftTitle: string;
  draftDue: string;
  busy: boolean;
  onStartEdit: (t: Todo) => void;
  onDraft: (key: "title" | "due", value: string) => void;
  onSave: () => void;
  onCancel: () => void;
  onToggle: (t: Todo) => void;
  onDelete: (t: Todo) => void;
}) {
  return (
    <div className="flex items-center gap-2 border-b px-1 py-2 last:border-b-0">
      <input
        type="checkbox"
        className="size-4 accent-primary"
        checked={todo.done}
        disabled={busy}
        onChange={() => onToggle(todo)}
        aria-label={todo.done ? "标记为未完成" : "标记为已完成"}
      />
      {editing ? (
        <>
          <Input
            value={draftTitle}
            onChange={(e) => onDraft("title", e.target.value)}
            className="flex-1"
            autoFocus
          />
          <Input
            type="date"
            value={draftDue}
            onChange={(e) => onDraft("due", e.target.value)}
            className="w-44"
          />
          <Button size="sm" disabled={busy || draftTitle.trim() === ""} onClick={onSave}>
            保存
          </Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={onCancel}>
            取消
          </Button>
        </>
      ) : (
        <>
          <span className={`min-w-0 flex-1 truncate text-sm ${todo.done ? "text-muted-foreground line-through" : ""}`}>
            {todo.title}
          </span>
          <Link to="/projects/$projectId" params={{ projectId: todo.projectId }} className="shrink-0">
            <Badge variant="outline">{projectLabel}</Badge>
          </Link>
          <span className="w-24 shrink-0 text-right font-mono text-xs text-muted-foreground">
            {todo.dueDate ?? "无截止"}
          </span>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => onStartEdit(todo)}>
            编辑
          </Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => onDelete(todo)}>
            <Trash2 />
          </Button>
        </>
      )}
    </div>
  );
}

export function TodosPage() {
  const [todos, setTodos] = useState<Todo[]>([]);
  const [milestones, setMilestones] = useState<Milestone[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [view, setView] = useState<TodoView>("all");
  const [filter, setFilter] = useState<string>(ALL_PROJECTS);
  const [newTitle, setNewTitle] = useState("");
  const [newDue, setNewDue] = useState("");
  const [newProject, setNewProject] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftTitle, setDraftTitle] = useState("");
  const [draftDue, setDraftDue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);

  const reload = useCallback(async () => {
    try {
      const [t, m, p] = await Promise.all([listTodos(null), listMilestones(null), listProjects()]);
      setTodos(t);
      setMilestones(m);
      setProjects(p);
      setError(null);
    } catch (e) {
      setError(toAppError(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const labelOf = useMemo(() => {
    // 项目名的显示形状只有一份（due-warning 的 projectLabel），这里不重拼一遍，免得两处漂
    const byId = new Map(projects.map((p) => [p.id, projectLabel(p)]));
    return (id: string) => byId.get(id) ?? "（项目缺失）";
  }, [projects]);

  const due = useMemo(
    () => buildDueWarnings({ todos, milestones, projects }, localToday()),
    [todos, milestones, projects],
  );

  const visible = useMemo(() => {
    const scoped = filter === ALL_PROJECTS ? todos : todos.filter((t) => t.projectId === filter);
    return view === "byProject" ? groupByProject(scoped, projects, view) : null;
  }, [todos, projects, filter, view]);

  const flat = useMemo(() => {
    const scoped = filter === ALL_PROJECTS ? todos : todos.filter((t) => t.projectId === filter);
    return sortTodos(scoped, view);
  }, [todos, filter, view]);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
      await reload();
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-4">
      <section className="rounded-xl border bg-background p-4">
        <h2 className="text-sm font-medium">到期预警</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          三处一起算：未完成的待办截止、未勾选的里程碑计划日期、未归档项目的交付截止。红=已逾期，黄=今天起 7 天内。
        </p>
        {due.unparsed > 0 && (
          <p className="mt-2 text-xs text-destructive">
            {due.unparsed} 条日期无法解析，没有参与预警（要写成 YYYY-MM-DD）
          </p>
        )}
        {due.items.length === 0 ? (
          <p className="mt-3 text-sm text-muted-foreground">眼下没有到期项。</p>
        ) : (
          <ul className="mt-3 space-y-1">
            {due.items.map((i: DueItem) => (
              <li key={`${i.kind}-${i.id}`} className="flex items-center gap-2 text-sm">
                <Badge variant={i.state === "overdue" ? "destructive" : "outline"}>
                  {i.state === "overdue" ? `逾期 ${-i.days} 天` : i.days === 0 ? "今天" : `${i.days} 天后`}
                </Badge>
                <span className="min-w-0 flex-1 truncate">
                  {i.projectLabel} · {i.label}
                </span>
                <span className="font-mono text-xs text-muted-foreground">{i.date}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      {error && (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}

      <section className="rounded-xl border bg-background p-4">
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <h2 className="text-sm font-medium">待办</h2>
          <span className="text-xs text-muted-foreground">共 {todos.length} 条未完成 {todos.filter((t) => !t.done).length}</span>
          <div className="ml-auto flex items-center gap-2">
            {VIEWS.map((v) => (
              <Button
                key={v.key}
                size="sm"
                variant={view === v.key ? "default" : "outline"}
                onClick={() => setView(v.key)}
              >
                {v.label}
              </Button>
            ))}
            <Select value={filter} onValueChange={setFilter}>
              <SelectTrigger className="w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL_PROJECTS}>全部项目</SelectItem>
                {projects.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {labelOf(p.id)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>

        <div className="mb-3 flex flex-wrap items-center gap-2">
          <Input
            value={newTitle}
            onChange={(e) => setNewTitle(e.target.value)}
            placeholder="待办内容"
            className="min-w-48 flex-1"
            onKeyDown={(e) => e.key === "Enter" && void add()}
          />
          <Input type="date" value={newDue} onChange={(e) => setNewDue(e.target.value)} className="w-44" />
          <Select value={newProject} onValueChange={setNewProject}>
            <SelectTrigger className="w-56">
              <SelectValue placeholder="选择项目" />
            </SelectTrigger>
            <SelectContent>
              {projects.map((p) => (
                <SelectItem key={p.id} value={p.id}>
                  {labelOf(p.id)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button size="sm" disabled={busy || newTitle.trim() === "" || newProject === ""} onClick={() => void add()}>
            <Plus /> 新增
          </Button>
        </div>

        <Separator className="mb-2" />

        {todos.length === 0 && <p className="py-6 text-center text-sm text-muted-foreground">还没有待办。</p>}

        {view === "byProject" && visible
          ? visible.map((g) => (
              <div key={g.projectId} className="mb-3">
                <div className="mb-1 text-xs font-medium text-muted-foreground">{g.projectLabel}</div>
                {g.todos.map((t) => row(t))}
              </div>
            ))
          : flat.map((t) => row(t))}
      </section>
    </div>
  );

  function add() {
    return run(() => createTodo({ projectId: newProject, title: newTitle.trim(), dueDate: dueOrNull(newDue) })).then(
      () => {
        setNewTitle("");
        setNewDue("");
      },
    );
  }

  function row(t: Todo) {
    const editing = editingId === t.id;
    return (
      <Row
        key={t.id}
        todo={t}
        projectLabel={labelOf(t.projectId)}
        editing={editing}
        draftTitle={editing ? draftTitle : ""}
        draftDue={editing ? draftDue : ""}
        busy={busy}
        onStartEdit={(x) => {
          setEditingId(x.id);
          setDraftTitle(x.title);
          setDraftDue(x.dueDate ?? "");
        }}
        onDraft={(k, v) => (k === "title" ? setDraftTitle(v) : setDraftDue(v))}
        onSave={() =>
          void run(() =>
            updateTodo(t.id, { title: draftTitle.trim(), dueDate: dueOrNull(draftDue), done: t.done }),
          ).then(() => setEditingId(null))
        }
        onCancel={() => setEditingId(null)}
        onToggle={(x) => void run(() => updateTodo(x.id, { title: x.title, dueDate: x.dueDate, done: !x.done }))}
        onDelete={(x) => void run(() => deleteTodo(x.id))}
      />
    );
  }
}
```

**三处形状要点，别改**：① 编辑表单里没有「所属项目」那一格 —— 后端 update 收不到 `projectId`（spec §十一），要挪就删了重建；② `dueOrNull` 把空串折成 `null`，直接把 `""` 传下去会被后端的形状判据拒；③ 勾选完成走 `updateTodo` 的 `done`（D12），不许新开命令。

- [ ] **Step 2: nav 与路由**

`src/components/app-shell.tsx` 的 `nav` 数组末尾加一行（第 5 格，UI 契约变化已在 spec D9/§4-B 记过）：

```tsx
  { to: "/todos", label: "待办" },
```

`src/router.tsx` 照 `backupRoute` 那三行加：

```tsx
import { TodosPage } from "@/pages/todos";
// …
const todosRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/todos",
  component: TodosPage,
});

const routeTree = rootRoute.addChildren([
  searchRoute,
  projectsRoute,
  projectDetailRoute,
  indexRoute,
  backupRoute,
  todosRoute,
]);
```

- [ ] **Step 3: 前端两条门**

Run: `npm run build > /tmp/nb2.log 2>&1; echo "build=$?"; grep -c "error TS" /tmp/nb2.log; node --test "tests/**/*.test.ts" > /tmp/n6.log 2>&1; echo "node=$?"; grep -E "^ℹ (tests|pass|fail)" /tmp/n6.log`
Expected：`build=0` 且 `error TS` 计数 **0**；node **25 / 25 / 0**（本格零新测试，属无回归检查）。`noUnusedLocals` 会挡住我代码里没用到的 import —— 若报 `TS6133`，**删掉没用的 import**，别加 `// eslint-disable` 或 `@ts-expect-error`。

- [ ] **Step 4: 提交**

```bash
git add src/pages/todos.tsx src/components/app-shell.tsx src/router.tsx
git commit -m "feat(m9): 待办页（预警块 + 三视图 + 项目筛选 + 新增/编辑/勾选/删除）+ nav 第 5 格与 /todos 路由；编辑表单不给改归属"
```

---

## Task 6: `milestone-panel.tsx`（项目详情的里程碑区）

**Files:**
- Create: `src/components/milestone-panel.tsx`
- Modify: `src/pages/project-detail.tsx`（import + 在 `<LedgerPanel />` 那个 wrapper `div` 之后插一个同形 wrapper）

**Interfaces:**
- Consumes: T4 的 `MILESTONE_STAGES`、`dueOrNull` 同形的 `dateOrNull`（本面板自备一个，两格逻辑一样但文件不同 —— **不要**为两行代码跨模块引用来引用过去）、`api.ts` 的里程碑四封装、`buildDueWarnings` 用的同一份 `localToday`
- Produces: `MilestonePanel({ projectId }: { projectId: string })`

- [ ] **Step 1: 写面板**（照 `ledger-panel.tsx` 的形制：面板本体只管自己的列表与表单，外框由宿主给）

```tsx
// 项目里程碑（需求 思维导图.md:33-36：立项、评审、联调、上线、验收，可记录日期、备注）。
// done 那一列是 spec D5 加的：立项/评审的计划日期天然在过去，没勾选态就会把预警刷成一屏误红。
import { useCallback, useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { createMilestone, deleteMilestone, listMilestones, updateMilestone } from "@/lib/api";
import { localToday, DUE_HORIZON_DAYS } from "@/lib/due-warning";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { MILESTONE_STAGES, type Milestone } from "@/types/milestone";

const dateOrNull = (raw: string): string | null => (raw.trim() === "" ? null : raw.trim());

function shiftDate(iso: string, days: number): string {
  const ms = Date.parse(`${iso}T00:00:00Z`) + days * 86_400_000;
  return new Date(ms).toISOString().slice(0, 10);
}

export function MilestonePanel({ projectId }: { projectId: string }) {
  const [rows, setRows] = useState<Milestone[]>([]);
  const [stage, setStage] = useState<string>(MILESTONE_STAGES[0]);
  const [planDate, setPlanDate] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);

  const reload = useCallback(async () => {
    try {
      setRows(await listMilestones(projectId));
      setError(null);
    } catch (e) {
      setError(toAppError(e));
    }
  }, [projectId]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
      await reload();
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setBusy(false);
    }
  };

  // 与预警块同一套判据、同一份 today：这里只判「这一行自己」是不是命中，不重新汇总三来源。
  const today = localToday();
  const stateOf = (m: Milestone): "overdue" | "soon" | null => {
    if (m.done || m.planDate === null) return null;
    if (m.planDate < today) return "overdue";
    if (m.planDate <= shiftDate(today, DUE_HORIZON_DAYS)) return "soon";
    return null;
  };

  return (
    <div>
      <div className="mb-1 flex items-center justify-between">
        <h2 className="text-sm font-medium">项目里程碑</h2>
        <span className="text-xs text-muted-foreground">阶段固定五段；勾掉=已完成，不再进预警</span>
      </div>

      {error && (
        <div className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}

      <div className="mb-3 flex flex-wrap items-center gap-2">
        <Select value={stage} onValueChange={setStage}>
          <SelectTrigger className="w-32">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {MILESTONE_STAGES.map((s) => (
              <SelectItem key={s} value={s}>
                {s}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Input type="date" value={planDate} onChange={(e) => setPlanDate(e.target.value)} className="w-44" />
        <Input
          value={note}
          onChange={(e) => setNote(e.target.value)}
          placeholder="备注（可留空）"
          className="min-w-40 flex-1"
        />
        <Button
          size="sm"
          disabled={busy}
          onClick={() => void run(() => createMilestone({ projectId, stage, planDate: dateOrNull(planDate), note: note.trim() === "" ? null : note.trim() }))}
        >
          <Plus /> 记一笔
        </Button>
      </div>

      {rows.length === 0 && <p className="py-4 text-center text-sm text-muted-foreground">还没有里程碑记录。</p>}

      {rows.map((m) => {
        const hit = stateOf(m);
        return (
          <div key={m.id} className="flex items-center gap-2 border-b px-1 py-2 last:border-b-0">
            <input
              type="checkbox"
              className="size-4 accent-primary"
              checked={m.done}
              disabled={busy}
              onChange={() => void run(() => updateMilestone(m.id, { stage: m.stage, planDate: m.planDate, note: m.note, done: !m.done }))}
            />
            <Badge variant={hit === "overdue" ? "destructive" : hit === "soon" ? "outline" : "secondary"}>
              {m.stage}
            </Badge>
            <span className="w-24 shrink-0 font-mono text-xs text-muted-foreground">{m.planDate ?? "无日期"}</span>
            <span className="min-w-0 flex-1 truncate text-sm">{m.note}</span>
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => void run(() => deleteMilestone(m.id))}>
              <Trash2 />
            </Button>
          </div>
        );
      })}
    </div>
  );
}
```

**这一格里 `stateOf` 与 `buildDueWarnings` 的判据是两份实现**（同一份阈值常量），代价是「改了红黄口径要改两处」。要不要合并成一处？不合并的理由：合并就得让面板去跑整份三来源汇总再按 `projectId` 过滤，而它手里只有本项目的里程碑 —— 为一个徽标把三份数据都拉齐是绕远。`DUE_HORIZON_DAYS` 与 `localToday` 已经是共享的，剩下的比较是三行；**T7 的变异取证里要把这两处各自打红一次**（见 T7 变异 M④）。

- [ ] **Step 2: 接进项目详情页**（`project-detail.tsx`，在 `<LedgerPanel projectId={project.id} />` 那个 `div` 之后、`<Dialog open={editing}` 之前）

```tsx
      <div className="mt-4 rounded-xl border bg-background p-4">
        <MilestonePanel projectId={project.id} />
      </div>
```

以及文件头 `import { MilestonePanel } from "@/components/milestone-panel";`（挨着 `import { LedgerPanel } …` 那一行）。

- [ ] **Step 3: 前端两条门 + Rust 全量（确认零回归）**

Run: `npm run build > /tmp/nb3.log 2>&1; echo "build=$?"; grep -c "error TS" /tmp/nb3.log; node --test "tests/**/*.test.ts" > /tmp/n7.log 2>&1; echo "node=$?"; cd src-tauri && cargo test -j 1 --lib > /tmp/t6.log 2>&1; echo "cargo=$?"; grep "test result" /tmp/t6.log`
Expected：`build=0` / `error TS` **0** / node **25-25-0** / cargo **254 passed / exit=0**（本格零 Rust 变动）。

- [ ] **Step 4: 提交**

```bash
git add src/components/milestone-panel.tsx src/pages/project-detail.tsx
git commit -m "feat(m9): 项目详情的里程碑区（五段阶段 + 计划日期 + 备注 + done 勾选，命中红黄的行按预警同一判据标色）"
```

---

## Task 7: 全量门禁 + 六处变异取证 + 真机十格（**控制方执行，子代理禁跑 dev**）

**Files:**
- Create: `.superpowers/sdd/2026-10-14-m9-todos-milestones/m9-cdp.mjs`（真机断言脚本，照同目录 `m8-cdp.mjs` 的形制：`node m9-cdp.mjs s1|s2|…` 分段可重跑）
- Create: `.superpowers/sdd/2026-10-14-m9-todos-milestones/mutation-evidence.txt`（六处变异的红/绿读数，逐条留原文）

**Interfaces:**
- Consumes: T1–T6 的终态树
- Produces: 门禁读数与真机读数，作为 T8 文档回写的**唯一**取材来源

- [ ] **Step 1: 全量门禁（终态树，退出码全要真的）**

```
cd src-tauri && cargo test -j 1 --lib > /tmp/g-test.log 2>&1; echo "test=$?"; grep "test result" /tmp/g-test.log
cargo clippy --lib -- -D warnings > /tmp/g-c1.log 2>&1; echo "c1=$?"; grep -c "warning" /tmp/g-c1.log
cargo clippy --lib --all-targets -- -D warnings > /tmp/g-c2.log 2>&1; echo "c2=$?"; grep -c "warning" /tmp/g-c2.log
cargo build --lib > /tmp/g-build.log 2>&1; echo "build=$?"
cd .. && node --test "tests/**/*.test.ts" > /tmp/g-node.log 2>&1; echo "node=$?"; grep -E "^ℹ (tests|pass|fail)" /tmp/g-node.log
npm run build > /tmp/g-npm.log 2>&1; echo "npm=$?"; grep -c "error TS" /tmp/g-npm.log
sed -n '/generate_handler/,/^      \]/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"      # 期望 53
grep -c 'tauri::command(async)' src-tauri/src/lib.rs                                          # 期望 4
grep -m1 'TARGET_VERSION' src-tauri/src/db.rs                                                  # 期望 i64 = 5
grep -rn "allow(dead_code)" src-tauri/src/ | wc -l                                             # 期望 1（只有 db.rs:30）
git diff --stat master..HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock                      # 期望空输出（M9 零依赖变动）
grep -c '^\[\[package\]\]' src-tauri/Cargo.lock                                                # 期望 616
```

Expected（**全部现跑核对，下面这串只是本计划按逐格列名推出的数**）：`test=0` + **254 passed / 0 failed**（225 基线 + T1 7 + T2 12 + T3 9 + T4 1）；两个 clippy `exit=0` 且 0 warning；`build=0`；node **25 / 25 / 0**；`npm=0` 且 `error TS` **0**；结构门 **53 / 4 / 5 / 1 / Cargo.lock 零字节变化 / 616**。

- [ ] **Step 2: 六处变异取证（每处都是「把被守护的那段摘掉/改错，看它当场红」，跑完 `cp` 还原 + `cmp` 验字节一致 + 复跑全量）**

| # | 靶点 | 怎么改 | 该红在哪 |
|---|---|---|---|
| M① | `todo.rs` 的 `list` | 删掉 `AND p.deleted_at IS NULL` | `cross_project_list_hides_todos_of_a_soft_deleted_project` |
| M①′ | `milestone.rs` 的 `list` | 同一处 | `cross_project_list_hides_milestones_of_a_soft_deleted_project`（**两处各打一次**：D8 的防线在两张表上各有一处落点，只打一张等于另一张没测） |
| M② | `todo.rs` 的 `is_iso_date` | 整个函数体改成 `true`（保留签名） | `create_rejects_a_non_iso_due_date` + `norm_date_trims…` 那一支 + T3 的 `create_rejects_non_iso_plan_date` |
| M③ | `due-warning.ts` 的两个 `if (t.done) continue;` / `if (m.done) continue;` | 改成 `if (false)` 形式（**保留行，只废判据**） | node 的「勾掉的待办不算」「勾掉的里程碑不算」两条 |
| M④ | `due-warning.ts` 的 `state: days < 0 ? "overdue" : "soon"` | 改成 `days <= 0` | node 的「今天到期 ⇒ soon」 |
| M⑤ | `due-warning.ts` 的 `if (p.status === "归档") continue;` | 删掉 | node 的「归档项目的交付截止不进预警」 |
| M⑥ | `milestone-panel.tsx` 的 `stateOf`：`if (m.planDate < today) return "overdue"` | 改成 `m.planDate <= today` | **无自动化测试会红**（面板层没有 DOM 测试网）⇒ 这一条只能靠真机第 4 格的人眼读数兜，按 deferred minor 记进 T8 的「没能自动验证」，不许当作已证 |

**取证口径**（照 M8/M8.1 的形制）：每一处跑完把命令、`test result` 行、红的测试名与断言原文（含 `file:line`）追加进 `mutation-evidence.txt`；还原用 `cp` 备份 + `cmp` 验字节一致，**不要**用 `git checkout --`（同 worktree 可能有别的未提交内容）；六处跑完复跑一次全量，读数必须回到 Step 1 的那串数。

- [ ] **Step 3: 真机十格（沙盒 `dev.zero.pfm.m9test`，一次性 identifier；真实目录只读）**

编译前两个必然失败的探针先走一遍（M5 起的红线：证 `npm run tauri -- dev -c '{"identifier":"…"}'` 的覆盖通道真走得通），再起正式沙盒：

```
# 开工前：真实库三文件基线
powershell -NoProfile -Command "$d=\"$env:APPDATA\\dev.zero.pfm\"; Get-ChildItem $d -Filter 'ledger.db*' | ForEach-Object { \"{0}`t{1}`t{2}`t{3}\" -f $_.Name,$_.Length,$_.LastWriteTime,(Get-FileHash $_.FullName -Algorithm SHA256).Hash.Substring(0,12) }"
# 起沙盒 dev（带 CDP 9222），跑完按 PID 精确回收，vite 孤儿占 1520 按坑 28 处理
```

十格逐条取读数（spec §十 的十条，一条也别省；本轮**零原生对话框**，所以第 2–8 格都能 CDP 驱动）：

1. 真实库三文件 `size + mtime + sha256` 前后逐字一致，**且没有任何新增文件**。
2. 建两个项目、各两条待办（一条未来 3 天、一条昨天）、项目详情各加两条里程碑（「上线」+3 天、「立项」过去日期）。
3. `/todos` 预警块：黄含「上线」与那条 +3 天待办，红含昨天那条待办；**过去日期的「立项」不亮红**（D5 的兑现点）。
4. 勾掉「立项」⇒ 预警不出现；勾掉「上线」⇒ 黄项当场消失；勾掉昨天的待办 ⇒ 红项当场消失。
5. 三视图切换各有读数（全部 / 按项目分组 / 按截止排序），`无截止` 的行在「按截止排序」下落最后。
6. 筛选某一项目 ⇒ 预警块与列表同步只剩该项目。
7. 删一条待办 ⇒ 列表与预警同时消失。
8. 软删整个项目 ⇒ 它的待办与里程碑从预警与跨项目列表消失，**但库里行还在**（沙盒库直查 `SELECT COUNT(*) FROM todos WHERE project_id=…` 非 0，D8 的兑现点）。
9. 用 `__TAURI_INTERNALS__.invoke` 绕开 `<input type="date">` 直接把交付日期填成点分隔那种形状 → 界面念出 `invalid_input` 那句 hint（后端判据在真链上走通）。
10. 底栏 `schema=5`、`journal_mode=wal`；`/todos` 在 nav 是第 5 格且 WebView2 真渲染出文字。

**要如实记为「没验到」的**（不许假称通过，坑 11）：M⑥ 那条面板判据只能靠第 4 格的人眼；`unparsed` 那句界面文案在真实库里有没有非 ISO 的交付截止可触发，取决于沙盒数据，取不到就写「未触发」。

- [ ] **Step 4: 提交（含取证产物）**

```bash
git add .superpowers/sdd/2026-10-14-m9-todos-milestones/m9-cdp.mjs .superpowers/sdd/2026-10-14-m9-todos-milestones/mutation-evidence.txt
git commit -m "test(m9): 六处变异取证与真机十格读数（M⑥ 那条面板判据无自动化网兜、如实记 deferred；cp/cmp 验字节还原后复跑全量）"
```

注：`.superpowers/` 被它自己的 `.gitignore` 挡住（M8 那轮已确认没进库）。**若 `git add` 报 ignored，就按本仓先例把证据留在工作区并在文档里按路径引用**，不要为了提交去改 `.gitignore`（M8 的历史账：`.superpowers/sdd/**` 一直被 ignore，`docs/` 里按路径引用它们）。

---

## Task 8: 活文档回写（按 spec §十三 逐条）

**Files:**
- Modify: `docs/技术方案.md`（§四 数据模型那两行、§九 里程碑表的 M9 与 M11 两行、§6.2 备份口径）
- Modify: `docs/开发进度.md`（里程碑表加 M9 行 + 新增「## M9 验收证据」节 + 「没能自动验证」清单 + 变更日志）
- Modify: `docs/HANDOFF.md`（§0/§2/§3/§4/§6 + 新增一条坑）

- [ ] **Step 1: `docs/技术方案.md` 三处**

行号一律现取，别照抄（坑 6）：`grep -n "milestones\|待办\|到期预警\|## 六\|### 6.2" docs/技术方案.md`。

1. **§四 数据模型**：`milestones 里程碑：阶段、日期、备注` 那行后面标注已实现（五段 CHECK + `done` + 软删三件套），并补一行 `todos 待办：项目、内容、截止日期、done（勾选完成）`。两处都要写明**日期列是 TEXT 的 `YYYY-MM-DD`，形状由写入侧判、SQL 不解释日期**（D1/D3）。
2. **§九 里程碑表**：M9 那格从「待办清单」四个字改成实际交付口径（**待办 + 里程碑 + 到期预警三件**）；**M11 那行必须点名缩量**：「到期预警已在 M9 提前兑现，但只盯里程碑/待办/交付截止三类日期；原文括号里的**维保、域名、证书**三类到期字段仍未建」。这条是本轮最大的风险（spec §十二 风险 1），漏写就等于下个里程碑以为已兑。
3. **§6.2 备份口径**：补一句「M9 新增的 `todos`/`milestones` 随整库快照进包，但**不进 `counts` 摘要**（7 字段线格式未变，理由见 M9 spec D9）；`warn_plaintext_columns()` 那句已把两档新明文列念进去」。

- [ ] **Step 2: `docs/开发进度.md`**

- 里程碑表加 M9 行（状态 + 证据列：254 / 25 / 结构门 53-4-5-1 / Cargo.lock 零变化 / 真机十格读数 / 六处变异）。
- 新增「## M9 验收证据（待办清单 + 里程碑 + 到期预警，✅ 日期现取）」整节：门禁终态、六处变异取证逐条（含 M⑥ 那条**没有自动化测试会红**的读数）、真机十格读数、结构门现取读数。
- 「没能自动验证」清单逐条点名：M⑥（面板 `stateOf` 的今日边界只有人眼）、`unparsed` 那句文案是否真被触发、以及任何本轮起不来 `tauri dev` 的格（**先试一次再判死**，坑 20）。
- 变更日志加一条（带实跑读数，不许只写计划）。

- [ ] **Step 3: `docs/HANDOFF.md`**

- §0 状态三条 + 当前分支/HEAD 现取；§2 加 M9 段落（关键落点与四处「回代码核对后翻转」的裁定）。
- §3 卡点改成「M9 已交付 + 等合并批准」的形状；§4-A 那串门禁命令的期望数从 225/45/4 **改成本轮终态**（254 / 25 / 53 / 4 / 5 / 1 / 616），并把 §4-B 换成「M10（日报 + Excel）」的勘察起点。
- **新增一条坑**（编号按本轮 `grep -n "^3[0-9]\." docs/HANDOFF.md` 现取的最大号往后接，别写死）：内容 = **判据搬前端 ⇒ `cargo test` 全绿不再守住它**（D1 的同族后果，取证要换到 `node --test`）；并带上本轮新踩的显示层那条：承重字面量（日期形状、分隔符、URL 编码）落盘后要按码点回验，工具显示可能骗人。

- [ ] **Step 4: 收口门禁 + 提交 + 推功能分支**

Step 1 那串命令整串重跑一遍（文档改动不该动读数，但**不许推断，要复跑**），然后：

```bash
git add docs/技术方案.md docs/开发进度.md docs/HANDOFF.md
git commit -m "docs(m9): 收口回写 —— 技术方案数据模型与里程碑表（M11 点名到期预警只兑一半）、开发进度 M9 证据节与没能自动验证清单、HANDOFF 下一格转 M10 + 门禁期望数换成 254/53/5"
git push origin feat/m9-todos-milestones
git ls-remote --heads origin | grep m9      # 远端 SHA 与本地逐字一致
```

**合 `master` 需需求方当次批准，不许自作**（仓内惯例：常驻推送授权只覆盖功能分支）。

---

## 有名缺口清单（写进 T8 的「没能自动验证」，逐条点名，不许漏）

1. **`milestone-panel.tsx` 的 `stateOf` 无自动化测试**（T7 变异 M⑥）：它与 `due-warning.ts` 是两份实现，面板那一处改成 `<=` 没有任何测试会红。⇒ 只能靠真机第 4 格的人眼；若那格也没点到，就记「deferred minor」，**不许因为「有 node 那 13 条」就把它算成已守**。
2. **`unparsed` 那句界面文案可能一次也没被触发**：沙盒数据里没有非 ISO 的交付截止时，这一格是「逻辑对了但没在真机见过」。
3. **命令接线层无端到端**：本仓不开 `tauri::test` 就驱动不了 IPC 的合成往返（同 M8.1 的账）⇒ 八条命令的「注册 + 取锁 + 传参」靠结构门计数 + 文本门 + 真机第 9 格兜，**没有一条自动化测试证明 `todo_list` 的 `projectId=null` 真的走了「全部」那支**（真机第 6 格的筛选用的是非 null 那支）。
4. **历法无效的日期值**（`2026-13-45`）会被形状判据收下，前端 `Date.UTC` 把它折成 2027-01 附近的真实日子 ⇒ 那条待办会在错误的日子亮黄。spec §十二 风险 5 已记为不做；这里是把它如实转登为「有形状门、无历法门」。
5. **`Counts` 不含新表** ⇒ `/backup` 界面看不出包里有没有待办/里程碑（D9 的当次取舍，不是漏做）。要验只能导一份包、`backup_inspect` 之后直查包内 `backup.db` 的两张表行数。
6. **归档项目跳过预警这一判据没有需求原文支撑**（判据⑤ 是我定的，需求方在设计 2/2 那一格看过并默认）：若以后要「归档了也照样催事」，改这一条 `continue` 即可，但它同时是 T7 变异 M⑤ 的靶子。
7. **计划文本本身的测试期望数（254 / 25）是推出的**：每格派发前必须按实跑订正（`verify-plan-test-count-expectations`）；实现者回报的数与本计划不符时**以实跑为准**并回来订正本文件，别让下游格接着累加。






