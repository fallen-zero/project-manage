//! 索引作业。分成两层是刻意的：
//! - `run_pass` 纯函数，收 &Connection + 进度回调，内存库就能测完（取消、增量、重建、不可达根目录）。
//! - `start` 只是把回调换成 Tauri 事件，并维护 running/cancel 标志。它不在本任务的测试范围里，
//!   正确性由 Task 12 的真机验证承担 —— 别在这里写只能证明 mock 的测试。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db;
use crate::error::{AppError, AppResult};
use crate::extract::extract_text;
use crate::index_scan::{scan_root, ScanOptions};
use crate::index_store::{clear_project, current_rowid, write_doc, DocOutcome};

pub const PROGRESS_EVENT: &str = "index://progress";
/// 每 20 个文件回一次进度：5 万文件全量跑，逐文件 emit 会把事件队列压垮，
/// 而界面上「10 秒动一次」已经够用。项目边界与结尾一定额外发一次。
pub const PROGRESS_EVERY: i64 = 20;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanTarget {
    pub project_id: String,
    pub project_name: String,
    pub root_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// running | done | cancelled | error
    pub state: &'static str,
    pub project_id: String,
    pub project_name: String,
    pub total: i64,
    pub done: i64,
    pub ok: i64,
    pub skipped: i64,
    pub failed: i64,
    /// 当前正在处理的文件路径，用于界面显示「在做什么」。只读展示，不落日志。
    pub current: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResult {
    pub project_id: String,
    pub project_name: String,
    pub scanned_total: i64,
    pub ok: i64,
    pub skipped: i64,
    pub failed: i64,
    pub unchanged: i64,
    pub walk_errors: i64,
    pub capped: bool,
    pub root_missing: bool,
}

impl ProjectResult {
    /// 已处理数由四个计数器算出，不另存一个 `done` 字段 —— 少一处能写错的地方。
    fn done_total(&self) -> i64 {
        self.ok + self.skipped + self.failed + self.unchanged
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub state: &'static str,
    pub results: Vec<ProjectResult>,
    pub started_at: String,
    pub finished_at: String,
}

/// 只取未删除项目的 root 目录：一个项目一个根，是「一个根一个扫描任务」的前提（spec 数据模型）。
pub fn targets(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<ScanTarget>> {
    let sql = "SELECT d.project_id, p.name, d.path
                 FROM project_dirs d
                 JOIN projects p ON p.id = d.project_id
                WHERE d.kind = 'root' AND p.deleted_at IS NULL
                  AND (?1 IS NULL OR d.project_id = ?1)
                ORDER BY p.name";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![project_id], |r| {
        Ok(ScanTarget {
            project_id: r.get(0)?,
            project_name: r.get(1)?,
            root_path: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// 时间戳走 SQLite 的 `datetime('now')`：`chrono` 虽在 spec 第七节的清单里，但 Cargo.toml
/// 目前没引（M0-M2 没用到），为一个字符串新增依赖不划算，且这样和 V1-V4 里 `created_at` 同源。
fn now(conn: &Connection) -> String {
    conn.query_row("SELECT datetime('now')", [], |r| r.get(0)).unwrap_or_default()
}

pub fn run_pass(
    conn: &Connection,
    opts: &ScanOptions,
    project_id: Option<&str>,
    rebuild: bool,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> AppResult<RunSummary> {
    let started_at = now(conn);
    let mut results = Vec::new();

    for target in targets(conn, project_id)? {
        if cancel.load(Ordering::Relaxed) {
            return Ok(RunSummary { state: "cancelled", results, started_at, finished_at: now(conn) });
        }
        let mut acc = ProjectResult {
            project_id: target.project_id.clone(),
            project_name: target.project_name.clone(),
            ..Default::default()
        };
        let root_missing = !std::path::Path::new(&target.root_path).is_dir();
        acc.root_missing = root_missing;
        let outcome = if root_missing {
            // 根目录不在（盘没挂载）时连 walk 都不发起：walkdir 会逐条吐错误，白跑一趟。
            Default::default()
        } else {
            scan_root(std::path::Path::new(&target.root_path), opts)
        };
        if rebuild && !root_missing {
            clear_project(conn, &target.project_id)?;
        }
        acc.walk_errors = outcome.walk_errors.len() as i64;
        acc.capped = outcome.capped;

        // 超限文件也要建行：spec 要求「超限记 skipped + 原因」，这样 UI 才能回答「为什么它不在结果里」
        let mut queue: Vec<(crate::index_scan::ScannedFile, bool)> = outcome
            .files
            .iter()
            .cloned()
            .map(|f| (f, false))
            .chain(outcome.over_size.iter().cloned().map(|f| (f, true)))
            .collect();
        queue.sort_by(|a, b| a.0.path.cmp(&b.0.path));
        acc.scanned_total = queue.len() as i64;

        for (file, oversize) in queue {
            if cancel.load(Ordering::Relaxed) {
                on_progress(progress_for(&target, &acc, "cancelled", &file.path));
                // 半成品也要交出去：少这一行，取消时返回的 `results` 是**空表**，
                // 摘要里连被取消的那个项目都不在，Task 11 的界面只能显示「什么都没做」。
                // 探针实测原版没有这行时 `cancel_stops_the_pass_early` 当场 panic：
                // `index out of bounds: the len is 0 but the index is 0`。
                results.push(acc);
                return Ok(RunSummary { state: "cancelled", results, started_at, finished_at: now(conn) });
            }
            let handled = if oversize {
                write_doc(conn, &target.project_id, &file, DocOutcome::Skipped("too_large"))?;
                acc.skipped += 1;
                true
            } else if !rebuild
                && current_rowid(conn, &target.project_id, &file.path, file.size as i64, file.mtime)?
                    .is_some()
            {
                acc.unchanged += 1; // size + mtime 没变且上次 ok：连文件都不打开
                false
            } else {
                match extract_text(std::path::Path::new(&file.path)) {
                    Ok(body) if body.trim().is_empty() => {
                        // 扫描件没文字层是日常（spec 明写「抽出为空属于正常，不是失败」）。
                        // 库里那行由 write_doc 落成 skipped/empty_text，计数器就必须也加到 skipped 上，
                        // 否则 Task 11 的摘要说「N 个成功」而状态统计里它是 skipped，两个口径当场对不上。
                        // 顺带把 DocOutcome::Empty 变成有生产构造点（删枚举级豁免后它不然会报
                        // `variant Empty is never constructed`，见 Interfaces 的死代码账）。
                        write_doc(conn, &target.project_id, &file, DocOutcome::Empty)?;
                        acc.skipped += 1;
                    }
                    Ok(body) => {
                        write_doc(conn, &target.project_id, &file, DocOutcome::Ok(body))?;
                        acc.ok += 1;
                    }
                    Err(e) if e.code == "extract_unsupported" => {
                        // 走到这里说明清单与分派表漂移了（Task 5 的一致性测试没兜住）。
                        write_doc(conn, &target.project_id, &file, DocOutcome::Skipped("type_unsupported"))?;
                        acc.skipped += 1;
                    }
                    Err(e) => {
                        // 一个坏文件只废它自己：panic 已在 extract 层兜住，这里连 Err 也只落一行 failed。
                        write_doc(
                            conn,
                            &target.project_id,
                            &file,
                            DocOutcome::Failed(truncate(&e.message, 300)),
                        )?;
                        acc.failed += 1;
                    }
                }
                true
            };
            // 每 20 个有效处理回一次；unchanged 不占进度条的事件密度。
            if handled && (acc.ok + acc.skipped + acc.failed) % PROGRESS_EVERY == 0 {
                on_progress(progress_for(&target, &acc, "running", &file.path));
            }
        }
        on_progress(progress_for(&target, &acc, "running", ""));
        results.push(acc);
    }

    let state = if cancel.load(Ordering::Relaxed) { "cancelled" } else { "done" };
    Ok(RunSummary { state, results, started_at, finished_at: now(conn) })
}

fn truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// 进度事件由「当前扫描目标 + 累计计数」拼成。做成自由函数而不是循环外的闭包：
/// 闭包要同时按可变借用拿 `on_progress`、按借用拿循环内的 `target`，`run_pass` 里过不了借用检查。
fn progress_for(
    target: &ScanTarget,
    acc: &ProjectResult,
    state: &'static str,
    current: &str,
) -> Progress {
    Progress {
        state,
        project_id: target.project_id.clone(),
        project_name: target.project_name.clone(),
        total: acc.scanned_total,
        done: acc.done_total(),
        ok: acc.ok,
        skipped: acc.skipped,
        failed: acc.failed,
        current: current.to_owned(),
    }
}

pub struct IndexShared {
    pub cancel: AtomicBool,
    pub running: AtomicBool,
    pub summary: Mutex<Option<RunSummary>>,
}

impl IndexShared {
    #[allow(dead_code)] // 构造点在 Task 10 的 AppState::manage，落地时删掉本行
    pub fn new() -> Self {
        Self { cancel: AtomicBool::new(false), running: AtomicBool::new(false), summary: Mutex::new(None) }
    }
}

/// 薄壳：自己开一条连接（不能借用 AppState 里那条 —— 一轮索引要几分钟，
/// 拿着 Mutex<Connection> 会把界面上所有 IPC 全卡住），把进度回调换成 emit。
/// WAL + busy_timeout 已在 db::after_open 里配好，两个连接一读一写是允许的。
#[allow(dead_code)] // caller 在 Task 10 的 index_start IPC，落地时删掉本行
pub fn start(
    app: AppHandle,
    data_dir: PathBuf,
    shared: Arc<IndexShared>,
    project_id: Option<String>,
    rebuild: bool,
) -> AppResult<()> {
    if shared.running.swap(true, Ordering::SeqCst) {
        return Err(AppError::new(
            "index_running",
            "已有一轮索引在跑",
            Some("先等它结束，或在索引页点「取消」"),
        ));
    }
    shared.cancel.store(false, Ordering::SeqCst);
    std::thread::spawn(move || {
        let finish = |summary: Option<RunSummary>, shared: &Arc<IndexShared>| {
            if let Ok(mut g) = shared.summary.lock() {
                *g = summary;
            }
            shared.running.store(false, Ordering::SeqCst);
        };
        let conn = match db::open(&data_dir) {
            Ok(c) => c,
            Err(_) => {
                finish(None, &shared);
                return;
            }
        };
        let opts = ScanOptions::load(&conn).ok();
        let mut emit = |p: Progress| {
            let _ = app.emit(PROGRESS_EVENT, p);
        };
        let summary = match &opts {
            Some(o) => run_pass(&conn, o, project_id.as_deref(), rebuild, &shared.cancel, &mut emit).ok(),
            None => None,
        };
        if let Some(mut s) = summary {
            s.state = if shared.cancel.load(Ordering::SeqCst) { "cancelled" } else { "done" };
            finish(Some(s), &shared);
        } else {
            finish(None, &shared);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, index_scan::ScanOptions};
    use rusqlite::{params, Connection};
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    const MAX: u64 = 20 * 1024 * 1024;

    fn opts() -> ScanOptions {
        ScanOptions { exclude_dirs: vec!["node_modules".to_owned()], max_file_bytes: MAX, max_files_per_project: 50000 }
    }

    fn seeded_project(conn: &Connection, id: &str, root: &Path) {
        conn.execute("INSERT INTO projects (id, name, status) VALUES (?1, ?2, '立项')", params![id, "政务云迁移"])
            .unwrap();
        conn.execute(
            "INSERT INTO project_dirs (id, project_id, kind, path) VALUES (?1, ?2, 'root', ?3)",
            params![format!("{id}-d"), id, root.display().to_string()],
        )
        .unwrap();
    }

    /// 造一棵内容确定的树：两个可索引文本、一个超限、一个不支持类型、一个排除目录。
    fn tree(dir: &Path) {
        std::fs::create_dir_all(dir.join("合同/node_modules")).unwrap();
        std::fs::write(dir.join("合同/验收说明.txt"), "甲方要求验收指标".as_bytes()).unwrap();
        std::fs::write(dir.join("维保.txt"), "维保期为十二个月".as_bytes()).unwrap();
        std::fs::write(dir.join("大图.txt"), vec![b'a'; 300]).unwrap(); // 配合下面的小上限
        std::fs::write(dir.join("空扫描.txt"), "   \n\t".as_bytes()).unwrap(); // 抽取成功但正文空白
        std::fs::write(dir.join("图片.png"), b"\x89PNG").unwrap();
        std::fs::write(dir.join("合同/node_modules/x.txt"), "不该被索引".as_bytes()).unwrap();
    }

    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    /// 库里的行数。重建用例要拿它证明「磁盘上没了的旧行跟着掉」，
    /// 光看 `doc_hits` 不够：命中原问句可能只是因为那行根本没被写过。
    fn doc_rows(conn: &Connection) -> i64 {
        conn.query_row("SELECT count(*) FROM index_docs WHERE project_id = 'p1'", [], |r| r.get(0)).unwrap()
    }

    /// 一轮 pass 要把结局各归其位：ok / skipped(too_large) / skipped(empty_text，空白抽取) / 未支持不建行 / 排除目录不进。
    /// 同时验证「pass 结束后可检索」这条端到端性质（内存库 + 真文件）。
    #[test]
    fn run_pass_indexes_a_generated_tree_and_separates_outcomes() {
        let dir = tempfile::tempdir().unwrap();
        tree(dir.path());
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        let mut o = opts();
        o.max_file_bytes = 100;

        let mut events = Vec::new();
        let summary = run_pass(&conn, &o, None, false, &no_cancel(), &mut |p| events.push(p.clone())).unwrap();
        assert_eq!(summary.state, "done");
        let one = &summary.results[0];
        assert_eq!(one.ok, 2, "两个正常文本文件应入库：{one:?}");
        assert_eq!(one.skipped, 2, "超限与空白抽取各一行都该记 skipped：{one:?}");
        assert_eq!(one.failed, 0);
        assert_eq!(one.scanned_total, 4, "未支持与排除目录不该计数：{one:?}");
        assert!(!events.is_empty(), "至少要有一次进度回调");
        assert_eq!(events.last().unwrap().done, 4);
        // 计数器与库里的行必须同一个口径：空白抽取在 index_docs 上是 skipped/empty_text，不是 ok。
        // 少这条断言，`acc.ok += 1` 与 write_doc 落库的 status 各自漂移也不会有人喊 ——
        // 而 Task 11 的界面要同时读这两个数（摘要读计数器、状态统计读 status_counts）。
        let blank: (String, Option<String>) = conn
            .query_row(
                "SELECT index_status, skip_reason FROM index_docs WHERE path LIKE '%空扫描.txt'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(blank, ("skipped".to_string(), Some("empty_text".to_string())), "空白抽取的行口径要和计数器一致：{blank:?}");

        let hits = crate::index_store::doc_hits(&conn, "验收", 20).unwrap();
        assert_eq!(hits.len(), 1, "pass 之后必须搜得到：{hits:?}");
        assert!(crate::index_store::doc_hits(&conn, "不该被索引", 20).unwrap().is_empty(),
            "排除目录里的内容混进了索引");
    }

    /// 增量：第二趟一个文件都不该再读（unchanged 计数），并且状态行不会被写坏。
    #[test]
    fn second_pass_skips_unchanged_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::write(dir.path().join("验收.txt"), "甲方要求验收指标".as_bytes()).unwrap();
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());

        let first = run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(first.results[0].ok, 1);
        assert_eq!(first.results[0].unchanged, 0);

        let second = run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(second.results[0].unchanged, 1, "size+mtime 没变的 ok 行不该重读：{:?}", second.results[0]);
        assert_eq!(second.results[0].ok, 0, "没重读就不该再计 ok");
        assert_eq!(crate::index_store::doc_hits(&conn, "验收", 20).unwrap().len(), 1, "跳过后仍可搜");
    }

    /// 取消：第一个进度回调就把标志立起来，pass 必须停在中间。
    #[test]
    fn cancel_stops_the_pass_early() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        for i in 0..60 {
            std::fs::write(dir.path().join(format!("文件{i}.txt")), format!("正文{i} 验收").as_bytes()).unwrap();
        }
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        let cancel = AtomicBool::new(false);

        let summary = run_pass(&conn, &opts(), None, false, &cancel, &mut |_p| {
            cancel.store(true, Ordering::Relaxed);
        })
        .unwrap();
        assert_eq!(summary.state, "cancelled");
        let done = summary.results[0].ok + summary.results[0].skipped;
        assert!(done < 60, "取消后不该把 60 个文件全跑完：{done}");
        assert!(done > 0, "至少要真的开始过，否则这条测试什么都没证明");
    }

    /// 重建：先把该项目的行与 FTS 清掉再写。两件事都要钉住 ——
    /// ① 内容变了的文件必须重读；② 磁盘上已经没了的旧行不能留。
    /// ②才是 `clear_project` 唯一承重的场景：`write_doc` 的 `ON CONFLICT DO UPDATE` 会自己
    /// 换掉同路径旧正文，所以「同一文件改了内容」根本测不到它，只有「这一轮不再出现的旧行」测得到。
    #[test]
    fn rebuild_rewrites_everything_and_leaves_no_stale_tokens() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        let p = dir.path().join("合同.txt");
        std::fs::write(&p, "第一版 验收".as_bytes()).unwrap();
        let gone = dir.path().join("归档说明.txt");
        std::fs::write(&gone, "这份要归档 验收".as_bytes()).unwrap();
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(doc_rows(&conn), 2, "第一趟两行都该在库里");
        assert_eq!(crate::index_store::doc_hits(&conn, "归档", 20).unwrap().len(), 1,
            "正面照控：被删的那个文件第一趟确实进了索引，否则后面的 0 命中什么都没证明");

        std::fs::write(&p, "第二版 报价".as_bytes()).unwrap();
        std::fs::remove_file(&gone).unwrap();
        let summary = run_pass(&conn, &opts(), None, true, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(summary.results[0].ok, 1, "重建必须重读：{:?}", summary.results[0]);
        assert_eq!(crate::index_store::doc_hits(&conn, "验收", 20).unwrap().len(), 0, "旧正文要跟着清掉");
        assert_eq!(crate::index_store::doc_hits(&conn, "报价", 20).unwrap().len(), 1);
        assert_eq!(crate::index_store::doc_hits(&conn, "归档", 20).unwrap().len(), 0, "磁盘上没了的旧行不能留");
        assert_eq!(doc_rows(&conn), 1, "库里的行数也要跟着掉");
    }

    /// 根目录没挂载是日常（移动盘/网络盘）。它只能让该项目自己标 root_missing，
    /// 不能中断整轮，也不能把别的项目的结果冲掉。
    #[test]
    fn missing_root_is_flagged_without_killing_the_run() {
        let good = tempfile::tempdir().unwrap();
        std::fs::write(good.path().join("验收.txt"), "甲方要求验收".as_bytes()).unwrap();
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", good.path());
        conn.execute("INSERT INTO projects (id, name, status) VALUES ('p2', '换盘项目', '立项')", []).unwrap();
        conn.execute("INSERT INTO project_dirs (id, project_id, kind, path) VALUES ('p2d', 'p2', 'root', 'Z:/没挂载/根目录')", [])
            .unwrap();

        let summary = run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(summary.state, "done");
        let p1 = summary.results.iter().find(|r| r.project_id == "p1").unwrap();
        let p2 = summary.results.iter().find(|r| r.project_id == "p2").unwrap();
        assert_eq!(p1.ok, 1, "可达项目照常索引：{p1:?}");
        assert!(p2.root_missing, "不可达项目必须被点名：{p2:?}");

        // 只指定 p2 时也必须正常返回（而不是 Err），UI 才能给出「根目录不可达」的提示
        let only_p2 = run_pass(&conn, &opts(), Some("p2"), false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(only_p2.results.len(), 1);
        assert!(only_p2.results[0].root_missing);
    }
}
