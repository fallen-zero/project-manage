use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::{AppError, AppResult};
use crate::index_store::clear_project;

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
///
/// 改指新根 = 声明旧根那一套文件不再属于本项目，所以旧路径的索引行要跟着回收（终审 I3）：
/// `index_job::targets()` 只认登记中的 root 行，旧根一被替换就永远不再进入任何一轮 pass，
/// 于是 `clear_project`（唯一会双侧删的入口）对它不可达，旧根的文件会继续命中搜索。
/// 并发前提（不额外加锁）：worker 用的是自己那条连接（见 `index_job::start` 的头注释）。这里要分清两件事：
/// **撞锁**由 `db.rs:70` 配的 `busy_timeout = 5000` 兜住；**覆写**不由它兜 —— 作业运行中改指新根时，
/// 本轮 `run_pass` 手里还握着轮次开头那一次 `targets()` 快照，会在 `clear_project` 之后继续为旧根的文件写行，
/// 刚回收的行被写回来。即时那一半由这里的 `clear_project` 负责，被写回来的那一半由
/// `index_job::sweep_unrooted_projects` 在本轮末尾（最迟下一轮末尾）收敛；本轮不引入跨连接互斥。
pub fn set_root_dir(conn: &Connection, project_id: &str, input: &DirInput) -> AppResult<ProjectDir> {
    let path = validate_path(&input.path)?;
    get_project(conn, project_id)?;
    let old = conn
        .query_row(
            "SELECT path FROM project_dirs WHERE project_id = ?1 AND kind = 'root'",
            [project_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    conn.execute("DELETE FROM project_dirs WHERE project_id = ?1 AND kind = 'root'", [project_id])?;
    conn.execute(
        "INSERT INTO project_dirs (id, project_id, kind, path, label, sort) VALUES (?1,?2,'root',?3,?4,0)",
        params![uuid::Uuid::new_v4().to_string(), project_id, path, normalize(input.label.clone())],
    )?;
    // 只有「旧根存在且指向别处」才回收。同一路径重复登记不许清空索引：那会把一次
    // 无意义的登记重写升级成几十 GB 的全量重抽。
    if old.is_some_and(|old_path| old_path != path) {
        clear_project(conn, project_id)?;
    }
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

/// 注销一个登记目录。签名与返回类型都不变（`lib.rs` 的命令层与 M1 的既有测试都吃它）。
///
/// 注销**根目录**等于声明「这个项目不再包含它」，所以同步回收该项目的索引行（终审 I3）：
/// root 行没了，`index_job::targets()` 里就不再出现该项目，`run_pass` 永远不会再对它调
/// `clear_project`，旧根的 `index_docs` 与 FTS 行会永久停在 `ok` 并可被搜到 —— 而 `/index`
/// 页面既看不到它们也没有按钮能清。schema 层事后判断不了一行属于哪次登记
/// （`index_docs` 没有任何列指向 `project_dirs.id`），所以只能在成因这一侧收口。
/// `entry` 类目录从来不是索引作用域，删它不许动索引（`removing_an_entry_dir_keeps_the_index_rows` 钉住）。
/// 并发前提同 `set_root_dir`，而且必须说全：`db.rs:70` 的 `busy_timeout = 5000` 只兜住**撞锁**，
/// 兜不住**覆写** —— 作业运行中注销根目录，本轮 `run_pass` 仍握着轮次开头那一次 `targets()` 快照，
/// 会在 `clear_project` 之后继续为该项目的文件写行，把刚删掉的行写回来（而 root 行一没，
/// `targets()` 就不再列出该项目，重建循环此后每次也都够不着它）。
/// 这里负责即时回收那一半，被写回来的那一半由 `index_job::sweep_unrooted_projects` 在本轮末尾
/// （最迟下一轮末尾）收敛；软删项目的行不在 sweep 范围内，那是留给恢复的。
pub fn remove_dir(conn: &Connection, project_id: &str, dir_id: &str) -> AppResult<()> {
    // 先读 kind 再删：删掉之后就问不出这一行原本是 root 还是 entry 了。
    let kind = conn
        .query_row(
            "SELECT kind FROM project_dirs WHERE id = ?1 AND project_id = ?2",
            params![dir_id, project_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    // 读不到就按现有语义返回 not_found，行为不变（下面的 changed == 0 那条也原样留着）。
    if kind.is_none() {
        return Err(not_found(dir_id));
    }
    let changed = conn.execute(
        "DELETE FROM project_dirs WHERE id = ?1 AND project_id = ?2",
        params![dir_id, project_id],
    )?;
    if changed == 0 {
        return Err(not_found(dir_id));
    }
    if kind.as_deref() == Some("root") {
        clear_project(conn, project_id)?;
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

    // ---- 终审 I3：索引行的生命周期以登记的 root 行为作用域 ----
    // 两行都是测试专用，别提到模块顶层：`cargo clippy --lib` 不带 test cfg，顶层引入会被判 unused。
    use crate::index_scan::ScannedFile;
    use crate::index_store::{doc_hits, fts_orphan_rows, write_doc, DocOutcome};

    fn scanned_file(path: &str) -> ScannedFile {
        ScannedFile {
            path: path.to_owned(),
            file_name: path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned(),
            ext: "docx".to_owned(),
            size: 1024,
            mtime: 1_700_000_000,
        }
    }

    /// (该项目的 index_docs 行数, 全库 index_docs_fts 行数)。两侧都要断：
    /// FTS5 虚表没有外键，只数主表的话孤儿行照样能继续被 `SELECT_SQL` JOIN 出来（它是 JOIN 主表，
    /// 主表行没了就连不上，但体积与 bm25 统计会被长期污染且没人能清）。
    fn both_sides(conn: &Connection, project_id: &str) -> (i64, i64) {
        (
            conn.query_row("SELECT count(*) FROM index_docs WHERE project_id = ?1", [project_id], |r| r.get(0)).unwrap(),
            conn.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get(0)).unwrap(),
        )
    }

    /// 登记一个 root 并手写两行索引（其中带正文的那行同时产生 FTS 行）。
    /// 不需要真文件：`clear_project` 回收的是库里的行，作用域只看 project_id，本轮没有扫描发生。
    /// 返回 (project_id, root 的 dir_id, 用于「注销前搜得到」的正文关键词)。
    fn root_project_with_two_rows(conn: &Connection) -> (String, String, &'static str) {
        let p = create_project(conn, &input("注销根项目")).unwrap();
        let root = tempfile::tempdir().unwrap();
        set_root_dir(
            conn,
            &p.id,
            &DirInput { path: root.path().display().to_string(), label: None, sort: None },
        )
        .unwrap();
        let root_id: String = conn
            .query_row("SELECT id FROM project_dirs WHERE project_id = ?1 AND kind = 'root'", [&p.id], |r| r.get(0))
            .unwrap();
        let base = root.path().display().to_string();
        write_doc(conn, &p.id, &scanned_file(&format!("{base}\\验收说明.docx")), DocOutcome::Ok("甲方要求验收指标".into()))
            .unwrap();
        write_doc(conn, &p.id, &scanned_file(&format!("{base}\\维保期.docx")), DocOutcome::Skipped("too_large")).unwrap();
        (p.id, root_id, "验收")
    }

    /// 注销根目录 = 声明这个项目不再包含它，所以双侧回收索引行（终审 I3 的成因修法）。
    /// 少了 `remove_dir` 里的 `clear_project`：`targets()` 只认登记中的 root 行，该项目从此
    /// 不再进入任何一轮 pass，旧根的 ok 行会永久留在库里继续命中搜索，而 `/index` 页既看不到
    /// 也没有按钮能清 —— 这条断言就是那个「永久」的反面。
    #[test]
    fn removing_the_root_dir_drops_its_index_rows() {
        let conn = crate::db::open_in_memory().unwrap();
        let (pid, root_id, keyword) = root_project_with_two_rows(&conn);
        assert_eq!(both_sides(&conn, &pid), (2, 1), "前提：两行真在库里，其中一行带了正文");
        assert_eq!(doc_hits(&conn, keyword, 20).unwrap().len(), 1, "注销前必须搜得到，否则后面的「搜不到」是假绿");

        remove_dir(&conn, &pid, &root_id).unwrap();

        assert_eq!(both_sides(&conn, &pid), (0, 0), "注销根目录要双侧回收：index_docs 与 index_docs_fts");
        assert!(doc_hits(&conn, keyword, 20).unwrap().is_empty(), "旧根的正文不该继续命中搜索");
        assert!(get_project(&conn, &pid).unwrap().dirs.is_empty(), "登记行照旧被删掉");
        assert_eq!(fts_orphan_rows(&conn), 0, "回收不能留下对不上主表的虚表行（终审 m2 的全表不变量）");
    }

    /// 改指新根也要回收旧路径的行；同时钉住「同一路径重复登记不清空」这条判断不是顺手删掉的
    /// —— 少了 `old != path` 那个条件，用户每次重新选同一个目录都会触发几十 GB 的全量重抽。
    #[test]
    fn repointing_the_root_drops_rows_of_the_old_path() {
        let conn = crate::db::open_in_memory().unwrap();
        let (pid, _root_id, keyword) = root_project_with_two_rows(&conn);
        assert_eq!(both_sides(&conn, &pid), (2, 1), "前提：两行真在库里");

        let other = tempfile::tempdir().unwrap();
        let new_path = other.path().display().to_string();
        set_root_dir(&conn, &pid, &DirInput { path: new_path.clone(), label: None, sort: None }).unwrap();
        assert_eq!(both_sides(&conn, &pid), (0, 0), "改指新根要把旧路径的行双侧回收");
        assert!(doc_hits(&conn, keyword, 20).unwrap().is_empty(), "旧根的文件继续命中搜索就是 I3 的现场");

        // 同一路径再登记一次：行数一格都不许动。
        write_doc(&conn, &pid, &scanned_file(&format!("{new_path}\\新文件.docx")), DocOutcome::Ok("付款条件与验收流程".into()))
            .unwrap();
        let before = both_sides(&conn, &pid);
        assert_eq!(before, (1, 1), "前提：新根下确实有一行带正文");
        set_root_dir(&conn, &pid, &DirInput { path: new_path.clone(), label: Some("同一份".into()), sort: None }).unwrap();
        assert_eq!(both_sides(&conn, &pid), before, "同一根路径重复登记不许清空索引");
        assert_eq!(doc_hits(&conn, keyword, 20).unwrap().len(), 1, "重写后那一行仍要搜得到");
        assert_eq!(
            conn.query_row("SELECT count(*) FROM project_dirs WHERE project_id = ?1 AND kind = 'root'", [&pid], |r| r.get::<_, i64>(0)).unwrap(),
            1,
            "登记行还是一根，没被换成第二根"
        );
        assert_eq!(fts_orphan_rows(&conn), 0, "换指与重写都不该留下虚表孤儿行");
    }

    /// `entry` 类目录从来不是索引作用域（`targets()` 只挑 kind='root'），删它不许动索引。
    /// 这条防的是将来把条件写成「删任何目录都清」——那种写法会让一次普通的快捷入口调整
    /// 悄悄清掉整个项目的索引，而上一/下一条测试都测不到它。
    #[test]
    fn removing_an_entry_dir_keeps_the_index_rows() {
        let conn = crate::db::open_in_memory().unwrap();
        let p = create_project(&conn, &input("快捷入口项目")).unwrap();
        add_entry_dir(&conn, &p.id, &DirInput { path: "C:\\Windows".into(), label: None, sort: None }).unwrap();
        let entry_id: String = conn
            .query_row("SELECT id FROM project_dirs WHERE project_id = ?1 AND kind = 'entry'", [&p.id], |r| r.get(0))
            .unwrap();
        write_doc(&conn, &p.id, &scanned_file("C:/x/验收说明.docx"), DocOutcome::Ok("甲方要求验收指标".into())).unwrap();
        let before = both_sides(&conn, &p.id);
        assert_eq!(before, (1, 1), "前提：这个项目有一行带正文的索引");

        remove_dir(&conn, &p.id, &entry_id).unwrap();

        assert_eq!(both_sides(&conn, &p.id), before, "删 entry 目录不许清掉索引行");
        assert_eq!(doc_hits(&conn, "验收", 20).unwrap().len(), 1, "删 entry 之后正文照旧可搜");
    }
}
