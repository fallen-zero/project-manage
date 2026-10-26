//! 项目模板（M12 spec §三/§四）：只存项目基础信息，供新建项目一键填充。
//! 与 `project.rs` 的区别：模板没有项目名/目录/标签子表；tags 存 JSON；**硬删除**。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::error::{AppError, AppResult};
use crate::project::STATUSES;

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查表单必填项与取值范围"))
}

fn not_found(id: &str) -> AppError {
    AppError::new("not_found", &format!("模板不存在：{id}"), Some("刷新后重试"))
}

fn norm(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn norm_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.into_iter()
        .filter_map(|tag| {
            let tag = tag.trim().to_owned();
            if tag.is_empty() || !seen.insert(tag.clone()) {
                None
            } else {
                Some(tag)
            }
        })
        .collect()
}

fn tags_to_json(tags: &[String]) -> String {
    serde_json::to_string(tags).expect("Vec<String> 一定能序列化成 JSON")
}

fn tags_from_json(raw: &str) -> AppResult<Vec<String>> {
    serde_json::from_str::<Vec<String>>(raw).map_err(|_| {
        AppError::new(
            "db_failed",
            "模板标签不是合法 JSON",
            Some("检查是否手改过库；正常写入不会产生这个形状"),
        )
    })
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTemplate {
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInput {
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

struct Normalized {
    name: String,
    code: Option<String>,
    manager: Option<String>,
    customer: Option<String>,
    contact_name: Option<String>,
    contact_phone: Option<String>,
    contract_no: Option<String>,
    contract_period: Option<String>,
    delivery_deadline: Option<String>,
    status: String,
    tags: Vec<String>,
}

fn normalize(input: &TemplateInput) -> AppResult<Normalized> {
    let name = norm(Some(input.name.clone())).ok_or_else(|| invalid("模板名称不能为空"))?;
    if !STATUSES.contains(&input.status.as_str()) {
        return Err(invalid(&format!("状态「{}」不在枚举内", input.status)));
    }
    Ok(Normalized {
        name,
        code: norm(input.code.clone()),
        manager: norm(input.manager.clone()),
        customer: norm(input.customer.clone()),
        contact_name: norm(input.contact_name.clone()),
        contact_phone: norm(input.contact_phone.clone()),
        contract_no: norm(input.contract_no.clone()),
        contract_period: norm(input.contract_period.clone()),
        delivery_deadline: norm(input.delivery_deadline.clone()),
        status: input.status.clone(),
        tags: norm_tags(input.tags.clone()),
    })
}

fn check_duplicate(conn: &Connection, name: &str, exclude_id: Option<&str>) -> AppResult<()> {
    let count: i64 = match exclude_id {
        Some(id) => conn.query_row(
            "SELECT COUNT(*) FROM project_templates WHERE name = ?1 AND id <> ?2",
            params![name, id],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*) FROM project_templates WHERE name = ?1",
            [name],
            |r| r.get(0),
        )?,
    };
    if count > 0 {
        return Err(invalid(&format!("模板名称「{name}」已存在")));
    }
    Ok(())
}

const SELECT_COLUMNS: &str = "id, name, code, manager, customer, contact_name, contact_phone,
     contract_no, contract_period, delivery_deadline, status, tags";

struct RowValues {
    id: String,
    name: String,
    code: Option<String>,
    manager: Option<String>,
    customer: Option<String>,
    contact_name: Option<String>,
    contact_phone: Option<String>,
    contract_no: Option<String>,
    contract_period: Option<String>,
    delivery_deadline: Option<String>,
    status: String,
    tags: String,
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RowValues> {
    Ok(RowValues {
        id: row.get(0)?,
        name: row.get(1)?,
        code: row.get(2)?,
        manager: row.get(3)?,
        customer: row.get(4)?,
        contact_name: row.get(5)?,
        contact_phone: row.get(6)?,
        contract_no: row.get(7)?,
        contract_period: row.get(8)?,
        delivery_deadline: row.get(9)?,
        status: row.get(10)?,
        tags: row.get(11)?,
    })
}

fn into_template(v: RowValues) -> AppResult<ProjectTemplate> {
    Ok(ProjectTemplate {
        id: v.id,
        name: v.name,
        code: v.code,
        manager: v.manager,
        customer: v.customer,
        contact_name: v.contact_name,
        contact_phone: v.contact_phone,
        contract_no: v.contract_no,
        contract_period: v.contract_period,
        delivery_deadline: v.delivery_deadline,
        status: v.status,
        tags: tags_from_json(&v.tags)?,
    })
}

fn read(conn: &Connection, id: &str) -> AppResult<ProjectTemplate> {
    let raw = conn
        .query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM project_templates WHERE id = ?1"),
            [id],
            read_row,
        )
        .optional()?;
    match raw {
        Some(v) => into_template(v),
        None => Err(not_found(id)),
    }
}

pub fn list(conn: &Connection) -> AppResult<Vec<ProjectTemplate>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} FROM project_templates ORDER BY created_at, id"
    ))?;
    let rows = stmt.query_map([], read_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(into_template(row?)?);
    }
    Ok(out)
}

pub fn create(conn: &Connection, input: &TemplateInput) -> AppResult<ProjectTemplate> {
    let n = normalize(input)?;
    check_duplicate(conn, &n.name, None)?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO project_templates
           (id, name, code, manager, customer, contact_name, contact_phone,
            contract_no, contract_period, delivery_deadline, status, tags)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            id,
            n.name,
            n.code,
            n.manager,
            n.customer,
            n.contact_name,
            n.contact_phone,
            n.contract_no,
            n.contract_period,
            n.delivery_deadline,
            n.status,
            tags_to_json(&n.tags)
        ],
    )?;
    read(conn, &id)
}

pub fn update(conn: &Connection, id: &str, input: &TemplateInput) -> AppResult<ProjectTemplate> {
    let n = normalize(input)?;
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM project_templates WHERE id = ?1)",
        [id],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(not_found(id));
    }
    check_duplicate(conn, &n.name, Some(id))?;
    conn.execute(
        "UPDATE project_templates SET name=?2, code=?3, manager=?4, customer=?5, contact_name=?6,
                contact_phone=?7, contract_no=?8, contract_period=?9, delivery_deadline=?10,
                status=?11, tags=?12, updated_at=datetime('now')
          WHERE id=?1",
        params![
            id,
            n.name,
            n.code,
            n.manager,
            n.customer,
            n.contact_name,
            n.contact_phone,
            n.contract_no,
            n.contract_period,
            n.delivery_deadline,
            n.status,
            tags_to_json(&n.tags)
        ],
    )?;
    read(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute("DELETE FROM project_templates WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn input(name: &str) -> TemplateInput {
        TemplateInput {
            name: name.to_owned(),
            code: None,
            manager: None,
            customer: None,
            contact_name: None,
            contact_phone: None,
            contract_no: None,
            contract_period: None,
            delivery_deadline: None,
            status: "立项".to_owned(),
            tags: vec![],
        }
    }

    #[test]
    fn create_update_list_round_trip_all_fields() {
        let conn = db::open_in_memory().unwrap();
        let mut i = input("模板A");
        i.code = Some("TPL-1".into());
        i.manager = Some("王经理".into());
        i.customer = Some("客户甲".into());
        i.contact_name = Some("联系人".into());
        i.contact_phone = Some("13800000000".into());
        i.contract_no = Some("HT-1".into());
        i.contract_period = Some("2026-01 ~ 2026-12".into());
        i.delivery_deadline = Some("2026-12-31".into());
        i.tags = vec!["运维".into(), "政务".into()];
        let created = create(&conn, &i).unwrap();
        assert_eq!(created.name, "模板A");
        assert_eq!(created.code.as_deref(), Some("TPL-1"));
        assert_eq!(created.tags, vec!["运维", "政务"]);

        let mut u = input("模板B");
        u.manager = Some("新经理".into());
        u.status = "开发".into();
        u.tags = vec!["新标签".into()];
        let updated = update(&conn, &created.id, &u).unwrap();
        assert_eq!(updated.name, "模板B");
        assert_eq!(updated.manager.as_deref(), Some("新经理"));
        assert_eq!(updated.status, "开发");
        assert_eq!(updated.tags, vec!["新标签"]);

        let rows = list(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], updated);
    }

    #[test]
    fn duplicate_name_is_rejected_naming_the_template() {
        let conn = db::open_in_memory().unwrap();
        create(&conn, &input("模板A")).unwrap();
        let err = create(&conn, &input("模板A")).unwrap_err();
        assert_eq!(err.code, "invalid_input", "命令路径要的是应用层错误，不是 db_failed");
        assert!(err.message.contains("模板A"), "要点名重名的模板：{}", err.message);

        let b = create(&conn, &input("模板B")).unwrap();
        let err = update(&conn, &b.id, &input("模板A")).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("模板A"));
        // 排除自己：把 B 改成 B 必须通过。
        assert_eq!(update(&conn, &b.id, &input("模板B")).unwrap().name, "模板B");
    }

    #[test]
    fn blank_name_and_bad_status_are_rejected() {
        let conn = db::open_in_memory().unwrap();
        let err = create(&conn, &input("   ")).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("模板名称"), "要点名模板名称：{}", err.message);

        let mut bad = input("模板C");
        bad.status = "已上线".into();
        let err = create(&conn, &bad).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("状态"), "要点名状态：{}", err.message);
        assert!(err.message.contains("已上线"), "要念出坏值：{}", err.message);
    }

    #[test]
    fn update_and_delete_unknown_id_are_not_found() {
        let conn = db::open_in_memory().unwrap();
        let err = update(&conn, "missing", &input("模板")).unwrap_err();
        assert_eq!(err.code, "not_found");
        let err = delete(&conn, "missing").unwrap_err();
        assert_eq!(err.code, "not_found");
    }

    #[test]
    fn tags_are_trimmed_deduped_and_round_trip() {
        let conn = db::open_in_memory().unwrap();
        let mut i = input("标签模板");
        i.tags = vec!["  运维 ".into(), "运维".into(), "".into(), "政务".into()];
        let created = create(&conn, &i).unwrap();
        assert_eq!(created.tags, vec!["运维", "政务"]);
        assert_eq!(list(&conn).unwrap()[0].tags, vec!["运维", "政务"]);
    }

    #[test]
    fn list_orders_by_created_at_then_id() {
        let conn = db::open_in_memory().unwrap();
        let a = create(&conn, &input("A")).unwrap();
        let b = create(&conn, &input("B")).unwrap();
        let c = create(&conn, &input("C")).unwrap();
        conn.execute("UPDATE project_templates SET created_at = '2026-01-01 00:00:03' WHERE id = ?1", [&a.id]).unwrap();
        conn.execute("UPDATE project_templates SET created_at = '2026-01-01 00:00:01' WHERE id = ?1", [&b.id]).unwrap();
        conn.execute("UPDATE project_templates SET created_at = '2026-01-01 00:00:02' WHERE id = ?1", [&c.id]).unwrap();
        let names: Vec<String> = list(&conn).unwrap().into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["B", "C", "A"], "按 created_at, id 稳定排序");
    }

    #[test]
    fn delete_is_hard_and_name_can_be_reused() {
        let conn = db::open_in_memory().unwrap();
        let t = create(&conn, &input("可重用")).unwrap();
        delete(&conn, &t.id).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM project_templates", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "硬删除后行必须消失");
        let again = create(&conn, &input("可重用")).unwrap();
        assert_eq!(again.name, "可重用");
        assert_eq!(list(&conn).unwrap().len(), 1);
    }
}
