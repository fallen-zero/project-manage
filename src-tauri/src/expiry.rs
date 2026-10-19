//! 到期事项：维保 / 域名 / 证书 / 其他 四操作（M11-a spec §五、§六）。
//!
//! 形制整段照 `milestone.rs`（M9 Task 3）：一条 `list` SQL 同时管「跨项目全部」与「按项目筛」，
//! `p.deleted_at IS NULL` 在本表这一处（检索侧的第二处由 Task 3 那条 collect 落，届时两处各自有守卫测试）。
//! 日期形状判据复用 `todo::req_date`（必填日期**不能**走 `norm_date`：它把空白折成 NULL，
//! 而 NULL 进 `expire_date NOT NULL` 会由 SQLite 冒成 `db_failed`，用户看到的是「数据库操作失败」
//! 而不是「你的日期没填」—— M10 为 `report_date` 立的同一条理由）。

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

/// 四类词表：需求原文那三类 + D7 当次批准的「其他」。
/// 与 `projects.status` 那八段、`milestones.stage` 那五段是**三套互不相干的词**，别互换下拉。
/// V7 的 CHECK 与本常量、`src/types/expiry.ts` 的 `EXPIRY_KINDS` 三处由 lib.rs 那条文本门钉住。
pub(crate) const KINDS: [&str; 4] = ["维保", "域名", "证书", "其他"];

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Expiry {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub label: String,
    pub expire_date: String,
    pub note: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpiryInput {
    pub project_id: String,
    pub kind: String,
    pub label: String,
    pub expire_date: Option<String>,
    pub note: Option<String>,
}

/// 只有 `expire_date` 与 `note`（spec §二 自拍①：按界面反推接口）。
/// 种类与名称录错走删了重建；改归属同理 —— 域名/证书天然属于一个项目。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpiryUpdate {
    pub expire_date: Option<String>,
    pub note: Option<String>,
}

const SELECT_COLUMNS: &str =
    "e.id, e.project_id, e.kind, e.label, e.expire_date, e.note, e.created_at, e.updated_at";

fn map_expiry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Expiry> {
    Ok(Expiry {
        id: row.get(0)?,
        project_id: row.get(1)?,
        kind: row.get(2)?,
        label: row.get(3)?,
        expire_date: row.get(4)?,
        note: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
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

/// `ORDER BY e.expire_date, e.kind, e.id`：最近的先看到（`expire_date` NOT NULL，不需要
/// `milestones` 那句 `plan_date IS NULL` 前置）；第三键 `, e.id` 是 M9 终审 N3 同一条理由 ——
/// `datetime('now')` 只到秒，同一天同类别的两行必须有确定次序，**摘掉它下面那条 `list_breaks_ties_by_id_when_date_and_kind_are_equal` 当场红**。
pub fn list(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<Expiry>> {
    let mut stmt = conn.prepare(
        &format!(
            "SELECT {SELECT_COLUMNS}
               FROM project_expiries e
               JOIN projects p ON p.id = e.project_id AND p.deleted_at IS NULL
              WHERE e.deleted_at IS NULL AND (?1 IS NULL OR e.project_id = ?1)
              ORDER BY e.expire_date, e.kind, e.id"
        ),
    )?;
    let rows = stmt.query_map(params![project_id], map_expiry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Expiry> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM project_expiries e WHERE e.id = ?1 AND e.deleted_at IS NULL"
    );
    conn.query_row(&sql, [id], map_expiry)
        .optional()?
        .ok_or_else(|| not_found(id))
}

/// 词表在**应用层**判：落到 SQLite 的 CHECK 会变成 `db_failed`，用户看不出是自己选错（spec §五）。
/// CHECK 仍留着，它挡的是绕过命令直写库的那条路（Task 1 的 `v7_kind_check_refuses_words_outside_the_four`）。
fn check_kind(kind: &str) -> AppResult<()> {
    if KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(invalid(&format!(
            "种类「{kind}」不在 {} 之内",
            KINDS.join("/")
        )))
    }
}

pub fn create(conn: &Connection, input: &ExpiryInput) -> AppResult<Expiry> {
    project_exists(conn, &input.project_id)?;
    check_kind(&input.kind)?;
    let label = norm(Some(input.label.clone())).ok_or_else(|| invalid("「名称」不能为空（如 oa.gov.cn、年度维保合同）"))?;
    let expire_date = req_date("到期日期", input.expire_date.clone())?;
    let note = norm(input.note.clone()).unwrap_or_default();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO project_expiries (id, project_id, kind, label, expire_date, note)
          VALUES (?1,?2,?3,?4,?5,?6)",
        params![id, input.project_id, input.kind, label, expire_date, note],
    )?;
    get(conn, &id)
}

/// 只改日期与备注两列（D2：续期 = 改日期）。`kind`/`label`/`project_id` 一律不碰。
pub fn update(conn: &Connection, id: &str, upd: &ExpiryUpdate) -> AppResult<()> {
    let expire_date = req_date("到期日期", upd.expire_date.clone())?;
    let note = norm(upd.note.clone()).unwrap_or_default();
    let changed = conn.execute(
        "UPDATE project_expiries SET expire_date=?2, note=?3, updated_at=datetime('now')
          WHERE id=?1 AND deleted_at IS NULL",
        params![id, expire_date, note],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE project_expiries SET deleted_at = datetime('now'), updated_at = datetime('now')
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
            "INSERT INTO projects (id, name, status) VALUES (?1, '到期事项测试项目', '立项')",
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

    fn input(project_id: &str, kind: &str, label: &str, date: Option<&str>, note: Option<&str>) -> ExpiryInput {
        ExpiryInput {
            project_id: project_id.to_owned(),
            kind: kind.to_owned(),
            label: label.to_owned(),
            expire_date: date.map(str::to_owned),
            note: note.map(str::to_owned),
        }
    }

    fn upd(date: Option<&str>, note: Option<&str>) -> ExpiryUpdate {
        ExpiryUpdate {
            expire_date: date.map(str::to_owned),
            note: note.map(str::to_owned),
        }
    }

    #[test]
    fn create_refuses_kind_words_outside_the_four_with_invalid_input() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        // 「维保中」是 project_tags 里的标签词（技术方案 §四），不在本词表 —— 恰是这条最容易混的一格。
        for bad in ["备案", "订阅", "维保中", "上线", ""] {
            let err = create(&conn, &input("p1", bad, "一行", Some("2027-01-31"), None)).unwrap_err();
            assert_eq!(err.code, "invalid_input", "「{bad}」要的是应用层那句，不是 db_failed");
        }
        for good in KINDS {
            create(&conn, &input("p1", good, "一行", Some("2027-01-31"), None)).unwrap();
        }
        assert_eq!(list(&conn, Some("p1")).unwrap().len(), 4, "四类各建一行都要成");
    }

    #[test]
    fn create_requires_expire_date_naming_the_column() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for missing in [None, Some("")] {
            let err = create(&conn, &input("p1", "域名", "a.gov.cn", missing, None)).unwrap_err();
            assert_eq!(err.code, "invalid_input", "空白与缺键都不许落进 SQL");
            assert!(err.message.contains("到期日期"), "错误文本要点名是哪一列：{}", err.message);
        }
        // 形状就不对的值（点分隔），不是「历法错」的值：拿合法日期断 unwrap_err 是自己红。
        let err = create(&conn, &input("p1", "域名", "a.gov.cn", Some("2027.01.31"), None)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert_eq!(err.hint.as_deref(), Some("日期要用 YYYY-MM-DD"), "hint 实际：{:?}", err.hint);
        let e = create(&conn, &input("p1", "域名", "a.gov.cn", Some("2027-01-31"), None)).unwrap();
        assert_eq!(e.expire_date, "2027-01-31");
    }

    #[test]
    fn create_rejects_blank_or_whitespace_label_naming_the_column() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for blank in ["", "   ", "\t"] {
            let err = create(&conn, &input("p1", "证书", blank, Some("2027-01-31"), None)).unwrap_err();
            assert_eq!(err.code, "invalid_input", "名称是 NOT NULL 且必填，空白要在这里就挡下");
            assert!(err.message.contains("名称"), "要点名是哪一列：{}", err.message);
        }
    }

    #[test]
    fn create_rejects_project_that_does_not_exist_or_is_deleted() {
        let conn = db::open_in_memory().unwrap();
        let err = create(&conn, &input("nope", "维保", "甲方", Some("2027-01-31"), None)).unwrap_err();
        assert_eq!(err.code, "not_found");
        seed(&conn, "p-gone");
        soft_delete_project(&conn, "p-gone");
        let err = create(&conn, &input("p-gone", "维保", "甲方", Some("2027-01-31"), None)).unwrap_err();
        assert_eq!(err.code, "not_found", "软删项目不许再往里录到期事项");
    }

    #[test]
    fn create_then_list_roundtrip_trims_label_and_note() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let e = create(&conn, &input("p1", "域名", "  oa.gov.cn  ", Some("2027-03-31"), Some("  已问厂商  ")))
            .unwrap();
        assert_eq!(e.label, "oa.gov.cn", "名称写入要 trim");
        assert_eq!(e.note, "已问厂商", "备注写入要 trim");
        let bare = create(&conn, &input("p1", "证书", "tls", Some("2027-04-01"), None)).unwrap();
        assert_eq!(bare.note, "", "不填 note 就是空串，不是 NULL（列 NOT NULL DEFAULT ''）");
        assert_eq!(get(&conn, &e.id).unwrap(), e);
        let labels: Vec<String> = list(&conn, Some("p1")).unwrap().into_iter().map(|x| x.label).collect();
        assert_eq!(labels, vec!["oa.gov.cn", "tls"], "按到期日升序");
        assert_eq!(list(&conn, None).unwrap().len(), 2);
    }

    /// `, e.id` 那个 tiebreaker 的唯一守卫：同一天 + 同一 kind + 插入顺序与 id 序相反。
    /// **摘掉 ORDER BY 里的 `, e.id` 这条当场红**（Task 8 变异 M③′ 的备选靶）。
    #[test]
    fn list_breaks_ties_by_id_when_date_and_kind_are_equal() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        for id in ["e-3", "e-1", "e-2"] {
            conn.execute(
                "INSERT INTO project_expiries (id, project_id, kind, label, expire_date, created_at)
                  VALUES (?1, 'p1', '域名', '同一个域名', '2027-05-01', '2026-10-19 09:00:00')",
                [id],
            )
            .unwrap();
        }
        let ids: Vec<String> = list(&conn, None).unwrap().into_iter().map(|e| e.id).collect();
        // 断言用 join 而不是 vec![...to_owned()...]：形状少一处，复验只需按码点（M9 踩过同一坑）。
        assert_eq!(ids.join(","), "e-1,e-2,e-3", "同日期同种类必须按 id 升序，插入顺序是 e-3、e-1、e-2");
    }

    #[test]
    fn list_hides_soft_deleted_expiries() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        let e = create(&conn, &input("p1", "证书", "tls", Some("2027-01-31"), None)).unwrap();
        delete(&conn, &e.id).unwrap();
        assert!(list(&conn, None).unwrap().is_empty());
        assert_eq!(get(&conn, &e.id).unwrap_err().code, "not_found");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM project_expiries WHERE deleted_at IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "软删：行要在库里");
    }

    /// spec §九.3 的兑现点，Task 8 变异 **M①** 在这一格的靶子：
    /// **摘掉 `AND p.deleted_at IS NULL` 这条当场红**（`ON DELETE CASCADE` 因项目软删而永不触发）。
    #[test]
    fn cross_project_list_hides_expiries_of_a_soft_deleted_project() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p-alive");
        seed(&conn, "p-dead");
        create(&conn, &input("p-alive", "域名", "a.gov.cn", Some("2027-01-31"), None)).unwrap();
        let dead = create(&conn, &input("p-dead", "证书", "b.gov.cn", Some("2027-02-28"), None)).unwrap();
        soft_delete_project(&conn, "p-dead");

        let labels: Vec<String> = list(&conn, None).unwrap().into_iter().map(|e| e.label).collect();
        assert_eq!(labels, vec!["a.gov.cn"], "软删项目的到期事项不能进跨项目列表（预警就是这条查询）");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM project_expiries WHERE id = ?1", [&dead.id], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "行留着，恢复项目即原样回来");
    }

    /// D2 的兑现点 + spec §二 自拍① 的兑现点：update 只动日期与备注，
    /// `kind`/`label`/`project_id` 三列**一个字也不动**（接口里根本没有它们）。
    #[test]
    fn update_changes_only_date_and_note_and_reports_not_found_for_missing_row() {
        let conn = db::open_in_memory().unwrap();
        seed(&conn, "p1");
        seed(&conn, "p2");
        let e = create(&conn, &input("p1", "维保", "甲方年度维保", Some("2026-12-31"), Some("去年签的"))).unwrap();
        update(&conn, &e.id, &upd(Some("2027-12-31"), None)).unwrap();
        let after = get(&conn, &e.id).unwrap();
        assert_eq!(after.expire_date, "2027-12-31", "续期就是改日期（D2）");
        assert_eq!(after.note, "", "note 传 None 就是清成空串");
        assert_eq!(after.kind, "维保", "update 改不了种类");
        assert_eq!(after.label, "甲方年度维保", "update 改不了名称");
        assert_eq!(after.project_id, "p1", "update 改不了归属");
        update(&conn, &e.id, &upd(Some("2028-01-15"), Some("已续"))).unwrap();
        let twice = get(&conn, &e.id).unwrap();
        assert_eq!((twice.expire_date.as_str(), twice.note.as_str()), ("2028-01-15", "已续"));
        // 空白/坏形状同样要拒，且拒了以后**原值一个字不变**（不许写坏一半）。
        let err = update(&conn, &e.id, &upd(Some(""), None)).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert_eq!(get(&conn, &e.id).unwrap().expire_date, "2028-01-15", "被拒的更新不能留下半成品");
        assert_eq!(update(&conn, "no-such", &upd(Some("2027-01-01"), None)).unwrap_err().code, "not_found");
    }

    /// 线格式钉键数：八键（spec §六）。少一键 = 前端读 undefined，多一键 = 白送（`deleted_at` 不许上线）。
    #[test]
    fn expiry_line_format_has_exactly_eight_keys() {
        let e = Expiry {
            id: "x1".to_owned(),
            project_id: "p1".to_owned(),
            kind: "域名".to_owned(),
            label: "oa.gov.cn".to_owned(),
            expire_date: "2027-01-31".to_owned(),
            note: "已问厂商".to_owned(),
            created_at: "2026-10-19 00:00:00".to_owned(),
            updated_at: "2026-10-19 00:00:00".to_owned(),
        };
        let obj = serde_json::to_value(&e).unwrap();
        let mut keys: Vec<&str> = obj.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "createdAt",
                "expireDate",
                "id",
                "kind",
                "label",
                "note",
                "projectId",
                "updatedAt"
            ],
            "Expiry 的键集合变了就要同步 src/types/expiry.ts"
        );
    }
}
