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
/// hint 用 spec D7 点名的那一句（终审 N9）：原先日期形状的拒绝把话全写在 message 里、
/// hint 还是泛泛的「检查表单必填项」⇒ 界面拼出来是「…（检查表单必填项）」，等于没说。
/// **只有这一支换 hint**，code 仍是 `invalid_input`（零新错误码，D7 的硬门）。
pub(crate) fn norm_date(column: &str, value: Option<String>) -> AppResult<Option<String>> {
    match norm(value) {
        None => Ok(None),
        Some(s) if is_iso_date(&s) => Ok(Some(s)),
        Some(s) => Err(AppError::new(
            "invalid_input",
            &format!("「{column}」要写成 YYYY-MM-DD（如 2026-06-30），收到「{s}」"),
            Some("日期要用 YYYY-MM-DD"),
        )),
    }
}

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

/// `?1 IS NULL OR t.project_id = ?1` 这一形是故意的：**只有一条 SQL**。
/// 拆成两条字符串再 `format!` 拼接，就会有人改了一条忘另一条（spec §十二 风险 3 的成因形状）。
/// `JOIN projects … AND p.deleted_at IS NULL` 是 D8 的兑现点：软删项目的待办读不到，但行留着。
/// `ORDER BY t.created_at, t.id` 里那个 `, t.id`（终审 N3，T2 就以 deferred minor 挂着的那一条）：
/// `datetime('now')` 只到秒，同秒建的两行光按 created_at 排等于全凭 rowid 的心情 ⇒ 补一个确定的 tiebreaker。
pub fn list(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<Todo>> {
    let workspace = crate::workspace::active_id(conn)?;
    let mut stmt = conn.prepare(
        &format!(
            "SELECT {SELECT_COLUMNS}
               FROM todos t
               JOIN projects p ON p.id = t.project_id AND p.deleted_at IS NULL
                                  AND p.workspace_id = ?2
              WHERE t.deleted_at IS NULL AND (?1 IS NULL OR t.project_id = ?1)
              ORDER BY t.created_at, t.id"
        ),
    )?;
    let rows = stmt.query_map(params![project_id, workspace], map_todo)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Todo> {
    crate::workspace::ensure_active_row(conn, "todos", id)?;
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
    crate::workspace::ensure_active_row(conn, "todos", id)?;
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
    crate::workspace::ensure_active_row(conn, "todos", id)?;
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
        // 「斜杠分隔」那一档（终审 N5，spec §八 判据 4 点名的形状）本机输出层会把 `/` 印成 `-`
        // ⇒ 别用眼睛确认，用 `[hex(ord(c))]` 复验码点是 U+002F。
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
            "2026/06/30",
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
        // 终审 N9：hint 必须是 D7 点名的那一句，不能继续吃泛泛的「检查表单必填项」。
        assert_eq!(
            err.hint.as_deref(),
            Some("日期要用 YYYY-MM-DD"),
            "日期形状这一支的 hint 要单独，实际：{:?}",
            err.hint
        );
        assert_eq!(norm_date("截止日期", Some("2026-06-30".to_owned())).unwrap().as_deref(), Some("2026-06-30"));
    }

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
        // 终审 N4：原先这里只断 code，没断错误文本点名是哪一列（milestone 侧断了）⇒ 两格现在同强度。
        assert!(err.message.contains("截止日期"), "错误文本要点名「截止日期」：{}", err.message);
        assert_eq!(err.hint.as_deref(), Some("日期要用 YYYY-MM-DD"), "终审 N9：hint 不再是「检查表单必填项」");
        let t = create(&conn, &input("p1", "乙", Some("2026-06-30"))).unwrap();
        assert_eq!(t.due_date.as_deref(), Some("2026-06-30"));
    }

    /// 终审 N3：`ORDER BY t.created_at` 原先没有**任何**可红的守卫，而 `datetime('now')` 只到秒
    /// ⇒ 同秒建的两行谁在前全凭 rowid。加了 `, t.id` 之后，这里用「插入顺序与 id 序故意相反」的三行钉死次序：
    /// **摘掉 `, t.id` 这条当场红**（读数见 `.superpowers/sdd/2026-10-14-m9-todos-milestones/mutation-evidence.txt`）。
    /// 直写 INSERT 而不是走 `create()`：uuid 主键给不出可控的 id 次序，同一秒的 `created_at` 也只能手写。
    #[test]
    fn list_orders_by_created_at_and_breaks_same_second_ties_by_id() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for (id, created) in [
            ("t-2", "2026-10-14 09:00:00"),
            ("t-1", "2026-10-14 09:00:00"),
            ("t-0", "2026-10-14 08:00:00"),
        ] {
            conn.execute(
                "INSERT INTO todos (id, project_id, title, created_at) VALUES (?1, 'p1', ?2, ?3)",
                params![id, id, created],
            )
            .unwrap();
        }
        let ids: Vec<String> = list(&conn, None).unwrap().into_iter().map(|t| t.id).collect();
        // 同上：断言用 `join`，别人复验这一格时只需要看一行、也少一处写盘形状风险。
        assert_eq!(ids.join(","), "t-0,t-1,t-2", "先按 created_at（t-0 最早），同秒内按 id 升序；插入顺序是 t-2、t-1、t-0");
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

    /// 待办的读与写都只认活动区（M15 spec §4.1 / §4.2）。
    #[test]
    fn todo_list_and_mutators_are_scoped() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p-scope");
        let t = create(
            &conn,
            &TodoInput {
                project_id: "p-scope".to_owned(),
                title: "写点东西".to_owned(),
                due_date: None,
            },
        )
        .unwrap();
        assert_eq!(list(&conn, None).unwrap().len(), 1, "活动区里的待办先要读得到");

        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        conn.execute(
            "UPDATE projects SET workspace_id = ?1 WHERE id = ?2",
            rusqlite::params![b.id, "p-scope"],
        )
        .unwrap();

        assert!(list(&conn, None).unwrap().is_empty(), "别区待办不许漏进当前区");
        assert!(list(&conn, Some("p-scope")).unwrap().is_empty());
        assert_eq!(get(&conn, &t.id).unwrap_err().code, "not_found");
        assert_eq!(
            update(
                &conn,
                &t.id,
                &TodoEdit { title: "改一下".to_owned(), due_date: None, done: false }
            )
            .unwrap_err()
            .code,
            "not_found"
        );
        assert_eq!(delete(&conn, &t.id).unwrap_err().code, "not_found");
        // create 的守卫：跨区 project_id 挂不上新行。
        assert_eq!(
            create(
                &conn,
                &TodoInput {
                    project_id: "p-scope".to_owned(),
                    title: "别区挂待办".to_owned(),
                    due_date: None,
                }
            )
            .unwrap_err()
            .code,
            "not_found"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM todos WHERE title = '别区挂待办'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0,
            "被拒的那条一条都不许多"
        );
    }
}
