//! 索引表的读写。三条不变量：
//! 1. 主表与 FTS 是两处存储，任何增删都要在同一个事务里做两笔（事实 3），否则命中列表里会
//!    出现已经不存在的文件。
//! 2. 只有 `DocOutcome::Ok` 会往虚表写东西。skipped/failed 只留状态行 —— 「为什么搜不到」
//!    的答案在行上，不在正文里，也就不该出现在搜索结果里。
//! 3. 本模块每个写函数**自己开事务**（`unchecked_transaction()` 直接下 `BEGIN DEFERRED`，
//!    rusqlite 0.40.2 里没有重入检查，所以调用方再包一层会让内层 `BEGIN` 当场失败：
//!    `cannot start a transaction within a transaction` → `db_failed`）。Task 9 的 `run_pass`
//!    逐文件调 `write_doc`，不要在外面套事务，也不要指望「一轮一个事务」的提速。

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::index_scan::ScannedFile;
use crate::tokenize::{index_text, query_expression, MAX_QUERY_CHARS};

/// `Ok(String)` 携带抽取到的正文，交给这里决定要不要落 FTS。
pub enum DocOutcome {
    Ok(String),
    Empty,
    Skipped(&'static str),
    Failed(String),
}

pub fn write_doc(
    conn: &Connection,
    project_id: &str,
    file: &ScannedFile,
    outcome: DocOutcome,
) -> AppResult<i64> {
    let (status, skip_reason, error_msg, body) = match outcome {
        DocOutcome::Ok(body) => {
            if body.trim().is_empty() {
                ("skipped", Some("empty_text"), None, None)
            } else {
                ("ok", None, None, Some(body))
            }
        }
        DocOutcome::Empty => ("skipped", Some("empty_text"), None, None),
        DocOutcome::Skipped(reason) => ("skipped", Some(reason), None, None),
        DocOutcome::Failed(msg) => ("failed", None, Some(msg), None),
    };

    let tx = conn.unchecked_transaction()?;
    // ON CONFLICT 不动 id / doc_rowid（事实 21）：同一行被重新索引时 rowid 稳定，
    // 于是 FTS 侧可以先按 rowid 点删再插，不需要先查一次。
    tx.execute(
        "INSERT INTO index_docs (id, project_id, path, ext, size, mtime, index_status, skip_reason, error_msg, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, CASE WHEN ?7 = 'ok' THEN datetime('now') ELSE NULL END)
         ON CONFLICT (project_id, path) DO UPDATE SET
             ext          = excluded.ext,
             size         = excluded.size,
             mtime        = excluded.mtime,
             index_status = excluded.index_status,
             skip_reason  = excluded.skip_reason,
             error_msg    = excluded.error_msg,
             indexed_at   = excluded.indexed_at,
             updated_at   = datetime('now')",
        params![
            Uuid::new_v4().to_string(),
            project_id,
            file.path,
            file.ext,
            file.size as i64,
            file.mtime,
            status,
            skip_reason,
            error_msg
        ],
    )?;
    let rowid: i64 = tx.query_row(
        "SELECT doc_rowid FROM index_docs WHERE project_id = ?1 AND path = ?2",
        params![project_id, file.path],
        |r| r.get(0),
    )?;

    // 这一笔是**硬性前提**而不是「免得累积历史版本」的优化：FTS5 对重复 rowid 直接回约束错误。
    // 控制方实测把这句抽掉，第二次写同路径当场 Err（rusqlite 文案 `constraint failed` → AppError `db_failed`）。
    tx.execute("DELETE FROM index_docs_fts WHERE rowid = ?1", params![rowid])?;
    if let Some(body) = body {
        tx.execute(
            "INSERT INTO index_docs_fts (rowid, name_tokens, body_tokens) VALUES (?1, ?2, ?3)",
            params![rowid, index_text(&file.file_name), index_text(&body)],
        )?;
    }
    tx.commit()?;
    Ok(rowid)
}

/// 增量跳过：同路径且 size + mtime 都没变、上次是 ok，就不再读文件。
/// 返回 None 表示需要（重新）抽取。
pub fn current_rowid(
    conn: &Connection,
    project_id: &str,
    path: &str,
    size: i64,
    mtime: i64,
) -> AppResult<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT doc_rowid FROM index_docs
              WHERE project_id = ?1 AND path = ?2 AND size = ?3 AND mtime = ?4 AND index_status = 'ok'",
            params![project_id, path, size, mtime],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn clear_project(conn: &Connection, project_id: &str) -> AppResult<u64> {
    let tx = conn.unchecked_transaction()?;
    let rowids: Vec<i64> = tx
        .prepare("SELECT doc_rowid FROM index_docs WHERE project_id = ?1")?
        .query_map(params![project_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for rowid in &rowids {
        tx.execute("DELETE FROM index_docs_fts WHERE rowid = ?1", params![rowid])?;
    }
    let n = tx.execute("DELETE FROM index_docs WHERE project_id = ?1", params![project_id])? as u64;
    tx.commit()?;
    Ok(n)
}

/// 跨表不变量的**全表**形态（终审 m2 里本轮要做的那条）：`index_docs_fts` 是 FTS5 虚表、没有外键，
/// 整个设计就靠 rowid 对齐，而虚表行不参与 `ON DELETE CASCADE`。既有测试只数过局部
/// （本模块 `rewriting_…` / `non_ok_outcomes_…` / `clear_project_…` 那几处 count），
/// 「任何路径都不留孤儿」这句话本身此前全仓无断言（grep `NOT IN`/orphan/孤立 0 命中）。
/// 孤儿行的后果不是错命中（`SELECT_SQL` 靠 JOIN 连不上），而是索引体积与 bm25 统计被长期污染且没人能清。
/// 只给测试用：产品路径不该拿它当检查点（每轮全表扫）。
#[cfg(test)]
pub(crate) fn fts_orphan_rows(conn: &Connection) -> i64 {
    // 注意：FTS5 虚表不能随手 `SELECT *`，这里按普通 query_row 只取 count(*)。
    conn.query_row(
        "SELECT count(*) FROM index_docs_fts WHERE rowid NOT IN (SELECT doc_rowid FROM index_docs)",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

pub fn status_counts(conn: &Connection, project_id: &str) -> AppResult<Vec<StatusCount>> {
    let mut stmt = conn.prepare(
        "SELECT index_status, count(*) FROM index_docs WHERE project_id = ?1 GROUP BY index_status
                  ORDER BY index_status",
    )?;
    let rows = stmt.query_map(params![project_id], |r| {
        Ok(StatusCount { status: r.get(0)?, count: r.get(1)? })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// 有几个项目已经有可搜的正文了。首屏空态要在「正文还没建索引」和「建了但没命中」之间分开
/// （M3 的 m8：文案对「根不存在」和「0 个可索引文件」说了同一句话），所以这里的谓词
/// **必须与 `SELECT_SQL` 同口径**：少半个谓词就会出现「文案说正文有索引、正文段却是空的」。
/// 取数走原生 `i64` 再转 `usize`：rusqlite 0.40.2 的 `from_sql_integral!(usize)`（`src/types/from_sql.rs:146`）
/// 挂在 `#[cfg(feature = "fallible_uint")]` 后面，本项目没开该 feature，所以 `usize` 直接 `get` 编译不过
/// （简报此处按「直取」写，实测 E0277）。`count(DISTINCT …)` 恒非负，转换不丢信息。
pub fn indexed_project_count(conn: &Connection) -> AppResult<usize> {
    let n = conn.query_row(
        "SELECT count(DISTINCT d.project_id)
           FROM index_docs d
           JOIN projects p ON p.id = d.project_id
          WHERE d.index_status = 'ok' AND p.deleted_at IS NULL",
        [],
        |r| r.get::<_, i64>(0),
    )?;
    Ok(n as usize)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocRow {
    pub id: String,
    pub path: String,
    pub ext: String,
    pub size: i64,
    pub status: String,
    pub skip_reason: Option<String>,
    pub error_msg: Option<String>,
    pub indexed_at: Option<String>,
}

pub fn list_docs(
    conn: &Connection,
    project_id: &str,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<Vec<DocRow>> {
    // status 走参数而不是拼进 SQL：这一层暴露给 IPC，任何字符串拼接都是注入面。
    let sql = "SELECT id, path, ext, size, index_status, skip_reason, error_msg, indexed_at
                 FROM index_docs
                WHERE project_id = ?1 AND (index_status = ?2 OR ?2 IS NULL)
                ORDER BY path
                LIMIT ?3 OFFSET ?4";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![project_id, status, limit, offset], |r| {
        Ok(DocRow {
            id: r.get(0)?,
            path: r.get(1)?,
            ext: r.get(2)?,
            size: r.get(3)?,
            status: r.get(4)?,
            skip_reason: r.get(5)?,
            error_msg: r.get(6)?,
            indexed_at: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocHit {
    pub doc_id: String,
    pub project_id: String,
    pub project_name: String,
    pub path: String,
    /// 已按 `clean_snippet` 收回 CJK 空格的摘要，命中词用 [ ] 包住。
    /// 两条 M4 要知道的契约（控制方探针实测）：摘要取自**正文列**（`snippet()` 的第 2 个实参固定为 1），
    /// 只靠文件名命中的结果摘要不带 [ ]；且摘要串是预分词后的正文，`cut_for_search` 的复合词原本会
    /// 连着出现两次，M4 起由 `clean_snippet` 的三连折叠收口（见 `tokenize.rs:fold_duplicated_compound`）；
    /// 两类残留不折、重复词仍连着出现两次：① 窗口首格被 `⋯` 粘住，② 命中词自己带着 `[ ]` 标记。
    /// 折叠只作用在这条展示串上，不影响召回，只影响观感。
    /// 要拿原文做摘要得在建表时给虚表加一列 UNINDEXED 正文，不许在检索侧拼。
    pub snippet: String,
    /// exact | prefix：放宽过的命中要能被界面标出来，否则用户会以为是 bug
    pub matched_by: &'static str,
    /// bm25 原值，**越小越好**：bundled `sqlite3.c:245090` 交回的是 `-1.0 * score`，
    /// 所以 `ORDER BY bm25(...)` 升序就是最相关在前。这个数只用于相对排序，别直接显示给用户。
    pub score: f64,
}

const SELECT_SQL: &str = "SELECT d.id, d.project_id, p.name, d.path,
       snippet(index_docs_fts, 1, '[', ']', '⋯', 12), bm25(index_docs_fts)
  FROM index_docs_fts f
  JOIN index_docs d   ON d.doc_rowid = f.rowid
  JOIN projects p    ON p.id = d.project_id
 WHERE index_docs_fts MATCH ?1
   AND d.index_status = 'ok'
   AND p.deleted_at IS NULL
 ORDER BY bm25(index_docs_fts)
 LIMIT ?2";

fn run_select(conn: &Connection, expr: &str, limit: i64, matched_by: &'static str) -> AppResult<Vec<DocHit>> {
    let mut stmt = conn.prepare(SELECT_SQL)?;
    let rows = stmt.query_map(params![expr, limit], |r| {
        Ok(DocHit {
            doc_id: r.get(0)?,
            project_id: r.get(1)?,
            project_name: r.get(2)?,
            path: r.get(3)?,
            snippet: crate::tokenize::clean_snippet(&r.get::<_, String>(4)?),
            score: r.get(5)?,
            matched_by,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// 两段式（中文分词检索的核心取舍）：第一段「精确」要求词形完全一致，结果最准；
/// 第二段「前缀」只在 0 命中时放宽成 `"维保"*`，把被切成「维保期」这种长词的情况捞回来。
/// 只做第一段会表现为「功能没坏但搜不到」，直接只做第二段则精度差。
/// 两段用同一个 `query_expression`，即同一套预处理，见 tokenize.rs 的头注释。
///
/// `limit` 原样进 SQL，这里不校验也不补默认值：SQLite 里负数 LIMIT = 不限行、0 = 无行，
/// clamp 属于调用方的系统边界（`lib.rs:542` 的 `search_docs` 是唯一做过 clamp 的入口，
/// 首屏那条走 `search::DOCS_FETCH_LIMIT`）。
/// 刻意不做第二道校验 —— 本项目只在边界校验一次，两道 clamp 会漂成两个数。
pub fn doc_hits(conn: &Connection, query: &str, limit: i64) -> AppResult<Vec<DocHit>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err(AppError::new(
            "invalid_input",
            "正文检索关键词过长",
            Some("请用短词搜正文；要找某个具体文件，去 /index 页按项目翻清单"),
        ));
    }
    if let Some(expr) = query_expression(q, false) {
        let hits = run_select(conn, &expr, limit, "exact")?;
        if !hits.is_empty() {
            return Ok(hits);
        }
        if let Some(loose) = query_expression(q, true) {
            return run_select(conn, &loose, limit, "prefix");
        }
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, index_scan::ScannedFile};
    use rusqlite::{params, Connection};

    fn conn() -> Connection {
        db::open_in_memory().unwrap()
    }

    fn seed_project(conn: &Connection, id: &str) {
        conn.execute("INSERT INTO projects (id, name, status) VALUES (?1, ?2, '立项')", params![id, "政务云迁移"])
            .unwrap();
    }

    fn file(path: &str) -> ScannedFile {
        ScannedFile {
            path: path.to_owned(),
            file_name: path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned(),
            ext: "docx".to_owned(),
            size: 1024,
            mtime: 1_700_000_000,
        }
    }

    /// 事实 3：主表和 FTS 是两处存储，删一边不会带动另一边。写入必须在一个事务里两点落。
    #[test]
    fn write_doc_lands_on_both_sides_and_is_searchable() {
        let c = conn();
        seed_project(&c, "p1");
        let rowid = write_doc(&c, "p1", &file("C:/x/合同验收.docx"), DocOutcome::Ok("甲方要求验收指标".into())).unwrap();
        let id: String = c
            .query_row("SELECT id FROM index_docs WHERE doc_rowid = ?1", params![rowid], |r| r.get(0))
            .unwrap();
        assert!(!id.is_empty());
        let hit: i64 = c
            .query_row(
                "SELECT count(*) FROM index_docs_fts f JOIN index_docs d ON d.doc_rowid = f.rowid
                  WHERE index_docs_fts MATCH ?1 AND d.id = ?2",
                params!["\"验收\"", id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, 1);
        let indexed_at: Option<String> = c
            .query_row("SELECT indexed_at FROM index_docs WHERE doc_rowid = ?1", params![rowid], |r| r.get(0))
            .unwrap();
        assert!(indexed_at.is_some(), "ok 行必须带索引时间，Task 10 的清单要能回答「什么时候建的索引」");
    }

    /// 事实 21：同路径重写不换行、不留重复 FTS 行（rowid 与对外 id 都稳定 + 点删再插）。
    ///
    /// 「旧正文不该还能搜到」这条断言用的词**不能出现在文件名里**：`name_tokens` 每次重写都
    /// 由同一个 `file_name` 重新生成，用「验收」这种文件名里就有的词去断「搜不到」，
    /// 永远为 1，断言当场变成装饰（控制方实测：原写法在正确实现下 `left: 1 / right: 0` 直接红）。
    #[test]
    fn rewriting_the_same_path_replaces_instead_of_duplicating() {
        let c = conn();
        seed_project(&c, "p1");
        let f = file("C:/x/合同验收.docx");
        let r1 = write_doc(&c, "p1", &f, DocOutcome::Ok("第一版正文 付款".into())).unwrap();
        let before: i64 = c
            .query_row("SELECT count(*) FROM index_docs_fts WHERE index_docs_fts MATCH ?1", params!["\"付款\""], |r| r.get(0))
            .unwrap();
        assert_eq!(before, 1, "旧正文的词先要真的能搜到，后面那条「搜不到」才不是空转");
        let id_before: String = c
            .query_row("SELECT id FROM index_docs WHERE doc_rowid = ?1", params![r1], |r| r.get(0))
            .unwrap();
        let f2 = ScannedFile { size: 2048, mtime: 1_700_000_999, ..f.clone() };
        let r2 = write_doc(&c, "p1", &f2, DocOutcome::Ok("第二版正文 报价".into())).unwrap();
        assert_eq!(r1, r2, "同一路径重复写入必须复用同一行");
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get::<_, i64>(0)).unwrap(), 1,
            "FTS 侧不该累积历史版本");
        let old: i64 = c
            .query_row("SELECT count(*) FROM index_docs_fts WHERE index_docs_fts MATCH ?1", params!["\"付款\""], |r| r.get(0))
            .unwrap();
        assert_eq!(old, 0, "旧正文不该还能搜到");
        let now: i64 = c
            .query_row("SELECT count(*) FROM index_docs_fts WHERE index_docs_fts MATCH ?1", params!["\"报价\""], |r| r.get(0))
            .unwrap();
        assert_eq!(now, 1, "新正文要能搜到");
        let id_after: String = c
            .query_row("SELECT id FROM index_docs WHERE doc_rowid = ?1", params![r2], |r| r.get(0))
            .unwrap();
        assert_eq!(id_before, id_after, "重写不能换掉对外的 id：Task 8 的 doc_hits 与 Task 10 的 IPC 都拿它当结果标识");
    }

    /// skipped/failed 只留状态行，不能往虚表塞空串：空串行会让 count 统计与 FTS 行数对不上。
    ///
    /// 末尾那条 `DocOutcome::Ok("   ")` 是 Task 5 评审 defer 过来的：扫描型 PDF 抽出来是空串，
    /// Task 9 走的是 `Ok(body)` 这一支，`empty_text` 在这里才是承重项 —— 少了 `.trim()`，
    /// 一具空正文会带着 ok 状态进库，UI 上显示「已索引」而永远搜不到。
    #[test]
    fn non_ok_outcomes_record_status_but_write_no_fts_rows() {
        let c = conn();
        seed_project(&c, "p1");
        write_doc(&c, "p1", &file("C:/a.docx"), DocOutcome::Skipped("too_large")).unwrap();
        write_doc(&c, "p1", &file("C:/b.docx"), DocOutcome::Failed("解包失败".into())).unwrap();
        write_doc(&c, "p1", &file("C:/c.docx"), DocOutcome::Empty).unwrap();
        write_doc(&c, "p1", &file("C:/d.docx"), DocOutcome::Ok("   ".into())).unwrap();
        let blank: (String, Option<String>, Option<String>) = c
            .query_row(
                "SELECT index_status, skip_reason, indexed_at FROM index_docs WHERE path = 'C:/d.docx'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            blank,
            ("skipped".to_string(), Some("empty_text".to_string()), None),
            "空白正文算 empty_text、不算 ok，且不得有索引时间"
        );
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs", [], |r| r.get::<_, i64>(0)).unwrap(), 4);
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        let reason: Option<String> = c
            .query_row("SELECT skip_reason FROM index_docs WHERE path = 'C:/a.docx'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(reason.as_deref(), Some("too_large"), "超限原因要能被 UI 直接回答");
        let err: Option<String> = c
            .query_row("SELECT error_msg FROM index_docs WHERE path = 'C:/b.docx'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(err.as_deref(), Some("解包失败"));
        // skipped 行重写成 ok：`indexed_at = excluded.indexed_at` 这一支必须真的把时间带进来。
        write_doc(&c, "p1", &file("C:/d.docx"), DocOutcome::Ok("甲方要求验收指标".into())).unwrap();
        let upgraded: Option<String> = c
            .query_row("SELECT indexed_at FROM index_docs WHERE path = 'C:/d.docx'", [], |r| r.get(0))
            .unwrap();
        assert!(upgraded.is_some(), "skipped 行重写成 ok 之后必须补上索引时间");
    }

    #[test]
    fn status_counts_group_per_project() {
        let c = conn();
        seed_project(&c, "p1");
        seed_project(&c, "p2");
        write_doc(&c, "p1", &file("C:/1.docx"), DocOutcome::Ok("验收".into())).unwrap();
        write_doc(&c, "p1", &file("C:/2.docx"), DocOutcome::Ok("报价".into())).unwrap();
        write_doc(&c, "p1", &file("C:/3.docx"), DocOutcome::Skipped("too_large")).unwrap();
        write_doc(&c, "p2", &file("D:/1.docx"), DocOutcome::Failed("x".into())).unwrap();
        let counts = status_counts(&c, "p1").unwrap();
        let got: Vec<(String, i64)> = counts.into_iter().map(|s| (s.status, s.count)).collect();
        assert!(got.contains(&("ok".to_string(), 2)), "{got:?}");
        assert!(got.contains(&("skipped".to_string(), 1)), "{got:?}");
        assert!(!got.iter().any(|(s, _)| s == "failed"), "另一个项目的行不该混进来：{got:?}");
    }

    /// 与 `SELECT_SQL` 同口径是这条的唯一价值：`ok` 之外不算、项目软删了不算。
    #[test]
    fn indexed_project_count_matches_the_search_predicate() {
        let c = conn();
        seed_project(&c, "p_ok");
        write_doc(&c, "p_ok", &file("C:/x/合同.docx"), DocOutcome::Ok("甲方要求验收指标".into())).unwrap();
        seed_project(&c, "p_skipped");
        write_doc(&c, "p_skipped", &file("C:/x/大图.png"), DocOutcome::Skipped("超出大小上限")).unwrap();
        seed_project(&c, "p_deleted");
        write_doc(&c, "p_deleted", &file("C:/x/验收.docx"), DocOutcome::Ok("验收指标".into())).unwrap();
        c.execute("UPDATE projects SET deleted_at = datetime('now') WHERE id = 'p_deleted'", []).unwrap();
        assert_eq!(indexed_project_count(&c).unwrap(), 1, "只有 p_ok 既在库里又没被软删");
    }

    /// 事实 3 的另一半：删的时候两边都要删。`delete_doc` 已按上面的裁定移出本任务，
    /// 这处两点删由 `clear_project` 顶上来钉住。
    #[test]
    fn clear_project_removes_rows_on_both_sides() {
        let c = conn();
        seed_project(&c, "p1");
        seed_project(&c, "p2");
        write_doc(&c, "p1", &file("C:/1.docx"), DocOutcome::Ok("验收".into())).unwrap();
        write_doc(&c, "p1", &file("C:/2.docx"), DocOutcome::Ok("报价".into())).unwrap();
        write_doc(&c, "p2", &file("D:/1.docx"), DocOutcome::Ok("付款".into())).unwrap();
        assert_eq!(clear_project(&c, "p1").unwrap(), 2, "清场回的是本项目被删掉的行数");
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get::<_, i64>(0)).unwrap(), 1,
            "删主表不删虚表会留下孤儿行，命中列表里会出现已消失的文件");
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs WHERE project_id = 'p1'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs WHERE project_id = 'p2'", [], |r| r.get::<_, i64>(0)).unwrap(), 1,
            "清场不能顺手把别的项目一起清掉");
        // 全表不变量（终审 m2）：上面两条 count 只证明「两边各自少了几行」，证明不了两边**对齐**。
        // 少这一句，把 clear_project 里那句按 rowid 点删改成删错列（例如删 doc_rowid 不存在的行），
        // 主表照样归零、虚表留下孤儿，而这两条断言都还是绿的。
        assert_eq!(fts_orphan_rows(&c), 0, "清场之后不该留下对不上主表的虚表行");
    }

    /// 只数行数证不出 OFFSET 生效：`LIMIT 2 OFFSET 4` 在 5 行数据上只回 1 行，
    /// 而「把 OFFSET 整个删掉」的错误实现照样回 2 行。所以这里断的是**具体是哪两行**。
    /// （控制方实测：原写的 `list_docs(.., 2, 4)` + `assert_eq!(page.len(), 2)` 在正确实现下
    /// 红在 `left: 1 / right: 2`。）
    #[test]
    fn list_docs_filters_by_status_and_pages() {
        let c = conn();
        seed_project(&c, "p1");
        for i in 0..5 {
            let outcome = if i % 2 == 0 {
                DocOutcome::Skipped("too_large")
            } else {
                DocOutcome::Ok(format!("正文{i} 验收"))
            };
            write_doc(&c, "p1", &file(&format!("C:/{i}.docx")), outcome).unwrap();
        }
        let skipped = list_docs(&c, "p1", Some("skipped"), 10, 0).unwrap();
        assert_eq!(skipped.len(), 3, "{:?}", skipped.iter().map(|d| &d.path).collect::<Vec<_>>());
        let page = list_docs(&c, "p1", None, 2, 2).unwrap();
        let got: Vec<String> = page.iter().map(|d| d.path.clone()).collect();
        assert_eq!(got, vec!["C:/2.docx".to_string(), "C:/3.docx".to_string()],
            "分页要按 path 排序真的跳过前两行");
    }

    /// 增量跳过闸门。这个函数没有 caller 之前（Task 9）最容易写漏的是 `index_status = 'ok'`
    /// 那一条：漏了它，上次 failed/skipped 的行会被当成「已经索引过」，用户看到的是一次失败
    /// 之后永久搜不到 —— 正是本项目要消灭的那类静默失效。
    #[test]
    fn current_rowid_gates_the_incremental_skip() {
        let c = conn();
        seed_project(&c, "p1");
        seed_project(&c, "p2");
        let f = file("C:/x/合同验收.docx");
        let rowid = write_doc(&c, "p1", &f, DocOutcome::Ok("正文 付款".into())).unwrap();
        assert_eq!(current_rowid(&c, "p1", &f.path, f.size as i64, f.mtime).unwrap(), Some(rowid),
            "同路径、size 与 mtime 都没变、上次是 ok：这一轮不该再读文件");
        assert_eq!(current_rowid(&c, "p1", &f.path, f.size as i64 + 1, f.mtime).unwrap(), None, "大小变了要重抽");
        assert_eq!(current_rowid(&c, "p1", &f.path, f.size as i64, f.mtime + 1).unwrap(), None, "修改时间变了要重抽");
        assert_eq!(current_rowid(&c, "p2", &f.path, f.size as i64, f.mtime).unwrap(), None,
            "另一个项目的同路径行不能冒充本项目已索引");
        let g = file("C:/x/坏文件.docx");
        write_doc(&c, "p1", &g, DocOutcome::Failed("解包失败".into())).unwrap();
        assert_eq!(current_rowid(&c, "p1", &g.path, g.size as i64, g.mtime).unwrap(), None,
            "上次失败的行必须重试");
    }

    /// 摘要 + 得分 + 命中来源，是 M4 首屏结果页要直接渲染的东西。
    fn hits(conn: &Connection, q: &str) -> Vec<DocHit> {
        doc_hits(conn, q, 50).unwrap()
    }

    fn seeded_with_docs() -> Connection {
        let c = conn();
        seed_project(&c, "p1");
        write_doc(&c, "p1", &file("C:/x/合同验收说明.docx"), DocOutcome::Ok("甲方要求验收指标见合同附件".into())).unwrap();
        write_doc(&c, "p1", &file("C:/x/维保期说明.docx"), DocOutcome::Ok("维保期为十二个月".into())).unwrap();
        write_doc(&c, "p1", &file("C:/x/报价单.xlsx"), DocOutcome::Ok("里面只有付款条件与验收流程".into())).unwrap();
        c
    }

    #[test]
    fn chinese_word_hits_with_a_readable_body_snippet() {
        let c = seeded_with_docs();
        let got = hits(&c, "验收");
        assert_eq!(got.len(), 2, "{:?}", got.iter().map(|h| &h.path).collect::<Vec<_>>());
        assert!(got.iter().all(|h| h.matched_by == "exact"), "精确段就该命中：{:?}", got.iter().map(|h| h.matched_by).collect::<Vec<_>>());
        let hit = &got[0];
        assert_eq!(hit.project_name, "政务云迁移", "结果要带项目名，供 M4 分组");
        assert!(hit.snippet.contains('[') && hit.snippet.contains(']'), "摘要要标出命中词：{}", hit.snippet);
        // 原来这条写的是 `!hit.snippet.contains(" 甲 方")`，控制方探针实测它是假断言：
        // 入库串是 jieba 词用空格拼的，「甲方」本身是一个词，原始摘要为
        // "甲方 要求 [验收] 指标 见 合同 附件"，里面根本不存在 " 甲 方" 这个子串，
        // 把 run_select 里的 clean_snippet 调用删掉，整套测试照样全绿。
        // 夹具正文全 CJK，所以「摘要里一个空格都不该剩」既断得住，又能被同一处变异打红。
        assert!(!hit.snippet.contains(' '), "摘要里不该残留分词空格：{}", hit.snippet);
        // 摘要只取自正文列（snippet 的第 2 个实参固定为 1）：只靠文件名命中的结果，摘要不带 [ ]。
        // 这条是 M4 的界面契约。正文入库串是 `里面 只有 付款 条件 付款条件 与 验收 流程`
        // （cut_for_search 把复合词连同子词一起写进索引列），M4 的三连折叠把它折回一个词，
        // 所以下面期望的是**逐字相等**而不是「不含空格」这种弱断言。
        let only_name = hits(&c, "报价单");
        assert_eq!(only_name.len(), 1);
        assert_eq!(only_name[0].snippet, "里面只有付款条件与验收流程");
    }

    /// 事实 5：复合词的子词查询靠 cut_for_search 才能命中。
    #[test]
    fn compound_and_subword_queries_hit() {
        let c = seeded_with_docs();
        assert_eq!(hits(&c, "付款条件").len(), 1);
        assert_eq!(hits(&c, "付款 条件").len(), 1, "子词没进索引的话这条会空");
        assert_eq!(hits(&c, "合同附件").len(), 1);
    }

    /// 事实 6：「维保」在库里是「维保期」的一部分，精确段够不着，必须落到前缀段。
    #[test]
    fn prefix_stage_reports_itself_so_the_ui_can_explain_the_looser_match() {
        let c = seeded_with_docs();
        let got = hits(&c, "维保");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].matched_by, "prefix", "放宽过的命中要能说清：{:?}", got[0]);
        assert!(got[0].path.contains("维保期说明"), "夹具名见事实 6 的夹具限制");
    }

    #[test]
    fn fts5_syntax_in_the_query_cannot_widen_the_result_set() {
        let c = seeded_with_docs();
        // 语法词被引号包成普通词：这三条都应该是 0 或窄结果，绝不等于「全表命中」。
        assert!(hits(&c, "验收 OR 报价").is_empty(), "OR 只是普通字符，整条要求两词都在");
        assert!(hits(&c, "*").is_empty(), "单星号不该等于全表命中");
        assert!(hits(&c, "NEAR(验收 报价)").is_empty());
        assert_eq!(hits(&c, "验收").len(), 2, "对照组：同一库上正常查询是有结果的");
    }

    #[test]
    fn soft_deleted_project_docs_drop_out_of_hits() {
        let c = seeded_with_docs();
        assert!(!hits(&c, "验收").is_empty());
        c.execute("UPDATE projects SET deleted_at = datetime('now') WHERE id = 'p1'", []).unwrap();
        assert!(hits(&c, "验收").is_empty(), "项目软删除后其文档不该再出现在结果里");
    }

    /// M3 的验收点。spec 明写「密文列不参与任何检索」，且要求结构性排除而不是查询后过滤：
    /// 台账的密文列从来没有进入 write_doc 的入参路径（正文只来自磁盘文件的抽取结果），
    /// 所以这条测的是「整条管线跑完后，敏感字段的明文在索引里彻底不存在」。
    #[test]
    fn cipher_column_plaintext_never_reaches_the_fts_index() {
        use crate::{ledger, vault};

        let c = seeded_with_docs();
        let code = vault::generate_recovery_code().unwrap();
        let mk = vault::initialize(&c, "主密码-至少八位", &code).unwrap();
        let secret = "机密串-Zhang@2026";
        ledger::create_credential(
            &c,
            &mk,
            "p1",
            &ledger::CredentialInput {
                env_id: None,
                title: "运维后台".to_owned(),
                username: Some(secret.to_owned()),
                password: Some(secret.to_owned()),
                url: Some("https://ops.example.gov.cn".to_owned()),
                note: Some("交付时用".to_owned()),
            },
        )
        .unwrap();

        // 1) 用敏感字段的明文检索，一条都不该命中。先钉住这条腿不是空转：
        //    查询串必须真的能成形，否则 doc_hits 跳过 SQL 直接回空、断言恒绿。
        //    探针实测 query_expression(secret,false) = Some("\"机密\" AND \"串\" AND \"Zhang\" AND \"2026\"")。
        assert!(query_expression(secret, false).is_some(), "第 1 段的查询串要能成形，否则空结果是假绿");
        assert!(hits(&c, secret).is_empty(), "敏感字段明文进不了 FTS");
        // 2) 结构性检查：虚表两列里连片段都不该有。needle 只用**一段连续**字符 ——
        //    原来写的 "%Zhang@2026%" 经探针实测：在「明文真的进了索引」的库里照样回 0
        //    （入库串是 jieba 词用空格拼的，Zhang 与 2026 之间被隔开了），
        //    也就是这条当时无论有没有泄露都恒绿。换 %Zhang% / %2026% 后各回 1，才是能变红的写法。
        for col in ["body_tokens", "name_tokens"] {
            for needle in ["%Zhang%", "%2026%"] {
                let n: i64 = c
                    .query_row(
                        &format!("SELECT count(*) FROM index_docs_fts WHERE {col} LIKE ?1"),
                        params![needle],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(n, 0, "{col} 里不该存在敏感值 {needle}：{n}");
            }
        }
        // 3) 自我证明（正面照控）：把同一串明文走**正常写入路径**放进索引，上面两条必须能变红。
        //    没有这一段，前两条只是「看起来像测试」。探针实测：hits(&c2, secret) 回 1 条、
        //    body_tokens LIKE '%2026%' 计数 1。
        let c2 = seeded_with_docs();
        write_doc(&c2, "p1", &file("C:/x/演示.docx"), DocOutcome::Ok(secret.into())).unwrap();
        assert_eq!(hits(&c2, secret).len(), 1, "正面照控：明文一旦进了索引，第 1 段断言真的会红");
        let n: i64 = c2
            .query_row("SELECT count(*) FROM index_docs_fts WHERE body_tokens LIKE '%2026%'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "正面照控：明文一旦进了索引，第 2 段的 LIKE 真的抓得到");
        // 4) 反向对照：台账的明文列 note（交付时用）走的是库内字段检索，
        //    而 FTS 这边只有磁盘文件正文，两边互不污染
        assert_eq!(hits(&c, "付款条件").len(), 1);
    }

    /// 两条 guard 分支和 limit 的去向。`search.rs` 给自己的同名 guard 留了测试
    /// （断 `invalid_input`），这里补齐，否则「空白查询不扫库」「超长要拒绝」全靠肉眼。
    /// limit 那三条钉的是**现状**不是意图：SQLite 里负数 LIMIT = 不限行、0 = 无行，
    /// `doc_hits` 不做二次校验，clamp 归调用方边界（Task 10 的 IPC 写 `limit.unwrap_or(50).clamp(1, 200)`）。
    #[test]
    fn query_guards_and_limit_pass_straight_through() {
        let c = seeded_with_docs();
        assert!(doc_hits(&c, "   ", 50).unwrap().is_empty(), "空白查询不扫库");
        let long = "验".repeat(129);
        let e = doc_hits(&c, &long, 50).unwrap_err();
        assert_eq!(e.code, "invalid_input", "超 128 字要拒绝，不是静默按前 128 字搜");
        assert_eq!(doc_hits(&c, "验收", 1).unwrap().len(), 1, "limit 要真的限制条数");
        assert_eq!(doc_hits(&c, "验收", -1).unwrap().len(), 2, "负数 limit 原样交给 SQL（SQLite：不限行）");
        assert!(doc_hits(&c, "验收", 0).unwrap().is_empty(), "0 就是 0 行，不许悄悄变成默认值");
    }
}
