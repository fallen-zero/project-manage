use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::project::{Project, ProjectInput};

const DEFAULT_LIMIT: i64 = 200;
const MAX_LIMIT: i64 = 200;
const ACTIONS: [&str; 3] = ["create", "update", "delete"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuditChange {
    pub field: String,
    pub label: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub action: String,
    pub changes: Vec<AuditChange>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditProjectOption {
    pub project_id: String,
    pub project_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditPage {
    pub items: Vec<AuditEntry>,
    pub has_more: bool,
    pub projects: Vec<AuditProjectOption>,
}

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("刷新页面后重试"))
}

fn normalize_opt(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

fn tags_text(tags: &[String]) -> Option<String> {
    let mut tags: Vec<String> = tags
        .iter()
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect();
    tags.sort();
    tags.dedup();
    if tags.is_empty() {
        None
    } else {
        Some(tags.join("、"))
    }
}

fn parent_name(conn: &Connection, id: Option<&str>) -> AppResult<String> {
    let Some(pid) = id else {
        return Ok("无（顶层项目）".to_owned());
    };
    let name: Option<String> = conn
        .query_row(
            "SELECT name FROM projects WHERE id = ?1 AND deleted_at IS NULL",
            [pid],
            |r| r.get(0),
        )
        .optional()?;
    Ok(name.unwrap_or_else(|| "已删除项目".to_owned()))
}

/// 区名。日志里存显示名而不是裸 id（与 `parent_name` 同一条理由）。
/// 有项目的区删不掉（M15 spec §五 守卫③），所以「已删除工作区」这一支实际不可达。
fn workspace_name(conn: &Connection, id: &str) -> AppResult<String> {
    crate::workspace::name_of(conn, id)
}

struct FieldValue {
    field: &'static str,
    label: &'static str,
    value: Option<String>,
}

fn input_fields(input: &ProjectInput) -> Vec<FieldValue> {
    vec![
        FieldValue { field: "name", label: "项目名称", value: Some(input.name.trim().to_owned()) },
        FieldValue { field: "code", label: "项目编号", value: normalize_opt(&input.code) },
        FieldValue { field: "manager", label: "项目经理", value: normalize_opt(&input.manager) },
        FieldValue { field: "customer", label: "客户名称", value: normalize_opt(&input.customer) },
        FieldValue { field: "contactName", label: "对接联系人", value: normalize_opt(&input.contact_name) },
        FieldValue { field: "contactPhone", label: "联系方式", value: normalize_opt(&input.contact_phone) },
        FieldValue { field: "contractNo", label: "合同编号", value: normalize_opt(&input.contract_no) },
        FieldValue { field: "contractPeriod", label: "合同周期", value: normalize_opt(&input.contract_period) },
        FieldValue { field: "deliveryDeadline", label: "交付截止", value: normalize_opt(&input.delivery_deadline) },
        FieldValue { field: "status", label: "项目状态", value: Some(input.status.clone()) },
        FieldValue { field: "tags", label: "自定义标签", value: tags_text(&input.tags) },
    ]
}

fn project_fields(p: &Project) -> Vec<FieldValue> {
    vec![
        FieldValue { field: "name", label: "项目名称", value: Some(p.name.clone()) },
        FieldValue { field: "code", label: "项目编号", value: p.code.clone() },
        FieldValue { field: "manager", label: "项目经理", value: p.manager.clone() },
        FieldValue { field: "customer", label: "客户名称", value: p.customer.clone() },
        FieldValue { field: "contactName", label: "对接联系人", value: p.contact_name.clone() },
        FieldValue { field: "contactPhone", label: "联系方式", value: p.contact_phone.clone() },
        FieldValue { field: "contractNo", label: "合同编号", value: p.contract_no.clone() },
        FieldValue { field: "contractPeriod", label: "合同周期", value: p.contract_period.clone() },
        FieldValue { field: "deliveryDeadline", label: "交付截止", value: p.delivery_deadline.clone() },
        FieldValue { field: "status", label: "项目状态", value: Some(p.status.clone()) },
        FieldValue { field: "tags", label: "自定义标签", value: tags_text(&p.tags) },
    ]
}

fn change(field: &str, label: &str, old_value: Option<String>, new_value: Option<String>) -> AuditChange {
    AuditChange {
        field: field.to_owned(),
        label: label.to_owned(),
        old_value,
        new_value,
    }
}

fn create_changes(
    conn: &Connection,
    input: &ProjectInput,
    workspace_id: &str,
) -> AppResult<Vec<AuditChange>> {
    let mut out: Vec<AuditChange> = input_fields(input)
        .into_iter()
        .filter_map(|f| f.value.map(|v| change(f.field, f.label, None, Some(v))))
        .collect();
    out.push(change(
        "workspaceId",
        "工作区",
        None,
        Some(workspace_name(conn, workspace_id)?),
    ));
    if let Some(pid) = normalize_opt(&input.parent_id) {
        out.push(change(
            "parentId",
            "所属总包",
            None,
            Some(parent_name(conn, Some(pid.as_str()))?),
        ));
    }
    Ok(out)
}

fn update_changes(conn: &Connection, old: &Project, input: &ProjectInput) -> AppResult<Vec<AuditChange>> {
    let mut out = Vec::new();
    let old_fields = project_fields(old);
    let new_fields = input_fields(input);
    debug_assert_eq!(old_fields.len(), new_fields.len());
    for (o, n) in old_fields.into_iter().zip(new_fields) {
        if o.value != n.value {
            out.push(change(o.field, o.label, o.value, n.value));
        }
    }
    // 换区只在**被编辑的那个根项目**下记一条；随迁的后代不各自记（M15 spec §七：
    // 否则一次换区产 N 条噪声行，而它们的区变化完全由父这一条解释）。
    let target_ws = normalize_opt(&input.workspace_id)
        .unwrap_or_else(|| old.workspace_id.clone());
    if target_ws != old.workspace_id {
        out.push(change(
            "workspaceId",
            "工作区",
            Some(workspace_name(conn, &old.workspace_id)?),
            Some(workspace_name(conn, &target_ws)?),
        ));
    }
    let old_pid = old.parent_id.as_deref();
    let new_pid = normalize_opt(&input.parent_id);
    if old_pid != new_pid.as_deref() {
        out.push(change(
            "parentId",
            "所属总包",
            Some(parent_name(conn, old_pid)?),
            Some(parent_name(conn, new_pid.as_deref())?),
        ));
    }
    Ok(out)
}

fn delete_changes(conn: &Connection, old: &Project) -> AppResult<Vec<AuditChange>> {
    let mut out: Vec<AuditChange> = project_fields(old)
        .into_iter()
        .filter_map(|f| f.value.map(|v| change(f.field, f.label, Some(v), None)))
        .collect();
    out.push(change(
        "workspaceId",
        "工作区",
        Some(workspace_name(conn, &old.workspace_id)?),
        None,
    ));
    if let Some(pid) = old.parent_id.as_deref() {
        out.push(change(
            "parentId",
            "所属总包",
            Some(parent_name(conn, Some(pid))?),
            None,
        ));
    }
    Ok(out)
}

fn insert(
    conn: &Connection,
    project_id: &str,
    project_name: &str,
    action: &str,
    changes: &[AuditChange],
) -> AppResult<()> {
    let json = serde_json::to_string(changes).map_err(|e| {
        AppError::new(
            "db_failed",
            &format!("日志 changes 序列化失败：{e}"),
            Some("这是内部错误，请反馈"),
        )
    })?;
    conn.execute(
        "INSERT INTO project_audit_logs (id, project_id, project_name, action, changes)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![uuid::Uuid::new_v4().to_string(), project_id, project_name, action, json],
    )?;
    Ok(())
}

pub fn record_create(
    conn: &Connection,
    project_id: &str,
    input: &ProjectInput,
    workspace_id: &str,
) -> AppResult<()> {
    let changes = create_changes(conn, input, workspace_id)?;
    insert(conn, project_id, input.name.trim(), "create", &changes)
}

pub fn record_update(conn: &Connection, old: &Project, input: &ProjectInput) -> AppResult<()> {
    let changes = update_changes(conn, old, input)?;
    if changes.is_empty() {
        return Ok(());
    }
    insert(conn, &old.id, input.name.trim(), "update", &changes)
}

pub fn record_delete(conn: &Connection, old: &Project) -> AppResult<()> {
    let changes = delete_changes(conn, old)?;
    insert(conn, &old.id, &old.name, "delete", &changes)
}

pub fn list(
    conn: &Connection,
    project_id: Option<&str>,
    action: Option<&str>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> AppResult<AuditPage> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(invalid("limit 必须在 1..=200 之间"));
    }
    let offset = offset.unwrap_or(0);
    if offset < 0 {
        return Err(invalid("offset 不能为负数"));
    }
    if let Some(a) = action {
        if !ACTIONS.contains(&a) {
            return Err(invalid("操作类型只能是 create/update/delete"));
        }
    }
    let project_filter = project_id.map(str::trim).filter(|s| !s.is_empty());
    let workspace = crate::workspace::active_id(conn)?;
    // **故意不加 `p.deleted_at IS NULL`**：软删项目的日志必须还出现在所属区
    // （M14 的「永久保留」+ 删除动作本身就产生一条日志，spec §七）。
    let mut stmt = conn.prepare(
        "SELECT a.id, a.project_id, a.project_name, a.action, a.changes, a.created_at
           FROM project_audit_logs a
           JOIN projects p ON p.id = a.project_id
          WHERE (?1 IS NULL OR a.project_id = ?1) AND (?2 IS NULL OR a.action = ?2)
            AND p.workspace_id = ?5
          ORDER BY a.created_at DESC, a.rowid DESC
          LIMIT ?3 OFFSET ?4",
    )?;
    let raw = stmt
        .query_map(params![project_filter, action, limit + 1, offset, workspace], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let has_more = raw.len() > limit as usize;
    let mut items = Vec::new();
    for (id, project_id, project_name, action, changes, created_at) in raw.into_iter().take(limit as usize) {
        let changes: Vec<AuditChange> = serde_json::from_str(&changes).map_err(|e| {
            AppError::new(
                "db_failed",
                &format!("日志 changes 不是合法 JSON：{e}"),
                Some("检查是否手改过库"),
            )
        })?;
        items.push(AuditEntry { id, project_id, project_name, action, changes, created_at });
    }
    Ok(AuditPage { items, has_more, projects: project_options(conn)? })
}

fn project_options(conn: &Connection) -> AppResult<Vec<AuditProjectOption>> {
    let workspace = crate::workspace::active_id(conn)?;
    let mut stmt = conn.prepare(
        "SELECT a.project_id, a.project_name
         FROM project_audit_logs a
         JOIN projects p ON p.id = a.project_id AND p.workspace_id = ?1
         JOIN (SELECT project_id, MAX(rowid) AS max_rowid FROM project_audit_logs GROUP BY project_id) b
           ON a.rowid = b.max_rowid
         ORDER BY a.project_name, a.project_id",
    )?;
    let out = stmt
        .query_map([&workspace], |r| {
            Ok(AuditProjectOption { project_id: r.get(0)?, project_name: r.get(1)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn input(name: &str) -> ProjectInput {
        ProjectInput {
            name: name.into(),
            code: Some(" P-1 ".into()),
            manager: Some("张三".into()),
            customer: Some("客户甲".into()),
            contact_name: None,
            contact_phone: Some("13800000000".into()),
            contract_no: None,
            contract_period: None,
            delivery_deadline: None,
            status: "开发".into(),
            parent_id: None,
            workspace_id: None,
            tags: vec!["运维".into(), " 政务 ".into(), "政务".into(), "".into()],
        }
    }

    fn logs(conn: &Connection) -> Vec<AuditEntry> {
        list(conn, None, None, None, None).unwrap().items
    }

    #[test]
    fn create_logs_initial_fields() {
        let conn = db::open_in_memory().unwrap();
        let p = crate::project::create_project(&conn, &input("甲项目")).unwrap();
        let rows = logs(&conn);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project_id, p.id);
        assert_eq!(rows[0].project_name, "甲项目");
        assert_eq!(rows[0].action, "create");
        let fields: Vec<&str> = rows[0].changes.iter().map(|c| c.field.as_str()).collect();
        // Task 4 起 create 日志多一格「工作区」（spec §七：create 记落进去的那个区）。
        assert_eq!(
            fields,
            vec!["name", "code", "manager", "customer", "contactPhone", "status", "tags", "workspaceId"]
        );
        assert!(rows[0].changes.iter().all(|c| c.old_value.is_none()));
        assert_eq!(rows[0].changes[1].new_value.as_deref(), Some("P-1"));
        assert_eq!(rows[0].changes[6].new_value.as_deref(), Some("政务、运维"));
        assert_eq!(
            rows[0].changes[7].new_value.as_deref(),
            Some("默认工作区"),
            "create 记的是落进去的区名，不是裸 id"
        );
    }

    #[test]
    fn update_logs_only_changed_fields() {
        let conn = db::open_in_memory().unwrap();
        let p = crate::project::create_project(&conn, &input("甲项目")).unwrap();
        let mut changed = input("甲项目");
        changed.customer = Some("客户乙".into());
        changed.status = "上线".into();
        crate::project::update_project(&conn, &p.id, &changed).unwrap();

        let rows = logs(&conn);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].action, "update");
        assert_eq!(rows[0].project_name, "甲项目");
        let fields: Vec<&str> = rows[0].changes.iter().map(|c| c.field.as_str()).collect();
        assert_eq!(fields, vec!["customer", "status"]);
        assert_eq!(rows[0].changes[0].old_value.as_deref(), Some("客户甲"));
        assert_eq!(rows[0].changes[0].new_value.as_deref(), Some("客户乙"));
    }

    #[test]
    fn no_op_update_writes_no_log() {
        let conn = db::open_in_memory().unwrap();
        let p = crate::project::create_project(&conn, &input("甲项目")).unwrap();
        crate::project::update_project(&conn, &p.id, &input("甲项目")).unwrap();
        assert_eq!(logs(&conn).len(), 1, "无变化编辑不得写第二行");
    }

    #[test]
    fn clearing_field_and_tags_normalization() {
        let conn = db::open_in_memory().unwrap();
        let p = crate::project::create_project(&conn, &input("甲项目")).unwrap();

        let mut clear = input("甲项目");
        clear.customer = None;
        crate::project::update_project(&conn, &p.id, &clear).unwrap();
        let rows = logs(&conn);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].changes.len(), 1);
        assert_eq!(rows[0].changes[0].field, "customer");
        assert_eq!(rows[0].changes[0].new_value, None);

        // 仅调整 tags 顺序/空格：归一化后相同，不写日志。
        let mut reorder = input("甲项目");
        reorder.customer = None;
        reorder.tags = vec![" 政务 ".into(), "运维".into(), "政务".into()];
        crate::project::update_project(&conn, &p.id, &reorder).unwrap();
        assert_eq!(logs(&conn).len(), 2, "tags 仅顺序变化不得写日志");

        let mut add = input("甲项目");
        add.customer = None;
        add.tags = vec!["政务".into(), "运维".into(), "新增".into()];
        crate::project::update_project(&conn, &p.id, &add).unwrap();
        let rows = logs(&conn);
        assert_eq!(rows[0].changes.len(), 1);
        assert_eq!(rows[0].changes[0].field, "tags");
        assert_eq!(rows[0].changes[0].old_value.as_deref(), Some("政务、运维"));
        assert_eq!(rows[0].changes[0].new_value.as_deref(), Some("政务、新增、运维"));
    }

    #[test]
    fn parent_change_uses_parent_names() {
        let conn = db::open_in_memory().unwrap();
        let parent = crate::project::create_project(&conn, &input("总包")).unwrap();
        let mut child_input = input("子项目");
        child_input.parent_id = Some(parent.id.clone());
        let child = crate::project::create_project(&conn, &child_input).unwrap();

        let mut clear = input("子项目");
        clear.parent_id = None;
        crate::project::update_project(&conn, &child.id, &clear).unwrap();
        let rows = logs(&conn);
        assert_eq!(rows[0].action, "update");
        assert_eq!(rows[0].changes.len(), 1);
        let c = &rows[0].changes[0];
        assert_eq!(c.field, "parentId");
        assert_eq!(c.old_value.as_deref(), Some("总包"));
        assert_eq!(c.new_value.as_deref(), Some("无（顶层项目）"));
    }

    #[test]
    fn delete_logs_snapshot_and_survives_soft_delete() {
        let conn = db::open_in_memory().unwrap();
        let p = crate::project::create_project(&conn, &input("甲项目")).unwrap();
        crate::project::delete_project(&conn, &p.id).unwrap();

        assert_eq!(
            crate::project::get_project(&conn, &p.id).unwrap_err().code,
            "not_found"
        );
        let rows = logs(&conn);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].action, "delete");
        assert_eq!(rows[0].project_name, "甲项目");
        assert!(rows[0].changes.iter().all(|c| c.new_value.is_none()));
        assert!(rows[0].changes.iter().any(|c| c.field == "name" && c.old_value.as_deref() == Some("甲项目")));
        assert!(rows[0].changes.iter().any(|c| c.field == "tags" && c.old_value.as_deref() == Some("政务、运维")));
        assert!(rows[0].changes.iter().any(|c| {
            c.field == "workspaceId" && c.old_value.as_deref() == Some("默认工作区")
        }));
    }

    #[test]
    fn list_filters_paginates_and_lists_deleted_projects() {
        let conn = db::open_in_memory().unwrap();
        let a = crate::project::create_project(&conn, &input("甲")).unwrap();
        let b = crate::project::create_project(&conn, &input("乙")).unwrap();
        let _c = crate::project::create_project(&conn, &input("丙")).unwrap();
        let mut a_edit = input("甲");
        a_edit.customer = Some("客户乙".into());
        crate::project::update_project(&conn, &a.id, &a_edit).unwrap();
        crate::project::delete_project(&conn, &b.id).unwrap();

        let one = list(&conn, Some(a.id.as_str()), Some("update"), Some(10), Some(0)).unwrap();
        assert_eq!(one.items.len(), 1);
        assert_eq!(one.items[0].project_id, a.id);
        assert!(!one.has_more);

        let first = list(&conn, None, None, Some(2), Some(0)).unwrap();
        assert_eq!(first.items.len(), 2);
        assert!(first.has_more);
        let second = list(&conn, None, None, Some(2), Some(2)).unwrap();
        assert_eq!(second.items.len(), 2);
        assert!(second.has_more);
        let third = list(&conn, None, None, Some(2), Some(4)).unwrap();
        assert_eq!(third.items.len(), 1);
        assert!(!third.has_more);

        let deleted = list(&conn, None, Some("delete"), Some(10), Some(0)).unwrap();
        assert_eq!(deleted.items.len(), 1);
        assert_eq!(deleted.items[0].project_id, b.id);
        assert!(deleted.projects.iter().any(|p| p.project_id == b.id && p.project_name == "乙"));
    }

    #[test]
    fn list_rejects_bad_action_and_limits() {
        let conn = db::open_in_memory().unwrap();
        assert_eq!(list(&conn, None, Some("rename"), None, None).unwrap_err().code, "invalid_input");
        assert_eq!(list(&conn, None, None, Some(0), None).unwrap_err().code, "invalid_input");
        assert_eq!(list(&conn, None, None, Some(201), None).unwrap_err().code, "invalid_input");
        assert_eq!(list(&conn, None, None, None, Some(-1)).unwrap_err().code, "invalid_input");
    }

    #[test]
    fn failed_writes_do_not_create_audit_rows() {
        let conn = db::open_in_memory().unwrap();
        let mut bad = input("   ");
        assert_eq!(crate::project::create_project(&conn, &bad).unwrap_err().code, "invalid_input");
        assert_eq!(logs(&conn).len(), 0);

        let parent = crate::project::create_project(&conn, &input("总包")).unwrap();
        let mut child_input = input("子项目");
        child_input.parent_id = Some(parent.id.clone());
        crate::project::create_project(&conn, &child_input).unwrap();
        assert_eq!(
            crate::project::delete_project(&conn, &parent.id).unwrap_err().code,
            "invalid_input"
        );
        assert_eq!(logs(&conn).len(), 2, "失败删除不得写日志");

        bad.status = "已上线".into();
        assert_eq!(
            crate::project::update_project(&conn, &parent.id, &bad).unwrap_err().code,
            "invalid_input"
        );
        assert_eq!(logs(&conn).len(), 2, "失败编辑不得写日志");
    }

    #[test]
    fn audit_insert_failure_rolls_back_project_write() {
        let conn = db::open_in_memory().unwrap();
        conn.execute("DROP TABLE project_audit_logs", []).unwrap();
        let err = crate::project::create_project(&conn, &input("回滚项目")).unwrap_err();
        assert_eq!(err.code, "db_failed");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "日志写失败必须把项目写一起回滚");
    }

    /// 根换区 ⇒ 恰一条 update 日志、「工作区」那条存两区中文名、随迁的后代不各自出日志。
    #[test]
    fn workspace_change_is_logged_once_for_root() {
        let conn = db::open_in_memory().unwrap();
        let root = crate::project::create_project(&conn, &input("总包甲")).unwrap();
        let mut child_in = input("子项目乙");
        child_in.parent_id = Some(root.id.clone());
        let child = crate::project::create_project(&conn, &child_in).unwrap();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();

        let mut move_in = input("总包甲");
        move_in.workspace_id = Some(b.id.clone());
        crate::project::update_project(&conn, &root.id, &move_in).unwrap();

        let page = list(&conn, None, Some("update"), None, None).unwrap();
        let moves: Vec<&AuditEntry> = page
            .items
            .iter()
            .filter(|e| {
                e.changes
                    .iter()
                    .any(|c| c.field == "workspaceId")
            })
            .collect();
        assert_eq!(moves.len(), 1, "一次换区只能有一条带工作区差异的日志：{}", page.items.len());
        assert_eq!(moves[0].project_id, root.id, "日志挂在被编辑的根项目下");
        let c = &moves[0].changes[0];
        assert_eq!((c.old_value.as_deref(), c.new_value.as_deref()), (Some("默认工作区"), Some("客户乙")), "存显示名不是裸 id：{:?}", c);

        let child_logs = list(&conn, Some(&child.id), Some("update"), None, None).unwrap();
        assert!(child_logs.items.is_empty(), "随迁的后代不许各自出日志：{}", child_logs.items.len());
    }

    /// 日志页与项目下拉都只出活动区。被硬删的项目行（这里用裸 SQL 造）它的日志从所有区视野消失
    /// —— M14 的「永久保留」针对行不针对可见性（spec §七 已点名，不藏）。
    #[test]
    fn audit_list_and_options_are_scoped_to_active_workspace() {
        let conn = db::open_in_memory().unwrap();
        let a = crate::project::create_project(&conn, &input("默认区项目")).unwrap();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        // 乙区的项目只能靠「切过去再建」得到：create 恒落活动区，input 里的区号会被忽略（§六.1）。
        crate::workspace::switch(&conn, &b.id).unwrap();
        let bp = crate::project::create_project(&conn, &input("乙区项目")).unwrap();
        crate::workspace::switch(&conn, crate::workspace::DEFAULT_ID).unwrap();

        let page = list(&conn, None, None, None, None).unwrap();
        let ids: Vec<&str> = page.items.iter().map(|e| e.project_id.as_str()).collect();
        assert!(ids.contains(&a.id.as_str()), "默认区日志要在：{ids:?}");
        assert!(!ids.contains(&bp.id.as_str()), "别区日志不能漏进来：{ids:?}");
        let opts: Vec<&str> = page.projects.iter().map(|p| p.project_id.as_str()).collect();
        assert_eq!(opts, vec![a.id.as_str()], "项目下拉只出活动区有日志的项目：{opts:?}");

        // 硬删的可见性必须在**乙区视野**里验：留在默认区删前删后都看不见 bp，那一步是恒真的。
        crate::workspace::switch(&conn, &b.id).unwrap();
        assert!(
            list(&conn, None, None, None, None)
                .unwrap()
                .items
                .iter()
                .any(|e| e.project_id == bp.id),
            "乙区视野里必须能看见 bp 的日志"
        );
        conn.execute("DELETE FROM projects WHERE id = ?1", [&bp.id]).unwrap();
        let after = list(&conn, None, None, None, None).unwrap();
        assert!(
            !after.items.iter().any(|e| e.project_id == bp.id),
            "硬删项目行后日志从该区视野消失"
        );
    }
}
