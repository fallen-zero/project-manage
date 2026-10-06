//! 索引表的读写。三条不变量：
//! 1. 主表与 FTS 是两处存储，任何增删都要在同一个事务里做两笔（事实 3），否则命中列表里会
//!    出现已经不存在的文件。
//! 2. 只有 `DocOutcome::Ok` 会往虚表写东西。skipped/failed 只留状态行 —— 「为什么搜不到」
//!    的答案在行上，不在正文里，也就不该出现在搜索结果里。
//! 3. 本模块每个写函数**自己开事务**（`unchecked_transaction()` 跳过重入检查，调用方再包一层
//!    会让内层 `commit()` 提前提交外层事务）。Task 9 的 `run_pass` 逐文件调 `write_doc`，
//!    不要在外面套事务，也不要指望「一轮一个事务」的提速。

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

use crate::error::AppResult;
use crate::index_scan::ScannedFile;
use crate::tokenize::index_text;

/// `Ok(String)` 携带抽取到的正文，交给这里决定要不要落 FTS。
#[allow(dead_code)] // 构造点在 Task 9 的 index_job；本任务只有测试构造它，落地时删掉本行
pub enum DocOutcome {
    Ok(String),
    Empty,
    Skipped(&'static str),
    Failed(String),
}

#[allow(dead_code)] // caller 在 Task 9 的 index_job，落地时删掉本行
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
#[allow(dead_code)] // caller 在 Task 9 的增量跳过分支，落地时删掉本行
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

#[allow(dead_code)] // caller 在 Task 9 的重建前清场，落地时删掉本行
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

#[allow(dead_code)] // caller 在 Task 10 的状态统计 IPC，落地时删掉本行
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

#[allow(dead_code)] // caller 在 Task 10 的文件清单 IPC，落地时删掉本行
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
}
