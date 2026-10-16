//! 日报管理：跨项目的工时条目（M10，spec §五）。四操作与判据在这里；
//! Excel 双向（Task 6/7）的两个私有函数同在本文件，共用 `EXPORT_HEADERS` 与日期/工时判据。
//!
//! 日期形状判据**不重写一份**：走 `crate::todo::req_date`（先例：`milestone.rs:10` 用 `todo::norm_date`）。
//! `hours` 的界与精度也先于 SQL 拒掉：CHECK 违例冒出来是 db_failed，会把校验失败伪装成库故障。

use std::path::Path;

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

/// 单条工时上限。不是「一天不能超 24 小时」的会计规则（多条之和本来可以超），
/// 而是挡住手滑多打一个 0 的那一格。前端 `types/daily-report.ts` 有同名常量，两处各自钉值。
pub(crate) const MAX_HOURS_PER_ROW: f64 = 24.0;

/// 工时的界与精度（先于 SQL，理由同 `milestone::check_stage`）。
/// 两位小数用「放大 100 倍后离整数有多远」判，容差 `1e-9`：`0.1 * 100.0` 落在 `10.000000000000002`，
/// 差 `1.8e-15` 收；`7.535 * 100.0` 离最近整数差 `0.5` 拒。**别改成 `h.fract()`**，那条对 0.1 也判不出。
pub(crate) fn check_hours(h: f64) -> AppResult<f64> {
    let scaled = h * 100.0;
    if !h.is_finite() || h <= 0.0 || h > MAX_HOURS_PER_ROW || (scaled - scaled.round()).abs() > 1e-9 {
        return Err(AppError::new(
            "invalid_input",
            &format!("「工时」要大于 0、不超过 {MAX_HOURS_PER_ROW}、最多两位小数，收到 {h}"),
            Some("工时按小时数填，如 7.5"),
        ));
    }
    Ok(h)
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReport {
    pub id: String,
    pub project_id: String,
    pub report_date: String,
    pub content: String,
    pub hours: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReportInput {
    pub project_id: String,
    /// 收 `Option<String>` 是为了让「缺键」与「空白」走同一句点名错误（`req_date`）。
    pub report_date: Option<String>,
    pub content: String,
    pub hours: f64,
}

/// update 的入参**含** `project_id`：改归属在本轮做（spec §五.4，与 M9 的 `TodoEdit` 不同）。
/// 理由是项目归属是日报最容易填错的字段，且导入按编号/名称匹配、项目改名后要把导错的行挪走。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReportEdit {
    pub project_id: String,
    pub report_date: Option<String>,
    pub content: String,
    pub hours: f64,
}

const SELECT_COLUMNS: &str =
    "d.id, d.project_id, d.report_date, d.content, d.hours, d.created_at, d.updated_at";

fn map_report(row: &rusqlite::Row<'_>) -> rusqlite::Result<DailyReport> {
    Ok(DailyReport {
        id: row.get(0)?,
        project_id: row.get(1)?,
        report_date: row.get(2)?,
        content: row.get(3)?,
        hours: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

/// 每个模块各留一份是本仓现形制（`todo.rs:101`、`milestone.rs:75` 已是两份），这里是第三份。
/// 不趁本轮顺手合并：合并要动那两格的既有测试，跟本轮目标无关，记进 Task 9 的欠账。
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

/// **SQL 里出现任何按项目状态的过滤都算缺陷**（spec §五.3 / D6）：归档项目的日报照常列、照常算。
/// 与 M9 判据⑤（预警跳过归档）形状相反，是有意为之。变异 M① 打的就是这一格。
/// `?1 IS NULL OR d.project_id = ?1` 一形照抄 `todo::list`：**只一条 SQL**，不拆两条拼接。
pub fn list(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<DailyReport>> {
    let mut stmt = conn.prepare(
        &format!(
            "SELECT {SELECT_COLUMNS}
               FROM daily_reports d
               JOIN projects p ON p.id = d.project_id AND p.deleted_at IS NULL
              WHERE d.deleted_at IS NULL AND (?1 IS NULL OR d.project_id = ?1)
              ORDER BY d.report_date, d.created_at, d.id"
        ),
    )?;
    let rows = stmt.query_map(params![project_id], map_report)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<DailyReport> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM daily_reports d WHERE d.id = ?1 AND d.deleted_at IS NULL"
    );
    conn.query_row(&sql, [id], map_report)
        .optional()?
        .ok_or_else(|| not_found(id))
}

pub fn create(conn: &Connection, input: &DailyReportInput) -> AppResult<DailyReport> {
    project_exists(conn, &input.project_id)?;
    let report_date = req_date("日报日期", input.report_date.clone())?;
    let content = input.content.trim().to_owned();
    if content.is_empty() {
        return Err(invalid("日报内容不能为空"));
    }
    let hours = check_hours(input.hours)?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO daily_reports (id, project_id, report_date, content, hours) VALUES (?1,?2,?3,?4,?5)",
        params![id, input.project_id, report_date, content, hours],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, edit: &DailyReportEdit) -> AppResult<()> {
    project_exists(conn, &edit.project_id)?;
    let report_date = req_date("日报日期", edit.report_date.clone())?;
    let content = edit.content.trim().to_owned();
    if content.is_empty() {
        return Err(invalid("日报内容不能为空"));
    }
    let hours = check_hours(edit.hours)?;
    let changed = conn.execute(
        "UPDATE daily_reports SET project_id=?2, report_date=?3, content=?4, hours=?5, updated_at=datetime('now')
          WHERE id=?1 AND deleted_at IS NULL",
        params![id, edit.project_id, report_date, content, hours],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE daily_reports SET deleted_at = datetime('now'), updated_at = datetime('now')
          WHERE id = ?1 AND deleted_at IS NULL",
        [id],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

/// 表头五列 —— 导出写它、导入按它定位列，**全仓只有这一处**（spec §六）。改一个词会同时打掉两格的
/// 测试：Task 6 的表头断言与 Task 7 的列定位断言读的都是这个常量。
pub const EXPORT_HEADERS: [&str; 5] = ["日期", "项目编号", "所属项目", "工作内容", "时长（小时）"];

/// 前端把**当前筛选后的行**传回来（spec §六.1：命令不收查询条件，免得在 SQL 里再造第二份日期比较）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRow {
    pub report_date: String,
    /// `projects.code` 是可空列，原样给出；导入侧按它优先匹配，为空则退到名称。
    pub project_code: Option<String>,
    /// **必须是 `projects.name` 原值**，不是界面上的 `projectLabel`（后者拼了编号就对不上）。
    pub project_name: String,
    pub content: String,
    pub hours: f64,
}

/// 写 xlsx 的失败按文件域报：码与文案对齐 `error.rs:21` 的 `AppError::io`。
/// 不直接调 `io()` 是因为 `rust_xlsxwriter` 的错误不是 `std::io::Error`，硬套会丢掉它自己那句话。
fn write_fail(path: &Path, e: impl std::fmt::Display) -> AppError {
    AppError::new(
        "fs_failed",
        &format!("写 {} 失败：{e}", path.display()),
        Some("确认该路径存在且可读写"),
    )
}

/// 写一份真 `.xlsx`。0 行数据也写出表头 —— 那就是「下载模板」。返回落地的数据行数。
pub fn write_xlsx(path: &Path, rows: &[ExportRow]) -> AppResult<usize> {
    use rust_xlsxwriter::{Format, Workbook};

    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("日报").map_err(|e| write_fail(path, e))?;
    ws.set_freeze_panes(1, 0).map_err(|e| write_fail(path, e))?;
    let bold = Format::new().set_bold();
    for (col, head) in EXPORT_HEADERS.iter().enumerate() {
        ws.write_with_format(0, col as u16, *head, &bold)
            .map_err(|e| write_fail(path, e))?;
    }
    for (i, r) in rows.iter().enumerate() {
        let row = (i + 1) as u32;
        ws.write(row, 0, r.report_date.as_str()).map_err(|e| write_fail(path, e))?;
        ws.write(row, 1, r.project_code.as_deref().unwrap_or(""))
            .map_err(|e| write_fail(path, e))?;
        ws.write(row, 2, r.project_name.as_str()).map_err(|e| write_fail(path, e))?;
        ws.write(row, 3, r.content.as_str()).map_err(|e| write_fail(path, e))?;
        // 工时写成数字（用户在 Excel 里要能求和），日期写成文本（上面那行）—— 两种类型是故意的，
        // 别顺手把日期也写成日期单元。
        ws.write(row, 4, r.hours).map_err(|e| write_fail(path, e))?;
    }
    wb.save(path).map_err(|e| write_fail(path, e))?;
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::params;

    fn seed(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, status) VALUES (?1, '日报测试项目', '立项')",
            [id],
        )
        .unwrap();
    }

    /// D6 的靶子夹具：归档状态取自 `projects` 的 8 段枚举（`db.rs` V2），词表外的值会被 CHECK 挡。
    fn seed_archived(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, status) VALUES (?1, '已归档项目', '归档')",
            [id],
        )
        .unwrap();
    }

    fn soft_delete_project(conn: &Connection, id: &str) {
        conn.execute("UPDATE projects SET deleted_at = datetime('now') WHERE id = ?1", [id])
            .unwrap();
    }

    fn input(project_id: &str, date: Option<&str>, content: &str, hours: f64) -> DailyReportInput {
        DailyReportInput {
            project_id: project_id.to_owned(),
            report_date: date.map(|s| s.to_owned()),
            content: content.to_owned(),
            hours,
        }
    }

    #[test]
    fn check_hours_accepts_the_bounds_and_two_decimals() {
        for h in [0.1_f64, 0.01, 7.5, 7.53, 24.0] {
            assert_eq!(check_hours(h).unwrap(), h, "{h} 要收");
        }
    }

    #[test]
    fn check_hours_refuses_zero_out_of_range_and_sub_cent_precision() {
        for h in [0.0_f64, -1.0, 24.01, 100.0, 0.001, 7.535] {
            let err = check_hours(h).unwrap_err();
            assert_eq!(err.code, "invalid_input", "{h} 要在进 SQL 之前拒掉");
            assert!(err.message.contains("工时"), "错误文本要点名「工时」：{}", err.message);
        }
    }

    #[test]
    fn create_requires_an_existing_undeleted_project() {
        let conn = db::open_in_memory().unwrap();
        let err = create(&conn, &input("nope", Some("2026-10-16"), "甲", 1.0)).unwrap_err();
        assert_eq!(err.code, "not_found", "项目不存在按 not_found(项目 id) 报，与 todo 同形");
        seed(&conn, "p-gone");
        soft_delete_project(&conn, "p-gone");
        assert_eq!(
            create(&conn, &input("p-gone", Some("2026-10-16"), "乙", 1.0)).unwrap_err().code,
            "not_found",
            "软删项目也不能挂新日报"
        );
    }

    #[test]
    fn create_rejects_empty_content() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let err = create(&conn, &input("p1", Some("2026-10-16"), "   ", 1.0)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "被拒的输入不许留下行");
    }

    /// **M② 的靶子**：若哪天有人把 `req_date` 换成 `norm_date`，空白会折成 NULL 走进 SQL，
    /// 报错变成 db_failed（V6 那条 NOT NULL 测试就是它的另一面），这条当场红。
    #[test]
    fn create_rejects_blank_and_absent_report_date_naming_the_column() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for date in [None, Some(""), Some("   ")] {
            let err = create(&conn, &input("p1", date, "甲", 1.0)).unwrap_err();
            assert_eq!(err.code, "invalid_input", "{date:?} 要报校验错，不是库错");
            assert!(err.message.contains("日报日期"), "{}", err.message);
        }
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn create_rejects_a_non_iso_report_date() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let err = create(&conn, &input("p1", Some("2026-6-30"), "甲", 1.0)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert_eq!(err.hint.as_deref(), Some("日期要用 YYYY-MM-DD"));
        assert!(err.message.contains("日报日期"), "{}", err.message);
    }

    #[test]
    fn create_rejects_out_of_range_hours() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        assert_eq!(create(&conn, &input("p1", Some("2026-10-16"), "甲", 30.0)).unwrap_err().code, "invalid_input");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "工时被拒也不许留下行");
    }

    #[test]
    fn create_then_read_roundtrip() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let r = create(&conn, &input("p1", Some(" 2026-10-16 "), "  联调第三方接口  ", 7.5)).unwrap();
        assert_eq!(r.content, "联调第三方接口", "写入要 trim");
        assert_eq!(r.report_date, "2026-10-16", "日期两侧都要 trim 后进判据");
        assert_eq!(r.hours, 7.5);
        assert_eq!(get(&conn, &r.id).unwrap(), r, "create 回来的行必须与再读的一致");
        assert_eq!(list(&conn, Some("p1")).unwrap().len(), 1);
        assert_eq!(list(&conn, None).unwrap().len(), 1, "None = 跨项目全部");
    }

    /// 三级排序键一起钉：`report_date` 是主键（按天填的直觉），后两级是 M9 N3 那条同秒 tiebreaker。
    /// 直写 INSERT 而不走 create()：uuid 主键给不出可控 id 次序，同秒 created_at 也只能手写。
    #[test]
    fn list_orders_by_report_date_then_created_at_then_id() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for (id, date, created) in [
            ("d-b", "2026-10-15", "2026-10-15 09:00:00"),
            ("d-a", "2026-10-15", "2026-10-15 09:00:00"),
            ("d-c", "2026-10-14", "2026-10-14 23:00:00"),
        ] {
            conn.execute(
                "INSERT INTO daily_reports (id, project_id, report_date, content, hours, created_at)
                 VALUES (?1, 'p1', ?2, ?1, 1.0, ?3)",
                params![id, date, created],
            )
            .unwrap();
        }
        let ids: Vec<String> = list(&conn, None).unwrap().into_iter().map(|r| r.id).collect();
        assert_eq!(ids.join(","), "d-c,d-a,d-b", "先按日期（10-14 在前），同日同秒按 id 升序；插入序是 b、a、c");
    }

    #[test]
    fn list_hides_soft_deleted_reports() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let r = create(&conn, &input("p1", Some("2026-10-16"), "甲", 1.0)).unwrap();
        delete(&conn, &r.id).unwrap();
        assert!(list(&conn, None).unwrap().is_empty());
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports WHERE deleted_at IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "delete 是软删：行要在库里（延续 M9 D8 的形制）");
        assert_eq!(get(&conn, &r.id).unwrap_err().code, "not_found");
    }

    #[test]
    fn cross_project_list_hides_reports_of_a_soft_deleted_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p-alive");
        seed(&conn, "p-dead");
        create(&conn, &input("p-alive", Some("2026-10-16"), "活项目的日报", 1.0)).unwrap();
        let dead = create(&conn, &input("p-dead", Some("2026-10-16"), "已删项目的日报", 1.0)).unwrap();
        soft_delete_project(&conn, "p-dead");

        let contents: Vec<String> = list(&conn, None).unwrap().into_iter().map(|r| r.content).collect();
        assert_eq!(contents, vec!["活项目的日报"], "软删项目的日报不能出现在跨项目列表里");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports WHERE id = ?1", [&dead.id], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "项目软删不许动日报的行（恢复项目即原样回来）");
        assert!(list(&conn, Some("p-dead")).unwrap().is_empty(), "指定已删项目也读不到");
    }

    /// **D6 的兑现点，也是变异 M① 的靶子**：给 `list` 顺手加一句 `AND p.status <> '归档'`，
    /// 这条当场红。归档项目的工时照常列、照常算 —— 跳过归档等于给领导的台账凭空少一截。
    #[test]
    fn list_includes_rows_of_an_archived_project() {
        let conn = db::open_in_memory().unwrap();
        seed_archived(&conn, "p-old");
        let r = create(&conn, &input("p-old", Some("2026-03-02"), "收尾交付物归档", 3.5)).unwrap();
        let all = list(&conn, None).unwrap();
        assert_eq!(all.len(), 1, "归档项目的日报要在跨项目列表里");
        assert_eq!(all[0].id, r.id);
        assert_eq!(list(&conn, Some("p-old")).unwrap().len(), 1, "按归档项目筛也要读到");
    }

    #[test]
    fn update_can_move_a_row_to_another_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        seed(&conn, "p2");
        let r = create(&conn, &input("p1", Some("2026-10-16"), "甲", 1.0)).unwrap();
        update(
            &conn,
            &r.id,
            &DailyReportEdit {
                project_id: "p2".to_owned(),
                report_date: Some("2026-10-17".to_owned()),
                content: "乙".to_owned(),
                hours: 2.0,
            },
        )
        .unwrap();
        let after = get(&conn, &r.id).unwrap();
        assert_eq!(after.project_id, "p2", "改归属在本轮做（spec §五.4，与 TodoEdit 不同）");
        assert_eq!(after.report_date, "2026-10-17");
        assert_eq!(after.content, "乙");
        assert_eq!(after.hours, 2.0);
    }

    #[test]
    fn update_refuses_a_missing_or_soft_deleted_project_and_bad_fields() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        seed(&conn, "p-gone");
        soft_delete_project(&conn, "p-gone");
        let r = create(&conn, &input("p1", Some("2026-10-16"), "甲", 1.0)).unwrap();
        let edit = |pid: &str, date: &str, hours: f64| DailyReportEdit {
            project_id: pid.to_owned(),
            report_date: Some(date.to_owned()),
            content: "乙".to_owned(),
            hours,
        };
        assert_eq!(update(&conn, &r.id, &edit("p-gone", "2026-10-16", 1.0)).unwrap_err().code, "not_found");
        assert_eq!(update(&conn, &r.id, &edit("p1", "2026-10-16", 99.0)).unwrap_err().code, "invalid_input");
        assert_eq!(update(&conn, &r.id, &edit("p1", "10-16", 1.0)).unwrap_err().code, "invalid_input");
        assert_eq!(update(&conn, "no-such-id", &edit("p1", "2026-10-16", 1.0)).unwrap_err().code, "not_found");
        assert_eq!(get(&conn, &r.id).unwrap().content, "甲", "被拒的 update 一条都不许改着");
    }

    #[test]
    fn delete_is_soft_and_a_second_delete_reports_not_found() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let r = create(&conn, &input("p1", Some("2026-10-16"), "甲", 1.0)).unwrap();
        delete(&conn, &r.id).unwrap();
        assert_eq!(
            delete(&conn, &r.id).unwrap_err().code,
            "not_found",
            "二次删要响亮，不能静默成功"
        );
    }

    /// 线格式钉键数（本仓形制：`todo.rs:424` 七键、manifest 八键、SearchBundle 七键）。
    #[test]
    fn daily_report_line_format_has_exactly_seven_keys() {
        let r = DailyReport {
            id: "d1".to_owned(),
            project_id: "p1".to_owned(),
            report_date: "2026-10-16".to_owned(),
            content: "甲".to_owned(),
            hours: 7.5,
            created_at: "2026-10-16 00:00:00".to_owned(),
            updated_at: "2026-10-16 00:00:00".to_owned(),
        };
        let obj = serde_json::to_value(&r).unwrap();
        let mut keys: Vec<&str> = obj.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["content", "createdAt", "hours", "id", "projectId", "reportDate", "updatedAt"],
            "DailyReport 的键集合变了就要同步 src/types/daily-report.ts 与 xlsx 表头"
        );
    }

    /// 读回用 calamine（与本任务的写同一把库），**不经 `extract::sheet_text`**：
    /// 那个函数把所有格子拼成一串文本，看不出「这一格是日期单元还是文本」，而那是 §六.1 的判据。
    fn read_cells(path: &std::path::Path) -> Vec<Vec<String>> {
        use calamine::Reader as _;
        let mut book = calamine::open_workbook_auto(path).unwrap();
        let names = book.sheet_names();
        let range = book.worksheet_range(&names[0]).unwrap();
        range.rows().map(|r| r.iter().map(|c| c.to_string()).collect()).collect()
    }

    fn export_row(name: &str, code: Option<&str>) -> ExportRow {
        ExportRow {
            report_date: "2026-10-16".to_owned(),
            project_code: code.map(|s| s.to_owned()),
            project_name: name.to_owned(),
            content: "联调第三方接口".to_owned(),
            hours: 7.5,
        }
    }

    #[test]
    fn export_writes_a_real_xlsx_that_calamine_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("out.xlsx");
        let n = write_xlsx(&p, &[export_row("甲项目", Some("P-01")), export_row("乙项目", None)]).unwrap();
        assert_eq!(n, 2);
        let cells = read_cells(&p);
        assert_eq!(cells[0].join("|"), "日期|项目编号|所属项目|工作内容|时长（小时）", "表头五列");
        assert_eq!(cells.len(), 3, "表头 + 两行数据");
        assert_eq!(cells[1].join("|"), "2026-10-16|P-01|甲项目|联调第三方接口|7.5");
        assert_eq!(cells[2][1], "", "编号为 None 要落成空格，不是字面 \"None\"");
    }

    /// §六.1：日期写成**文本**。钉的是「导出 → 用户编辑 → 再导回来」这条环不经过 Excel 序列号 ——
    /// 日期单元读回来是 `Data::DateTime`，`to_string()` 带时分秒，与文本分支不同形状。
    #[test]
    fn export_writes_the_date_as_text_not_a_date_cell() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.xlsx");
        write_xlsx(&p, &[export_row("甲项目", None)]).unwrap();
        assert_eq!(read_cells(&p)[1][0], "2026-10-16", "要读回纯日期文本，不许是日期单元的序列化形状");
    }

    /// 「下载模板」= 传空 rows：表头必须在，否则用户填无可填、导入必然报「缺列」。
    #[test]
    fn export_with_no_rows_still_writes_the_five_headers() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("template.xlsx");
        assert_eq!(write_xlsx(&p, &[]).unwrap(), 0);
        let cells = read_cells(&p);
        assert_eq!(cells.len(), 1, "模板只有表头一行");
        assert_eq!(cells[0].len(), EXPORT_HEADERS.len());
    }

    /// 写不出去按 `fs_failed` 报（`error.rs:21` 的既有码与文案），不许冒成 invalid_input 说「你输入错了」。
    #[test]
    fn write_xlsx_reports_fs_failed_when_the_directory_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("no-such-dir").join("out.xlsx");
        let err = write_xlsx(&p, &[export_row("甲项目", None)]).unwrap_err();
        assert_eq!(err.code, "fs_failed", "落点不可写是文件域，不是校验域：{}", err.message);
        assert!(err.message.contains("失败"), "{}", err.message);
    }
}
