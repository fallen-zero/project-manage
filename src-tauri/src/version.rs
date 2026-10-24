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
