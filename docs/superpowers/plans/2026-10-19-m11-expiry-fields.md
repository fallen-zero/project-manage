# M11-a 到期事项（维保/域名/证书/其他）+ 预警第四类来源 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新建 `project_expiries` 表与 `expiry_*` 四条命令，把「维保 / 域名 / 证书 / 其他」四类到期事项接进项目详情页面板、`/todos` 到期预警块（单独 30 天窗口）与全局检索，并同步备份包那句明文告警。

**Architecture:** 后端沿用 M9 `milestones` 那一整条形制（一表 + 四同步命令 + 应用层判据 + 键集合钉死断言），日期语义一律不进 SQL；前端把第四类来源加进既有的单份判据件 `dueStateOf`，窗口按 kind 选一处、比较本体不分叉；页面侧无组件测试网，判据取证面在 `node --test`，接线面只有 `tsc` + `vite build` + 真机。

**Tech Stack:** Tauri 2.12 + Rust（rusqlite 0.40.2 bundled，单条 `Mutex<Connection>`）、React 19 + TypeScript + Vite + Tailwind 4 + TanStack Router（hash history）、`node --test`。

**Spec:** [docs/superpowers/specs/2026-10-19-m11-expiry-fields-design.md](../specs/2026-10-19-m11-expiry-fields-design.md)（D1–D8 全部口径、§九 十二条不变量逐条裁定、§十 十四格验收网格、§十二 七处变异靶）

## Global Constraints

- **零新依赖**：`Cargo.toml` 与 `Cargo.lock` 只许 `[[package]]` 数量不变（**616**），`package.json` 零字节差异。引 `regex`、`chrono`、jsdom 一律禁止（spec §五 历法有效性、§十一 组件测试网）。
- **不新开错误码**：只用既有的 `invalid_input` / `not_found`（spec §五）。
- **命令数**：`generate_handler!` **60→64**（需求方 2026-10-19 当次批准，本仓第 5 次破例）；`tauri::command(async)` **仍 7**（四条新命令函数体一律零 `await`，取 `MutexGuard` 的那条 `db(&state)?` 不许横跨 await 点 —— HANDOFF 坑 21）。
- **迁移**：`TARGET_VERSION` **6→7**，线性追加、一版本一事务、`CREATE TABLE` 不带 `IF NOT EXISTS`、**零回填**（迁移里不许塞业务行）。
- **一条判据只写一处**：日期形状只在 `todo::req_date`；`kind` 词表只在 `expiry::KINDS`（Rust）+ `src/types/expiry.ts` 的 `EXPIRY_KINDS`（前端）+ V7 的 CHECK（SQL 兜底），三处由一条 `include_str!` 文本门钉住；比较本体只在 `dueStateOf`。
- **SQL 里零日期比较**：30 天、7 天、逾期一律在前端纯函数（M9 D7 / M10 D9 / spec §九.8）。
- **`deleted_at` 不上线**：`Expiry` 线格式恰好 **8 个键**，`serde(rename_all = "camelCase")`。
- **输出与提交信息一律中文**（需求方常驻口径；错误码、SQL、命令名等字面量除外）。
- **每个 Task 结束即提交并推 `origin feat/m11-expiry-fields`**；**合并回 `master` 需要需求方当次批准**，推 `master` 另需单独指示。
- **子代理一律不跑 `tauri dev` / `npm run dev`**；真机格只由控制方按 HANDOFF 坑 9 的一次性 identifier 沙盒（`dev.zero.pfm.m11test`）跑，真实数据目录 `%APPDATA%\dev.zero.pfm\` **只读**，验证前后三文件 `size + mtime + sha256` 逐字一致。
- **本机命令纪律**（HANDOFF 坑 3/4/32/38/39）：cargo 一律 `-j 1`；退出码用 `cmd > /tmp/x.log 2>&1; echo "exit=$?"`，**绝不** `cmd | tail; echo $?`；`node --test` 在**仓库根**跑；含反引号的中文走 heredoc 或单引号串，落盘后立刻回读；显示层会把 `/` 印成 `-`，日期分隔符按码点判断。
- **测试期望数由实跑数出来**，不累加印象数（坑 5）。本计划下面那条数链的基线是**控制方现数**的：`grep -c '#\[test\]' src-tauri/src/*.rs` 逐文件相加 = **301**，`grep -c '^test(' tests/*.test.ts` 相加 = **41**（`backup-name` 2 / `due-warning` 18 / `hours-summary` 10 / `search-order` 8 / `todo-view` 3）。

---

## 任务总览与派发前必读

| Task | 交付 | 新增测试 | cargo 累计 | node 累计 |
|---|---|---|---|---|
| 1 | V7 迁移：`project_expiries` + 零回填 | db.rs +4（并**改名**一条版本断言） | 301→**305** | 41 |
| 2 | `expiry.rs` 四操作 + 4 条 IPC + 词表文本门 | expiry.rs +10、lib.rs +1 | 305→**316** | 41 |
| 3 | 全局检索第 9 条 + 前端 `HitSource` | search.rs +2 | 316→**318** | 41 |
| 4 | `api.ts` 四封装 + 预警第四类与 30 天窗口 + `/todos` 拉数四份一起缩（**页面上移到此格，否则 `tsc` 不过**） | node +6、改 1 条既有（三份→四份） | 318 | 41→**47** |
| 5 | `expiry-panel.tsx` + 项目详情页挂载 | 0（页面网没有，只有 `tsc`/`vite build`） | 318 | 47 |
| 6 | 预警块展示层（块头四处文案从常量念双窗口、行尾「（归档项目）」） | 0 | 318 | 47 |
| 7 | 备份那句明文告警补「到期事项名称与备注」 | 0（往既有测试**加断言**） | 318 | 47 |
| 8 | 全量门禁 + 七处变异取证 + 真机 14 格与双向映射表 | — | **318** | **47** |
| 9 | 活文档回写 | — | 318 | 47 |

**派发前必读的三条裁定**（控制方已定，实现者不要再讨论）：
1. **`ExpiryUpdate` 只有 `expire_date` 与 `note`**（spec §二 控制方自拍①）。种类和名称录错走删了重建 ⇒ 面板上不许出现改 kind/label 的入口，也不许为了「凑齐字段」给 `ExpiryUpdate` 加没有界面消费者的字段（M9 终审 I4 就是这一格）。
2. **归档门只包 `deadline` 那一类**（spec §七）。`buildDueWarnings` 里 `p.status === "归档"` 那道 continue **不许**扩到 expiry / todo / milestone 三类上；本轮到期类照常亮是需求方 D4 拍的。
3. **`warn_plaintext_columns()` 那句话落在 Task 7**（spec §八 / §九.6）。M10 的教训是这句话没被分配到任何一格，于是九次任务级评审结构性看不见它 —— 本轮它是一格的必改文件，评审要按文件清单核对。

**验收网格映射规矩（spec §十开头那条）**：Task 8 的真机格必须逐格映射到 spec §十 的 1–14，收口前交一张双向映射表；「全部格都有读数」这句只能按 spec 那张表说，**不许**按计划的编号说（M10 坑 40）。

### Task 1: V7 迁移 —— `project_expiries` 表 + 零回填

**Files:**
- Modify: `src-tauri/src/db.rs`（`TARGET_VERSION` 在 :98，`migrate` 的 match 在 :118-126，`const V6` 在 :375-388，测试在 :657-828）
- Test: 同文件的 `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes：无（本任务在最前）
- Produces：库里存在表 `project_expiries(id, project_id, kind, label, expire_date, note, created_at, updated_at, deleted_at)` 与索引 `ix_expiry_project`；`db::TARGET_VERSION == 7`；`db::open_in_memory()` 建出来的库自带这张空表（Task 2/3/4 全部依赖这一事实）。

- [ ] **Step 1: 先把版本断言改名并改成期望 7（这一条会红，是本轮的 RED）**

把 `src-tauri/src/db.rs:657` 那支测试整个替换（改名 + 两个数字都动，别的不动）：

```rust
    #[test]
    fn v7_schema_meta_reaches_target_version_seven() {
        let conn = open_in_memory().unwrap();
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 7, "迁移必须跑到 7");
        assert_eq!(TARGET_VERSION, 7, "= 上一条，两处别各写各的");
    }
```

- [ ] **Step 2: 跑它，确认红在「迁移没跑到 7」**

Run: `cd src-tauri && cargo test -j 1 --lib v7_schema_meta > /tmp/t1-red.log 2>&1; echo "exit=$?"; grep -E "test result|panicked|assertion" /tmp/t1-red.log`
Expected：`exit=101`（**cargo 测试失败的退出码是 101，不是 1** —— 本计划原文这里写错过一次，实现者按实跑带回后订正），失败信息是 `assert_eq!(ver, 7)` 那一行（左边 6、右边 7）。**若这里编不过或报的是别的断言，说明 Step 1 改错了位置。**

- [ ] **Step 3: 提 `TARGET_VERSION` 并接上 match 臂**

`src-tauri/src/db.rs:98`：

```rust
pub(crate) const TARGET_VERSION: i64 = 7;
```

同文件 `migrate` 的 match（:118-126）加一臂，`_ => ""` 保持最后一臂：

```rust
        let ddl = match target {
            1 => V1,
            2 => V2,
            3 => V3,
            4 => V4,
            5 => V5,
            6 => V6,
            7 => V7,
            _ => "",
        };
```

- [ ] **Step 4: 写 `const V7`（放在 `const V6` 之后、`#[cfg(test)]` 之前）**

```rust
/// V7 = 到期事项表（M11-a spec §四）。四条口径延续 V5/V6 的注释：
/// 1. 软删沿用 `deleted_at`；`ON DELETE CASCADE` 因项目是软删（`project.rs` 的 UPDATE）而永不触发，
///    所以「跨项目读要自带 p.deleted_at IS NULL」这条规矩照旧适用于本表（Task 2/3 各落一次）。
/// 2. `kind` 这里留 CHECK 只是**直写库的最后闸门**：命令路径的合法值判据在 `expiry::check_kind`，
///    因为 CHECK 违例冒出来是 `db_failed`，会把「你选的种类不在词表里」伪装成库故障（V5 的 stage 同形）。
/// 3. `expire_date` **必填但不加形状 CHECK**：形状与空白由写入侧的 `todo::req_date` 判（D2 的语义是
///    「这一行的有效期到 X」，没 X 就没意义）；SQL 里不做任何日期解释（`strftime`/`date` 是 UTC）。
/// 4. **零回填**：老项目的维保/域名/证书信息不由迁移猜，由用户自己录（延续 M9 为 V5 立的规矩）。
const V7: &str = "
CREATE TABLE project_expiries (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('维保','域名','证书','其他')),
    label       TEXT NOT NULL,
    expire_date TEXT NOT NULL,
    note        TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at  TEXT
);

CREATE INDEX ix_expiry_project ON project_expiries(project_id);
";
```

- [ ] **Step 5: 补四条 V7 断言（放在 `v6_backfills_nothing` 之后）**

```rust
    #[test]
    fn v7_creates_project_expiries_with_documented_columns() {
        let conn = db::open_in_memory().unwrap();
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('project_expiries') ORDER BY cid")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(
            cols.join(","),
            "id,project_id,kind,label,expire_date,note,created_at,updated_at,deleted_at",
            "V7 的列集合/顺序变了要同步 expiry.rs 的 SELECT_COLUMNS"
        );
        // `AND sql IS NOT NULL` 是必需的，不是洁癖（Task 1 实现者实测带回、控制方复核后订正本计划原文）：
        // `id TEXT PRIMARY KEY` 让 SQLite 自带一支 `sqlite_autoindex_*`，它在 sqlite_master 里 `sql` 为 NULL，
        // 每张表都有。不排掉它，下面那句「索引恰好只有 ix_expiry_project」在任何正确 DDL 下**恒红**；
        // 排掉后钉的才是**本迁移自己写下的索引**：多建一条日期索引会红，漏建 ix_expiry_project 也会红。
        let idx: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'project_expiries' AND sql IS NOT NULL ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(idx, vec!["ix_expiry_project"], "只建项目维度的索引，**没有日期索引**（spec §四）：{idx:?}");
    }

    /// D2 的 SQL 层兜底：`expire_date` NOT NULL 真的挡得住直写库。
    /// 这条钉的是「为什么必填日期走 `req_date` 而不是 `norm_date`」的同一条理由（延续 v6 那格）。
    #[test]
    fn v7_expire_date_not_null_is_enforced_at_sqlite_layer() {
        let conn = db::open_in_memory().unwrap();
        seed_project(&conn, "p1");
        let err = conn
            .execute(
                "INSERT INTO project_expiries (id, project_id, kind, label) VALUES ('x1','p1','域名','a.gov.cn')",
                [],
            )
            .unwrap_err();
        assert!(
            format!("{err}").contains("NOT NULL"),
            "预期是 NOT NULL 挡住（而不是 CHECK 或外键），实际：{err}"
        );
    }

    /// 四类词表外的值必须被 SQL 的 CHECK 挡住：这是绕过命令直写库的那条路的最后一道闸门。
    /// 报错要确认是 CHECK 而不是别的约束（延续 `v5_milestone_stage_check_...` 的判法）。
    #[test]
    fn v7_kind_check_refuses_words_outside_the_four() {
        let conn = db::open_in_memory().unwrap();
        seed_project(&conn, "p-kind");
        for kind in ["维保", "域名", "证书", "其他"] {
            conn.execute(
                "INSERT INTO project_expiries (id, project_id, kind, label, expire_date)
                  VALUES (?1, 'p-kind', ?2, '一行', '2027-01-31')",
                rusqlite::params![format!("e-{kind}"), kind],
            )
            .unwrap_or_else(|e| panic!("四类词表里的「{kind}」必须插得进去：{e}"));
        }
        let err = conn
            .execute(
                "INSERT INTO project_expiries (id, project_id, kind, label, expire_date)
                  VALUES ('e-bad', 'p-kind', '备案', '一行', '2027-01-31')",
                [],
            )
            .expect_err("词表外的种类名必须被 CHECK 拒掉（需求原文只有三类，「其他」是 D7 加的那一格）");
        assert!(
            err.to_string().contains("CHECK constraint failed"),
            "要的是 CHECK 那条错，实际：{err}"
        );
    }

    #[test]
    fn v7_backfills_nothing() {
        let conn = db::open_in_memory().unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM project_expiries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "新建库跑完迁移后本表必须是空的（迁移里不许塞业务行）");
    }
```

- [ ] **Step 6: 跑全套 Rust 门，确认数链到 305**

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/t1-green.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t1-green.log`
Expected：`exit=0` 且 **`305 passed; 0 failed`**（基线 301 − 改名不减 + 4 条新断言）。**若报 `E0786`/`os error 1455` 且没有 `error[Exxxx]`，先重跑同一条命令**（坑 32）。

- [ ] **Step 7: clippy 两条门**

Run: `cd src-tauri && cargo clippy -j 1 --lib -- -D warnings > /tmp/t1-clippy.log 2>&1; echo "lib=$?"; cargo clippy -j 1 --all-targets --all-features -- -D warnings > /tmp/t1-clippy-all.log 2>&1; echo "all=$?"`
Expected：两个都 `=0` 且日志里没有 `warning`。

- [ ] **Step 8: 提交并推功能分支**

```bash
git add src-tauri/src/db.rs
git commit -m "feat(m11-a): V7 建 project_expiries（四类 kind 的 CHECK + expire_date NOT NULL + 只建项目索引），TARGET_VERSION 6→7 且零回填 —— 305 passed / 两条 clippy exit=0"
git push origin feat/m11-expiry-fields
```

---

### Task 2: `expiry.rs` 四操作 + 四条同步命令 + 词表文本门

**Files:**
- Create: `src-tauri/src/expiry.rs`
- Create: `src/types/expiry.ts`（本任务就要建：Task 2 的文本门 `include_str!` 它，Task 4/5 只是消费者）
- Modify: `src-tauri/src/lib.rs`（`mod` 表在 :1-20、`use crate::` 表在 :28-36、`milestone_*` 四条命令在 :729-754、`generate_handler!` 尾部在 :881 起、测试模块在 :948 起）
- Test: `src-tauri/src/expiry.rs` 的 `#[cfg(test)] mod tests` + `src-tauri/src/lib.rs` 的测试模块

**Interfaces:**
- Consumes：Task 1 的 `project_expiries` 表；`todo::req_date`（既有：`pub(crate) fn req_date(column: &str, value: Option<String>) -> AppResult<String>`，见 `src-tauri/src/todo.rs:63`）；`error::{AppError, AppResult}`；`db::{open_in_memory, TARGET_VERSION}`。
- Produces：`expiry::{Expiry, ExpiryInput, ExpiryUpdate, KINDS, list, get, create, update, delete}`；4 条 IPC 命令名 `expiry_list / expiry_create / expiry_update / expiry_delete`；前端 `src/types/expiry.ts` 的 `Expiry / ExpiryInput / ExpiryUpdate / EXPIRY_KINDS`（Task 4 的 api 封装与预警第四类吃这些名字）。

> **本计划对 spec §六 的一处订正（控制方裁定，派发前已同步回 spec）**：`ExpiryInput.expire_date` 与 `ExpiryUpdate.expire_date` 用 `Option<String>` 而不是 spec 草图里的 `String`。理由与 M10 的 `DailyReportInput.report_date: Option<String>` 完全同形：缺键 / 传 `null` / 传空串这三种形状都要落进 `req_date`，由它回一句点名「到期日期」的 `invalid_input`；若声明成 `String`，缺键会先在 Tauri 参数反序列化层炸成一句**不带我们错误码的裸字符串**（M10 真机第 5 格实测过那个形状，见 `docs/开发进度.md`「M10 没能自动验证的」第 6 条），界面的 `toAppError` 只能走 `unknown` 兜底。

- [ ] **Step 1: 写 `src/types/expiry.ts`**

```ts
export interface Expiry {
  id: string;
  projectId: string;
  kind: string;
  label: string;
  expireDate: string;
  note: string;
  createdAt: string;
  updatedAt: string;
}

// expireDate 传 string | null（后端 Option<String>）：空白与缺键都要走后端的 invalid_input，
// 不许在前端偷偷折成 undefined —— 缺键会先炸在 Tauri 参数层，回一句没有 code 的裸串。
export interface ExpiryInput {
  projectId: string;
  kind: string;
  label: string;
  expireDate: string | null;
  note: string | null;
}

// 只有这两件有界面消费者（spec §二 控制方自拍① / §七）：种类与名称录错走删了重建，
// 所以这里**故意不含** kind / label / projectId。别「顺手」补上凑齐字段 —— M9 终审 I4 就是那一格。
export interface ExpiryUpdate {
  expireDate: string | null;
  note: string | null;
}

// 需求原文 `docs/思维导图.md:73` 是前三类；「其他」是 spec D7 当次加的一格
// （ICP 备案、等保测评、软件订阅、服务器续费这些没处放，硬塞进「证书」就丢了语义）。
// lib.rs 的 kind_words_are_identical_in_rust_and_the_frontend_type 钉这两份与 V7 的 CHECK 逐字一致。
export const EXPIRY_KINDS = ["维保", "域名", "证书", "其他"] as const;
```

- [ ] **Step 2: 写 `src-tauri/src/expiry.rs` 的失败测试（整份贴进去，此时编不过 = 本轮 RED）**

```rust
//! 到期事项：维保 / 域名 / 证书 / 其他 四操作（M11-a spec §五、§六）。
//!
//! 形制整段照 `milestone.rs`（M9 Task 3）：一条 `list` SQL 同时管「跨项目全部」与「按项目筛」，
//! `p.deleted_at IS NULL` 在本表这一处（检索侧的第二处由 Task 3 那条 collect 落，届时两处各自有守卫测试）。
//! 日期形状判据复用 `todo::req_date`（必填日期**不能**走 `norm_date`：它把空白折成 NULL，
//! 而 NULL 进 `expire_date NOT NULL` 会由 SQLite 冒成 `db_failed`，用户看到的是「数据库操作失败」
//! 而不是「你的日期没填」—— M10 为 `report_date` 立的同一条理由）。

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

fn norm(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// 四类词表：需求原文那三类 + D7 当次批准的「其他」。
/// 与 `projects.status` 那八段、`milestones.stage` 那五段是**三套互不相干的词**，别互换下拉。
/// V7 的 CHECK 与本常量、`src/types/expiry.ts` 的 `EXPIRY_KINDS` 三处由 lib.rs 那条文本门钉住。
pub(crate) const KINDS: [&str; 4] = ["维保", "域名", "证书", "其他"];

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Expiry {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub label: String,
    pub expire_date: String,
    pub note: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpiryInput {
    pub project_id: String,
    pub kind: String,
    pub label: String,
    pub expire_date: Option<String>,
    pub note: Option<String>,
}

/// 只有 `expire_date` 与 `note`（spec §二 自拍①：按界面反推接口）。
/// 种类与名称录错走删了重建；改归属同理 —— 域名/证书天然属于一个项目。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpiryUpdate {
    pub expire_date: Option<String>,
    pub note: Option<String>,
}

const SELECT_COLUMNS: &str =
    "e.id, e.project_id, e.kind, e.label, e.expire_date, e.note, e.created_at, e.updated_at";

fn map_expiry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Expiry> {
    Ok(Expiry {
        id: row.get(0)?,
        project_id: row.get(1)?,
        kind: row.get(2)?,
        label: row.get(3)?,
        expire_date: row.get(4)?,
        note: row.get(5)?,
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

/// `ORDER BY e.expire_date, e.kind, e.id`：最近的先看到（`expire_date` NOT NULL，不需要
/// `milestones` 那句 `plan_date IS NULL` 前置）；第三键 `, e.id` 是 M9 终审 N3 同一条理由 ——
/// `datetime('now')` 只到秒，同一天同类别的两行必须有确定次序，**摘掉它下面那条 `list_breaks_ties_by_id_when_date_and_kind_are_equal` 当场红**。
pub fn list(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<Expiry>> {
    let mut stmt = conn.prepare(
        &format!(
            "SELECT {SELECT_COLUMNS}
               FROM project_expiries e
               JOIN projects p ON p.id = e.project_id AND p.deleted_at IS NULL
              WHERE e.deleted_at IS NULL AND (?1 IS NULL OR e.project_id = ?1)
              ORDER BY e.expire_date, e.kind, e.id"
        ),
    )?;
    let rows = stmt.query_map(params![project_id], map_expiry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Expiry> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM project_expiries e WHERE e.id = ?1 AND e.deleted_at IS NULL"
    );
    conn.query_row(&sql, [id], map_expiry)
        .optional()?
        .ok_or_else(|| not_found(id))
}

/// 词表在**应用层**判：落到 SQLite 的 CHECK 会变成 `db_failed`，用户看不出是自己选错（spec §五）。
/// CHECK 仍留着，它挡的是绕过命令直写库的那条路（Task 1 的 `v7_kind_check_refuses_words_outside_the_four`）。
fn check_kind(kind: &str) -> AppResult<()> {
    if KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(invalid(&format!(
            "种类「{kind}」不在 {} 之内",
            KINDS.join("/")
        )))
    }
}

pub fn create(conn: &Connection, input: &ExpiryInput) -> AppResult<Expiry> {
    project_exists(conn, &input.project_id)?;
    check_kind(&input.kind)?;
    let label = norm(Some(input.label.clone())).ok_or_else(|| invalid("「名称」不能为空（如 oa.gov.cn、年度维保合同）"))?;
    let expire_date = req_date("到期日期", input.expire_date.clone())?;
    let note = norm(input.note.clone()).unwrap_or_default();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO project_expiries (id, project_id, kind, label, expire_date, note)
          VALUES (?1,?2,?3,?4,?5,?6)",
        params![id, input.project_id, input.kind, label, expire_date, note],
    )?;
    get(conn, &id)
}

/// 只改日期与备注两列（D2：续期 = 改日期）。`kind`/`label`/`project_id` 一律不碰。
pub fn update(conn: &Connection, id: &str, upd: &ExpiryUpdate) -> AppResult<()> {
    let expire_date = req_date("到期日期", upd.expire_date.clone())?;
    let note = norm(upd.note.clone()).unwrap_or_default();
    let changed = conn.execute(
        "UPDATE project_expiries SET expire_date=?2, note=?3, updated_at=datetime('now')
          WHERE id=?1 AND deleted_at IS NULL",
        params![id, expire_date, note],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE project_expiries SET deleted_at = datetime('now'), updated_at = datetime('now')
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
            "INSERT INTO projects (id, name, status) VALUES (?1, '到期事项测试项目', '立项')",
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

    fn input(project_id: &str, kind: &str, label: &str, date: Option<&str>, note: Option<&str>) -> ExpiryInput {
        ExpiryInput {
            project_id: project_id.to_owned(),
            kind: kind.to_owned(),
            label: label.to_owned(),
            expire_date: date.map(str::to_owned),
            note: note.map(str::to_owned),
        }
    }

    fn upd(date: Option<&str>, note: Option<&str>) -> ExpiryUpdate {
        ExpiryUpdate {
            expire_date: date.map(str::to_owned),
            note: note.map(str::to_owned),
        }
    }

    #[test]
    fn create_refuses_kind_words_outside_the_four_with_invalid_input() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        // 「维保中」是 project_tags 里的标签词（技术方案 §四），不在本词表 —— 恰是这条最容易混的一格。
        for bad in ["备案", "订阅", "维保中", "上线", ""] {
            let err = create(&conn, &input("p1", bad, "一行", Some("2027-01-31"), None)).unwrap_err();
            assert_eq!(err.code, "invalid_input", "「{bad}」要的是应用层那句，不是 db_failed");
        }
        for good in KINDS {
            create(&conn, &input("p1", good, "一行", Some("2027-01-31"), None)).unwrap();
        }
        assert_eq!(list(&conn, Some("p1")).unwrap().len(), 4, "四类各建一行都要成");
    }

    #[test]
    fn create_requires_expire_date_naming_the_column() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for missing in [None, Some("")] {
            let err = create(&conn, &input("p1", "域名", "a.gov.cn", missing, None)).unwrap_err();
            assert_eq!(err.code, "invalid_input", "空白与缺键都不许落进 SQL");
            assert!(err.message.contains("到期日期"), "错误文本要点名是哪一列：{}", err.message);
        }
        // 形状就不对的值（点分隔），不是「历法错」的值：拿合法日期断 unwrap_err 是自己红。
        let err = create(&conn, &input("p1", "域名", "a.gov.cn", Some("2027.01.31"), None)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert_eq!(err.hint.as_deref(), Some("日期要用 YYYY-MM-DD"), "hint 实际：{:?}", err.hint);
        let e = create(&conn, &input("p1", "域名", "a.gov.cn", Some("2027-01-31"), None)).unwrap();
        assert_eq!(e.expire_date, "2027-01-31");
    }

    #[test]
    fn create_rejects_blank_or_whitespace_label_naming_the_column() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for blank in ["", "   ", "\t"] {
            let err = create(&conn, &input("p1", "证书", blank, Some("2027-01-31"), None)).unwrap_err();
            assert_eq!(err.code, "invalid_input", "名称是 NOT NULL 且必填，空白要在这里就挡下");
            assert!(err.message.contains("名称"), "要点名是哪一列：{}", err.message);
        }
    }

    #[test]
    fn create_rejects_project_that_does_not_exist_or_is_deleted() {
        let conn = db::open_in_memory().unwrap();
        let err = create(&conn, &input("nope", "维保", "甲方", Some("2027-01-31"), None)).unwrap_err();
        assert_eq!(err.code, "not_found");
        seed(&conn, "p-gone");
        soft_delete_project(&conn, "p-gone");
        let err = create(&conn, &input("p-gone", "维保", "甲方", Some("2027-01-31"), None)).unwrap_err();
        assert_eq!(err.code, "not_found", "软删项目不许再往里录到期事项");
    }

    #[test]
    fn create_then_list_roundtrip_trims_label_and_note() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let e = create(&conn, &input("p1", "域名", "  oa.gov.cn  ", Some("2027-03-31"), Some("  已问厂商  ")))
            .unwrap();
        assert_eq!(e.label, "oa.gov.cn", "名称写入要 trim");
        assert_eq!(e.note, "已问厂商", "备注写入要 trim");
        let bare = create(&conn, &input("p1", "证书", "tls", Some("2027-04-01"), None)).unwrap();
        assert_eq!(bare.note, "", "不填 note 就是空串，不是 NULL（列 NOT NULL DEFAULT ''）");
        assert_eq!(get(&conn, &e.id).unwrap(), e);
        let labels: Vec<String> = list(&conn, Some("p1")).unwrap().into_iter().map(|x| x.label).collect();
        assert_eq!(labels, vec!["oa.gov.cn", "tls"], "按到期日升序");
        assert_eq!(list(&conn, None).unwrap().len(), 2);
    }

    /// `, e.id` 那个 tiebreaker 的唯一守卫：同一天 + 同一 kind + 插入顺序与 id 序相反。
    /// **摘掉 ORDER BY 里的 `, e.id` 这条当场红**（Task 8 变异 M③′ 的备选靶）。
    #[test]
    fn list_breaks_ties_by_id_when_date_and_kind_are_equal() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for id in ["e-3", "e-1", "e-2"] {
            conn.execute(
                "INSERT INTO project_expiries (id, project_id, kind, label, expire_date, created_at)
                  VALUES (?1, 'p1', '域名', '同一个域名', '2027-05-01', '2026-10-19 09:00:00')",
                [id],
            )
            .unwrap();
        }
        let ids: Vec<String> = list(&conn, None).unwrap().into_iter().map(|e| e.id).collect();
        // 断言用 join 而不是 vec![...to_owned()...]：形状少一处，复验只需按码点（M9 踩过同一坑）。
        assert_eq!(ids.join(","), "e-1,e-2,e-3", "同日期同种类必须按 id 升序，插入顺序是 e-3、e-1、e-2");
    }

    #[test]
    fn list_hides_soft_deleted_expiries() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let e = create(&conn, &input("p1", "证书", "tls", Some("2027-01-31"), None)).unwrap();
        delete(&conn, &e.id).unwrap();
        assert!(list(&conn, None).unwrap().is_empty());
        assert_eq!(get(&conn, &e.id).unwrap_err().code, "not_found");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM project_expiries WHERE deleted_at IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "软删：行要在库里");
    }

    /// spec §九.3 的兑现点，Task 8 变异 **M①** 在这一格的靶子：
    /// **摘掉 `AND p.deleted_at IS NULL` 这条当场红**（`ON DELETE CASCADE` 因项目软删而永不触发）。
    #[test]
    fn cross_project_list_hides_expiries_of_a_soft_deleted_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p-alive");
        seed(&conn, "p-dead");
        create(&conn, &input("p-alive", "域名", "a.gov.cn", Some("2027-01-31"), None)).unwrap();
        let dead = create(&conn, &input("p-dead", "证书", "b.gov.cn", Some("2027-02-28"), None)).unwrap();
        soft_delete_project(&conn, "p-dead");

        let labels: Vec<String> = list(&conn, None).unwrap().into_iter().map(|e| e.label).collect();
        assert_eq!(labels, vec!["a.gov.cn"], "软删项目的到期事项不能进跨项目列表（预警就是这条查询）");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM project_expiries WHERE id = ?1", [&dead.id], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "行留着，恢复项目即原样回来");
    }

    /// D2 的兑现点 + spec §二 自拍① 的兑现点：update 只动日期与备注，
    /// `kind`/`label`/`project_id` 三列**一个字也不动**（接口里根本没有它们）。
    #[test]
    fn update_changes_only_date_and_note_and_reports_not_found_for_missing_row() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        seed(&conn, "p2");
        let e = create(&conn, &input("p1", "维保", "甲方年度维保", Some("2026-12-31"), Some("去年签的"))).unwrap();
        update(&conn, &e.id, &upd(Some("2027-12-31"), None)).unwrap();
        let after = get(&conn, &e.id).unwrap();
        assert_eq!(after.expire_date, "2027-12-31", "续期就是改日期（D2）");
        assert_eq!(after.note, "", "note 传 None 就是清成空串");
        assert_eq!(after.kind, "维保", "update 改不了种类");
        assert_eq!(after.label, "甲方年度维保", "update 改不了名称");
        assert_eq!(after.project_id, "p1", "update 改不了归属");
        update(&conn, &e.id, &upd(Some("2028-01-15"), Some("已续"))).unwrap();
        let twice = get(&conn, &e.id).unwrap();
        assert_eq!((twice.expire_date.as_str(), twice.note.as_str()), ("2028-01-15", "已续"));
        // 空白/坏形状同样要拒，且拒了以后**原值一个字不变**（不许写坏一半）。
        let err = update(&conn, &e.id, &upd(Some(""), None)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert_eq!(get(&conn, &e.id).unwrap().expire_date, "2028-01-15", "被拒的更新不能留下半成品");
        assert_eq!(update(&conn, "no-such", &upd(Some("2027-01-01"), None)).unwrap_err().code, "not_found");
    }

    /// 线格式钉键数：八键（spec §六）。少一键 = 前端读 undefined，多一键 = 白送（`deleted_at` 不许上线）。
    #[test]
    fn expiry_line_format_has_exactly_eight_keys() {
        let e = Expiry {
            id: "x1".to_owned(),
            project_id: "p1".to_owned(),
            kind: "域名".to_owned(),
            label: "oa.gov.cn".to_owned(),
            expire_date: "2027-01-31".to_owned(),
            note: "已问厂商".to_owned(),
            created_at: "2026-10-19 00:00:00".to_owned(),
            updated_at: "2026-10-19 00:00:00".to_owned(),
        };
        let obj = serde_json::to_value(&e).unwrap();
        let mut keys: Vec<&str> = obj.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "createdAt",
                "expireDate",
                "id",
                "kind",
                "label",
                "note",
                "projectId",
                "updatedAt"
            ],
            "Expiry 的键集合变了就要同步 src/types/expiry.ts"
        );
    }
}
```

- [ ] **Step 3: 跑聚焦测试，确认红在「模块不存在」**

Run: `cd src-tauri && cargo test -j 1 --lib expiry > /tmp/t2-red.log 2>&1; echo "exit=$?"; tail -20 /tmp/t2-red.log`
Expected：`exit=101`（10 条全跑绿，但本步骤先只贴测试时会因 `Expiry`/`KINDS` 未定义**编不过**）。
**RED 的正确形状**：Step 2 是把「实现 + 测试」一起贴进去的，所以这一步的 RED 只能靠**先删掉实现段**取得 —— 若实现者按 Step 2 原样贴，请在贴完后立刻做一次变异：把 `check_kind` 的 `KINDS.contains(&kind)` 改成 `true` 跑一次，确认 `create_refuses_kind_words_outside_the_four_with_invalid_input` 当场红，再把那行改回来。**不许**用「模块不存在」当 RED（那只是编译层空转，M10 Task 1 已登记过这条口径）。

- [ ] **Step 4: 在 `lib.rs` 接线（mod / use / 四条命令 / 注册表）**

`src-tauri/src/lib.rs` 模块表：在 `mod error;`(:5) 与 `mod extract;`(:6) 之间插入（保持字母序，`expiry` < `extract`）：

```rust
mod expiry;
```

`use crate::` 表：在 `use crate::error::{AppError, AppResult};`(:29) 与 `use crate::ledger::{`(:30) 之间插入：

```rust
use crate::expiry::{Expiry, ExpiryInput, ExpiryUpdate};
```

四条命令：紧跟在 `milestone_delete`(:752-754) 之后、日报那一段注释之前插入（形制逐字照 `milestone_*`）：

```rust
/// 到期事项四条命令（M11-a Task 2，形制照 `milestone_*`）：同步、函数体不写 await 点。
/// `db(&state)?` 取回的 `MutexGuard` 不是 `Send`，在 async fn 里让它横跨 await 点才编不过（M8 D11）；
/// 本批四条不碰文件，所以 async 标记那扇门**保持 7 条不动**（spec §六）。
/// 勾选态那一列本表没有（D2：续期 = 改日期），所以这里没有 `done` 那一条形状。
#[tauri::command]
fn expiry_list(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> AppResult<Vec<Expiry>> {
    let conn = db(&state)?;
    expiry::list(&conn, project_id.as_deref())
}

#[tauri::command]
fn expiry_create(state: State<'_, AppState>, input: ExpiryInput) -> AppResult<Expiry> {
    let conn = db(&state)?;
    expiry::create(&conn, &input)
}

#[tauri::command]
fn expiry_update(state: State<'_, AppState>, id: String, input: ExpiryUpdate) -> AppResult<()> {
    let conn = db(&state)?;
    expiry::update(&conn, &id, &input)
}

#[tauri::command]
fn expiry_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    expiry::delete(&conn, &id)
}
```

> **两处计划文本缺陷由 Task 2 实现者实测带回、控制方复核后订正本文件原文**（与 Task 1 那两条同属「计划自己写错」）：① `create` 里那句 `norm(input.label.clone())` 编不过（`E0308`）—— `ExpiryInput.label` 是 `String`（与 M10 的 `DailyReportInput.content` 同形，缺键由 Tauri 参数层挡、空白由 `norm` 挡），`norm` 收的是 `Option<String>`，所以必须写 `norm(Some(...))`。② 上面那段注释原先字面写着 `#[tauri::command(async)]`，而本仓的 async 门是**对 `lib.rs` 做原始 grep 计数**的（HANDOFF 坑 26 同族：结构门对注释正文敏感），于是一条正常注释把门读数从 7 抬成 8 —— 实现者按 `milestone_*` 那段注释的先例改写成「async 标记那扇门」。**判据没变，只是别把被数的字面量写进注释。**

注册表：在 `generate_handler!` 的 `daily_report_import_apply`(:884 区段末，**无尾逗号**) 之后补逗号并追加四条（末条 `expiry_delete` **不带逗号**）：

```rust
            daily_report_import_apply,
            expiry_list,
            expiry_create,
            expiry_update,
            expiry_delete
        ])
```

- [ ] **Step 5: 加词表文本门（`lib.rs` 测试模块内，紧跟 `stage_words_are_identical_in_rust_and_the_frontend_type`(:1085-1096) 之后）**

```rust
    /// 四类词表三处同源：`expiry::KINDS`、`src/types/expiry.ts` 的 `EXPIRY_KINDS`、V7 的 CHECK。
    /// 这一条是 include_str! 文本门，钉的是**源码文本形状**（HANDOFF 坑 33：它挡得住「有人删了/改了那一行」，
    /// 挡不住参数写错）—— 判据本体在 expiry.rs 的 create_refuses_kind_words_outside_the_four 与
    /// db.rs 的 v7_kind_check_refuses_words_outside_the_four 两处，各有真断言。
    #[test]
    fn kind_words_are_identical_in_rust_and_the_frontend_type() {
        assert_eq!(
            crate::expiry::KINDS,
            ["维保", "域名", "证书", "其他"],
            "Rust 侧词表被改了：V7 的 CHECK 与前端常量要一起改"
        );
        let ts = include_str!("../../src/types/expiry.ts");
        assert!(
            ts.contains("维保\", \"域名\", \"证书\", \"其他"),
            "前端 EXPIRY_KINDS 那四个词的引号/顺序/字面与 Rust 侧不一致"
        );
        let ddl = include_str!("db.rs");
        assert!(
            ddl.contains("CHECK (kind IN ('维保','域名','证书','其他'))"),
            "V7 的 CHECK 词表与 Rust/前端两份不一致（注意 SQL 里是单引号、无空格）"
        );
    }
```

- [ ] **Step 6: 跑全套 Rust 门 + 结构门，确认数链 316 与命令数 64**

Run:
```bash
cd src-tauri && cargo test -j 1 --lib > /tmp/t2-green.log 2>&1; echo "exit=$?"
grep "test result" /tmp/t2-green.log
cargo clippy -j 1 --lib -- -D warnings > /tmp/t2-cl.log 2>&1; echo "clippy-lib=$?"
cargo clippy -j 1 --all-targets --all-features -- -D warnings > /tmp/t2-cla.log 2>&1; echo "clippy-all=$?"
cd ..
sed -n '/generate_handler/,/^\s*\])/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"
grep -c 'tauri::command(async)' src-tauri/src/lib.rs
grep -oE 'TARGET_VERSION: [a-z0-9]+ = [0-9]+' src-tauri/src/db.rs
grep -rn "allow(dead_code)" src-tauri/src/ | wc -l
```
Expected：`exit=0` 且 **316 passed**；两条 clippy `=0`；命令数 **64**、async **7**、`TARGET_VERSION: i64 = 7`、豁免 **1**（`db.rs:30`）。**豁免数若变成 2，说明 `expiry.rs` 里有没被消费的件**（本任务四条命令全接线，不该有残留）—— 不许加 `allow(dead_code)`，要把没接的那件接上或删掉。

- [ ] **Step 7: `npm run build` 过一遍 `tsc`（新 `.ts` 文件进了类型图）**

Run: `npm run build > /tmp/t2-build.log 2>&1; echo "exit=$?"; grep -c "error TS" /tmp/t2-build.log`
Expected：`exit=0` 且 `error TS` 计数 **0**。

- [ ] **Step 8: 提交并推**

```bash
git add src-tauri/src/expiry.rs src/types/expiry.ts src-tauri/src/lib.rs
git commit -m "feat(m11-a): expiry.rs 四操作 + expiry_* 四条同步命令（命令数 60→64，第 5 次批准破例）+ 四类词表三处同源的文本门 —— 316 passed / 两条 clippy exit=0 / tsc 0 错"
git push origin feat/m11-expiry-fields
```

---

### Task 3: 全局检索的第 9 条 collect + 前端 `HitSource`

**Files:**
- Modify: `src-tauri/src/search.rs`（`FieldHit.source` 的枚举注释 :42、`PER_GROUP_LIMIT` 注释 :19、`collect` :125、`field_hits` 的 ledger 段 :159-291、测试 `use` 行 :369、新测试放在 `soft_deleted_project_hides_daily_and_todo_hits`(:922) 之后）
- Modify: `src/types/search.ts`（`HitSource` :5-13、`SOURCE_LABELS` :25-34）
- Test: `src-tauri/src/search.rs` 的 `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes：Task 2 的 `expiry::{create, ExpiryInput}` 与表 `project_expiries`；既有 `collect(source: &'static str, sql: &str, conn, pattern)`（固定收 7 个 SELECT 列：`id, project_id, project_name, title, b1, b2, b3`，`b*` 进 `join_bits`）；测试夹具 `fixture() -> (Connection, String, vault::MasterKey)`、`only_hit(conn, needle, source, title)`、`sources(hits)`。
- Produces：`field_hits` 的返回里 `source == "expiry"` 的命中；前端 `HitSource` 含 `"expiry"`、`SOURCE_LABELS.expiry == "到期事项"`（搜索结果页与预览框按这张表渲染标签）。

- [ ] **Step 1: 先写两条失败测试（放进 `search.rs` 的 tests 模块）**

`use` 行（:369）加 `expiry`：

```rust
    use crate::{db, daily_report, expiry, index_scan::ScannedFile, index_store, ledger, project, todo, vault};
```

两条测试：

```rust
    /// D6：到期事项的名称与备注进全局检索，副标题念出种类与到期日。
    /// **日期本身不参与匹配**（spec §八）：否则搜「10」能把一堆日期捞出来，那是噪声不是结果。
    /// Task 8 变异 **M⑥** 的靶子：把 `WHERE` 里的 `e.label/e.note` 换成 `e.expire_date` 去 LIKE，
    /// 下面那两条断言会同时反向红（搜得到日期、搜不到备注）。
    #[test]
    fn expiry_label_and_note_are_searchable_but_the_date_column_is_not() {
        let (conn, pid, _mk) = fixture();
        expiry::create(
            &conn,
            &expiry::ExpiryInput {
                project_id: pid.clone(),
                kind: "域名".to_owned(),
                label: "oa-gateway.gov.cn".to_owned(),
                expire_date: Some("2027-01-31".to_owned()),
                note: Some("续费要走信息中心审批".to_owned()),
            },
        )
        .unwrap();

        let by_label = only_hit(&conn, "oa-gateway", "expiry", "oa-gateway.gov.cn");
        assert_eq!(by_label.project_id, pid, "到期事项命中要带所属项目，供结果页分组与跳转");
        assert!(by_label.detail.contains("域名"), "副标题要念种类：{}", by_label.detail);
        assert!(by_label.detail.contains("2027-01-31"), "副标题要念到期日：{}", by_label.detail);

        let by_note = only_hit(&conn, "信息中心审批", "expiry", "oa-gateway.gov.cn");
        assert_eq!(by_note.title, "oa-gateway.gov.cn", "备注命中出来的还是这一行");
        // 第三条 detail bit 是 `SUBSTR(e.note, 1, 80)`（spec §八）。少了它，「靠备注命中」和
        // 「靠名称命中」两行在结果页渲染成逐字相同的行，用户看不出命中的是哪一列。
        assert!(
            by_note.detail.contains("信息中心审批"),
            "靠备注命中时副标题要念得出备注：{}",
            by_note.detail
        );

        // 日期列不进匹配：单独搜日期片段必须一条台账命中也没有（正文段是另一条通路，本夹具里没有正文）。
        assert!(
            field_hits(&conn, "2027-01").unwrap().is_empty(),
            "日期不许当检索列：{:?}",
            sources(&field_hits(&conn, "2027-01").unwrap())
        );
    }

    /// spec §九.3 在检索侧的兑现点，Task 8 变异 **M①** 的第二格靶子：
    /// 摘掉本条 collect 的 `p.deleted_at IS NULL` ⇒ 这条当场红。
    #[test]
    fn soft_deleted_project_hides_expiry_hits() {
        let (conn, pid, _mk) = fixture();
        expiry::create(
            &conn,
            &expiry::ExpiryInput {
                project_id: pid.clone(),
                kind: "证书".to_owned(),
                label: "tls-wildcard".to_owned(),
                expire_date: Some("2027-02-28".to_owned()),
                note: None,
            },
        )
        .unwrap();
        assert_eq!(sources(&field_hits(&conn, "tls-wildcard").unwrap()), vec!["expiry"]);

        conn.execute(
            "UPDATE projects SET deleted_at = datetime('now') WHERE id = ?1",
            [&pid],
        )
        .unwrap();
        assert!(
            field_hits(&conn, "tls-wildcard").unwrap().is_empty(),
            "软删项目的到期事项不能出现在搜索结果里"
        );
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM project_expiries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "软删项目只挡住读，行本身留在库里");

        // 把项目恢复回活状态再搜一次：这一句才是上面那句断言里「恢复项目即原样可搜」的兑现点，
        // 一次同时钉住两件事 —— 库里的行是活的（不是被连带软删了）、`p.deleted_at IS NULL`
        // 那道门是可逆的（不是把行读没了）。
        conn.execute("UPDATE projects SET deleted_at = NULL WHERE id = ?1", [&pid])
            .unwrap();
        assert_eq!(
            sources(&field_hits(&conn, "tls-wildcard").unwrap()),
            vec!["expiry"],
            "恢复项目后到期事项要原样搜得到"
        );
    }
```

- [ ] **Step 2: 跑它，确认红在「没有 expiry 命中」**

Run: `cd src-tauri && cargo test -j 1 --lib expiry_ > /tmp/t3-red.log 2>&1; echo "exit=$?"; grep -E "test result|panicked|left|right" /tmp/t3-red.log | head -12`
Expected：`exit=101`，两条都红（`assert_eq!(sources(...), vec!["expiry"])` 那边左边是空 `Vec`）。**注意**：聚焦过滤词要能同时命中这两条又别牵进 Task 2 的 `expiry::` 单测 —— 用 `--lib expiry` 会连带跑 `expiry.rs` 那 10 条（它们该绿），所以这里用 `expiry_label_and_note` 与 `soft_deleted_project_hides_expiry` 两个词各跑一次，或按本步骤的 `expiry_`（尾下划线，只匹配 `expiry_label...`/`expiry_...`，不匹配模块名）。**cargo test 只接一个筛选词**，别指望传两个。

- [ ] **Step 3: 加第 9 条 collect（放在 `todo` 那条之后、`Ok(hits)` 之前，即 :291 附近）**

```rust
    // 到期事项：搜名称与备注两列（spec D6 / §八）。**日期列不进 WHERE** —— 搜「10」「2027」这种
    // 短串会把一堆到期日捞成命中，那是噪声；日期只进副标题的 bit。种类也只进副标题。
    // 第三个 bit 用 `SUBSTR(e.note, 1, 80)` 而不是裸 `e.note`：`note` 长度无上限，仓内两格先例
    // （上面 note 那条的 `SUBSTR(n.body, 1, 80)`、daily 那条的 `SUBSTR(d.content, 1, 80)`）对长
    // 自由文本一律截 80 再进副标题，否则结果页一行能被备注撑成整段。
    hits.append(&mut collect(
        "expiry",
        "SELECT e.id, e.project_id, p.name, e.label, e.kind, e.expire_date,
                SUBSTR(e.note, 1, 80)
           FROM project_expiries e
           JOIN projects p ON p.id = e.project_id AND p.deleted_at IS NULL
          WHERE e.deleted_at IS NULL
            AND (e.label LIKE ?1 ESCAPE '\\' OR e.note LIKE ?1 ESCAPE '\\')
          ORDER BY e.updated_at DESC
          LIMIT ?2",
        conn,
        &p,
    )?);
```

- [ ] **Step 4: 改两处注释里的条数与枚举（结构门是 grep 出来的，注释正文是承重的）**

- `src-tauri/src/search.rs:42` 的 `FieldHit.source` 注释改成：

```rust
    /// project | env | credential | server | link | note | daily | todo | expiry
```

- `src-tauri/src/search.rs:143` 那句「不扫库，也避免首屏一进来就发八条全表 LIKE」把 **八条** 改成 **九条**。

- [ ] **Step 5: 改前端 `src/types/search.ts`**

`HitSource`（:5-13）末尾加一员，`SOURCE_LABELS`（:25-34）加一项：

```ts
export type HitSource =
  | "project"
  | "env"
  | "credential"
  | "server"
  | "link"
  | "note"
  | "daily"
  | "todo"
  | "expiry";
```

```ts
export const SOURCE_LABELS: Record<HitSource, string> = {
  project: "项目档案",
  env: "环境地址",
  credential: "账号凭据",
  server: "服务器",
  link: "外部链接",
  note: "备注台账",
  daily: "日报",
  todo: "待办",
  expiry: "到期事项",
};
```

**这一处不需要额外断言**：`SOURCE_LABELS` 是 `Record<HitSource, string>` 的穷举映射，漏加标签 `tsc` 直接红（spec §三 核实到的免费守护）。**不许**给它写一条恒绿断言。

- [ ] **Step 6: 跑全套 Rust 门 + `npm run build`，确认数链 318**

Run:
```bash
cd src-tauri && cargo test -j 1 --lib > /tmp/t3-green.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t3-green.log
cargo clippy -j 1 --lib -- -D warnings > /tmp/t3-cl.log 2>&1; echo "clippy-lib=$?"
cargo clippy -j 1 --all-targets --all-features -- -D warnings > /tmp/t3-cla.log 2>&1; echo "clippy-all=$?"
cd .. && npm run build > /tmp/t3-build.log 2>&1; echo "build=$?"; grep -c "error TS" /tmp/t3-build.log
grep -c 'hits.append(&mut collect(' src-tauri/src/search.rs
```
Expected：`exit=0` 且 **318 passed**；clippy 两个 `=0`；`build=0` 且 `error TS` **0**；collect 条数 **9**。

- [ ] **Step 7: 提交并推**

```bash
git add src-tauri/src/search.rs src/types/search.ts
git commit -m "feat(m11-a): 到期事项进全局检索（第 9 条 collect，名称与备注可搜、日期列不进 WHERE）+ HitSource/SOURCE_LABELS 各补一格 —— 318 passed / tsc 0 错"
git push origin feat/m11-expiry-fields
```

---

### Task 4: 预警第四类来源 + 30 天窗口 + `/todos` 拉数接线

**Files:**
- Modify: `src/lib/api.ts`（M9/M10 封装区 :86-112，import 块 :20-30）
- Modify: `src/lib/due-warning.ts`（`DUE_HORIZON_DAYS` :7、`DueKind` :9、`DueItem` :12-24、`DueInput` :32-36、`scopeDueInput` :81-88、`buildDueWarnings` 的 `emit` :99-118 与三个循环 :120-132）
- Modify: `src/pages/todos.tsx`（import :21-26、state 区 :130 附近、`reload` :137-148、`scoped` :162-165）
- Test: `tests/due-warning.test.ts`（`empty` 夹具 :53、`scopeDueInput` 那条 :213-233、末尾追加 6 条）

**Interfaces:**
- Consumes：Task 1/2 的 `Expiry` 线格式与 `expiry_*` 命令名；`dueStateOf(date, today, horizonDays)`（:66，**不改它**）；`projectLabel`、`localToday`、`ISO_DATE`（同文件既有件）。
- Produces：`DueKind` 含 `"expiry"`、`DueItem.archived: boolean`、`DueInput.expiries: Expiry[]`、`EXPIRY_HORIZON_DAYS = 30`、`horizonOf(kind)`、api 封装 `listExpiries / createExpiry / updateExpiry / deleteExpiry`（Task 5 的面板吃这四个名字）。

> **为什么页面的拉数接线落在本任务而不是 Task 6**：`DueInput` 加了必填的 `expiries` 之后，`todos.tsx` 那句 `scopeDueInput({ todos, milestones, projects })` 会当场 `tsc` 红 —— 每个任务收尾都要能过 `npm run build`，所以「多拉一份数据、四份一起缩」必须和纯函数改动同一格落。Task 6 只剩**展示层**（块头文案与行尾归档标注），那部分不动类型。

- [ ] **Step 1: 先把测试改红（新夹具 + 6 条新断言 + 把「三份一起缩」那条改成四份）**

`tests/due-warning.test.ts` 顶部 import 补两个名字：

```ts
import { buildDueWarnings, dueStateOf, localToday, scopeDueInput, DUE_HORIZON_DAYS, EXPIRY_HORIZON_DAYS, horizonOf } from "../src/lib/due-warning.ts";
import type { Expiry } from "../src/types/expiry.ts";
```

`empty` 夹具（:53）改成四份，并在它**上面**加一支 `expiry` 夹具（形状与 `todo`/`milestone` 同形，`expireDate` 是必填所以夹具里也给默认值）：

```ts
const expiry = (over: Partial<Expiry> = {}): Expiry => ({
  id: "x1",
  projectId: "p1",
  kind: "域名",
  label: "oa.gov.cn",
  expireDate: iso(20),
  note: "",
  createdAt: "2026-10-01 00:00:00",
  updatedAt: "2026-10-01 00:00:00",
  ...over,
});

const empty = {
  todos: [] as Todo[],
  milestones: [] as Milestone[],
  expiries: [] as Expiry[],
  projects: [] as Project[],
};
```

> `expiry` 夹具用了 `iso(20)`，所以它必须定义在 `iso` 之后、`empty` 之前（文件里 `iso` 在 :11，`project` 夹具在 :16 起 —— 把 `expiry` 插在 `milestone` 夹具与 `empty` 之间即可）。

> **还有三处既有调用点必须一起补 `expiries: []`（控制方派发前实测发现，行号会漂 ⇒ 按测试名找）**：
> `没日期的行整个跳过，且不进 unparsed`、`三来源汇总 + 排序：overdue 在前、再按日期、再按项目名`、
> `同一天两条 ⇒ 第三级键走项目名的拼音序，不是码点序` 这三条喂给 `buildDueWarnings` 的是**字面量对象**、
> 没有 spread `empty`。`DueInput` 加了必填 `expiries` 之后：`npm run build` 在 `tsc` 层报「Property 'expiries' is missing」，
> 而 `node --test` 走的是类型擦除（不查类型），会在新的 `for (const e of input.expiries)` 上抛
> `input.expiries is not iterable` ⇒ **三条既有测试一起红在崩溃上**，看着像实现者写坏了循环。
> 每处补一行 `expiries: [],`（放在 `milestones` 之后、`projects` 之前，与 `DueInput` 的键序一致）。

六条新断言追加到文件末尾：

```ts
test("第四类来源：到期事项进 items，label 念出种类与名称，kind 是 expiry", () => {
  const r = buildDueWarnings({ ...empty, expiries: [expiry()], projects: [project()] }, TODAY);
  assert.deepEqual(r.items.map((i) => [i.kind, i.label, i.state, i.days]), [["expiry", "域名 oa.gov.cn", "soon", 20]]);
  assert.equal(r.unparsed, 0);
});

test("到期事项第 30 天亮、第 31 天不亮（D3 的边界，两边都要钉）", () => {
  const in30 = buildDueWarnings({ ...empty, expiries: [expiry({ expireDate: iso(30) })], projects: [project()] }, TODAY);
  assert.equal(in30.items.length, 1, "第 30 天必须亮（含边界）");
  const in31 = buildDueWarnings({ ...empty, expiries: [expiry({ expireDate: iso(31) })], projects: [project()] }, TODAY);
  assert.equal(in31.items.length, 0, "第 31 天不许亮 —— 把 30 改成 31 或把窗口拉宽都要在这里红");
  const past = buildDueWarnings({ ...empty, expiries: [expiry({ expireDate: iso(-3) })], projects: [project()] }, TODAY);
  assert.deepEqual(past.items.map((i) => [i.state, i.days]), [["overdue", -3]], "过去的到期日亮红且 days 为负");
});

test("归档项目的到期事项照常亮，且 archived=true（D4，与 M10 工时那格同形）", () => {
  const r = buildDueWarnings(
    { ...empty, expiries: [expiry()], projects: [project({ status: "归档" })] },
    TODAY,
  );
  assert.equal(r.items.length, 1, "归档门只包交付截止那一类，不包到期事项");
  assert.equal(r.items[0].archived, true, "界面要靠这个布尔在行尾念「（归档项目）」");
  const live = buildDueWarnings({ ...empty, expiries: [expiry()], projects: [project({ status: "开发" })] }, TODAY);
  assert.equal(live.items[0].archived, false, "未归档的项目不能误标");
});

test("待办/里程碑/交付截止仍用 7 天：第 8 天的待办不因本轮变亮（防把 30 传给他们）", () => {
  const r = buildDueWarnings(
    {
      ...empty,
      todos: [todo({ dueDate: iso(8) })],
      milestones: [milestone({ planDate: iso(8) })],
      projects: [project({ deliveryDeadline: iso(8) })],
    },
    TODAY,
  );
  assert.equal(r.items.length, 0, "三来源落在第 8 天 ⇒ 7 天窗口外，一条都不该亮");
  assert.equal(horizonOf("todo"), DUE_HORIZON_DAYS);
  assert.equal(horizonOf("milestone"), DUE_HORIZON_DAYS);
  assert.equal(horizonOf("deadline"), DUE_HORIZON_DAYS);
  assert.equal(horizonOf("expiry"), EXPIRY_HORIZON_DAYS, "只有到期事项用长窗口");
});

test("形状不对的到期日计入 unparsed，不亮也不静默吞（判据②搬到第四类）", () => {
  const r = buildDueWarnings(
    { ...empty, expiries: [expiry({ expireDate: "10月底前" })], projects: [project()] },
    TODAY,
  );
  assert.equal(r.items.length, 0);
  assert.equal(r.unparsed, 1, "坏形状的到期日要计数，界面那句「N 条无法解析」才念得对");
});

test("EXPIRY_HORIZON_DAYS 钉死为 30（阈值钉值，不钉符号）", () => {
  assert.equal(EXPIRY_HORIZON_DAYS, 30);
  assert.equal(DUE_HORIZON_DAYS, 7, "= 上一条同理由：常量漂了要在这里红，别靠边界用例间接发现");
});
```

把既有的 `scopeDueInput：三份一起缩`（:213）整条替换成四份版：

```ts
test("scopeDueInput：四份一起缩 ⇒ 别项目的行既不出现、也不计进 unparsed", () => {
  const input = {
    todos: [
      todo({ id: "t-jia", projectId: "pj", dueDate: iso(-1) }),
      todo({ id: "t-yi", projectId: "py", dueDate: iso(-1) }),
    ],
    milestones: [milestone({ id: "m-yi", projectId: "py", planDate: iso(-1) })],
    expiries: [
      expiry({ id: "x-jia", projectId: "pj", label: "甲的域名" }),
      expiry({ id: "x-yi", projectId: "py", label: "乙的证书" }),
    ],
    projects: [
      project({ id: "pj", name: "甲项目", code: null }),
      project({ id: "py", name: "乙项目", code: null, deliveryDeadline: "10月底前" }),
    ],
  };
  const all = buildDueWarnings(input, TODAY);
  assert.equal(all.items.length, 5, "不筛时五行都该在（t-jia / t-yi / m-yi / x-jia / x-yi；下面那半条的对照读数）");
  assert.equal(all.unparsed, 1, "乙项目那个「10月底前」在全量口径里计 unparsed");

  const scoped = buildDueWarnings(scopeDueInput(input, "pj"), TODAY);
  assert.deepEqual(
    scoped.items.map((i) => [i.kind, i.id]),
    [["todo", "t-jia"], ["expiry", "x-jia"]],
    "筛到甲项目 ⇒ 预警只剩甲的行（第四类也一起缩，摘掉 expiries 那份筛选这条当场红）",
  );
  assert.equal(scoped.unparsed, 0, "别的项目的坏日期不该记进当前筛选的 unparsed 数");
  assert.equal(scopeDueInput(input, null), input, "null = 全部项目，原样返回同一份引用");
});
```

- [ ] **Step 2: 跑 node，确认红在「没有 `EXPIRY_HORIZON_DAYS` / `expiries` 这些件」**

Run: `node --test "tests/**/*.test.ts" > /tmp/t4-red.log 2>&1; echo "exit=$?"; grep -E "^# (tests|pass|fail)|SyntaxError|does not export|TypeError" /tmp/t4-red.log | head -10`
Expected：`exit=1`，报 `does not provide an export named 'EXPIRY_HORIZON_DAYS'`（本轮的 RED 在导入层，因为纯函数还没这件 —— **不许**把这份缺件读成「测试写错了」）。

- [ ] **Step 3: 改 `src/lib/due-warning.ts`（五处）**

顶部 import 与常量（:3-7）：

```ts
import type { Project } from "@/types/project";
import type { Milestone } from "@/types/milestone";
import type { Todo } from "@/types/todo";
import type { Expiry } from "@/types/expiry";

export const DUE_HORIZON_DAYS = 7;

/** 到期事项单独一个更长的窗口（spec D3）：域名/证书续费要走合同与审批，7 天等于到期当天才亮红、那天已经晚了。 */
export const EXPIRY_HORIZON_DAYS = 30;

export type DueKind = "todo" | "milestone" | "deadline" | "expiry";
```

`DueItem` 加一个显示用的布尔（放在 `days` 之后），`DueInput` 加第四份：

```ts
  /** 距今天的日历日数：overdue 为负，含今天为 0。 */
  days: number;
  /** 所属项目是否已归档（D4：归档项目的到期事项照常亮，但界面必须念出来，别让人以为录错）。 */
  archived: boolean;
}

export interface DueInput {
  todos: Todo[];
  milestones: Milestone[];
  expiries: Expiry[];
  projects: Project[];
}
```

`horizonOf` 与 `scopeDueInput`：

```ts
/** 窗口按类只在这一处选（M9 终审 I2 的同一条规矩：比较本体 `dueStateOf` 只有一份，谁也不许再写第二份日期比较）。
 *  把「到期事项用 30 天」写成两份（一处 emit、一处面板）就是下一次分叉的开始。 */
export function horizonOf(kind: DueKind): number {
  return kind === "expiry" ? EXPIRY_HORIZON_DAYS : DUE_HORIZON_DAYS;
}

export function scopeDueInput(input: DueInput, projectId: string | null): DueInput {
  if (projectId === null) return input;
  return {
    todos: input.todos.filter((t) => t.projectId === projectId),
    milestones: input.milestones.filter((m) => m.projectId === projectId),
    expiries: input.expiries.filter((e) => e.projectId === projectId),
    projects: input.projects.filter((p) => p.id === projectId),
  };
}
```

`buildDueWarnings` 的签名、`emit` 与第四个循环。**第三参数别丢**：既有件是 `buildDueWarnings(input, today, horizonDays = DUE_HORIZON_DAYS)`，既有测试「horizonDays 传 0 ⇒ 只算今天与已逾期」（`tests/due-warning.test.ts:152`）吃的就是这一个显式覆盖位 —— 把它换成内部 `horizonOf(kind)` 而丢掉参数，那条测试会红、测试钩子也被顺手拆掉（本计划自审第三问抓到的正是这一条）。签名改成：

```ts
export function buildDueWarnings(
  input: DueInput,
  today: string,
  /** 给定时**所有类**都用它（既有的覆盖位，M9 T7 变异 M⑦ 那条常量的消费点）；
   *  省略时按类走 `horizonOf(kind)` —— 这才是 D3「到期事项单独 30 天」的唯一落点。 */
  horizonOverride?: number,
): DueReport {
```

```ts
  const emit = (kind: DueKind, id: string, projectId: string, label: string, raw: string | null) => {
    if (raw === null || raw === "") return;
    if (!ISO_DATE.test(raw)) {
      unparsed += 1;
      return;
    }
    const hit = dueStateOf(raw, today, horizonOverride ?? horizonOf(kind));
    if (hit === null) return;
    const p = byId.get(projectId);
    items.push({
      kind,
      id,
      projectId,
      projectLabel: p ? projectLabel(p) : "（项目缺失）",
      label,
      date: raw,
      state: hit.state,
      days: hit.days,
      archived: p?.status === "归档",
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
    // 判据⑤：**只有这一类**带归档门（交付截止不催已退役的项目）。到期事项不跟着这道门走 —— 见 D4。
    if (p.status === "归档") continue;
    emit("deadline", p.id, p.id, "交付截止", p.deliveryDeadline);
  }
  for (const e of input.expiries) {
    // D4：归档项目的到期事项照常亮（归档是管理上的退役，域名/证书/维保是还在计费的事实）。
    emit("expiry", e.id, e.projectId, `${e.kind} ${e.label}`, e.expireDate);
  }
```

> `items.sort(...)` 那一段**一字不改**（`state → date → 项目名拼音序 → id`）。30 天窗口的条目日期通常更远，自然排在逾期条目之后。

- [ ] **Step 4: 加 `api.ts` 四个封装**

import 块补一行（跟在 `import type { Milestone, ... } from "@/types/milestone";`(:21) 之后）：

```ts
import type { Expiry, ExpiryInput, ExpiryUpdate } from "@/types/expiry";
```

封装区末尾（M10 那一段之后）追加：

```ts
// —— M11-a 到期事项：四个封装 ——
// list 的 projectId 传显式 `null` 而不是省键（同 M9/M10 那条告诫）。
export const listExpiries = (projectId: string | null) => invoke<Expiry[]>("expiry_list", { projectId });
export const createExpiry = (input: ExpiryInput) => invoke<Expiry>("expiry_create", { input });
export const updateExpiry = (id: string, input: ExpiryUpdate) => invoke<void>("expiry_update", { id, input });
export const deleteExpiry = (id: string) => invoke<void>("expiry_delete", { id });
```

- [ ] **Step 5: `/todos` 页拉第四份数据，四份一起缩**

`src/pages/todos.tsx` 的 import：把 `listMilestones, listProjects, listTodos`(:21-23) 那组补成含 `listExpiries`，并补类型 import：

```ts
  listExpiries,
```

```ts
import type { Expiry } from "@/types/expiry";
```

state 区（`const [milestones, setMilestones] = useState<Milestone[]>([]);` 那一组旁边）加：

```ts
  const [expiries, setExpiries] = useState<Expiry[]>([]);
```

`reload`（:137-148）改成四路一起拉：

```ts
      const [t, m, x, p] = await Promise.all([
        listTodos(null),
        listMilestones(null),
        listExpiries(null),
        listProjects(),
      ]);
      setTodos(t);
      setMilestones(m);
      setExpiries(x);
      setProjects(p);
      setError(null);
```

`scoped` 那个 `useMemo`（:162-165）改成四份并补依赖：

```ts
  const scoped = useMemo(
    () => scopeDueInput({ todos, milestones, expiries, projects }, filter === ALL_PROJECTS ? null : filter),
    [todos, milestones, expiries, projects, filter],
  );
```

- [ ] **Step 6: 跑 node 全套 + `npm run build`，确认 47 与 tsc 0 错**

Run:
```bash
node --test "tests/**/*.test.ts" > /tmp/t4-green.log 2>&1; echo "node=$?"; grep -E "^ℹ (tests|pass|fail)" /tmp/t4-green.log
npm run build > /tmp/t4-build.log 2>&1; echo "build=$?"; grep -c "error TS" /tmp/t4-build.log
cd src-tauri && cargo test -j 1 --lib > /tmp/t4-cargo.log 2>&1; echo "cargo=$?"; grep "test result" /tmp/t4-cargo.log
```
Expected：`node=0` 且 **tests 47 / pass 47 / fail 0**（41 + 6；那条「三份一起缩」是改名不增数）；`build=0`、`error TS` **0**；`cargo=0` 且 **318 passed** 不变。

- [ ] **Step 7: 变异自证（本任务两处新防线，各打一次，按 `cp` + `cmp` 还原）**

1. 把 `due-warning.ts` 里 `scopeDueInput` 的 `expiries: input.expiries.filter(...)` 改成 `expiries: input.expiries` ⇒ `node --test` 必须红在「四份一起缩」那条（Task 8 的 **M④** 正式靶，先在这格确认它可红）。
2. 把 `horizonOf` 的函数体改成 `return DUE_HORIZON_DAYS;` ⇒ 必须红在「第 30 天亮」与 `horizonOf("expiry")` 那两条（Task 8 的 **M③** 正式靶）。

Run（每次改完）：`node --test "tests/**/*.test.ts" > /tmp/t4-mut.log 2>&1; echo "exit=$?"; grep -E "^ℹ (tests|pass|fail)" /tmp/t4-mut.log`
Expected：两次都 `exit=1` 且红在那两条具名断言上；然后 `cmp` 验字节还原、复跑 Step 6 确认 47 全绿。**若某一次全绿，说明那条断言恒绿 —— 停下来报告，别改测试凑绿。**

- [ ] **Step 8: 提交并推**

```bash
git add src/lib/due-warning.ts src/lib/api.ts src/pages/todos.tsx tests/due-warning.test.ts
git commit -m "feat(m11-a): 预警补第四类来源（到期事项）且单独 30 天窗口 —— horizonOf 一处选窗口、scopeDueInput 四份一起缩、/todos 多拉一路数据；node 41→47，两处新防线各自变异验证过"
git push origin feat/m11-expiry-fields
```

---

### Task 5: `expiry-panel.tsx` 面板 + 项目详情页挂载

**Files:**
- Create: `src/components/expiry-panel.tsx`
- Modify: `src/pages/project-detail.tsx`（import 区、:263-270 那两段面板容器）
- Test: 无新增（**页面层没有组件测试网** —— spec §十一 明写，本任务的验收只有 `tsc` + `vite build` + Task 8 的真机格 2/7/11/12）

**Interfaces:**
- Consumes：Task 4 的 `listExpiries / createExpiry / updateExpiry / deleteExpiry`、`Expiry / ExpiryInput / ExpiryUpdate / EXPIRY_KINDS`、`dueStateOf / horizonOf / localToday`（Task 4 的 `horizonOf`）、`toAppError`。
- Produces：项目详情页上那段「到期事项」面板（Task 8 真机格 2/7/11/12 的操作对象）。

- [ ] **Step 1: 写 `src/components/expiry-panel.tsx`（整段，形制照 `milestone-panel.tsx`，差异只在「没有勾选态 + 行上可改日期与备注」）**

```tsx
// 到期事项（需求 思维导图.md:73「维保、域名、证书到期提醒」+ spec D7 当次加的「其他」）。
// 与里程碑面板差在两处：① 没有 done 勾选态（D2：续期 = 改日期，不留勾选态）；
// ② 行上提供「改日期 / 改备注」两件事，因为续期是本表的主操作 —— 种类与名称录错走删了重建，
// 所以这里**不出现**改 kind/label 的入口（spec §二 自拍①，ExpiryUpdate 里也没有那两个字段）。
import { useCallback, useEffect, useState } from "react";
import { Plus, Save, Trash2 } from "lucide-react";
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
import { createExpiry, deleteExpiry, listExpiries, updateExpiry } from "@/lib/api";
import { dueStateOf, horizonOf, localToday, type DueState } from "@/lib/due-warning";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { EXPIRY_KINDS, type Expiry } from "@/types/expiry";

// 到期日的空值不在面板里折 NULL：`req_date` 要收到 "" 才能回一句点名「到期日期」的 invalid_input，
// 所以这里原样传字符串（后端 Option<String>，空串与缺键同样拒）。
const KIND = EXPIRY_KINDS[0];

export function ExpiryPanel({ projectId }: { projectId: string }) {
  const [rows, setRows] = useState<Expiry[]>([]);
  const [kind, setKind] = useState<string>(KIND);
  const [label, setLabel] = useState("");
  const [expireDate, setExpireDate] = useState("");
  const [note, setNote] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftDate, setDraftDate] = useState("");
  const [draftNote, setDraftNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);

  const reload = useCallback(async () => {
    try {
      setRows(await listExpiries(projectId));
      setError(null);
    } catch (e) {
      setError(toAppError(e));
    }
  }, [projectId]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // run 的契约与 milestone-panel/todos 页同一份：吞异常进 state、回报成败，
  // 所以「成功后才清输入框」必须靠 `if (!ok) return`，不能挂在 .then 上无条件执行。
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

  const today = localToday();
  // 与预警块**同一份**判据与同一个窗口（`horizonOf("expiry")` = 30 天）：面板自己不许再写一份日期比较。
  const stateOf = (date: string): DueState | null => dueStateOf(date, today, horizonOf("expiry"))?.state ?? null;

  return (
    <div>
      <div className="mb-1 flex items-center justify-between">
        <h2 className="text-sm font-medium">到期事项</h2>
        <span className="text-xs text-muted-foreground">
          种类固定四类（维保/域名/证书/其他）；续期就改日期，不再生效就删掉。徽标与预警块同判据：红=已逾期，描边=今天起 30 天内
        </span>
      </div>

      {error && (
        <div className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}

      <div className="mb-3 flex flex-wrap items-center gap-2">
        <Select value={kind} onValueChange={setKind}>
          <SelectTrigger className="w-28">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {EXPIRY_KINDS.map((k) => (
              <SelectItem key={k} value={k}>
                {k}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Input
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          placeholder="名称（如 oa.gov.cn、年度维保合同）"
          className="min-w-40 flex-1"
        />
        <Input type="date" value={expireDate} onChange={(e) => setExpireDate(e.target.value)} className="w-44" />
        <Input
          value={note}
          onChange={(e) => setNote(e.target.value)}
          placeholder="备注（可留空）"
          className="min-w-40 flex-1"
        />
        <Button
          size="sm"
          disabled={busy}
          onClick={() =>
            void run(() => createExpiry({ projectId, kind, label: label.trim(), expireDate, note: note.trim() }))
              .then((ok) => {
                if (!ok) return; // 被后端拒了就留着用户刚敲的内容（同 milestone-panel 终审 I3 那条契约）
                setLabel("");
                setExpireDate("");
                setNote("");
              })
          }
        >
          <Plus /> 记一笔
        </Button>
      </div>

      {rows.length === 0 && <p className="py-4 text-center text-sm text-muted-foreground">还没有到期事项。</p>}

      {rows.map((e) => {
        const hit = stateOf(e.expireDate);
        const editing = editingId === e.id;
        return (
          <div key={e.id} className="flex flex-wrap items-center gap-2 border-b px-1 py-2 last:border-b-0">
            <Badge variant={hit === "overdue" ? "destructive" : hit === "soon" ? "outline" : "secondary"}>
              {e.kind}
            </Badge>
            <span className="min-w-0 flex-1 truncate text-sm" title={e.label}>
              {e.label}
            </span>
            {editing ? (
              <>
                <Input type="date" value={draftDate} onChange={(ev) => setDraftDate(ev.target.value)} className="w-44" />
                <Input value={draftNote} onChange={(ev) => setDraftNote(ev.target.value)} placeholder="备注" className="min-w-32 flex-1" />
                <Button
                  size="sm"
                  disabled={busy}
                  onClick={() =>
                    void run(() => updateExpiry(e.id, { expireDate: draftDate, note: draftNote.trim() })).then((ok) => {
                      if (ok) setEditingId(null);
                    })
                  }
                >
                  <Save /> 存
                </Button>
                <Button size="sm" variant="ghost" disabled={busy} onClick={() => setEditingId(null)}>
                  取消
                </Button>
              </>
            ) : (
              <>
                <span className="w-24 shrink-0 font-mono text-xs text-muted-foreground">{e.expireDate}</span>
                <span className="min-w-0 max-w-40 truncate text-xs text-muted-foreground" title={e.note}>
                  {e.note}
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => {
                    setEditingId(e.id);
                    setDraftDate(e.expireDate);
                    setDraftNote(e.note);
                  }}
                >
                  续期/改备注
                </Button>
                <Button size="sm" variant="ghost" disabled={busy} onClick={() => void run(() => deleteExpiry(e.id))}>
                  <Trash2 />
                </Button>
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
```

- [ ] **Step 2: 挂到项目详情页（在里程碑面板那段之后加一段同形容器）**

`src/pages/project-detail.tsx` 的 import 区补：

```tsx
import { ExpiryPanel } from "@/components/expiry-panel";
```

容器（把 :267-269 那段 `MilestonePanel` 之后接上）：

```tsx
      <div className="mt-4 rounded-xl border bg-background p-4">
        <MilestonePanel projectId={project.id} />
      </div>

      <div className="mt-4 rounded-xl border bg-background p-4">
        <ExpiryPanel projectId={project.id} />
      </div>
```

- [ ] **Step 3: 跑编译门（本任务没有新测试，能证的就这两道）**

Run:
```bash
npm run build > /tmp/t5-build.log 2>&1; echo "build=$?"; grep -c "error TS" /tmp/t5-build.log
node --test "tests/**/*.test.ts" > /tmp/t5-node.log 2>&1; echo "node=$?"; grep -E "^ℹ (tests|pass|fail)" /tmp/t5-node.log
cd src-tauri && cargo test -j 1 --lib > /tmp/t5-cargo.log 2>&1; echo "cargo=$?"; grep "test result" /tmp/t5-cargo.log
```
Expected：`build=0` 且 `error TS` **0**；`node=0`、**47/47/0** 不变；`cargo=0`、**318** 不变。
**如实写进账本**：面板的接线（按钮真的调到哪个命令、编辑态的显示）**没有自动化网**，留到 Task 8 真机格 2/7/11/12（HANDOFF 坑 11 那条口径：不许假称已验）。

- [ ] **Step 4: 提交并推**

```bash
git add src/components/expiry-panel.tsx src/pages/project-detail.tsx
git commit -m "feat(m11-a): 到期事项面板（四类下拉 + 记一笔 + 行上续期/改备注 + 删除）挂进项目详情页 —— tsc 0 错、node 47、cargo 318 不变；面板接线无自动化网，取证留真机"
git push origin feat/m11-expiry-fields
```

---

### Task 6: 预警块的展示层（四处一起算的文案 + 双窗口 + 归档标注）

**Files:**
- Modify: `src/pages/todos.tsx`（预警块块头 :197-200 那句 `<p>`、行渲染 :209-219 那圈 `<li>`）
- Test: 无新增（展示层，判据本体在 Task 4 已有断言）

**Interfaces:**
- Consumes：Task 4 的 `DueItem.archived`、`EXPIRY_HORIZON_DAYS`（块头那句要念出两个窗口值，从常量念、不写死数字）。
- Produces：预警块在界面上对第四类的呈现（真机格 7/10 的对象）。

- [ ] **Step 1: 改块头那句文案**

`src/pages/todos.tsx` :197-200 那段 `<p>` 换成（**窗口值从常量来**，别在文案里写死「7 天 / 30 天」—— M9 终审 I5 与 M10 都因写死文案漂过一次）：

```tsx
        <p className="mt-1 text-xs text-muted-foreground">
          四处一起算：未完成的待办截止、未勾选的里程碑计划日期、未归档项目的交付截止、到期事项。
          红=已逾期，描边=今天起 {DUE_HORIZON_DAYS} 天内；到期事项单独看 {EXPIRY_HORIZON_DAYS} 天内（续费要走审批，窗口更长），归档项目的到期事项照常提醒。
        </p>
```

import 补两个常量：

```ts
import {
  buildDueWarnings,
  DUE_HORIZON_DAYS,
  EXPIRY_HORIZON_DAYS,
  localToday,
  projectLabel,
  scopeDueInput,
  type DueItem,
} from "@/lib/due-warning";
```

- [ ] **Step 2: 行尾念出「（归档项目）」**

:209-219 那个 `<li>` 里，把项目名那一段换成：

```tsx
            {due.items.map((i: DueItem) => (
              <li key={`${i.kind}-${i.id}`} className="flex items-center gap-2 text-sm">
                <Badge variant={i.state === "overdue" ? "destructive" : "outline"}>
                  {i.state === "overdue" ? `逾期 ${-i.days} 天` : i.days === 0 ? "今天" : `${i.days} 天后`}
                </Badge>
                <span className="min-w-0 flex-1 truncate">
                  {i.projectLabel} · {i.label}
                  {i.archived && <span className="text-muted-foreground">（归档项目）</span>}
                </span>
                <span className="font-mono text-xs text-muted-foreground">{i.date}</span>
              </li>
            ))}
```

- [ ] **Step 3: 跑编译门**

Run: `npm run build > /tmp/t6-build.log 2>&1; echo "build=$?"; grep -c "error TS" /tmp/t6-build.log; node --test "tests/**/*.test.ts" > /tmp/t6-node.log 2>&1; echo "node=$?"; grep -E "^ℹ (tests|pass|fail)" /tmp/t6-node.log`
Expected：`build=0` 且 `error TS` **0**；`node=0`、**47/47/0** 不变。
**如实写**：块头文案与「（归档项目）」那段是否真渲染出来，只有真机能证（格 7、格 10）；`tsc` 只保证它编得过。

- [ ] **Step 4: 提交并推**

```bash
git add src/pages/todos.tsx
git commit -m "feat(m11-a): 预警块块头改成四处一起算并从常量念双窗口，行尾标注（归档项目）—— tsc 0 错、node 47 不变"
git push origin feat/m11-expiry-fields
```

---

### Task 7: 备份那句明文告警补「到期事项」（M10 终审 I1 的同一格，本轮分配到任务）

**Files:**
- Modify: `src-tauri/src/backup.rs`（`warn_plaintext_columns()` :22-24、测试 `plaintext_warning_names_the_two_m9_tables` :663-670）
- Test: 同文件既有测试（**改名 + 加断言，测试条数不变**）

**Interfaces:**
- Consumes：Task 1 的表与 Task 2 的列语义（`label` / `note` 两列是明文）。
- Produces：`warn_plaintext_columns()` 的新那句 —— 它同时进 `/backup` 界面（`backup.tsx:117` 从命令取，不手抄）与 `manifest.json` 的 `plaintext_warning` 字段。

- [ ] **Step 1: 先把断言改成会红的（改名 + 加一格）**

`src-tauri/src/backup.rs:663` 那条测试整条替换（M10 当时往 `..._two_m9_tables` 里加了日报那句而没改名，名字漂了一轮；这次一并改正）：

```rust
    /// 包里躺着什么明文，这句必须念全 —— M9 加了两张明文表、M10 加了日报、M11-a 加了到期事项。
    /// 这条是**文本门性质**的行为断言：判据本体（哪些列真明文）在 V7 的 DDL 与 `expiry::SELECT_COLUMNS` 里。
    #[test]
    fn plaintext_warning_names_every_plain_text_table_since_m9() {
        let warn = warn_plaintext_columns();
        assert!(warn.contains("待办内容"), "漏了待办的明文列：{warn}");
        assert!(warn.contains("里程碑备注"), "漏了里程碑的明文列：{warn}");
        // M10 的第三张：`daily_reports.content` 既随整库快照进包，又因 D4 进全局搜索结果页（M10 spec §九.2）。
        assert!(warn.contains("日报内容"), "漏了日报的明文列：{warn}");
        // M11-a 的第四张（spec §九.6）：到期事项的名称与备注都是明文，且**都进全局检索**（D6）。
        // M10 那次是这句话没被分配到任何一格任务，于是九次任务评审结构性看不见它 —— 本轮它是一格的必改文件。
        assert!(warn.contains("到期事项"), "漏了到期事项的明文列：{warn}");
    }
```

- [ ] **Step 2: 跑它，确认红**

Run: `cd src-tauri && cargo test -j 1 --lib plaintext_warning_names > /tmp/t7-red.log 2>&1; echo "exit=$?"; grep -E "test result|漏了到期事项" /tmp/t7-red.log | head -4`
Expected：`exit=101`，失败信息里带 `漏了到期事项的明文列`。

- [ ] **Step 3: 改那句**

`src-tauri/src/backup.rs:22-24`：

```rust
pub fn warn_plaintext_columns() -> &'static str {
    "包内非 *_cipher 列为明文：项目名/编号/负责人/客户/联系人/合同号、服务器 ip 与堡垒机、各类 url、各表 note、待办内容、里程碑备注、日报内容、到期事项名称与备注。"
}
```

- [ ] **Step 4: 跑全套 Rust 门（**条数不变**，别以为加了断言就要改数链）**

Run:
```bash
cd src-tauri && cargo test -j 1 --lib > /tmp/t7-green.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t7-green.log
cargo clippy -j 1 --all-targets --all-features -- -D warnings > /tmp/t7-cl.log 2>&1; echo "clippy=$?"
cd .. && node --test "tests/**/*.test.ts" > /tmp/t7-node.log 2>&1; echo "node=$?"; grep -E "^ℹ (tests|pass|fail)" /tmp/t7-node.log
```
Expected：`exit=0` 且 **318 passed**（加断言不加测试数）、clippy `=0`、`node=0` 47/47/0。

- [ ] **Step 5: 变异自证（Task 8 的 M⑦ 正式靶，先在这格确认它可红）**

把 `warn_plaintext_columns()` 尾部 `、到期事项名称与备注` 删掉跑一次：
Run: `cd src-tauri && cargo test -j 1 --lib plaintext_warning_names > /tmp/t7-mut.log 2>&1; echo "exit=$?"; grep -E "test result" /tmp/t7-mut.log`
Expected：`exit=101` 红在那条新断言上。然后 `cp` 备份还原 + `cmp` 验字节 + 复跑 Step 4 确认 318 全绿。

- [ ] **Step 6: 提交并推**

```bash
git add src-tauri/src/backup.rs
git commit -m "feat(m11-a): 备份包那句明文告警补上「到期事项名称与备注」并把测试改名对齐实际覆盖面 —— 318 passed（加断言不加条数）"
git push origin feat/m11-expiry-fields
```

### Task 8: 全量门禁 + 七处变异取证 + 真机十四格与双向映射表

**Files:**
- Create: `.superpowers/sdd/2026-10-19-m11-expiry-fields/mutation-evidence.txt`、`real-machine-readings.md`、`m11-cdp.mjs`、`spec-plan-mapping.md`（**这一目录不入库**，被文档按路径引用，收口时别删 —— M10 的账本先例）
- Modify: 无产品代码。**本任务不许改实现**：变异跑完必须按字节还原。

**Interfaces:**
- Consumes：Task 1–7 的全部交付。
- Produces：终态读数 + 变异证据 + 真机读数 + 「spec 格 ↔ 计划格」双向映射表（Task 9 写活文档时逐条引用）。

- [ ] **Step 1: 全量门禁（退出码逐个真取，绝不 `cmd | tail; echo $?`）**

```bash
cd src-tauri
cargo test -j 1 --lib > /tmp/t8-cargo.log 2>&1; echo "cargo=$?"
cargo clippy -j 1 --lib -- -D warnings > /tmp/t8-cl.log 2>&1; echo "clippy-lib=$?"
cargo clippy -j 1 --all-targets --all-features -- -D warnings > /tmp/t8-cla.log 2>&1; echo "clippy-all=$?"
cargo build -j 1 --lib > /tmp/t8-build.log 2>&1; echo "rust-build=$?"
cd ..
node --test "tests/**/*.test.ts" > /tmp/t8-node.log 2>&1; echo "node=$?"
grep -E "^ℹ (tests|pass|fail)" /tmp/t8-node.log
npm run build > /tmp/t8-web.log 2>&1; echo "web=$?"; grep -c "error TS" /tmp/t8-web.log
sed -n '/generate_handler/,/^\s*\])/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"
grep -c 'tauri::command(async)' src-tauri/src/lib.rs
grep -oE 'TARGET_VERSION: [a-z0-9]+ = [0-9]+' src-tauri/src/db.rs
grep -rn "allow(dead_code)" src-tauri/src/ | wc -l
grep -c '^\[\[package\]\]' src-tauri/Cargo.lock
git diff --stat origin/master..HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock package.json
grep -c 'hits.append(&mut collect(' src-tauri/src/search.rs
git rev-list --merges --count --all
```
Expected：`cargo=0` **318 passed**；两条 clippy `=0`；`rust-build=0`；`node=0` **47/47/0**；`web=0` 且 `error TS` **0**；结构门 **64 / 7 / 7 / 1 / 616**；`Cargo.lock` 与 `package.json` 的 diff **空输出**（`Cargo.toml` 也必须空：本轮零新依赖，与 M10 不同）；collect **9** 条；merge commit **0**。

> **命令数那条 `sed` 的范围式已改准**（Task 2 评审带回）：原文 `/generate_handler/,/^      \]/p` 的结束式对不上（收尾行是 `        ])`，六个空格加 `]` 不成立），范围一路跑到文件末尾 —— 现在读数仍是 64，但那是「后面没有别的行匹配 `^ +[a-z_]+,?$`」的运气。改成 `/generate_handler/,/^\s*\])/p`（实测范围 66 行、读数 64）。**HANDOFF §4-A 那串命令里带着同一个脆弱范围式，Task 9 回写时一并改准**，否则下一个照抄它的对话会在某天读出 65、66 然后误判成漏注册。
`os error 1455` / `E0786` / 无 `error[Exxxx]` 的编译失败 ⇒ 先重跑同一条命令（坑 32/3）。

- [ ] **Step 2: 七处变异，每处都要具名打红（`cp` 备份 + `cmp` 还原）**

| 靶 | 怎么改 | 必须红在哪 | 取证档 |
|---|---|---|---|
| **M①** | 摘 `expiry::list` 里的 `AND p.deleted_at IS NULL`；再单独摘第 9 条 collect 里那一处 | `cross_project_list_hides_expiries_of_a_soft_deleted_project` + `soft_deleted_project_hides_expiry_hits` | cargo |
| **M②** | `check_kind` 的条件改成 `true` | `create_refuses_kind_words_outside_the_four_with_invalid_input` | cargo |
| **M③** | `horizonOf` 改成 `return DUE_HORIZON_DAYS;` | 「第 30 天亮」+ `horizonOf("expiry")` 那条 node 断言 | node |
| **M③′** | `ORDER BY e.expire_date, e.kind, e.id` 摘掉 `, e.id` | `list_breaks_ties_by_id_when_date_and_kind_are_equal` | cargo |
| **M④** | `scopeDueInput` 里 `expiries` 那份 `.filter` 摘掉 | 「四份一起缩」那条 node 断言 | node |
| **M⑤** | `expiry::create` 里把 `req_date(...)` 换成 `norm_date(...)`（`use crate::todo::norm_date`） | `create_requires_expire_date_naming_the_column`（空白那半条：`norm_date` 会折成 NULL，INSERT 撞 NOT NULL 冒 `db_failed`，`err.code` 就不是 `invalid_input`） | cargo |
| **M⑥** | 第 9 条 collect 的 `WHERE` 换成只 `e.expire_date LIKE ?1` | `expiry_label_and_note_are_searchable_but_the_date_column_is_not`（备注搜不到 + 日期搜得到，两条反向都红） | cargo |
| **M⑦** | `warn_plaintext_columns()` 尾部删掉 `、到期事项名称与备注` | `plaintext_warning_names_every_plain_text_table_since_m9` | cargo |

跑法（每一靶一轮）：

```bash
cp src-tauri/src/expiry.rs /tmp/expiry.bak
# ...改一处...
cd src-tauri && cargo test -j 1 --lib > /tmp/m1.log 2>&1; echo "exit=$?"; grep -E "test result|FAILED|panicked at" /tmp/m1.log | head
cd .. && cp /tmp/expiry.bak src-tauri/src/expiry.rs && cmp /tmp/expiry.bak src-tauri/src/expiry.rs && echo RESTORED
```
把每一靶的**原始输出片段**贴进 `mutation-evidence.txt`（含测试名、红在哪一条、还原 `cmp` 的结果）。
**红线**：跑绿了就照实写「这一处打不红」并改判断据或补断言（M10 的 M⑤ 假守护就是这么抓出来的），**不许**把未跑出来的读数写进证据文件（HANDOFF 坑 29）。

- [ ] **Step 3: 真机沙盒起法（只由控制方跑，子代理一律不碰）**

```bash
# 1) 先量真实数据目录（只读红线）
node .superpowers/sdd/2026-10-19-m11-expiry-fields/realdir-snapshot.mjs before
# 2) 起一次性 identifier 的沙盒，带 9222；1520 上若残留 vite 先按 PID 确认是本仓再起（坑 28）
npm run tauri -- dev -c '{"identifier":"dev.zero.pfm.m11test"}' -- --port 9222 &
# 3) 跑断言脚本（下面 Step 4）
```

- [ ] **Step 4: 真机十四格 = spec §十 的 1–14 逐格跑（编号直接对齐，不留缺口）**

`m11-cdp.mjs` 按格分段跑：`node m11-cdp.mjs s1`…`s14`，每段只回**布尔/条数/错误码名/日期串**，不许把台账正文打印进日志（坑 11）。逐格判据以 spec §十 为准，其中要特别记清的三格：

- **格 1**：底栏 `schema=7 wal`（`db_status` 往返）。
- **格 3**：到期日留空点「记一笔」⇒ 界面念出的错误文本含「到期日期」且库里 `expiry_list` 条数不变（前端没折 NULL，见 Task 5 的注释）。
- **格 7 / 格 8**：同一份数据里既有归档项目（带到期行）又有归档项目（带 `deliveryDeadline`）⇒ 前者**亮**且行尾念「（归档项目）」，后者**不亮**。这两格是同一条 `if (p.status === "归档") continue;` 的两面，跑反了就是判据搬错。

- [ ] **Step 5: 收口前写 `spec-plan-mapping.md` 双向映射表**

左列 spec §十 的 14 格、右列实际跑到的计划格与读数所在小节；**任何一格没有读数就写在表里标「未验 + 为什么」**。「计划 1–14 全部有读数」这句只有在这张表成立时才允许出现在文档里（M10 坑 40 / 终审 I2 的直接教训）。

- [ ] **Step 6: 沙盒与进程回收、真实目录复验**

```bash
# 按自己记下的 PID 精确回收（先 Get-CimInstance 看 CommandLine 确认是本仓的 vite/tauri，绝不按映像名通杀）
node .superpowers/sdd/2026-10-19-m11-expiry-fields/realdir-snapshot.mjs after
# before/after 两次快照的 ledger.db / -wal / -shm 三文件 size+mtime+sha256 必须逐字一致
```
沙盒目录（`%APPDATA%\dev.zero.pfm.m11test\`）跑完 `rm -rf` 删净，并 `tasklist` 复核 1520/9222 无残留监听。

- [ ] **Step 7: 把证据目录与读数落盘并提交（`.superpowers/` 不入库，但账本要提交进文档的只有引用路径）**

```bash
git status --porcelain   # 确认只剩 Python/ 未跟踪 + 无产品代码残留改动
git log --oneline -8
```
Expected：`git status` 干净（除了那颗永不动的 `?? Python/`）；若 `src-tauri/src/*.rs` 还有改动，说明某一靶没还原 —— 先 `cmp` 定位再处理，**不许** `git checkout --` 一把梭（M10 用过这条路，前提是先确认那些改动全是本轮自己的）。

---

### Task 9: 活文档回写

**Files:**
- Modify: `docs/技术方案.md`（§四 数据模型补一行、§九 里程碑表 M11 行、:148 那句「八段 SQL」条数、:189 备份那段的 M11 补句）
- Modify: `docs/开发进度.md`（里程碑表加 M11-a 行、「## M11-a 验收证据」一节、变更日志一条、「没能自动验证」清单）
- Modify: `docs/HANDOFF.md`（快照头、§0 加 M11-a bullet、§3 卡点与下一格、§4-A 那串命令的期望数、§6 指针表补三行、必要时新增一条坑）
- Modify: `docs/superpowers/specs/2026-10-14-m9-todos-milestones-design.md`（**顶部加一行指针**，§一 对照表与 §十一 原文不动 —— 沿用 M10 对 M9 的处理法）
- Modify: `src-tauri/src/expiry.rs`（模块头那句前向引用已经兑现、时态要改准 —— Task 3 评审 Minor-2，见 Step 4b；纯注释，行为零改动）

**Interfaces:**
- Consumes：Task 8 的终态读数与映射表。
- Produces：下一个对话（M11-b 台账导出）的恢复地图。

- [ ] **Step 1: `docs/技术方案.md` 四处**

1. §四 数据模型表（`project_tags` 那一行附近，照 `daily_reports` 那行的措辞密度）加：

```markdown
project_expiries    到期事项（M11-a 新增，V7）：id / project_id→projects / kind(维保|域名|证书|其他) / label / expire_date TEXT NOT NULL / note / 软删三件套；无日期索引，SQL 里不做日期解释
```

2. :148 那句「八段 SQL（M10 起含日报正文与待办标题两类）」改成 **九段 SQL（M11-a 起含到期事项名称与备注）**，并把那句里的安全论断（`SELECT`/`WHERE` 只写明文列）保持原强度。
3. §九 里程碑表 M11 行改成「到期预警的后半已兑（M11-a：四类到期事项进面板 + 预警第四类 + 检索第 9 条）；**台账 Excel/Markdown 导出仍推后**，导的是躺着密文列的五表，要先单独定红线」。
4. :189 备份那段补一句：M11-a 的 `project_expiries` 同样随整库快照进包、不进 `counts`，且明文告警那句已含「到期事项名称与备注」。

- [ ] **Step 2: `docs/开发进度.md`**

里程碑表加一行 M11-a（状态 = 待合并时按实写），并新增一节「## M11-a 验收证据（到期事项 + 预警第四类，✅ 2026-10-19）」，逐条写：门禁终态读数（含退出码）、七处变异各自红在哪、真机 14 格读数与映射表路径、**没能自动验证的清单**（至少含：面板接线无 DOM 测试网、`/todos` 块头文案与「（归档项目）」标注只有真机、原生对话框不参与本轮但 `/todos` 的筛选交互仍是人眼、`PER_GROUP_LIMIT=50` 与新表的上限关系没量）。

- [ ] **Step 3: `docs/HANDOFF.md`**

- §0 加一条 M11-a bullet：**在真合并发生之前，那一格不许出现任何「合并点 = `<sha>`」的写法**，只写取数命令（`git rev-parse --short master`、`git ls-remote --heads origin`）与当次读数；合并完成后由收口提交把读数填进去 —— 本轮 M10 收口刚为「master 未推 origin」这类瞬时句补了两次提交，别再犯（见 HANDOFF 的新增那条口径）。
- §3「当前卡在哪」与「下一格动作」改成 M11-b（台账导出，先定密文红线）。
- §4-A 那串命令的期望值整串换成本轮终态（**64 / 7 / 7 / 1 / 616 / cargo 318 / node 47 / collect 9**），并把里面那条数命令数的 `sed` 范围式从 `/^      \]/` 改成 `/^\s*\])/`（Task 2 评审带回：原式对不上收尾行 `        ])`，范围跑到 EOF，读数全靠后面没有匹配行才碰巧对 —— 见 Task 8 Step 1 的那条引述）。
- §6 指针表补三行：M11-a spec、M11-a 计划、`.superpowers/sdd/2026-10-19-m11-expiry-fields/{progress.md,mutation-evidence.txt,real-machine-readings.md,spec-plan-mapping.md}`。
- 「只剩需求方能做的四格」那条照实保留（本轮一条也没兑掉）。

- [ ] **Step 4: M9 spec 顶部指针一行**

在 `docs/superpowers/specs/2026-10-14-m9-todos-milestones-design.md` 的状态行下面加：

```markdown
> **后续指针（2026-10-19）**：本文 §一 对照表与 §十一 里推给 M11 的「维保 / 域名 / 证书三类到期字段」已由 M11-a 兑现（`docs/superpowers/specs/2026-10-19-m11-expiry-fields-design.md`）。本文正文**不 retro-edit**，欠账的结算记在 M11-a spec §一 与本行。
```

- [ ] **Step 4b: `src-tauri/src/expiry.rs` 模块头那句前向引用改准（Task 3 评审 Minor-2）**

`src-tauri/src/expiry.rs` 第 4 行现在写的是「检索侧的第二处由 Task 3 那条 collect 落，届时两处各自有守卫测试」—— Task 3 已交付，这句仍是将来时 = 假话（M10 终审 I1 同族：写在计划里却没被分配到任何一格的句子）。整句换成现在时并给出可 grep 的落点：

```rust
//! `p.deleted_at IS NULL` 在本表这一处（检索侧的第二处已在 `search.rs` 的到期事项那条 collect 落，
//! 两处各自有守卫测试：本文件 cross_project_list_hides_expiries_of_a_soft_deleted_project 与
//! search.rs 的 soft_deleted_project_hides_expiry_hits）。
```

> 注释里别写字面量 `collect("expiry", …)` 的完整形态：`grep -c 'hits.append(&mut collect('` 是原始计数门（坑 42）。上面这行拆成「到期事项那条 collect」就是为它。

改完复跑 `cd src-tauri && cargo test -j 1 --lib > /tmp/t9.log 2>&1; echo "exit=$?"; grep -E "^test result" /tmp/t9.log`（期望终态读数不变）与 `cargo clippy --all-targets --all-features -- -D warnings`（`exit=0`）。

- [ ] **Step 5: 文档级复跑与提交**

文档改动不动行为，但按本仓规矩复跑受影响的结构门（grep 类）一次并记录：

```bash
grep -c "九段 SQL" docs/技术方案.md
grep -rn "八段 SQL\|八条同形状" docs/*.md | wc -l
```
Expected：前者 ≥1；后者 **0**（活文档里的旧条数要一起改准；历史规格文档里的基线句不 retro-edit，所以这里的 0 只统计 `docs/*.md` 这一层）。

```bash
git add docs/技术方案.md docs/开发进度.md docs/HANDOFF.md docs/superpowers/specs/2026-10-14-m9-todos-milestones-design.md src-tauri/src/expiry.rs
git commit -m "docs(m11-a): 活文档回写 —— 技术方案补 V7 与九段 SQL、开发进度加 M11-a 验收证据与没验清单、HANDOFF §0/§3/§4-A/§6 换成 64·7·7·1·616 终态并把下一格改成 M11-b、M9 spec 顶部加结算指针、expiry.rs 模块头前向引用改准"
git push origin feat/m11-expiry-fields
```

---

## 自审记录（控制方 2026-10-19 写完计划本体后现跑现数）

**1. spec 覆盖对照**：spec §二 D1–D8 → 分别落 Task 1（D1/D7）、Task 2（D2、自拍①②③）、Task 4（D3/D4）、Task 4+6（D5）、Task 3（D6）、Task 2 Step 4（D8）；§四 V7 → Task 1；§五 判据 → Task 2；§六 契约与结构门 → Task 2；§七 面板与预警 → Task 4/5/6；§八 检索与备份告警 → Task 3/7；§九 十二条不变量 → 分别由 Task 1（.1）、Task 2（.2/.3/.9/.10）、Task 3（.3/.4）、Task 4（.5）、Task 7（.6）、Task 1/2（.7 备份随快照，无需接线，只在文档里记）、Task 2/4（.8）、Task 5/6（页面接线无网这条在 Task 8 的未验清单）、Task 2/3/4/5（写盘落点不动，无任务）、Task 9（.12 M9 spec 指针）承接；§十 十四格 → Task 8 Step 4–5；§十二 七处变异 → Task 8 Step 2（其中 M③/M④/M⑦ 在 Task 4/7 先各自预跑一轮，正式取证仍在 Task 8）。**§九.11（写盘落点不变）没有对应任务** —— 它是「本轮不做任何落盘」的负向不变量，验收方式是 Task 8 Step 1 的 `Cargo.lock` 空 diff + 没有新命令碰 `std::fs`，已在 Task 8 的门禁里。

**2. 占位符扫描**：全文无 TBD/TODO/「参照 Task N」；每个改代码的 Step 都带可粘贴的整段代码或精确到行号的替换文本。

**3. 数链核对（本轮现跑实数，不是凭印象累加）**：
- 基线：`grep -c '#\[test\]' src-tauri/src/*.rs` 逐文件相加 = **301**（与本文件 Global Constraints 那条读数一致）；`grep -c '^test(' tests/*.test.ts` = **41**。
- Task 1：`db.rs` 新增 4 支（`v7_creates...`、`v7_expire_date...`、`v7_kind_check...`、`v7_backfills_nothing`）+ 改名 1 支（不减）⇒ **305**。
- Task 2：`expiry.rs` 10 支（`create_refuses_kind`、`create_requires_expire_date`、`create_rejects_blank_or_whitespace_label`、`create_rejects_project...`、`create_then_list_roundtrip...`、`list_breaks_ties_by_id...`、`list_hides_soft_deleted_expiries`、`cross_project_list_hides...`、`update_changes_only_date_and_note...`、`expiry_line_format...`）+ `lib.rs` 1 支（`kind_words_are_identical...`）⇒ **316**。
- Task 3：`search.rs` 2 支 ⇒ **318**。Task 4–7 均 **0 新测试**（Task 7 是往既有测试加断言并改名），故终态 cargo **318**。
- Task 4：`due-warning.test.ts` 新增 6 支（第四类来源 / 30 与 31 天边界 / 归档照常亮 / 其余三类仍 7 天 / 坏形状计 unparsed / 两个常量钉值）+ 改名 1 支（三份 → 四份，不减）⇒ node **47**。
- 结构门终态：**64 / 7 / 7 / 1 / 616**，collect **9**。

**4. 三处与 spec 的偏差（已在本文件内点名，Task 9 回写时要带上）**：
1. **`ExpiryInput.expire_date` / `ExpiryUpdate.expire_date` 用 `Option<String>`**（spec §六 草图原先写的是 `String`）。**spec 原文已在同一轮修订**（`docs/superpowers/specs/2026-10-19-m11-expiry-fields-design.md` §六 那段代码块下面补了理由），理由也抄在 Task 2 的引述块里。
2. **「`/todos` 拉第四份数据 + 四份一起缩」从 Task 6 上移到 Task 4**：`DueInput` 加必填 `expiries` 之后页面不同步改就过不了 `tsc`，而本仓每个任务收尾都要能过编译门。Task 6 只剩展示层文案与归档标注。
3. **`backup.rs` 那条测试改名** `plaintext_warning_names_the_two_m9_tables` → `plaintext_warning_names_every_plain_text_table_since_m9`：M10 往这条里加了日报断言却没改名，名字漂了一轮，本轮一并改正（条数不变，数链不受影响）。

**5. 一处 RED 形状的修正（写进来免得实现者绕）**：Task 2 Step 2 把实现与测试一起贴，所以「模块不存在」那种红只是编译层空转。真正的 RED 证据用**先删实现段**或**注入变异**（本任务指定 `check_kind` 恒真那一档）来取 —— 这条口径与 M10 Task 1 的裁定一致。

**6. 自审第三问（类型/签名一致性）抓到并改掉的一处真缺陷**：Task 4 原先把 `emit` 内的比较直接写成 `dueStateOf(raw, today, horizonOf(kind))`，那是**把既有的第三参数 `horizonDays` 拆掉**——`tests/due-warning.test.ts:152` 那条「horizonDays 传 0 ⇒ 只算今天与已逾期」会当场红，而且 M9 为变异 M⑦ 留的那个覆盖位就没了。现在改成 `buildDueWarnings(input, today, horizonOverride?: number)` + `horizonOverride ?? horizonOf(kind)`。**必须是 `??` 不能是 `||`**：既有测试传的正是 `0`，用 `||` 会把 0 当成「没传」而静默落回 `horizonOf`，那条测试会变成恒绿断言（同一族：M10 的 M⑤ 假守护）。

**7. 机械复算过的两条数链**（用命令在计划文本上现数，不是靠手加）：按 `### Task` 切段后数 `#[test]` 与 `^test(` 的块数 —— Task 1 = 5（1 改名 + 4 新增）、Task 2 = 11（expiry 10 + lib 1）、Task 3 = 2、Task 7 = 1（改名 + 加断言，净新增 0）⇒ 净新增 **17**，301 → **318**；Task 4 = 7 个 `test(` 块（6 新增 + 1 改写）⇒ 41 → **47**。上面「任务总览」表与本节因此对齐。

