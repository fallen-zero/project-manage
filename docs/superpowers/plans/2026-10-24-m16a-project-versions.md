# M16-a 项目版本台账 + 子文件选择器 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在项目详情页补上版本台账（版本号 / 上线时间 / 上线内容，增改删），并给「快捷入口」加一个原生文件选择器；数据进 V12、备份 schema 12，全程按活动工作区隔离。

**Architecture:** 新表 `project_versions`（V12，零回填）+ `version.rs` 四操作 + 四条同步命令（75→79）；前端独立 `version-panel.tsx`（本地 state，照 `milestone-panel`），项目详情页挂载。子文件入口只加前端 `pickFile()` 与按钮，后端 `add_entry_dir` 不改。

**Tech Stack:** Rust / rusqlite 0.40.2（bundled；`uuid` 已在树里）/ Tauri 2 同步命令 / React 19 + TypeScript / shadcn-radix / `node --test`（本批不加 node 测试）。

**Spec:** `docs/superpowers/specs/2026-10-24-m16a-project-versions-design.md`（plan 从 spec 论证，执行者两份一起读）

## Global Constraints

- 零新依赖：`src-tauri/Cargo.lock` 的 `[[package]]` 保持 **616**；`git diff --stat master..HEAD -- src-tauri/Cargo.lock src-tauri/Cargo.toml package.json` 必须空输出。
- 结构门终态：`generate_handler!` **79**（现 75，+4）、`tauri::command(async)` **保持 7**（四条新命令一律同步）、`allow(dead_code)` **保持 1**（`db.rs`）、`hits.append(&mut collect(` **保持 9**、`TARGET_VERSION` **12**（现 11）。
- 错误码只用既有 `invalid_input` / `not_found` / `db_failed`，不新增码名。
- SQL 一律参数绑定；表名只允许 `&'static str` 常量。
- 版本 CRUD 全程按活动工作区：`project_exists` 带 `workspace_id`，行级 update/delete 首行 `ensure_active_row("project_versions", id)`。
- 注释、用户文案、commit message 一律中文；错误码字面量与 SQL 除外。
- 基线（2026-10-24，`master 2280de4` 实测）：`cargo test -j 1 --lib` **405 passed**、`node --test "tests/**/*.test.ts"` **75/75/0**、`npm run build` **0 TS**、命令 **75** / async **7** / `TARGET_VERSION` **11** / 豁免 **1** / `Cargo.lock` **616** / collect **9**。
- 每个 Task 结尾先跑本 Task 覆盖的 target，再跑全量 `cargo test -j 1 --lib`（前端 Task 跑 `npm run build`），拿到真实退出码后才提交。
- 子代理**一律不跑** `npm run tauri dev` / `npm run dev`；真机格与变异由控制方跑。
- 新建文件后 `unix2dos`（工作树 CRLF）；`npm run build` 的 `error TS` 用 `grep -c` 另取。
- **测试条数账**（逐 target 实数；与下表不符时以实跑为准并写进账本）：基线 405 → Task 1 +3=**408** → Task 2 +7=**415**；Task 3–5 不再增 cargo。node 保持 **75**。

---

## Task 1: V12 迁移 + 备份 schema/告警

**Files:**
- Modify: `src-tauri/src/db.rs`（`TARGET_VERSION`、`migrate` match、`V12` 常量、测试）
- Modify: `src-tauri/src/backup.rs`（告警句、三处 schema 断言、告警测试补一条、新增一条备份测试）

**Interfaces:**
- Consumes: 无（本 Task 是链头）
- Produces: `project_versions(id, project_id, version, released_at, content, created_at, updated_at, deleted_at)`、`db::TARGET_VERSION == 12`、`crate::db::V12` 常量；Task 2 的版本模块建在其上。

- [ ] **Step 1: 写 V12 失败测试（表/索引 + 零回填）**

在 `db.rs` 的 `mod tests` 末尾加：

```rust
    /// V12 必须建出 project_versions 的八列与两条具名索引；唯一索引必须带 partial 条件。
    #[test]
    fn v12_project_versions_table_and_indexes_exist() {
        let conn = open_in_memory().unwrap();
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('project_versions') ORDER BY cid")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(
            cols.join(","),
            "id,project_id,version,released_at,content,created_at,updated_at,deleted_at",
            "V12 的 project_versions 列集合/顺序不对：{cols:?}"
        );
        let idx: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master
                  WHERE type='index' AND tbl_name='project_versions' AND sql IS NOT NULL
                  ORDER BY name",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(
            idx,
            vec!["ix_project_version_project", "ux_project_version_project_version"],
            "V12 的具名索引不对：{idx:?}"
        );
        let ux: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='ux_project_version_project_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(ux.contains("WHERE deleted_at IS NULL"), "唯一索引必须是 partial：{ux}");
    }

    /// V12 零回填：库停在 V11 时先落项目与标签，跑 migrate 后版本表必须 0 行，
    /// 其余业务字段一个字节都不许变。
    #[test]
    fn v12_backfills_nothing() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_meta (version INTEGER NOT NULL, applied_at TEXT NOT NULL);",
        )
        .unwrap();
        for (target, ddl) in [
            (1, V1), (2, V2), (3, V3), (4, V4), (5, V5),
            (6, V6), (7, V7), (8, V8), (9, V9), (10, V10), (11, V11),
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

        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM project_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "V12 不许回填任何版本行");
        let bits: String = conn
            .query_row(
                "SELECT name || '/' || customer FROM projects WHERE id = 'p-old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bits, "存量项目/甲客户", "V12 只建表，不许碰存量项目");
        let tag: String = conn
            .query_row("SELECT tag FROM project_tags WHERE id = 't-old'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tag, "政务");
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, TARGET_VERSION);
    }
```

- [ ] **Step 2: 写备份测试与告警断言（会红）**

在 `backup.rs` 的 `mod tests` 末尾加：

```rust
    /// M16-a 的备份半边：版本行随整库快照进包，但 Counts 仍是七张表。
    #[test]
    fn backup_package_carries_project_versions_without_counting_them() {
        let live = crate::db::open_in_memory().unwrap();
        live.execute("INSERT INTO projects (id, name) VALUES ('p1', '甲项目')", []).unwrap();
        live.execute(
            "INSERT INTO project_versions (id, project_id, version, released_at, content)
             VALUES ('ver-1', 'p1', '1.0.0', '2026-01-01', '上线一批')",
            [],
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("versions.zip");
        let sum = export(&live, &dst).unwrap();
        assert_eq!(sum.schema_version, crate::db::TARGET_VERSION);
        assert_eq!(sum.schema_version, 12, "M16-a 起包内 schema 必须是 12");
        assert_eq!(sum.counts.projects, 1);
        assert_eq!(
            serde_json::to_value(sum.counts).unwrap().as_object().unwrap().len(),
            7,
            "project_versions 是新表，不进 Counts"
        );
        let (_unpacked, got) = unpack_db(&dst);
        let row: (String, String) = got
            .query_row(
                "SELECT version, released_at FROM project_versions WHERE id = 'ver-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, ("1.0.0".to_owned(), "2026-01-01".to_owned()));
    }
```

在既有 `plaintext_warning_names_every_plain_text_table_since_m9` 里补一条：

```rust
        assert!(warn.contains("版本号与上线内容"), "漏了版本台账的明文：{warn}");
```

Run: `cd src-tauri && cargo test -j 1 --lib v12_`
Expected: FAIL（`no such table: project_versions`）。
Run: `cd src-tauri && cargo test -j 1 --lib backup_package_carries_project_versions`
Expected: FAIL（编译/建表失败）。

- [ ] **Step 3: 加 V12 DDL 与迁移链**

在 `db.rs` 的 V11 常量之后加：

```rust
/// v12：项目版本台账（M16-a spec §三）。一行一次上线；软删；同项目版本号唯一（软删不占号）。
const V12: &str = "
CREATE TABLE project_versions (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    version     TEXT NOT NULL,
    released_at TEXT NOT NULL,
    content     TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at  TEXT
);

CREATE INDEX ix_project_version_project ON project_versions(project_id, released_at);

CREATE UNIQUE INDEX ux_project_version_project_version
    ON project_versions(project_id, version)
    WHERE deleted_at IS NULL;
";
```

`TARGET_VERSION` 11→**12**；`migrate` 的 match 加 `12 => V12,`。

- [ ] **Step 4: 备份 schema 与告警**

- `backup.rs` 的 `warn_plaintext_columns()` 句末在「项目修改日志的字段旧值/新值（含联系人、联系方式）」之前补「、版本号与上线内容」。
- 三处 `assert_eq!(sum.schema_version, 11, ...)` / `assert_eq!(sum.schema_version, 11);` 全部改 **12**，并把注释里的「M15 起」改成「M16-a 起」。

Run: `cd src-tauri && cargo test -j 1 --lib v12_`
Expected: 2 passed。
Run: `cd src-tauri && cargo test -j 1 --lib backup_package_carries_project_versions`
Expected: 1 passed。
Run: `cd src-tauri && cargo test -j 1 --lib plaintext_warning_names_every_plain_text_table_since_m9`
Expected: PASS。

- [ ] **Step 5: 全量 + 提交**

Run: `cd src-tauri && cargo test -j 1 --lib`
Expected: `408 passed; 0 failed`（405 + 3）。

```bash
git add src-tauri/src/db.rs src-tauri/src/backup.rs
git commit -m "feat(m16a): V12 项目版本表 + 备份 schema 12 与明文告警"
```

---

## Task 2: `version.rs` + 四条同步命令

**Files:**
- Create: `src-tauri/src/version.rs`
- Modify: `src-tauri/src/lib.rs`（`mod version;`、type import、四条命令、`generate_handler!`）
- Modify: `src-tauri/src/workspace.rs`（`ensure_active_row` 注释「九张表」→「十张表」）

**Interfaces:**
- Consumes: Task 1 的 `project_versions` 表；`workspace::active_id` / `ensure_active_row`；`todo::req_date`。
- Produces:
  - `version::Version { id, project_id, version, released_at, content, created_at, updated_at }`
  - `version::VersionInput { project_id, version, released_at: Option<String>, content: Option<String> }`
  - `version::VersionUpdate { version, released_at: Option<String>, content: Option<String> }`
  - `list(conn, project_id) -> AppResult<Vec<Version>>`、`create(conn, &VersionInput) -> AppResult<Version>`、`update(conn, id, &VersionUpdate) -> AppResult<Version>`、`delete(conn, id) -> AppResult<()>`
  - 命令 `version_list/create/update/delete`（命令数 75→79）。

- [ ] **Step 1: 写 `version.rs` 的失败测试**

新建 `src-tauri/src/version.rs`，先只写测试模块（实现暂缺会编译失败，属预期 RED）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::Connection;

    fn seed(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, status) VALUES (?1, '版本测试项目', '立项')",
            [id],
        )
        .unwrap();
    }

    fn input(project_id: &str, version: &str, date: Option<&str>, content: Option<&str>) -> VersionInput {
        VersionInput {
            project_id: project_id.to_owned(),
            version: version.to_owned(),
            released_at: date.map(str::to_owned),
            content: content.map(str::to_owned),
        }
    }

    #[test]
    fn create_rejects_blank_version_and_missing_date() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        assert_eq!(create(&conn, &input("p1", "   ", Some("2026-01-01"), None)).unwrap_err().code, "invalid_input");
        assert_eq!(create(&conn, &input("p1", "1.0.0", None, None)).unwrap_err().code, "invalid_input");
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM project_versions", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn create_and_list_versions_in_release_order() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let v1 = create(&conn, &input("p1", "1.0.0", Some("2026-01-01"), None)).unwrap();
        assert_eq!(v1.content, "", "内容缺省落空串");
        create(&conn, &input("p1", "1.1.0", Some("2026-02-01"), Some("上线二期"))).unwrap();
        let rows = list(&conn, "p1").unwrap();
        assert_eq!(rows.iter().map(|v| v.version.as_str()).collect::<Vec<_>>(), vec!["1.1.0", "1.0.0"]);
    }

    #[test]
    fn duplicate_version_is_rejected_on_create() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        create(&conn, &input("p1", "1.0.0", Some("2026-01-01"), None)).unwrap();
        assert_eq!(
            create(&conn, &input("p1", " 1.0.0 ", Some("2026-01-02"), None)).unwrap_err().code,
            "invalid_input"
        );
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM project_versions", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn duplicate_version_is_rejected_on_update() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let a = create(&conn, &input("p1", "1.0.0", Some("2026-01-01"), None)).unwrap();
        let b = create(&conn, &input("p1", "1.1.0", Some("2026-02-01"), None)).unwrap();
        assert_eq!(
            update(&conn, &b.id, &VersionUpdate { version: "1.0.0".to_owned(), released_at: Some("2026-02-01".to_owned()), content: None })
                .unwrap_err()
                .code,
            "invalid_input"
        );
        // 改回自身原版本必须允许
        update(&conn, &b.id, &VersionUpdate { version: "1.1.0".to_owned(), released_at: Some("2026-02-02".to_owned()), content: Some("补记".to_owned()) }).unwrap();
        assert_eq!(get(&conn, &a.id).unwrap().version, "1.0.0");
    }

    #[test]
    fn update_and_delete_are_soft_and_allow_recreate() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let v = create(&conn, &input("p1", "1.0.0", Some("2026-01-01"), None)).unwrap();
        update(&conn, &v.id, &VersionUpdate { version: "1.0.1".to_owned(), released_at: Some("2026-01-02".to_owned()), content: Some("修补".to_owned()) }).unwrap();
        delete(&conn, &v.id).unwrap();
        assert!(list(&conn, "p1").unwrap().is_empty());
        let deleted: Option<String> = conn
            .query_row("SELECT deleted_at FROM project_versions WHERE id = ?1", [&v.id], |r| r.get(0))
            .unwrap();
        assert!(deleted.is_some(), "删除必须是软删");
        create(&conn, &input("p1", "1.0.1", Some("2026-01-02"), None)).unwrap();
    }

    #[test]
    fn version_reads_and_mutations_are_scoped() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let v = create(&conn, &input("p1", "1.0.0", Some("2026-01-01"), None)).unwrap();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        conn.execute("UPDATE projects SET workspace_id = ?1 WHERE id = 'p1'", [&b.id]).unwrap();
        assert_eq!(list(&conn, "p1").unwrap_err().code, "not_found");
        assert_eq!(create(&conn, &input("p1", "1.1.0", Some("2026-02-01"), None)).unwrap_err().code, "not_found");
        assert_eq!(update(&conn, &v.id, &VersionUpdate { version: "1.1.0".to_owned(), released_at: Some("2026-02-01".to_owned()), content: None }).unwrap_err().code, "not_found");
        assert_eq!(delete(&conn, &v.id).unwrap_err().code, "not_found");
        crate::workspace::switch(&conn, &b.id).unwrap();
        assert_eq!(list(&conn, "p1").unwrap().len(), 1, "切到新区后原样可见");
    }

    #[test]
    fn version_wire_format_is_camel_case() {
        let v = Version {
            id: "v1".to_owned(),
            project_id: "p1".to_owned(),
            version: "1.0.0".to_owned(),
            released_at: "2026-01-01".to_owned(),
            content: "".to_owned(),
            created_at: "2026-01-01 00:00:00".to_owned(),
            updated_at: "2026-01-01 00:00:00".to_owned(),
        };
        let obj = serde_json::to_value(&v).unwrap();
        let keys: Vec<&String> = obj.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 7, "Version 键集合变了：{keys:?}");
        for key in ["id", "projectId", "version", "releasedAt", "content", "createdAt", "updatedAt"] {
            assert!(obj.get(key).is_some(), "缺 {key}");
        }
    }
}
```

- [ ] **Step 2: 跑红**

Run: `cd src-tauri && cargo test -j 1 --lib version::tests::create_rejects`
Expected: FAIL（`cannot find function create` / 模块未注册）。

- [ ] **Step 3: 写 `version.rs` 实现并注册模块**

在测试模块之前写：

```rust
//! 项目版本台账（M16-a spec §三/§四）。

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

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub version: String,
    pub released_at: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInput {
    pub project_id: String,
    pub version: String,
    pub released_at: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionUpdate {
    pub version: String,
    pub released_at: Option<String>,
    pub content: Option<String>,
}

const SELECT_COLUMNS: &str =
    "v.id, v.project_id, v.version, v.released_at, v.content, v.created_at, v.updated_at";

fn map_version(row: &rusqlite::Row<'_>) -> rusqlite::Result<Version> {
    Ok(Version {
        id: row.get(0)?,
        project_id: row.get(1)?,
        version: row.get(2)?,
        released_at: row.get(3)?,
        content: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn project_exists(conn: &Connection, project_id: &str) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects
          WHERE id = ?1 AND deleted_at IS NULL AND workspace_id = ?2",
        params![project_id, crate::workspace::active_id(conn)?],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(not_found(project_id));
    }
    Ok(())
}

/// 版本号在同一项目内唯一；软删行不占号；update 排除自身。
fn ensure_unique_version(conn: &Connection, project_id: &str, version: &str, exclude_id: &str) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM project_versions
          WHERE project_id = ?1 AND version = ?2 AND deleted_at IS NULL AND id <> ?3",
        params![project_id, version, exclude_id],
        |r| r.get(0),
    )?;
    if n > 0 {
        return Err(invalid(&format!("同一项目下版本号不能重复：{version}")));
    }
    Ok(())
}

fn validate(version: &str, released_at: Option<String>, content: Option<String>) -> AppResult<(String, String, String)> {
    let version = version.trim().to_owned();
    if version.is_empty() {
        return Err(invalid("版本号不能为空"));
    }
    let released_at = req_date("上线时间", released_at)?;
    Ok((version, released_at, norm(content).unwrap_or_default()))
}

fn get(conn: &Connection, id: &str) -> AppResult<Version> {
    let sql = format!("SELECT {SELECT_COLUMNS} FROM project_versions v WHERE v.id = ?1 AND v.deleted_at IS NULL");
    conn.query_row(&sql, [id], map_version)
        .optional()?
        .ok_or_else(|| not_found(id))
}

pub fn list(conn: &Connection, project_id: &str) -> AppResult<Vec<Version>> {
    project_exists(conn, project_id)?;
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM project_versions v
          WHERE v.project_id = ?1 AND v.deleted_at IS NULL
          ORDER BY v.released_at DESC, v.id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([project_id], map_version)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn create(conn: &Connection, input: &VersionInput) -> AppResult<Version> {
    project_exists(conn, &input.project_id)?;
    let (version, released_at, content) =
        validate(&input.version, input.released_at.clone(), input.content.clone())?;
    ensure_unique_version(conn, &input.project_id, &version, "")?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO project_versions (id, project_id, version, released_at, content)
          VALUES (?1,?2,?3,?4,?5)",
        params![id, input.project_id, version, released_at, content],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, input: &VersionUpdate) -> AppResult<Version> {
    crate::workspace::ensure_active_row(conn, "project_versions", id)?;
    let (version, released_at, content) =
        validate(&input.version, input.released_at.clone(), input.content.clone())?;
    let project_id: String = conn
        .query_row(
            "SELECT project_id FROM project_versions WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |r| r.get(0),
        )
        .map_err(|_| not_found(id))?;
    ensure_unique_version(conn, &project_id, &version, id)?;
    let changed = conn.execute(
        "UPDATE project_versions
            SET version=?2, released_at=?3, content=?4, updated_at=datetime('now')
          WHERE id=?1 AND deleted_at IS NULL",
        params![id, version, released_at, content],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    crate::workspace::ensure_active_row(conn, "project_versions", id)?;
    let changed = conn.execute(
        "UPDATE project_versions SET deleted_at = datetime('now'), updated_at = datetime('now')
          WHERE id = ?1 AND deleted_at IS NULL",
        [id],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}
```

`lib.rs`：`mod version;`；顶部 import 加 `use crate::version::{Version, VersionInput, VersionUpdate};`；四条命令（照 `expiry_*` 形制）：

```rust
#[tauri::command]
fn version_list(state: State<'_, AppState>, project_id: String) -> AppResult<Vec<Version>> {
    let conn = db(&state)?;
    version::list(&conn, &project_id)
}

#[tauri::command]
fn version_create(state: State<'_, AppState>, input: VersionInput) -> AppResult<Version> {
    let conn = db(&state)?;
    version::create(&conn, &input)
}

#[tauri::command]
fn version_update(state: State<'_, AppState>, id: String, input: VersionUpdate) -> AppResult<Version> {
    let conn = db(&state)?;
    version::update(&conn, &id, &input)
}

#[tauri::command]
fn version_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    version::delete(&conn, &id)
}
```

`generate_handler!` 在 `expiry_delete` 之后加四项：`version_list, version_create, version_update, version_delete,`。

`workspace.rs` 的 `ensure_active_row` 注释「九张表都有」改「十张表都有」（零行为）。

Run: `cd src-tauri && cargo test -j 1 --lib version::tests::`
Expected: 7 passed。

- [ ] **Step 4: 全量 + 结构门**

Run: `cd src-tauri && cargo test -j 1 --lib`
Expected: `415 passed; 0 failed`（408 + 7）。
Run: `cd src-tauri && cargo clippy -j 1 --lib -- -D warnings`
Expected: exit 0。
Run: `sed -n '/generate_handler/,/^\s*\])/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"`
Expected: **79**。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/version.rs src-tauri/src/lib.rs src-tauri/src/workspace.rs
git commit -m "feat(m16a): 版本台账四操作与四条同步命令（命令 75→79）"
```

---

## Task 3: 前端版本面板 + 文件选择器

**Files:**
- Create: `src/types/version.ts`、`src/components/version-panel.tsx`
- Modify: `src/lib/api.ts`（四条版本包装 + `pickFile`）
- Modify: `src/pages/project-detail.tsx`（挂 `VersionPanel`、`chooseEntryFile`、「选择文件」按钮）

**Interfaces:**
- Consumes: Task 2 的四条命令与 `Version` camelCase 线格式。
- Produces: 项目详情页可新增/编辑/删除版本；快捷入口可选择文件。

- [ ] **Step 1: 写 `src/types/version.ts`**

```ts
export interface Version {
  id: string;
  projectId: string;
  version: string;
  releasedAt: string;
  content: string;
  createdAt: string;
  updatedAt: string;
}

export interface VersionInput {
  projectId: string;
  version: string;
  releasedAt: string | null;
  content: string | null;
}

export interface VersionUpdate {
  version: string;
  releasedAt: string | null;
  content: string | null;
}
```

- [ ] **Step 2: `api.ts` 四条包装 + `pickFile`**

在 expiry 四条之后加：

```ts
// —— M16-a 项目版本台账：四个封装 ——
export const listVersions = (projectId: string) => invoke<Version[]>("version_list", { projectId });
export const createVersion = (input: VersionInput) => invoke<Version>("version_create", { input });
export const updateVersion = (id: string, input: VersionUpdate) =>
  invoke<Version>("version_update", { id, input });
export const deleteVersion = (id: string) => invoke<void>("version_delete", { id });
```

顶部 type import 加 `import type { Version, VersionInput, VersionUpdate } from "@/types/version";`。
`pickFolder` 之后加：

```ts
// 原生文件选择器。取消返回 null（与 pickFolder 同一取消语义）。
export async function pickFile(): Promise<string | null> {
  const picked = await openDialog({ multiple: false, directory: false });
  return typeof picked === "string" ? picked : null;
}
```

- [ ] **Step 3: `src/components/version-panel.tsx`**

```tsx
import { useCallback, useEffect, useState } from "react";
import { Pencil, Plus, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { createVersion, deleteVersion, listVersions, updateVersion } from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import type { Version } from "@/types/version";

const dateOrNull = (raw: string): string | null => (raw.trim() === "" ? null : raw.trim());

export function VersionPanel({ projectId }: { projectId: string }) {
  const [rows, setRows] = useState<Version[]>([]);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [version, setVersion] = useState("");
  const [releasedAt, setReleasedAt] = useState("");
  const [content, setContent] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setRows(await listVersions(projectId));
      setError(null);
    } catch (e) {
      setError(toAppError(e));
    }
  }, [projectId]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const reset = () => {
    setEditingId(null);
    setVersion("");
    setReleasedAt("");
    setContent("");
  };

  const run = async (fn: () => Promise<unknown>, okText: string): Promise<boolean> => {
    setBusy(true);
    setNotice(null);
    try {
      await fn();
      await reload();
      setNotice(okText);
      return true;
    } catch (e) {
      setError(toAppError(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    const fields = {
      version,
      releasedAt: dateOrNull(releasedAt),
      content: content.trim() === "" ? null : content,
    };
    const ok = editingId
      ? await run(() => updateVersion(editingId, fields), "版本已更新")
      : await run(() => createVersion({ projectId, ...fields }), "版本已新增");
    if (ok) reset();
  };

  const startEdit = (r: Version) => {
    setEditingId(r.id);
    setVersion(r.version);
    setReleasedAt(r.releasedAt);
    setContent(r.content);
    setNotice(null);
  };

  return (
    <div>
      <div className="mb-1 flex items-center justify-between">
        <h2 className="text-sm font-medium">项目版本台账</h2>
        <span className="text-xs text-muted-foreground">版本号与上线时间必填；同项目版本号唯一（删除后可重建）</span>
      </div>

      {error && (
        <div className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}
      {notice && !error && <p className="mb-3 text-xs text-muted-foreground">{notice}</p>}

      <div className="mb-3 grid gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <Input
            value={version}
            onChange={(e) => setVersion(e.target.value)}
            placeholder="版本号，如 1.2.0"
            className="w-40"
          />
          <Input
            type="date"
            value={releasedAt}
            onChange={(e) => setReleasedAt(e.target.value)}
            className="w-44"
            aria-label="上线时间"
          />
          <Button size="sm" disabled={busy || !version.trim() || !releasedAt} onClick={() => void save()}>
            <Plus /> {editingId ? "保存" : "记一笔"}
          </Button>
          {editingId && (
            <Button size="sm" variant="ghost" disabled={busy} onClick={reset}>
              <X /> 取消
            </Button>
          )}
        </div>
        <textarea
          value={content}
          onChange={(e) => setContent(e.target.value)}
          placeholder="本次上线内容（可留空）"
          className="min-h-20 w-full rounded-md border bg-transparent px-3 py-2 text-sm outline-none focus-visible:border-ring"
        />
      </div>

      {rows.length === 0 && <p className="py-4 text-center text-sm text-muted-foreground">还没有版本记录。</p>}

      {rows.map((r) => (
        <div key={r.id} className="flex items-start gap-2 border-b px-1 py-2 last:border-b-0">
          <span className="w-28 shrink-0 font-mono text-xs">{r.version}</span>
          <span className="w-24 shrink-0 font-mono text-xs text-muted-foreground">{r.releasedAt}</span>
          <span className="min-w-0 flex-1 whitespace-pre-wrap text-sm">{r.content || "—"}</span>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => startEdit(r)}>
            <Pencil /> 编辑
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={() => void run(() => deleteVersion(r.id), "版本已删除")}
          >
            <Trash2 />
          </Button>
        </div>
      ))}
    </div>
  );
}
```

- [ ] **Step 4: 项目详情页接线**

`src/pages/project-detail.tsx`：
- lucide import 加 `FilePlus`。
- `@/lib/api` import 加 `pickFile`。
- `@/components/version-panel` import `VersionPanel`。
- 在 `chooseEntry` 后加：

```tsx
  const chooseEntryFile = () =>
    guard(async () => {
      const path = await pickFile();
      if (path) await addEntry(project.id, path);
    });
```

- 「快捷入口」头部的「选择目录」按钮后加：

```tsx
          <Button size="sm" variant="ghost" className="ml-auto" disabled={saving} onClick={chooseEntryFile}>
            <FilePlus /> 选择文件
          </Button>
```

（保留原「选择目录」按钮上的 `ml-auto`，在它后面加「选择文件」；两个按钮一起靠右。）
- `MilestonePanel` 容器之后加：

```tsx
      <div className="mt-4 rounded-xl border bg-background p-4">
        <VersionPanel projectId={project.id} />
      </div>
```

- [ ] **Step 5: 类型门 + node**

Run: `npm run build`
Expected: exit 0；`grep -c "error TS"` = 0。
Run: `node --test "tests/**/*.test.ts"`
Expected: `75/75/0`（本批不加 node 测试）。

- [ ] **Step 6: 提交**

```bash
git add src/types/version.ts src/lib/api.ts src/components/version-panel.tsx src/pages/project-detail.tsx
git commit -m "feat(m16a): 项目版本面板与子文件选择器（node 仍 75）"
```

---

## Task 4: 全量门禁 + 变异取证 + 真机五格（控制方跑）

**Files:**
- Create（git-ignored 证据）：`.superpowers/sdd/2026-10-24-m16a-project-versions/{mutation-evidence.txt,real-machine-readings.md,m16-cdp.mjs,m16-cdp.stdout.txt,realdir-before.txt,realdir-after.txt}`
- Modify: 无生产代码（变异下靶后必须逐条还原并 `git diff --exit-code` 验 0）

- [ ] **Step 1: 整套门禁**

```bash
cd src-tauri && cargo test -j 1 --lib; echo "exit=$?"
cd src-tauri && cargo clippy -j 1 --lib -- -D warnings; echo "exit=$?"
cd src-tauri && cargo clippy -j 1 --lib --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo build -j 1 --lib; echo "exit=$?"
node --test "tests/**/*.test.ts" 2>&1 | tail -8
npm run build 2>&1 | tail -5
grep -c "TARGET_VERSION: i64 = 12" src-tauri/src/db.rs
sed -n '/generate_handler/,/^\s*\])/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"
grep -c "tauri::command(async)" src-tauri/src/lib.rs
grep -rn "allow(dead_code)" src-tauri/src/ | wc -l
grep -c '^\[\[package\]\]' src-tauri/Cargo.lock
grep -c "hits.append(&mut collect(" src-tauri/src/search.rs
git diff --stat master..HEAD -- src-tauri/Cargo.lock src-tauri/Cargo.toml package.json
git rev-list --merges --count --all
```

Expected：cargo **415 passed**、node **75/75/0**、`error TS` 0、命令 **79**、async **7**、TARGET **12**、豁免 **1**、lock **616**、collect **9**、依赖 diff 空、merge 0。

- [ ] **Step 2: 五处变异**

| # | 靶 | 怎么改 | 必红测试 |
|---|---|---|---|
| M① | 版本列表不判区 | `version::project_exists` 的 `AND workspace_id = ?2` 摘掉（实参留着改恒真） | `version::tests::version_reads_and_mutations_are_scoped` |
| M② | create 不查重 | `create` 里 `ensure_unique_version(...)` 那行删掉 | `version::tests::duplicate_version_is_rejected_on_create` |
| M③ | update 不查重 | `update` 里 `ensure_unique_version(...)` 那行删掉 | `version::tests::duplicate_version_is_rejected_on_update` |
| M④ | 删除不是软删 | `delete` 的 UPDATE 改成 `DELETE FROM project_versions WHERE id=?1` | `version::tests::update_and_delete_are_soft_and_allow_recreate` |
| M⑤ | 列表顺序反了 | `ORDER BY v.released_at DESC, v.id` 的 `DESC` 去掉 | `version::tests::create_and_list_versions_in_release_order` |

每处记录：靶、文件:行、原句→改句、`command`、`exit`、失败测试名、`restore_cmp=identical`、`git_diff_after_restore=clean`。

- [ ] **Step 3: 真机五格（临时 identifier + CDP）**

沙盒 `dev.zero.pfm.m16test`（临时编译期 identifier，跑完原字节还原）；CDP 9333；真实数据目录跑前/跑后拍 `size+mtime+sha256`。

| 格 | 动作 | 读数 |
|---|---|---|
| C-1 | 详情页新增版本 1.0.0 / 2026-01-01 / 内容 | 行出现，三字段正确 |
| C-2 | 编辑内容与日期，再删除 | 行更新；删除后消失；库内软删行仍在 |
| C-3 | 同项目再次 1.0.0 | 界面显示「版本号不能重复」；库里不增行 |
| C-4 | 切工作区 | 版本列表只随活动区项目；跨区深链不可见 |
| C-5 | 「选择文件」 | **人真点原生对话框**选一个文件：入口行出现；取消不报错；打开/定位可用 |

C-1…C-4 可由 `m16-cdp.mjs` 驱动（复用 `m15-cdp.mjs` 的 CDP 骨架）；C-5 是原生文件对话框，只能人点（坑 12/47），报告里如实写「人点」。

Expected: `M16A-RESULT: ALL-PASS`，五格逐条读数落 `real-machine-readings.md`。

- [ ] **Step 4: 映射与证据**

写 `spec-plan-mapping.md`：spec §一…§十一 → Task/Step → 测试名 / 真机格 / 变异靶；没有落点的句子写「本轮不做」或「有名缺口」。证据目录 git-ignored，收口前别清。

---

## Task 5: 活文档回写与提交

**Files:**
- Modify: `docs/技术方案.md`（数据模型里既有 `project_versions` 行标 V12/M16-a；里程碑表加 M16-a 行）
- Modify: `docs/开发进度.md`（M16-a 验收证据 + 变更日志）
- Modify: `docs/HANDOFF.md`（快照、§4-A 命令 79 / TARGET 12、下一格 M16-b）
- Modify: `docs/思维导图.md`（版本台账行标 M16-a 已交付；`note_assets` 仍留给 M16-b）

- [ ] **Step 1: 技术方案**

把数据模型里既有的 `project_versions 版本台账...` 行改成：

```text
project_versions    项目版本台账（M16-a 新增，V12）：id / project_id→projects / version / released_at TEXT NOT NULL / content /
                    软删三件套；同项目 (project_id, version) 在未删行内唯一（partial unique index）；零回填。
```

里程碑表加一行 M16-a：版本台账 + 子文件选择器；状态写「代码 + 变异 + 真机全完，待合并」；命令 79、TARGET 12。

- [ ] **Step 2: 开发进度**

加 `## M16-a 验收证据（项目版本台账，<状态>；cargo <实测> / node 75）`：门禁串、5 处变异、真机 5 格、映射路径、有名缺口（C-5 人点、UI 无 jsdom）。变更日志追加一条。

- [ ] **Step 3: HANDOFF**

顶部 M16-a 快照；§4-A 门禁串改 `命令 79 / async 7 / TARGET_VERSION 12 / 豁免 1 / Cargo.lock 616 / collect 9 / cargo 415 / node 75`；下一格写 M16-b（图文笔记/截图 + 附件进备份）。

- [ ] **Step 4: 校验 + 提交 + 推功能分支**

```bash
unix2dos docs/技术方案.md docs/开发进度.md docs/HANDOFF.md docs/思维导图.md
git diff --check
git add docs/技术方案.md docs/开发进度.md docs/HANDOFF.md docs/思维导图.md
git commit -m "docs(m16a): 活文档回写 —— 版本台账 V12、门禁串与下一格 M16-b"
git -c http.proxy= -c https.proxy= push origin feat/m16a-project-versions
```

（本地代理 `127.0.0.1:7897` 的 TLS 握手是坏的，push 必须带 `-c http.proxy= -c https.proxy=`。）

---

## Self-Review

- **Spec coverage**：§三 V12（Task 1）、§四/§五 命令与守卫（Task 2）、§六 备份（Task 1）、§七 前端/文件选择（Task 3）、§八 T-1…T-10 与 C-1…C-5（Task 1/2/3/4）、§九 不做（无任务）、§十 回写（Task 5）。
- **Placeholder scan**：无 TBD/TODO；每个代码步骤都有可粘贴代码。
- **Type consistency**：`Version` 七键、`VersionInput`/`VersionUpdate` 在 Rust/TS/命令/测试中同名；`version_list` 参数 `projectId`；`version_update` 返回整行 `Version`。
- **Known deviations**：`VersionInput.released_at` 用 `Option<String>`（照 `ExpiryInput.expire_date`），不是必填 `String`；缺键/空串由 `req_date` 回 `invalid_input`，与 spec §四一致。
