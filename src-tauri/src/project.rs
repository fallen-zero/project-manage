use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::{AppError, AppResult};

/// 全局统一的项目状态枚举（需求方确认：状态全局统一，不按项目自定义阶段模板）。
pub const STATUSES: [&str; 8] = [
    "立项", "需求", "开发", "测试", "预发", "上线", "运维", "归档",
];

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
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
    pub dirs: Vec<ProjectDir>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDir {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub label: Option<String>,
    pub sort: i64,
    /// 登记的路径当下是否还在磁盘上。仅用于提示，不作为写入前提——
    /// 可移动磁盘与未挂载的盘符本来就时常不可达，硬校验会把正常登记挡掉。
    pub exists: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInput {
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirInput {
    pub path: String,
    pub label: Option<String>,
    pub sort: Option<i64>,
}

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查表单必填项与取值范围"))
}

fn normalize(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

fn validate(input: &ProjectInput) -> AppResult<()> {
    if input.name.trim().is_empty() {
        return Err(invalid("项目名称不能为空"));
    }
    if !STATUSES.contains(&input.status.as_str()) {
        return Err(invalid(&format!("项目状态「{}」不在枚举内", input.status)));
    }
    Ok(())
}

/// 只登记路径，绝不创建、移动或改写目标；因此要求绝对路径，
/// 否则相对路径会随工作目录变化指向不同位置，索引器与「一键打开」都会指错。
fn validate_path(path: &str) -> AppResult<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(invalid("目录路径不能为空"));
    }
    if !Path::new(trimmed).is_absolute() {
        return Err(invalid(&format!(
            "「{trimmed}」不是绝对路径，需要形如 D:\\项目\\客户A 的完整路径"
        )));
    }
    Ok(trimmed.to_owned())
}

fn path_exists(path: &str) -> bool {
    Path::new(path).exists()
}

pub fn create_project(conn: &Connection, input: &ProjectInput) -> AppResult<Project> {
    validate(input)?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO projects (id, name, code, manager, customer, contact_name, contact_phone,
                               contract_no, contract_period, delivery_deadline, status)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
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
        ],
    )?;
    replace_tags(conn, &id, &input.tags)?;
    get_project(conn, &id)
}

pub fn update_project(conn: &Connection, id: &str, input: &ProjectInput) -> AppResult<Project> {
    validate(input)?;
    let changed = conn.execute(
        "UPDATE projects SET name=?2, code=?3, manager=?4, customer=?5, contact_name=?6,
                contact_phone=?7, contract_no=?8, contract_period=?9, delivery_deadline=?10,
                status=?11, updated_at=datetime('now')
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
        ],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    replace_tags(conn, id, &input.tags)?;
    get_project(conn, id)
}

/// 软删除：数据模型带 deleted_at 是「先单人、预留多人」的唯一代价，
/// 保留行才能在未来同步时区分「删除」与「本机从未有过」。
pub fn delete_project(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE projects SET deleted_at = datetime('now'), updated_at = datetime('now')
         WHERE id = ?1 AND deleted_at IS NULL",
        [id],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

pub fn list_projects(conn: &Connection) -> AppResult<Vec<Project>> {
    let mut rows = conn.prepare(
        "SELECT id, name, code, manager, customer, contact_name, contact_phone,
                contract_no, contract_period, delivery_deadline, status
         FROM projects WHERE deleted_at IS NULL ORDER BY updated_at DESC",
    )?;
    let mut projects: Vec<Project> = rows
        .query_map([], |r| {
            Ok(Project {
                id: r.get(0)?,
                name: r.get(1)?,
                code: r.get(2)?,
                manager: r.get(3)?,
                customer: r.get(4)?,
                contact_name: r.get(5)?,
                contact_phone: r.get(6)?,
                contract_no: r.get(7)?,
                contract_period: r.get(8)?,
                delivery_deadline: r.get(9)?,
                status: r.get(10)?,
                tags: Vec::new(),
                dirs: Vec::new(),
            })
        })?
        .collect::<Result<_, _>>()?;
    drop(rows);

    // 三条 SQL 装完整个列表，避免逐项目查标签与目录的 N+1
    let mut tag_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut stmt = conn.prepare("SELECT project_id, tag FROM project_tags ORDER BY tag")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (pid, tag) = row?;
        tag_map.entry(pid).or_default().push(tag);
    }
    drop(stmt);

    let mut dir_map: HashMap<String, Vec<ProjectDir>> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT id, project_id, kind, path, label, sort FROM project_dirs ORDER BY kind DESC, sort, created_at",
    )?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(1)?,
            ProjectDir {
                id: r.get(0)?,
                kind: r.get(2)?,
                path: r.get(3)?,
                label: r.get(4)?,
                sort: r.get(5)?,
                exists: false,
            },
        ))
    })? {
        let (pid, mut dir) = row?;
        dir.exists = path_exists(&dir.path);
        dir_map.entry(pid).or_default().push(dir);
    }
    drop(stmt);

    for p in &mut projects {
        if let Some(tags) = tag_map.remove(&p.id) {
            p.tags = tags;
        }
        if let Some(dirs) = dir_map.remove(&p.id) {
            p.dirs = dirs;
        }
    }
    Ok(projects)
}

pub fn get_project(conn: &Connection, id: &str) -> AppResult<Project> {
    let base = conn
        .query_row(
            "SELECT name, code, manager, customer, contact_name, contact_phone,
                    contract_no, contract_period, delivery_deadline, status
             FROM projects WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, String>(9)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| not_found(id))?;

    let tags = query_strings(
        conn,
        "SELECT tag FROM project_tags WHERE project_id = ?1 ORDER BY tag",
        id,
    )?;
    let dirs = query_dirs(conn, id)?;

    Ok(Project {
        id: id.to_owned(),
        name: base.0,
        code: base.1,
        manager: base.2,
        customer: base.3,
        contact_name: base.4,
        contact_phone: base.5,
        contract_no: base.6,
        contract_period: base.7,
        delivery_deadline: base.8,
        status: base.9,
        tags,
        dirs,
    })
}

fn query_strings(conn: &Connection, sql: &str, id: &str) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    let out = stmt
        .query_map([id], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    Ok(out)
}

fn query_dirs(conn: &Connection, id: &str) -> AppResult<Vec<ProjectDir>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, path, label, sort FROM project_dirs
         WHERE project_id = ?1 ORDER BY kind DESC, sort, created_at",
    )?;
    let out = stmt
        .query_map([id], |r| {
            Ok(ProjectDir {
                id: r.get(0)?,
                kind: r.get(1)?,
                path: r.get(2)?,
                label: r.get(3)?,
                sort: r.get(4)?,
                exists: false,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    let mut out = out;
    for d in &mut out {
        d.exists = path_exists(&d.path);
    }
    Ok(out)
}

fn replace_tags(conn: &Connection, project_id: &str, tags: &[String]) -> AppResult<()> {
    conn.execute("DELETE FROM project_tags WHERE project_id = ?1", [project_id])?;
    for tag in tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        conn.execute(
            "INSERT OR IGNORE INTO project_tags (id, project_id, tag) VALUES (?1,?2,?3)",
            params![uuid::Uuid::new_v4().to_string(), project_id, tag],
        )?;
    }
    Ok(())
}

/// 设置或替换主根目录。一个项目只有一个 root，由部分唯一索引保证。
pub fn set_root_dir(conn: &Connection, project_id: &str, input: &DirInput) -> AppResult<ProjectDir> {
    let path = validate_path(&input.path)?;
    get_project(conn, project_id)?;
    conn.execute("DELETE FROM project_dirs WHERE project_id = ?1 AND kind = 'root'", [project_id])?;
    conn.execute(
        "INSERT INTO project_dirs (id, project_id, kind, path, label, sort) VALUES (?1,?2,'root',?3,?4,0)",
        params![uuid::Uuid::new_v4().to_string(), project_id, path, normalize(input.label.clone())],
    )?;
    find_dir(conn, project_id, &path)
}

pub fn add_entry_dir(conn: &Connection, project_id: &str, input: &DirInput) -> AppResult<ProjectDir> {
    let path = validate_path(&input.path)?;
    get_project(conn, project_id)?;
    conn.execute(
        "INSERT INTO project_dirs (id, project_id, kind, path, label, sort) VALUES (?1,?2,'entry',?3,?4,?5)",
        params![
            uuid::Uuid::new_v4().to_string(),
            project_id,
            path,
            normalize(input.label.clone()),
            input.sort.unwrap_or(0)
        ],
    )?;
    find_dir(conn, project_id, &path)
}

pub fn remove_dir(conn: &Connection, project_id: &str, dir_id: &str) -> AppResult<()> {
    let changed = conn.execute(
        "DELETE FROM project_dirs WHERE id = ?1 AND project_id = ?2",
        params![dir_id, project_id],
    )?;
    if changed == 0 {
        return Err(not_found(dir_id));
    }
    Ok(())
}

fn find_dir(conn: &Connection, project_id: &str, path: &str) -> AppResult<ProjectDir> {
    query_dirs(conn, project_id)?
        .into_iter()
        .find(|d| d.path == path)
        .ok_or_else(|| not_found(path))
}

fn not_found(id: &str) -> AppError {
    AppError::new(
        "not_found",
        &format!("记录不存在：{id}"),
        Some("记录可能已被删除，刷新后重试"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str) -> ProjectInput {
        ProjectInput {
            name: name.into(),
            code: Some(" P-01 ".into()),
            manager: Some("张三".into()),
            customer: Some("某客户".into()),
            contact_name: None,
            contact_phone: Some("".into()),
            contract_no: None,
            contract_period: None,
            delivery_deadline: None,
            status: "开发".into(),
            tags: vec!["政务".into(), " 政务 ".into(), "".into()],
        }
    }

    #[test]
    fn create_then_read_roundtrip_trims_and_dedups() {
        let conn = crate::db::open_in_memory().unwrap();
        let created = create_project(&conn, &input("客户A项目")).unwrap();
        let read = get_project(&conn, &created.id).unwrap();

        assert_eq!(read.name, "客户A项目");
        assert_eq!(read.code.as_deref(), Some("P-01"), "首尾空格应被裁掉");
        assert_eq!(read.contact_phone, None, "空串归一为 NULL");
        assert_eq!(read.tags, vec!["政务"], "标签去空格且去重");
        assert_eq!(read.status, "开发");
    }

    #[test]
    fn empty_name_and_bad_status_are_rejected() {
        let conn = crate::db::open_in_memory().unwrap();
        assert_eq!(create_project(&conn, &input("   ")).unwrap_err().code, "invalid_input");

        let mut bad = input("正常项目");
        bad.status = "已上线".into();
        assert_eq!(create_project(&conn, &bad).unwrap_err().code, "invalid_input");
    }

    #[test]
    fn soft_delete_hides_from_all_reads() {
        let conn = crate::db::open_in_memory().unwrap();
        let p = create_project(&conn, &input("待删项目")).unwrap();
        delete_project(&conn, &p.id).unwrap();

        assert!(list_projects(&conn).unwrap().is_empty());
        assert_eq!(get_project(&conn, &p.id).unwrap_err().code, "not_found");
        // 行仍在表里，这是「预留多人」时要能区分删除与从未存在的前提
        let live: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects WHERE id = ?1", [&p.id], |r| r.get(0))
            .unwrap();
        assert_eq!(live, 1);
    }

    #[test]
    fn relative_paths_are_rejected() {
        let conn = crate::db::open_in_memory().unwrap();
        let p = create_project(&conn, &input("路径校验项目")).unwrap();
        let err = set_root_dir(
            &conn,
            &p.id,
            &DirInput { path: "项目/资料".into(), label: None, sort: None },
        )
        .unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("不是绝对路径"));
    }

    #[test]
    fn root_dir_is_replaced_not_duplicated() {
        let conn = crate::db::open_in_memory().unwrap();
        let p = create_project(&conn, &input("根目录项目")).unwrap();
        let dir = std::env::temp_dir();
        set_root_dir(
            &conn,
            &p.id,
            &DirInput { path: dir.display().to_string(), label: None, sort: None },
        )
        .unwrap();
        let again = DirInput {
            path: "C:\\Windows".into(),
            label: Some("系统".into()),
            sort: None,
        };
        set_root_dir(&conn, &p.id, &again).unwrap();

        let read = get_project(&conn, &p.id).unwrap();
        let roots: Vec<_> = read.dirs.iter().filter(|d| d.kind == "root").collect();
        assert_eq!(roots.len(), 1, "一个项目只能有一个主根目录");
        assert_eq!(roots[0].path, "C:\\Windows");
        assert!(roots[0].exists, "已存在的路径要标记为可达");
    }

    #[test]
    fn missing_path_is_reported_as_unreachable_but_still_stored() {
        let conn = crate::db::open_in_memory().unwrap();
        let p = create_project(&conn, &input("失联路径项目")).unwrap();
        add_entry_dir(
            &conn,
            &p.id,
            &DirInput { path: "Z:\\根本不存在\\的盘".into(), label: None, sort: None },
        )
        .unwrap();

        let d = &get_project(&conn, &p.id).unwrap().dirs[0];
        assert!(!d.exists, "路径不可达要能被 UI 提示，但不能拒绝登记");
    }

    #[test]
    fn list_avoids_n_plus_1_and_keeps_dirs_grouped() {
        let conn = crate::db::open_in_memory().unwrap();
        let a = create_project(&conn, &input("甲项目")).unwrap();
        let b = create_project(&conn, &input("乙项目")).unwrap();
        add_entry_dir(&conn, &a.id, &DirInput { path: "C:\\Windows".into(), label: None, sort: None })
            .unwrap();

        let list = list_projects(&conn).unwrap();
        assert_eq!(list.len(), 2);
        let a = list.iter().find(|x| x.id == a.id).unwrap();
        assert_eq!(a.dirs.len(), 1);
        assert_eq!(list.iter().find(|x| x.id == b.id).unwrap().dirs.len(), 0);
    }
}
