# 项目模板（M12）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用户可维护「项目模板」，新建项目时选模板一键填充项目基础信息（项目名永远由用户输入）。

**Architecture:** V8 新建 `project_templates` 表（硬删、模板名 UNIQUE、tags 存 JSON），四条同步 IPC 命令完成 CRUD；前端在项目页加模板管理弹窗、在新建项目对话框加「从模板填充」下拉，套用逻辑收敛到一个纯函数 `applyTemplate`（项目名不动、模板空字段不清空）。

**Tech Stack:** Tauri 2.12 / Rust rusqlite 0.40.2 / React 19 + TypeScript / zustand / shadcn Dialog、Select / node:test。

**Spec:** `docs/superpowers/specs/2026-10-22-m12-project-templates-design.md`

## Global Constraints

- 本轮只做项目基础信息模板：不含项目名、台账行、目录映射、待办、里程碑、密文。
- V8 零回填；模板硬删除（无 `deleted_at`）；模板名 UNIQUE；tags 存 JSON 数组。
- 四条同步命令：`template_list` / `template_create` / `template_update` / `template_delete`；`generate_handler!` **65→69**；`tauri::command(async)` 仍 **7**。
- `TARGET_VERSION` **7→8**；`backup::Counts` 仍七张表（模板不进 counts）；`warn_plaintext_columns` 补「项目模板」；`search.rs` 的 collect 仍 **9**。
- 零新依赖：`Cargo.toml` / `Cargo.lock` / `package.json` 对 `master` 的 diff 必须空；`Cargo.lock` `[[package]]` **616**。
- 错误面只用 `invalid_input` / `not_found`；模板重名由应用层判，DB `UNIQUE` 只兜底。
- 前端复用 `ProjectForm`；新增 `nameLabel` / `nameRequiredMessage` 两个可选 prop，现有两处调用不传、行为不变。
- 同一 worktree 只允许一个会提交的实现者；Task 5 与 Task 6 都改 `src/pages/projects.tsx`，必须串行。
- 子代理一律不跑 `tauri dev`；真机格只由控制方用一次性 identifier 沙盒跑，真实数据目录三文件前后逐字一致。
- 退出码真取：`cmd > log 2>&1; echo "exit=$?"`，绝不 `cmd | tail; echo $?`。

## 任务总览与数链

| Task | 交付 | 新增 cargo 测试 | cargo 累计 | 新增 node 测试 | node 累计 | 结构门 |
|---|---|---|---|---|---|---|
| 基线 | M11-b 已合 master | — | **337** | — | **52** | 65 / 7 / 7 / 1 / 616 / 9 |
| 1 | V8 + 备份/DB 测试 | +2 | **339** | 0 | 52 | TARGET 变 **8** |
| 2 | `project_template.rs` + 4 条 IPC | +7 | **346** | 0 | 52 | 命令变 **69**；async 7 |
| 3 | 类型 / API / `applyTemplate` | 0 | 346 | +5 | **57** | 不变 |
| 4 | templates store + ProjectForm props | 0 | 346 | 0 | 57 | 不变 |
| 5 | 模板管理弹窗 + 项目页按钮 | 0 | 346 | 0 | 57 | 不变 |
| 6 | 新建项目模板下拉 + 套用 | 0 | 346 | 0 | 57 | 不变 |
| 7 | 门禁 + 变异 + 真机 + 映射 | 0 | 346 | 0 | 57 | 终态 69 / 7 / 8 / 1 / 616 / 9 |
| 8 | 活文档回写 | 0 | 346 | 0 | 57 | 不变 |

新增 cargo 9 支（Task 1 的 2 支 + Task 2 的 7 支）；新增 node 5 支。**具体数在 Task 1/2/3 的实现者报告里按 `test result` 与 `ℹ tests` 复核，任一格对不上先查测试函数个数，不要硬凑。**

## 派发前预检（控制方填）

- [x] SQL DDL / UNIQUE / tags JSON：Python sqlite3 在临时库实跑 —— `PRAGMA index_list` **2 条唯一索引**（`id` 的 `pk` 索引 + `name` 的 `u` 索引），所以测试改为按 `origin='u'` 找约束、`PRAGMA index_info` 断列名 `name`；重复名与非法 status 都抛 `IntegrityError`；tags `["运维","政务"]` JSON 往返成功（终端显示层把中文印成乱码，不是数据问题）。
- [x] `applyTemplate` 5 条断言在临时 `.mjs` 实跑：**5/5 PASS**（名字保留、非空覆盖、空/null 不清空、status/tags、入参不变）。
- [x] 测试函数计数：Task 1 新增 2 支（`v8_project_templates_table_and_unique_name_exist`、`backup_package_carries_project_templates_without_counting_them`；`v8_schema_meta...` 是既有 v7 测试改名，不增数）、Task 2 新增 7 支、`tests/apply-template.test.ts` 5 条 `test(...)`。
- [x] 命令数：基线 65 + 4 = **69**；实现后按 `sed` 门实跑复核。
- [x] 本机 CRLF：新建/修改文件用 CRLF（`git ls-files --eol` 应为 `i/lf w/crlf`），提交前 `git diff --check`。

---

### Task 1: V8 迁移 + 备份/DB 测试

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/backup.rs`
- Test: 两个文件的 `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `project_templates` 表；`TARGET_VERSION: i64 = 8`；`warn_plaintext_columns()` 新文本。
- Consumes: 无（模板业务模块在 Task 2 才出现；本任务的备份测试用 SQL 直插一行，不依赖 Task 2）。

- [ ] **Step 1: 写失败测试（db.rs）**

在 `src-tauri/src/db.rs` 的迁移测试区把现有 `v7_schema_meta_reaches_target_version_seven` **改名并改期望**为：

```rust
    #[test]
    fn v8_schema_meta_reaches_target_version_eight() {
        let conn = open_in_memory().unwrap();
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 8, "迁移必须跑到 8");
        assert_eq!(TARGET_VERSION, 8, "= 上一条，两处别各写各的");
    }

    /// V8 必须把模板表与 name 唯一索引建出来，且迁移不带任何模板行（零回填）。
    #[test]
    fn v8_project_templates_table_and_unique_name_exist() {
        let conn = open_in_memory().unwrap();
        let table: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'project_templates'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(table, 1, "V8 必须建 project_templates");
        let mut stmt = conn.prepare("PRAGMA index_list('project_templates')").unwrap();
        let indexes: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(1)?, r.get(3)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        // `id TEXT PRIMARY KEY` 也会生成一条 origin='pk' 的唯一索引；只认 origin='u' 的约束，
        // 再用 index_info 精确断它落在 name 上 —— 直接数「唯一索引恰好 1 条」会恒红（实测 2 条）。
        let unique = indexes
            .iter()
            .find(|(_, origin)| origin == "u")
            .expect("name 必须有 UNIQUE 约束");
        let mut cols = conn
            .prepare(&format!("PRAGMA index_info('{}')", unique.0))
            .unwrap();
        let names: Vec<String> = cols
            .query_map([], |r| r.get(2))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(names, vec!["name"], "UNIQUE 约束必须落在 name 上");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM project_templates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "V8 零回填：老库迁移后模板表必须为空");
    }
```

- [ ] **Step 2: 写失败测试（backup.rs）**

在 `src-tauri/src/backup.rs` 的测试模块（有 `unpack_db` 辅助件的那一块）加：

```rust
    /// M12 格 11 的 cargo 半边：模板随整库快照进包，但 counts 仍是七张表。
    #[test]
    fn backup_package_carries_project_templates_without_counting_them() {
        let live = crate::db::open_in_memory().unwrap();
        live.execute(
            "INSERT INTO project_templates (id, name, status) VALUES ('tpl-1', '备份模板', '立项')",
            [],
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("templates.zip");
        let sum = export(&live, &dst).unwrap();
        assert_eq!(sum.schema_version, crate::db::TARGET_VERSION);
        assert_eq!(sum.schema_version, 8, "M12 起包内 schema 必须是 8");
        assert_eq!(sum.counts.projects, 0);
        let (_unpacked, got) = unpack_db(&dst);
        let n: i64 = got
            .query_row("SELECT COUNT(*) FROM project_templates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "模板行必须随整库快照进包");
        let name: String = got
            .query_row("SELECT name FROM project_templates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "备份模板");
    }
```

并把既有的 `plaintext_warning_names_every_plain_text_table_since_m9` 补一句：

```rust
        assert!(warn.contains("项目模板"), "漏了项目模板的明文列：{warn}");
```

- [ ] **Step 3: 跑它确认红**

```bash
cd src-tauri && cargo test -j 1 --lib > /tmp/m12-t1-red.log 2>&1; echo "exit=$?"
grep -E "test result|FAILED|error\[" /tmp/m12-t1-red.log | head -12
```

Expected：exit=101；新加的两支测试红（v8 版本断言还是 7；备份测试在 `INSERT INTO project_templates` 处表不存在）。

- [ ] **Step 4: 写最小实现**

`db.rs`：`TARGET_VERSION` 改 **8**；`migrate` 的 `match` 加 `8 => V8`；在 `V7` 后加：

```rust
/// v8：项目模板（M12 spec §三）。只存项目基础信息；模板名 UNIQUE；tags 是 JSON 数组；
/// **硬删除，没有 deleted_at**（配置型快捷方式，不是业务记录）。零回填。
const V8: &str = "
CREATE TABLE project_templates (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL UNIQUE,
    code              TEXT,
    manager           TEXT,
    customer          TEXT,
    contact_name      TEXT,
    contact_phone     TEXT,
    contract_no       TEXT,
    contract_period   TEXT,
    delivery_deadline TEXT,
    status            TEXT NOT NULL DEFAULT '立项'
                      CHECK (status IN ('立项','需求','开发','测试','预发','上线','运维','归档')),
    tags              TEXT NOT NULL DEFAULT '[]',
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now'))
);
";
```

`backup.rs` 的 `warn_plaintext_columns` 改成：

```rust
pub fn warn_plaintext_columns() -> &'static str {
    "包内非 *_cipher 列为明文：项目名/编号/负责人/客户/联系人/合同号（含项目模板的同类字段）、服务器 ip 与堡垒机、各类 url、各表 note、待办内容、里程碑备注、日报内容、到期事项名称与备注。"
}
```

- [ ] **Step 5: 跑绿并确认数链**

```bash
cd src-tauri && cargo test -j 1 --lib > /tmp/m12-t1-green.log 2>&1; echo "exit=$?"
grep "test result" /tmp/m12-t1-green.log
```

Expected：**339 passed / 0 failed / exit=0**（337 + 2）；`TARGET_VERSION` 现取为 8。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/db.rs src-tauri/src/backup.rs
git commit -m "feat(m12): V8 建 project_templates（硬删/name UNIQUE/tags JSON）+ 备份包携带模板与明文告警 —— 339 passed"
```

---

### Task 2: `project_template.rs` + 四条 IPC

**Files:**
- Create: `src-tauri/src/project_template.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces（Task 3 的 API/命令对齐）：
  - `project_template::ProjectTemplate { id, name, code, manager, customer, contact_name, contact_phone, contract_no, contract_period, delivery_deadline, status, tags }`（camelCase 线格式）
  - `project_template::TemplateInput`（同形，无 id）
  - `pub fn list(conn) -> AppResult<Vec<ProjectTemplate>>`
  - `pub fn create(conn, &TemplateInput) -> AppResult<ProjectTemplate>`
  - `pub fn update(conn, id, &TemplateInput) -> AppResult<ProjectTemplate>`
  - `pub fn delete(conn, id) -> AppResult<()>`
  - IPC：`template_list` / `template_create` / `template_update` / `template_delete`
- Consumes: `crate::project::STATUSES`、`crate::error::{AppError, AppResult}`、`uuid`、`serde_json`。

- [ ] **Step 1: 写测试（新模块，先让函数体 `unimplemented!()`）**

新建 `src-tauri/src/project_template.rs`，放类型、函数签名与下面的测试；先把 `list/create/update/delete` 的函数体写成 `unimplemented!()`。**同时在 `src-tauri/src/lib.rs` 模块表加 `mod project_template;`**（先不加命令，否则测试不会编译进去），跑红：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn input(name: &str) -> TemplateInput {
        TemplateInput {
            name: name.to_owned(),
            code: None,
            manager: None,
            customer: None,
            contact_name: None,
            contact_phone: None,
            contract_no: None,
            contract_period: None,
            delivery_deadline: None,
            status: "立项".to_owned(),
            tags: vec![],
        }
    }

    #[test]
    fn create_update_list_round_trip_all_fields() {
        let conn = db::open_in_memory().unwrap();
        let mut i = input("模板A");
        i.code = Some("TPL-1".into());
        i.manager = Some("王经理".into());
        i.customer = Some("客户甲".into());
        i.contact_name = Some("联系人".into());
        i.contact_phone = Some("13800000000".into());
        i.contract_no = Some("HT-1".into());
        i.contract_period = Some("2026-01 ~ 2026-12".into());
        i.delivery_deadline = Some("2026-12-31".into());
        i.tags = vec!["运维".into(), "政务".into()];
        let created = create(&conn, &i).unwrap();
        assert_eq!(created.name, "模板A");
        assert_eq!(created.code.as_deref(), Some("TPL-1"));
        assert_eq!(created.tags, vec!["运维", "政务"]);

        let mut u = input("模板B");
        u.manager = Some("新经理".into());
        u.status = "开发".into();
        u.tags = vec!["新标签".into()];
        let updated = update(&conn, &created.id, &u).unwrap();
        assert_eq!(updated.name, "模板B");
        assert_eq!(updated.manager.as_deref(), Some("新经理"));
        assert_eq!(updated.status, "开发");
        assert_eq!(updated.tags, vec!["新标签"]);

        let rows = list(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], updated);
    }

    #[test]
    fn duplicate_name_is_rejected_naming_the_template() {
        let conn = db::open_in_memory().unwrap();
        create(&conn, &input("模板A")).unwrap();
        let err = create(&conn, &input("模板A")).unwrap_err();
        assert_eq!(err.code, "invalid_input", "命令路径要的是应用层错误，不是 db_failed");
        assert!(err.message.contains("模板A"), "要点名重名的模板：{}", err.message);

        let b = create(&conn, &input("模板B")).unwrap();
        let err = update(&conn, &b.id, &input("模板A")).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("模板A"));
        // 排除自己：把 B 改成 B 必须通过。
        assert_eq!(update(&conn, &b.id, &input("模板B")).unwrap().name, "模板B");
    }

    #[test]
    fn blank_name_and_bad_status_are_rejected() {
        let conn = db::open_in_memory().unwrap();
        let err = create(&conn, &input("   ")).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("模板名称"), "要点名模板名称：{}", err.message);

        let mut bad = input("模板C");
        bad.status = "已上线".into();
        let err = create(&conn, &bad).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("状态"), "要点名状态：{}", err.message);
        assert!(err.message.contains("已上线"), "要念出坏值：{}", err.message);
    }

    #[test]
    fn update_and_delete_unknown_id_are_not_found() {
        let conn = db::open_in_memory().unwrap();
        let err = update(&conn, "missing", &input("模板")).unwrap_err();
        assert_eq!(err.code, "not_found");
        let err = delete(&conn, "missing").unwrap_err();
        assert_eq!(err.code, "not_found");
    }

    #[test]
    fn tags_are_trimmed_deduped_and_round_trip() {
        let conn = db::open_in_memory().unwrap();
        let mut i = input("标签模板");
        i.tags = vec!["  运维 ".into(), "运维".into(), "".into(), "政务".into()];
        let created = create(&conn, &i).unwrap();
        assert_eq!(created.tags, vec!["运维", "政务"]);
        assert_eq!(list(&conn).unwrap()[0].tags, vec!["运维", "政务"]);
    }

    #[test]
    fn list_orders_by_created_at_then_id() {
        let conn = db::open_in_memory().unwrap();
        let a = create(&conn, &input("A")).unwrap();
        let b = create(&conn, &input("B")).unwrap();
        let c = create(&conn, &input("C")).unwrap();
        conn.execute("UPDATE project_templates SET created_at = '2026-01-01 00:00:03' WHERE id = ?1", [&a.id]).unwrap();
        conn.execute("UPDATE project_templates SET created_at = '2026-01-01 00:00:01' WHERE id = ?1", [&b.id]).unwrap();
        conn.execute("UPDATE project_templates SET created_at = '2026-01-01 00:00:02' WHERE id = ?1", [&c.id]).unwrap();
        let names: Vec<String> = list(&conn).unwrap().into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["B", "C", "A"], "按 created_at, id 稳定排序");
    }

    #[test]
    fn delete_is_hard_and_name_can_be_reused() {
        let conn = db::open_in_memory().unwrap();
        let t = create(&conn, &input("可重用")).unwrap();
        delete(&conn, &t.id).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM project_templates", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "硬删除后行必须消失");
        let again = create(&conn, &input("可重用")).unwrap();
        assert_eq!(again.name, "可重用");
        assert_eq!(list(&conn).unwrap().len(), 1);
    }
}
```

- [ ] **Step 2: 跑它确认红**

```bash
cd src-tauri && cargo test -j 1 --lib project_template > /tmp/m12-t2-red.log 2>&1; echo "exit=$?"
grep -E "not implemented|test result|error\[" /tmp/m12-t2-red.log | head -10
```

Expected：`unimplemented!()` panic / 编译红；测试函数共 7 支。

- [ ] **Step 3: 写实现**

`src-tauri/src/project_template.rs` 顶部到函数体（替换掉 `unimplemented!()`）：

```rust
//! 项目模板（M12 spec §三/§四）：只存项目基础信息，供新建项目一键填充。
//! 与 `project.rs` 的区别：模板没有项目名/目录/标签子表；tags 存 JSON；**硬删除**。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::error::{AppError, AppResult};
use crate::project::STATUSES;

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查表单必填项与取值范围"))
}

fn not_found(id: &str) -> AppError {
    AppError::new("not_found", &format!("模板不存在：{id}"), Some("刷新后重试"))
}

fn norm(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn norm_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.into_iter()
        .filter_map(|tag| {
            let tag = tag.trim().to_owned();
            if tag.is_empty() || !seen.insert(tag.clone()) {
                None
            } else {
                Some(tag)
            }
        })
        .collect()
}

fn tags_to_json(tags: &[String]) -> String {
    serde_json::to_string(tags).expect("Vec<String> 一定能序列化成 JSON")
}

fn tags_from_json(raw: &str) -> AppResult<Vec<String>> {
    serde_json::from_str::<Vec<String>>(raw).map_err(|_| {
        AppError::new(
            "db_failed",
            "模板标签不是合法 JSON",
            Some("检查是否手改过库；正常写入不会产生这个形状"),
        )
    })
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTemplate {
    pub id: String,
    pub name: String,
    pub code: Option<String>,
    pub manager: Option<String>,
    pub customer: Option<String>,
    pub contact_name: Option<String>,
    pub contact_phone: Option<String>,
    pub contract_no: Option<String>,
    pub contract_period: Option<String>,
    pub delivery_deadline: Option<String>,
    pub status: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInput {
    pub name: String,
    pub code: Option<String>,
    pub manager: Option<String>,
    pub customer: Option<String>,
    pub contact_name: Option<String>,
    pub contact_phone: Option<String>,
    pub contract_no: Option<String>,
    pub contract_period: Option<String>,
    pub delivery_deadline: Option<String>,
    pub status: String,
    pub tags: Vec<String>,
}

struct Normalized {
    name: String,
    code: Option<String>,
    manager: Option<String>,
    customer: Option<String>,
    contact_name: Option<String>,
    contact_phone: Option<String>,
    contract_no: Option<String>,
    contract_period: Option<String>,
    delivery_deadline: Option<String>,
    status: String,
    tags: Vec<String>,
}

fn normalize(input: &TemplateInput) -> AppResult<Normalized> {
    let name = norm(Some(input.name.clone())).ok_or_else(|| invalid("模板名称不能为空"))?;
    if !STATUSES.contains(&input.status.as_str()) {
        return Err(invalid(&format!("状态「{}」不在枚举内", input.status)));
    }
    Ok(Normalized {
        name,
        code: norm(input.code.clone()),
        manager: norm(input.manager.clone()),
        customer: norm(input.customer.clone()),
        contact_name: norm(input.contact_name.clone()),
        contact_phone: norm(input.contact_phone.clone()),
        contract_no: norm(input.contract_no.clone()),
        contract_period: norm(input.contract_period.clone()),
        delivery_deadline: norm(input.delivery_deadline.clone()),
        status: input.status.clone(),
        tags: norm_tags(input.tags.clone()),
    })
}

fn check_duplicate(conn: &Connection, name: &str, exclude_id: Option<&str>) -> AppResult<()> {
    let count: i64 = match exclude_id {
        Some(id) => conn.query_row(
            "SELECT COUNT(*) FROM project_templates WHERE name = ?1 AND id <> ?2",
            params![name, id],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*) FROM project_templates WHERE name = ?1",
            [name],
            |r| r.get(0),
        )?,
    };
    if count > 0 {
        return Err(invalid(&format!("模板名称「{name}」已存在")));
    }
    Ok(())
}

const SELECT_COLUMNS: &str = "id, name, code, manager, customer, contact_name, contact_phone,
     contract_no, contract_period, delivery_deadline, status, tags";

struct RowValues {
    id: String,
    name: String,
    code: Option<String>,
    manager: Option<String>,
    customer: Option<String>,
    contact_name: Option<String>,
    contact_phone: Option<String>,
    contract_no: Option<String>,
    contract_period: Option<String>,
    delivery_deadline: Option<String>,
    status: String,
    tags: String,
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RowValues> {
    Ok(RowValues {
        id: row.get(0)?,
        name: row.get(1)?,
        code: row.get(2)?,
        manager: row.get(3)?,
        customer: row.get(4)?,
        contact_name: row.get(5)?,
        contact_phone: row.get(6)?,
        contract_no: row.get(7)?,
        contract_period: row.get(8)?,
        delivery_deadline: row.get(9)?,
        status: row.get(10)?,
        tags: row.get(11)?,
    })
}

fn into_template(v: RowValues) -> AppResult<ProjectTemplate> {
    Ok(ProjectTemplate {
        id: v.id,
        name: v.name,
        code: v.code,
        manager: v.manager,
        customer: v.customer,
        contact_name: v.contact_name,
        contact_phone: v.contact_phone,
        contract_no: v.contract_no,
        contract_period: v.contract_period,
        delivery_deadline: v.delivery_deadline,
        status: v.status,
        tags: tags_from_json(&v.tags)?,
    })
}

fn read(conn: &Connection, id: &str) -> AppResult<ProjectTemplate> {
    let raw = conn
        .query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM project_templates WHERE id = ?1"),
            [id],
            read_row,
        )
        .optional()?;
    match raw {
        Some(v) => into_template(v),
        None => Err(not_found(id)),
    }
}

pub fn list(conn: &Connection) -> AppResult<Vec<ProjectTemplate>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} FROM project_templates ORDER BY created_at, id"
    ))?;
    let rows = stmt.query_map([], read_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(into_template(row?)?);
    }
    Ok(out)
}

pub fn create(conn: &Connection, input: &TemplateInput) -> AppResult<ProjectTemplate> {
    let n = normalize(input)?;
    check_duplicate(conn, &n.name, None)?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO project_templates
           (id, name, code, manager, customer, contact_name, contact_phone,
            contract_no, contract_period, delivery_deadline, status, tags)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            id,
            n.name,
            n.code,
            n.manager,
            n.customer,
            n.contact_name,
            n.contact_phone,
            n.contract_no,
            n.contract_period,
            n.delivery_deadline,
            n.status,
            tags_to_json(&n.tags)
        ],
    )?;
    read(conn, &id)
}

pub fn update(conn: &Connection, id: &str, input: &TemplateInput) -> AppResult<ProjectTemplate> {
    let n = normalize(input)?;
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM project_templates WHERE id = ?1)",
        [id],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(not_found(id));
    }
    check_duplicate(conn, &n.name, Some(id))?;
    conn.execute(
        "UPDATE project_templates SET name=?2, code=?3, manager=?4, customer=?5, contact_name=?6,
                contact_phone=?7, contract_no=?8, contract_period=?9, delivery_deadline=?10,
                status=?11, tags=?12, updated_at=datetime('now')
          WHERE id=?1",
        params![
            id,
            n.name,
            n.code,
            n.manager,
            n.customer,
            n.contact_name,
            n.contact_phone,
            n.contract_no,
            n.contract_period,
            n.delivery_deadline,
            n.status,
            tags_to_json(&n.tags)
        ],
    )?;
    read(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute("DELETE FROM project_templates WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}
```

- [ ] **Step 4: 跑模块测试确认绿**

```bash
cd src-tauri && cargo test -j 1 --lib project_template > /tmp/m12-t2-module.log 2>&1; echo "exit=$?"
grep "test result" /tmp/m12-t2-module.log
```

Expected：7 passed / 0 failed / exit=0（模块内测试）。

- [ ] **Step 5: 接 IPC 并注册**

`lib.rs` 模块表**确认已有** `mod project_template;`（Task 2 Step 1 已加）。在命令区加：

```rust
#[tauri::command]
fn template_list(state: State<'_, AppState>) -> AppResult<Vec<project_template::ProjectTemplate>> {
    let conn = db(&state)?;
    project_template::list(&conn)
}

#[tauri::command]
fn template_create(
    state: State<'_, AppState>,
    input: project_template::TemplateInput,
) -> AppResult<project_template::ProjectTemplate> {
    let conn = db(&state)?;
    project_template::create(&conn, &input)
}

#[tauri::command]
fn template_update(
    state: State<'_, AppState>,
    id: String,
    input: project_template::TemplateInput,
) -> AppResult<project_template::ProjectTemplate> {
    let conn = db(&state)?;
    project_template::update(&conn, &id, &input)
}

#[tauri::command]
fn template_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    project_template::delete(&conn, &id)
}
```

注册表：`ledger_export` 现在是最后一条、无尾逗号；改成带逗号，并在其后追加：

```rust
            ledger_export,
            template_list,
            template_create,
            template_update,
            template_delete
```

把 `bootstrap_data_opens_a_wal_db_at_the_given_dir` 那句陈旧注释「链头已随 M9 V5 走到 5」改成「链头已随 M12 V8 走到 8」。

- [ ] **Step 6: 跑全量 + 结构门**

```bash
cd src-tauri && cargo test -j 1 --lib > /tmp/m12-t2-green.log 2>&1; echo "cargo=$?"
grep "test result" /tmp/m12-t2-green.log
cargo clippy -j 1 --lib -- -D warnings > /tmp/m12-t2-cl1.log 2>&1; echo "clippy-lib=$?"
cargo clippy -j 1 --all-targets --all-features -- -D warnings > /tmp/m12-t2-cl2.log 2>&1; echo "clippy-all=$?"
cd ..
echo "cmd=$(sed -n '/generate_handler/,/^\s*\])/p' src-tauri/src/lib.rs | grep -cE '^ +[a-z_]+,?$') async=$(grep -c 'tauri::command(async)' src-tauri/src/lib.rs) dead=$(grep -rn 'allow(dead_code)' src-tauri/src/ | wc -l)"
```

Expected：cargo **346 passed / exit=0**；两条 clippy exit=0；`cmd=69 async=7 dead=1`。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/project_template.rs src-tauri/src/lib.rs
git commit -m "feat(m12): 项目模板 CRUD（硬删/JSON tags/应用层查重）+ 四条同步 IPC —— 命令数 65→69，346 passed"
```

---

### Task 3: 前端类型 / API / `applyTemplate`

**Files:**
- Create: `src/types/project-template.ts`
- Create: `src/lib/apply-template.ts`
- Create: `tests/apply-template.test.ts`
- Modify: `src/lib/api.ts`

**Interfaces:**
- Produces：`ProjectTemplate` / `TemplateInput` / `EMPTY_TEMPLATE_INPUT`；`listTemplates/createTemplate/updateTemplate/deleteTemplate`；`applyTemplate(input, template) -> ProjectInput`。
- Consumes：Task 2 的线格式与命令名。

- [ ] **Step 1: 写失败测试**

`tests/apply-template.test.ts`：

```ts
import test from "node:test";
import assert from "node:assert/strict";
import { applyTemplate } from "../src/lib/apply-template.ts";
import type { ProjectInput } from "../src/types/project.ts";
import type { ProjectTemplate } from "../src/types/project-template.ts";

const base: ProjectInput = {
  name: "手工项目名",
  code: "P-1",
  manager: "原经理",
  customer: "原客户",
  contactName: "原联系人",
  contactPhone: "13800000000",
  contractNo: "HT-1",
  contractPeriod: "2026-01 ~ 2026-12",
  deliveryDeadline: "2026-12-31",
  status: "立项",
  tags: ["原标签"],
};

const tpl = (patch: Partial<ProjectTemplate> = {}): ProjectTemplate => ({
  id: "t1",
  name: "模板名",
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
  ...patch,
});

test("项目名永远保留，模板名不会变成项目名", () => {
  const got = applyTemplate(base, tpl({ name: "模板名" }));
  assert.equal(got.name, "手工项目名");
});

test("模板非空字段覆盖当前值", () => {
  const got = applyTemplate(base, tpl({ manager: "模板经理", customer: "模板客户", code: "TPL-1" }));
  assert.equal(got.manager, "模板经理");
  assert.equal(got.customer, "模板客户");
  assert.equal(got.code, "TPL-1");
});

test("模板 null 或空白字段不清空当前值", () => {
  const got = applyTemplate(base, tpl({ code: "", customer: null, manager: "   " }));
  assert.equal(got.code, "P-1");
  assert.equal(got.customer, "原客户");
  assert.equal(got.manager, "原经理");
});

test("status 总覆盖；tags 非空才覆盖，空 tags 保留原值", () => {
  const withTags = applyTemplate(base, tpl({ tags: ["模板标签"] }));
  assert.equal(withTags.status, "开发");
  assert.deepEqual(withTags.tags, ["模板标签"]);
  const noTags = applyTemplate(base, tpl({ tags: [] }));
  assert.deepEqual(noTags.tags, ["原标签"]);
});

test("返回新对象，不修改入参", () => {
  const before = JSON.stringify(base);
  const got = applyTemplate(base, tpl({ manager: "模板经理", tags: ["模板标签"] }));
  assert.notEqual(got, base);
  assert.equal(JSON.stringify(base), before);
  assert.notEqual(got.tags, base.tags);
});
```

- [ ] **Step 2: 跑它确认红**

```bash
node --test "tests/apply-template.test.ts" > /tmp/m12-t3-red.log 2>&1; echo "exit=$?"
grep -E "Cannot find module|tests|pass|fail" /tmp/m12-t3-red.log | head -6
```

Expected：exit=1，找不到 `apply-template.ts` 模块。

- [ ] **Step 3: 写类型与纯函数**

`src/types/project-template.ts`：

```ts
// 与 Rust 的 #[serde(rename_all = "camelCase")] 一一对应；name 是模板名，不是项目名。
export interface ProjectTemplate {
  id: string;
  name: string;
  code: string | null;
  manager: string | null;
  customer: string | null;
  contactName: string | null;
  contactPhone: string | null;
  contractNo: string | null;
  contractPeriod: string | null;
  deliveryDeadline: string | null;
  status: string;
  tags: string[];
}

export interface TemplateInput {
  name: string;
  code: string | null;
  manager: string | null;
  customer: string | null;
  contactName: string | null;
  contactPhone: string | null;
  contractNo: string | null;
  contractPeriod: string | null;
  deliveryDeadline: string | null;
  status: string;
  tags: string[];
}

export const EMPTY_TEMPLATE_INPUT: TemplateInput = {
  name: "",
  code: "",
  manager: "",
  customer: "",
  contactName: "",
  contactPhone: "",
  contractNo: "",
  contractPeriod: "",
  deliveryDeadline: "",
  status: "立项",
  tags: [],
};
```

`src/lib/apply-template.ts`：

```ts
// 套用语义（M12 spec D3）：项目名永不动；模板有值覆盖；模板空/null 不清空；
// status 总覆盖；tags 非空才覆盖。只 import type ⇒ node --test 可直跑。
import type { ProjectInput } from "@/types/project";
import type { ProjectTemplate } from "@/types/project-template";

const filled = (v: string | null): v is string => v !== null && v.trim() !== "";
// ProjectInput 的可空字段是 string | null（types/project.ts），所以 current 也要接 null；
// 模板空/null 时原样保留 null，不退化成 ""。
const pick = (current: string | null, next: string | null): string | null =>
  filled(next) ? next : current;

export function applyTemplate(input: ProjectInput, template: ProjectTemplate): ProjectInput {
  return {
    name: input.name,
    code: pick(input.code, template.code),
    manager: pick(input.manager, template.manager),
    customer: pick(input.customer, template.customer),
    contactName: pick(input.contactName, template.contactName),
    contactPhone: pick(input.contactPhone, template.contactPhone),
    contractNo: pick(input.contractNo, template.contractNo),
    contractPeriod: pick(input.contractPeriod, template.contractPeriod),
    deliveryDeadline: pick(input.deliveryDeadline, template.deliveryDeadline),
    status: template.status,
    tags: template.tags.length > 0 ? [...template.tags] : [...input.tags],
  };
}
```

`src/lib/api.ts` 顶部加类型 import：

```ts
import type { ProjectTemplate, TemplateInput } from "@/types/project-template";
```

在项目相关 API 之后加：

```ts
// —— M12 项目模板 ——
export const listTemplates = () => invoke<ProjectTemplate[]>("template_list");
export const createTemplate = (input: TemplateInput) =>
  invoke<ProjectTemplate>("template_create", { input });
export const updateTemplate = (id: string, input: TemplateInput) =>
  invoke<ProjectTemplate>("template_update", { id, input });
export const deleteTemplate = (id: string) => invoke<void>("template_delete", { id });
```

- [ ] **Step 4: 跑 node 与 build**

```bash
node --test "tests/**/*.test.ts" > /tmp/m12-t3-node.log 2>&1; echo "node=$?"
grep -E "^ℹ (tests|pass|fail)" /tmp/m12-t3-node.log
npm run build > /tmp/m12-t3-web.log 2>&1; echo "web=$?"
grep -c "error TS" /tmp/m12-t3-web.log
```

Expected：node **57/57/0**（52+5）；web exit=0、`error TS` 0。

- [ ] **Step 5: 提交**

```bash
git add src/types/project-template.ts src/lib/apply-template.ts tests/apply-template.test.ts src/lib/api.ts
git commit -m "feat(m12): 模板类型/API + applyTemplate 纯函数（项目名保留、空字段不清空）—— node 52→57"
```

---

### Task 4: templates store + `ProjectForm` 两个 prop

**Files:**
- Create: `src/stores/templates.ts`
- Modify: `src/components/project-form.tsx`

**Interfaces:**
- Produces：`useTemplatesStore`（`items/loading/saving/error/refresh/save/remove`）；`ProjectForm` 的可选 `nameLabel` / `nameRequiredMessage`。
- Consumes：Task 3 的 API 与类型。

- [ ] **Step 1: 写 store**

`src/stores/templates.ts`：

```ts
import { create } from "zustand";
import * as api from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import type { ProjectTemplate, TemplateInput } from "@/types/project-template";

function upsert(items: ProjectTemplate[], t: ProjectTemplate): ProjectTemplate[] {
  const i = items.findIndex((x) => x.id === t.id);
  if (i === -1) return [t, ...items];
  const next = items.slice();
  next[i] = t;
  return next;
}

interface TemplatesState {
  items: ProjectTemplate[];
  loading: boolean;
  saving: boolean;
  error: AppErrorShape | null;
  refresh: () => Promise<void>;
  save: (id: string | null, input: TemplateInput) => Promise<boolean>;
  remove: (id: string) => Promise<boolean>;
  clearError: () => void;
}

export const useTemplatesStore = create<TemplatesState>((set, get) => {
  const run = async (fn: () => Promise<ProjectTemplate | null | void>): Promise<boolean> => {
    set({ saving: true, error: null });
    try {
      const t = await fn();
      if (t) set({ items: upsert(get().items, t) });
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
    loading: true,
    saving: false,
    error: null,

    refresh: async () => {
      set({ loading: true, error: null });
      try {
        set({ items: await api.listTemplates() });
      } catch (e) {
        set({ error: toAppError(e) });
      } finally {
        set({ loading: false });
      }
    },

    save: (id, input) => run(() => (id ? api.updateTemplate(id, input) : api.createTemplate(input))),

    remove: async (id) => {
      set({ saving: true, error: null });
      try {
        await api.deleteTemplate(id);
        set({ items: get().items.filter((t) => t.id !== id) });
        return true;
      } catch (e) {
        set({ error: toAppError(e) });
        return false;
      } finally {
        set({ saving: false });
      }
    },

    clearError: () => set({ error: null }),
  };
});
```

- [ ] **Step 2: 改 `ProjectForm`（两个文案 prop + `onChange` 回传）**

在 `src/components/project-form.tsx` 的 props 类型与解构里加（都有默认值；两处既有调用不传）：

```tsx
export function ProjectForm({
  initial,
  statuses,
  saving,
  onSubmit,
  onCancel,
  nameLabel = "项目名称",
  nameRequiredMessage = "项目名称不能为空",
  onChange,
}: {
  initial?: ProjectInput;
  statuses: string[];
  saving: boolean;
  onSubmit: (input: ProjectInput) => Promise<boolean>;
  onCancel: () => void;
  nameLabel?: string;
  nameRequiredMessage?: string;
  onChange?: (input: ProjectInput) => void;
}) {
```

并在组件内把 `patch` 与 tags 输入改成「更新本地 state + 回传 live 值」：

```tsx
  const emit = (next: ProjectInput, tags: string) =>
    onChange?.({ ...next, tags: parseTags(tags) });
  const patch = (key: keyof ProjectInput, value: string) => {
    const next = { ...form, [key]: value };
    setForm(next);
    emit(next, tagsText);
  };
  const onTagsChange = (value: string) => {
    setTagsText(value);
    emit(form, value);
  };
```

tags 的 `Input` 改成 `onChange={(e) => onTagsChange(e.target.value)}`。把提交处的空名错误改成 `setNameError(nameRequiredMessage)`，把名称 `Label` 的固定文案改成 `{nameLabel}`。其余（状态、`FIELDS`、按钮）不动；`onChange` 可选，现有两处调用不传、行为不变。

> **执行期修复轮（2026-10-22）**：Task 6 首版只靠 `formKey` 重挂，`applyTemplate` 读 `editing.input`；而 `ProjectForm` 的手改值在它自己的 `useState`，**先输项目名再选模板会丢名字**（违反 spec D3 / §八 格 6）。修复轮给 `ProjectForm` 补可选 `onChange` 回传 live 值，Task 6 的 `ProjectForm` 传 `onChange` 把 `editing.input` 同步成当前表单值。详见账本 Task 6 fix round 1。

- [ ] **Step 3: 跑 build 与全量门**

```bash
npm run build > /tmp/m12-t4-web.log 2>&1; echo "web=$?"
grep -c "error TS" /tmp/m12-t4-web.log
node --test "tests/**/*.test.ts" > /tmp/m12-t4-node.log 2>&1; echo "node=$?"
grep -E "^ℹ (tests|pass|fail)" /tmp/m12-t4-node.log
(cd src-tauri && cargo test -j 1 --lib > /tmp/m12-t4-cargo.log 2>&1); echo "cargo=$?"
grep "test result" /tmp/m12-t4-cargo.log
```

Expected：web exit=0、0 错；node 57/57/0；cargo 346。

- [ ] **Step 4: 提交**

```bash
git add src/stores/templates.ts src/components/project-form.tsx
git commit -m "feat(m12): 模板 store + ProjectForm 名称标签参数（既有调用不变）"
```

---

### Task 5: 模板管理弹窗 + 项目页按钮

**Files:**
- Create: `src/components/template-manager.tsx`
- Modify: `src/pages/projects.tsx`

**Interfaces:**
- Consumes：`useTemplatesStore`、`ProjectForm` 的 `nameLabel` / `nameRequiredMessage`、`EMPTY_TEMPLATE_INPUT`。
- Produces：`<TemplateManager open onOpenChange statuses />`；项目页右上「模板」按钮。

- [ ] **Step 1: 写组件**

`src/components/template-manager.tsx`：

```tsx
import { useEffect, useState } from "react";
import { Pencil, Plus, Trash2 } from "lucide-react";
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
import { ProjectForm } from "@/components/project-form";
import { useTemplatesStore } from "@/stores/templates";
import { EMPTY_TEMPLATE_INPUT, type ProjectTemplate } from "@/types/project-template";
import type { ProjectInput } from "@/types/project";

// 模板名放进 ProjectInput.name 这一格；applyTemplate 从不读 template.name 写项目名。
function toForm(t: ProjectTemplate): ProjectInput {
  return {
    name: t.name,
    code: t.code ?? "",
    manager: t.manager ?? "",
    customer: t.customer ?? "",
    contactName: t.contactName ?? "",
    contactPhone: t.contactPhone ?? "",
    contractNo: t.contractNo ?? "",
    contractPeriod: t.contractPeriod ?? "",
    deliveryDeadline: t.deliveryDeadline ?? "",
    status: t.status,
    tags: t.tags,
  };
}

export function TemplateManager({
  open,
  onOpenChange,
  statuses,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  statuses: string[];
}) {
  const { items, loading, saving, error, refresh, save, remove } = useTemplatesStore();
  const [editing, setEditing] = useState<{ id: string | null; input: ProjectInput } | null>(null);
  const [confirming, setConfirming] = useState<ProjectTemplate | null>(null);

  useEffect(() => {
    if (open) void refresh();
  }, [open, refresh]);

  const close = () => {
    setEditing(null);
    setConfirming(null);
    onOpenChange(false);
  };

  return (
    <>
      <Dialog open={open} onOpenChange={(o) => (o ? onOpenChange(true) : close())}>
        <DialogContent className="max-h-[90vh] overflow-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{editing ? (editing.id ? "编辑模板" : "新建模板") : "项目模板"}</DialogTitle>
            <DialogDescription>
              模板只保存项目基础信息，新建项目时可以一键填充；不包含项目名、台账或目录。
            </DialogDescription>
          </DialogHeader>

          {error && (
            <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              <div>{error.message}</div>
              {error.hint && <div className="mt-0.5 opacity-80">建议：{error.hint}</div>}
            </div>
          )}

          {editing ? (
            <ProjectForm
              initial={editing.input}
              statuses={statuses}
              saving={saving}
              nameLabel="模板名称"
              nameRequiredMessage="模板名称不能为空"
              onCancel={() => setEditing(null)}
              onSubmit={async (input) => {
                const ok = await save(editing.id, input);
                if (ok) setEditing(null);
                return ok;
              }}
            />
          ) : (
            <div className="space-y-2">
              <div className="flex justify-end">
                <Button size="sm" onClick={() => setEditing({ id: null, input: { ...EMPTY_TEMPLATE_INPUT, tags: [] } })}>
                  <Plus /> 新建模板
                </Button>
              </div>
              {loading ? (
                <p className="text-sm text-muted-foreground">读取模板…</p>
              ) : items.length === 0 ? (
                <div className="rounded-xl border bg-background p-8 text-center text-sm text-muted-foreground">
                  还没有模板，点「新建模板」开始。
                </div>
              ) : (
                <ul className="grid gap-2">
                  {items.map((t) => (
                    <li key={t.id} className="flex items-center gap-2 rounded-xl border bg-background px-3 py-2">
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <span className="truncate text-sm font-medium">{t.name}</span>
                          <Badge variant="secondary">{t.status}</Badge>
                          {t.tags.map((tag) => (
                            <Badge key={tag} variant="outline">
                              {tag}
                            </Badge>
                          ))}
                        </div>
                        <p className="mt-1 truncate text-xs text-muted-foreground">
                          {[t.code, t.manager, t.customer, t.contractNo].filter(Boolean).join(" · ") || "—"}
                        </p>
                      </div>
                      <Button size="icon" variant="ghost" title="编辑" onClick={() => setEditing({ id: t.id, input: toForm(t) })}>
                        <Pencil />
                      </Button>
                      <Button size="icon" variant="ghost" title="删除" onClick={() => setConfirming(t)}>
                        <Trash2 />
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </DialogContent>
      </Dialog>

      <Dialog open={confirming !== null} onOpenChange={(o) => !o && setConfirming(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>删除模板</DialogTitle>
            <DialogDescription>只删除模板「{confirming?.name}」，不会影响已创建的项目。</DialogDescription>
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

- [ ] **Step 2: 项目页挂按钮**

`src/pages/projects.tsx` 顶部 import 加 `TemplateManager`；状态加 `const [templatesOpen, setTemplatesOpen] = useState(false);`；标题行按钮容器改成：

```tsx
        <div className="flex items-center gap-2">
          <Button size="sm" variant="outline" onClick={() => setTemplatesOpen(true)}>
            模板
          </Button>
          <Button size="sm" onClick={() => setEditing({ id: null, input: toInput(emptySeed()) })}>
            <Plus /> 新建项目
          </Button>
        </div>
```

在页面根 `</div>` 前渲染：

```tsx
      <TemplateManager open={templatesOpen} onOpenChange={setTemplatesOpen} statuses={statuses} />
```

- [ ] **Step 3: 跑 build 与全量门**

```bash
npm run build > /tmp/m12-t5-web.log 2>&1; echo "web=$?"
grep -c "error TS" /tmp/m12-t5-web.log
node --test "tests/**/*.test.ts" > /tmp/m12-t5-node.log 2>&1; echo "node=$?"
grep -E "^ℹ (tests|pass|fail)" /tmp/m12-t5-node.log
```

Expected：web exit=0、0 错；node 57/57/0。

- [ ] **Step 4: 提交**

```bash
git add src/components/template-manager.tsx src/pages/projects.tsx
git commit -m "feat(m12): 项目页模板管理弹窗（列表/新建/编辑/删除确认）"
```

---

### Task 6: 新建项目「从模板填充」下拉

**Files:**
- Modify: `src/pages/projects.tsx`

**Interfaces:**
- Consumes：`useTemplatesStore`、`applyTemplate`、Task 5 的 `templatesOpen` / `TemplateManager`。
- Produces：新建项目对话框的模板选择、`ProjectForm onChange` 的 live 值同步与 `formKey` 重挂。

> **执行期修复轮（2026-10-22，见账本 Task 6 fix round 1）**：只看 `editing.input` 不够——`ProjectForm` 的手改值在它的 `useState`，先输项目名再选模板会丢名字（spec D3 / §八 格 6）。下面的 `ProjectForm` 必须传 `onChange`，把 live 值写回 `editing.input`；无模板提示加 `!templatesLoading`。

- [ ] **Step 1: 加 import 与 state/effect**

`src/pages/projects.tsx`：

```tsx
import { applyTemplate } from "@/lib/apply-template";
import { useTemplatesStore } from "@/stores/templates";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
```

在组件内（`templatesOpen` 之后）：

```tsx
  const { items: templates, loading: templatesLoading, refresh: refreshTemplates } = useTemplatesStore();
  const [formKey, setFormKey] = useState(0);
  const [selectedTemplate, setSelectedTemplate] = useState("none");
```

在现有 `useEffect` 里加 `void refreshTemplates();`（依赖数组加 `refreshTemplates`）。

「新建项目」按钮改成：

```tsx
          <Button
            size="sm"
            onClick={() => {
              setSelectedTemplate("none");
              setFormKey((k) => k + 1);
              setEditing({ id: null, input: toInput(emptySeed()) });
            }}
          >
            <Plus /> 新建项目
          </Button>
```

- [ ] **Step 2: 对话框里加模板下拉**

在 `{editing && (` 里、`<ProjectForm …>` 前加：

```tsx
            <div className="grid gap-3">
              {editing.id === null && (
                <div className="grid gap-2">
                  <Label className="text-xs text-muted-foreground">从模板填充</Label>
                  <Select
                    value={selectedTemplate}
                    onValueChange={(v) => {
                      if (v === "none") {
                        setSelectedTemplate("none");
                        return;
                      }
                      const t = templates.find((x) => x.id === v);
                      if (!t) return;
                      setEditing((prev) => (prev ? { ...prev, input: applyTemplate(prev.input, t) } : prev));
                      setFormKey((k) => k + 1);
                      setSelectedTemplate("none");
                    }}
                    disabled={templatesLoading || templates.length === 0}
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue placeholder="选择模板" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">不使用模板</SelectItem>
                      {templates.map((t) => (
                        <SelectItem key={t.id} value={t.id}>
                          {t.name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  {!templatesLoading && templates.length === 0 && (
                    <p className="text-xs text-muted-foreground">暂无模板，先在「模板」里新建。</p>
                  )}
                </div>
              )}
              <ProjectForm
                key={formKey}
                initial={editing.input}
                statuses={statuses}
                saving={saving}
                onChange={(input) => setEditing((prev) => (prev ? { ...prev, input } : prev))}
                onCancel={() => setEditing(null)}
                onSubmit={async (input) => {
                  const ok = await save(editing.id, input);
                  if (ok) setEditing(null);
                  return ok;
                }}
              />
            </div>
```

删除原来单独的 `<ProjectForm …>` 块（上面的新块已包含它）。

- [ ] **Step 3: 跑 build 与全量门**

```bash
npm run build > /tmp/m12-t6-web.log 2>&1; echo "web=$?"
grep -c "error TS" /tmp/m12-t6-web.log
node --test "tests/**/*.test.ts" > /tmp/m12-t6-node.log 2>&1; echo "node=$?"
grep -E "^ℹ (tests|pass|fail)" /tmp/m12-t6-node.log
(cd src-tauri && cargo test -j 1 --lib > /tmp/m12-t6-cargo.log 2>&1); echo "cargo=$?"
grep "test result" /tmp/m12-t6-cargo.log
```

Expected：web exit=0、0 错；node 57/57/0；cargo 346。

- [ ] **Step 4: 提交**

```bash
git add src/pages/projects.tsx
git commit -m "feat(m12): 新建项目从模板填充（项目名保留、空字段不清空、可切换模板）"
```

---

### Task 7: 门禁 + 变异 + 真机 + 映射（控制方自跑）

**Files:**
- Create: `.superpowers/sdd/2026-10-22-m12-project-templates/{mutation-evidence.txt,real-machine-readings.md,spec-plan-mapping.md,m12-cdp.mjs}`
- Modify: 无产品代码；变异必须按字节还原。

**执行口径**：子代理一律不跑 `tauri dev`；真机由控制方用一次性 identifier `dev.zero.pfm.m12test` + CDP **9333** 跑，真实数据目录三文件前后 `size+mtime+sha256` 逐字一致，跑完删净。

- [ ] **Step 1: 全量门禁（退出码真取）**

```bash
cd src-tauri
cargo test -j 1 --lib > /tmp/m12-gate-cargo.log 2>&1; echo "cargo=$?"
grep "test result" /tmp/m12-gate-cargo.log
cargo clippy -j 1 --lib -- -D warnings > /tmp/m12-gate-cl1.log 2>&1; echo "clippy-lib=$?"
cargo clippy -j 1 --all-targets --all-features -- -D warnings > /tmp/m12-gate-cl2.log 2>&1; echo "clippy-all=$?"
cargo build -j 1 --lib > /tmp/m12-gate-build.log 2>&1; echo "rust-build=$?"
cd ..
node --test "tests/**/*.test.ts" > /tmp/m12-gate-node.log 2>&1; echo "node=$?"
grep -E "^ℹ (tests|pass|fail)" /tmp/m12-gate-node.log
npm run build > /tmp/m12-gate-web.log 2>&1; echo "web=$?"; grep -c "error TS" /tmp/m12-gate-web.log
echo "cmd=$(sed -n '/generate_handler/,/^\s*\])/p' src-tauri/src/lib.rs | grep -cE '^ +[a-z_]+,?$') async=$(grep -c 'tauri::command(async)' src-tauri/src/lib.rs) dead=$(grep -rn 'allow(dead_code)' src-tauri/src/ | wc -l) pkg=$(grep -c '^\[\[package\]\]' src-tauri/Cargo.lock) collect=$(grep -c 'hits.append(&mut collect(' src-tauri/src/search.rs)"
grep -oE 'TARGET_VERSION: [a-z0-9]+ = [0-9]+' src-tauri/src/db.rs
git diff --stat master..HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock package.json
git rev-list --merges --count --all
git status --porcelain
```

Expected：cargo **346**；node **57**；两条 clippy 0；build 0；web 0 错；`cmd=69 async=7 dead=1 pkg=616 collect=9`；`TARGET_VERSION: i64 = 8`；依赖 diff 空；merge 0；状态只剩 `?? Python/`。

- [ ] **Step 2: 十处变异（每处备份 → 打红 → `cmp` 还原）**

| # | 变异 | 期望红在哪 |
|---|---|---|
| M① | 删 `create` 里的 `check_duplicate` | `duplicate_name_is_rejected_naming_the_template`（变 `db_failed`/成功） |
| M② | 把空名校验改成不 trim/不拒绝 | `blank_name_and_bad_status_are_rejected` |
| M③ | 删 `STATUSES.contains` 校验 | 同上（非法状态变成 `db_failed`） |
| M④ | `normalize` 的 tags 改成 `input.tags.clone()` | `tags_are_trimmed_deduped_and_round_trip` |
| M⑤ | `delete` 的 SQL 改成 `UPDATE project_templates SET name=name` | `delete_is_hard_and_name_can_be_reused` |
| M⑥ | `applyTemplate` 的 `name` 改成 `template.name` | node「项目名永远保留」 |
| M⑦ | `applyTemplate` 的 `pick` 改成 `next ?? ""` | node「null/空白不清空」 |
| M⑧ | V8 改成空 DDL | `v8_project_templates_table_and_unique_name_exist` |
| M⑨ | `warn_plaintext_columns` 删「项目模板」 | `plaintext_warning_names_every_plain_text_table_since_m9` |
| M⑩ | `list` 去掉 `ORDER BY created_at, id` | `list_orders_by_created_at_then_id` |

每处：`cp` 备份 → 改一处 → 定向测试取真退出码（cargo 失败 101 / node 失败 1）→ `cp` 还原 → `cmp` 字节一致。全部还原后复跑 cargo 346 / node 57。

- [ ] **Step 3: 真机网格（§八 12 格）**

沙盒 `dev.zero.pfm.m12test` + CDP 9333。探针用真 DOM 点击：项目页「模板」按钮、模板弹窗的输入/保存/编辑/删除确认、新建项目对话框的模板 Select 与保存。备份格用 `backup_export` 直接 IPC 到 `%TEMP%` 路径，Python sqlite3 读包内 `backup.db`。只回布尔/条数/字段名，不打印项目正文。覆盖：

1. 空模板列表；2. 新建模板；3. 编辑模板；4. 重名错误；5. 新建项目预填；6. 项目名保留；7. 空字段不清空；8. 编辑项目无下拉；9. 删除与同名重建；10. 模板不进检索；11. 备份包含模板（`Counts` 7 键、`schemaVersion=8`）；12. V8/校验（cargo，不重复真机）。

- [ ] **Step 4: 映射表**

写 `spec-plan-mapping.md`：spec §八 12 格 ↔ 本计划 §八/真机读数；任何未跑的格明写「未验 + 为什么」。证据文件与行号线索落同目录；被活文档按路径引用后**不许 `git clean -fdx`**。

---

### Task 8: 活文档回写

**Files:**
- Modify: `docs/技术方案.md`（§四 数据模型加 `project_templates`；§八 里程碑表加 M12 行）
- Modify: `docs/开发进度.md`（里程碑表 M12 行 + `## M12 验收证据` + 变更日志）
- Modify: `docs/HANDOFF.md`（快照头、§0 M12 bullet、§3 下一格、§4-A 期望数：命令 69 / TARGET 8 / cargo 346 / node 57、§6 指针）
- Modify: 项目记忆（M12 口径与交付终态）

- [ ] **Step 1: 技术方案两处**
- [ ] **Step 2: 开发进度三处**
- [ ] **Step 3: HANDOFF 五处**
- [ ] **Step 4: 文档门复跑与提交**

```bash
grep -c "M12" docs/技术方案.md
grep -rn "待合并\|合并点 = " docs/HANDOFF.md | wc -l
git status --porcelain
git diff --check
```

Expected：第一条 ≥1；第二条 0（合并前）；`git diff --check` 0；只多本任务改的文档。

---

## 自审记录（控制方写完后现跑现数）

- **Spec 覆盖**：D1→Task 2/3/6（线格式、applyTemplate）；D2→Task 5/6；D3→Task 3/6（node）；D4→Task 5；D5→Task 1/2（V8/硬删/JSON）；D6→Task 2（4 命令/69）；D7→Task 1（Counts/告警/schema 8）；D8→Task 7（collect 9/搜索 0 命中）；D9→Task 2（ORDER BY）；D10→Task 2（错误码）。
- **类型一致性**：Rust `ProjectTemplate` / `TemplateInput` ↔ TS 同名接口 ↔ 命令 `template_*`；`applyTemplate` 只读 `template` 字段、不读 `template.name` 写项目名；`ProjectForm` 两 prop 有默认值。
- **数链**：Task 1 +2（V8 表 + 备份包）=339；Task 2 +7=346；Task 3 node +5=57；命令 69；TARGET 8。
- **预检结果**：见「派发前预检」勾选项；SQL 与 applyTemplate 的实际读数回填在这里。
