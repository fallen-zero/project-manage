# M10 日报管理 + 工时统计 + Excel 双向导入导出 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给软件加一格「日报管理」：按项目按天登记工时条目，页面按日/周/月与按项目聚合，并能把日报导成**真 `.xlsx`**、从填好的 `.xlsx` **两段式（预检 → 事务）批量导回**。

**Architecture:** 后端一张 `daily_reports` 表 + 一个 `daily_report.rs` 模块（四操作 + Excel 读写两个私有函数），日期与工时的判据**先于 SQL**；聚合与筛选全在前端纯函数 `hours-summary.ts`，沿用 M9 D7 的「SQL 不解释日期」。全局检索的台账侧从六条同形状 per-table LIKE 变八条（`daily` + `todo`），不碰 FTS、不碰备份。

**Tech Stack:** Tauri 2 + Rust（`rusqlite` bundled、`calamine` 读 xlsx、`rust_xlsxwriter` 写 xlsx）、React 19 + TS + Tailwind 4 + TanStack Router（hash history）+ zustand + shadcn/ui；`node --test` 跑前端纯函数。

**Spec:** `docs/superpowers/specs/2026-10-16-m10-daily-reports-design.md`（D1–D10、§三 六格免费能力、§九 十条不变量裁定、§十 十四格验收、§十二 八条变异都在里面；本计划按 spec 论证，不重述理由）

---

## Global Constraints

每一格的要求都隐含这一节。

- **零新依赖**：唯一允许的依赖动作是把 `rust_xlsxwriter = "0.99.1"` 从 `[dev-dependencies]` 提到 `[dependencies]`（Task 6）。**不许**新增 `chrono`、`regex`、`umya-spreadsheet`、jsdom 或任何包。`grep -c '^\[\[package\]\]' src-tauri/Cargo.lock` 本轮全程必须是 **616**。
- **不许用 `calamine` 的 chrono 边界**：取年月日只能 `ExcelDateTime::to_ymd_hms_milli()`（**方法名结尾是 `milli`，没有 `s`**）。`datatype.rs:791` 那个 `as_datetime() -> Option<chrono::NaiveDateTime>` 一进我们的代码面就把 chrono 拖进来了，属违规。
- **一条判据只写一处**：日期形状只有 `todo::is_iso_date`；日期必填只有 `todo::req_date`；xlsx 表头只有 `daily_report::EXPORT_HEADERS` 一个常量（导出写它、导入按它定位列）；筛选与聚合只有前端 `hours-summary.ts` 一处。出现第二份就是缺陷（M9 终审 I2 的账）。
- **一个新错误码都不开**，本轮用到五个既有码（spec §八 有表）：`invalid_input`、`not_found`、`backup_destination_unsafe`（复用守卫）、`fs_failed`（`error.rs:21` 的 `AppError::io(path, e)`，写文件失败）、`extract_failed`（打开表格失败，与 `extract.rs:179` 同码同文案）。
- **前端纯函数的测试形状**：只在仓库根跑 `node --test "tests/**/*.test.ts"`（**在 `src-tauri` 下跑会返回 `tests 0`**）。纯函数模块里 value import 必须走相对路径带 `.ts` 后缀（先例 `todo-view.ts:8`），`@/` 别名只允许出现在 `import type` 里。
- **注释里禁止出现这两串字面量原形**：`tauri::command(async)`、`allow(dead_code)`。两道门是 `grep -c`，注释里的字面量会把计数撑破（M9 真踩过一次）。要提到就写成「async 属性那条」「dead_code 豁免」这种人话。
- **结构门数字一律现跑取数**：`generate_handler!` 本轮 53→57→58→60、`tauri::command(async)` 4→7、`TARGET_VERSION` 5→6。别把任何数写死进脚本或文档（HANDOFF 坑 21）。
- **命令注册表的形状**：`generate_handler![...]` 末项**不带逗号**，追加时给上一行补逗号、自己那行不带（M9 踩过）。取数口径 `sed -n '/generate_handler/,/^      \]/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"`。
- **本机 shell 风险**：cargo/clippy 前要**绝对路径** `cd /e/zero/demo/2026/project-files-manage/src-tauri`（cwd 在多次调用之间残留，`cd: No such file` 会让 `exit=1` 变成 `cd` 的退出码）。仓库源文件是 **CRLF**，脚本读写都要 `newline=''`。输出层会把 `/` 印成 `-`：日期字面量、路径分隔一律按码点复验；含反引号或中文的正文**走 UTF-8 脚本文件再执行**，不许 `python -c`/heredoc（M9 因此把一行变更日志掏空并推了上去）。
- **红线**：真实数据目录 `%APPDATA%\dev.zero.pfm\` 只读；真机用一次性 identifier 沙盒并跑完删净；**子代理一律不跑 `tauri dev` / `npm run dev`**；未跟踪的 `Python/` 绝不 stage、绝不删；清理垃圾禁用通配符删除。
- **输出语言**：对话、进度、commit message 一律中文（仓库历史全英文也照写中文），错误码与 SQL 字面量除外。

---

## 任务总览与派发前必读

| # | 任务 | 交付 | 新增自动化 |
|---|---|---|---|
| 1 | V6 迁移 | `daily_reports` 表 + `TARGET_VERSION` 6 + 零回填断言 | cargo +3 |
| 2 | 判据与四操作 | `todo::req_date` + `daily_report.rs` + 4 条同步命令 | cargo +18 |
| 3 | 全局检索接入 | 台账段六条 collect → 八条 + `SOURCE_LABELS` 两项 | cargo +2 |
| 4 | 前端契约与聚合 | `types/daily-report.ts` + 4 个 api 包装 + `hours-summary.ts` | node +10 |
| 5 | 日报页 | `/daily-reports` 页面 + nav 第 6 格 + 路由 | 0（靠真机） |
| 6 | Excel 导出 | `write_xlsx` + `daily_report_export`（async）+ 落点守卫 + 页面按钮 | cargo +4，async 门 4→5 |
| 7 | Excel 导入 | `read_rows` + preview/apply（async）+ 两段式 UI | cargo +18，async 门 5→7 |
| 8 | 门禁与取证 | 全量门 + 八条变异 + 真机十六格（spec 十四格 + 两格人手 Excel） | 0 |
| 9 | 活文档回写 | 技术方案 / 开发进度 / HANDOFF / M9 spec 反转 | 0 |

**基线读数（2026-10-16 在 `feat/m10-daily-reports` = spec 提交 `5386d2b` 上现跑）**：`cargo test -j 1 --lib` **256 passed / 0 failed**，`node --test "tests/**/*.test.ts"` **tests 31 / pass 31 / fail 0**。按本文实际写出的测试条数累加，本轮终态应为 **cargo 301 / node 41**（cargo：+3 +18 +2 +4 +18；node：+10）。这三个数是用脚本从本文的 `#[test]` / `test(` 块实数点出来的（控制方 2026-10-16 落盘后复点，Task 7 原写 16、实数 17 已改准），不是估的；派发每一格时仍要用现跑读数对账 —— 同一格若被别人改过，聚合数会整条漂（HANDOFF 坑 21）。

**派发顺序不是纯线性的**：T3 依赖 T2 的表；T5 依赖 T4；T6/T7 依赖 T2 的表与 T5 的页面存在（按钮挂在页上）。T1→T2→T3→T4→T5→T6→T7 单线走最稳，同一 worktree **只允许一个会提交的实现者**。

**每格收尾三件**：受影响门禁现跑（绝对路径 cd）、`git status` 看过再 stage（`Python/` 不许进去）、按本文 commit 步的原样提交。

---

### Task 1: V6 迁移 —— `daily_reports` 表与零回填

**Files:**
- Modify: `src-tauri/src/db.rs:98`（`TARGET_VERSION`）、`src-tauri/src/db.rs:118-125`（`match target`）、`src-tauri/src/db.rs:365` 之后（追加 `const V6`）
- Modify: `src-tauri/src/db.rs:634-641`（把 `v5_schema_meta_reaches_target_version_five` 改成六）
- Test: 同一文件的 `mod tests`

**Interfaces:**
- Consumes: 无（本轮第一格）
- Produces: 表 `daily_reports(id TEXT PK, project_id TEXT NOT NULL, report_date TEXT NOT NULL, content TEXT NOT NULL, hours REAL NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, deleted_at TEXT NULL)` + 索引 `ix_daily_report_project`；`TARGET_VERSION = 6`。Task 2 起的每一格都假定这两样存在。

- [ ] **Step 1: 改版本号并把 `V6` 挂进迁移链**

`src-tauri/src/db.rs:98`：

```rust
pub(crate) const TARGET_VERSION: i64 = 6;
```

`db.rs:118-125` 的 `match target` 里，在 `5 => V5,` 之后插一行（`_ => "",` 保持原位）：

```rust
            5 => V5,
            6 => V6,
            _ => "",
```

- [ ] **Step 2: 写 V6 的 DDL**

紧跟在 `const V5` 的收尾 `";`（`db.rs:365`）之后追加。**照 V5 上面那段注释的三条口径继续写**（`done`/CHECK 那两条这里用不上，`report_date` 那条用得上）：

```rust
/// V6 = 日报表（M10 spec §五.1）。三条口径延续 V5 的注释：
/// 1. 软删沿用 `deleted_at`；`ON DELETE CASCADE` 因项目是软删（`project.rs` 的 UPDATE）而永不触发，
///    所以「跨项目读要自带 p.deleted_at IS NULL」这条规矩照旧适用于本表。
/// 2. `hours` 用 REAL 且**不加 CHECK**：表由本程序唯一写入，`0<h<=24` 与两位小数的判据在 Rust 侧
///    （`daily_report::check_hours`）。CHECK 违例冒出来是 `db_failed`，会把「校验失败」伪装成库故障。
/// 3. `report_date` 的形状与必填**只由写入侧保证**（`todo::req_date`）：SQL 里不做任何日期解释，
///    `strftime`/`date` 是 UTC，用它比较会把本机日期算错（M9 D7 同一个理由，spec D9 沿用）。
const V6: &str = "
CREATE TABLE daily_reports (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    report_date TEXT NOT NULL,
    content     TEXT NOT NULL,
    hours       REAL NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at  TEXT
);

CREATE INDEX ix_daily_report_project ON daily_reports(project_id);
";
```

- [ ] **Step 3: 跑测试确认「旧断言」红**

Run: `cd /e/zero/demo/2026/project-files-manage/src-tauri && cargo test -j 1 --lib v5_schema_meta_reaches_target_version`
Expected: FAIL —— `v5_schema_meta_reaches_target_version_five` 里两处 `assert_eq!(.., 5)` 现在拿到 6。这一格红是**预期的**，它证明迁移链真的跑到了新链头；把它改成断 6 而不是删掉。

- [ ] **Step 4: 改那条版本断言**

`db.rs:634-641` 整条换成：

```rust
    #[test]
    fn v6_schema_meta_reaches_target_version_six() {
        let conn = open_in_memory().unwrap();
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 6, "迁移必须跑到 6");
        assert_eq!(TARGET_VERSION, 6, "= 上一条，两处别各写各的");
    }
```

- [ ] **Step 5: 写 V6 的三条新断言**

追加到 `mod tests` 末尾（`v5_milestone_stage_check_refuses_words_outside_the_five` 那一族之后即可）。`seed_project`（`db.rs:377`）就在同一个 tests 模块里，直接用：

```rust
    /// 列集合与顺序一起钉：顺序漂了 `SELECT_COLUMNS`（Task 2）会静默错位读，
    /// 而改名会同时打掉 `src/types/daily-report.ts` 与 xlsx 表头两处。
    #[test]
    fn v6_creates_daily_reports_with_documented_columns() {
        let conn = db::open_in_memory().unwrap();
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('daily_reports') ORDER BY cid")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(
            cols.join(","),
            "id,project_id,report_date,content,hours,created_at,updated_at,deleted_at",
            "V6 的列集合/顺序变了要同步 daily_report.rs 的 SELECT_COLUMNS 与 xlsx 表头"
        );
    }

    /// 这一格钉的是**为什么必填日期必须走 `req_date` 而不是 `norm_date`**（spec §五.2）：
    /// 缺列/NULL 走到 SQL 才有约束来挡，而那一挡冒出来是 SQLite 的 NOT NULL，不是我们的 invalid_input。
    #[test]
    fn v6_report_date_not_null_is_enforced_at_sqlite_layer() {
        let conn = db::open_in_memory().unwrap();
        seed_project(&conn, "p1");
        let err = conn
            .execute(
                "INSERT INTO daily_reports (id, project_id, content, hours) VALUES ('d1','p1','甲',1.0)",
                [],
            )
            .unwrap_err();
        assert!(
            format!("{err}").contains("NOT NULL"),
            "预期是 NOT NULL 挡住（而不是 CHECK 或外键），实际：{err}"
        );
    }

    /// 本轮新立的规矩：**V6 零回填**（延续 M9 为 V5 立的那条）。迁移里不许塞业务行。
    #[test]
    fn v6_backfills_nothing() {
        let conn = db::open_in_memory().unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "新建库跑完迁移后本表必须是空的");
    }
```

- [ ] **Step 6: 跑全量 Rust 测试**

Run: `cd /e/zero/demo/2026/project-files-manage/src-tauri && cargo test -j 1 --lib`
Expected: `259 passed; 0 failed`（基线 256，本格 +3）。**若别的测试因版本变成 6 而红**（例如备份验包拿 `TARGET_VERSION` 比包内版本的那条），按现跑读数核对：断言里写死 `5` 的只有 Step 4 那条（全文 `grep -n 'assert_eq!(.*TARGET_VERSION' src-tauri/src/db.rs` 与 `grep -n ', 5, ' src-tauri/src/backup.rs` 各跑一遍确认），别顺手放宽成 `>=`。

- [ ] **Step 7: 结构门现跑**

```bash
cd /e/zero/demo/2026/project-files-manage
grep -rn "TARGET_VERSION: i64" src-tauri/src/db.rs                 # 期望 6
grep -n "6 => V6" src-tauri/src/db.rs                              # 期望命中一行
grep -c '^\[\[package\]\]' src-tauri/Cargo.lock                    # 期望仍是 616（本格零依赖动作）
sed -n '/generate_handler/,/^      \]/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"   # 期望仍 53
```

- [ ] **Step 8: 提交**

```bash
cd /e/zero/demo/2026/project-files-manage
git add src-tauri/src/db.rs
git commit -m "feat(m10): V6 建 daily_reports 表 + TARGET_VERSION 5→6 + 零回填断言 —— hours 不加 CHECK（Rust 侧判据，CHECK 违例会伪装成 db_failed）、report_date 的必填与形状只由写入侧保证（SQL 不解释日期，沿用 M9 D7）；版本断言从断 5 改成断 6 并把三条列形状钉住"
```

---

### Task 2: `daily_report.rs` 判据与四操作 + 四条同步命令

**Files:**
- Modify: `src-tauri/src/todo.rs:56` 之后（加 `req_date`）、`src-tauri/src/todo.rs` 的 `mod tests`（加 2 条）
- Create: `src-tauri/src/daily_report.rs`
- Modify: `src-tauri/src/lib.rs:1-2`（`mod daily_report;` 插在 `mod backup;` 与 `mod db;` 之间）、`lib.rs:27`（`use crate::daily_report::{...}` 插在 `use crate::error::{AppError, AppResult};` **之前**，按模块路径字母序）、`lib.rs:747` 之后（四条命令）、`lib.rs:855` 附近（注册表末项 `milestone_delete` 补逗号 + 追加四条）、`lib.rs:690-691`（把那句假话改准，Step 6）
- Test: `src-tauri/src/daily_report.rs` 的 `mod tests`（16 条）

**Interfaces:**
- Consumes: Task 1 的 `daily_reports` 表；`todo::is_iso_date`（不直接用，经 `req_date`）。
- Produces:
  - `pub(crate) fn todo::req_date(column: &str, value: Option<String>) -> AppResult<String>`
  - `pub fn daily_report::list(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<DailyReport>>`
  - `pub fn daily_report::get(conn, id) -> AppResult<DailyReport>`、`create(conn, &DailyReportInput) -> AppResult<DailyReport>`、`update(conn, id, &DailyReportEdit) -> AppResult<()>`、`delete(conn, id) -> AppResult<()>`
  - `pub(crate) const daily_report::MAX_HOURS_PER_ROW: f64 = 24.0`、`pub(crate) fn daily_report::check_hours(h: f64) -> AppResult<f64>`
  - 线格式 `DailyReport` **七键**：`id, projectId, reportDate, content, hours, createdAt, updatedAt`；`DailyReportInput{projectId, reportDate(可缺), content, hours}`；`DailyReportEdit{projectId, reportDate, content, hours}`（**含 projectId**，与 M9 的 `TodoEdit` 不同）。
  - 四条命令名：`daily_report_list / daily_report_create / daily_report_update / daily_report_delete`（注册表 53→57）。

- [ ] **Step 1: 给 `todo.rs` 加 `req_date`（先跑 RED）**

先只加 2 条测试（放 `norm_date_refuses_a_non_iso_value_naming_the_column` 之后）：

```rust
    /// M10 spec §五.2：必填日期列不能走 `norm_date` —— 它把空白折成 NULL，NULL 进到 NOT NULL 列
    /// 会由 SQLite 冒成 db_failed，用户看到的是「数据库操作失败」而不是「你的日期没填」。
    #[test]
    fn req_date_refuses_blank_and_absent_naming_the_column() {
        for v in [None, Some("".to_owned()), Some("   ".to_owned())] {
            let err = req_date("日报日期", v.clone()).unwrap_err();
            assert_eq!(err.code, "invalid_input", "{v:?} 要在进 SQL 之前就被拒");
            assert!(err.message.contains("日报日期"), "错误文本要点名是哪一列：{}", err.message);
            assert_eq!(err.hint.as_deref(), Some("日期要用 YYYY-MM-DD"), "hint 与 norm_date 同句（D7）");
        }
    }

    /// 形状那两支与 `norm_date` 共用同一条规则（`is_iso_date`），**不许在这里再写一份判据**。
    #[test]
    fn req_date_accepts_the_shape_and_refuses_everything_else() {
        assert_eq!(req_date("日报日期", Some("2026-06-30".to_owned())).unwrap(), "2026-06-30");
        assert_eq!(req_date("日报日期", Some("  2026-06-30 ".to_owned())).unwrap(), "2026-06-30", "要 trim");
        let err = req_date("日报日期", Some("2026-6-30".to_owned())).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("日报日期"), "{}", err.message);
    }
```

Run: `cd /e/zero/demo/2026/project-files-manage/src-tauri && cargo test -j 1 --lib req_date_`
Expected: 编译失败 —— `cannot find function req_date in this scope`。这一格 RED 落在**编译层**（本仓模块不是孤儿文件，孤儿 `.rs` 照样编得过，所以不能用「文件不存在」当 RED；先例见 M9 T2/T3 的同一处更正）。

- [ ] **Step 2: 实现 `req_date`**

`todo.rs:56`（`norm_date` 收尾的 `}`）之后插入：

```rust
/// **必填**日期列专用（M10 spec §五.2）。与 `norm_date` 分家的唯一理由是空白那一支：
/// `norm_date` 折成 `NULL`（可选列的正确行为），本函数把空白与缺键都当成用户错误响亮拒掉。
/// 形状规则仍然只有一处（`is_iso_date`），这里只多一道空白拒绝。
/// 入参收 `Option<String>`：JSON 缺键 ⇒ `None` ⇒ 与空白同一句点名错误，
/// 而不是把 serde 那句线格式错抛给用户。
pub(crate) fn req_date(column: &str, value: Option<String>) -> AppResult<String> {
    match norm(value) {
        None => Err(AppError::new(
            "invalid_input",
            &format!("「{column}」不能为空，要写成 YYYY-MM-DD（如 2026-06-30）"),
            Some("日期要用 YYYY-MM-DD"),
        )),
        Some(s) if is_iso_date(&s) => Ok(s),
        Some(s) => Err(AppError::new(
            "invalid_input",
            &format!("「{column}」要写成 YYYY-MM-DD（如 2026-06-30），收到「{s}」"),
            Some("日期要用 YYYY-MM-DD"),
        )),
    }
}
```

Run: `cd /e/zero/demo/2026/project-files-manage/src-tauri && cargo test -j 1 --lib req_date_`
Expected: `2 passed; 0 failed`

- [ ] **Step 3: 先声明模块，再落 16 条测试（RED 落在编译层）**

顺序是：先把 `lib.rs:1-2` 之间插 `mod daily_report;`（字母序在 `backup` 后、`db` 前，Step 6 只补 `use` 与四条命令），**然后**新建 `src-tauri/src/daily_report.rs`，本步只写文件头的 `use` 那几行与末尾的 `#[cfg(test)] mod tests { ... }`（Step 5 那批），实现函数一支都不写。

Run: `cd /e/zero/demo/2026/project-files-manage/src-tauri && cargo test -j 1 --lib daily_report`
Expected: **编译失败**（`E0425/E0433: cannot find function create/list/get/update/delete in this scope`、`cannot find type DailyReport`）。这就是本格的 RED。

**为什么不许拿「先建孤儿文件、跑测试说没报错」当 RED**：M9 实测过 —— 没在 `lib.rs` 里 `mod` 声明的 `.rs` **根本不进编译图**，`cargo test` 以 exit=0 跑完，那种「红」是探针没接上，不是判据红。所以 Step 3 第一件事是声明模块。

> **本任务的执行顺序是 Step 3 → Step 4 → 回跑 Step 3 的命令看红 → Step 5 → Step 6**。下面两块代码的**物理排布是实现块在前、测试块在后**，但标题上的序号就是顺序：先粘 Step 4 那块（文件头 `use` + 16 条测试）取编译层 RED，再粘 Step 5 那块补实现。别照着自上而下的排布把实现先落进去 —— 那样 RED 这一格根本没取证，本仓不收这种交付。

- [ ] **Step 5: 补实现（RED 取完之后才粘这一整块）**

```rust
//! 日报管理：跨项目的工时条目（M10，spec §五）。四操作与判据在这里；
//! Excel 双向（Task 6/7）的两个私有函数同在本文件，共用 `EXPORT_HEADERS` 与日期/工时判据。
//!
//! 日期形状判据**不重写一份**：走 `crate::todo::req_date`（先例：`milestone.rs:10` 用 `todo::norm_date`）。
//! `hours` 的界与精度也先于 SQL 拒掉：CHECK 违例冒出来是 db_failed，会把校验失败伪装成库故障。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::todo::req_date;

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查表单必填项"))
}

fn not_found(id: &str) -> AppError {
    AppError::new("not_found", &format!("记录不存在：{id}"), Some("刷新后重试"))
}

/// 单条工时上限。不是「一天不能超 24 小时」的会计规则（多条之和本来可以超），
/// 而是挡住手滑多打一个 0 的那一格。前端 `types/daily-report.ts` 有同名常量，两处各自钉值。
pub(crate) const MAX_HOURS_PER_ROW: f64 = 24.0;

/// 工时的界与精度（先于 SQL，理由同 `milestone::check_stage`）。
/// 两位小数用「放大 100 倍后离整数有多远」判，容差 `1e-9`：`0.1 * 100.0` 落在 `10.000000000000002`，
/// 差 `1.8e-15` 收；`7.535 * 100.0` 离最近整数差 `0.5` 拒。**别改成 `h.fract()`**，那条对 0.1 也判不出。
pub(crate) fn check_hours(h: f64) -> AppResult<f64> {
    let scaled = h * 100.0;
    if !h.is_finite() || h <= 0.0 || h > MAX_HOURS_PER_ROW || (scaled - scaled.round()).abs() > 1e-9 {
        return Err(AppError::new(
            "invalid_input",
            &format!("「工时」要大于 0、不超过 {MAX_HOURS_PER_ROW}、最多两位小数，收到 {h}"),
            Some("工时按小时数填，如 7.5"),
        ));
    }
    Ok(h)
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReport {
    pub id: String,
    pub project_id: String,
    pub report_date: String,
    pub content: String,
    pub hours: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReportInput {
    pub project_id: String,
    /// 收 `Option<String>` 是为了让「缺键」与「空白」走同一句点名错误（`req_date`）。
    pub report_date: Option<String>,
    pub content: String,
    pub hours: f64,
}

/// update 的入参**含** `project_id`：改归属在本轮做（spec §五.4，与 M9 的 `TodoEdit` 不同）。
/// 理由是项目归属是日报最容易填错的字段，且导入按编号/名称匹配、项目改名后要把导错的行挪走。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReportEdit {
    pub project_id: String,
    pub report_date: Option<String>,
    pub content: String,
    pub hours: f64,
}

const SELECT_COLUMNS: &str =
    "d.id, d.project_id, d.report_date, d.content, d.hours, d.created_at, d.updated_at";

fn map_report(row: &rusqlite::Row<'_>) -> rusqlite::Result<DailyReport> {
    Ok(DailyReport {
        id: row.get(0)?,
        project_id: row.get(1)?,
        report_date: row.get(2)?,
        content: row.get(3)?,
        hours: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

/// 每个模块各留一份是本仓现形制（`todo.rs:101`、`milestone.rs:75` 已是两份），这里是第三份。
/// 不趁本轮顺手合并：合并要动那两格的既有测试，跟本轮目标无关，记进 Task 9 的欠账。
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

/// **SQL 里出现任何按项目状态的过滤都算缺陷**（spec §五.3 / D6）：归档项目的日报照常列、照常算。
/// 与 M9 判据⑤（预警跳过归档）形状相反，是有意为之。变异 M① 打的就是这一格。
/// `?1 IS NULL OR d.project_id = ?1` 一形照抄 `todo::list`：**只一条 SQL**，不拆两条拼接。
pub fn list(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<DailyReport>> {
    let mut stmt = conn.prepare(
        &format!(
            "SELECT {SELECT_COLUMNS}
               FROM daily_reports d
               JOIN projects p ON p.id = d.project_id AND p.deleted_at IS NULL
              WHERE d.deleted_at IS NULL AND (?1 IS NULL OR d.project_id = ?1)
              ORDER BY d.report_date, d.created_at, d.id"
        ),
    )?;
    let rows = stmt.query_map(params![project_id], map_report)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<DailyReport> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM daily_reports d WHERE d.id = ?1 AND d.deleted_at IS NULL"
    );
    conn.query_row(&sql, [id], map_report)
        .optional()?
        .ok_or_else(|| not_found(id))
}

pub fn create(conn: &Connection, input: &DailyReportInput) -> AppResult<DailyReport> {
    project_exists(conn, &input.project_id)?;
    let report_date = req_date("日报日期", input.report_date.clone())?;
    let content = input.content.trim().to_owned();
    if content.is_empty() {
        return Err(invalid("日报内容不能为空"));
    }
    let hours = check_hours(input.hours)?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO daily_reports (id, project_id, report_date, content, hours) VALUES (?1,?2,?3,?4,?5)",
        params![id, input.project_id, report_date, content, hours],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, edit: &DailyReportEdit) -> AppResult<()> {
    project_exists(conn, &edit.project_id)?;
    let report_date = req_date("日报日期", edit.report_date.clone())?;
    let content = edit.content.trim().to_owned();
    if content.is_empty() {
        return Err(invalid("日报内容不能为空"));
    }
    let hours = check_hours(edit.hours)?;
    let changed = conn.execute(
        "UPDATE daily_reports SET project_id=?2, report_date=?3, content=?4, hours=?5, updated_at=datetime('now')
          WHERE id=?1 AND deleted_at IS NULL",
        params![id, edit.project_id, report_date, content, hours],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE daily_reports SET deleted_at = datetime('now'), updated_at = datetime('now')
          WHERE id = ?1 AND deleted_at IS NULL",
        [id],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}
```

- [ ] **Step 4: 先粘这一块 —— `daily_report.rs` 的文件头与 16 条测试（Step 3 的 RED 就是它）**

同一文件末尾追加。**每一支都按可红标准写**：反值先心算（写 `assert!(check_hours(0.0).is_err())` 之前确认实现确实拒 0）。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::params;

    fn seed(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, status) VALUES (?1, '日报测试项目', '立项')",
            [id],
        )
        .unwrap();
    }

    /// D6 的靶子夹具：归档状态取自 `projects` 的 8 段枚举（`db.rs` V2），词表外的值会被 CHECK 挡。
    fn seed_archived(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, status) VALUES (?1, '已归档项目', '归档')",
            [id],
        )
        .unwrap();
    }

    fn soft_delete_project(conn: &Connection, id: &str) {
        conn.execute("UPDATE projects SET deleted_at = datetime('now') WHERE id = ?1", [id])
            .unwrap();
    }

    fn input(project_id: &str, date: Option<&str>, content: &str, hours: f64) -> DailyReportInput {
        DailyReportInput {
            project_id: project_id.to_owned(),
            report_date: date.map(|s| s.to_owned()),
            content: content.to_owned(),
            hours,
        }
    }

    #[test]
    fn check_hours_accepts_the_bounds_and_two_decimals() {
        for h in [0.1_f64, 0.01, 7.5, 7.53, 24.0] {
            assert_eq!(check_hours(h).unwrap(), h, "{h} 要收");
        }
    }

    #[test]
    fn check_hours_refuses_zero_out_of_range_and_sub_cent_precision() {
        for h in [0.0_f64, -1.0, 24.01, 100.0, 0.001, 7.535] {
            let err = check_hours(h).unwrap_err();
            assert_eq!(err.code, "invalid_input", "{h} 要在进 SQL 之前拒掉");
            assert!(err.message.contains("工时"), "错误文本要点名「工时」：{}", err.message);
        }
    }

    #[test]
    fn create_requires_an_existing_undeleted_project() {
        let conn = db::open_in_memory().unwrap();
        let err = create(&conn, &input("nope", Some("2026-10-16"), "甲", 1.0)).unwrap_err();
        assert_eq!(err.code, "not_found", "项目不存在按 not_found(项目 id) 报，与 todo 同形");
        seed(&conn, "p-gone");
        soft_delete_project(&conn, "p-gone");
        assert_eq!(
            create(&conn, &input("p-gone", Some("2026-10-16"), "乙", 1.0)).unwrap_err().code,
            "not_found",
            "软删项目也不能挂新日报"
        );
    }

    #[test]
    fn create_rejects_empty_content() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let err = create(&conn, &input("p1", Some("2026-10-16"), "   ", 1.0)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "被拒的输入不许留下行");
    }

    /// **M② 的靶子**：若哪天有人把 `req_date` 换成 `norm_date`，空白会折成 NULL 走进 SQL，
    /// 报错变成 db_failed（V6 那条 NOT NULL 测试就是它的另一面），这条当场红。
    #[test]
    fn create_rejects_blank_and_absent_report_date_naming_the_column() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for date in [None, Some(""), Some("   ")] {
            let err = create(&conn, &input("p1", date, "甲", 1.0)).unwrap_err();
            assert_eq!(err.code, "invalid_input", "{date:?} 要报校验错，不是库错");
            assert!(err.message.contains("日报日期"), "{}", err.message);
        }
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn create_rejects_a_non_iso_report_date() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let err = create(&conn, &input("p1", Some("2026-6-30"), "甲", 1.0)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert_eq!(err.hint.as_deref(), Some("日期要用 YYYY-MM-DD"));
        assert!(err.message.contains("日报日期"), "{}", err.message);
    }

    #[test]
    fn create_rejects_out_of_range_hours() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        assert_eq!(create(&conn, &input("p1", Some("2026-10-16"), "甲", 30.0)).unwrap_err().code, "invalid_input");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "工时被拒也不许留下行");
    }

    #[test]
    fn create_then_read_roundtrip() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let r = create(&conn, &input("p1", Some(" 2026-10-16 "), "  联调第三方接口  ", 7.5)).unwrap();
        assert_eq!(r.content, "联调第三方接口", "写入要 trim");
        assert_eq!(r.report_date, "2026-10-16", "日期两侧都要 trim 后进判据");
        assert_eq!(r.hours, 7.5);
        assert_eq!(get(&conn, &r.id).unwrap(), r, "create 回来的行必须与再读的一致");
        assert_eq!(list(&conn, Some("p1")).unwrap().len(), 1);
        assert_eq!(list(&conn, None).unwrap().len(), 1, "None = 跨项目全部");
    }

    /// 三级排序键一起钉：`report_date` 是主键（按天填的直觉），后两级是 M9 N3 那条同秒 tiebreaker。
    /// 直写 INSERT 而不走 create()：uuid 主键给不出可控 id 次序，同秒 created_at 也只能手写。
    #[test]
    fn list_orders_by_report_date_then_created_at_then_id() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for (id, date, created) in [
            ("d-b", "2026-10-15", "2026-10-15 09:00:00"),
            ("d-a", "2026-10-15", "2026-10-15 09:00:00"),
            ("d-c", "2026-10-14", "2026-10-14 23:00:00"),
        ] {
            conn.execute(
                "INSERT INTO daily_reports (id, project_id, report_date, content, hours, created_at)
                 VALUES (?1, 'p1', ?2, ?1, 1.0, ?3)",
                params![id, date, created],
            )
            .unwrap();
        }
        let ids: Vec<String> = list(&conn, None).unwrap().into_iter().map(|r| r.id).collect();
        assert_eq!(ids.join(","), "d-c,d-a,d-b", "先按日期（10-14 在前），同日同秒按 id 升序；插入序是 b、a、c");
    }

    #[test]
    fn list_hides_soft_deleted_reports() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let r = create(&conn, &input("p1", Some("2026-10-16"), "甲", 1.0)).unwrap();
        delete(&conn, &r.id).unwrap();
        assert!(list(&conn, None).unwrap().is_empty());
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports WHERE deleted_at IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "delete 是软删：行要在库里（延续 M9 D8 的形制）");
        assert_eq!(get(&conn, &r.id).unwrap_err().code, "not_found");
    }

    #[test]
    fn cross_project_list_hides_reports_of_a_soft_deleted_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p-alive");
        seed(&conn, "p-dead");
        create(&conn, &input("p-alive", Some("2026-10-16"), "活项目的日报", 1.0)).unwrap();
        let dead = create(&conn, &input("p-dead", Some("2026-10-16"), "已删项目的日报", 1.0)).unwrap();
        soft_delete_project(&conn, "p-dead");

        let contents: Vec<String> = list(&conn, None).unwrap().into_iter().map(|r| r.content).collect();
        assert_eq!(contents, vec!["活项目的日报"], "软删项目的日报不能出现在跨项目列表里");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports WHERE id = ?1", [&dead.id], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "项目软删不许动日报的行（恢复项目即原样回来）");
        assert!(list(&conn, Some("p-dead")).unwrap().is_empty(), "指定已删项目也读不到");
    }

    /// **D6 的兑现点，也是变异 M① 的靶子**：给 `list` 顺手加一句 `AND p.status <> '归档'`，
    /// 这条当场红。归档项目的工时照常列、照常算 —— 跳过归档等于给领导的台账凭空少一截。
    #[test]
    fn list_includes_rows_of_an_archived_project() {
        let conn = db::open_in_memory().unwrap();
        seed_archived(&conn, "p-old");
        let r = create(&conn, &input("p-old", Some("2026-03-02"), "收尾交付物归档", 3.5)).unwrap();
        let all = list(&conn, None).unwrap();
        assert_eq!(all.len(), 1, "归档项目的日报要在跨项目列表里");
        assert_eq!(all[0].id, r.id);
        assert_eq!(list(&conn, Some("p-old")).unwrap().len(), 1, "按归档项目筛也要读到");
    }

    #[test]
    fn update_can_move_a_row_to_another_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        seed(&conn, "p2");
        let r = create(&conn, &input("p1", Some("2026-10-16"), "甲", 1.0)).unwrap();
        update(
            &conn,
            &r.id,
            &DailyReportEdit {
                project_id: "p2".to_owned(),
                report_date: Some("2026-10-17".to_owned()),
                content: "乙".to_owned(),
                hours: 2.0,
            },
        )
        .unwrap();
        let after = get(&conn, &r.id).unwrap();
        assert_eq!(after.project_id, "p2", "改归属在本轮做（spec §五.4，与 TodoEdit 不同）");
        assert_eq!(after.report_date, "2026-10-17");
        assert_eq!(after.content, "乙");
        assert_eq!(after.hours, 2.0);
    }

    #[test]
    fn update_refuses_a_missing_or_soft_deleted_project_and_bad_fields() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        seed(&conn, "p-gone");
        soft_delete_project(&conn, "p-gone");
        let r = create(&conn, &input("p1", Some("2026-10-16"), "甲", 1.0)).unwrap();
        let edit = |pid: &str, date: &str, hours: f64| DailyReportEdit {
            project_id: pid.to_owned(),
            report_date: Some(date.to_owned()),
            content: "乙".to_owned(),
            hours,
        };
        assert_eq!(update(&conn, &r.id, &edit("p-gone", "2026-10-16", 1.0)).unwrap_err().code, "not_found");
        assert_eq!(update(&conn, &r.id, &edit("p1", "2026-10-16", 99.0)).unwrap_err().code, "invalid_input");
        assert_eq!(update(&conn, &r.id, &edit("p1", "10-16", 1.0)).unwrap_err().code, "invalid_input");
        assert_eq!(update(&conn, "no-such-id", &edit("p1", "2026-10-16", 1.0)).unwrap_err().code, "not_found");
        assert_eq!(get(&conn, &r.id).unwrap().content, "甲", "被拒的 update 一条都不许改着");
    }

    #[test]
    fn delete_is_soft_and_a_second_delete_reports_not_found() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let r = create(&conn, &input("p1", Some("2026-10-16"), "甲", 1.0)).unwrap();
        delete(&conn, &r.id).unwrap();
        assert_eq!(
            delete(&conn, &r.id).unwrap_err().code,
            "not_found",
            "二次删要响亮，不能静默成功"
        );
    }

    /// 线格式钉键数（本仓形制：`todo.rs:424` 七键、manifest 八键、SearchBundle 七键）。
    #[test]
    fn daily_report_line_format_has_exactly_seven_keys() {
        let r = DailyReport {
            id: "d1".to_owned(),
            project_id: "p1".to_owned(),
            report_date: "2026-10-16".to_owned(),
            content: "甲".to_owned(),
            hours: 7.5,
            created_at: "2026-10-16 00:00:00".to_owned(),
            updated_at: "2026-10-16 00:00:00".to_owned(),
        };
        let obj = serde_json::to_value(&r).unwrap();
        let mut keys: Vec<&str> = obj.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["content", "createdAt", "hours", "id", "projectId", "reportDate", "updatedAt"],
            "DailyReport 的键集合变了就要同步 src/types/daily-report.ts 与 xlsx 表头"
        );
    }
}
```

- [ ] **Step 6: 接线四条命令，并顺手改掉那句假话**

`lib.rs:1-2` 之间插模块声明（字母序在 `backup` 后、`db` 前）：

```rust
mod backup;
mod daily_report;
mod db;
```

`lib.rs:27`（`use crate::error::{AppError, AppResult};`）**之前**插类型导入：

```rust
use crate::daily_report::{DailyReport, DailyReportEdit, DailyReportInput};
```

在 `milestone_delete` 那条命令（`lib.rs:743-747`）之后追加四条。**注释里别抄 `lib.rs:690` 那句被下面更正的话**：

```rust
/// 日报四条命令（M10 Task 2，形制照 `todo_*`）：同步、函数体不写 await 点。
/// `db(&state)?` 取回的 `MutexGuard` 不是 `Send`，**在 async fn 里让它横跨 await 点**才编不过（M8 D11）。
/// 只挂 async 属性、函数体照常同步是合法的 —— `backup_import` 与 `doc_preview` 就是这个形态，
/// M10 Task 6/7 的导出与导入三条也走这一形（spec §五.5）。
#[tauri::command]
fn daily_report_list(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> AppResult<Vec<DailyReport>> {
    let conn = db(&state)?;
    daily_report::list(&conn, project_id.as_deref())
}

#[tauri::command]
fn daily_report_create(state: State<'_, AppState>, input: DailyReportInput) -> AppResult<DailyReport> {
    let conn = db(&state)?;
    daily_report::create(&conn, &input)
}

#[tauri::command]
fn daily_report_update(state: State<'_, AppState>, id: String, input: DailyReportEdit) -> AppResult<()> {
    let conn = db(&state)?;
    daily_report::update(&conn, &id, &input)
}

#[tauri::command]
fn daily_report_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    daily_report::delete(&conn, &id)
}
```

同一次编辑里把 `lib.rs:690-691` 那两句换成：

```rust
/// 待办四条命令（M9 Task 2，形制照 `ledger_env_create`）：全部同步、函数体不写 await 点。
/// **本注释原先写的是「给它们加 async 标记会直接编不过」——那是句假话，M10 更正**：
/// 真取 `db(&state)?` 的 `MutexGuard` 又挂着 async 属性的是 `backup_import` 与 `doc_preview` 这两条，
/// 它们今天就编得过（`backup_export` 开的是另一条连接、`backup_inspect` 根本不碰库，不能当证据）。
/// 真编不过的是 `async fn` 里让 guard 横跨 await 点；只加属性、函数体不 await，
/// 得到的正是 M10 Task 6/7 那三条导出/导入命令用的形态（spec §五.5）。
```

> **这句假话在本文件里有两份，两份都要改**（Task 2 复审时控制方才发现）：`todo_*` 那块之外，`milestone_*` 那块（原 `lib.rs:723-724`）带着同一句「给它们加 async 标记会直接编不过」，本 Step 原先只写了前一份。第二份换成同样的更正说法。同一格还顺带纠正了一处**枚举过宽**：真在 async 属性下取 `MutexGuard` 的是 `backup_import` 与 `doc_preview` 两条，`backup_export` 开的是新连接、`backup_inspect` 不碰库，把它们一起算进「那三条都取 guard」是错的（结论不变，证据要准）。

注册表（`lib.rs:855` 附近）：给 `milestone_delete` **补上逗号**，末尾追加四条且**最后一条不带逗号**：

```rust
            milestone_delete,
            daily_report_list,
            daily_report_create,
            daily_report_update,
            daily_report_delete
        ])
```

- [ ] **Step 7: 跑测试与门**

```bash
cd /e/zero/demo/2026/project-files-manage/src-tauri
cargo test -j 1 --lib            # 期望 277 passed / 0 failed（256 + T1 的 3 + 本格的 18）
cargo clippy --lib -- -D warnings # 期望 exit=0、0 warning
cargo clippy --lib --all-targets -- -D warnings
cd /e/zero/demo/2026/project-files-manage
sed -n '/generate_handler/,/^      \]/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"   # 期望 57
grep -c 'tauri::command(async)' src-tauri/src/lib.rs    # 期望仍是 4（本格不加 async）
```

若 clippy 报 `DailyReportEdit` 未使用之类，说明 Step 6 的接线漏了半边，补接线而不是加豁免（全仓 `dead_code` 豁免只许有 `db.rs:30` 那一条）。

- [ ] **Step 8: 提交**

```bash
cd /e/zero/demo/2026/project-files-manage
git add src-tauri/src/daily_report.rs src-tauri/src/todo.rs src-tauri/src/lib.rs
git commit -m "feat(m10): daily_report 四操作 + req_date + 四条同步命令（53→57）—— report_date 走 req_date 而不是 norm_date（空白折 NULL 会让 NOT NULL 冒成 db_failed，M②靶）、hours 先于 SQL 校 0<h<=24 且两位小数（整数化容差 1e-9，0.1 收 7.535 拒）、list 里不许出现状态过滤（归档项目照常列，M①靶）、DailyReportEdit 含 projectId（与 M9 TodoEdit 不同，改名后能把导错的行挪走）；顺手更正 lib.rs:690 那句「加 async 标记编不过」的假话（备份那三条今天照样编得过）"
```

---

### Task 3: 全局检索接入 —— 日报正文与待办标题进台账段

**Files:**
- Modify: `src-tauri/src/search.rs:259`（`field_hits` 里 `note` 那条 collect 之后、`Ok(hits)`（`:261`）之前追加两条）
- Modify: `src-tauri/src/search.rs` 的 `mod tests`（`use crate::{...}` 那行加 `daily_report, todo`，追加 2 条测试）
- Modify: `src/types/search.ts:5`（`HitSource`）与 `src/types/search.ts:17-24`（`SOURCE_LABELS`）
- Test: `src-tauri/src/search.rs`

**Interfaces:**
- Consumes: `daily_reports` 表与 `daily_report::create`、`todos` 表与 `todo::create`（Task 2 / M9）；`collect(source, sql, conn, pattern)`（`search.rs:125`，SELECT 必须是 **7 列**：id、project_id、项目名、title、b1、b2、b3）；`field_hits(conn, query)`（`search.rs:144`）。
- Produces: 台账段八条 collect（`project|env|credential|server|link|note|daily|todo`）；前端 `HitSource` 多两支、`SOURCE_LABELS` 多两项。**`SearchBundle` 的七键线格式不变**（`:774` 那条键集断言不许动）。

- [ ] **Step 1: 写测试（RED 落在编译层：`source` 是字面量，新支不存在时断言编不过？不会 —— 所以 RED 用「命中为空」取）**

追加到 `search.rs` 的 `mod tests` 末尾。先把 `:337` 那行 `use crate::{db, index_scan::ScannedFile, index_store, ledger, project, vault};` 改成含 `daily_report`、`todo`：

```rust
    use crate::{db, daily_report, index_scan::ScannedFile, index_store, ledger, project, todo, vault};
```

```rust
    /// D4 的兑现点：需求原文 §四.1 点名的「日报内容」进台账检索，M9 欠的待办那格一起兑。
    /// 诱饵词故意与各表既有夹具都不重叠 —— `only_hit` 断的是「本次命中恰好只有这一条」。
    #[test]
    fn daily_report_content_and_todo_title_join_the_ledger_section() {
        let (conn, pid, mk) = fixture();
        seed_ledger(&conn, &pid, &mk);
        daily_report::create(
            &conn,
            &daily_report::DailyReportInput {
                project_id: pid.clone(),
                report_date: Some("2026-10-16".to_owned()),
                content: "压测报告补材料".to_owned(),
                hours: 3.5,
            },
        )
        .unwrap();
        todo::create(
            &conn,
            &todo::TodoInput {
                project_id: pid.clone(),
                title: "补齐压测报告附件清单".to_owned(),
                due_date: Some("2026-10-20".to_owned()),
            },
        )
        .unwrap();

        let hit = only_hit(&conn, "压测报告补材料", "daily", "压测报告补材料");
        assert_eq!(hit.project_id, pid, "日报命中要带上所属项目，供结果页分组与跳转");
        assert!(hit.detail.contains("2026-10-16"), "副标题要念日期：{}", hit.detail);
        assert!(hit.detail.contains("3.5"), "副标题要念工时：{}", hit.detail);

        let t = only_hit(&conn, "补齐压测报告附件清单", "todo", "补齐压测报告附件清单");
        assert_eq!(t.project_id, pid);
        assert!(t.detail.contains("未完成"), "待办副标题要念勾选状态：{}", t.detail);
    }

    /// 软删项目的行不进检索 —— 这条与 `todo.rs`/`daily_report.rs` 的同名判据是**同一形状**，
    /// 变异 M①（摘掉 `p.deleted_at IS NULL`）在这三处任一都能被相应断言打红。
    #[test]
    fn soft_deleted_project_hides_daily_and_todo_hits() {
        let (conn, pid, _mk) = fixture();
        daily_report::create(
            &conn,
            &daily_report::DailyReportInput {
                project_id: pid.clone(),
                report_date: Some("2026-10-15".to_owned()),
                content: "季度巡检记录".to_owned(),
                hours: 2.0,
            },
        )
        .unwrap();
        todo::create(
            &conn,
            &todo::TodoInput {
                project_id: pid.clone(),
                title: "季度巡检待办跟进".to_owned(),
                due_date: None,
            },
        )
        .unwrap();
        assert!(!field_hits(&conn, "季度巡检记录").unwrap().is_empty(), "前置条件：项目还活着时两条都要能搜到");
        assert_eq!(field_hits(&conn, "季度巡检记录").unwrap().len(), 1);
        assert_eq!(field_hits(&conn, "季度巡检待办跟进").unwrap().len(), 1);

        conn.execute("UPDATE projects SET deleted_at = datetime('now') WHERE id = ?1", [&pid])
            .unwrap();
        for needle in ["季度巡检记录", "季度巡检待办跟进"] {
            assert!(
                field_hits(&conn, needle).unwrap().is_empty(),
                "软删项目的行不该进检索：{needle}"
            );
        }
    }
```

Run: `cd /e/zero/demo/2026/project-files-manage/src-tauri && cargo test -j 1 --lib search::tests::daily_report_content_and_todo_title`
Expected: FAIL —— `only_hit` 那句 `assert_eq!(sources(&hits), vec!["daily"])` 拿到空 vec（`left: []`, `right: ["daily"]）。**这一格是断言层的红，不是编译层的红**（`source` 只是 `&'static str`，新支不存在时什么都编不出来）。

- [ ] **Step 2: 加两条 collect**

在 `note` 那条 `collect(...)` 的收尾 `)?;`（`search.rs:259`）之后、`Ok(hits)` 之前插入。**两条都必须是「`JOIN projects … AND p.deleted_at IS NULL` + `x.deleted_at IS NULL` + LIKE 转义 + `ORDER BY updated_at DESC` + `LIMIT ?2`」这一形，SELECT 恰好 7 列**（`map_row` 按 0..6 取，列数错了当场红）：

```rust
    // 日报：只搜工作内容。工时与日期拼成副标题（CAST 成 TEXT 是为了进 `join_bits` 那个
    // [Option<String>; 3]，别改成 printf —— bundled SQLite 的 printf 不在本轮验证过的前提上）。
    hits.append(&mut collect(
        "daily",
        "SELECT d.id, d.project_id, p.name, SUBSTR(d.content, 1, 80), d.report_date,
                ('工时 ' || CAST(d.hours AS TEXT) || ' 小时'), NULL
           FROM daily_reports d
           JOIN projects p ON p.id = d.project_id AND p.deleted_at IS NULL
          WHERE d.deleted_at IS NULL
            AND d.content LIKE ?1 ESCAPE '\\'
          ORDER BY d.updated_at DESC
          LIMIT ?2",
        conn,
        &p,
    )?);

    // 待办：只搜标题（M9 已裁定 todos 表没有 note 列，正文就是 title）。
    // D4 当次反转了 M9 spec §十一「待办正文不进任何检索通路」那句，理由写在 spec §九.8。
    hits.append(&mut collect(
        "todo",
        "SELECT t.id, t.project_id, p.name, t.title, t.due_date,
                CASE t.done WHEN 1 THEN '已完成' ELSE '未完成' END, NULL
           FROM todos t
           JOIN projects p ON p.id = t.project_id AND p.deleted_at IS NULL
          WHERE t.deleted_at IS NULL
            AND t.title LIKE ?1 ESCAPE '\\'
          ORDER BY t.updated_at DESC
          LIMIT ?2",
        conn,
        &p,
    )?);
```

Run: `cd /e/zero/demo/2026/project-files-manage/src-tauri && cargo test -j 1 --lib search::`
Expected: 全绿（含新增 2 条；`search.rs` 原有那条七键 `SearchBundle` 键集断言必须仍然绿 —— 它不该被碰到）。

- [ ] **Step 3: 前端 `HitSource` 与标签**

`src/types/search.ts:5`：

```ts
export type HitSource =
  | "project"
  | "env"
  | "credential"
  | "server"
  | "link"
  | "note"
  | "daily"
  | "todo";
```

`:17-24` 的 `SOURCE_LABELS` 末尾补两项（`Record<HitSource, string>` 是**穷尽的**，漏一项 `npm run build` 直接 type error —— 这一格由编译期守着，不用另写测试）：

```ts
  note: "备注台账",
  daily: "日报",
  todo: "待办",
```

- [ ] **Step 4: 门禁 + 提交**

```bash
cd /e/zero/demo/2026/project-files-manage/src-tauri && cargo test -j 1 --lib    # 期望 279 passed（277 + 本格 2）
cargo clippy --lib -- -D warnings
cd /e/zero/demo/2026/project-files-manage && npm run build                       # exit=0 且 error TS = 0
```

```bash
git add src-tauri/src/search.rs src/types/search.ts
git commit -m "feat(m10): 台账检索从六条 collect 变八条 —— 日报正文与待办标题各加一条同形状 per-table LIKE（JOIN 自带 p.deleted_at IS NULL，工时 CAST 成 TEXT 拼副标题，不引 printf），前端 HitSource/SOURCE_LABELS 补两支（Record 穷尽性由 tsc 守）；D4 反转 M9 §十一 那句「待办正文不进检索」并按 spec §九.8 记账"
```

---

### Task 4: 前端契约层与工时聚合纯函数

**Files:**
- Create: `src/types/daily-report.ts`
- Create: `src/lib/hours-summary.ts`
- Create: `tests/hours-summary.test.ts`
- Modify: `src/lib/api.ts:91`（`deleteMilestone` 之后追加四个包装）

**Interfaces:**
- Consumes: Task 2 的四条命令与七键线格式；`due-warning.ts:55` 的 `projectLabel(p)`（value import）。
- Produces（Task 5 只消费这一节）:
  - `DailyReport` / `DailyReportInput` / `DailyReportEdit` / `MAX_HOURS_PER_ROW = 24`
  - `listDailyReports(projectId: string | null)`、`createDailyReport(input)`、`updateDailyReport(id, input)`、`deleteDailyReport(id)`
  - `hours-summary.ts`：`HoursBucket`、`HoursFilter`、`HoursRow`、`ProjectHours`、`HoursSummary`、`sumHours`、`isoWeekKey`、`monthKey`、`dayKey`、`scopeDailyReports`、`scopedHoursSummary`、`previousReportDay`
  - `previousReportDay(rows, targetDate): string | null` —— 「最近一个有日报的那天」的唯一实现处（D10）。

- [ ] **Step 1: `src/types/daily-report.ts`**

```ts
// 与 src-tauri/src/daily_report.rs 的出参逐字段对齐（camelCase 来自 #[serde(rename_all = "camelCase")]）。
// 七键由 daily_report.rs::daily_report_line_format_has_exactly_seven_keys 钉住；
// xlsx 表头五列由 daily_report.rs::EXPORT_HEADERS 钉住，两处不一致时以 Rust 侧为真值。
export interface DailyReport {
  id: string;
  projectId: string;
  reportDate: string;
  content: string;
  hours: number;
  createdAt: string;
  updatedAt: string;
}

/// `reportDate` 传 `null` 是合法的请求形状 —— 后端 `req_date` 会把它连同空白一起
/// 当成「你没填日期」响亮拒掉（错误文本点名「日报日期」）。前端不做二次校验来「省一次往返」，
/// 因为判据只有一处（后端），界面自己先拦会漂成两套口径。
export interface DailyReportInput {
  projectId: string;
  reportDate: string | null;
  content: string;
  hours: number;
}

/// 与 Rust 的 DailyReportEdit 对齐：**含 projectId**（改归属在本轮，spec §五.4，与 TodoEdit 不同）。
export interface DailyReportEdit {
  projectId: string;
  reportDate: string | null;
  content: string;
  hours: number;
}

/// 与 Rust 侧 `daily_report::MAX_HOURS_PER_ROW` 同值；两格各钉各的断言，漂了就红。
export const MAX_HOURS_PER_ROW = 24;

/// 表单里的空串折成 null（同 `todo.ts` 的 `dueOrNull` 形制）。
export const dateOrNull = (raw: string): string | null => (raw.trim() === "" ? null : raw.trim());
```

- [ ] **Step 2: `src/lib/api.ts` 四个包装**

在 `deleteMilestone`（`:91`）之后追加：

```ts
// —— M10 日报：四个封装（导出/导入的三个在 Task 6/7 各自那一格补，别提前塞）——
// list 的 projectId 传显式 `null` 而不是省键（同 M9 那条告诫）。
export const listDailyReports = (projectId: string | null) =>
  invoke<DailyReport[]>("daily_report_list", { projectId });
export const createDailyReport = (input: DailyReportInput) =>
  invoke<DailyReport>("daily_report_create", { input });
export const updateDailyReport = (id: string, input: DailyReportEdit) =>
  invoke<void>("daily_report_update", { id, input });
export const deleteDailyReport = (id: string) => invoke<void>("daily_report_delete", { id });
```

并在 `:20-21` 的 `import type` 区补一行（同形制）：

```ts
import type { DailyReport, DailyReportEdit, DailyReportInput } from "@/types/daily-report";
```

- [ ] **Step 3: 写 `tests/hours-summary.test.ts`（RED：模块还不存在）**

```ts
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  dayKey,
  isoWeekKey,
  monthKey,
  previousReportDay,
  scopeDailyReports,
  scopedHoursSummary,
  sumHours,
  type HoursFilter,
} from "../src/lib/hours-summary.ts";
import type { DailyReport } from "../src/types/daily-report.ts";
import type { Project } from "../src/types/project.ts";

const row = (over: Partial<DailyReport> = {}): DailyReport => ({
  id: "d1",
  projectId: "p1",
  reportDate: "2026-10-16",
  content: "甲",
  hours: 1,
  createdAt: "2026-10-16 09:00:00",
  updatedAt: "2026-10-16 09:00:00",
  ...over,
});

const project = (over: Partial<Project> = {}): Project => ({
  id: "p1",
  name: "乙项目",
  // `code` 故意留 null：`projectLabel` 在有编号时返回「名（编号）」，
  // 下面按组序断言的是纯项目名，编号一进来就变成比较「乙项目（P-02）」这种串了。
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
  ...over,
});

// —— ISO 周：三条跨年边界是本轮最容易被写错、写错又不报错的一格 ——
test("isoWeekKey 钉住跨年周三条边界（周一为周首，含周四的那一周属于该年）", () => {
  assert.equal(isoWeekKey("2026-01-01"), "2026-W01", "2026-01-01 是周四 ⇒ 该周是 2026 年第 1 周");
  assert.equal(isoWeekKey("2026-12-31"), "2026-W53", "2026 是 53 周年，12-31 是周四");
  assert.equal(isoWeekKey("2027-01-01"), "2026-W53", "跨年的周五仍属 12-31 那一周，不许漂成 2027-W01");
  assert.equal(isoWeekKey("2027-01-04"), "2027-W01", "下一个周一起才换年");
});

test("isoWeekKey 同周不同日同键、周日归上一周", () => {
  assert.equal(isoWeekKey("2026-10-12"), isoWeekKey("2026-10-18"), "周一到周日同一周");
  assert.equal(isoWeekKey("2026-10-11"), "2026-W41", "2026-10-11 是周日，归 W41（周一为周首）");
});

test("日桶用原串、月桶取前七位", () => {
  assert.equal(dayKey("2026-10-16"), "2026-10-16");
  assert.equal(monthKey("2026-10-16"), "2026-10");
  assert.equal(monthKey("2026-01-05"), "2026-01");
});

test("sumHours 整数化求和：十条 0.1 得 1 而不是 1.0000000000000002", () => {
  const rows = Array.from({ length: 10 }, (_, i) => row({ id: `d${i}`, hours: 0.1 }));
  assert.equal(sumHours(rows), 1, "别改成 toBeCloseTo，那样恒绿");
  assert.equal(sumHours([]), 0, "空数组得 0，不是 NaN");
});

test("筛选三份条件一起缩：项目、日期闭区间、关键词（大小写与首尾空格都收）", () => {
  const rows = [
    row({ id: "a", projectId: "p1", reportDate: "2026-10-01", content: "联调甲" }),
    row({ id: "b", projectId: "p2", reportDate: "2026-10-16", content: "联调乙" }),
    row({ id: "c", projectId: "p1", reportDate: "2026-10-20", content: "验收甲" }),
  ];
  const f = (over: Partial<HoursFilter> = {}): HoursFilter => ({
    projectId: null,
    from: "",
    to: "",
    keyword: "",
    ...over,
  });
  assert.deepEqual(scopeDailyReports(rows, f()).map((r) => r.id), ["a", "b", "c"], "无条件 = 全部");
  assert.deepEqual(scopeDailyReports(rows, f({ projectId: "p1" })).map((r) => r.id), ["a", "c"]);
  assert.deepEqual(
    scopeDailyReports(rows, f({ from: "2026-10-01", to: "2026-10-01" })).map((r) => r.id),
    ["a"],
    "闭区间：两端都要含进来",
  );
  assert.deepEqual(scopeDailyReports(rows, f({ keyword: " 联调 " })).map((r) => r.id), ["a", "b"], "关键词两侧 trim");
  assert.deepEqual(scopeDailyReports(rows, f({ keyword: "验收" })).map((r) => r.id), ["c"]);
});

test("scopedHoursSummary 只算筛选后的行（M⑥ 靶：改成吃全量这条当场红）", () => {
  const rows = [
    row({ id: "a", projectId: "p1", reportDate: "2026-10-16", content: "甲", hours: 3 }),
    row({ id: "b", projectId: "p2", reportDate: "2026-10-16", content: "乙", hours: 5 }),
  ];
  const f = (over: Partial<HoursFilter> = {}): HoursFilter => ({ projectId: null, from: "", to: "", keyword: "", ...over });
  const onlyP1 = scopedHoursSummary(rows, f({ projectId: "p1" }), "day", [project({ id: "p1", name: "甲项目" }), project({ id: "p2", name: "乙项目" })]);
  assert.equal(onlyP1.total, 3, "筛到甲项目，总工时不能把乙项目的 5 也算进来");
  assert.equal(onlyP1.byProject.length, 1);
  assert.equal(onlyP1.buckets[0].count, 1);
  const all = scopedHoursSummary(rows, f(), "day", []);
  assert.equal(all.total, 8);
  assert.equal(all.count, 2);
});

test("按月分桶与组序：项目名按拼音、工时相同也要有确定次序", () => {
  const rows = [
    row({ id: "a", projectId: "p2", reportDate: "2026-10-16", hours: 2 }),
    row({ id: "b", projectId: "p1", reportDate: "2026-09-30", hours: 2 }),
    row({ id: "c", projectId: "p1", reportDate: "2026-09-01", hours: 1 }),
  ];
  const f: HoursFilter = { projectId: null, from: "", to: "", keyword: "" };
  const s = scopedHoursSummary(rows, f, "month", [project({ id: "p1", name: "乙项目" }), project({ id: "p2", name: "甲项目" })]);
  assert.deepEqual(s.buckets.map((b) => `${b.key}:${b.hours}`), ["2026-09:3", "2026-10:2"], "月桶按 key 升序");
  assert.deepEqual(s.byProject.map((p) => `${p.projectLabel}:${p.hours}`), ["甲项目:2", "乙项目:3"], "组序按拼音，不是码元序");
});

test("形状不对的行被跳过并计数，不许静默少算", () => {
  const rows = [
    row({ id: "a", reportDate: "2026-10-16" }),
    row({ id: "b", reportDate: "20261016" }),
    row({ id: "c", reportDate: "" }),
    row({ id: "d", reportDate: "2026-13-45" }),
  ];
  const s = scopedHoursSummary(rows, { projectId: null, from: "", to: "", keyword: "" }, "day", []);
  assert.equal(s.unparsed, 2, "无分隔与空串不是形状；「2026-13-45」形状对、历法错也算对（与后端 is_iso_date 同口径，引历法库就破零新依赖）");
  assert.equal(s.total, 2, "只有 a 与 d 进桶，各 1 小时");
  assert.equal(s.count, 2);
  assert.deepEqual(s.buckets.map((b) => b.key), ["2026-10-16", "2026-13-45"], "串比就是日期序，但历法错的值自己一格一桶");
});

test("previousReportDay 取最近一个有日报的那天，不是机械的前一天（D10）", () => {
  const rows = [
    row({ id: "f", reportDate: "2026-10-16" }),
    row({ id: "g", reportDate: "2026-10-15" }),
    row({ id: "m", reportDate: "2026-10-12" }),
  ];
  assert.equal(previousReportDay(rows, "2026-10-19"), "2026-10-16", "周一复制的是上周五，不是周日");
  assert.equal(previousReportDay(rows, "2026-10-16"), "2026-10-15", "目标日本身不算");
  assert.equal(previousReportDay(rows, "2026-10-01"), null, "之前一条都没有 ⇒ null，界面据此禁用按钮");
  assert.equal(previousReportDay([], "2026-10-19"), null);
});

// value import 走相对路径带 .ts 是「node --test 能直跑」的前提（先例 todo-view.ts:8 的注释）。
// `@/` 别名在纯 Node 下解析不到，而 tsc/vite 都看不出来 —— 这条守卫不写，以后有人顺手改成 `@/` 就炸在 CI 之外。
test("hours-summary.ts 的 value import 只走相对路径带 .ts", () => {
  const src = readFileSync(new URL("../src/lib/hours-summary.ts", import.meta.url), "utf8");
  for (const line of src.split(/\r?\n/)) {
    if (!/^import\b/.test(line) || /^import\s+type\b/.test(line)) continue;
    assert.match(line, /from "\.\/[^"]+\.ts"/, `value import 必须是相对路径带 .ts：${line}`);
  }
});
```

Run（在**仓库根**）: `cd /e/zero/demo/2026/project-files-manage && node --test "tests/hours-summary.test.ts"`
Expected: FAIL —— `Cannot find module .../src/lib/hours-summary.ts`（模块还不存在，这一格是加载层的红）。
**别在 `src-tauri` 目录下跑 `node --test`**：那里的 `tests/**/*.test.ts` 一条都匹配不到，返回 `tests 0` 会被当成「红过了」（本仓踩过）。

- [ ] **Step 4: 实现 `src/lib/hours-summary.ts`**

```ts
// 工时统计的纯逻辑（M10 spec §七 / D9）。聚合放前端是因为 SQL 不解释日期：
// `strftime`/`date` 按 UTC，周首也与 ISO 直觉不一致，M9 D7 就是为躲这个把判据搬过来的（沿用）。
// 本文件不读机器时钟 —— 今天由调用方从 `localToday()` 传进来，判据才能喂常量做断言。
import type { Project } from "@/types/project";
import type { DailyReport } from "@/types/daily-report";
// 唯一的 value import：相对路径 + .ts 后缀（`@/` 别名在 node --test 下解析不到，见 todo-view.ts:8 同条注释）。
import { projectLabel } from "./due-warning.ts";

export type HoursBucket = "day" | "week" | "month";

export interface HoursFilter {
  /** `null` = 全部项目。 */
  projectId: string | null;
  /** 闭区间的两端，`""` = 不限。形状不校验，交给比较本身（YYYY-MM-DD 串比就是日期序）。 */
  from: string;
  to: string;
  keyword: string;
}

export interface HoursRow {
  key: string;
  hours: number;
  /** 条目数：工时相同但条数不同的两格要看得出差别。 */
  count: number;
}

export interface ProjectHours {
  projectId: string;
  projectLabel: string;
  hours: number;
  count: number;
}

export interface HoursSummary {
  buckets: HoursRow[];
  byProject: ProjectHours[];
  total: number;
  count: number;
  /** 日期形状解析不了、被跳过的行数。界面必须念出来，别静默少算（沿用 due-warning 的 unparsed 形制）。 */
  unparsed: number;
}

const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;
const DAY_MS = 86_400_000;

export const dayKey = (date: string): string => date;
export const monthKey = (date: string): string => date.slice(0, 7);

/// ISO-8601 周键 `YYYY-Www`：周一起算，**包含周四的那一周属于该年**。
/// 算法就是「把日期挪到本周四 → 取那一年 → 与当年第一周的周四相减」。
/// 不许改成 `getDay()` 直接算周序，也不许用 `strftime('%W')`：两者的周首与年首边界都不符 ISO。
export function isoWeekKey(date: string): string {
  const [y, m, d] = date.split("-").map(Number);
  const thursday = new Date(Date.UTC(y, m - 1, d));
  const dow = thursday.getUTCDay() || 7; // 周一=1 … 周日=7
  thursday.setUTCDate(thursday.getUTCDate() + 4 - dow);
  const isoYear = thursday.getUTCFullYear();
  const jan4 = new Date(Date.UTC(isoYear, 0, 4));
  jan4.setUTCDate(jan4.getUTCDate() + 4 - (jan4.getUTCDay() || 7));
  const week = Math.floor((thursday.getTime() - jan4.getTime()) / (7 * DAY_MS)) + 1;
  return `${isoYear}-W${String(week).padStart(2, "0")}`;
}

/// 求和一律整数化（spec §五.2）。`0.1 * 10` 直接累加得 `1.0000000000000002`，
/// 那条界面写出来就是「本周合计 1.0000000000000002 小时」。
export function sumHours(rows: DailyReport[]): number {
  let cents = 0;
  for (const r of rows) cents += Math.round(r.hours * 100);
  return cents / 100;
}

/// 三份条件一起缩：这是「聚合的入参必须是筛选后的行」的唯一实现处（M9 终审 I1 同族的教训）。
export function scopeDailyReports(rows: DailyReport[], f: HoursFilter): DailyReport[] {
  const keyword = f.keyword.trim().toLowerCase();
  return rows.filter((r) => {
    if (f.projectId !== null && r.projectId !== f.projectId) return false;
    if (f.from !== "" && r.reportDate < f.from) return false;
    if (f.to !== "" && r.reportDate > f.to) return false;
    if (keyword !== "" && !r.content.toLowerCase().includes(keyword)) return false;
    return true;
  });
}

/** 项目 id → 展示名；库里没有该项目时念「（项目缺失）」（同 todo-view.ts 的形制）。 */
function labelOf(projects: Project[], id: string): string {
  const p = projects.find((x) => x.id === id);
  return p ? projectLabel(p) : "（项目缺失）";
}

/// **页面的唯一入口**：内部先 scope 再聚合，所以「忘了缩」在结构上不可能发生（M⑥ 的靶就是这个函数改成吃全量）。
export function scopedHoursSummary(
  rows: DailyReport[],
  f: HoursFilter,
  bucket: HoursBucket,
  projects: Project[],
): HoursSummary {
  const scoped = scopeDailyReports(rows, f);
  const keyed = bucket === "day" ? dayKey : bucket === "week" ? isoWeekKey : monthKey;
  const buckets = new Map<string, DailyReport[]>();
  const byProject = new Map<string, DailyReport[]>();
  let unparsed = 0;

  for (const r of scoped) {
    if (!ISO_DATE.test(r.reportDate)) {
      unparsed += 1; // 形状不对：进不了任何桶，但要计数（判据②的形制）
      continue;
    }
    pushInto(buckets, keyed(r.reportDate), r);
    pushInto(byProject, r.projectId, r);
  }

  const valid = scoped.filter((r) => ISO_DATE.test(r.reportDate));
  return {
    buckets: [...buckets.entries()]
      .sort((a, b) => (a[0] < b[0] ? -1 : 1))
      .map(([key, rs]) => ({ key, hours: sumHours(rs), count: rs.length })),
    byProject: [...byProject.entries()]
      .map(([projectId, rs]) => ({
        projectId,
        projectLabel: labelOf(projects, projectId),
        hours: sumHours(rs),
        count: rs.length,
      }))
      // 组序按项目名的拼音序（`Intl.Collator("zh")`，同 todo-view.ts:54 的理由：码元序里「乙」在「甲」前）。
      .sort((a, b) => a.projectLabel.localeCompare(b.projectLabel, "zh")),
    total: sumHours(valid),
    count: valid.length,
    unparsed,
  };
}

function pushInto<T>(map: Map<string, T[]>, key: string, value: T): void {
  const list = map.get(key);
  if (list) list.push(value);
  else map.set(key, [value]);
}

/// 「复制上一天」的源日期：**所选日期之前最近一个有日报的那天**，不是机械的前一天
/// （D10：周一按前一天取会拿到周末的 0 条，等于按钮失灵）。返回 `null` = 之前一条都没有。
/// 只认形状合法的行：坏形状的日期不参与「哪天有日报」的判断。
export function previousReportDay(rows: DailyReport[], targetDate: string): string | null {
  let best: string | null = null;
  for (const r of rows) {
    if (!ISO_DATE.test(r.reportDate)) continue;
    if (r.reportDate >= targetDate) continue;
    if (best === null || r.reportDate > best) best = r.reportDate;
  }
  return best;
}
```

> **Step 4 里两处「故意这么写」别被顺手改掉**：① `scopedHoursSummary` 的 `total`/`count` 只在 `valid` 上算，因为 `unparsed` 那几行进不了桶，把它们算进合计会让「合计」与桶里数字对不上；② `pushInto` 是文件级私有 helper，不为它单独出测试 —— 它由上面每一格间接钉着。

Run（仓库根）: `node --test "tests/hours-summary.test.ts"`
Expected: 10 条全绿。再跑 `node --test "tests/**/*.test.ts"` → **tests 41 / pass 41 / fail 0**（基线 31 + 本格 10）。

- [ ] **Step 5: 类型门 + 提交**

```bash
cd /e/zero/demo/2026/project-files-manage && npm run build    # exit=0，`grep -c "error TS"` 为 0
node --test "tests/**/*.test.ts"                               # 41 / 41 / 0
```

```bash
git add src/types/daily-report.ts src/lib/hours-summary.ts src/lib/api.ts tests/hours-summary.test.ts
git commit -m "feat(m10): 前端契约层与工时聚合纯函数 —— types/daily-report.ts 七键对齐 Rust、api 四个包装（list 显式传 null），hours-summary.ts 落 ISO 周跨年三条边界（2026-01-01→W01、2026-12-31→W53、2027-01-01→2026-W53）、求和整数化（十条 0.1 断言得 1）、scopedHoursSummary 内部先缩后聚（M⑥ 靶，结构上让「忘缩」不可能）、previousReportDay 取最近有日报的那天而不是机械前一天（D10）；node 31→41"
```

---

### Task 5: `/daily-reports` 页（汇总 + 明细 + 筛选 + CRUD + 复制上一天）+ nav 第 6 格

**Files:**
- Create: `src/pages/daily-reports.tsx`
- Modify: `src/router.tsx:47`（`todosRoute` 之后插路由）、`src/router.tsx:49-56`（`addChildren` 末尾追加）、`src/router.tsx` 顶部 `import`
- Modify: `src/components/app-shell.tsx:13`（nav 第 6 格）
- Test: 无自动化 —— 本格的网是 `tsc` + `vite build` + Task 8 的真机十六格。**这条要在派发词里明写**（M9 终审 N7 记的同族：判据搬前端 ⇒ `cargo test` 不再守住它，取证面换到 `node --test`；而**页面接线本身连 `node --test` 也够不着**，只能人眼与真机）

**Interfaces:**
- Consumes: Task 4 的 `listDailyReports/createDailyReport/updateDailyReport/deleteDailyReport`；`hours-summary.ts` 的 `scopedHoursSummary/scopeDailyReports/previousReportDay/HoursBucket/HoursFilter`；`due-warning.ts` 的 `localToday()`（唯一时钟读取者）与 `projectLabel()`（显示名唯一形状）；`ipc.ts` 的 `toAppError`。
- Produces: 路由 `dailyReportsRoute`（`/daily-reports`）与组件 `DailyReportsPage`；Task 6/7 往本页的 `明细` 段尾部加导出/导入按钮。

- [ ] **Step 1: 建页面**

```tsx
// 日报管理：跨项目的工作量填报（需求 思维导图.md:47-59）。汇总在上、明细在下（spec §七）。
// 数据一次拉全（listDailyReports(null) + listProjects()）：沿用待办页口径 —— 筛选、排序、聚合都在前端，
// 比按视图来回打后端简单也更省往返（`daily_report_list` 不带 LIMIT，先例 `todo::list`）。
import { useCallback, useEffect, useMemo, useState } from "react";
import { Copy, Plus, Trash2 } from "lucide-react";
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
  createDailyReport,
  deleteDailyReport,
  listDailyReports,
  listProjects,
  updateDailyReport,
} from "@/lib/api";
import { localToday, projectLabel } from "@/lib/due-warning";
import {
  previousReportDay,
  scopeDailyReports,
  scopedHoursSummary,
  type HoursBucket,
  type HoursFilter,
} from "@/lib/hours-summary";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { dateOrNull, type DailyReport, type DailyReportEdit, type DailyReportInput } from "@/types/daily-report";
import type { Project } from "@/types/project";

const ALL_PROJECTS = "__all__";

const BUCKETS: { key: HoursBucket; label: string }[] = [
  { key: "day", label: "按日" },
  { key: "week", label: "按周" },
  { key: "month", label: "按月" },
];

export function DailyReportsPage() {
  const [reports, setReports] = useState<DailyReport[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // 一份筛选同时喂汇总与明细：`scopedHoursSummary` 内部先缩再聚，页面没有第二份比较可写错（spec §七）。
  const [filterProject, setFilterProject] = useState(ALL_PROJECTS);
  const [filterFrom, setFilterFrom] = useState("");
  const [filterTo, setFilterTo] = useState("");
  const [keyword, setKeyword] = useState("");
  const [bucket, setBucket] = useState<HoursBucket>("week");

  const [newDate, setNewDate] = useState(() => localToday());
  const [newProject, setNewProject] = useState("");
  const [newContent, setNewContent] = useState("");
  const [newHours, setNewHours] = useState("");

  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftDate, setDraftDate] = useState("");
  const [draftProject, setDraftProject] = useState("");
  const [draftContent, setDraftContent] = useState("");
  const [draftHours, setDraftHours] = useState("");

  const reload = useCallback(async () => {
    try {
      const [rows, ps] = await Promise.all([listDailyReports(null), listProjects()]);
      setReports(rows);
      setProjects(ps);
      setError(null);
    } catch (e) {
      setError(toAppError(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const labelOf = useMemo(() => {
    // 显示名只有一份形状（due-warning 的 projectLabel，有编号时是「名（编号）」），这里不重拼。
    const byId = new Map(projects.map((p) => [p.id, projectLabel(p)]));
    return (id: string) => byId.get(id) ?? "（项目缺失）";
  }, [projects]);

  const filter = useMemo<HoursFilter>(
    () => ({
      projectId: filterProject === ALL_PROJECTS ? null : filterProject,
      from: filterFrom.trim(),
      to: filterTo.trim(),
      keyword,
    }),
    [filterProject, filterFrom, filterTo, keyword],
 );
  const summary = useMemo(
    () => scopedHoursSummary(reports, filter, bucket, projects),
    [reports, filter, bucket, projects],
  );
  const visible = useMemo(() => scopeDailyReports(reports, filter), [reports, filter]);

  // 复制上一天的源日期：最近一个有日报的那天（D10）。null ⇒ 按钮禁用。
  const targetDate = dateOrNull(newDate) ?? localToday();
  const copySource = useMemo(() => previousReportDay(reports, targetDate), [reports, targetDate]);

  // run 必须回报成败：它把异常吞进 state、promise 永不 reject，任何 `.then(清理)` 都会无条件执行
  // —— M9 的 I3（失败的新增照样清空输入、连点两次插重复行）就是这么来的。调用方一律 `if (ok)` 再清理。
  const run = async (fn: () => Promise<unknown>): Promise<boolean> => {
    setBusy(true);
    try {
      await fn();
      await reload();
      return true;
    } catch (e) {
      setError(toAppError(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  // `Number("abc")` 是 NaN，而 `JSON.stringify(NaN)` 出 `null` ⇒ IPC 层会报「参数不合法」那种看不懂的错。
  // 折成 0 让它走进后端 `check_hours`（0 不合法），报出来就点名「工时」—— 判据仍然只在后端一处。
  const hoursOf = (raw: string): number => {
    const n = Number(raw.trim());
    return Number.isFinite(n) ? n : 0;
  };

  const onAdd = () => {
    if (newProject === "") {
      // 这不算「第二份校验」，而是「别把一个空 id 发出去」：`projectId: ""` 到后端报的是
      // `not_found()`，用户看不出是自己没选项目。（`AppErrorShape.hint` 是 `string | null`，不是可选键。）
      setError({ code: "invalid_input", message: "要先选所属项目", hint: "日报必须挂一个项目" });
      return;
    }
    const date = dateOrNull(newDate);
    const input: DailyReportInput = {
      projectId: newProject,
      reportDate: date,
      content: newContent,
      hours: hoursOf(newHours),
    };
    // 只清内容与工时：日期与项目是「正在填的上下文」，留着才连得上 §三 的连续填报。
    // 成功回执写在 `run` 的 fn 里 ⇒ create 抛错时根本走不到这几行（M9 I3 的规矩）。
    void run(async () => {
      await createDailyReport(input);
      setNewContent("");
      setNewHours("");
      setNotice(`已记一条 ${date ?? "（日期没填，这条会被后端拒）"} 的日报`);
    });
  };

  const onCopyPreviousDay = () => {
    const source = copySource;
    if (source === null) return; // 按钮此时 disabled，这一句只是防御
    const src = reports.filter((r) => r.reportDate === source);
    void (async () => {
      // **逐条走 `run()`，不走「一个 run 里循环」**：`run()` 只在成功路径里 `await reload()`，
      // 循环版一旦中途被后端拒绝（或落库后 reload 自己失败），已经进库的那几条就**不会回到列表里** ——
      // 用户看不见它们，既没法按 D10 说的「逐条删」撤销，也会再点一次「复制上一天」把同一批**再复制一遍**。
      // Task 5 复审抓的就是这一条（单条写入的 `todos.tsx` 不会摊上这事，多条循环才会）。
      // 代价是 N 次 reload，几十条量级无所谓；`copied` 只在自己 resolve 之后才加，所以不会多报。
      let copied = 0;
      for (const r of src) {
        const ok = await run(() =>
          createDailyReport({
            projectId: r.projectId,
            reportDate: targetDate,
            content: r.content,
            hours: r.hours,
          }),
        );
        if (!ok) break; // 失败已由 run() 落进 error，且前面成功的几条已经被 run() 内部的 reload 拉回列表
        copied += 1;
      }
      if (copied > 0) setNotice(`从 ${source} 复制了 ${copied} 条到 ${targetDate}`);
    })();
  };

  const onStartEdit = (r: DailyReport) => {
    setEditingId(r.id);
    setDraftDate(r.reportDate);
    setDraftProject(r.projectId);
    setDraftContent(r.content);
    setDraftHours(String(r.hours));
    setNotice(null);
  };

  const onSaveEdit = (id: string) => {
    const edit: DailyReportEdit = {
      projectId: draftProject,
      reportDate: dateOrNull(draftDate),
      content: draftContent,
      hours: hoursOf(draftHours),
    };
    void run(async () => {
      await updateDailyReport(id, edit);
      setEditingId(null); // 只有成功才退出编辑态（M9 I3 同规）
    });
  };

  const onDelete = (r: DailyReport) => {
    void run(() => deleteDailyReport(r.id));
  };

  return (
    <div className="space-y-4">
      <section className="rounded-xl border bg-background p-4">
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <h2 className="text-sm font-medium">日报</h2>
          <span className="text-xs text-muted-foreground">
            共 {reports.length} 条 · 当前范围 {summary.count} 条 / {summary.total} 小时
          </span>
          <div className="ml-auto flex items-center gap-2">
            {BUCKETS.map((b) => (
              <Button
                key={b.key}
                size="sm"
                variant={bucket === b.key ? "default" : "outline"}
                onClick={() => setBucket(b.key)}
              >
                {b.label}
              </Button>
            ))}
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Select value={filterProject} onValueChange={setFilterProject}>
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
          <Input
            className="w-36"
            type="date"
            value={filterFrom}
            onChange={(e) => setFilterFrom(e.target.value)}
            aria-label="起始日期"
          />
          <Input
            className="w-36"
            type="date"
            value={filterTo}
            onChange={(e) => setFilterTo(e.target.value)}
            aria-label="结束日期"
          />
          <Input
            className="w-48"
            placeholder="工作内容关键词"
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
          />
        </div>
        {summary.unparsed > 0 && (
          <p className="mt-2 text-xs text-destructive">
            {summary.unparsed} 条日期不是 YYYY-MM-DD，没有进汇总（界面上仍能在明细里看到它们）
          </p>
        )}
      </section>

      {error && (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}
      {notice && !error && (
        <div className="rounded-lg border bg-muted px-3 py-2 text-xs text-muted-foreground">{notice}</div>
      )}

      <section className="rounded-xl border bg-background p-4">
        <h2 className="text-sm font-medium">工时汇总</h2>
        <table className="mt-2 w-full text-sm">
          <thead className="text-left text-xs text-muted-foreground">
            <tr>
              <th className="py-1">{BUCKETS.find((b) => b.key === bucket)?.label}</th>
              <th className="py-1">条目数</th>
              <th className="py-1">小时</th>
            </tr>
          </thead>
          <tbody>
            {summary.buckets.length === 0 ? (
              <tr>
                <td colSpan={3} className="py-2 text-muted-foreground">
                  当前范围里没有可汇总的日报。
                </td>
              </tr>
            ) : (
              summary.buckets.map((b) => (
                <tr key={b.key} className="border-t">
                  <td className="py-1 font-mono text-xs">{b.key}</td>
                  <td className="py-1">{b.count}</td>
                  <td className="py-1">{b.hours}</td>
                </tr>
              ))
            )}
          </tbody>
        </table>
        <Separator className="my-3" />
        <h3 className="text-sm font-medium">按项目</h3>
        <ul className="mt-2 space-y-1">
          {summary.byProject.length === 0 ? (
            <li className="text-sm text-muted-foreground">当前范围里没有可汇总的日报。</li>
          ) : (
            summary.byProject.map((p) => (
              <li key={p.projectId} className="flex items-center gap-2 text-sm">
                <Badge variant="outline">{p.hours} h</Badge>
                <span className="min-w-0 flex-1 truncate">{p.projectLabel}</span>
                <span className="text-xs text-muted-foreground">{p.count} 条</span>
              </li>
            ))
          )}
        </ul>
      </section>

      <section className="rounded-xl border bg-background p-4">
        <h2 className="text-sm font-medium">明细</h2>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <Input
            className="w-36"
            type="date"
            value={newDate}
            onChange={(e) => setNewDate(e.target.value)}
            aria-label="日报日期"
          />
          <Select value={newProject} onValueChange={setNewProject}>
            <SelectTrigger className="w-48">
              <SelectValue placeholder="所属项目" />
            </SelectTrigger>
            <SelectContent>
              {projects.map((p) => (
                <SelectItem key={p.id} value={p.id}>
                  {labelOf(p.id)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Input
            className="min-w-48 flex-1"
            placeholder="工作内容"
            value={newContent}
            onChange={(e) => setNewContent(e.target.value)}
          />
          <Input
            className="w-24"
            inputMode="decimal"
            placeholder="小时"
            value={newHours}
            onChange={(e) => setNewHours(e.target.value)}
            aria-label="工时"
          />
          <Button size="sm" disabled={busy || newProject === ""} onClick={onAdd}>
            <Plus className="mr-1 size-4" />
            新增
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={busy || copySource === null}
            onClick={onCopyPreviousDay}
            title={copySource === null ? "之前还没有任何一天记过日报" : `复制 ${copySource}`}
          >
            <Copy className="mr-1 size-4" />
            复制上一天
          </Button>
        </div>

        <ul className="mt-3 space-y-1">
          {visible.length === 0 ? (
            <li className="py-2 text-sm text-muted-foreground">
              {reports.length === 0 ? "还没有日报。" : "当前筛选条件下没有日报。"}
            </li>
          ) : (
            visible.map((r) => {
              const editing = editingId === r.id;
              return (
                <li key={r.id} className="flex items-center gap-2 border-b py-2 last:border-b-0">
                  {editing ? (
                    <>
                      <Input
                        className="w-36"
                        type="date"
                        value={draftDate}
                        onChange={(e) => setDraftDate(e.target.value)}
                        aria-label="日报日期"
                      />
                      <Select value={draftProject} onValueChange={setDraftProject}>
                        <SelectTrigger className="w-48">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {projects.map((p) => (
                            <SelectItem key={p.id} value={p.id}>
                              {labelOf(p.id)}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                      <Input
                        className="min-w-48 flex-1"
                        value={draftContent}
                        onChange={(e) => setDraftContent(e.target.value)}
                      />
                      <Input
                        className="w-24"
                        inputMode="decimal"
                        value={draftHours}
                        onChange={(e) => setDraftHours(e.target.value)}
                        aria-label="工时"
                      />
                      <Button size="sm" disabled={busy} onClick={() => onSaveEdit(r.id)}>
                        保存
                      </Button>
                      <Button size="sm" variant="outline" onClick={() => setEditingId(null)}>
                        取消
                      </Button>
                    </>
                  ) : (
                    <>
                      <span className="font-mono text-xs text-muted-foreground">{r.reportDate}</span>
                      <Badge variant="outline">{r.hours} h</Badge>
                      <span className="min-w-0 flex-1 truncate">{r.content}</span>
                      <span className="max-w-40 truncate text-xs text-muted-foreground">
                        {labelOf(r.projectId)}
                      </span>
                      <Button size="sm" variant="ghost" disabled={busy} onClick={() => onStartEdit(r)}>
                        编辑
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={busy}
                        onClick={() => onDelete(r)}
                        aria-label="删除这条日报"
                      >
                        <Trash2 className="size-4" />
                      </Button>
                    </>
                  )}
                </li>
              );
            })
          )}
        </ul>
      </section>
    </div>
  );
}
```

> **两处故意不做的校验**（别顺手加）：① 页面不检查日期形状与工时范围，判据只在后端一处（`req_date`/`check_hours`），界面自己先拦会漂成两套口径；② `onAdd` 里只挡「没选项目」这一种 —— 因为那种情况下发到后端的 `projectId: ""` 报的是 `not_found(项目 id)`，用户看不出是自己没选，属界面失职。`Select` 初值空串与 `ALL_PROJECTS` 那个哨兵不同，不能混用。
>
> **新增行的 `Select` 就写 `value={newProject}`，不许改成 `newProject === "" ? undefined : newProject`**（Task 5 复审打回的第三样东西，我核过 vendored 源码）：Radix 的 `shouldShowPlaceholder(value)` 是 `value === "" || value === void 0`（`node_modules/@radix-ui/react-select/dist/index.mjs:1140-1142`），空串**照样**显示 placeholder；而 `undefined` 会让它按 `isControlled = prop !== void 0` 判定成非受控，于是「挂载时非受控 → 选完变受控」，dev 构建里就有 Radix 那条 uncontrolled→controlled 警告；更坏的是将来任何「保存后重置成空串」的写法会让 trigger 显示上一次选的项目而 state 是空串（界面与状态分叉）。同仓先例是 `todos.tsx:271` 的 `value={newProject}` 配 `placeholder="选择项目"`，本文件上方的筛选器用的是 `ALL_PROJECTS` 哨兵，两者不同用途。

- [ ] **Step 2: 接路由与 nav**

`src/router.tsx`：在 `todosRoute`（`:43-47`）之后加：

```tsx
const dailyReportsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/daily-reports",
  component: DailyReportsPage,
});
```

`addChildren([...])` 里 `todosRoute,` 之后追加 `dailyReportsRoute,`。顶部 `import` 区按现有形制加 `import { DailyReportsPage } from "@/pages/daily-reports";`（与 `TodosPage` 那行相邻、保持字母序）。

`src/components/app-shell.tsx:13` 之后：

```tsx
  { to: "/todos", label: "待办" },
  { to: "/daily-reports", label: "日报" },
```

- [ ] **Step 3: 类型与构建门**

```bash
cd /e/zero/demo/2026/project-files-manage
npm run build                       # exit=0，且 npm run build 输出里 `error TS` 计数为 0
node --test "tests/**/*.test.ts"    # 41 / 41 / 0（本格不加测试，这条是防手滑改坏纯函数）
cd src-tauri && cargo test -j 1 --lib   # 279 passed（本格没动 Rust）
```

- [ ] **Step 4: 提交**

```bash
cd /e/zero/demo/2026/project-files-manage
git add src/pages/daily-reports.tsx src/router.tsx src/components/app-shell.tsx
git commit -m "feat(m10): /daily-reports 页 + nav 第 6 格 —— 汇总在上（日/周/月三档 + 按项目，数字只认筛选后的行）、明细在下（日期/项目/关键词/区间一份筛选喂两处）、CRUD 挂 run() 的布尔回执（失败不清输入、不退出编辑态，M9 I3 同规），复制上一天按「最近一个有日报的那天」取源并在按钮 disabled 与 title 里念出源日期（D10）；页面不做第二份日期与工时校验（判据只在后端一处）"
```

---

### Task 6: Excel 导出 —— `write_xlsx` + 本轮第一条 async 命令（全仓第 5 条）

**Files:**
- Modify: `src-tauri/Cargo.toml`（`rust_xlsxwriter` 从 `[dev-dependencies]` 提到 `[dependencies]`）
- Modify: `src-tauri/src/daily_report.rs`（加 `EXPORT_HEADERS`、`ExportRow`、`write_fail`、`write_xlsx` + 4 条测试）
- Modify: `src-tauri/src/backup.rs:202`（那句 hint 里的「备份包」改成「导出文件」，Step 5）
- Modify: `src-tauri/src/lib.rs`（`use` 补 `ExportRow` + `daily_report_export` async + 注册表 57→58）
- Modify: `src/types/daily-report.ts`、`src/lib/api.ts`、`src/pages/daily-reports.tsx`

**Interfaces:**
- Consumes: `backup::guard_destination(&Path, &Path)`（`backup.rs:162`，`pub fn`）；`fs_failed` 的码与文案（`error.rs:21` 的 `AppError::io`）；`rust_xlsxwriter` 的 `Workbook::add_worksheet()`（`workbook.rs:514`）、`Worksheet::write(row: u32, col: u16, impl IntoExcelData)`（`worksheet.rs:2149`）、`write_with_format(row, col, data, &Format)`（`:2209`）、`set_name(impl Into<String>)`（`:2040`）、`set_freeze_panes(row: u32, col: u16)`（`:13241`，**0.99 里不叫 `freeze_panes`**）、`Workbook::save<P: AsRef<Path>>`（`workbook.rs:1319`）。
- Produces: `pub const daily_report::EXPORT_HEADERS: [&str; 5]`（**Task 7 的列定位读的就是这一个常量**）、`pub struct daily_report::ExportRow`、`pub fn daily_report::write_xlsx(path: &Path, rows: &[ExportRow]) -> AppResult<usize>`；命令 `daily_report_export(path, rows)`；前端 `pickXlsxSavePath(defaultName)`、`exportDailyReports(path, rows)`、`DailyReportExportRow`。

- [ ] **Step 1: 提依赖一级（先做，否则测试编不过）**

`src-tauri/Cargo.toml`：把 `[dev-dependencies]` 段里的 `rust_xlsxwriter = "0.99.1"` **删掉**，在 `[dependencies]` 段 `sha2 = "0.11"` 那行之前插入（注释形制照 `sha2` 那三行）：

```toml
# M10 日报导出：写 .xlsx。这个 crate 早在 Cargo.lock（extract.rs:419 的 xlsx 测试夹具拉它），
# 从 dev-dependencies 提为直接依赖不新增下载、也不新增编译单元 —— 与上面 sha2 同一格的处理。
# 不许改成「自己手写 SpreadsheetML」：仓里已经有 zip 8.6.0 与 quick-xml，但手写 sharedStrings、
# 日期格式、列宽这些的坑比换库的坑难查得多。
rust_xlsxwriter = "0.99.1"
```

```bash
cd /e/zero/demo/2026/project-files-manage
grep -c '^\[\[package\]\]' src-tauri/Cargo.lock   # 期望仍 616：提一级不新增包
cd src-tauri && cargo build --lib                 # 期望 exit=0（extract.rs 的测试夹具仍能用它：依赖对测试可见）
```

- [ ] **Step 2: 写 4 条失败测试（RED 在编译层）**

`daily_report.rs` 的 `mod tests` 末尾追加（`tempfile` 已在 `[dev-dependencies]`）：

```rust
    /// 读回用 calamine（与本任务的写同一把库），**不经 `extract::sheet_text`**：
    /// 那个函数把所有格子拼成一串文本，看不出「这一格是日期单元还是文本」，而那是 §六.1 的判据。
    ///
    /// **但 `to_string()` 也分不出类型**（控制方按包缓存核过）：`Data::Float(7.5)` 与
    /// `Data::String("7.5")` 都印成 `7.5`（`calamine-0.36.1/src/datatype.rs:212-216`），
    /// `Data::DateTime(e)` 走 `ExcelDateTime` 的 Display，而它印的就是裸 `self.value`
    /// （`datatype.rs:986-990`）—— 序列号 `46311`，**不带时分秒**。
    /// 所以凡要钉「这一格是数字还是文本」的判据，必须另按**变体**断（下面的 `read_typed`），
    /// 只比字符串会得到一条打不红的假守护（Task 6 复审就是这么打回第一版的）。
    fn read_cells(path: &std::path::Path) -> Vec<Vec<String>> {
        use calamine::Reader as _;
        let mut book = calamine::open_workbook_auto(path).unwrap();
        let names = book.sheet_names();
        let range = book.worksheet_range(&names[0]).unwrap();
        range.rows().map(|r| r.iter().map(|c| c.to_string()).collect()).collect()
    }

    /// 按**变体**读回：字符串读法证不了类型，这一支才证得了（复审 Important）。
    fn read_typed(path: &std::path::Path) -> Vec<Vec<calamine::Data>> {
        use calamine::Reader as _;
        let mut book = calamine::open_workbook_auto(path).unwrap();
        let names = book.sheet_names();
        let range = book.worksheet_range(&names[0]).unwrap();
        range.rows().map(|r| r.to_vec()).collect()
    }

    /// calamine 对数值单元可能给 `Float` 也可能给 `Int`（取决于它解析出什么），
    /// 所以「是数字」的判据走 `DataType::as_f64()`，而不是 `matches!(.., Data::Float(_))`。
    fn is_numeric(d: &calamine::Data) -> bool {
        use calamine::DataType as _;
        d.as_f64().is_some() && !matches!(d, calamine::Data::String(_))
    }

    /// 工作表名也要能断到（`read_cells`/`read_typed` 都是盲取 `names[0]`）。
    fn first_sheet_name(path: &std::path::Path) -> String {
        use calamine::Reader as _;
        let book = calamine::open_workbook_auto(path).unwrap();
        book.sheet_names().first().cloned().unwrap_or_default()
    }

    fn export_row(name: &str, code: Option<&str>) -> ExportRow {
        ExportRow {
            report_date: "2026-10-16".to_owned(),
            project_code: code.map(|s| s.to_owned()),
            project_name: name.to_owned(),
            content: "联调第三方接口".to_owned(),
            hours: 7.5,
        }
    }

    #[test]
    fn export_writes_a_real_xlsx_that_calamine_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("out.xlsx");
        let n = write_xlsx(&p, &[export_row("甲项目", Some("P-01")), export_row("乙项目", None)]).unwrap();
        assert_eq!(n, 2);
        let cells = read_cells(&p);
        assert_eq!(cells[0].join("|"), "日期|项目编号|所属项目|工作内容|时长（小时）", "表头五列");
        assert_eq!(cells.len(), 3, "表头 + 两行数据");
        assert_eq!(cells[1].join("|"), "2026-10-16|P-01|甲项目|联调第三方接口|7.5");
        assert_eq!(cells[2][1], "", "编号为 None 要落成空格，不是字面 \"None\"");
        // **按变体**钉两条刻意决定的类型（复审 Important：只比字符串这两条都打不红）：
        // 工时要是数字单元（用户在 Excel 里要能求和），日期要文本（这条环不经过序列号）。
        let typed = read_typed(&p);
        assert!(is_numeric(&typed[1][4]), "工时列必须是数字单元，实际：{:?}", typed[1][4]);
        assert!(
            matches!(typed[1][0], calamine::Data::String(_)),
            "日期列必须是文本单元，实际：{:?}",
            typed[1][0]
        );
        // 工作表名也是 spec 钉住的值（§六.1「工作表名 日报」），Task 7 只读第一个 sheet，名字错了没人报。
        assert_eq!(first_sheet_name(&p), "日报");
    }

    /// §六.1：日期写成**文本**。钉的是「导出 → 用户编辑 → 再导回来」这条环不经过 Excel 序列号。
    /// **形状分不出来**（Task 6 复审核包缓存改准了原先这句：`ExcelDateTime` 的 `Display` 印的是裸
    /// `self.value`，也就是序列号 `46311`，与 `Data::Float` 的印法一样，`datatype.rs:986-990`）——
    /// 字符串比对在这里确实能红（`2026-10-16` ≠ `46311`），但**类型要靠上面那条 `matches!(.., Data::String(_))`**。
    #[test]
    fn export_writes_the_date_as_text_not_a_date_cell() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.xlsx");
        write_xlsx(&p, &[export_row("甲项目", None)]).unwrap();
        assert_eq!(read_cells(&p)[1][0], "2026-10-16", "要读回纯日期文本，不许是日期单元的序列化形状");
    }

    /// 「下载模板」= 传空 rows：表头必须在，否则用户填无可填、导入必然报「缺列」。
    #[test]
    fn export_with_no_rows_still_writes_the_five_headers() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("template.xlsx");
        assert_eq!(write_xlsx(&p, &[]).unwrap(), 0);
        let cells = read_cells(&p);
        assert_eq!(cells.len(), 1, "模板只有表头一行");
        assert_eq!(cells[0].len(), EXPORT_HEADERS.len());
    }

    /// 写不出去按 `fs_failed` 报（`error.rs:21` 的既有码与文案），不许冒成 invalid_input 说「你输入错了」。
    #[test]
    fn write_xlsx_reports_fs_failed_when_the_directory_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("no-such-dir").join("out.xlsx");
        let err = write_xlsx(&p, &[export_row("甲项目", None)]).unwrap_err();
        assert_eq!(err.code, "fs_failed", "落点不可写是文件域，不是校验域：{}", err.message);
        assert!(err.message.contains("失败"), "{}", err.message);
    }
```

Run: `cd /e/zero/demo/2026/project-files-manage/src-tauri && cargo test -j 1 --lib daily_report`
Expected: 编译失败（`cannot find type ExportRow`、`cannot find function write_xlsx`）。编译层的红，理由同 Task 2 Step 3。

- [ ] **Step 3: 实现**

`daily_report.rs` 顶部 `use` 区加一行（本任务起需要 `Path`）：

```rust
use std::path::Path;
```

在 `delete` 之后追加：

```rust
/// 表头五列 —— 导出写它、导入按它定位列，**全仓只有这一处**（spec §六）。改一个词会同时打掉两格的
/// 测试：Task 6 的表头断言与 Task 7 的列定位断言读的都是这个常量。
pub const EXPORT_HEADERS: [&str; 5] = ["日期", "项目编号", "所属项目", "工作内容", "时长（小时）"];

/// 前端把**当前筛选后的行**传回来（spec §六.1：命令不收查询条件，免得在 SQL 里再造第二份日期比较）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRow {
    pub report_date: String,
    /// `projects.code` 是可空列，原样给出；导入侧按它优先匹配，为空则退到名称。
    pub project_code: Option<String>,
    /// **必须是 `projects.name` 原值**，不是界面上的 `projectLabel`（后者拼了编号就对不上）。
    pub project_name: String,
    pub content: String,
    pub hours: f64,
}

/// 写 xlsx 的失败按文件域报：码与文案对齐 `error.rs:21` 的 `AppError::io`。
/// 不直接调 `io()` 是因为 `rust_xlsxwriter` 的错误不是 `std::io::Error`，硬套会丢掉它自己那句话。
fn write_fail(path: &Path, e: impl std::fmt::Display) -> AppError {
    AppError::new(
        "fs_failed",
        &format!("写 {} 失败：{e}", path.display()),
        Some("确认该路径存在且可读写"),
    )
}

/// 写一份真 `.xlsx`。0 行数据也写出表头 —— 那就是「下载模板」。返回落地的数据行数。
pub fn write_xlsx(path: &Path, rows: &[ExportRow]) -> AppResult<usize> {
    use rust_xlsxwriter::{Format, Workbook};

    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("日报").map_err(|e| write_fail(path, e))?;
    ws.set_freeze_panes(1, 0).map_err(|e| write_fail(path, e))?;
    let bold = Format::new().set_bold();
    for (col, head) in EXPORT_HEADERS.iter().enumerate() {
        ws.write_with_format(0, col as u16, *head, &bold)
            .map_err(|e| write_fail(path, e))?;
    }
    for (i, r) in rows.iter().enumerate() {
        let row = (i + 1) as u32;
        ws.write(row, 0, r.report_date.as_str()).map_err(|e| write_fail(path, e))?;
        ws.write(row, 1, r.project_code.as_deref().unwrap_or(""))
            .map_err(|e| write_fail(path, e))?;
        ws.write(row, 2, r.project_name.as_str()).map_err(|e| write_fail(path, e))?;
        ws.write(row, 3, r.content.as_str()).map_err(|e| write_fail(path, e))?;
        // 工时写成数字（用户在 Excel 里要能求和），日期写成文本（上面那行）—— 两种类型是故意的，
        // 别顺手把日期也写成日期单元。
        ws.write(row, 4, r.hours).map_err(|e| write_fail(path, e))?;
    }
    wb.save(path).map_err(|e| write_fail(path, e))?;
    Ok(rows.len())
}
```

Run: `cd /e/zero/demo/2026/project-files-manage/src-tauri && cargo test -j 1 --lib daily_report`
Expected: 聚焦过滤 `--lib daily_report` 会连带匹配 `db.rs`/`search.rs` 里含 `daily_report` 的既有测试，**实测 22 条**（本格新增 4 条 + Task 2 的 16 条 + 另两处同名各 1 条）；全量 `cargo test -j 1 --lib` 应为 **283 passed**。别把聚焦数当聚合数用。

- [ ] **Step 4: 第三条 async 命令与注册表**

`lib.rs` 的 `use crate::daily_report::{...}` 那行补 `ExportRow`：

```rust
use crate::daily_report::{DailyReport, DailyReportEdit, DailyReportInput, ExportRow};
```

在四条 `daily_report_*` 同步命令之后追加：

```rust
/// 日报导出（M10 Task 6，spec §六.1）：挂 async 属性、函数体不写 await 点 —— 写整个 xlsx 是文件 IO，
/// 不该压在消息泵那一个线程上（与备份那三条同一个理由，见本文件上方那条备份命令）。
/// **不取 `db(&state)?`**：要导出的行由前端传回来（判据只有一处），所以这条与索引 job、
/// 与其它读写完全并行，不占那把全局 Mutex。
#[tauri::command(async)]
fn daily_report_export(
    state: State<'_, AppState>,
    path: String,
    rows: Vec<ExportRow>,
) -> AppResult<usize> {
    // 落点守卫必须早于任何文件写：`File::create` 是 CREATE_ALWAYS 截断，把落点指进数据目录
    // 会覆掉活库或那次导入的撤销键（`backup_export` 里同一条注释）。
    backup::guard_destination(&state.data_dir, std::path::Path::new(&path))?;
    daily_report::write_xlsx(std::path::Path::new(&path), &rows)
}
```

注册表：给 `daily_report_delete` 补逗号，追加 `daily_report_export` 作末项（**不带逗号**）。

- [ ] **Step 5: 把 `guard_destination` 那句 hint 的「备份包」改准**

`backup.rs:202` 的 hint 现在写「请把备份包保存到数据目录以外的位置」，而这条守卫从今天起也管日报导出。整句换成：

```rust
            Some("请把导出文件保存到数据目录以外的位置（数据目录里是活库和应急包，覆盖会毁掉它们）"),
```

**这一句没有任何测试断言**（`backup.rs:1296`/`:1389` 只断 `e.code`，控制方已 grep 确认全仓无消费者按这句话取数），所以不需要配套改测试；也别为了「保险」新加一条断言文案的测试 —— 那会让下轮改文案变成破测试。

- [ ] **Step 6: 前端接线**

`src/types/daily-report.ts` 末尾：

```ts
/// 与 Rust 的 ExportRow 对齐。`projectName` 必须是 `Project.name` 原值 ——
/// 它是导入侧名称匹配的对象，写成 `projectLabel`（拼了编号）会导出去再导回来对不上。
export interface DailyReportExportRow {
  reportDate: string;
  projectCode: string | null;
  projectName: string;
  content: string;
  hours: number;
}
```

`src/lib/api.ts`（照 M8 那两个对话框封装的形制；**取消返回 null，不是错误**）：

```ts
// —— M10 日报导出 ——
export async function pickXlsxSavePath(defaultName: string): Promise<string | null> {
  const picked = await saveDialog({
    defaultPath: defaultName,
    filters: [{ name: "Excel 工作簿", extensions: ["xlsx"] }],
  });
  return typeof picked === "string" ? picked : null;
}

export const exportDailyReports = (path: string, rows: DailyReportExportRow[]) =>
  invoke<number>("daily_report_export", { path, rows });
```

并在 `:20-21` 那片 `import type` 里补 `DailyReportExportRow`（同 `@/types/daily-report` 那一行）。

`src/pages/daily-reports.tsx`：`import` 区补 `exportDailyReports, pickXlsxSavePath` 与 `type DailyReportExportRow`；在 `onCopyPreviousDay` 之后加：

```tsx
  const projectById = useMemo(() => new Map(projects.map((p) => [p.id, p])), [projects]);

  // 导出**所见即所得**：`visible` 是筛过的行（spec §六.1，命令不收查询条件）。
  // 名称列写 `p.name` 而不是界面上的 label —— 它是导入匹配的对象。
  const onExport = (templateOnly: boolean) => {
    const defaultName = templateOnly ? "日报模板.xlsx" : `日报-${localToday().replace(/-/g, "")}.xlsx`;
    void (async () => {
      const path = await pickXlsxSavePath(defaultName);
      if (path === null) return; // 取消不是错误：绝不写进 error（M8 的同一告诫）
      const rows: DailyReportExportRow[] = (templateOnly ? [] : visible).map((r) => {
        const p = projectById.get(r.projectId);
        return {
          reportDate: r.reportDate,
          projectCode: p?.code ?? null,
          projectName: p?.name ?? "（项目缺失）",
          content: r.content,
          hours: r.hours,
        };
      });
      const ok = await run(() => exportDailyReports(path, rows));
      if (ok) setNotice(`已导出 ${rows.length} 条到 ${path}`);
    })();
  };
```

「复制上一天」那两个按钮之后加：

```tsx
          <Button size="sm" variant="outline" disabled={busy} onClick={() => onExport(false)}>
            导出当前范围
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => onExport(true)}>
            下载模板
          </Button>
```

- [ ] **Step 7: 门禁**

```bash
cd /e/zero/demo/2026/project-files-manage/src-tauri
cargo test -j 1 --lib                        # 期望 283 passed（279 + 4）
cargo clippy --lib -- -D warnings            # exit=0
cargo clippy --lib --all-targets -- -D warnings   # exit=0
cd /e/zero/demo/2026/project-files-manage
npm run build                               # exit=0，输出里 error TS = 0
node --test "tests/**/*.test.ts"            # 41 / 41 / 0
sed -n '/generate_handler/,/^      \]/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"   # 期望 58
grep -c 'tauri::command(async)' src-tauri/src/lib.rs   # 期望 5（4→5）
grep -c '^\[\[package\]\]' src-tauri/Cargo.lock        # 期望 616
```

- [ ] **Step 8: 提交**

```bash
cd /e/zero/demo/2026/project-files-manage
git add src-tauri/Cargo.toml src-tauri/src/daily_report.rs src-tauri/src/lib.rs src-tauri/src/backup.rs src/types/daily-report.ts src/lib/api.ts src/pages/daily-reports.tsx
git commit -m "feat(m10): 日报导出真 .xlsx + 第一条 async 命令（57→58、async 4→5）—— rust_xlsxwriter 从 dev 提直接依赖（Cargo.lock 616 不变）、EXPORT_HEADERS 五列全仓只一处、日期写成文本而工时写成数字（导出→编辑→导回来这条环不经过 Excel 序列号）、所属项目列写 name 原值不写 projectLabel（那是名称匹配对象）、命令不收查询条件而收前端筛好的行（避免 SQL 里第二份日期比较）、落点先过 backup::guard_destination 再碰文件；写失败按 error.rs:21 的 fs_failed 报，并把该守卫 hint 里的「备份包」改准成「导出文件」（已 grep 确认全仓无测试按那句取数）"
```

---

### Task 7: Excel 导入 —— `read_rows` + 预检 + 事务落库（两段式，D5）

**Files:**
- Modify: `src-tauri/src/daily_report.rs`（`Cell`/`SheetRow`/`Reject`/`ReadyRow`/`ProjectRow` + `to_cell`/`text_of`/`date_of_cell`/`hours_of_cell`/`project_of_cells`/`validate_row`/`read_rows`/`locate_columns`/`load_projects`/`existing_keys`/`preview`/`apply` + `ImportPreview`/`ImportOutcome` + 18 条测试）
- Modify: `src-tauri/src/daily_report.rs` 顶部 `use`：`use crate::todo::req_date;` 改成 `use crate::todo::{is_iso_date, req_date};`（本任务起 `date_of_cell` 要用形状判据；**不许在这文件里再写一份形状判断**）
- Modify: `src-tauri/src/lib.rs`（两条 async 命令 + 注册表 58→60、async 门 5→7）
- Modify: `src/types/daily-report.ts`（`ImportReject`/`ImportPreview`/`ImportOutcome`）、`src/lib/api.ts`（`pickXlsxOpenPath`/`importPreview`/`importApply`）、`src/pages/daily-reports.tsx`（导入区）

**Interfaces:**
- Consumes: `EXPORT_HEADERS`（Task 6，**列定位的唯一依据**）、`check_hours`/`MAX_HOURS_PER_ROW`（Task 2）、`todo::is_iso_date`、`calamine::{Data, Reader}`（用法先例 `extract.rs:176-182`）、`Range::start() -> Option<(u32,u32)>`（绝对 0-based 左上角，本任务用它算 Excel 绝对行号）、`rust_xlsxwriter` 造夹具（形制照 `extract.rs:419-422`）。
- Produces: `pub fn daily_report::preview(conn: &Connection, path: &Path) -> AppResult<ImportPreview>`、`pub fn daily_report::apply(conn: &mut Connection, path: &Path) -> AppResult<ImportOutcome>`、`pub const MAX_IMPORT_ROWS: usize = 2000`；命令 `daily_report_import_preview(path)` / `daily_report_import_apply(path)`；前端 `pickXlsxOpenPath()`、`importDailyReportsPreview(path)`、`importDailyReportsApply(path)`。

- [ ] **Step 1: 写 18 条失败测试（RED：编译层，缺的类型与函数一整套）**

`daily_report.rs` 的 `mod tests` 末尾追加。**夹具手法**：文本/数字单元用 `rust_xlsxwriter` 直接写（本机就能造）；日期格式单元造不出来（要给 number format），所以 `Cell::DateTime` 那一支走**纯函数直喂**（`date_of_cell` 收 `Cell`，不经过文件），端到端只验文本/数字/空三类。这条限制写进 Step 6 的残余记录，不许假装测过。

> **抄代码时别把分隔符“看顺眼”改回去**：`preview_counts_ok_rejects_and_truncated_separately` 里那条坏日期的字面量是 **`2026` + U+002F + `10` + U+002F + `17`（斜杠）**，而 `apply_inserts_all_or_nothing` 里那条是 **U+002D 连字符的合法日期**（它要拒的是工时 30）。本机的输出层会把 `/` 印成 `-`，两个看起来一模一样 —— 改错了这条测试就从「能红」变成恒绿或恒红。复验方法只有一条：按码点打出来（`[hex(ord(c))]`），别用眼睛（M9 终审 N5 同一条教训）。

```rust
    /// 造一份表头 + 若干行文本/数字单元的 xlsx。**列按 `headers` 的顺序写**，
    /// 所以「插一列」「换列序」这两种用户动作都能造出来 —— M⑧ 的靶就是这里。
    fn fixture_book(rows: &[Vec<String>], headers: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("in.xlsx");
        {
            use rust_xlsxwriter::Workbook;
            let mut wb = Workbook::new();
            let ws = wb.add_worksheet();
            for (c, h) in headers.iter().enumerate() {
                ws.write(0, c as u16, *h).unwrap();
            }
            for (r, cells) in rows.iter().enumerate() {
                for (c, v) in cells.iter().enumerate() {
                    // 数字串按原样写成文本；要数字单元用 `fixture_book_typed`（下面第 5 条测试用）。
                    ws.write((r + 1) as u32, c as u16, v.as_str()).unwrap();
                }
            }
            wb.save(&p).unwrap();
        }
        dir
    }

    /// 五列都齐的完整行（日期、编号、名称、内容、工时）。
    fn full_row(line: usize) -> Vec<String> {
        vec![
            format!("2026-10-{line:02}"),
            "P-01".to_owned(),
            "甲项目".to_owned(),
            "联调第三方接口".to_owned(),
            "7.5".to_owned(),
        ]
    }

    #[test]
    fn read_rows_locates_columns_by_header_text_even_when_reordered_or_inserted() {
        // 用户把「工作内容」挪到最后一列、又在最前面插了一列「备注」⇒ 按列序硬数的实现会整表错位（M⑧）。
        let headers = ["备注", "所属项目", "日期", "时长（小时）", "工作内容", "项目编号"];
        let dir = fixture_book(&[vec!["随便".to_owned(), "甲项目".to_owned(), "2026-10-16".to_owned(), "7.5".to_owned(), "联调".to_owned(), "P-01".to_owned()], ], &headers);
        let rows = read_rows(&dir.path().join("in.xlsx")).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].line, 2, "表头是第 1 行 ⇒ 首条数据的 Excel 绝对行号是 2");
        assert_eq!(rows[0].content, Cell::Text("联调".to_owned()));
        assert_eq!(rows[0].hours, Cell::Text("7.5".to_owned()));
        assert_eq!(rows[0].code, Cell::Text("P-01".to_owned()));
    }

    #[test]
    fn read_rows_names_the_missing_header_columns() {
        let dir = fixture_book(&[], &["日期", "所属项目", "工作内容"]);
        let err = read_rows(&dir.path().join("in.xlsx")).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("项目编号"), "要点名缺哪一列：{}", err.message);
        assert!(err.message.contains("时长（小时）"), "两列都要念出来：{}", err.message);
    }

    #[test]
    fn read_rows_returns_excel_absolute_line_numbers() {
        // 三行数据 ⇒ 行号 2、3、4。用户看的是 Excel，报 0-based 下标等于没报（spec §六.2）。
        let dir = fixture_book(
            &[full_row(16), full_row(17), full_row(18)],
            &EXPORT_HEADERS,
        );
        let rows = read_rows(&dir.path().join("in.xlsx")).unwrap();
        let lines: Vec<u32> = rows.iter().map(|r| r.line).collect();
        assert_eq!(lines, vec![2, 3, 4], "实际：{lines:?}");
    }

    /// **这一支是本计划原先记为「只能靠真机第 15 格」的那格，Task 6 的 M④ 变异证明本机造得出来**
    /// （`write_number` 一个序列号 + `set_num_format("yyyy\\-mm\\-dd")` 就是 Excel 手填日期的形状），
    /// 所以它回到单测里。它同时是「calamine 到底把日期样式归成哪个 `Data` 臂」的端到端追问：
    /// 若这里红在 `Cell::Number(46311.0)`，说明样式没被认出来，就要在 `to_cell` 里补序列号分支
    /// （基准 1899-12-30），并回来改 spec §三.2 那句「序列号换算是库内部的事」——
    /// 两种结果都算这条测试赢了，因为它把一个原本只能靠人眼的问题变成了一次读数。
    #[test]
    fn read_rows_recognizes_a_date_formatted_cell() {
        use rust_xlsxwriter::{Format, Workbook};
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("serial.xlsx");
        {
            let mut wb = Workbook::new();
            let ws = wb.add_worksheet();
            for (c, h) in EXPORT_HEADERS.iter().enumerate() {
                ws.write(0, c as u16, *h).unwrap();
            }
            let dfmt = Format::new().set_num_format("yyyy\\-mm\\-dd");
            ws.write_with_format(1, 0, 46311.0_f64, &dfmt).unwrap(); // 46311 = 2026-10-16 的序列号
            ws.write(1, 1, "P-01").unwrap();
            ws.write(1, 2, "甲项目").unwrap();
            ws.write(1, 3, "联调第三方接口").unwrap();
            ws.write(1, 4, "7.5").unwrap();
            wb.save(&p).unwrap();
        }
        let rows = read_rows(&p).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].date,
            Cell::DateTime(2026, 10, 16, 0, 0, 0),
            "日期样式必须走 DateTime 臂，实际：{:?}",
            rows[0].date
        );
        let (date, truncated) = date_of_cell(&rows[0].date, 2).unwrap();
        assert_eq!(date, "2026-10-16");
        assert!(!truncated, "纯日期样式（时分秒全 0）不该报截断");
    }

    #[test]
    fn date_of_cell_accepts_iso_text_and_refuses_numbers_and_blanks() {
        assert_eq!(date_of_cell(&Cell::Text(" 2026-10-16 ".to_owned()), 2).unwrap(), ("2026-10-16".to_owned(), false));
        // 形状对、历法错的值照收（与后端 is_iso_date 同口径，引历法库就破零新依赖）：
        assert!(date_of_cell(&Cell::Text("2026-13-45".to_owned()), 2).is_ok());
        for bad in [Cell::Empty, Cell::Number(20261016.0), Cell::Text("2026.10.16".to_owned()), Cell::Other] {
            let r = date_of_cell(&bad, 7).unwrap_err();
            assert_eq!(r.line, 7);
            assert_eq!(r.field, "日期");
        }
    }

    #[test]
    fn date_of_cell_truncates_the_time_part_of_a_date_cell_and_says_so() {
        let (s, truncated) = date_of_cell(&Cell::DateTime(2026, 10, 16, 8, 30, 0), 3).unwrap();
        assert_eq!(s, "2026-10-16", "日期单元取到日为止");
        assert!(truncated, "带时分秒 ⇒ 要记一行提示，不许静默截（spec §六.2）");
        let (_, clean) = date_of_cell(&Cell::DateTime(2026, 10, 16, 0, 0, 0), 3).unwrap();
        assert!(!clean, "纯日期单元不该报截断");
        let (two_digit, _) = date_of_cell(&Cell::DateTime(2026, 1, 5, 0, 0, 0), 3).unwrap();
        assert_eq!(two_digit, "2026-01-05", "月日要补零成 YYYY-MM-DD");
    }

    #[test]
    fn hours_of_cell_accepts_float_and_text_and_refuses_a_time_format_cell() {
        assert_eq!(hours_of_cell(&Cell::Number(8.5), 4).unwrap(), 8.5);
        assert_eq!(hours_of_cell(&Cell::Number(8.0), 4).unwrap(), 8.0);
        assert_eq!(hours_of_cell(&Cell::Text(" 7.53 ".to_owned()), 4).unwrap(), 7.53);
        let err = hours_of_cell(&Cell::DateTime(1899, 12, 31, 8, 30, 0), 4).unwrap_err();
        assert_eq!(err.field, "工时");
        assert!(err.reason.contains("8:30") && err.reason.contains("8.5"), "要教用户怎么改：{}", err.reason);
    }

    #[test]
    fn hours_of_cell_refuses_full_width_thousands_separators_and_out_of_range() {
        for bad in ["８", "1,5", "1e2", "abc", "", "30", "0.001"] {
            let r = hours_of_cell(&Cell::Text(bad.to_owned()), 9).unwrap_err();
            assert_eq!(r.line, 9, "「{bad}」要按行号报出来：{r:?}");
            assert_eq!(r.field, "工时");
        }
        assert!(hours_of_cell(&Cell::Number(30.0), 9).is_err(), "越界的数字单元也要拒");
    }

    fn project_rows() -> Vec<ProjectRow> {
        vec![
            ProjectRow { id: "p1".to_owned(), code: Some("P-01".to_owned()), name: "甲项目".to_owned() },
            ProjectRow { id: "p2".to_owned(), code: None, name: "乙项目".to_owned() },
            ProjectRow { id: "p3".to_owned(), code: Some("P-03".to_owned()), name: "甲项目".to_owned() },
        ]
    }

    #[test]
    fn project_of_cells_prefers_code_over_name_when_both_are_filled() {
        let ps = project_rows();
        // p1 与 p3 同名「甲项目」⇒ 只按名称会歧义，填了编号就必须按编号定。
        let id = project_of_cells(&Cell::Text("P-03".to_owned()), &Cell::Text("甲项目".to_owned()), 5, &ps).unwrap();
        assert_eq!(id, "p3", "编号优先，命中即用");
    }

    #[test]
    fn project_of_cells_falls_back_to_name_and_rejects_unknown_or_ambiguous() {
        let ps = project_rows();
        assert_eq!(project_of_cells(&Cell::Empty, &Cell::Text("乙项目".to_owned()), 5, &ps).unwrap(), "p2");
        // 编号写错但名称唯一 ⇒ 退到名称（用户可能改了编号那列）。
        assert_eq!(project_of_cells(&Cell::Text("P-99".to_owned()), &Cell::Text("乙项目".to_owned()), 5, &ps).unwrap(), "p2");
        let amb = project_of_cells(&Cell::Empty, &Cell::Text("甲项目".to_owned()), 5, &ps).unwrap_err();
        assert!(amb.reason.contains("重名 2 个") && amb.reason.contains("项目编号"), "要说清怎么办：{}", amb.reason);
        let none = project_of_cells(&Cell::Empty, &Cell::Text("没这个项目".to_owned()), 5, &ps).unwrap_err();
        assert!(none.reason.contains("没这个项目"), "要念出用户写的那个名字：{}", none.reason);
        assert_eq!(
            project_of_cells(&Cell::Empty, &Cell::Empty, 5, &ps).unwrap_err().field,
            "所属项目"
        );
    }

    /// 端到端：一份齐的行 + 一行坏日期 + 一行不存在的项目 ⇒ 预检各计其数、行号是绝对行号。
    #[test]
    fn preview_counts_ok_rejects_and_truncated_separately() {
        let conn = db::open_in_memory().unwrap();
        conn.execute("INSERT INTO projects (id, name, code, status) VALUES ('p1','甲项目','P-01','立项')", []).unwrap();
        let dir = fixture_book(
            &[
                full_row(16),
                vec!["2026/10/17".to_owned(), "P-01".to_owned(), "甲项目".to_owned(), "乙".to_owned(), "1".to_owned()],
                vec!["2026-10-18".to_owned(), "".to_owned(), "不存在".to_owned(), "丙".to_owned(), "1".to_owned()],
            ],
            &EXPORT_HEADERS,
        );
        let pv = preview(&conn, &dir.path().join("in.xlsx")).unwrap();
        assert_eq!(pv.total, 3);
        assert_eq!(pv.ok, 1, "实际：{pv:?}");
        assert_eq!(pv.rejected.len(), 2);
        assert_eq!(pv.rejected[0].line, 3, "坏日期那条在 Excel 里是第 3 行");
        assert_eq!(pv.rejected[0].field, "日期");
        assert_eq!(pv.rejected[1].line, 4);
        assert_eq!(pv.rejected[1].field, "所属项目");
        assert_eq!(pv.duplicates, 0);
    }

    #[test]
    fn preview_flags_exact_duplicates_without_blocking_them() {
        let conn = db::open_in_memory().unwrap();
        conn.execute("INSERT INTO projects (id, name, code, status) VALUES ('p1','甲项目','P-01','立项')", []).unwrap();
        create(&conn, &input("p1", Some("2026-10-16"), "联调第三方接口", 7.5)).unwrap();
        let dir = fixture_book(&[full_row(16)], &EXPORT_HEADERS);
        let pv = preview(&conn, &dir.path().join("in.xlsx")).unwrap();
        assert_eq!(pv.ok, 1, "重复不是错误，D2 之后「同一天同项目同内容同工时」合法");
        assert_eq!(pv.duplicates, 1, "但要提示，否则用户会连点两次导入按钮");
        assert_eq!(pv.duplicate_lines, vec![2]);
    }

    #[test]
    fn preview_refuses_more_than_max_import_rows() {
        let conn = db::open_in_memory().unwrap();
        conn.execute("INSERT INTO projects (id, name, code, status) VALUES ('p1','甲项目','P-01','立项')", []).unwrap();
        let rows: Vec<Vec<String>> = (0..=MAX_IMPORT_ROWS).map(|i| full_row(i % 28 + 1)).collect();
        let dir = fixture_book(&rows, &EXPORT_HEADERS);
        let err = preview(&conn, &dir.path().join("in.xlsx")).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains(&MAX_IMPORT_ROWS.to_string()), "要念出上限：{}", err.message);
    }

    /// **M⑦ 的靶子**：事务拆成逐行提交，这条当场红 —— 半套台账是本轮最贵的失败形态。
    #[test]
    fn apply_inserts_all_or_nothing() {
        let conn = db::open_in_memory().unwrap();
        conn.execute("INSERT INTO projects (id, name, code, status) VALUES ('p1','甲项目','P-01','立项')", []).unwrap();
        let dir = fixture_book(
            &[full_row(16), vec!["2026-10-17".to_owned(), "P-01".to_owned(), "甲项目".to_owned(), "乙".to_owned(), "30".to_owned()]],
            &EXPORT_HEADERS,
        );
        let mut conn = conn;
        let err = apply(&mut conn, &dir.path().join("in.xlsx")).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("第 3 行"), "整批回滚也要说清坏在哪一行：{}", err.message);
        assert!(err.message.contains("工时"), "{}", err.message);
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "任一行不合格 ⇒ 库里一条都不许多");
    }

    #[test]
    fn apply_rereads_the_file_and_reports_the_inserted_count() {
        let conn = db::open_in_memory().unwrap();
        conn.execute("INSERT INTO projects (id, name, code, status) VALUES ('p1','甲项目','P-01','立项')", []).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("in.xlsx");
        std::fs::copy(fixture_book(&[full_row(16), full_row(17)], &EXPORT_HEADERS).path().join("in.xlsx"), &p).unwrap();
        let mut conn = conn;
        let out = apply(&mut conn, &p).unwrap();
        assert_eq!(out.inserted, 2);
        assert_eq!(list(&conn, None).unwrap().len(), 2);
        assert_eq!(list(&conn, None).unwrap()[0].project_id, "p1", "按编号匹配回来的项目 id 要落进行");
    }

    #[test]
    fn apply_refuses_a_header_only_file() {
        let conn = db::open_in_memory().unwrap();
        let dir = fixture_book(&[], &EXPORT_HEADERS);
        let mut conn = conn;
        let err = apply(&mut conn, &dir.path().join("in.xlsx")).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("没有可导入的行"), "{}", err.message);
    }

    #[test]
    fn import_preview_line_format_has_exactly_six_keys() {
        let pv = ImportPreview {
            total: 0,
            ok: 0,
            duplicates: 0,
            duplicate_lines: vec![],
            truncated_lines: vec![],
            rejected: vec![],
        };
        let obj = serde_json::to_value(&pv).unwrap();
        let mut keys: Vec<&str> = obj.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["duplicates", "duplicateLines", "ok", "rejected", "total", "truncatedLines"],
            "ImportPreview 的键集合变了就要同步 src/types/daily-report.ts"
        );
    }

    #[test]
    fn import_outcome_line_format_has_exactly_one_key() {
        let obj = serde_json::to_value(ImportOutcome { inserted: 3 }).unwrap();
        assert_eq!(obj.as_object().unwrap().keys().collect::<Vec<_>>(), vec!["inserted"]);
    }
```

Run: `cd /e/zero/demo/2026/project-files-manage/src-tauri && cargo test -j 1 --lib daily_report`
Expected: 编译失败（`cannot find type Cell`、`SheetRow`、`read_rows`、`preview`、`apply`、`ImportPreview`…）。编译层 RED。

- [ ] **Step 2: 落数据结构与单元转换**

`daily_report.rs` 顶部 `use` 区改成：

```rust
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use crate::error::{AppError, AppResult};
use crate::todo::{is_iso_date, req_date};
```

在 `write_xlsx` 之后追加：

```rust
/// 一次导入最多多少行。上限是给「手滑选了整份年度台账」那一格兜底的，
/// 不是因为几百行会慢 —— 本机的物理内存会把这些行的校验与事务全压在一个 Mutex 上。
pub const MAX_IMPORT_ROWS: usize = 2000;

/// 一格读出来的形状。**保留 calamine 的类型区分**（文本 / 数字 / 日期单元 / 其它），
/// 因为 §六.2 的判据就是按类型分岔的：工时列出现日期单元要单独教用户怎么改。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Cell {
    Empty,
    Text(String),
    Number(f64),
    /// 年、月、日、时、分、秒。`to_ymd_hms_milli()` 是**单数 milli**（`calamine-0.36.1/src/datatype.rs:776`）；
    /// 不许改用它旁边的 `as_datetime()` —— 那返回 `chrono::NaiveDateTime`，一用就把 chrono 拖进代码面。
    DateTime(u16, u8, u8, u8, u8, u8),
    /// 布尔或错误单元：五列里任何一种都不认。
    Other,
}

/// `Data`（owned，`datatype.rs:36` 起）一共九臂 ⇒ 这个 match 必须写满九支。
/// 少一支编译器会报 non-exhaustive，多写不存在的臂会报 unreachable —— 臂数由编译器守着，别凭记忆删。
fn to_cell(d: &calamine::Data) -> Cell {
    use calamine::Data;
    match d {
        Data::Empty => Cell::Empty,
        Data::String(s) => Cell::Text(s.clone()),
        Data::Float(f) => Cell::Number(*f),
        Data::Int(i) => Cell::Number(*i as f64),
        Data::Bool(_) | Data::Error(_) => Cell::Other,
        Data::DateTime(e) => {
            let (y, mo, d, hh, mm, ss, _milli) = e.to_ymd_hms_milli();
            Cell::DateTime(y, mo, d, hh, mm, ss)
        }
        // xlsb/ods 才给 ISO 形状；退化成文本，交给 `date_of_cell` 的形状判据。
        Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::Text(s.clone()),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SheetRow {
    /// **Excel 里的绝对行号**（表头是第 1 行）。用户看的是 Excel，报 0-based 下标等于没报。
    pub line: u32,
    pub date: Cell,
    pub code: Cell,
    pub name: Cell,
    pub content: Cell,
    pub hours: Cell,
}

/// 一行里某一个字段的拒绝。`field` 用表头那五个词，与界面上的列名同词。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reject {
    pub line: u32,
    pub field: String,
    pub reason: String,
}

fn reject(line: u32, field: &str, reason: &str) -> Reject {
    Reject { line, field: field.to_owned(), reason: reason.to_owned() }
}

/// 通过校验、可以往库里写的一行。**不带 `line`**：行号只在拒绝时用（`Reject.line`），
/// 写库这一步不需要它 —— 带上就是一条没人读的字段，会逼出第二条 `dead_code` 豁免。
#[derive(Debug, Clone)]
pub(crate) struct ReadyRow {
    pub project_id: String,
    pub report_date: String,
    pub content: String,
    pub hours: f64,
}

pub(crate) struct ProjectRow {
    pub id: String,
    pub code: Option<String>,
    pub name: String,
}

/// 单元格 → 用来匹配的文本。整数形状的数字不留 `.0` 尾巴（Excel 常把编号列识别成数字）。
/// **已知边界**：`"01"` 这种前导零的编号在 Excel 里会变成 `1`，本函数不补救 —— 那种项目要用编号列
/// 匹配不上时，退到名称那列仍然能命中（`project_of_cells` 的两级次序就是为这个存在的）。
fn text_of(cell: &Cell) -> Option<String> {
    match cell {
        Cell::Empty => None,
        Cell::Text(s) => {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_owned())
        }
        Cell::Number(f) => {
            if f.fract() == 0.0 {
                Some(format!("{}", *f as i64))
            } else {
                Some(f.to_string())
            }
        }
        _ => None,
    }
}
```

- [ ] **Step 3: 落四支判据纯函数**

```rust
/// `(日期串, 是否截掉了时分秒)`。判据收在纯函数里（M8 终审 C1 教训③），所以 `Cell::DateTime`
/// 这一支不必造出真日期格式单元也能测到。形状只走 `is_iso_date`，**历法有效性不在本轮**。
fn date_of_cell(cell: &Cell, line: u32) -> Result<(String, bool), Reject> {
    match cell {
        Cell::Text(s) => {
            let t = s.trim();
            if is_iso_date(t) {
                Ok((t.to_owned(), false))
            } else {
                Err(reject(line, "日期", &format!("不是 YYYY-MM-DD：「{t}」")))
            }
        }
        Cell::DateTime(y, mo, d, hh, mm, ss) => Ok((
            format!("{y:04}-{mo:02}-{d:02}"),
            *hh != 0 || *mm != 0 || *ss != 0,
        )),
        Cell::Number(_) => Err(reject(
            line,
            "日期",
            "日期列不要填数字；要填 2026-10-16 这样的文本或日期格式",
        )),
        Cell::Empty => Err(reject(line, "日期", "日期不能空着")),
        Cell::Other => Err(reject(line, "日期", "这一格不是日期")),
    }
}

/// 工时：数字单元直接进 `check_hours`；文本只认「全是数字与小数点」的形状，
/// 所以全角（`８`）、千分位（`1,5`）、科学计数（`1e2`）都在这里被挡在 parse 之前。
/// 日期单元 ⇒ 单独一句教用户改（`8:30` 是时间格式，本表要的是小时数）。
fn hours_of_cell(cell: &Cell, line: u32) -> Result<f64, Reject> {
    match cell {
        Cell::Number(f) => check_hours(*f)
            .map_err(|e| reject(line, "工时", &format!("{}（要大于 0、不超过 {MAX_HOURS_PER_ROW}、最多两位小数）", e.message))),
        Cell::Text(s) => {
            let t = s.trim();
            if t.chars().any(|c| !(c.is_ascii_digit() || c == '.')) {
                return Err(reject(line, "工时", &format!("工时要填小时数（如 8.5），收到「{t}」")));
            }
            match t.parse::<f64>() {
                Ok(f) => check_hours(f)
                    .map_err(|e| reject(line, "工时", &format!("{}（要大于 0、不超过 {MAX_HOURS_PER_ROW}、最多两位小数）", e.message))),
                Err(_) => Err(reject(line, "工时", &format!("工时不是数字：「{t}」"))),
            }
        }
        Cell::DateTime(..) => Err(reject(
            line,
            "工时",
            "工时要填小时数（8.5），不要用时间格式（8:30）",
        )),
        Cell::Empty => Err(reject(line, "工时", "工时不能空着")),
        Cell::Other => Err(reject(line, "工时", "这一格不是数字")),
    }
}

fn project_of_cells(code: &Cell, name: &Cell, line: u32, projects: &[ProjectRow]) -> Result<String, Reject> {
    // 两级：**先编号、命中即用**；编号为空或编号匹配不到 ⇒ 退到名称。
    // 退到名称而不是直接报错，是为了「用户改了编号列 / 项目没填编号」两种常态还能走通。
    if let Some(c) = text_of(code) {
        let hits: Vec<&ProjectRow> = projects.iter().filter(|p| p.code.as_deref() == Some(c.as_str())).collect();
        match hits.len() {
            1 => return Ok(hits[0].id.clone()),
            n if n > 1 => return Err(reject(line, "项目编号", &format!("编号「{c}」对应 {n} 个项目，请改用所属项目列"))),
            _ => {}
        }
    }
    match text_of(name) {
        None => Err(reject(line, "所属项目", "项目编号与所属项目不能同时空着")),
        Some(n) => {
            let hits: Vec<&ProjectRow> = projects.iter().filter(|p| p.name == n).collect();
            match hits.len() {
                0 => Err(reject(line, "所属项目", &format!("找不到项目「{n}」（要先在项目页建好，或改填项目编号）"))),
                1 => Ok(hits[0].id.clone()),
                k => Err(reject(line, "所属项目", &format!("项目名「{n}」重名 {k} 个，请改填项目编号"))),
            }
        }
    }
}

/// 一行 → 可写入的 `ReadyRow`，或**第一个**拒绝理由。
/// `preview` 与 `apply` 共用这一支：两段式里两处判据必须同源（spec §六.2）。
fn validate_row(r: &SheetRow, projects: &[ProjectRow]) -> Result<ReadyRow, Reject> {
    let (report_date, _truncated) = date_of_cell(&r.date, r.line)?;
    let content = text_of(&r.content).ok_or_else(|| reject(r.line, "工作内容", "工作内容不能空着"))?;
    let hours = hours_of_cell(&r.hours, r.line)?;
    let project_id = project_of_cells(&r.code, &r.name, r.line, projects)?;
    Ok(ReadyRow { project_id, report_date, content, hours })
}
```

- [ ] **Step 4: `read_rows` 与库侧读取**

```rust
/// 五列在表里的下标（按表头文字定位，**不是列序**）。
#[derive(Debug, Clone, Copy)]
struct Columns {
    date: usize,
    code: usize,
    name: usize,
    content: usize,
    hours: usize,
}

fn read_fail(path: &Path, e: impl std::fmt::Display) -> AppError {
    AppError::new(
        "extract_failed",
        &format!("打开表格失败：{e}"),
        Some("文件可能已损坏、受密码保护，或是伪装成表格的其它格式"),
    )
}

/// 读第一个工作表。多 sheet 只读第一个（spec §十一）。
pub(crate) fn read_rows(path: &Path) -> AppResult<Vec<SheetRow>> {
    use calamine::Reader as _; // `open_workbook_auto` 后必须带这条（先例 `extract.rs:176`）

    let mut book = calamine::open_workbook_auto(path).map_err(|e| read_fail(path, e))?;
    let names = book.sheet_names();
    let Some(first) = names.first() else {
        return Err(invalid("这份工作簿里没有工作表"));
    };
    let range = book.worksheet_range(first).map_err(|e| read_fail(path, e))?;
    let mut it = range.rows();
    let Some(header) = it.next() else {
        return Err(invalid("这份表是空的，至少要有表头一行（可以先「下载模板」）"));
    };
    let cols = locate_columns(header)?;

    // `Range::start()` 给的是绝对 0-based 左上角行号（`calamine-0.36.1/src/lib.rs:784`）。
    // Excel 显示的行号从 1 起，表头占第一行 ⇒ 数据的绝对行号 = start + 1 + (下标 + 1)。
    let base = range.start().map(|(r, _)| r).unwrap_or(0) + 1;
    let mut out = Vec::new();
    for (i, row) in it.enumerate() {
        let pick = |idx: usize| row.get(idx).map(to_cell).unwrap_or(Cell::Empty);
        let line = base + i as u32 + 1;
        // 整行全空 ⇒ 跳过：用户在 Excel 里删过内容但留了空行，是常态，不该变成一条拒绝。
        if matches!(
            (pick(cols.date), pick(cols.code), pick(cols.name), pick(cols.content), pick(cols.hours)),
            (Cell::Empty, Cell::Empty, Cell::Empty, Cell::Empty, Cell::Empty)
        ) {
            continue;
        }
        out.push(SheetRow {
            line,
            date: pick(cols.date),
            code: pick(cols.code),
            name: pick(cols.name),
            content: pick(cols.content),
            hours: pick(cols.hours),
        });
    }
    if out.len() > MAX_IMPORT_ROWS {
        return Err(AppError::new(
            "invalid_input",
            &format!("一次最多导入 {MAX_IMPORT_ROWS} 行，这份有 {} 行，请分批", out.len()),
            Some("用页面上的日期范围筛小一点再导出、再导入"),
        ));
    }
    Ok(out)
}

/// **按表头文字定位列**：插一列、换列序都不该让导入错位（M⑧ 的靶子就是「改成按列序硬数」）。
/// 认不出必需列 ⇒ 点名缺哪几列。
fn locate_columns(header: &[calamine::Data]) -> AppResult<Columns> {
    let texts: Vec<String> = header.iter().map(|c| c.to_string().trim().to_owned()).collect();
    let mut found: [Option<usize>; 5] = [None; 5];
    for (i, t) in texts.iter().enumerate() {
        for (slot, head) in EXPORT_HEADERS.iter().enumerate() {
            if t == *head && found[slot].is_none() {
                found[slot] = Some(i);
            }
        }
    }
    let missing: Vec<&str> = EXPORT_HEADERS
        .iter()
        .enumerate()
        .filter(|(slot, _)| found[*slot].is_none())
        .map(|(_, head)| *head)
        .collect();
    if !missing.is_empty() {
        return Err(invalid(&format!(
            "表头缺列：{}。要用「下载模板」出来的那五列 —— 列的顺序可以变、名字不能改",
            missing.join("、")
        )));
    }
    Ok(Columns {
        date: found[0].unwrap(),
        code: found[1].unwrap(),
        name: found[2].unwrap(),
        content: found[3].unwrap(),
        hours: found[4].unwrap(),
    })
}

/// 匹配用的项目清单：软删项目**不在**清单里（挂不上新行，与 `create` 同一判据）。
fn load_projects(conn: &Connection) -> AppResult<Vec<ProjectRow>> {
    let mut stmt = conn.prepare("SELECT id, code, name FROM projects WHERE deleted_at IS NULL")?;
    let rows = stmt.query_map([], |r| {
        Ok(ProjectRow { id: r.get(0)?, code: r.get(1)?, name: r.get(2)? })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 查重用的键集合。**f64 不进 hash**：换成放大 100 倍的整数（`check_hours` 已保证两位小数）。
fn existing_keys(conn: &Connection) -> AppResult<HashSet<(String, String, String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT report_date, project_id, content, hours FROM daily_reports WHERE deleted_at IS NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        let h: f64 = r.get(3)?;
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            (h * 100.0).round() as i64,
        ))
    })?;
    let mut out = HashSet::new();
    for r in rows {
        out.insert(r?);
    }
    Ok(out)
}
```

- [ ] **Step 5: `preview` 与 `apply`**

```rust
/// 预检的结果。**只读、不写库**（D5）：界面上先给用户看清「几行能进、哪几行为什么不进」，
/// 再决定要不要点确认。重复行只提示、不拦（D2 之后「同一天同项目同内容同工时」是合法的）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub total: usize,
    pub ok: usize,
    pub duplicates: usize,
    pub duplicate_lines: Vec<u32>,
    /// 日期单元带时分秒、被截到「日」的那些行 —— 不静默（spec §六.2）。
    pub truncated_lines: Vec<u32>,
    pub rejected: Vec<Reject>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub inserted: usize,
}

pub fn preview(conn: &Connection, path: &Path) -> AppResult<ImportPreview> {
    let rows = read_rows(path)?;
    let projects = load_projects(conn)?;
    let existing = existing_keys(conn)?;
    let mut ok = 0usize;
    let mut rejected: Vec<Reject> = Vec::new();
    let mut duplicate_lines: Vec<u32> = Vec::new();
    let mut truncated_lines: Vec<u32> = Vec::new();
    for r in &rows {
        let (date, truncated) = match date_of_cell(&r.date, r.line) {
            Ok(v) => v,
            Err(e) => {
                rejected.push(e);
                continue;
            }
        };
        if truncated {
            truncated_lines.push(r.line);
        }
        // 其余三段逐个拿第一个拒绝理由，与 `validate_row` 同次序（同一支判据，不重拼第二份）：
        let ready = match validate_row(r, &projects) {
            Ok(v) => v,
            Err(e) => {
                rejected.push(e);
                continue;
            }
        };
        ok += 1;
        if existing.contains(&(ready.report_date, ready.project_id, ready.content, (ready.hours * 100.0).round() as i64)) {
            duplicate_lines.push(r.line);
        }
    }
    Ok(ImportPreview {
        total: rows.len(),
        ok,
        duplicates: duplicate_lines.len(),
        duplicate_lines,
        truncated_lines,
        rejected,
    })
}

/// 落库：**重新解析同一个路径**（不缓存 preview 的结果，也不锁文件 —— D5 当次写明的代价），
/// 重新校验，任一行不合格 ⇒ 一句 `invalid_input` 报出「第几行 + 哪个字段 + 为什么」，**库里一条都不许多**。
pub fn apply(conn: &mut Connection, path: &Path) -> AppResult<ImportOutcome> {
    let rows = read_rows(path)?;
    let projects = load_projects(conn)?;
    let mut ready: Vec<ReadyRow> = Vec::with_capacity(rows.len());
    for r in &rows {
        match validate_row(r, &projects) {
            Ok(v) => ready.push(v),
            Err(e) => {
                return Err(AppError::new(
                    "invalid_input",
                    &format!("第 {} 行的{}不合格：{}", e.line, e.field, e.reason),
                    Some("修好整份文件后重新预检；任何一行不合格都不写入"),
                ))
            }
        }
    }
    if ready.is_empty() {
        return Err(invalid("没有可导入的行（这份表只有表头？）"));
    }
    let tx = conn.transaction()?;
    for r in &ready {
        tx.execute(
            "INSERT INTO daily_reports (id, project_id, report_date, content, hours) VALUES (?1,?2,?3,?4,?5)",
            params![uuid::Uuid::new_v4().to_string(), r.project_id, r.report_date, r.content, r.hours],
        )?;
    }
    // `Transaction` 的 drop 默认回滚（`rusqlite-0.40.2/src/transaction.rs` 里 `DropBehavior::Rollback`
    // 那句注释），所以这里只要不 `commit` 就是整批撤销；不需要手写 rollback 分支（先例 `db::apply_version`）。
    tx.commit()?;
    Ok(ImportOutcome { inserted: ready.len() })
}
```

- [ ] **Step 6: 两条 async 命令与注册表**

```rust
/// 导入预检（M10 Task 7）：先解析文件（**不取库锁**，大文件在这里读），再取锁做匹配与查重。
#[tauri::command(async)]
fn daily_report_import_preview(state: State<'_, AppState>, path: String) -> AppResult<ImportPreview> {
    let p = std::path::Path::new(&path);
    let conn = db(&state)?;
    daily_report::preview(&conn, p)
}

/// 导入落库：单事务全成全败（D5）。命令体同样不写 await 点。
#[tauri::command(async)]
fn daily_report_import_apply(state: State<'_, AppState>, path: String) -> AppResult<ImportOutcome> {
    let p = std::path::Path::new(&path);
    let mut conn = db(&state)?;
    daily_report::apply(&mut conn, p)
}
```

注册表：给 `daily_report_export` 补逗号，追加两条，末项不带逗号。`lib.rs` 的 `use crate::daily_report::{...}` 补 `ImportPreview, ImportOutcome`。

> **本格的三处已知残余（写进 Task 9 的开发进度「没能自动验证」，不许含糊）**：
> ① `to_cell` 的 `Data::DateTime` 一支**有端到端测试了**（`read_rows_recognizes_a_date_formatted_cell`）：Task 6 的 M④ 变异证明「序列号 + `set_num_format`」本机就能造出日期格式单元，所以本计划原先记的「这一支只能靠真机」不成立。这条测试同时负责追问 calamine 把它归成哪个 `Data` 臂 —— 若归成 `Float`，测试会红在 `Cell::Number(46311.0)`，那时要在 `to_cell` 补序列号分支（基准 1899-12-30）并回来改 spec §三.2 那句「换算是库内部的事」。
> ② `ReadyRow` **不带**行号字段（只有 `Reject` 带）：写库那一步不需要知道是第几行，带上就是一枚没人读的字段，会多逼出一条 `allow(dead_code)` 豁免。全仓豁免数本轮**保持 1**（只有 `db.rs:30`）。
> ③ 导入期间文件被人改动（两步之间不锁文件）是 D5 当次接受的代价，不是缺陷。

- [ ] **Step 7: 前端两段式**

`src/types/daily-report.ts` 末尾：

```ts
export interface ImportReject {
  /** Excel 里的绝对行号（表头是第 1 行）。 */
  line: number;
  field: string;
  reason: string;
}

/// 与 Rust 的 ImportPreview 对齐（六键，`import_preview_line_format_has_exactly_six_keys` 钉着）。
export interface ImportPreview {
  total: number;
  ok: number;
  duplicates: number;
  duplicateLines: number[];
  truncatedLines: number[];
  rejected: ImportReject[];
}

export interface ImportOutcome {
  inserted: number;
}
```

`src/lib/api.ts`（`exportDailyReports` 之后）：

```ts
export async function pickXlsxOpenPath(): Promise<string | null> {
  const picked = await openDialog({
    multiple: false,
    filters: [{ name: "Excel 工作簿", extensions: ["xlsx"] }],
  });
  return typeof picked === "string" ? picked : null;
}

export const importDailyReportsPreview = (path: string) =>
  invoke<ImportPreview>("daily_report_import_preview", { path });
export const importDailyReportsApply = (path: string) =>
  invoke<ImportOutcome>("daily_report_import_apply", { path });
```

`src/pages/daily-reports.tsx`：先补 `import`（漏一行就是 `tsc` 当场红）——`@/lib/api` 那条里加 `importDailyReportsApply, importDailyReportsPreview, pickXlsxOpenPath`，`@/types/daily-report` 那条里加 `type ImportPreview`；然后状态加两个、按钮加两个、预检结果加一段。**确认后重新发一次 apply（后端自己重新解析文件），前端不把预检结果回传给后端**（判据只有一处，且 apply 不信任客户端）。

```tsx
  const [importPath, setImportPath] = useState<string | null>(null);
  const [previewData, setPreviewData] = useState<ImportPreview | null>(null);
```

```tsx
  const onPickImport = async () => {
    const path = await pickXlsxOpenPath();
    if (path === null) return; // 取消不是错误
    setPreviewData(null);
    const ok = await run(async () => setPreviewData(await importDailyReportsPreview(path)));
    if (ok) setImportPath(path);
    else setImportPath(null);
  };

  const onApplyImport = () => {
    const path = importPath;
    if (path === null) return;
    void run(async () => {
      const out = await importDailyReportsApply(path);
      setNotice(`已导入 ${out.inserted} 条`);
      setPreviewData(null);
      setImportPath(null);
    });
  };
```

按钮（「下载模板」之后）：

```tsx
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void onPickImport()}>
            从 Excel 导入
          </Button>
```

预检结果段（放在 `notice` 那条之后、`工时汇总` 段之前）：

```tsx
      {previewData && (
        <section className="rounded-xl border bg-background p-4">
          <h2 className="text-sm font-medium">导入预检</h2>
          <p className="mt-1 text-xs text-muted-foreground">
            {previewData.total} 行里 {previewData.ok} 行可导入 · {previewData.rejected.length} 行不合格
            {previewData.duplicates > 0 && ` · ${previewData.duplicates} 行与库里已有记录完全相同`}
            {previewData.truncatedLines.length > 0 &&
              ` · ${previewData.truncatedLines.length} 行的日期带时分秒，将截到「日」`}
          </p>
          {previewData.rejected.length > 0 && (
            <ul className="mt-2 space-y-1 text-xs">
              {previewData.rejected.map((r) => (
                <li key={`${r.line}-${r.field}`}>
                  <span className="font-mono">第 {r.line} 行</span> · {r.field}：{r.reason}
                </li>
              ))}
            </ul>
          )}
          <div className="mt-3 flex items-center gap-2">
            <Button size="sm" disabled={busy || previewData.ok === 0} onClick={onApplyImport}>
              确认导入 {previewData.ok} 条
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                setPreviewData(null);
                setImportPath(null);
              }}
            >
              取消
            </Button>
            <span className="text-xs text-muted-foreground">{importPath}</span>
          </div>
          <p className="mt-2 text-xs text-muted-foreground">
            任何一行不合格 ⇒ 整批不写。确认时后端会重新读一遍这个文件，所以改过的内容按新的算。
          </p>
        </section>
      )}
```

- [ ] **Step 8: 门禁**

```bash
cd /e/zero/demo/2026/project-files-manage/src-tauri
cargo test -j 1 --lib                        # 期望 301 passed（283 + 18）
cargo clippy --lib -- -D warnings            # exit=0
cargo clippy --lib --all-targets -- -D warnings
cd /e/zero/demo/2026/project-files-manage
npm run build                                # exit=0，error TS = 0
node --test "tests/**/*.test.ts"             # 41 / 41 / 0
sed -n '/generate_handler/,/^      \]/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"   # 期望 60
grep -c 'tauri::command(async)' src-tauri/src/lib.rs      # 期望 7（5→7）
grep -rn "allow(dead_code)" src-tauri/src/ | wc -l        # 期望 1（只有 db.rs:30 —— 本轮不开新豁免）
```

**`dead_code` 豁免本轮保持 1 条**（`db.rs:30`）。写到这里曾有一条 `ReadyRow.line` 需要开第二条例外 —— 已改成**不给 `ReadyRow` 带那个字段**（行号只在拒绝时用），因为多一条豁免就多一个下轮要解释的例外粒度（M8 实测过「dead_code 不沿调用链下传 ⇒ 未接线的入口件才是豁免的粒度」，字段级是另一种粒度，不值得为它开门）。若 clippy 报的是别的地方（例如 `Cell::Other` 在某支从未被构造），**按报错改代码，不要顺手再加一条豁免**。

- [ ] **Step 9: 提交**

```bash
cd /e/zero/demo/2026/project-files-manage
git add src-tauri/src/daily_report.rs src-tauri/src/lib.rs src/types/daily-report.ts src/lib/api.ts src/pages/daily-reports.tsx
git commit -m "feat(m10): Excel 导入两段式（预检 + 单事务落库，58→60、async 5→7）—— 按 EXPORT_HEADERS 文字定位列（插列换序不错位，M⑧靶）、行号一律 Excel 绝对行号、日期接受 ISO 文本与日期单元两种（带时分秒截到日并单独报行）、工时的文本支只认数字与小数点（全角/千分位/科学计数都挡在 parse 之前）而日期单元单独教用户改 8:30→8.5、项目按编号优先名称兜底（重名 2 个要说清怎么办）、重复四字段全等只提示不拦（f64 换成放大 100 的整数进 hash）、apply 重新解析同一路径任一行不合格整批回滚（M⑦靶，Transaction drop 即回滚）；preview 与 apply 共用 validate_row 一支判据；三处残余（日期格式单元造不出端到端夹具、ReadyRow 不带行号因此不开新豁免、两步之间不锁文件）写进开发进度"
```

---

### Task 8: 全量门禁 + 八处变异取证 + 真机十六格（**控制方执行，子代理禁跑 dev**）

**Files:**
- Create: `.superpowers/sdd/2026-10-16-m10-daily-reports/m10-cdp.mjs`（真机断言脚本，照同目录 `m9-cdp.mjs` 形制：`node m10-cdp.mjs s1|s2|…` 分段可重跑）
- Create: `.superpowers/sdd/2026-10-16-m10-daily-reports/mutation-evidence.txt`（八处变异的红/绿读数，逐条留命令、`test result` 行、红的测试名与断言原文）
- Create: `.superpowers/sdd/2026-10-16-m10-daily-reports/realdir-snapshot.mjs`（真实库三文件 `size + mtime + sha256` 前后对照，照 M9 同名脚本）

**Interfaces:**
- Consumes: T1–T7 的终态树
- Produces: 门禁读数、变异读数、真机读数 —— Task 9 文档回写的**唯一**取材来源

- [ ] **Step 1: 全量门禁（终态树，退出码全要真的）**

```bash
cd /e/zero/demo/2026/project-files-manage/src-tauri
cargo test -j 1 --lib > /tmp/g-test.log 2>&1; echo "test=$?"; grep "test result" /tmp/g-test.log
cargo clippy --lib -- -D warnings > /tmp/g-c1.log 2>&1; echo "c1=$?"; grep -c "warning" /tmp/g-c1.log
cargo clippy --lib --all-targets -- -D warnings > /tmp/g-c2.log 2>&1; echo "c2=$?"; grep -c "warning" /tmp/g-c2.log
cargo build --lib > /tmp/g-build.log 2>&1; echo "build=$?"
cd /e/zero/demo/2026/project-files-manage
node --test "tests/**/*.test.ts" > /tmp/g-node.log 2>&1; echo "node=$?"; grep -E "^ℹ (tests|pass|fail)" /tmp/g-node.log
npm run build > /tmp/g-npm.log 2>&1; echo "npm=$?"; grep -c "error TS" /tmp/g-npm.log
sed -n '/generate_handler/,/^      \]/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"   # 期望 60
grep -c 'tauri::command(async)' src-tauri/src/lib.rs                     # 期望 7
grep -m1 'TARGET_VERSION' src-tauri/src/db.rs                            # 期望 i64 = 6
grep -rn "allow(dead_code)" src-tauri/src/ | wc -l                       # 期望 1（只有 db.rs:30，本轮不开新豁免）
grep -c '^\[\[package\]\]' src-tauri/Cargo.lock                          # 期望 616
git diff --stat master..HEAD -- src-tauri/Cargo.lock                     # 期望空输出（提一级不动 Cargo.lock 内容）
git diff --stat master..HEAD -- src-tauri/Cargo.toml                     # 期望只有 rust_xlsxwriter 那一行移动
```

Expected（**全部现跑核对**，下面这串是本计划按逐格列名加出来的数）：`test=0` + **301 passed / 0 failed**（基线 256 + T1 3 + T2 18 + T3 2 + T6 4 + T7 18）；两个 clippy `exit=0` 且 0 warning；`build=0`；node **41 / 41 / 0**；`npm=0` 且 `error TS` **0**；结构门 **60 / 7 / 6 / 1 / 616**（命令数 / async 属性 / TARGET_VERSION / dead_code 豁免 / Cargo.lock 包数）。

**若某个数对不上**：先怀疑「有人少写/多写了一条测试」或「有一格被回退」，按 `cargo test --lib daily_report` / `--lib search` / `--lib db` 分模块取数定位，**不许**为了凑数放宽断言，也不许把聚合数改成实际读数了事而不解释差异。

- [ ] **Step 2: 八处变异取证**

| # | 靶点 | 怎么改 | 该红在哪 |
|---|---|---|---|
| M① | `daily_report.rs` 的 `list` | 在 WHERE 里加 `AND p.status <> '归档'` | `list_includes_rows_of_an_archived_project`（D6 的兑现点；归档项目的工时从台账里静默消失就是这条） |
| M①′ | `search.rs` 的 `daily` 那条 collect | 删掉 `AND p.deleted_at IS NULL` | `soft_deleted_project_hides_daily_and_todo_hits` |
| M② | `daily_report.rs` 的 `create` | 把 `req_date("日报日期", …)` 换成 `norm_date("日报日期", …)` | `create_rejects_blank_and_absent_report_date_naming_the_column`（空白折 NULL ⇒ 报出来是 db_failed） |
| M③ | `daily_report.rs` 的 `check_hours` | 整个函数体改成 `Ok(h)`（保留签名） | `check_hours_refuses_zero_out_of_range_and_sub_cent_precision` + `create_rejects_out_of_range_hours` + `hours_of_cell_refuses_full_width_thousands_separators_and_out_of_range`（**三层各打一次**：命令层、页内新增、导入文本支） |
| M④ | `hours-summary.ts` 的 `isoWeekKey` | 周首从周一改成周日（`dow = thursday.getUTCDay() + 1`） | node 的「跨年周三条边界」与「同周不同日同键」 |
| M⑤ | `hours-summary.ts` 的 `sumHours` | 去掉整数化，直接 `+= r.hours` | node 的「十条 0.1 得 1」（实际 `1.0000000000000002`） |
| M⑥ | `hours-summary.ts` 的 `scopedHoursSummary` | 把 `scopeDailyReports(rows, f)` 改成 `rows` | node 的「只算筛选后的行」（M9 终审 I1 的同族靶） |
| M⑦ | `daily_report.rs` 的 `apply` | 去掉 `transaction()`，逐行 `conn.execute` 自动提交 | `apply_inserts_all_or_nothing`（半套台账 —— 本轮最贵的失败形态） |
| M⑧ | `daily_report.rs` 的 `locate_columns` | 改成硬编码 `Columns { date: 0, code: 1, name: 2, content: 3, hours: 4 }` | `read_rows_locates_columns_by_header_text_even_when_reordered_or_inserted` |

**取证口径**（照 M8/M9）：每处跑完把命令、`test result` 行、红的测试名与断言原文（含 `file:line`）追加进 `mutation-evidence.txt`；还原用 `cp` 备份 + `cmp` 验字节一致，**不要**用 `git checkout --`（同 worktree 可能有别的未提交内容）；八处跑完复跑一次全量，读数必须回到 Step 1 那串数。

**Task 8 开局要先结掉的一格**：`to_cell` 的 `Data::DateTime` 一支原先记为「无自动化网、只能真机第 15 格」，Task 6 复审后已由 `read_rows_recognizes_a_date_formatted_cell` 端到端覆盖（Task 7 落地时跑红跑绿都算有读数）。T7 若红在 `Cell::Number(46311.0)`，说明 calamine 没认出该样式 ⇒ 按 Task 7 计划文本补 `to_cell` 的序列号分支，别把它记成环境失败。

- [ ] **Step 3: 真机十六格（沙盒 `dev.zero.pfm.m10test`，一次性 identifier；真实目录只读）**

编译前两个必然失败的探针先走一遍（M5 起的红线：证 `npm run tauri -- dev -c '{"identifier":"…"}'` 的覆盖通道真走得通），再起带 CDP 9222 的沙盒 dev，跑完**按记下的 PID 精确回收**（**坑 38：抽 PID 的正则不许把端口号喂给 `Stop-Process -Force`**，`PORT 1520 PID 1188` 那种两列的输出要按列取）。

spec §十 的十四格一条别省，另加两格 Excel 专用：

1. 真实库三文件 `size + mtime + sha256` 前后逐字一致，**且没有任何新增文件**。
2. 空库启动 → 底栏 `schema=6`、`journal_mode=wal`；`/daily-reports` 是 nav 第 6 格且 WebView2 真渲染出文字。
3. 新增一条（日期默认今天、项目必填、工时 `7.5`）→ 刷新还在。
4. 日期留空提交 ⇒ 界面念出点名「日报日期」那句，**不是**「数据库操作失败」。
5. 工时填 `24.5`、`0`、`0.001` ⇒ 三条都点名「工时」。
6. 给一个 `归档` 状态的项目写日报 ⇒ 列表看得见、按项目汇总里有它。
7. 筛到某一个项目 ⇒ 日/周/月三档数字与按项目数字**只含该项目**，空态文案也吃 scoped。
8. 跨年周：`2026-12-31` 与 `2027-01-01` 各一条 ⇒ 落进同一个周桶 `2026-W53`。
9. 十条 `0.1` ⇒ 合计显示 `1`。
10. **`__TAURI_INTERNALS__.invoke("daily_report_export", {path, rows})` 直接绕开原生 save 对话框**，落点用 `%TEMP%` 下的沙盒文件 ⇒ 文件真存在、用 `daily_report_import_preview` 读回来 `total` 与行数一致。
11. 落点指进沙盒自己的数据目录 ⇒ 报 `backup_destination_unsafe` **且目录里没多出文件**（守卫在真链上生效，不是只在单测里）。
12. 手写一份 xlsx 夹具（用 `rust_xlsxwriter` 在 `%TEMP%` 造，含一行坏日期、一行不存在的项目）⇒ `preview` 报出两个**绝对行号**与各自原因、`ok=1`；`apply` 只写 1 条。
13. 把同一文件再 `apply` 一次 ⇒ 仍成功（重复只提示），库里 2 倍；此时 `preview` 的 `duplicates` 要报出那些行。
14. 故意留一行不合格再 `apply` ⇒ 库里**一条都没多**（M⑦ 的真机对照面）。
15. **人手格**：用 Excel 双击打开第 10 格导出的文件 ⇒ 表格能开、表头五列齐、日期列显示成日期而不带「数字存成文本」的绿三角、工时列**能直接求和**（这是「日期写文本、工时写数字」那个类型分工在真 Excel 里的唯一读数）。`to_cell` 的日期单元那一半已经由 Task 7 的端到端测试接管，不再依赖这一格。
16. **人手格**：点界面上的「导出当前范围 / 从 Excel 导入」两个**原生对话框**真走一遍（save 与 open 各一次）。这与 M8 欠的那四格同类：原生对话框点不通 ⇒ 唯一入口只能人眼验。

**要如实记为「没验到」的**（不许假称通过，坑 11）：第 15、16 格若当轮没人手点，就写「未点」；`backup::guard_destination` 在日报导出上的第 11 格取的是**沙盒**数据目录，不等于真实数据目录那一格的对照。

- [ ] **Step 4: 留证据并提交**

- [ ] **Step 4: 收口检查（本格不产生提交）**

`.superpowers/` 里的产物**本仓从未跟踪过**（`git ls-files .superpowers` 空输出，M3 至今九轮都是这样；`.gitignore` 里也没有专门的规则，是那个目录本身不被 add）。所以：**证据三件留在工作区、文档按路径引用，不加 `-f`、不改 `.gitignore`、不为它们开提交**。M9 的产物目录 `.superpowers/sdd/2026-10-14-m9-todos-milestones/` 同样**不删**（`docs/` 里按路径引用着它）。

```bash
cd /e/zero/demo/2026/project-files-manage
git status --short                      # 只应剩未跟踪的 Python/；变异跑完必须已把源码还原成字节一致
cmp /dev/null /dev/null && echo "还原自检：下面这条若列出任何 M.* 靶点文件，说明有一处没还原" && git diff --name-only
```

若 `git diff --name-only` 非空 ⇒ 那处变异**没还原**，按 `mutation-evidence.txt` 里记的备份路径 `cp` 回去、`cmp` 验字节一致，再复跑一次全量确认读数回到 Step 1。**不许**用 `git checkout --` 图快（同 worktree 可能有别的未提交内容，坑 32）。

---

### Task 9: 活文档回写（按 spec §九、§十一 逐条）

**Files:**
- Modify: `docs/技术方案.md`（§四 数据模型加 `daily_reports` 一行；§九 M10 行从占位改成实际交付口径；`§6.2` 那句备份口径补「M10 的新表随整库快照进包、但不进 counts 摘要（沿用 M9 D9）」）
- Modify: `docs/开发进度.md`（M10 表行 + 变更日志 + 「没能自动验证」逐条）
- Modify: `docs/HANDOFF.md`（§0 状态与读数、§3 卡点改成「下一格是 M11」、§4-A 那串门里**三个数**（`generate_handler!` 53→60、async 属性 4→7、`TARGET_VERSION` 5→6；`dead_code` 仍 1）、§6 指针行、新坑登记）
- Modify: `docs/superpowers/specs/2026-10-14-m9-todos-milestones-design.md`（**只在文件顶部加一行指针**，见 Step 3）

**Interfaces:**
- Consumes: Task 8 的全部读数（**文档里每个数都必须是现跑读数**，不许抄本计划的推算值）
- Produces: 下一个对话能直接接手 M11 的恢复图

- [ ] **Step 1: 技术方案两格**

`docs/技术方案.md` §四 数据模型清单里，紧跟 M9 那两行（`todos`、`milestones`）之后加一行，形制照它们：

```
- `daily_reports`（id / project_id→projects / report_date TEXT NOT NULL / content / hours REAL / created_at / updated_at / deleted_at）—— 日报条目，一天一项目可多条；日期只存形状（`YYYY-MM-DD`，由写入侧 `todo::req_date` 保证），工时上限与两位小数由 Rust 校验（不加 CHECK）。索引只建 `project_id` 一条，不建日期索引（量级几千行，M9 也未为 `due_date` 建）。SQL 不解释日期。
```

§九 的 M10 行从「待办清单/日报」那几个占位字改成实际交付口径，并**点名两处缩量与一处反转**：

- 已交付：日报 CRUD、复制上一天（源 = 最近有日报的那天）、按日/周/月与按项目聚合、真 `.xlsx` 导出（含模板）与两段式导入（预检 + 单事务全成全败）、日报正文与待办标题一起进全局检索。
- 未交付（**本轮主动推后，不是遗漏**）：`思维导图.md:70` 的「导出 Excel/Markdown **台账**文件」整格推 M11 —— 那格导的是台账五张表，里面躺着账号密码密文列，导成文件等于把明文交到用户手里，要先单独定红线。
- 反转：M9 裁的「待办正文不进任何检索通路」被本轮 D4 翻回来（需求方当次改口径，理由与账见 M10 spec §九.8）。

- [ ] **Step 2: 开发进度两格**

`docs/开发进度.md` 的 M10 表行写：完成日期、终态读数（Task 8 Step 1 的现跑数）、九处结构门的实际数、八条变异各自的红项名、真机十六格哪些点通/哪些没点。变更日志按本仓形制（一条一行、写「为什么」不是「改了啥」），至少涵盖：

- 六格「以为要新写、其实树上免费」的核实结论（calamine 已是直接依赖、写库已在锁文件只落在 dev、`to_ymd_hms_milli` 让序列号换算归库、台账检索根本不是 FTS、命中跳转已统一、`guard_destination` 本就是 `pub fn`）—— 这一格要写，否则下轮会重复去问「要不要引 Excel 库」。
- `req_date` 与 `norm_date` 分家的原因（空白折 NULL 在 NOT NULL 列上伪装成库故障）。
- 为什么导出命令收行而不收查询条件（避免 SQL 里第二份日期比较），以及为什么所属项目列写 `name` 不写 `projectLabel`。
- 为什么 `async` 门从 4 变 7 不是破例（那 4 条本就是碰文件的一家，函数体一律零 await）。

「没能自动验证」逐条列（**至少**这六条，取 Task 8 实态）：① 页面接线无自动化网（判据在前端 ⇒ `cargo test` 不再守住它，取证面在 `node --test`，而页面本身只有 `tsc` + `vite build` + 人眼）；② 若 Task 7 的 `read_rows_recognizes_a_date_formatted_cell` 走了「补序列号分支」那条岔路，spec §三.2「换算是库内部的事」那句要跟着改准；③ 两条原生对话框（save/open）若本轮没人手点就是未验；④ 排序与周归属的实际渲染只由真机读数兜；⑤ 导入 2000 行上限没有真实性能取证（上限是给内存兜底的，不是量出来的）；⑥ `guard_destination` 第 11 格取的是沙盒数据目录，不等于真实目录那一格。另记一条**结构性事实**而非缺测：全仓 `dead_code` 豁免本轮仍是 1 条，没为 `ReadyRow` 开行号字段因此没开新豁免（下轮若有人数对不上，别以为是漏改）。

- [ ] **Step 3: HANDOFF 四格 + M9 spec 的指针行**

`docs/HANDOFF.md`：

1. **§4-A 那串门**：`generate_handler!` 的注释数改 60、async 属性那条改 7、`TARGET_VERSION` 改 6；`allow(dead_code)` **仍是 1**（本轮没开新豁免，别把它改成 2 —— 计划里曾考虑过、后来按「不给 `ReadyRow` 带行号」消掉了）；`Cargo.lock` 616 那条不动。
2. **§0 与 §3**：状态改成「M0–M10 已合入 `master`」（合并后填实际合并点），卡点改成「代码面无卡点，下一格是 M11：到期预警（维保/域名/证书三类字段还没建，M9 只兑了一半）+ 本轮推过去的『项目台账导出 Excel/Markdown』」。
3. **§6 指针行**：加 M10 的 spec 与 plan 两行；把「M9 那格待终审/待合并」的旧说法改准。
4. **坑登记**：把 Task 8 里新踩的坑**按本轮实态**加编号（没有就不加，别为凑数编）。已经确定要写的一条：**显示层会把 `/` 印成 `-`，所以「放进拒绝列表的日期样本」必须按码点确认它是斜杠** —— 本轮控制方在写计划时三次被这一点打到（`todo.rs:242` 那条历史样本、计划里两支夹具、以及一次 Edit 因两侧字面量看起来相同而 0 命中）。同族第一条：M9 终审 N5。
5. **M9 历史 bullet（`HANDOFF.md:113`）不动**：那是 M9 当轮的裁定记录。反转的记录位置在 M10 spec §九.8 与本文件 §3/§6。
6. **`docs/superpowers/specs/2026-10-14-m9-todos-milestones-design.md` 顶部加一行指针**（不改它 §十一 的原文，历史规格不 retro-edit）：

```markdown
> **2026-10-16 更新指针**：本文件 §十一 的「待办正文不进任何检索通路」已被 M10 的 D4 反转（需求方当次改口径），现状见 `2026-10-16-m10-daily-reports-design.md` §九.8。本文件其余裁定保持原样，作为 M9 当轮的历史记录。
```

- [ ] **Step 4: 复跑受影响的结构门并回读**

改完文档要复跑：`cargo test -j 1 --lib`（文档不该影响它，但 `daily_report.rs` 里若为解释改过注释，`grep` 类门会漂）、三条计数门、以及 `grep -c 'tauri::command(async)'`（**注释里不许出现这串字面量**，M9 真踩过）。写完**回读**关键行确认没被 shell 掏空（坑 38 的第二条：含反引号的中文正文走脚本文件，别 `python -c` / heredoc）。

- [ ] **Step 5: 提交**

```bash
cd /e/zero/demo/2026/project-files-manage
git add docs/技术方案.md docs/开发进度.md docs/HANDOFF.md docs/superpowers/specs/2026-10-14-m9-todos-milestones-design.md
git commit -m "docs(m10): 活文档回写 —— 技术方案 §四 补 daily_reports 一行与 §九 M10 实际口径（点名台账导出整格推 M11、M9「待办不进检索」被 D4 反转）；开发进度补 M10 表行（现跑读数 + 八条变异的红项名）与「没能自动验证」六条（页面接线无网、日期单元若需补序列号分支要回写 spec、两条原生对话框、排序与周归属靠人眼、2000 行上限无量能取证、守卫只在沙盒对照）；HANDOFF §0/§3/§4-A/§6 三个门数字改到 60/7/6（dead_code 仍 1，本轮没开新豁免）、卡点改 M11、新坑登记「斜杠被印成连字符 ⇒ 拒收样本必须按码点确认」；M9 spec 顶部只加一行反转指针，历史裁定原文不动"
```

---

## 自审记录（控制方 2026-10-16 落盘后按四项重读）

1. **spec 覆盖对账**：§二 十条决策 → D1 T6/T7、D2 T1/T2、D3 T2、D4 T3、D5 T7、D6 T2（`list_includes_rows_of_an_archived_project`）、D7 T9（推 M11 记账）、D8 T2/T6/T7（57→58→60 与 async 4→5→7）、D9 T4、D10 T4（`previousReportDay`）+ T5。spec §十 十四格 → Task 8 Step 3 的 1–14，另加 15/16 两格人手格。spec §十二 八条变异 → Task 8 Step 2 的 M①–M⑧（M①′ 是软删过滤的第二次落点，属加严）。**无遗漏**。
2. **占位符扫描**：全文无「TBD/待补/同上/自行补全」。Task 5 无自动化测试那一格写的是「本格的网是 `tsc` + `vite build` + 真机十四格」并给了派发词要求 —— 这是**如实**，不是占位。
3. **类型一致性**：`DailyReportEdit` 在 T2（Rust）与 T4（TS）同为四字段含 `projectId`；`ImportPreview` 六键在 T7 的两处（Rust 结构 + TS interface + 键集断言）一致；`EXPORT_HEADERS` 五列在 T6 表头断言与 T7 `locate_columns` 共用同一常量；`hoursOf`/`check_hours`/`MAX_HOURS_PER_ROW` 三层（Rust 校验、导入文本支、前端表单折 NaN→0）不重复判据。
4. **本轮改掉的四处计划自身缺陷**（都属「简报文本缺陷」，按本仓先例改计划本体 + 重生成简报，不为它开修复轮）：① Task 2 的 Step 3/4/5 一度把实现块排在测试块之前，会让 RED 变成没取证 —— 已按 M9 的终态写法（`plans/2026-10-14-…:800-803`：先 `mod` 声明 + 只落类型与测试骨架）钉死执行顺序；② Task 5 的 `onCopyPreviousDay` 初稿用 `.then(() => true, () => false)` 吞掉失败，与它自己要遵守的 M9 I3 相反 —— 改成不吞、直接 `await`；③ Task 7 的两支夹具日期一度把合法日期放进拒绝列表（恒红），已按码点改成斜杠/点分隔并加了「不许用眼睛复验」的告诫；④ spec §八「错误码只有三个」漏了文件读写这一档，spec 已改准为五个既有码（`fs_failed` 与 `extract_failed` 都是现成的，**不需要批准新码**）。
