//! 库内字段检索：在项目档案与台账各表的**明文列**上做子串匹配。
//!
//! 密文列（`*_cipher` / `*_nonce`）根本不出现在 SQL 里，所以「用密码去搜索不到」
//! 是结构性的，而不是查出来再过滤 —— 后者只要漏一处就泄漏。
//!
//! 结果是一条扁平列表，每条自带 `source`（哪一类）与 `project_id`（属于哪个项目），
//! 分组与聚合交给 M4 的首屏结果页做，这里不预先按分数排序：SQL 匹配与 FTS 的 BM25
//! 分数不可比，硬归一化只会造假。

use rusqlite::{params, Connection, Row};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::tokenize::MAX_QUERY_CHARS;

/// 每组最多返回这么多条：命中几千条时把整表灌进前端没有意义，界面也翻不完。
const PER_GROUP_LIMIT: i64 = 50;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldHit {
    /// project | env | credential | server | link | note
    pub source: &'static str,
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub title: String,
    /// 一行副标题，只由明文列拼成
    pub detail: String,
}

/// LIKE 的元字符必须转义，否则用户敲一个 `%` 就把全表都当成命中。
fn like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

fn join_bits(bits: [Option<String>; 3]) -> String {
    bits.into_iter()
        .flatten()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

type Cells = (String, String, String, String, Option<String>, Option<String>, Option<String>);

fn map_row(r: &Row<'_>) -> rusqlite::Result<Cells> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
    ))
}

fn collect(source: &'static str, sql: &str, conn: &Connection, pattern: &str) -> AppResult<Vec<FieldHit>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![pattern, PER_GROUP_LIMIT], map_row)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, project_id, project_name, title, b1, b2, b3) = row?;
        out.push(FieldHit {
            source,
            id,
            project_id,
            project_name,
            title,
            detail: join_bits([b1, b2, b3]),
        });
    }
    Ok(out)
}

/// 空查询直接返回空：不扫库，也避免首屏一进来就发六条全表 LIKE。
pub fn field_hits(conn: &Connection, query: &str) -> AppResult<Vec<FieldHit>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err(AppError::new(
            "invalid_input",
            "搜索关键词过长",
            Some("库内字段检索只吃短词，找正文请用文档检索"),
        ));
    }
    let p = like_pattern(q);
    let mut hits = Vec::new();

    hits.append(&mut collect(
        "project",
        "SELECT p.id, p.id, p.name, p.name, p.code, p.customer, p.status
           FROM projects p
          WHERE p.deleted_at IS NULL
            AND (  p.name LIKE ?1 ESCAPE '\\'
                OR p.code LIKE ?1 ESCAPE '\\'
                OR p.manager LIKE ?1 ESCAPE '\\'
                OR p.customer LIKE ?1 ESCAPE '\\'
                OR p.contact_name LIKE ?1 ESCAPE '\\'
                OR p.contact_phone LIKE ?1 ESCAPE '\\'
                OR p.contract_no LIKE ?1 ESCAPE '\\'
                OR p.contract_period LIKE ?1 ESCAPE '\\'
                OR p.delivery_deadline LIKE ?1 ESCAPE '\\'
                OR p.status LIKE ?1 ESCAPE '\\'
                OR EXISTS (SELECT 1 FROM project_tags t WHERE t.project_id = p.id AND t.tag LIKE ?1 ESCAPE '\\'))
          ORDER BY p.updated_at DESC
          LIMIT ?2",
        conn,
        &p,
    )?);

    hits.append(&mut collect(
        "env",
        "SELECT e.id, e.project_id, p.name, e.name, e.env, e.url, e.port
           FROM ledger_envs e
           JOIN projects p ON p.id = e.project_id AND p.deleted_at IS NULL
          WHERE e.deleted_at IS NULL
            AND (  e.name LIKE ?1 ESCAPE '\\'
                OR e.url LIKE ?1 ESCAPE '\\'
                OR e.port LIKE ?1 ESCAPE '\\'
                OR e.note LIKE ?1 ESCAPE '\\')
          ORDER BY e.updated_at DESC
          LIMIT ?2",
        conn,
        &p,
    )?);

    // 凭据：只搜用途标题、登录地址、备注。username/password 是密文列，不在 SQL 里。
    hits.append(&mut collect(
        "credential",
        "SELECT c.id, c.project_id, p.name, c.title, c.url, c.note, NULL
           FROM ledger_credentials c
           JOIN projects p ON p.id = c.project_id AND p.deleted_at IS NULL
          WHERE c.deleted_at IS NULL
            AND (  c.title LIKE ?1 ESCAPE '\\'
                OR c.url LIKE ?1 ESCAPE '\\'
                OR c.note LIKE ?1 ESCAPE '\\')
          ORDER BY c.updated_at DESC
          LIMIT ?2",
        conn,
        &p,
    )?);

    // 服务器：远程账号与密码同样是密文列，只搜名称/IP/堡垒机/备注。
    hits.append(&mut collect(
        "server",
        "SELECT s.id, s.project_id, p.name, s.name, s.ip, s.bastion, s.note
           FROM ledger_servers s
           JOIN projects p ON p.id = s.project_id AND p.deleted_at IS NULL
          WHERE s.deleted_at IS NULL
            AND (  s.name LIKE ?1 ESCAPE '\\'
                OR s.ip LIKE ?1 ESCAPE '\\'
                OR s.bastion LIKE ?1 ESCAPE '\\'
                OR s.note LIKE ?1 ESCAPE '\\')
          ORDER BY s.updated_at DESC
          LIMIT ?2",
        conn,
        &p,
    )?);

    hits.append(&mut collect(
        "link",
        "SELECT l.id, l.project_id, p.name, l.title, l.kind, l.url, l.note
           FROM ledger_links l
           JOIN projects p ON p.id = l.project_id AND p.deleted_at IS NULL
          WHERE l.deleted_at IS NULL
            AND (  l.title LIKE ?1 ESCAPE '\\'
                OR l.url LIKE ?1 ESCAPE '\\'
                OR l.note LIKE ?1 ESCAPE '\\')
          ORDER BY l.updated_at DESC
          LIMIT ?2",
        conn,
        &p,
    )?);

    // 备注正文可能很长，副标题只取前 80 字，整段留给详情页看。
    hits.append(&mut collect(
        "note",
        "SELECT n.id, n.project_id, p.name, n.title, n.tag, SUBSTR(n.body, 1, 80), NULL
           FROM ledger_notes n
           JOIN projects p ON p.id = n.project_id AND p.deleted_at IS NULL
          WHERE n.deleted_at IS NULL
            AND (  n.title LIKE ?1 ESCAPE '\\'
                OR n.tag LIKE ?1 ESCAPE '\\'
                OR n.body LIKE ?1 ESCAPE '\\')
          ORDER BY n.updated_at DESC
          LIMIT ?2",
        conn,
        &p,
    )?);

    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, ledger, project, vault};
    use rusqlite::Connection;

    const SECRET: &str = "机密串-Zhang@2026";
    const MAIN_PW: &str = "主密码-至少八位";

    /// 每个用例一套内存库 + 已解锁的主密钥 + 一个项目。
    fn fixture() -> (Connection, String, vault::MasterKey) {
        let conn = db::open_in_memory().unwrap();
        let code = vault::generate_recovery_code().unwrap();
        let mk = vault::initialize(&conn, MAIN_PW, &code).unwrap();
        let pid = seed_project(&conn, "政务云迁移");
        (conn, pid, mk)
    }

    fn seed_project(conn: &Connection, name: &str) -> String {
        project::create_project(
            conn,
            &project::ProjectInput {
                name: name.to_owned(),
                code: Some("P-2026-009".to_owned()),
                manager: Some("李工".to_owned()),
                customer: Some("市大数据中心".to_owned()),
                contact_name: None,
                contact_phone: None,
                contract_no: None,
                contract_period: None,
                delivery_deadline: None,
                status: "开发".to_owned(),
                tags: vec!["政务".to_owned(), "维保中".to_owned()],
            },
        )
        .unwrap()
        .id
    }

    /// 五类台账各一条，其中凭据与服务器的敏感列填的是 SECRET，明文凭据列填的是别的词。
    fn seed_ledger(conn: &Connection, pid: &str, mk: &vault::MasterKey) {
        ledger::create_env(
            conn,
            pid,
            &ledger::EnvInput {
                env: "prod".to_owned(),
                name: "生产门户".to_owned(),
                url: Some("https://prod.example.gov.cn".to_owned()),
                port: Some("8443".to_owned()),
                note: Some("只读入口".to_owned()),
            },
        )
        .unwrap();
        ledger::create_credential(
            conn,
            mk,
            pid,
            &ledger::CredentialInput {
                env_id: None,
                title: "运维后台".to_owned(),
                username: Some(SECRET.to_owned()),
                password: Some(SECRET.to_owned()),
                url: Some("https://ops.example.gov.cn".to_owned()),
                note: Some("交付时用".to_owned()),
            },
        )
        .unwrap();
        ledger::create_server(
            conn,
            mk,
            pid,
            &ledger::ServerInput {
                name: "应用服务器".to_owned(),
                ip: Some("10.20.30.40".to_owned()),
                bastion: Some("跳板机A".to_owned()),
                account: Some(SECRET.to_owned()),
                password: Some(SECRET.to_owned()),
                note: None,
            },
        )
        .unwrap();
        ledger::create_link(
            conn,
            pid,
            &ledger::LinkInput {
                kind: "jira".to_owned(),
                title: "缺陷看板".to_owned(),
                url: "https://jira.example.gov.cn/browse/YUN".to_owned(),
                note: None,
            },
        )
        .unwrap();
        ledger::create_note(
            conn,
            pid,
            &ledger::NoteInput {
                title: "接口坑点".to_owned(),
                body: Some("单点登录回调地址要带租户参数。".to_owned()),
                tag: Some("遗留".to_owned()),
            },
        )
        .unwrap();
    }

    fn sources(hits: &[FieldHit]) -> Vec<&'static str> {
        hits.iter().map(|h| h.source).collect()
    }

    fn only_hit(conn: &Connection, needle: &str, source: &str, title: &str) -> FieldHit {
        let hits = field_hits(conn, needle).unwrap();
        assert_eq!(sources(&hits), vec![source], "「{needle}」应该只命中一条 {source}");
        assert_eq!(hits[0].title, title);
        hits.into_iter().next().unwrap()
    }

    #[test]
    fn every_group_is_searchable_and_carries_its_owning_project() {
        let (conn, pid, mk) = fixture();
        seed_ledger(&conn, &pid, &mk);

        for (needle, source, title) in [
            ("生产门户", "env", "生产门户"),
            ("运维后台", "credential", "运维后台"),
            ("应用服务器", "server", "应用服务器"),
            ("缺陷看板", "link", "缺陷看板"),
            ("接口坑点", "note", "接口坑点"),
            ("政务云迁移", "project", "政务云迁移"),
        ] {
            let hit = only_hit(&conn, needle, source, title);
            assert_eq!(hit.project_id, pid, "「{needle}」要带上所属项目 id");
            assert_eq!(hit.project_name, "政务云迁移", "「{needle}」要带上项目名，供结果页分组");
            assert!(!hit.detail.is_empty(), "「{needle}」的副标题不该为空");
        }
    }

    /// 这是 M2 的验收点：拿敏感字段的明文去搜，一条都不该命中。
    /// 保障来自「SQL 里根本不写密文列」，不是来自事后过滤。
    #[test]
    fn cipher_columns_never_match_even_though_the_plaintext_is_stored() {
        let (conn, pid, mk) = fixture();
        seed_ledger(&conn, &pid, &mk);

        let cred_id = conn
            .query_row(
                "SELECT id FROM ledger_credentials WHERE title = '运维后台'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap();
        let stored = ledger::reveal(&conn, &mk, "credential", &cred_id, "password").unwrap();
        assert_eq!(stored, SECRET, "前置条件：密文列里解出来的确实是这个词");

        assert!(
            field_hits(&conn, SECRET).unwrap().is_empty(),
            "用敏感字段明文检索不应该命中任何行"
        );
        // 同一行不是搜不到，而是只能靠明文列搜到
        assert_eq!(sources(&field_hits(&conn, "交付时用").unwrap()), vec!["credential"]);
    }

    #[test]
    fn plain_text_columns_are_the_only_ones_a_hit_can_show() {
        let (conn, pid, mk) = fixture();
        seed_ledger(&conn, &pid, &mk);
        let hit = only_hit(&conn, "10.20.30.40", "server", "应用服务器");
        assert!(hit.detail.contains("跳板机A"), "副标题由明文列拼出：{}", hit.detail);
        assert!(!hit.detail.contains(SECRET), "副标题里不能混进敏感列的值");
        assert!(!hit.detail.contains("•••"), "副标题不该是占位符");
    }

    #[test]
    fn tags_and_project_fields_are_searchable() {
        let (conn, pid, mk) = fixture();
        seed_ledger(&conn, &pid, &mk);
        assert_eq!(sources(&field_hits(&conn, "租户参数").unwrap()), vec!["note"]);
        assert_eq!(sources(&field_hits(&conn, "遗留").unwrap()), vec!["note"]);
        assert_eq!(sources(&field_hits(&conn, "维保中").unwrap()), vec!["project"]);
        assert_eq!(sources(&field_hits(&conn, "P-2026-009").unwrap()), vec!["project"]);
        assert_eq!(sources(&field_hits(&conn, "市大数据").unwrap()), vec!["project"]);
    }

    /// LIKE 的元字符必须转义，否则一个 % 就等于全表命中，检索结果会自欺欺人。
    #[test]
    fn like_wildcards_in_the_query_are_treated_as_literal() {
        let (conn, pid, mk) = fixture();
        seed_ledger(&conn, &pid, &mk);
        let all = field_hits(&conn, "a").unwrap().len();
        assert!(all > 0, "先确认库里有能被单字母命中的行");
        assert!(field_hits(&conn, "%").unwrap().is_empty(), "一个 % 不该等于全表命中");
        assert!(field_hits(&conn, "_").unwrap().is_empty(), "一个 _ 不该等于全表命中");
        // 反斜杠参与转义，写错了会直接 SQL 语法错，这里要求它只是「没命中」
        assert!(field_hits(&conn, "\\%").unwrap().is_empty());
    }

    #[test]
    fn blank_query_scans_nothing() {
        let (conn, pid, mk) = fixture();
        seed_ledger(&conn, &pid, &mk);
        assert!(field_hits(&conn, "").unwrap().is_empty());
        assert!(field_hits(&conn, "   ").unwrap().is_empty());
    }

    #[test]
    fn deleted_rows_and_deleted_projects_drop_out_of_hits() {
        let (conn, pid, mk) = fixture();
        seed_ledger(&conn, &pid, &mk);
        assert!(!field_hits(&conn, "生产门户").unwrap().is_empty());

        let env_id = conn
            .query_row("SELECT id FROM ledger_envs WHERE name = '生产门户'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap();
        ledger::delete_env(&conn, &env_id).unwrap();
        assert!(field_hits(&conn, "生产门户").unwrap().is_empty(), "软删除的台账行不该再被搜到");

        let note_id = conn
            .query_row("SELECT id FROM ledger_notes WHERE title = '接口坑点'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap();
        ledger::delete_note(&conn, &note_id).unwrap();
        assert!(field_hits(&conn, "租户参数").unwrap().is_empty());

        project::delete_project(&conn, &pid).unwrap();
        assert!(field_hits(&conn, "运维后台").unwrap().is_empty(), "项目删除后其台账不再可搜");
        assert!(field_hits(&conn, "政务云迁移").unwrap().is_empty());
    }

    #[test]
    fn each_group_is_capped_so_one_query_cannot_dump_a_table() {
        let (conn, pid, _mk) = fixture();
        for i in 0..(PER_GROUP_LIMIT + 20) {
            ledger::create_note(
                &conn,
                &pid,
                &ledger::NoteInput {
                    title: format!("批量备注{i}"),
                    body: None,
                    tag: Some("批量".to_owned()),
                },
            )
            .unwrap();
        }
        assert_eq!(field_hits(&conn, "批量备注").unwrap().len(), PER_GROUP_LIMIT as usize);
        assert_eq!(field_hits(&conn, "批量").unwrap().len(), PER_GROUP_LIMIT as usize);
        assert!(
            field_hits(&conn, MAIN_PW).unwrap().is_empty(),
            "主密码本身不该躺在任何可检索的明文列里"
        );
    }

    #[test]
    fn overly_long_query_is_rejected_instead_of_becoming_a_full_scan() {
        let (conn, _pid, _mk) = fixture();
        let e = field_hits(&conn, &"啊".repeat(200)).unwrap_err();
        assert_eq!(e.code, "invalid_input");
    }
}
