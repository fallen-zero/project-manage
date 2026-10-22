# M11-b 台账导出（按项目，Excel / Markdown）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在项目详情页把该项目的台账五表导出成 `.xlsx`（五个工作表）或 `.md`（五个小节）；默认只导明文列，含账号密码明文必须当次显式勾选并二次确认。

**Architecture:** 导出逻辑全在 Rust 新模块 `src-tauri/src/ledger_export.rs`：一个「导出行模型」（`ExportDoc`/`Sheet`）+ 一个取数函数 `collect(conn, mk: Option<&MasterKey>, project_id)`（`None` = 不读密文列）+ 两个渲染器（`write_xlsx` / `write_markdown`）+ 一个 `write` 分派与回执。IPC 只加一条 `ledger_export`，**先 `backup::guard_destination` 再取数/写盘**。前端只做「格式下拉 + 明文勾选 + 二次确认 + 保存对话框」，默认文件名由 TS 纯函数 `defaultFileName` 生成（后端只收最终 `path`）。

**Tech Stack:** Rust / `rusqlite`（bundled SQLite）/ `rust_xlsxwriter`（写 xlsx，M10 起已是直接依赖）/ `calamine`（测试读回，已在树里）/ `vault::decrypt_field`（按行 id + 逻辑列名解密）/ React 19 + TS + `@tauri-apps/plugin-dialog` 的 `save`。

**Spec:** `docs/superpowers/specs/2026-10-21-m11b-ledger-export-design.md`（D1–D12、§三 列集合、§四 IPC 契约、§九 13 格验收、§十一 7 处变异靶）。**规格与计划冲突时以规格为准**，并停下来报告。

## Global Constraints

- **按项目导出**（D1）；**双格式**（D2）；**默认不读密文列、显式勾选 + 二次确认才解密**（D3）；**不导软删行、归档项目照导**（D4）。
- **命令数 64→65**（D5，需求方 2026-10-21 当次批准的第 6 次破例）；`tauri::command(async)` 仍 **7**，新命令**同步**。
- **零新依赖**：`src-tauri/Cargo.toml`、`Cargo.lock`、`package.json` 对 `master` 的 diff 必须 **0 字节**；`Cargo.lock` `[[package]]` 仍 **616**。
- **不新增表/迁移**：`TARGET_VERSION: i64 = 7` 不动；全仓 `allow(dead_code)` 仍 **1**（`db.rs:30`，本轮不许开新豁免）。
- **落点守卫只落一次**：`ledger_export` 命令体里、任何写盘之前调 `backup::guard_destination`；`ledger_export.rs` 内部不再写一份判据。
- **错误码不新开**：只用 `invalid_input` / `not_found` / `backup_destination_unsafe` / `field_decrypt_failed` / `fs_failed`。
- **取数顺序与 `ledger::list` 一致**（环境 `dev→test→pre→prod`），不新增第二套排序；环境值与链接类型**导库内原值**（不把 `ENV_LABELS` 搬进 Rust）。
- **注释正文是承重的**：任何 Rust 源文件注释里都不许写字面量 `#[tauri::command(async)]`、`allow(dead_code)`、`hits.append(&mut collect(`（结构门是原始 grep）。
- **每个 Task 结束即提交并推 `origin feat/m11b-ledger-export`**；**合并回 `master` 要需求方当次批准**。
- **本机 16G**：cargo 一律 `-j 1`；报「could not compile 却没有 `error[Exxxx]`」或 `os error 1455` ⇒ 先重跑同一条命令（那是提交内存）。
- **退出码口径**：一律 `cmd > /tmp/x.log 2>&1; echo "exit=$?"; grep … /tmp/x.log`；**绝不** `cmd | tail; echo $?`（管道里 `$?` 是 tail 的）。cargo 测试失败退出码是 **101**。
- **工作树是 CRLF**（`core.autocrlf=true`）：新建文件也要落成 `w/crlf`（`git ls-files --eol` 复核）；长文档用 Edit 逐处替换，别按行号切。

## 数链（控制方派发前现数，见 §自审记录）

| Task | 新增 cargo 测试 | cargo 累计 | 新增 node 测试 | node 累计 |
|---|---|---|---|---|
| 基线（`master` 合并点 + M11-b spec） | — | **318** | — | **47** |
| 1 行模型 + 默认取数 | +3 | **321** | 0 | 47 |
| 2 含明文路径 | +5 | **326** | 0 | 47 |
| 3 `write_xlsx` | +2（另 +1 长单元格，修复轮补） | **329** | 0 | 47 |
| 4 `write_markdown` | +2 | **331** | 0 | 47 |
| 5 IPC + 守卫 + 格式校验 | +2 | **333** | 0 | 47 |
| 6 前端（默认名纯函数 + 组件） | 0 | 333 | +5（含修复轮 1 支 emoji 边界） | **52** |
| 7 门禁 + 变异 + 真机 | 0 | **333** | 0 | **52** |
| 8 活文档回写 | 0 | 333 | 0 | 52 |

---

### Task 1: `ledger_export.rs` 行模型 + 默认路径取数（不读密文列）

**Files:**
- Create: `src-tauri/src/ledger_export.rs`
- Modify: `src-tauri/src/lib.rs`（模块表加 `mod ledger_export;`）
- Test: `src-tauri/src/ledger_export.rs` 里的 `#[cfg(test)] mod tests`

**Interfaces:**
- Produces：`Sheet { name: &'static str, headers: &'static [&'static str], rows: Vec<Vec<String>> }`、`ExportDoc { project_name, generated_at, included_secrets, sheets }`、`collect(conn: &Connection, mk: Option<&MasterKey>, project_id: &str) -> AppResult<ExportDoc>`（Task 2 在本函数里补 `Some(mk)` 分支）、`SHEET_NAMES: [&str; 5]`、`format_of(&str) -> AppResult<()>`。
- Consumes：`crate::error::{AppError, AppResult}`、`crate::vault::MasterKey`、`rusqlite::{Connection, OptionalExtension}`。

- [ ] **Step 1: 写失败测试（三支：五表恒定、环境排序、软删不出现）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::ledger;

    fn fixture() -> (Connection, String) {
        let conn = db::open_in_memory().unwrap();
        let pid = crate::project::create_project(
            &conn,
            &crate::project::ProjectInput {
                name: "政务云迁移".to_owned(),
                code: Some("GOV-01".to_owned()),
                manager: None,
                customer: None,
                contact_name: None,
                contact_phone: None,
                contract_no: None,
                contract_period: None,
                delivery_deadline: None,
                status: "开发".to_owned(),
                tags: vec![],
            },
        )
        .unwrap()
        .id;
        (conn, pid)
    }

    #[test]
    fn collect_always_returns_five_sheets_in_fixed_order() {
        let (conn, pid) = fixture();
        let doc = collect(&conn, None, &pid).unwrap();
        let names: Vec<&str> = doc.sheets.iter().map(|s| s.name).collect();
        assert_eq!(names, SHEET_NAMES.to_vec(), "五张表的名字与顺序是线格式契约");
        assert!(doc.sheets.iter().all(|s| s.rows.is_empty()));
        assert!(!doc.included_secrets, "默认路径不算含明文");
        assert_eq!(doc.project_name, "政务云迁移");
    }

    #[test]
    fn collect_orders_envs_dev_test_pre_prod_regardless_of_insert_order() {
        let (conn, pid) = fixture();
        for env in ["prod", "dev", "test", "pre"] {
            ledger::create_env(&conn, &pid, &ledger::EnvInput {
                env: env.to_owned(),
                name: format!("{env} 环境"),
                url: None,
                port: None,
                note: None,
            })
            .unwrap();
        }
        let doc = collect(&conn, None, &pid).unwrap();
        let envs: Vec<&str> = doc.sheets[0].rows.iter().map(|r| r[0].as_str()).collect();
        assert_eq!(envs, vec!["dev", "test", "pre", "prod"], "排序复用 ledger::list 的同一套 CASE");
    }

    #[test]
    fn collect_skips_soft_deleted_rows() {
        let (conn, pid) = fixture();
        let env = ledger::create_env(&conn, &pid, &ledger::EnvInput {
            env: "dev".to_owned(),
            name: "要删的".to_owned(),
            url: None,
            port: None,
            note: None,
        })
        .unwrap();
        ledger::delete_env(&conn, &env.id).unwrap();
        let doc = collect(&conn, None, &pid).unwrap();
        assert!(doc.sheets[0].rows.is_empty(), "软删行不导（D4）");
    }
}
```

- [ ] **Step 2: 跑它，确认红在「模块不存在」以外的地方**

Run: `cd src-tauri && cargo test -j 1 --lib ledger_export > /tmp/te1-red.log 2>&1; echo "exit=$?"; grep -E "cannot find|test result|error\[E" /tmp/te1-red.log | head -8`
Expected：`exit=101`，报 `cannot find function collect` / `cannot find value SHEET_NAMES`（编译层红）。**本格 RED 只到编译层** ⇒ Step 6 必须用变异取证补上「断言有牙」的证据（本仓口径）。

- [ ] **Step 3: 写最小实现（行模型 + 默认取数）**

> **执行期偏差（Task 1 修复轮 1）**：四张非环境表的排序在代码里补成了 `created_at, id`（同秒可复现），环境那段 `CASE env …` 抽成了 `ledger.rs` 的 `pub(crate) const ENV_ORDER_SQL` 并在 `ledger::list` 与导出两处共用。**下面代码块是初版**，以仓库实现与 spec §三 为准。

```rust
//! 台账导出（M11-b spec）：把五张台账表摊成「导出行模型」，再由两个渲染器写成 xlsx / Markdown。
//!
//! 红线（spec D3）：**默认路径不读任何密文列** —— `mk` 为 `None` 时下面的 SQL 里根本不出现
//! `*_cipher` / `*_nonce`；只有调用方显式传 `Some(mk)` 才解密（Task 2）。变异 M② 打的就是这条。
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::vault::MasterKey;

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查导出参数"))
}

fn not_found(id: &str) -> AppError {
    AppError::new("not_found", &format!("记录不存在：{id}"), Some("刷新后重试"))
}

fn write_fail(path: &Path, e: impl std::fmt::Display) -> AppError {
    AppError::new(
        "fs_failed",
        &format!("写 {} 失败：{e}", path.display()),
        Some("确认该路径存在且可读写"),
    )
}

pub struct Sheet {
    pub name: &'static str,
    pub headers: &'static [&'static str],
    pub rows: Vec<Vec<String>>,
}

pub struct ExportDoc {
    pub project_name: String,
    pub generated_at: String,
    pub included_secrets: bool,
    pub sheets: Vec<Sheet>,
}

/// 五张表的固定顺序与名字（spec §三）。xlsx 的工作表名与 Markdown 的小节名共用这一份。
pub const SHEET_NAMES: [&str; 5] = ["环境", "凭据", "服务器", "链接", "备注"];

const ENV_HEADERS: &[&str] = &["环境", "名称", "地址", "端口", "备注"];
const CRED_HEADERS: &[&str] = &["标题", "所属环境", "账号", "密码", "地址", "备注"];
const SERVER_HEADERS: &[&str] = &["名称", "IP", "堡垒机", "账号", "密码", "备注"];
const LINK_HEADERS: &[&str] = &["类型", "标题", "地址", "备注"];
const NOTE_HEADERS: &[&str] = &["标题", "标签", "正文"];

/// 只收 xlsx / md（spec §四 的第一道前置）。
pub fn format_of(format: &str) -> AppResult<()> {
    match format {
        "xlsx" | "md" => Ok(()),
        other => Err(invalid(&format!(
            "导出格式只支持 xlsx 与 md，收到「{other}」"
        ))),
    }
}

pub fn collect(conn: &Connection, mk: Option<&MasterKey>, project_id: &str) -> AppResult<ExportDoc> {
    let project_name: String = conn
        .query_row(
            "SELECT name FROM projects WHERE id = ?1 AND deleted_at IS NULL",
            [project_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| not_found(project_id))?;
    let generated_at: String = conn.query_row("SELECT datetime('now','localtime')", [], |r| r.get(0))?;

    let envs = collect_envs(conn, project_id)?;
    let credentials = collect_credentials(conn, mk, project_id)?;
    let servers = collect_servers(conn, mk, project_id)?;
    let links = collect_links(conn, project_id)?;
    let notes = collect_notes(conn, project_id)?;

    Ok(ExportDoc {
        project_name,
        generated_at,
        included_secrets: mk.is_some(),
        sheets: vec![
            Sheet { name: SHEET_NAMES[0], headers: ENV_HEADERS, rows: envs },
            Sheet { name: SHEET_NAMES[1], headers: CRED_HEADERS, rows: credentials },
            Sheet { name: SHEET_NAMES[2], headers: SERVER_HEADERS, rows: servers },
            Sheet { name: SHEET_NAMES[3], headers: LINK_HEADERS, rows: links },
            Sheet { name: SHEET_NAMES[4], headers: NOTE_HEADERS, rows: notes },
        ],
    })
}

fn collect_envs(conn: &Connection, project_id: &str) -> AppResult<Vec<Vec<String>>> {
    let mut stmt = conn.prepare(
        "SELECT env, name, COALESCE(url,''), COALESCE(port,''), COALESCE(note,'')
           FROM ledger_envs
          WHERE project_id = ?1 AND deleted_at IS NULL
          ORDER BY CASE env WHEN 'dev' THEN 0 WHEN 'test' THEN 1 WHEN 'pre' THEN 2 ELSE 3 END, created_at",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(vec![r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?])
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn collect_credentials(
    conn: &Connection,
    _mk: Option<&MasterKey>,
    project_id: &str,
) -> AppResult<Vec<Vec<String>>> {
    // 默认路径：SQL 里**没有** username_cipher / password_cipher —— 这一条是 D3 的可证伪形态。
    let mut stmt = conn.prepare(
        "SELECT c.title, COALESCE(e.name,''), COALESCE(c.url,''), COALESCE(c.note,'')
           FROM ledger_credentials c
           LEFT JOIN ledger_envs e ON e.id = c.env_id
          WHERE c.project_id = ?1 AND c.deleted_at IS NULL
          ORDER BY c.created_at, c.id",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(vec![
            r.get(0)?,
            r.get(1)?,
            String::new(),
            String::new(),
            r.get(2)?,
            r.get(3)?,
        ])
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn collect_servers(
    conn: &Connection,
    _mk: Option<&MasterKey>,
    project_id: &str,
) -> AppResult<Vec<Vec<String>>> {
    let mut stmt = conn.prepare(
        "SELECT name, COALESCE(ip,''), COALESCE(bastion,''), COALESCE(note,'')
           FROM ledger_servers
          WHERE project_id = ?1 AND deleted_at IS NULL
          ORDER BY created_at",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(vec![
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            String::new(),
            String::new(),
            r.get(3)?,
        ])
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn collect_links(conn: &Connection, project_id: &str) -> AppResult<Vec<Vec<String>>> {
    let mut stmt = conn.prepare(
        "SELECT kind, title, url, COALESCE(note,'')
           FROM ledger_links
          WHERE project_id = ?1 AND deleted_at IS NULL
          ORDER BY created_at",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(vec![r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?])
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn collect_notes(conn: &Connection, project_id: &str) -> AppResult<Vec<Vec<String>>> {
    let mut stmt = conn.prepare(
        "SELECT title, COALESCE(tag,''), body
           FROM ledger_notes
          WHERE project_id = ?1 AND deleted_at IS NULL
          ORDER BY created_at",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(vec![r.get(0)?, r.get(1)?, r.get(2)?])
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
```

`lib.rs` 模块表：在 `mod ledger;` 之后、`mod legacy_office;` 之前插入（字母序 `ledger` < `ledger_export` < `legacy_office`）：

```rust
mod ledger_export;
```

- [ ] **Step 4: 跑测试确认绿**

Run: `cd src-tauri && cargo test -j 1 --lib ledger_export > /tmp/te1-green.log 2>&1; echo "exit=$?"; grep -E "test result" /tmp/te1-green.log; cd .. && cargo test -j 1 --lib 2>/dev/null | tail -1`
Expected：聚焦三条全绿；全量 **321 passed / exit=0**。

- [ ] **Step 5: 变异自证（本格 RED 只在编译层，这一格补牙）**

把 `collect_envs` 的 `AND deleted_at IS NULL` 删掉 ⇒ `collect_skips_soft_deleted_rows` 必须红；再用 `cp` 备份还原、`cmp` 验字节、复跑 321 绿。把原始输出记进 Task 7 证据文件之前，先贴进本任务的实现报告。

- [ ] **Step 6: 提交并推**

```bash
git add src-tauri/src/ledger_export.rs src-tauri/src/lib.rs
git commit -m "feat(m11-b): 台账导出行模型 + 默认路径取数（不读密文列）—— 321 passed"
git push origin feat/m11b-ledger-export
```

---

### Task 2: 含明文路径（显式 `Some(mk)` 才解密）

**Files:**
- Modify: `src-tauri/src/ledger_export.rs`（`collect_credentials` / `collect_servers` 的 `Some(mk)` 分支 + `decrypt_or_empty`）
- Test: 同文件 tests

**Interfaces:**
- Consumes：Task 1 的 `collect`；`crate::vault::{self, EncryptedField, MasterKey}`；`vault::decrypt_field(mk, row_id, column, &EncryptedField { cipher, nonce })` —— **`column` 传逻辑列名**（`"username"` / `"password"` / `"account"`），不是 `*_cipher` 列名（AAD 拼的是 `aad(row_id, column)`，`ledger::reveal` 就是这么传的）。
- Produces：`collect(conn, Some(mk), pid)` 的凭据/服务器行在账号、密码两列给出**明文**；`included_secrets = true`。

- [ ] **Step 1: 写失败测试（五支）**

```rust
    fn fixture_with_secret() -> (Connection, String, MasterKey) {
        let (conn, pid) = fixture();
        let code = crate::vault::generate_recovery_code().unwrap();
        let mk = crate::vault::initialize(&conn, "主密码-至少八位", &code).unwrap();
        (conn, pid, mk)
    }

    #[test]
    fn collect_without_secrets_leaves_secret_cells_empty() {
        let (conn, pid, mk) = fixture_with_secret();
        ledger::create_credential(&conn, &mk, &pid, &ledger::CredentialInput {
            env_id: None,
            title: "运维账号".to_owned(),
            username: Some("gov-admin".to_owned()),
            password: Some("S3cret-值".to_owned()),
            url: None,
            note: None,
        })
        .unwrap();
        let doc = collect(&conn, None, &pid).unwrap();
        let row = &doc.sheets[1].rows[0];
        assert_eq!(row[2], "", "默认路径账号列留空");
        assert_eq!(row[3], "", "默认路径密码列留空");
        assert!(!doc.included_secrets);
    }

    #[test]
    fn collect_with_secrets_decrypts_and_matches_reveal() {
        let (conn, pid, mk) = fixture_with_secret();
        let cred = ledger::create_credential(&conn, &mk, &pid, &ledger::CredentialInput {
            env_id: None,
            title: "运维账号".to_owned(),
            username: Some("gov-admin".to_owned()),
            password: Some("S3cret-值".to_owned()),
            url: None,
            note: None,
        })
        .unwrap();
        let doc = collect(&conn, Some(&mk), &pid).unwrap();
        let row = &doc.sheets[1].rows[0];
        assert_eq!(row[2], ledger::reveal(&conn, &mk, "credential", &cred.id, "username").unwrap());
        assert_eq!(row[3], ledger::reveal(&conn, &mk, "credential", &cred.id, "password").unwrap());
        assert!(doc.included_secrets);
    }

    #[test]
    fn collect_with_secrets_decrypts_server_account_and_password() {
        let (conn, pid, mk) = fixture_with_secret();
        let srv = ledger::create_server(&conn, &mk, &pid, &ledger::ServerInput {
            name: "跳板机".to_owned(),
            ip: Some("10.0.0.8".to_owned()),
            bastion: None,
            account: Some("root".to_owned()),
            password: Some("P@ss-服务器".to_owned()),
            note: None,
        })
        .unwrap();
        let doc = collect(&conn, Some(&mk), &pid).unwrap();
        let row = &doc.sheets[2].rows[0];
        assert_eq!(row[3], ledger::reveal(&conn, &mk, "server", &srv.id, "account").unwrap());
        assert_eq!(row[4], ledger::reveal(&conn, &mk, "server", &srv.id, "password").unwrap());
    }

    #[test]
    fn collect_with_secrets_keeps_empty_secret_columns_empty() {
        let (conn, pid, mk) = fixture_with_secret();
        ledger::create_credential(&conn, &mk, &pid, &ledger::CredentialInput {
            env_id: None,
            title: "只有标题".to_owned(),
            username: None,
            password: None,
            url: None,
            note: None,
        })
        .unwrap();
        let doc = collect(&conn, Some(&mk), &pid).unwrap();
        assert_eq!(doc.sheets[1].rows[0][2], "");
        assert_eq!(doc.sheets[1].rows[0][3], "");
    }

    #[test]
    fn collect_returns_field_decrypt_failed_on_tampered_cipher() {
        let (conn, pid, mk) = fixture_with_secret();
        let cred = ledger::create_credential(&conn, &mk, &pid, &ledger::CredentialInput {
            env_id: None,
            title: "会被改坏".to_owned(),
            username: Some("u".to_owned()),
            password: Some("p".to_owned()),
            url: None,
            note: None,
        })
        .unwrap();
        conn.execute(
            "UPDATE ledger_credentials SET password_cipher = X'00' WHERE id = ?1",
            [&cred.id],
        )
        .unwrap();
        let err = collect(&conn, Some(&mk), &pid).unwrap_err();
        assert_eq!(err.code, "field_decrypt_failed", "密文坏掉要整份失败，不许静默导出半份");
    }
```

- [ ] **Step 2: 跑它确认红**

Run: `cd src-tauri && cargo test -j 1 --lib ledger_export > /tmp/te2-red.log 2>&1; echo "exit=$?"; grep -E "^test |test result" /tmp/te2-red.log | head -10`
Expected：`exit=101`，`collect_with_secrets_decrypts_and_matches_reveal` 等红在「账号列为空」。

- [ ] **Step 3: 实现 `Some(mk)` 分支**

```rust
fn decrypt_or_empty(
    mk: &MasterKey,
    row_id: &str,
    field: &str,
    cipher: Option<Vec<u8>>,
    nonce: Option<Vec<u8>>,
) -> AppResult<String> {
    match (cipher, nonce) {
        (Some(cipher), Some(nonce)) => {
            crate::vault::decrypt_field(mk, row_id, field, &crate::vault::EncryptedField { cipher, nonce })
        }
        _ => Ok(String::new()),
    }
}
```

`collect_credentials` 改成按 `mk` 分岔（**默认那支保持 Task 1 的 SQL 原样，别把两支护揉进一条动态 SQL**）：

```rust
fn collect_credentials(
    conn: &Connection,
    mk: Option<&MasterKey>,
    project_id: &str,
) -> AppResult<Vec<Vec<String>>> {
    let Some(mk) = mk else {
        let mut stmt = conn.prepare(
            "SELECT c.title, COALESCE(e.name,''), COALESCE(c.url,''), COALESCE(c.note,'')
               FROM ledger_credentials c
               LEFT JOIN ledger_envs e ON e.id = c.env_id
              WHERE c.project_id = ?1 AND c.deleted_at IS NULL
              ORDER BY c.created_at, c.id",
        )?;
        let rows = stmt.query_map([project_id], |r| {
            Ok(vec![r.get(0)?, r.get(1)?, String::new(), String::new(), r.get(2)?, r.get(3)?])
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        return Ok(out);
    };

    let mut stmt = conn.prepare(
        "SELECT c.id, c.title, COALESCE(e.name,''), COALESCE(c.url,''), COALESCE(c.note,''),
                c.username_cipher, c.username_nonce, c.password_cipher, c.password_nonce
           FROM ledger_credentials c
           LEFT JOIN ledger_envs e ON e.id = c.env_id
          WHERE c.project_id = ?1 AND c.deleted_at IS NULL
          ORDER BY c.created_at, c.id",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<Vec<u8>>>(5)?,
            r.get::<_, Option<Vec<u8>>>(6)?,
            r.get::<_, Option<Vec<u8>>>(7)?,
            r.get::<_, Option<Vec<u8>>>(8)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, title, env_name, url, note, uc, un, pc, pn) = row?;
        let username = decrypt_or_empty(mk, &id, "username", uc, un)?;
        let password = decrypt_or_empty(mk, &id, "password", pc, pn)?;
        out.push(vec![title, env_name, username, password, url, note]);
    }
    Ok(out)
}
```

`collect_servers` 同形：默认支保持 Task 1 原样；`Some(mk)` 支查 `id, name, ip, bastion, note, account_cipher, account_nonce, password_cipher, password_nonce`，逻辑列名传 `"account"` 与 `"password"`。

- [ ] **Step 4: 跑全套确认绿**

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/te2-green.log 2>&1; echo "exit=$?"; grep "test result" /tmp/te2-green.log`
Expected：**326 passed / exit=0**。

- [ ] **Step 5: 变异自证（M② 的预演）**

把 `collect` 里的 `mk` 恒传给 `Some(&mk_from_somewhere)` 不可行（没有 key）；改为**把默认支的 SQL 改成也 SELECT 密文列并解密**（临时让 `let Some(mk) = mk else { … }` 走解密支）⇒ `collect_without_secrets_leaves_secret_cells_empty` 必须红。`cp` 还原 + `cmp` + 复跑 326 绿。

- [ ] **Step 6: 提交并推**

```bash
git add src-tauri/src/ledger_export.rs
git commit -m "feat(m11-b): 台账导出含明文路径（显式 Some(mk) 才解密，逻辑列名进 AAD）—— 326 passed"
git push origin feat/m11b-ledger-export
```

---

### Task 3: `write_xlsx`（五个工作表）

**Files:**
- Modify: `src-tauri/src/ledger_export.rs`
- Test: 同文件 tests

**Interfaces:**
- Produces：`write_xlsx(path: &Path, doc: &ExportDoc) -> AppResult<()>`（Task 5 经 `write` 调它）。
- Consumes：`rust_xlsxwriter::{Format, Workbook}`（写法照 `daily_report::write_xlsx`：`add_worksheet` → `set_name` → 冻结首行 → 表头加粗 → 逐格 `write`）。

- [ ] **Step 1: 写失败测试（两支：五表往返、零行也写表头）**

```rust
    fn read_sheets(path: &std::path::Path) -> Vec<(String, Vec<Vec<String>>)> {
        use calamine::Reader as _;
        let mut book = calamine::open_workbook_auto(path).unwrap();
        let names = book.sheet_names().to_vec();
        names
            .into_iter()
            .map(|n| {
                let range = book.worksheet_range(&n).unwrap();
                let rows = range
                    .rows()
                    .map(|r| r.iter().map(|c| c.to_string()).collect::<Vec<String>>())
                    .collect::<Vec<Vec<String>>>();
                (n, rows)
            })
            .collect()
    }

    #[test]
    fn write_xlsx_emits_five_worksheets_with_headers_and_multiline_note() {
        let (conn, pid) = fixture();
        ledger::create_note(&conn, &pid, &ledger::NoteInput {
            title: "上线步骤".to_owned(),
            body: Some("第一步\n第二步".to_owned()),
            tag: Some("运维".to_owned()),
        })
        .unwrap();
        let doc = collect(&conn, None, &pid).unwrap();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("m11b-xlsx-{}.xlsx", std::process::id()));
        write_xlsx(&path, &doc).unwrap();
        let sheets = read_sheets(&path);
        let names: Vec<String> = sheets.iter().map(|(n, _)| n.clone()).collect();
        assert_eq!(names, vec!["环境", "凭据", "服务器", "链接", "备注"]);
        assert_eq!(sheets[3].1[0], vec!["类型", "标题", "地址", "备注"], "表头逐字是契约");
        let note = &sheets[4].1[1];
        assert_eq!(note[0], "上线步骤");
        assert!(note[2].contains("第一步") && note[2].contains("第二步"), "多行正文要连换行一起写进单元格");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn write_xlsx_with_zero_rows_still_writes_headers() {
        let (conn, pid) = fixture();
        let doc = collect(&conn, None, &pid).unwrap();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("m11b-xlsx-empty-{}.xlsx", std::process::id()));
        write_xlsx(&path, &doc).unwrap();
        let sheets = read_sheets(&path);
        assert_eq!(sheets.len(), 5);
        assert_eq!(sheets[0].1.len(), 1, "零行也要有表头一行");
        std::fs::remove_file(&path).ok();
    }
```

- [ ] **Step 2: 跑它确认红**

Run: `cd src-tauri && cargo test -j 1 --lib write_xlsx > /tmp/te3-red.log 2>&1; echo "exit=$?"; grep -E "cannot find|error\[E|test result" /tmp/te3-red.log | head -6`
Expected：`exit=101`（`write_xlsx` 不存在）。

- [ ] **Step 3: 实现**

```rust
pub fn write_xlsx(path: &Path, doc: &ExportDoc) -> AppResult<()> {
    use rust_xlsxwriter::{Format, Workbook};

    let mut wb = Workbook::new();
    let bold = Format::new().set_bold();
    let wrap = Format::new().set_text_wrap();
    for sheet in &doc.sheets {
        let ws = wb.add_worksheet();
        ws.set_name(sheet.name).map_err(|e| write_fail(path, e))?;
        ws.set_freeze_panes(1, 0).map_err(|e| write_fail(path, e))?;
        for (col, head) in sheet.headers.iter().enumerate() {
            ws.write_with_format(0, col as u16, *head, &bold)
                .map_err(|e| write_fail(path, e))?;
        }
        for (i, row) in sheet.rows.iter().enumerate() {
            let r = (i + 1) as u32;
            for (col, cell) in row.iter().enumerate() {
                ws.write_with_format(r, col as u16, cell.as_str(), &wrap)
                    .map_err(|e| write_fail(path, e))?;
            }
        }
    }
    wb.save(path).map_err(|e| write_fail(path, e))?;
    Ok(())
}
```

- [ ] **Step 4: 跑全套确认绿**

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/te3-green.log 2>&1; echo "exit=$?"; grep "test result" /tmp/te3-green.log`
Expected：**328 passed / exit=0**。

- [ ] **Step 5: 提交并推**

```bash
git add src-tauri/src/ledger_export.rs
git commit -m "feat(m11-b): xlsx 渲染器（五工作表、表头加粗、正文换行保留）—— 328 passed"
git push origin feat/m11b-ledger-export
```

---

### Task 4: `write_markdown`（五小节 + 转义 + 空表标记）

**Files:**
- Modify: `src-tauri/src/ledger_export.rs`
- Test: 同文件 tests

**Interfaces:**
- Produces：`write_markdown(path: &Path, doc: &ExportDoc) -> AppResult<()>`、`pub fn escape_md(s: &str) -> String`（测试直接调）。

- [ ] **Step 1: 写失败测试（两支）**

```rust
    #[test]
    fn write_markdown_escapes_pipe_and_converts_newlines() {
        let (conn, pid) = fixture();
        ledger::create_note(&conn, &pid, &ledger::NoteInput {
            title: "含|竖线".to_owned(),
            body: Some("第一行\n第二行".to_owned()),
            tag: None,
        })
        .unwrap();
        let doc = collect(&conn, None, &pid).unwrap();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("m11b-md-{}.md", std::process::id()));
        write_markdown(&path, &doc).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# 台账 — "), "标题行");
        assert!(text.contains("## 备注"), "五个小节都在");
        assert!(text.contains("含\\|竖线"), "单元格里的 | 必须转义：{text}");
        assert!(text.contains("第一行<br>第二行"), "换行转 <br>");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn write_markdown_marks_empty_sheet_as_no_records() {
        let (conn, pid) = fixture();
        let doc = collect(&conn, None, &pid).unwrap();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("m11b-md-empty-{}.md", std::process::id()));
        write_markdown(&path, &doc).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("（无记录）").count(), 5, "五张空表各写一次：{text}");
        std::fs::remove_file(&path).ok();
    }
```

- [ ] **Step 2: 跑它确认红**

Run: `cd src-tauri && cargo test -j 1 --lib write_markdown > /tmp/te4-red.log 2>&1; echo "exit=$?"; grep -E "cannot find|error\[E|test result" /tmp/te4-red.log | head -6`
Expected：`exit=101`。

- [ ] **Step 3: 实现**

```rust
pub fn escape_md(s: &str) -> String {
    s.replace('|', "\\|").replace('\r', "").replace('\n', "<br>")
}

pub fn write_markdown(path: &Path, doc: &ExportDoc) -> AppResult<()> {
    let mut out = String::new();
    out.push_str(&format!("# 台账 — {}\n\n", doc.project_name));
    out.push_str(&format!("- 导出时间：{}\n", doc.generated_at));
    out.push_str(&format!(
        "- 含明文：{}\n\n",
        if doc.included_secrets { "是" } else { "否" }
    ));
    for sheet in &doc.sheets {
        out.push_str(&format!("## {}\n\n", sheet.name));
        if sheet.rows.is_empty() {
            out.push_str("（无记录）\n\n");
            continue;
        }
        out.push_str("| ");
        out.push_str(
            &sheet
                .headers
                .iter()
                .map(|h| escape_md(h))
                .collect::<Vec<_>>()
                .join(" | "),
        );
        out.push_str(" |\n|");
        out.push_str(&sheet.headers.iter().map(|_| " --- ").collect::<Vec<_>>().join("|"));
        out.push_str("|\n");
        for row in &sheet.rows {
            out.push_str("| ");
            out.push_str(&row.iter().map(|c| escape_md(c)).collect::<Vec<_>>().join(" | "));
            out.push_str(" |\n");
        }
        out.push('\n');
    }
    std::fs::write(path, out).map_err(|e| AppError::io(path, &e))
}
```

- [ ] **Step 4: 跑全套确认绿**

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/te4-green.log 2>&1; echo "exit=$?"; grep "test result" /tmp/te4-green.log`
Expected：**330 passed / exit=0**。

- [ ] **Step 5: 变异自证（M④/M⑤ 预演）**

把 `escape_md` 的 `.replace('|', "\\|")` 删掉 ⇒ 转义那条红；把空表分支整块删掉 ⇒ 「（无记录）」那条红。各自 `cp` 还原 + `cmp` + 复跑 330 绿。

- [ ] **Step 6: 提交并推**

```bash
git add src-tauri/src/ledger_export.rs
git commit -m "feat(m11-b): Markdown 渲染器（五小节、| 转义、换行 <br>、空表标记）—— 330 passed"
git push origin feat/m11b-ledger-export
```

---

### Task 5: IPC 命令 + 落点守卫 + 格式校验

**Files:**
- Modify: `src-tauri/src/ledger_export.rs`（`write` 分派 + `LedgerExportSummary` + 两条测试）
- Modify: `src-tauri/src/lib.rs`（`LedgerExportInput`、`ledger_export` 命令、`generate_handler!` 注册、守卫顺序文本门）

**Interfaces:**
- Consumes：`backup::guard_destination(&state.data_dir, &Path)`、`unlocked_mk(&state)`、`db(&state)`、Task 1–4 的 `collect`/`write_xlsx`/`write_markdown`。
- Produces：命令 `ledger_export(input: LedgerExportInput) -> LedgerExportSummary`；`write(path: &str, format: &str, doc: &ExportDoc) -> AppResult<LedgerExportSummary>`；`format_of`（Task 1 已有）。

- [ ] **Step 1: 写失败测试（两条：格式校验、守卫文本门）**

`ledger_export.rs`：

```rust
    #[test]
    fn format_of_accepts_only_xlsx_and_md() {
        assert!(format_of("xlsx").is_ok());
        assert!(format_of("md").is_ok());
        let err = format_of("csv").unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("xlsx") && err.message.contains("md"), "点名两个合法值：{}", err.message);
    }
```

`lib.rs` 测试模块（照 `backup_export_wires_the_guard_before_any_write` 的形制）：

```rust
    #[test]
    fn ledger_export_wires_the_guard_before_any_write() {
        let src = include_str!("lib.rs");
        let start = src
            .find(concat!("fn ledger_", "export("))
            .expect("ledger_export 必须还在 lib.rs 里");
        let body = &src[start..start + src[start..].find("\n}").expect("函数体收尾")];
        let guard = concat!("backup::guard_", "destination(");
        let write = concat!("ledger_export::", "write(");
        assert_eq!(body.matches(guard).count(), 1, "命令层必须恰好调用一次落点守卫");
        let guard_at = body.find(guard).expect("守卫调用不见了");
        let write_at = body.find(write).expect("写盘调用不见了");
        assert!(guard_at < write_at, "守卫必须早于写盘：守卫在 {guard_at}、写盘在 {write_at}");
    }
```

- [ ] **Step 2: 跑它确认红**

Run: `cd src-tauri && cargo test -j 1 --lib format_of ledger_export_wires > /tmp/te5-red.log 2>&1; echo "exit=$?"; grep -E "^test |test result" /tmp/te5-red.log | head -6`
Expected：`exit=101`。**cargo 只接一个筛选词**：分两次跑（`--lib format_of` 与 `--lib ledger_export_wires`）。

- [ ] **Step 3: 实现 `write` 分派与回执**

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerExportSummary {
    pub path: String,
    pub sheets: usize,
    pub rows: usize,
    pub included_secrets: bool,
}

pub fn write(path: &str, format: &str, doc: &ExportDoc) -> AppResult<LedgerExportSummary> {
    format_of(format)?;
    let target = Path::new(path);
    match format {
        "xlsx" => write_xlsx(target, doc)?,
        _ => write_markdown(target, doc)?,
    }
    Ok(LedgerExportSummary {
        path: path.to_owned(),
        sheets: doc.sheets.len(),
        rows: doc.sheets.iter().map(|s| s.rows.len()).sum(),
        included_secrets: doc.included_secrets,
    })
}
```

- [ ] **Step 4: 实现命令并注册**

`lib.rs`（放在 `expiry_delete` 之后）：

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LedgerExportInput {
    project_id: String,
    format: String,
    include_secrets: bool,
    path: String,
}

/// 台账导出（M11-b spec D5/D7）：**先守卫、再取数、最后写盘**。
/// 顺序是承重的 —— `Write`/`File::create` 会 CREATE_ALWAYS 截断，落点指进数据目录会毁掉用户唯一的库。
#[tauri::command]
fn ledger_export(
    state: State<'_, AppState>,
    input: LedgerExportInput,
) -> AppResult<ledger_export::LedgerExportSummary> {
    ledger_export::format_of(&input.format)?;
    backup::guard_destination(&state.data_dir, std::path::Path::new(&input.path))?;
    let mk_guard = if input.include_secrets {
        Some(unlocked_mk(&state)?)
    } else {
        None
    };
    let mk = mk_guard.as_ref().and_then(|g| g.as_ref());
    let conn = db(&state)?;
    let doc = ledger_export::collect(&conn, mk, &input.project_id)?;
    ledger_export::write(&input.path, &input.format, &doc)
}
```

注册表：把 `expiry_delete`（**当前最后一条、无尾逗号**）改成带逗号，并在其后追加 `ledger_export`：

```rust
            expiry_delete,
            ledger_export
```

- [ ] **Step 5: 跑全套确认绿**

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/te5-green.log 2>&1; echo "exit=$?"; grep "test result" /tmp/te5-green.log; cd .. && echo "cmd=$(sed -n '/generate_handler/,/^\s*\])/p' src-tauri/src/lib.rs | grep -cE '^ +[a-z_]+,?$')"`
Expected：**333 passed / exit=0**；`cmd=65`。

- [ ] **Step 6: 提交并推**

```bash
git add src-tauri/src/ledger_export.rs src-tauri/src/lib.rs
git commit -m "feat(m11-b): ledger_export 命令（先守卫再取数最后写盘、格式只收 xlsx/md）—— 命令数 64→65，333 passed"
git push origin feat/m11b-ledger-export
```

---

### Task 6: 前端（默认名纯函数 + 导出组件 + 挂载）

**Files:**
- Create: `src/lib/ledger-export-name.ts`
- Create: `src/components/ledger-export.tsx`
- Create: `tests/ledger-export-name.test.ts`
- Modify: `src/lib/api.ts`（类型 + `exportLedger`）
- Modify: `src/pages/project-detail.tsx`（`LedgerPanel` 之后挂组件）

**Interfaces:**
- Consumes：`invoke`（api.ts）、`saveDialog`（plugin-dialog）、`toAppError`、`AppErrorShape`、`Button`/`Select`/`Dialog`/`Label`、Task 5 的命令 `ledger_export`。
- Produces：`defaultFileName(projectName, format, today)`；`<LedgerExport projectId={…} projectName={…} />`。

> **执行期修复轮（2026-10-22）**：评审 I-1 指出初版 `.slice(0, 60)` 按 UTF-16 码元截断，会把「59 汉字 + emoji」切成孤立高位代理（`JSON.stringify` → `serde_json` 报 `LoneLeadingSurrogateInHexEscape`，保存对话框的 invoke 失败、项目无法导出）。下面的 Step 1/3 已改成按码点截（`Array.from(...).slice(0, 60).join("")`）+ emoji 边界断言，并把 `\u0001` 控制字符样本并入第一条；node 累计 51→**52**。原实现提交 `da8aae4` 的 51 条读数只作历史。

- [ ] **Step 1: 写失败测试（node，5 条）**

`tests/ledger-export-name.test.ts`：

```ts
import test from "node:test";
import assert from "node:assert/strict";
import { defaultFileName } from "../src/lib/ledger-export-name.ts";

test("把 Windows 非法字符与控制字符替换成下划线", () => {
  assert.equal(
    defaultFileName('甲/乙:丙*丁?戊"己<庚>辛|壬\\癸\u0001子', "xlsx", "2026-10-21"),
    "台账-甲_乙_丙_丁_戊_己_庚_辛_壬_癸_子-2026-10-21.xlsx",
  );
});

test("去掉首尾空白与末尾的点", () => {
  assert.equal(defaultFileName("  政务云.  ", "md", "2026-10-21"), "台账-政务云-2026-10-21.md");
});

test("超长项目名截到 60 个字符，扩展名仍在", () => {
  const name = "长".repeat(80);
  const got = defaultFileName(name, "xlsx", "2026-10-21");
  assert.equal(got, `台账-${"长".repeat(60)}-2026-10-21.xlsx`);
});

test("截断按码点计，不切开 emoji 的代理对", () => {
  const got = defaultFileName("长".repeat(59) + "😀", "xlsx", "2026-10-21");
  assert.equal(got, `台账-${"长".repeat(59)}😀-2026-10-21.xlsx`);
  assert.equal(got.isWellFormed(), true);
});

test("空名回落到「项目」", () => {
  assert.equal(defaultFileName("   ", "md", "2026-10-21"), "台账-项目-2026-10-21.md");
});
```

- [ ] **Step 2: 跑它确认红**

Run: `node --test "tests/ledger-export-name.test.ts" > /tmp/te6-red.log 2>&1; echo "exit=$?"; grep -E "Cannot find module|tests|pass|fail" /tmp/te6-red.log | head -6`
Expected：`exit=1`，报找不到模块。

- [ ] **Step 3: 实现纯函数**

`src/lib/ledger-export-name.ts`：

```ts
// 台账导出的默认文件名（M11-b spec D8）。只有保存对话框之前需要这个名字，所以它活在前端；
// 后端只收最终 path，不复制同一套清洗规则（一份规则一处活）。
const ILLEGAL = /[\\/:*?"<>|\u0000-\u001F]/g;

export function defaultFileName(projectName: string, format: "xlsx" | "md", today: string): string {
  const stem = Array.from(
    projectName
      .replace(ILLEGAL, "_")
      .trim()
      .replace(/[. ]+$/, ""),
  )
    .slice(0, 60)
    .join("");
  return `台账-${stem || "项目"}-${today}.${format}`;
}
```

- [ ] **Step 4: 跑 node 全套确认绿**

Run: `node --test "tests/**/*.test.ts" > /tmp/te6-green.log 2>&1; echo "exit=$?"; grep -E "^ℹ (tests|pass|fail)" /tmp/te6-green.log`
Expected：**52/52/0**。

- [ ] **Step 5: 加 api 封装**

类型形状放 `src/types/ledger.ts` 末尾（本仓的线格式都住在 `src/types/`），封装放 `src/lib/api.ts`：

```ts
// src/types/ledger.ts 末尾
export interface LedgerExportInput {
  projectId: string;
  format: "xlsx" | "md";
  includeSecrets: boolean;
  path: string;
}

export interface LedgerExportSummary {
  path: string;
  sheets: number;
  rows: number;
  includedSecrets: boolean;
}
```

```ts
// src/lib/api.ts 末尾（M11-a 那段之后）
import type { LedgerExportInput, LedgerExportSummary } from "@/types/ledger"; // 与既有 ledger 类型合并进现有 import

// —— M11-b 台账导出 ——
export const exportLedger = (input: LedgerExportInput) =>
  invoke<LedgerExportSummary>("ledger_export", { input });
```

- [ ] **Step 6: 写组件并挂载**

`src/components/ledger-export.tsx`：

```tsx
// 台账导出（M11-b spec §七）：格式下拉 + 明文勾选 + 二次确认 + 保存对话框。
// 导出逻辑全在后端；这里只收集意图。勾了明文必须先过一次 in-app 确认 ——
// `window.confirm` 在 WebView2 下是原生窗口，脚本与自动化都点不到（HANDOFF 坑 12）。
import { useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { exportLedger } from "@/lib/api";
import { defaultFileName } from "@/lib/ledger-export-name";
import { toAppError, type AppErrorShape } from "@/lib/ipc";

type Format = "xlsx" | "md";

const todayIso = () => {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};

export function LedgerExport({ projectId, projectName }: { projectId: string; projectName: string }) {
  const [format, setFormat] = useState<Format>("xlsx");
  const [includeSecrets, setIncludeSecrets] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);
  const [ok, setOk] = useState<string | null>(null);

  const runExport = async () => {
    setConfirmOpen(false);
    setBusy(true);
    setError(null);
    setOk(null);
    try {
      const path = await saveDialog({
        defaultPath: defaultFileName(projectName, format, todayIso()),
        filters: [
          format === "xlsx"
            ? { name: "Excel 工作簿", extensions: ["xlsx"] }
            : { name: "Markdown", extensions: ["md"] },
        ],
      });
      if (path === null) return; // 取消不是错误（M4 的取消语义同一口径）
      const s = await exportLedger({ projectId, format, includeSecrets, path });
      setOk(`已导出 ${s.sheets} 张表 / 共 ${s.rows} 行${s.includedSecrets ? "（含明文）" : ""}`);
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mt-3 space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <Select value={format} onValueChange={(v) => setFormat(v as Format)}>
          <SelectTrigger className="w-32">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="xlsx">Excel</SelectItem>
            <SelectItem value="md">Markdown</SelectItem>
          </SelectContent>
        </Select>
        <Label className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <input
            type="checkbox"
            className="size-4 accent-primary"
            checked={includeSecrets}
            onChange={(e) => setIncludeSecrets(e.target.checked)}
          />
          包含账号与密码明文
        </Label>
        <Button
          size="sm"
          disabled={busy}
          onClick={() => (includeSecrets ? setConfirmOpen(true) : void runExport())}
        >
          导出台账
        </Button>
      </div>

      {error && (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}
      {ok && <p className="text-xs text-muted-foreground">{ok}</p>}

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>导出文件里会含账号与密码明文</DialogTitle>
            <DialogDescription>
              导出文件**不受主密码保护** —— 任何拿到这个文件的人都能直接看到明文。只在你确实需要时才继续。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setConfirmOpen(false)}>
              取消
            </Button>
            <Button variant="destructive" disabled={busy} onClick={() => void runExport()}>
              我明白，继续导出
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
```

`src/pages/project-detail.tsx`：import 区补 `import { LedgerExport } from "@/components/ledger-export";`；把 `LedgerPanel` 那段容器改成：

```tsx
      <div className="mt-4 rounded-xl border bg-background p-4">
        <LedgerPanel projectId={project.id} />
        <LedgerExport projectId={project.id} projectName={project.name} />
      </div>
```

- [ ] **Step 7: 跑编译门**

Run: `npm run build > /tmp/te6-build.log 2>&1; echo "build=$?"; grep -c "error TS" /tmp/te6-build.log; node --test "tests/**/*.test.ts" > /tmp/te6-node.log 2>&1; echo "node=$?"; grep -E "^ℹ (tests|pass|fail)" /tmp/te6-node.log; cd src-tauri && cargo test -j 1 --lib > /tmp/te6-cargo.log 2>&1; echo "cargo=$?"; grep "test result" /tmp/te6-cargo.log`
Expected：`build=0` 且 `error TS` **0**；node **52/52/0**；cargo **333 passed**。

- [ ] **Step 8: 提交并推**

```bash
git add src/lib/ledger-export-name.ts src/components/ledger-export.tsx tests/ledger-export-name.test.ts src/lib/api.ts src/pages/project-detail.tsx
git commit -m "feat(m11-b): 台账导出前端（默认名纯函数 + 格式/明文/二次确认/保存对话框）—— node 47→51、tsc 0 错"
git push origin feat/m11b-ledger-export
```

---

### Task 7: 全量门禁 + 七处变异 + 真机十一格（另两格是 cargo）

**Files:**
- Create: `.superpowers/sdd/2026-10-21-m11b-ledger-export/{mutation-evidence.txt, real-machine-readings.md, m11b-cdp.mjs, spec-plan-mapping.md}`
- Modify: 无产品代码。**本任务不许改实现**：变异跑完必须按字节还原。

> **执行方式**：与 M11-a Task 8 同口径 —— **门禁与变异由控制方自跑**（子代理一律不跑 `tauri dev`；变异要动产品文件，同 worktree 只允许一个会提交的人）。真机用一次性 identifier `dev.zero.pfm.m11btest` + CDP 端口 **9333**（9222 被 Qoder 自己占着），真实数据目录三文件前后 `size+mtime+sha256` 逐字一致，跑完 `rm -rf` 沙盒。

- [ ] **Step 1: 全量门禁（退出码逐个真取，绝不 `cmd | tail; echo $?`）**

```bash
cd src-tauri
cargo test -j 1 --lib > /tmp/b-gate-cargo.log 2>&1; echo "cargo=$?"
grep "test result" /tmp/b-gate-cargo.log
cargo clippy -j 1 --lib -- -D warnings > /tmp/b-gate-cl1.log 2>&1; echo "clippy-lib=$?"
cargo clippy -j 1 --all-targets --all-features -- -D warnings > /tmp/b-gate-cl2.log 2>&1; echo "clippy-all=$?"
cargo build -j 1 --lib > /tmp/b-gate-build.log 2>&1; echo "rust-build=$?"
cd ..
node --test "tests/**/*.test.ts" > /tmp/b-gate-node.log 2>&1; echo "node=$?"
grep -E "^ℹ (tests|pass|fail)" /tmp/b-gate-node.log
npm run build > /tmp/b-gate-web.log 2>&1; echo "web=$?"; grep -c "error TS" /tmp/b-gate-web.log
echo "cmd=$(sed -n '/generate_handler/,/^\s*\])/p' src-tauri/src/lib.rs | grep -cE '^ +[a-z_]+,?$') async=$(grep -c 'tauri::command(async)' src-tauri/src/lib.rs) dead=$(grep -rn 'allow(dead_code)' src-tauri/src/ | wc -l) pkg=$(grep -c '^\[\[package\]\]' src-tauri/Cargo.lock) collect=$(grep -c 'hits.append(&mut collect(' src-tauri/src/search.rs)"
grep -oE 'TARGET_VERSION: [a-z0-9]+ = [0-9]+' src-tauri/src/db.rs
git diff --stat master..HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock package.json
git rev-list --merges --count --all
git status --porcelain
```
Expected：`cargo=0` **333 passed**；两条 clippy `=0`；`rust-build=0`；`node=0` **52/52/0**；`web=0` 且 `error TS` **0**；结构门 **65 / 7 / `TARGET_VERSION: i64 = 7` / dead 1 / pkg 616 / collect 9**；依赖 diff **空**；merge **0**；`git status` 只剩 `?? Python/`（以及本任务证据目录，git-ignored）。
- [ ] **Step 2: 七处变异**（§十一 的 M①–M⑦，每处 `cp` 备份 + 具名打红 + `cmp` 还原；**跑绿就照实写「打不红」并补断言**）
- [ ] **Step 3–6: 真机 §九 的 13 格**（格 1/2/3/4/5/6/7/8/9/10/12 走真机；格 11 结构门、格 13 解密失败面是 cargo）
- [ ] **Step 7: 写 `spec-plan-mapping.md`**（spec §九 ↔ 计划格，任何未跑的格明写「未验 + 为什么」）

---

### Task 8: 活文档回写

**Files:**
- Modify: `docs/技术方案.md`（§四 补一句导出只读；§九 M11 行改成「M11-b 已兑（按项目导出）」）
- Modify: `docs/开发进度.md`（里程碑表 M11-b 行、`## M11-b 验收证据` 一节、变更日志、未验清单）
- Modify: `docs/HANDOFF.md`（快照头、§0 M11-b bullet、§3 下一格、§4-A 期望数、§6 指针）
- Modify: `docs/superpowers/specs/2026-10-19-m11-expiry-fields-design.md`（只在顶部加一行指针：M11-b 已兑）
- Test: 无

- [ ] **Step 1: `docs/技术方案.md` 两处**
  1. §四 数据模型那节补一句：台账导出是**只读**（不新增表、不改 `TARGET_VERSION`），导出内容 = `ledger_envs/credentials/servers/links/notes` 五表的明文列，密文列默认留空（D3）。
  2. §九 里程碑表 M11 行改成「M11-a + **M11-b 已兑**（按项目导出 Excel/Markdown，密文默认不导）；全局多项目导出与自定义列仍不做」。

- [ ] **Step 2: `docs/开发进度.md`**
  1. 里程碑表加 M11-b 行（状态按实写：合入前写「代码在功能分支」+ 现取 sha；合入后按收口规则回写合并点）。
  2. 新增一节「## M11-b 验收证据（台账导出，2026-10-2X）」，逐条写：终态门禁读数与退出码、七处变异各自红在哪（**具名，从 `mutation-evidence.txt` 抄**）、真机十二格读数（从 `real-machine-readings.md` 抄）、**「没能自动验证」清单**（至少含：导出按钮/二次确认/保存对话框的界面行为无自动化网；`window.confirm` 不可用所以用 in-app 对话框；Markdown 在第三方渲染器里的观感；大项目（数百行）的导出耗时没量）。
  3. 变更日志加一条（中文，写清本轮的评审轮次与裁定）。

- [ ] **Step 3: `docs/HANDOFF.md`**
  1. 快照头与 §0 加 M11-b bullet（**合并前不写合并点**，只写取数命令）。
  2. §3「当前卡在哪」与「下一格动作」改成 M11 整批收口后的候选增强（项目模板 / 父子项目 / 操作日志 / 多工作区，`思维导图.md:71-74`），并保留「只剩需求方能做的四格」。
  3. §4-A 那串命令的期望值换成 M11-b 终态（命令 **65**、cargo **333**、node **52**，其余不变）。
  4. §6 指针表补 M11-b spec / 计划 / 证据目录三行。

- [ ] **Step 4: `docs/superpowers/specs/2026-10-19-m11-expiry-fields-design.md` 顶部加一行指针**（M11-b 已兑，正文不 retro-edit）。

- [ ] **Step 5: 文档门复跑与提交**
```bash
grep -c "M11-b" docs/技术方案.md
grep -rn "待合并\|合并点 = " docs/HANDOFF.md | wc -l   # 合并前不应出现写死的合并点
git status --porcelain
```
Expected：第一条 ≥1；第二条 **0**（合并前）；`git status` 只剩 `?? Python/` 与本任务改的文档。

---

## 自审记录（控制方写完计划后现跑现数）

- **数链**：按各 Task 代码块里 `#[test]` 逐个计数 —— T1 3、T2 5、T3 2、T4 2、T5 2 = **14 支** ⇒ cargo 318+14+1=**333**（多出的 1 支是 Task 3 修复轮的长单元格边界测试）；node `tests/ledger-export-name.test.ts` **4 支** + 修复轮 emoji 边界 **1 支** ⇒ 47+5=**52**。T6 之后的 T7/T8 不新增测试。
- **spec 覆盖**：D1–D12 逐条落 Task（D1/D2 → T1 模型 + T3/T4 渲染器；D3 → T2；D4 → T1 测试 + T7 真机格 6/7；D5 → T5；D6 → T1–T5 全在后端；D7 → T5 守卫文本门；D8 → T6 纯函数；D9 → T4；D10 → T5 同步命令；D11/D12 → 全批无迁移无依赖）；§九 13 格 → T7。
- **类型一致性**：`collect` 的三处调用点（T1 测试、T2 测试、T5 命令）用同一签名 `(conn, Option<&MasterKey>, &str)`；`write` 的 `format: &str` 在 T5 命令与 T4 测试里一致；`SHEET_NAMES` 是 `[&str; 5]`，测试用 `.to_vec()` 比较。
- **已知风险（派发时要带）**：① T1/T2 的 RED 只到编译层，断言有牙靠变异补（本仓口径）；② `rust_xlsxwriter` 的 `set_name` 对工作表名有 31 字符上限 —— 本轮五个名字都在 3 字以内；③ 测试写临时文件用 `std::env::temp_dir()` 而不是 `/tmp`（Windows 原生版 Python/Node 看不见 MSYS 的 `/tmp`，Rust 里同理要走 `temp_dir`）。
