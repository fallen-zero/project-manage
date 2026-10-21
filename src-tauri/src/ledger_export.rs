//! 台账导出（M11-b spec）：把五张台账表摊成「导出行模型」，再由两个渲染器写成 xlsx / Markdown。
//!
//! 红线（spec D3）：**默认路径不读任何密文列** —— `mk` 为 `None` 时下面的 SQL 里根本不出现
//! `*_cipher` / `*_nonce`；只有调用方显式传 `Some(mk)` 才解密（Task 2）。变异 M② 打的就是这条。
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::vault::MasterKey;

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查导出参数"))
}

fn not_found(id: &str) -> AppError {
    AppError::new("not_found", &format!("记录不存在：{id}"), Some("刷新后重试"))
}

// 两个渲染器（Task 3/4）落地前本模块整体是 pub 的，故这里先取同样可见性：
// 等 `write` 命令接上、渲染器真的调用它之后可以收回 `fn`。
pub fn write_fail(path: &Path, e: impl std::fmt::Display) -> AppError {
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
          ORDER BY c.created_at",
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
