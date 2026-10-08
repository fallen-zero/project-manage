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
use crate::index_store::{self, DocHit};
use crate::tokenize::MAX_QUERY_CHARS;

/// 每组最多返回这么多条：命中几千条时把整表灌进前端没有意义，界面也翻不完。
const PER_GROUP_LIMIT: i64 = 50;

/// 正文段一次向库取多少条。这是**取数**上限，不是展示上限；展示截断走下面三个数。
/// 顶对齐 IPC 侧 `search_docs` 的 limit clamp（`lib.rs:542`，那里是边界唯一一次校验），
/// 让首屏与 `/index` 的「试搜正文」试验台在同一条 SQL 上取数。
const DOCS_FETCH_LIMIT: i64 = 200;
/// 段级：一个段最多展示多少个簇。整簇被扔掉的簇数进 `ClusterSection::hidden_clusters`。
const MAX_CLUSTERS_PER_SECTION: usize = 20;
/// 簇级：一个簇最多展示多少条，被截掉的条数进该簇 `hidden`。
const MAX_ITEMS_PER_CLUSTER: usize = 10;
/// 「项目」段是平铺列表，没有簇可挂计数，截掉的条数进 `projects_hidden`。
const MAX_PROJECT_HITS: usize = 10;

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

/// 一个项目在一个段里的命中。两段的簇形状除 `items` 元素类型外逐字段相同，所以只写一个泛型结构；
/// 泛型不动线格式：serde 按 `T` 的具体实例化生成序列化，`rename_all` 照样生效。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cluster<T> {
    pub project_id: String,
    pub project_name: String,
    pub items: Vec<T>,
    /// 簇内被 `MAX_ITEMS_PER_CLUSTER` 截掉的条数。只显示、不做展开（§十）。
    pub hidden: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSection<C> {
    pub clusters: Vec<C>,
    /// 被 `MAX_CLUSTERS_PER_SECTION` 整簇扔掉的簇数。
    pub hidden_clusters: usize,
}

/// 首屏一次 IPC 的全部返回。三段之间**不做分数比较**：bm25 与 LIKE 不可比（§3.4）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBundle {
    pub query: String,
    /// `source == "project"` 的命中，平铺，最多 `MAX_PROJECT_HITS` 条。
    pub projects: Vec<FieldHit>,
    pub projects_hidden: usize,
    pub ledger: ClusterSection<Cluster<FieldHit>>,
    pub docs: ClusterSection<Cluster<DocHit>>,
    /// 正文段存在 `matchedBy == "prefix"`。段级说明只出一次（D7）。
    /// 在截断前的 docs 上算：否则放宽命中全被截掉时，这个说明会凭空消失。
    pub relaxed: bool,
    /// `index_store::indexed_project_count`：空态文案用它区分「还没建索引」与「没命中」。
    pub indexed_projects: usize,
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

/// 段内按项目聚簇。簇间序 = 命中条数降序 → 项目名升序 → 项目 id 升序。
/// 第三级兜底不是因为库里会有两个同项目（`projects.id` 是主键），而是为了「簇序可复现」：
/// 前两级在库里**不唯一**（name 无 UNIQUE，条数更是常并列），少了第三级，
/// `doc_clusters_sort_by_hit_count_then_project_name` 会在不同插入序下翻红。
fn cluster_by_project<T>(
    items: Vec<T>,
    project_of: impl Fn(&T) -> (String, String),
) -> ClusterSection<Cluster<T>> {
    let mut grouped: Vec<(String, String, Vec<T>)> = Vec::new();
    for item in items {
        let (project_id, project_name) = project_of(&item);
        match grouped.iter_mut().find(|(pid, _, _)| *pid == project_id) {
            Some(entry) => entry.2.push(item),
            None => grouped.push((project_id, project_name, vec![item])),
        }
    }
    grouped.sort_by(|a, b| {
        b.2.len()
            .cmp(&a.2.len())
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.0.cmp(&b.0))
    });
    let hidden_clusters = grouped.len().saturating_sub(MAX_CLUSTERS_PER_SECTION);
    let clusters = grouped
        .into_iter()
        .take(MAX_CLUSTERS_PER_SECTION)
        .map(|(project_id, project_name, mut items)| {
            let hidden = items.len().saturating_sub(MAX_ITEMS_PER_CLUSTER);
            items.truncate(MAX_ITEMS_PER_CLUSTER);
            Cluster { project_id, project_name, items, hidden }
        })
        .collect();
    ClusterSection { clusters, hidden_clusters }
}

/// 首屏唯一入口：一次调用带回三段与三个截断计数。
/// 空判与长度守卫都不在这里做（§八：`field_hits` 与 `doc_hits` 各自负责，本项目只在边界校验一次），
/// 所以 129 字的查询由 `field_hits` 回 `invalid_input`，空白查询回一个空 bundle。
#[allow(dead_code)] // 第一个 caller 在 Task 6 的 `search_all` IPC，该任务落地时必须删掉本行（与 M3 的豁免同形，不是永久豁免）
pub fn unified_bundle(conn: &Connection, query: &str) -> AppResult<SearchBundle> {
    let fields = field_hits(conn, query)?;
    let docs = index_store::doc_hits(conn, query, DOCS_FETCH_LIMIT)?;
    // 「项目」段平铺、其余五段聚簇：思维导图把这两类分开画，前端渲染也不同款。
    let (mut projects, ledger_rows): (Vec<FieldHit>, Vec<FieldHit>) =
        fields.into_iter().partition(|h| h.source == "project");
    let projects_hidden = projects.len().saturating_sub(MAX_PROJECT_HITS);
    projects.truncate(MAX_PROJECT_HITS);
    // relaxed 取自**截断前**的完整 docs 集合，簇内截断不影响它。
    let relaxed = docs.iter().any(|h| h.matched_by == "prefix");
    let indexed_projects = index_store::indexed_project_count(conn)?;
    Ok(SearchBundle {
        query: query.trim().to_owned(),
        projects,
        projects_hidden,
        ledger: cluster_by_project(ledger_rows, |h| (h.project_id.clone(), h.project_name.clone())),
        docs: cluster_by_project(docs, |h| (h.project_id.clone(), h.project_name.clone())),
        relaxed,
        indexed_projects,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, index_scan::ScannedFile, index_store, ledger, project, vault};
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

    /// 一条可搜的正文行。`path` 在同一项目内唯一（`db.rs:297` `index_docs` 的 `UNIQUE (project_id, path)`）。
    fn seed_doc(conn: &Connection, pid: &str, path: &str, body: &str) {
        index_store::write_doc(
            conn,
            pid,
            &ScannedFile {
                path: path.to_owned(),
                file_name: path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned(),
                ext: "docx".to_owned(),
                size: 1024,
                mtime: 1_700_000_000,
            },
            index_store::DocOutcome::Ok(body.to_owned()),
        )
        .unwrap();
    }

    fn cluster_names(section: &ClusterSection<Cluster<DocHit>>) -> Vec<String> {
        section.clusters.iter().map(|c| c.project_name.clone()).collect()
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

    #[test]
    fn bundle_splits_three_sections_without_letting_one_borrow_another() {
        let (c, pid, mk) = fixture();
        seed_ledger(&c, &pid, &mk); // 五类台账各一条，逐条读过：没有一条含「验收」
        seed_project(&c, "验收流程梳理"); // 「项目」段的命中只可能来自这里
        seed_doc(&c, &pid, "C:/x/合同验收.docx", "甲方要求验收指标见合同附件");
        let b = unified_bundle(&c, "验收").unwrap();
        assert_eq!(b.query, "验收");
        assert_eq!(b.projects.len(), 1, "项目名命中只进「项目」段");
        assert_eq!(b.projects[0].title, "验收流程梳理");
        assert_eq!(b.projects[0].source, "project");
        assert!(b.ledger.clusters.is_empty(), "台账没有「验收」，正文命中不该被塞进台账段");
        assert_eq!(b.docs.clusters.len(), 1);
        assert_eq!(b.docs.clusters[0].items.len(), 1);
        assert_eq!(b.docs.clusters[0].project_name, "政务云迁移");
        assert_eq!(b.indexed_projects, 1, "只有一个项目有 ok 正文行");
    }

    #[test]
    fn doc_clusters_sort_by_hit_count_then_project_name() {
        let (c, _, _) = fixture();
        // 先建 Beta（2 条）再建 Alpha（2 条）：并列时若按插入序排会得 Beta,Alpha，
        // 这条阳性对照保证守的是「项目名升序」。Gamma 5 条，条数降序排第一。
        let beta = seed_project(&c, "Beta 项目");
        let alpha = seed_project(&c, "Alpha 项目");
        let gamma = seed_project(&c, "Gamma 项目");
        for i in 0..2 {
            seed_doc(&c, &beta, &format!("C:/b/{i}.docx"), "甲方要求验收指标");
            seed_doc(&c, &alpha, &format!("C:/a/{i}.docx"), "甲方要求验收指标");
        }
        for i in 0..5 {
            seed_doc(&c, &gamma, &format!("C:/g/{i}.docx"), "甲方要求验收指标");
        }
        let b = unified_bundle(&c, "验收").unwrap();
        assert_eq!(cluster_names(&b.docs), ["Gamma 项目", "Alpha 项目", "Beta 项目"]);
    }

    #[test]
    fn items_keep_the_bm25_order_that_doc_hits_returns() {
        let (c, pid, _) = fixture();
        for name in ["C:/x/甲.docx", "C:/x/乙.docx", "C:/x/丙.docx"] {
            seed_doc(&c, &pid, name, "甲方要求验收指标见合同附件");
        }
        let b = unified_bundle(&c, "验收").unwrap();
        let cluster = &b.docs.clusters[0];
        assert_eq!(cluster.items.len(), 3, "对照：三条命中都在同一簇");
        let in_bundle: Vec<String> = cluster.items.iter().map(|h| h.doc_id.clone()).collect();
        // 这里**刻意引常量而不是写字面量 200**：一是 Step 5 的 grep 门禁要过，二是把
        // 「上限改成 1」这种变异交给上一条 `items.len() == 3` 去打红，而不是靠两条硬编码数字互证。
        let from_store: Vec<String> = index_store::doc_hits(&c, "验收", DOCS_FETCH_LIMIT)
            .unwrap()
            .into_iter()
            .map(|h| h.doc_id)
            .collect();
        assert!(from_store.len() == 3 && from_store.windows(2).all(|w| w[0] != w[1]));
        assert_eq!(in_bundle, from_store, "M4 不许重排 bm25 序（§3.4：只做聚簇，不做再排序）");
    }

    #[test]
    fn section_drops_the_21st_cluster_and_reports_it() {
        let (c, _, _) = fixture();
        for i in 0..21 {
            let pid = seed_project(&c, &format!("p{i:02}"));
            seed_doc(&c, &pid, "C:/x/合同.docx", "甲方要求验收指标");
        }
        let b = unified_bundle(&c, "验收").unwrap();
        assert_eq!(b.docs.clusters.len(), 20);
        assert_eq!(b.docs.hidden_clusters, 1, "整簇被扔掉的簇数要有承载，否则等于静默丢弃");
        assert!(
            !cluster_names(&b.docs).iter().any(|n| n == "p20"),
            "名序最大者被截：{:?}",
            cluster_names(&b.docs)
        );
    }

    #[test]
    fn cluster_keeps_10_items_and_reports_the_other_2() {
        let (c, pid, _) = fixture();
        for i in 0..12 {
            seed_doc(&c, &pid, &format!("C:/x/{i:02}.docx"), "甲方要求验收指标");
        }
        let b = unified_bundle(&c, "验收").unwrap();
        assert_eq!(b.docs.clusters.len(), 1);
        assert_eq!(b.docs.clusters[0].items.len(), 10);
        assert_eq!(b.docs.clusters[0].hidden, 2);
        assert_eq!(b.docs.hidden_clusters, 0);
    }

    #[test]
    fn flat_project_section_truncates_into_projects_hidden() {
        let (c, _, _) = fixture();
        for i in 0..15 {
            seed_project(&c, &format!("验收{i:02}"));
        }
        let b = unified_bundle(&c, "验收").unwrap();
        assert_eq!(b.projects.len(), 10);
        assert_eq!(b.projects_hidden, 5, "平铺段没有簇可挂计数，只能单独给一个数");
        assert!(b.docs.clusters.is_empty());
        assert_eq!(b.indexed_projects, 0, "有命中 ≠ 有正文索引，两个数不许互相推出来");
    }

    #[test]
    fn empty_query_yields_an_empty_bundle_instead_of_an_error() {
        let (c, pid, _) = fixture();
        seed_doc(&c, &pid, "C:/x/合同.docx", "甲方要求验收指标");
        let b = unified_bundle(&c, "   ").unwrap();
        assert!(b.projects.is_empty() && b.ledger.clusters.is_empty() && b.docs.clusters.is_empty());
        assert!(!b.relaxed);
    }

    #[test]
    fn over_long_query_is_rejected_by_the_existing_guard() {
        let (c, _, _) = fixture();
        let err = unified_bundle(&c, &"验".repeat(129)).unwrap_err();
        assert_eq!(err.code, "invalid_input", "聚合层不许再加第三道长度校验");
    }

    #[test]
    fn relaxed_is_the_prefix_stage_and_nothing_else() {
        let (c, pid, _) = fixture();
        seed_doc(&c, &pid, "C:/x/维保期说明.docx", "维保期为十二个月");
        seed_doc(&c, &pid, "C:/x/合同验收.docx", "甲方要求验收指标见合同附件");
        let loose = unified_bundle(&c, "维保").unwrap();
        assert!(loose.relaxed, "库里只有「维保期」，落到前缀段要说得出口");
        assert_eq!(loose.docs.clusters[0].items[0].matched_by, "prefix");
        let tight = unified_bundle(&c, "验收").unwrap();
        assert!(!tight.relaxed, "精确段命中了就不该出段级说明");
    }

    #[test]
    fn bundle_wire_format_is_camel_case() {
        let (c, pid, _) = fixture();
        seed_doc(&c, &pid, "C:/x/合同.docx", "甲方要求验收指标");
        let v = serde_json::to_value(unified_bundle(&c, "验收").unwrap()).unwrap();
        let obj = v.as_object().unwrap();
        for key in ["query", "projects", "projectsHidden", "ledger", "docs", "relaxed", "indexedProjects"] {
            assert!(obj.contains_key(key), "缺 {key}：{:?}", obj.keys().collect::<Vec<_>>());
        }
        assert!(!obj.contains_key("projects_hidden"));
        let cluster = &v["docs"]["clusters"][0];
        for key in ["projectId", "projectName", "items", "hidden"] {
            assert!(cluster.as_object().unwrap().contains_key(key), "簇缺 {key}");
        }
        assert!(v["ledger"].as_object().unwrap().contains_key("hiddenClusters"));
    }
}
