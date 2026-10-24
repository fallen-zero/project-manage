//! 工作区（M15）。一张 `workspaces` + `settings.active_workspace_id` 一个当前区。
//! **隔离是逻辑分区，不是访问控制**：同库同进程，拿到 `ledger.db` 的人看得到全部区
//! （spec §十二.6 要求这句写在模块头，别让「隔离」两个字被读成加密）。

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashSet;

use crate::error::{AppError, AppResult};

/// 默认区的 id 与显示名：V11 种的就是这两行，`projects.workspace_id` 的列 DEFAULT 也是这个 id。
pub const DEFAULT_ID: &str = "default";
pub const ACTIVE_KEY: &str = "active_workspace_id";
/// 区名要进顶栏那个下拉，超长会把 header 撑破。只在后端判一次（本项目只在边界校验一次）。
const NAME_MAX_CHARS: usize = 40;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// 只数未软删的项目：管理弹窗里那个数字是「删不删得掉」的判据之一，
    /// 而 §五 的删除守卫**含软删行**，两者口径不同是刻意的（软删行仍占着区，不能让区先删掉）。
    pub project_count: i64,
    pub is_active: bool,
}

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查表单必填项与取值范围"))
}

/// 跨区与不存在共用这一条：一个 crafted id 得到的答复和 id 打错完全一样（spec §四）。
/// hint 里点名工作区，因为界面最常见的真成因就是「刚切过区」。
fn not_found(what: &str, id: &str) -> AppError {
    AppError::new(
        "not_found",
        &format!("{what}不存在：{id}"),
        Some("刷新后重试；如果刚切换过工作区，这条数据在别的工作区里"),
    )
}

/// 当前活动区 id。
pub fn active_id(conn: &Connection) -> AppResult<String> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [ACTIVE_KEY],
            |r| r.get(0),
        )
        .optional()?;
    // 读不到只可能是有人手工 `DELETE FROM settings`（V11 恒种这一行）。这里报错会让每条命令
    // 都失败、整个界面打不开；回默认区至少还看得见数据（spec §4.3，T-5 钉这一支）。
    Ok(value.unwrap_or_else(|| DEFAULT_ID.to_owned()))
}

/// 把当前区写成 `id`。**不校验存在性**：调用方分两类 —— `switch` 自己先查过（§五），
/// Task 4 的换区跟随传的是它刚用 `exists` 查过的目标区。这一层是写，不是门。
pub fn set_active(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![ACTIVE_KEY, id],
    )?;
    Ok(())
}

pub fn exists(conn: &Connection, id: &str) -> AppResult<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM workspaces WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}

/// 区名，给操作日志的「工作区」字段用（spec §七：日志里存显示名，不存裸 id）。
pub fn name_of(conn: &Connection, id: &str) -> AppResult<String> {
    let name: Option<String> = conn
        .query_row("SELECT name FROM workspaces WHERE id = ?1", [id], |r| r.get(0))
        .optional()?;
    // 有项目的区删不掉（§五 守卫③），所以这一支在命令路径上实际不可达；写它是为了让
    // 日志的 changes 永远不含裸 id。形制照 `project_audit.rs:72 parent_name` 的「已删除项目」。
    Ok(name.unwrap_or_else(|| "已删除工作区".to_owned()))
}

/// 名字判据：trim 非空、≤ 40 字符。返回 trim 后的显示值。
fn check_name(name: &str) -> AppResult<String> {
    let trimmed = name.trim().to_owned();
    if trimmed.is_empty() {
        return Err(invalid("工作区名称不能为空"));
    }
    if trimmed.chars().count() > NAME_MAX_CHARS {
        return Err(invalid(&format!(
            "工作区名称不能超过 {} 个字符",
            NAME_MAX_CHARS
        )));
    }
    Ok(trimmed)
}

/// 重名先查再插：`workspaces.name` 上有 UNIQUE 兜底，但撞它得到的是 `db_failed`，
/// 用户看不出是自己打重了名（与 `project.rs` 把状态 CHECK 提到应用层判是同一条理由）。
/// `exclude_id` 给改名用：允许把区改成它自己现在就叫的名字。
fn name_taken(conn: &Connection, name: &str, exclude_id: Option<&str>) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM workspaces WHERE name = ?1 AND (?2 IS NULL OR id <> ?2)",
        params![name, exclude_id],
        |r| r.get(0),
    )?;
    if n > 0 {
        return Err(invalid("工作区名称已存在"));
    }
    Ok(())
}

/// 单行读回。给 `create`/`rename` 的返回值用：不自己拼 `Workspace`，
/// 免得「新建的区到底是当前区吗」这类问题出现两个真值。
fn row(conn: &Connection, id: &str) -> AppResult<Workspace> {
    let active = active_id(conn)?;
    conn.query_row(
        "SELECT w.id, w.name,
                (SELECT COUNT(*) FROM projects p
                  WHERE p.workspace_id = w.id AND p.deleted_at IS NULL),
                w.id = ?2
           FROM workspaces w WHERE w.id = ?1",
        params![id, active],
        |r| {
            Ok(Workspace {
                id: r.get(0)?,
                name: r.get(1)?,
                project_count: r.get(2)?,
                is_active: r.get(3)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| not_found("工作区", id))
}

pub fn list(conn: &Connection) -> AppResult<Vec<Workspace>> {
    let active = active_id(conn)?;
    let mut stmt = conn.prepare(
        "SELECT w.id, w.name,
                (SELECT COUNT(*) FROM projects p
                  WHERE p.workspace_id = w.id AND p.deleted_at IS NULL)
           FROM workspaces w ORDER BY w.created_at, w.id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Workspace {
                id: r.get(0)?,
                name: r.get(1)?,
                project_count: r.get(2)?,
                is_active: false,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    Ok(rows
        .into_iter()
        .map(|w| Workspace { is_active: w.id == active, ..w })
        .collect())
}

/// 新建区。**不自动切过去**（spec §五：先看列表，切不切由人决定）。
pub fn create(conn: &Connection, name: &str) -> AppResult<Workspace> {
    let name = check_name(name)?;
    name_taken(conn, &name, None)?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute("INSERT INTO workspaces (id, name) VALUES (?1, ?2)", params![id, name])?;
    row(conn, &id)
}

/// 改名。默认区可以改名，只是不可删（D3）。
pub fn rename(conn: &Connection, id: &str, name: &str) -> AppResult<Workspace> {
    if !exists(conn, id)? {
        return Err(not_found("工作区", id));
    }
    let name = check_name(name)?;
    name_taken(conn, &name, Some(id))?;
    conn.execute("UPDATE workspaces SET name = ?2 WHERE id = ?1", params![id, name])?;
    row(conn, id)
}

/// 删除。三道守卫的顺序就是文案的顺序（spec §五）：默认区 → 当前区 → 非空。
pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    if !exists(conn, id)? {
        return Err(not_found("工作区", id));
    }
    if id == DEFAULT_ID {
        return Err(invalid("默认工作区不可删除"));
    }
    if id == active_id(conn)? {
        return Err(invalid("当前工作区不可删除，请先切换到其他工作区"));
    }
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE workspace_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    if n > 0 {
        return Err(invalid(&format!(
            "该工作区下还有 {n} 个项目（含已删除项目），不能删除"
        )));
    }
    conn.execute("DELETE FROM workspaces WHERE id = ?1", [id])?;
    Ok(())
}

/// 切换。只认存在的区，配合 `delete` 的「当前区不可删」，`settings` 里的指针永不悬空（§4.3）。
pub fn switch(conn: &Connection, id: &str) -> AppResult<String> {
    if !exists(conn, id)? {
        return Err(not_found("工作区", id));
    }
    set_active(conn, id)?;
    Ok(id.to_owned())
}

// ---- 三件读侧门：`ensure_active_project` / `ensure_active_row` 已在 Task 5 接到生产路径
// （台账、四个关联表模块与三个目录 mutator）；`active_project_ids` 留给 Task 6 的
// `index_overview` 按区过滤，届时删掉它最后这一处豁免。
// 注释里不许出现那个方括号字面量：豁免门是 grep -rn 数行数（与 async 门同款坑，Task 2 评审抓到）。
/// 断言项目属于活动区且未软删。所有「按 project_id 挂新数据」的 create 路径靠它（spec §4.2）。
pub fn ensure_active_project(conn: &Connection, project_id: &str) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects
          WHERE id = ?1 AND deleted_at IS NULL AND workspace_id = ?2",
        params![project_id, active_id(conn)?],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(not_found("项目", project_id));
    }
    Ok(())
}

/// 断言某一行的宿主项目属于活动区。`table` 只接编译期常量（与 `ledger.rs:212 soft_delete`
/// 同一种「表名由调用方以常量传入」的写法），九张表都有 `project_id` 与 `deleted_at` 两列。
/// **不加 `p.deleted_at IS NULL`**：spec §四 的 SQL 就是这个形状，软删项目下的行仍可靠地
/// 属于那个区，多半个谓词只会让读路径与写路径口径分叉。
pub fn ensure_active_row(conn: &Connection, table: &'static str, id: &str) -> AppResult<()> {
    let n: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM {table} t
               JOIN projects p ON p.id = t.project_id
              WHERE t.id = ?1 AND t.deleted_at IS NULL AND p.workspace_id = ?2"
        ),
        params![id, active_id(conn)?],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(not_found("记录", id));
    }
    Ok(())
}

/// 活动区里**含软删**的项目 id 集合。给 `index_overview` 过滤用（spec §4.1：
/// worker 照旧全库扫，只有呈现层按区收窄）。
#[allow(dead_code)]
pub fn active_project_ids(conn: &Connection) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM projects WHERE workspace_id = ?1")?;
    let ids = stmt
        .query_map(params![active_id(conn)?], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, project};

    /// 活动区兜底 + 守卫能不能真的挡：这一支同时钉 §4.3（读不到设置行回 default）
    /// 与 §4.2（跨区项目读不到）。设置行被删掉时如果 `active_id` 报错，整条命令链全挂。
    #[test]
    fn active_id_defaults_when_the_setting_row_is_gone() {
        let conn = db::open_in_memory().unwrap();
        assert_eq!(active_id(&conn).unwrap(), DEFAULT_ID, "V11 种好后直接读要得 default");

        conn.execute("DELETE FROM settings WHERE key = ?1", [ACTIVE_KEY])
            .unwrap();
        assert_eq!(active_id(&conn).unwrap(), DEFAULT_ID, "设置行被手工删掉时必须兜回默认区");

        let p = project::create_project(&conn, &project_input("兜底项目")).unwrap();
        assert_eq!(p.workspace_id, DEFAULT_ID);
        ensure_active_project(&conn, &p.id).unwrap();
    }

    fn project_input(name: &str) -> project::ProjectInput {
        project::ProjectInput {
            name: name.into(),
            code: None,
            manager: None,
            customer: None,
            contact_name: None,
            contact_phone: None,
            contract_no: None,
            contract_period: None,
            delivery_deadline: None,
            status: "立项".into(),
            parent_id: None,
            workspace_id: None,
            tags: vec![],
        }
    }

    #[test]
    fn create_rejects_blank_duplicate_and_overlong_names() {
        let conn = db::open_in_memory().unwrap();
        assert_eq!(create(&conn, "   ").unwrap_err().code, "invalid_input", "空名要应用层码，不是 db_failed");
        assert_eq!(create(&conn, "客户乙").unwrap().name, "客户乙");
        // 显示值要去空白：区名进顶栏下拉，前后空格会让人以为是两个区。
        assert_eq!(create(&conn, "  客户丙  ").unwrap().name, "客户丙");
        assert_eq!(create(&conn, "客户乙").unwrap_err().code, "invalid_input", "重名要回人话，不是 UNIQUE 撞出来的 db_failed");
        let long = "区".repeat(NAME_MAX_CHARS + 1);
        assert_eq!(create(&conn, &long).unwrap_err().code, "invalid_input");
        // 简报这里原写 `assert_eq!(….is_ok(), "…")`（bool 与 &str 比，编不过）：
        // 第二个实参是断言消息，`assert_eq!` 只有三个实参时才吃消息。改 `assert!` 与本仓
        // `lib.rs` 的既有口径一致（`assert_eq!(x, true)` 会撞 clippy::bool_assert_comparison）。
        assert!(create(&conn, &"区".repeat(NAME_MAX_CHARS)).is_ok(), "{NAME_MAX_CHARS} 字是闭区间");
    }

    #[test]
    fn rename_and_switch_and_delete_on_unknown_id_are_not_found() {
        let conn = db::open_in_memory().unwrap();
        for err in [
            rename(&conn, "ws-ghost", "新名字").unwrap_err(),
            switch(&conn, "ws-ghost").unwrap_err(),
            delete(&conn, "ws-ghost").unwrap_err(),
        ] {
            assert_eq!(err.code, "not_found", "未知 id 一律 not_found：{:?}", err.code);
        }
    }

    #[test]
    fn switch_writes_and_reads_back_and_is_idempotent() {
        let conn = db::open_in_memory().unwrap();
        let ws = create(&conn, "客户乙").unwrap();
        assert_eq!(switch(&conn, &ws.id).unwrap(), ws.id);
        assert_eq!(active_id(&conn).unwrap(), ws.id, "切换要真的落到 settings 并被读回");
        assert_eq!(switch(&conn, &ws.id).unwrap(), ws.id, "重复切同一个区幂等成功");
        assert_eq!(switch(&conn, DEFAULT_ID).unwrap(), DEFAULT_ID);
        assert_eq!(active_id(&conn).unwrap(), DEFAULT_ID);

        // `create`/`rename` 的返回值走 `row()`，那一支的 is_active 与 `list()` 是两处 SQL，各钉一次
        // （Task 2 评审点名 `row()` 的 is_active 无测试）。
        assert!(!create(&conn, "新建未激活区").unwrap().is_active, "新建区不该是活动区（row() 那一支）");
        let cur = create(&conn, "当前改名区").unwrap();
        switch(&conn, &cur.id).unwrap();
        assert!(
            rename(&conn, &cur.id, "改名后仍活动").unwrap().is_active,
            "改名要读回当前区标记（row() 那一支）"
        );
    }

    #[test]
    fn delete_refuses_default_active_and_non_empty_workspaces() {
        let conn = db::open_in_memory().unwrap();
        // ①默认区：`projects.workspace_id` 的列 DEFAULT 指向它，删掉等于给孤儿列找不着家。
        assert_eq!(delete(&conn, DEFAULT_ID).unwrap_err().code, "invalid_input");
        assert!(exists(&conn, DEFAULT_ID).unwrap());

        // ③非空区：守卫含软删行 —— 软删项目仍占着 workspace_id，先删区会让它的日志从所有视野消失。
        let ws = create(&conn, "有项目的区").unwrap();
        let p = project::create_project(&conn, &project_input("区内项目")).unwrap();
        conn.execute(
            "UPDATE projects SET workspace_id = ?1 WHERE id = ?2",
            params![ws.id, p.id],
        )
        .unwrap();
        assert_eq!(delete(&conn, &ws.id).unwrap_err().code, "invalid_input");
        // 软删走裸 SQL，不用 `project::delete_project`：Task 3 之后 `get_project` 按活动区过滤，
        // 这个项目的区是「有项目的区」而活动区还是默认区，走命令路径会先 not_found，守卫③那一支就测不到。
        conn.execute(
            "UPDATE projects SET deleted_at = datetime('now') WHERE id = ?1",
            [p.id],
        )
        .unwrap();
        assert_eq!(delete(&conn, &ws.id).unwrap_err().code, "invalid_input", "软删行也算非空");

        // ②当前区：切走才让删。
        let empty = create(&conn, "空区").unwrap();
        switch(&conn, &empty.id).unwrap();
        assert_eq!(delete(&conn, &empty.id).unwrap_err().code, "invalid_input");
        switch(&conn, DEFAULT_ID).unwrap();
        delete(&conn, &empty.id).unwrap();
        assert!(!exists(&conn, &empty.id).unwrap(), "空区删除是硬删 workspaces 行");
        assert!(exists(&conn, &ws.id).unwrap(), "别的区不受影响");
    }

    #[test]
    fn list_reports_project_count_and_marks_the_active_row() {
        let conn = db::open_in_memory().unwrap();
        // 简报原样跑这条会随机红（约 13/16）：`created_at` 是 `datetime('now')`，**秒级**精度，
        // 同一次测试里 V11 种的 default 与刚建的区必定同秒 ⇒ 落到 `ORDER BY` 的第二键 `id`，
        // 而 uuid 首字符小于 'd' 时它就排在 default 前面。定序判据本身（§五 的 `created_at, id`）
        // 不动，只把 default 的时间戳钉到过去，让「按 created_at 定序」这一支被确定地测到。
        conn.execute(
            "UPDATE workspaces SET created_at = '2000-01-01 00:00:00' WHERE id = ?1",
            [DEFAULT_ID],
        )
        .unwrap();
        let b = create(&conn, "客户乙").unwrap();
        let rows = list(&conn).unwrap();
        assert_eq!(rows[0].id, DEFAULT_ID, "default 恒在最前（按 created_at, id 定序）");
        assert_eq!(rows.len(), 2, "新建的区要出现在列表里，且**不自动跳过去**（spec §五）");
        assert!(!rows.iter().find(|w| w.id == b.id).unwrap().is_active);
        assert!(rows.iter().find(|w| w.id == DEFAULT_ID).unwrap().is_active);

        let p = project::create_project(&conn, &project_input("区内项目")).unwrap();
        conn.execute(
            "UPDATE projects SET workspace_id = ?1 WHERE id = ?2",
            params![b.id, p.id],
        )
        .unwrap();
        project::create_project(&conn, &project_input("区内项目乙")).unwrap();
        // 软删走裸 SQL，和上面 `delete_refuses_…` 那处同一理由（Task 3 起 `get_project` 按活动区
        // 过滤）：p 已被挪去乙区而活动区还是默认区，走 `project::delete_project` 会先 not_found，
        // `project_count 只数未软删` 这一支就测不到。
        conn.execute(
            "UPDATE projects SET deleted_at = datetime('now') WHERE id = ?1",
            [p.id],
        )
        .unwrap();
        let rows = list(&conn).unwrap();
        assert_eq!(rows.iter().find(|w| w.id == b.id).unwrap().project_count, 0, "project_count 只数未软删");
        assert_eq!(rows.iter().find(|w| w.id == DEFAULT_ID).unwrap().project_count, 1);
    }

    #[test]
    fn ensure_active_row_rejects_a_foreign_workspace_row() {
        let conn = db::open_in_memory().unwrap();
        let p = project::create_project(&conn, &project_input("默认区项目")).unwrap();
        let t = crate::todo::create(
            &conn,
            &crate::todo::TodoInput {
                project_id: p.id.clone(),
                title: "写点东西".to_owned(),
                due_date: None,
            },
        )
        .unwrap();
        // 先在活动区内验一次「放行」，否则下面「跨区被拒」可能因为 id 根本不存在而假绿。
        ensure_active_row(&conn, "todos", &t.id).unwrap();

        // 裸 SQL 把宿主项目整体挪走，等价于「这条行属于别区」（此刻行还在库里）。
        conn.execute(
            "UPDATE projects SET workspace_id = 'other' WHERE id = ?1",
            [p.id.clone()],
        )
        .unwrap();
        assert_eq!(
            ensure_active_row(&conn, "todos", &t.id).unwrap_err().code,
            "not_found",
            "跨区行必须与不存在的行同码（spec §四）"
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM todos WHERE id = ?1", [&t.id], |r| r.get::<_, i64>(0))
                .unwrap(),
            1,
            "守卫只挡可见性，不许顺手删行"
        );
    }
}
