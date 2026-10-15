//! 待办清单：跨项目的日期形状判据与四操作（M9 Task 2，spec §五）。
//!
//! 判据收在纯函数里（`is_iso_date`/`norm_date`），命令层才有可测的落点（M8 终审 C1 教训③）；
//! `norm_date` 是 `pub(crate)` 给 T3 的 `milestone.rs` 共用 —— 一条日期形状规则只写一处（D7）。

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

#[derive(Debug, PartialEq, Serialize)]
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
        assert!(!t.done, "新行默认未完成");
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
