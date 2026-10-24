//! 里程碑：阶段白名单与四操作（M9 Task 3，spec §五）。
//!
//! 日期形状判据复用 `todo::norm_date`（D7：一条日期规则只写一处）；
//! 五个阶段词在**应用层**判，SQL 的 CHECK 只留作直写库的最后闸门（spec §五 第 3 道）。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::todo::norm_date;

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查表单必填项"))
}

fn not_found(id: &str) -> AppError {
    AppError::new("not_found", &format!("记录不存在：{id}"), Some("刷新后重试"))
}

fn norm(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// 需求原文那五个词（`思维导图.md:35-36`）。与 `projects.status` 那八段是**两套词**（D4），
/// 别去复用 `project::STATUS_OPTIONS`，也别让一个下拉喂两个字段。
pub(crate) const STAGES: [&str; 5] = ["立项", "评审", "联调", "上线", "验收"];

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub id: String,
    pub project_id: String,
    pub stage: String,
    pub plan_date: Option<String>,
    pub note: String,
    pub done: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MilestoneInput {
    pub project_id: String,
    pub stage: String,
    pub plan_date: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MilestoneEdit {
    pub stage: String,
    pub plan_date: Option<String>,
    pub note: Option<String>,
    pub done: bool,
}

const SELECT_COLUMNS: &str =
    "m.id, m.project_id, m.stage, m.plan_date, m.note, m.done, m.created_at, m.updated_at";

fn map_milestone(row: &rusqlite::Row<'_>) -> rusqlite::Result<Milestone> {
    Ok(Milestone {
        id: row.get(0)?,
        project_id: row.get(1)?,
        stage: row.get(2)?,
        plan_date: row.get(3)?,
        note: row.get(4)?,
        done: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
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

/// 与 `todo::list` 同一条 SQL 形状：一条语句同时管「跨项目全部」与「按项目筛」，
/// 且 `p.deleted_at IS NULL` 在这张表上同样只有这一处（D8）。
/// `ORDER BY` 里那个 `, m.id`（终审 N3）：`datetime('now')` 只到秒，同日期同秒的两行必须有确定次序。
/// **与 spec §四 那句「同 `todo_list`」有偏差**（多排了 plan_date）：更合理但没授权，
/// 已按裁定在 spec §四 记一句，代码不改（N10）。
pub fn list(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<Milestone>> {
    let workspace = crate::workspace::active_id(conn)?;
    let mut stmt = conn.prepare(
        &format!(
            "SELECT {SELECT_COLUMNS}
               FROM milestones m
               JOIN projects p ON p.id = m.project_id AND p.deleted_at IS NULL
                                  AND p.workspace_id = ?2
              WHERE m.deleted_at IS NULL AND (?1 IS NULL OR m.project_id = ?1)
              ORDER BY m.plan_date IS NULL, m.plan_date, m.created_at, m.id"
        ),
    )?;
    let rows = stmt.query_map(params![project_id, workspace], map_milestone)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Milestone> {
    crate::workspace::ensure_active_row(conn, "milestones", id)?;
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM milestones m WHERE m.id = ?1 AND m.deleted_at IS NULL"
    );
    conn.query_row(&sql, [id], map_milestone)
        .optional()?
        .ok_or_else(|| not_found(id))
}

/// 白名单在**应用层**判：落到 SQLite 的 CHECK 会变成 `db_failed`，用户看不出是自己填错（spec §五 第 3 道）。
/// CHECK 仍然留着，它挡的是绕过命令直写库的那条路。
fn check_stage(stage: &str) -> AppResult<()> {
    if STAGES.contains(&stage) {
        Ok(())
    } else {
        Err(invalid(&format!(
            "阶段「{stage}」不在 {} 之内",
            STAGES.join("/")
        )))
    }
}

pub fn create(conn: &Connection, input: &MilestoneInput) -> AppResult<Milestone> {
    project_exists(conn, &input.project_id)?;
    check_stage(&input.stage)?;
    let plan_date = norm_date("计划日期", input.plan_date.clone())?;
    let note = norm(input.note.clone()).unwrap_or_default();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO milestones (id, project_id, stage, plan_date, note) VALUES (?1,?2,?3,?4,?5)",
        params![id, input.project_id, input.stage, plan_date, note],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, edit: &MilestoneEdit) -> AppResult<()> {
    crate::workspace::ensure_active_row(conn, "milestones", id)?;
    check_stage(&edit.stage)?;
    let plan_date = norm_date("计划日期", edit.plan_date.clone())?;
    let note = norm(edit.note.clone()).unwrap_or_default();
    let changed = conn.execute(
        "UPDATE milestones SET stage=?2, plan_date=?3, note=?4, done=?5, updated_at=datetime('now')
          WHERE id=?1 AND deleted_at IS NULL",
        params![id, edit.stage, plan_date, note, edit.done],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    crate::workspace::ensure_active_row(conn, "milestones", id)?;
    let changed = conn.execute(
        "UPDATE milestones SET deleted_at = datetime('now'), updated_at = datetime('now')
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
            "INSERT INTO projects (id, name, status) VALUES (?1, '里程碑测试项目', '立项')",
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

    fn input(project_id: &str, stage: &str, date: Option<&str>, note: Option<&str>) -> MilestoneInput {
        MilestoneInput {
            project_id: project_id.to_owned(),
            stage: stage.to_owned(),
            plan_date: date.map(str::to_owned),
            note: note.map(str::to_owned),
        }
    }

    fn edit(stage: &str, date: Option<&str>, note: Option<&str>, done: bool) -> MilestoneEdit {
        MilestoneEdit {
            stage: stage.to_owned(),
            plan_date: date.map(str::to_owned),
            note: note.map(str::to_owned),
            done,
        }
    }

    #[test]
    fn create_refuses_stage_words_outside_the_five_with_invalid_input() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for bad in ["初验", "需求", "开发", "归档", ""] {
            let err = create(&conn, &input("p1", bad, None, None)).unwrap_err();
            assert_eq!(err.code, "invalid_input", "「{bad}」要的是应用层那句，不是 db_failed");
        }
        for good in STAGES {
            create(&conn, &input("p1", good, None, None)).unwrap();
        }
        assert_eq!(list(&conn, Some("p1")).unwrap().len(), 5, "五段各建一行都要成");
    }

    #[test]
    fn create_rejects_non_iso_plan_date() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        // 左半条必须拿「形状就不对」的值去断：点分隔那种长度也是 10，但第 5、8 位不是 `-`，
        // 所以它测的是形状判据本身（拿合法日期断 unwrap_err 就是自己红，T2 踩过一次）。
        let err = create(&conn, &input("p1", "上线", Some("2026.10.14"), None)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("计划日期"), "错误文本要点名是哪一列：{}", err.message);
        // 终审 N9：norm_date 是两张表共用的那一份（D7），换了 hint 这里同步钉住。
        assert_eq!(err.hint.as_deref(), Some("日期要用 YYYY-MM-DD"), "hint 实际：{:?}", err.hint);
        let m = create(&conn, &input("p1", "上线", Some("2026-10-14"), None)).unwrap();
        assert_eq!(m.plan_date.as_deref(), Some("2026-10-14"));
    }

    /// 终审 N3：`ORDER BY plan_date IS NULL, plan_date, created_at` 三键全没有可红的守卫
    /// （`create_then_list_roundtrip_keeps_note_and_done` 只断到「有日期在无日期前」）。
    /// 这里用「同一天 + 同一秒 + 插入顺序与 id 序相反」的三行钉死 `, m.id`：**摘掉它当场红**。
    /// 直写 INSERT 而不走 `create()`：uuid 主键给不出可控的 id 次序。
    #[test]
    fn list_orders_dated_rows_first_then_undated_and_breaks_ties_by_id() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for (id, plan) in [("m-2", Some("2026-10-20")), ("m-1", Some("2026-10-20")), ("m-3", None)] {
            conn.execute(
                "INSERT INTO milestones (id, project_id, stage, plan_date, created_at)
                  VALUES (?1, 'p1', '上线', ?2, '2026-10-14 09:00:00')",
                params![id, plan],
            )
            .unwrap();
        }
        let ids: Vec<String> = list(&conn, None).unwrap().into_iter().map(|m| m.id).collect();
        // 断言用 `join` 而不是 `vec![...to_owned()...]`：本轮写盘把最后一个 `.to_owned()` 的括号折成了一个引号
        // （本机显示层也会把字节印成别的字符 ⇒ 这种形状少一处，复验只能按码点。）
        assert_eq!(ids.join(","), "m-1,m-2,m-3", "有日期的在前、同日期按 id 升序、空日期落最后；插入顺序是 m-2、m-1、m-3");
    }

    #[test]
    fn create_rejects_project_that_does_not_exist_or_is_deleted() {
        let conn = db::open_in_memory().unwrap();
        assert_eq!(create(&conn, &input("nope", "立项", None, None)).unwrap_err().code, "not_found");
        seed(&conn, "p-gone");
        soft_delete_project(&conn, "p-gone");
        assert_eq!(create(&conn, &input("p-gone", "立项", None, None)).unwrap_err().code, "not_found");
    }

    #[test]
    fn create_then_list_roundtrip_keeps_note_and_done() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let m = create(&conn, &input("p1", "评审", Some("2026-10-14"), Some("  专家意见已回  ")))
            .unwrap();
        assert_eq!(m.note, "专家意见已回", "写入要 trim");
        // `assert!(!..)` 而不是 `assert_eq!(.., false)`：后者撞 `clippy::bool_assert_comparison`
        // （-D warnings 下打成红，lib.rs 的 `only_post_overwrite_import_failures...` 记过同一坑），
        // 断言强度不变。
        assert!(!m.done, "新行默认未完成");
        let bare = create(&conn, &input("p1", "验收", None, None)).unwrap();
        assert_eq!(bare.note, "", "不填 note 就是空串，不是 NULL（列本身 NOT NULL DEFAULT ''）");
        assert_eq!(get(&conn, &m.id).unwrap(), m);
        // 本模块的 list 默认按 plan_date 升序、空日期落最后（前端三视图还要各排各的，这条只是稳定基线）
        let ids: Vec<String> = list(&conn, Some("p1")).unwrap().into_iter().map(|x| x.id).collect();
        assert_eq!(ids, vec![m.id.clone(), bare.id.clone()]);
        assert_eq!(list(&conn, None).unwrap().len(), 2);
    }

    #[test]
    fn list_hides_soft_deleted_milestones() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let m = create(&conn, &input("p1", "联调", None, None)).unwrap();
        delete(&conn, &m.id).unwrap();
        assert!(list(&conn, None).unwrap().is_empty());
        assert_eq!(get(&conn, &m.id).unwrap_err().code, "not_found");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM milestones WHERE deleted_at IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "软删：行要在库里");
    }

    /// D8 的兑现点，T7 变异 M① 在这一格的靶子：**摘掉 `AND p.deleted_at IS NULL` 这条当场红**。
    #[test]
    fn cross_project_list_hides_milestones_of_a_soft_deleted_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p-alive");
        seed(&conn, "p-dead");
        create(&conn, &input("p-alive", "上线", None, None)).unwrap();
        let dead = create(&conn, &input("p-dead", "评审", None, None)).unwrap();
        soft_delete_project(&conn, "p-dead");

        let stages: Vec<String> = list(&conn, None)
            .unwrap()
            .into_iter()
            .map(|m| m.stage)
            .collect();
        assert_eq!(stages, vec!["上线"], "软删项目的里程碑不能进跨项目列表（预警就是这条查询）");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM milestones WHERE id = ?1", [&dead.id], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "行留着，恢复项目即原样回来");
    }

    #[test]
    fn update_sets_done_without_touching_the_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        seed(&conn, "p2");
        let m = create(&conn, &input("p1", "立项", Some("2026-10-14"), Some("备查"))).unwrap();
        update(&conn, &m.id, &edit("评审", None, None, true)).unwrap();
        let after = get(&conn, &m.id).unwrap();
        assert_eq!(after.stage, "评审");
        assert_eq!(after.plan_date, None, "计划日期要能清回 NULL");
        assert_eq!(after.note, "", "note 传 None 就是清成空串");
        assert!(after.done, "D5 的兑现点：勾掉的历史里程碑不再进预警");
        assert_eq!(after.project_id, "p1", "update 改不了归属");
        update(&conn, &m.id, &edit("验收", None, None, false)).unwrap();
        assert_eq!(get(&conn, &m.id).unwrap().stage, "验收");
        assert_eq!(update(&conn, "no-such", &edit("上线", None, None, false)).unwrap_err().code, "not_found");
    }

    #[test]
    fn delete_is_soft_and_a_second_delete_reports_not_found() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let m = create(&conn, &input("p1", "验收", None, None)).unwrap();
        delete(&conn, &m.id).unwrap();
        assert_eq!(delete(&conn, &m.id).unwrap_err().code, "not_found");
    }

    /// 线格式钉键数：八键（spec §四）。少一个键 = 前端读 undefined，多一个键 = 白送。
    #[test]
    fn milestone_line_format_has_exactly_eight_keys() {
        let m = Milestone {
            id: "m1".to_owned(),
            project_id: "p1".to_owned(),
            stage: "立项".to_owned(),
            plan_date: Some("2026-10-14".to_owned()),
            note: "备查".to_owned(),
            done: false,
            created_at: "2026-10-14 00:00:00".to_owned(),
            updated_at: "2026-10-14 00:00:00".to_owned(),
        };
        let obj = serde_json::to_value(&m).unwrap();
        let mut keys: Vec<&str> = obj.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "createdAt",
                "done",
                "id",
                "note",
                "planDate",
                "projectId",
                "stage",
                "updatedAt"
            ],
            "Milestone 的键集合变了就要同步 src/types/milestone.ts"
        );
    }

    /// 里程碑的读与写都只认活动区（M15 spec §4.1 / §4.2）。
    #[test]
    fn milestone_list_and_mutators_are_scoped() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p-scope");
        let m = create(&conn, &input("p-scope", "立项", Some("2026-11-30"), None)).unwrap();
        assert_eq!(list(&conn, None).unwrap().len(), 1, "活动区里的里程碑先要读得到");

        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        conn.execute(
            "UPDATE projects SET workspace_id = ?1 WHERE id = ?2",
            params![b.id, "p-scope"],
        )
        .unwrap();

        assert!(list(&conn, None).unwrap().is_empty(), "别区里程碑不许漏进当前区");
        assert!(list(&conn, Some("p-scope")).unwrap().is_empty());
        assert_eq!(get(&conn, &m.id).unwrap_err().code, "not_found");
        assert_eq!(
            update(&conn, &m.id, &edit("上线", Some("2026-11-30"), None, false)).unwrap_err().code,
            "not_found"
        );
        assert_eq!(delete(&conn, &m.id).unwrap_err().code, "not_found");
        // create 的守卫：跨区 project_id 挂不上新行（阶段用白名单里的词，不让 invalid_input 抢答）。
        assert_eq!(
            create(&conn, &input("p-scope", "评审", Some("2026-12-31"), None)).unwrap_err().code,
            "not_found"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM milestones WHERE stage = '评审'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0,
            "被拒的那条一条都不许多"
        );
    }
}
