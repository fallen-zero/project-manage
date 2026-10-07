//! 索引作业。分成两层是刻意的：
//! - `run_pass` 纯函数，收 &Connection + 进度回调，内存库就能测完（取消、增量、重建、不可达根目录）。
//! - `start` 把回调换成 Tauri 事件，并负责 running/cancel 两个标志的生命周期（`RunningGuard`
//!   的 Drop 复位、`spawn` 起不来时返回错误而不是 panic）。它的线程体不在本任务的测试范围里
//!   （要 `AppHandle`），正确性由 Task 12 的真机验证承担 —— 别在这里写只能证明 mock 的测试；
//!   但 `RunningGuard` 的复位语义是可测的，`running_flag_is_released_even_when_the_job_panics` 钉它。

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
    /// running | done | cancelled | error。四个都有构造点：`running` 由 `progress_for` 发，
    /// `done`/`cancelled` 与 `error` 由 `terminal` 发（修复轮之前 `done`/`error` 一个都没有，
    /// 只订阅事件的前端因此等不到「这一轮结束了」的信号）。
    pub state: &'static str,
    pub project_id: String,
    pub project_name: String,
    pub total: i64,
    pub done: i64,
    pub ok: i64,
    pub skipped: i64,
    pub failed: i64,
    /// 当前正在处理的文件路径，用于界面显示「在做什么」。只读展示，不落日志。
    /// 终止事件（`terminal`）里是空串：那一刻不属于任何一个文件。
    pub current: String,
    /// 只有 `state == "error"` 时有值，是给界面直接念的一句话（含 `[code]` 前缀）。
    /// 单独立一个字段而不是塞进 `current`：`current` 的契约是文件路径，混用会让 Task 11
    /// 的「当前文件」栏显示成错误文案。
    pub error: Option<String>,
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

/// 无根项目回收：把「登记中没有 root 行的项目不拥有任何索引行」这条不变量做成**每轮自动收敛**。
/// 为什么必须有它（终审定向复审 I-1）：`run_pass` 只在轮次开头取一次 `targets()` 快照，作业运行中
/// 注销根目录时 `project::remove_dir` 的 `clear_project` 先把行删掉，那一轮却仍拿着旧快照继续为
/// 该项目的文件 `write_doc` ⇒ 「可被搜到的陈旧行」悄悄长回来，而没有 root 行的项目不再出现在
/// `targets()` 里，`clear_project` 对它重新变成不可达。`db.rs:70` 的 `busy_timeout` 只兜撞锁，兜不住这种覆写。
/// 为什么软删项目不在范围内（`p.deleted_at IS NULL` 这一条不许省掉也不许写反）：软删项目的行是
/// **故意留着**给恢复用的，`index_store::SELECT_SQL` 已经用同一个条件把它们挡在检索外，
/// sweep 若顺手清掉，就等于把「恢复项目」变成「重建几十 GB 索引」。这条由
/// `the_sweep_spares_a_soft_deleted_project` 钉住。
/// 收敛时机**不是即时**：本轮末尾清一次，sweep 之后才发生的注销要等下一轮末尾；
/// 两条 cancel 早退路径不做额外写库（见 `run_pass` 末尾调用点的注释）。
pub fn sweep_unrooted_projects(conn: &Connection) -> AppResult<usize> {
    let ids: Vec<String> = conn
        .prepare(
            "SELECT p.id FROM projects p
              WHERE p.deleted_at IS NULL
                AND NOT EXISTS (SELECT 1 FROM project_dirs d WHERE d.project_id = p.id AND d.kind = 'root')",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in &ids {
        clear_project(conn, id)?;
    }
    Ok(ids.len())
}

/// 时间戳走 SQLite 的 `datetime('now')`：`chrono` 虽在 spec 第七节的清单里，但 Cargo.toml
/// 目前没引（M0-M2 没用到），为一个字符串新增依赖不划算，且这样和 V1-V4 里 `created_at` 同源。
fn now(conn: &Connection) -> String {
    conn.query_row("SELECT datetime('now')", [], |r| r.get(0)).unwrap_or_default()
}

/// 把「可能 panic 的调用」统一转成 `AppResult`：panic → `Err(extract_failed)`，
/// 于是 `run_pass` 的失败 arm 同时接住普通错误与 panic，红线的实现只有一条路径。
/// 为什么必须有这一层：`技术方案.md` 2.2 把 `pdf-extract` **和 zip+XML 解析**并列列为 panic 源，
/// 而抽取层里只有 `pdf_text` 自己包了 `catch_unwind`；docx/pptx/xlsx/txt 四条路径的 panic
/// 原本会一路炸穿 `run_pass`。前提：release 保持 unwind（见
/// `extract::tests::release_profile_does_not_abort_so_panic_guards_work`），否则这里抓不到任何东西。
fn panic_to_err<T, F: FnOnce() -> T>(f: F, what: &str) -> AppResult<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => Ok(v),
        Err(_) => Err(AppError::new(
            "extract_failed",
            &format!("抽取器 panic，已兜住并跳过：{what}"),
            Some("该文件本轮不索引；这类文件可换工具另存一份再登记"),
        )),
    }
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
                // panic 边界打在调用点上：抽取层只有 `pdf_text` 自己包了 catch_unwind，
                // docx/pptx/xlsx/txt 四条路径的 panic 原本会直接炸穿本轮。panic 转成 `Err` 之后
                // 落进下面同一条失败 arm，于是「一个坏文件只废它自己」是代码事实而不是注释事实。
                // `panic_to_err` 的 T 在这里就是 `AppResult<String>`（双层 Result），所以用
                // and_then 拍平一层，好让下面四个 arm 的语义一个都不必改。
                match panic_to_err(|| extract_text(std::path::Path::new(&file.path)), &file.path).and_then(|r| r) {
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
                        // 一个坏文件只废它自己：panic 已在本 pass 的调用点经 `panic_to_err` 兜成 Err，
                        // 普通抽取错误本来就是 Err —— 两条都只落这一行 failed，整轮继续。
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

    // 轮末回收（成因修法，见 `sweep_unrooted_projects` 的头注释）：把「没有 root 行的项目不拥有索引行」
    // 这一条在本轮末尾收敛掉，历史残留与运行中注销被旧快照写回的那批行都走这里，用户不需要任何操作。
    // 返回的被清项目数刻意丢弃：塞进 `RunSummary` 要连带改 `src/types/index.ts`、store、页面与 IPC 契约，
    // 越出本轮范围；代价记在 `docs/开发进度.md` —— 界面上看不到「刚才替你回收了几行」。
    // 上面两条 cancel 早退路径（循环开头那条、循环内 `cancelled` 那条）刻意不加 sweep：一轮被用户
    // 取消时不做额外写库，垃圾行留给下一轮末尾收敛，这是取舍不是漏写。
    // 而下面这行 `state` 即使算出 `"cancelled"` 也不属于那两条 —— 那是「循环跑完后才收到取消」，
    // 本轮的活已经干完，所以 sweep 照常在返回之前执行，不要再为它补一个条件。
    let _ = sweep_unrooted_projects(conn)?;
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
        error: None,
    }
}

/// 轮级终止事件。`run_pass` 只在项目边界发「这个项目」的进度，一轮结束/失败没有任何事件，
/// 只订阅事件的前端就只能反过来轮询 summary。`done`/`cancelled`/`error` 三个状态全靠这里发。
/// `project_id`/`project_name` 在终止事件里是空串 —— 它不属于某一个项目，Task 11 别拿它当 key。
fn terminal(state: &'static str, results: &[ProjectResult], error: Option<String>) -> Progress {
    let sum = |f: fn(&ProjectResult) -> i64| results.iter().map(f).sum();
    Progress {
        state,
        project_id: String::new(),
        project_name: String::new(),
        total: sum(|r| r.scanned_total),
        done: sum(|r| r.done_total()),
        ok: sum(|r| r.ok),
        skipped: sum(|r| r.skipped),
        failed: sum(|r| r.failed),
        current: String::new(),
        error,
    }
}

/// `running` 唯一的复位点。线程体正常走完、提前 `return`、还是 panic unwind，Drop 都会执行。
/// 少了它，一次 panic 就把 `running` 永久留在 true —— 界面上再也起不动第二轮，只能重启应用，
/// 而且没有任何日志说明为什么。`start` 里不许再出现第二处 `running.store(false)`
/// （唯一例外是 `spawn` 返回 Err 时守卫根本没构造出来，只能就地复位）。
struct RunningGuard(Arc<IndexShared>);

impl RunningGuard {
    fn enter(shared: &Arc<IndexShared>) -> Self {
        Self(Arc::clone(shared))
    }
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::SeqCst);
    }
}

pub struct IndexShared {
    pub cancel: AtomicBool,
    pub running: AtomicBool,
    pub summary: Mutex<Option<RunSummary>>,
    /// 上一轮为什么没跑成。三个失败出口（库打不开 / settings 读不出 / pass 中途报错）都必须
    /// 往这里写一句话，Task 10 的界面才能回答「点了索引按钮怎么什么都没发生」。
    /// 存 `format!("{e}")` 而不是 `AppError` 本体：它只 derive 了 `Debug`、没有 `Clone`，
    /// 而这里要跨线程留一份。代价是机器码混在串里，Task 10 若要按 code 分支再改存三元组。
    pub last_error: Mutex<Option<String>>,
}

impl IndexShared {
    pub fn new() -> Self {
        Self {
            cancel: AtomicBool::new(false),
            running: AtomicBool::new(false),
            summary: Mutex::new(None),
            last_error: Mutex::new(None),
        }
    }
}

/// 一轮彻底失败的出口：原因同时写进 `last_error`（给 Task 10 的 IPC 读）和一次 `error` 终止事件
/// （给只订阅事件的前端），两处用同一个串，界面上才不会「按钮说失败了、事件说没有」。
/// 失败轮的 `summary` 置 `None`：一轮没跑完就不该留下半截摘要，Task 11 显示的是「这一轮的结果」。
fn report_failure(emit: &mut impl FnMut(Progress), shared: &IndexShared, msg: String) {
    let event = terminal("error", &[], Some(msg.clone()));
    if let Ok(mut g) = shared.summary.lock() {
        *g = None;
    }
    if let Ok(mut g) = shared.last_error.lock() {
        *g = Some(msg);
    }
    emit(event);
}

/// 薄壳：自己开一条连接（不能借用 AppState 里那条 —— 一轮索引要几分钟，
/// 拿着 Mutex<Connection> 会把界面上所有 IPC 全卡住），把进度回调换成 emit。
/// WAL + busy_timeout 已在 db::after_open 里配好，两个连接一读一写是允许的。
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
    // 用 Builder 而不是 thread::spawn：后者在线程创建失败时**直接 panic**，而本机物理 16 GB、
    // commit charge 耗尽（os error 1455）是这轮开发里反复出现过的实况。起不来就返回错误，
    // 让 Task 10 的 IPC 能念出原因，而不是把 panic 丢进 Tauri 的命令线程里。
    let worker = Arc::clone(&shared);
    if let Err(e) = std::thread::Builder::new().name("index-job".to_owned()).spawn(move || {
        // emit 放在被兜底闭包**外面**：panic 那条路径也要能用它发终态事件。
        let mut emit = |p: Progress| {
            let _ = app.emit(PROGRESS_EVENT, p);
        };
        // 线程体最外层的兜底（终审 I1 (b)）：任何 panic 都要产生一个终态事件。
        // 少了这层，worker 炸掉时 RunningGuard 照样把 running 复位（后端不卡死），
        // 但 report_failure 不会被执行 —— 没有终态事件、没有 last_error、summary 停在上一轮，
        // 而前端 index-status.tsx:114-121 的复位链只认「收到终态事件」：徽章会永远停在
        // 「索引进行中」，取消按钮只能把 cancelRequested 吃成 true、非 remount 解不开。
        // 已核到这个前端失效形态，这就是补这层边界的全部理由。
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // 守卫留在被包的闭包**内部**：panic 展开时照样 Drop 复位 running
            // （running_flag_is_released_even_when_the_job_panics 钉的就是这条）。
            let _guard = RunningGuard::enter(&worker);
            let conn = match db::open(&data_dir) {
                Ok(c) => c,
                Err(err) => return report_failure(&mut emit, &worker, format!("{err}")),
            };
            let opts = match ScanOptions::load(&conn) {
                Ok(o) => o,
                Err(err) => return report_failure(&mut emit, &worker, format!("{err}")),
            };
            match run_pass(&conn, &opts, project_id.as_deref(), rebuild, &worker.cancel, &mut emit) {
                // state 由 run_pass 自己定（它末尾已经按 cancel 判过一次），这里不再重算第二遍。
                // 注意是 `Ok(s)` 而不是 `Ok(mut s)`：少了「外面再算一遍 state」之后没人改它，
                // 留着 `mut` 会被 `-D warnings` 打成 `variable does not need to be mutable`（控制方实测）。
                Ok(s) => {
                    let event = terminal(s.state, &s.results, None);
                    if let Ok(mut g) = worker.summary.lock() {
                        *g = Some(s);
                    }
                    if let Ok(mut g) = worker.last_error.lock() {
                        *g = None;
                    }
                    emit(event);
                }
                Err(err) => report_failure(&mut emit, &worker, format!("{err}")),
            }
        }));
        // 没 panic 的四条出口各自都发过**且只发过**一个终态事件（三个 report_failure 加 Ok 分支的
        // emit），所以这里只在 Err 时补发 —— 正常路径不会多出第二个终态事件。
        if ran.is_err() {
            report_failure(
                &mut emit,
                &worker,
                "索引作业 panic：作业线程在未知位置炸掉，本轮未完成".to_owned(),
            );
        }
    }) {
        // 线程根本没起来：闭包没跑过、守卫也没构造，`running` 只能在这里自己复位。
        shared.running.store(false, Ordering::SeqCst);
        return Err(AppError::new(
            "index_spawn_failed",
            &format!("索引线程启动失败：{e}"),
            Some("本机内存/提交空间不足时会出现，先关掉几个占内存大的程序再试"),
        ));
    }
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
    /// `project_id` 走参数而不是写死 `'p1'` —— `missing_root_...` 那条用例里同时有 p1 和 p2，
    /// 助手写死 id 会让它读出 0、然后以错误的理由变红。
    fn doc_rows(conn: &Connection, project_id: &str) -> i64 {
        conn.query_row("SELECT count(*) FROM index_docs WHERE project_id = ?1", [project_id], |r| r.get(0)).unwrap()
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

    /// 重建：先把该项目的行与 FTS 清掉再写。三件事都要钉住 ——
    /// ① 内容变了的文件必须重读；② 磁盘上已经没了的旧行不能留；③ 一个字节都没动的文件
    /// 在重建趟也必须被重读（`unchanged == 0`）。
    /// ②才是 `clear_project` 唯一承重的场景：`write_doc` 的 `ON CONFLICT DO UPDATE` 会自己
    /// 换掉同路径旧正文，所以「同一文件改了内容」根本测不到它，只有「这一轮不再出现的旧行」测得到。
    /// ③不是给 `!rebuild` 那个短路守卫当检测的 —— 探针实测：把 `} else if !rebuild` 改成
    /// `} else if true`，7 条**全绿**。原因是 rebuild 趟开头 `clear_project` 已把该项目的行全删了，
    /// `current_rowid` 必然回 None，所以那个短路在 rebuild 趟里是不可观测的死逻辑；它只在
    /// 「以后谁把 `clear_project` 挪走」时才显形，而那一改动同时会红 ②（见 Step 4 变异清单）。
    /// `不变.txt` 的真正价值：它是两轮之间唯一「路径、内容、mtime 全不变」的在场文件，
    /// 给 `doc_rows == 3` 与 `稳定 == 1` 两条正面照控提供落点 —— 没有它，本用例只能断「归零」。
    #[test]
    fn rebuild_rewrites_everything_and_leaves_no_stale_tokens() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        let p = dir.path().join("合同.txt");
        std::fs::write(&p, "第一版 验收".as_bytes()).unwrap();
        let gone = dir.path().join("归档说明.txt");
        std::fs::write(&gone, "这份要归档 验收".as_bytes()).unwrap();
        let same = dir.path().join("不变.txt");
        std::fs::write(&same, "这一份两轮都不动 稳定".as_bytes()).unwrap();
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(doc_rows(&conn, "p1"), 3, "第一趟三行都该在库里");
        // 正面照控：验收 只出现在 合同/归档说明 两个文件的**正文**里（两个文件名都没有它），
        // 所以 == 2 才真的证明正文进了索引。只查「归档」会被 name_tokens 满足，证不到正文那条腿。
        assert_eq!(crate::index_store::doc_hits(&conn, "验收", 20).unwrap().len(), 2,
            "正面照控：两份正文都在库里，否则后面的 0 命中什么都没证明");

        std::fs::write(&p, "第二版 报价".as_bytes()).unwrap();
        std::fs::remove_file(&gone).unwrap();
        let summary = run_pass(&conn, &opts(), None, true, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(summary.results[0].ok, 2, "重建必须把在场的全重读：{:?}", summary.results[0]);
        assert_eq!(summary.results[0].unchanged, 0, "重建趟不该有文件被跳过：{:?}", summary.results[0]);
        assert_eq!(crate::index_store::doc_hits(&conn, "验收", 20).unwrap().len(), 0, "旧正文要跟着清掉");
        assert_eq!(crate::index_store::doc_hits(&conn, "归档", 20).unwrap().len(), 0, "磁盘上没了的旧行不能留");
        assert_eq!(crate::index_store::doc_hits(&conn, "报价", 20).unwrap().len(), 1);
        assert_eq!(crate::index_store::doc_hits(&conn, "稳定", 20).unwrap().len(), 1, "没动的文件要被重写回来");
        assert_eq!(doc_rows(&conn, "p1"), 2, "库里的行数也要跟着掉");
    }

    /// 撞配额这一轮的口径：`capped` 必须原样透到 `ProjectResult`，`scanned_total` 必须是
    /// 「两桶之和」—— 它就是本轮会产生多少行，Task 11 的摘要与状态统计要拿它对账。
    /// `ScanOutcome` 里**没有**「被丢弃了几条」的字段，所以这里只能断「收下的那 3 条」，
    /// 别说「还差 M 个」；也别为了凑那个数去给 `ScanOutcome` 加字段。
    /// 没有这条用例，`acc.capped = outcome.capped` 与 `acc.walk_errors` 两行是零守护的赋值。
    #[test]
    fn capped_and_scanned_total_survive_the_quota() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..4 {
            std::fs::write(dir.path().join(format!("验收{i}.txt")), format!("第{i}份 报价").as_bytes()).unwrap();
        }
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        let mut o = opts();
        o.max_files_per_project = 3;

        let summary = run_pass(&conn, &o, None, false, &no_cancel(), &mut |_| {}).unwrap();
        let r = &summary.results[0];
        assert!(r.capped, "触顶必须透上来，否则 Task 11 说不出「为什么少了几个」：{r:?}");
        assert_eq!(r.scanned_total, 3, "两桶之和就是本轮的行数：{r:?}");
        assert_eq!(r.ok, 3);
        assert_eq!(r.walk_errors, 0, "触顶不是遍历错误，不该混进 walk_errors");
        assert_eq!(doc_rows(&conn, "p1"), 3, "被丢弃的文件既不建行也不计数");
    }

    /// `running` 只由 `RunningGuard` 的 Drop 复位。`start` 的线程体在本任务不可测（要 AppHandle），
    /// 但「一次 panic 把 running 永久留在 true、界面上再也起不动第二轮、只能重启应用」是这个模块
    /// 最贵的一类失效，必须有一处能变红的断言钉住守卫本身。
    #[test]
    fn running_flag_is_released_even_when_the_job_panics() {
        let shared = Arc::new(IndexShared::new());
        shared.running.store(true, Ordering::SeqCst);
        let s = Arc::clone(&shared);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guard = RunningGuard::enter(&s);
            panic!("作业里炸了，这条用例的前提就是它要炸");
        }))
        .is_err();
        assert!(panicked, "内部没有 panic，这条用例什么都没测");
        assert!(!shared.running.load(Ordering::SeqCst), "panic 也必须释放 running");
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

        // `rebuild && !root_missing` 的另一半：盘没挂载**不等于**文件被删了，重建不该顺手清掉
        // 这个项目已有的行。少了 `!root_missing` 这个条件，拔掉的移动盘会在下一次重建时
        // 把它的索引全冲成空表，而摘要照样报 done。
        // 用「先建后删的临时目录」造这个场景：p2 上面那条 SQL 里的 Z:/ 从来就没有行，
        // 拿它断「行数没变」是恒真的，什么也没证明。
        let removable = tempfile::tempdir().unwrap();
        std::fs::write(removable.path().join("验收.txt"), "甲方要求验收".as_bytes()).unwrap();
        let conn2 = db::open_in_memory().unwrap();
        seeded_project(&conn2, "p9", removable.path());
        run_pass(&conn2, &opts(), Some("p9"), false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(doc_rows(&conn2, "p9"), 1, "前提：这个项目的行真的在库里");
        drop(removable); // 目录一旦没了，本轮既扫不到它、也不该清掉它的历史行
        let after = run_pass(&conn2, &opts(), Some("p9"), true, &no_cancel(), &mut |_| {}).unwrap();
        assert!(after.results[0].root_missing, "{:?}", after.results[0]);
        assert_eq!(after.results[0].scanned_total, 0);
        assert_eq!(doc_rows(&conn2, "p9"), 1, "根不可达时 rebuild 不该动它已有的行");
    }

    /// 轮级终止事件的求和（修复轮 2 复审补的守护）。`terminal` 是纯函数、不要 `AppHandle`，
    /// 而 Task 11 的摘要条数全靠它 —— 它写错就是「界面说 19 个、状态统计说 17 个」这类当场对不上。
    /// 两个 `ProjectResult` 的数字**刻意全部互不相同**，且 `scanned_total` 之和不等于 `done_total()`
    /// 之和：否则 `total` 与 `done` 两个字段被互换时这条也绿。`walk_errors` 给非零值是为了钉住
    /// 「遍历错误不参与这两条计数」。
    #[test]
    fn terminal_event_sums_scanned_and_done_separately() {
        let r1 = ProjectResult {
            project_id: "p1".into(), project_name: "一号".into(),
            scanned_total: 12, ok: 4, skipped: 3, failed: 1, unchanged: 2,
            walk_errors: 5, capped: true, root_missing: false,
        };
        let r2 = ProjectResult {
            project_id: "p2".into(), project_name: "二号".into(),
            scanned_total: 7, ok: 5, skipped: 1, failed: 0, unchanged: 1,
            walk_errors: 0, capped: false, root_missing: true,
        };
        let e = terminal("done", &[r1, r2], None);
        assert_eq!(e.state, "done");
        assert_eq!(e.total, 19, "total 是 scanned_total 之和：{} vs {}", e.total, 19);
        assert_eq!(e.done, 17, "done 是 ok+skipped+failed+unchanged 之和");
        assert_eq!(e.ok, 9);
        assert_eq!(e.skipped, 4);
        assert_eq!(e.failed, 1);
        assert_eq!(e.project_id, "", "终止事件不属于某一个项目，Task 11 别拿它当 key");
        assert_eq!(e.project_name, "");
        assert_eq!(e.current, "");
        assert_eq!(e.error, None, "成功轮的终止事件不该带原因");
    }

    /// Important 2 的落地证据。`start` 的线程体要 `AppHandle`、本任务测不到，但 `report_failure`
    /// 收的是 `&IndexShared` 和一个 `FnMut` 闭包，**不需要** `AppHandle` —— 所以「失败留下一条可读的
    /// 原因 + 发且只发一条 error 终止事件 + 把上一轮的摘要冲掉」这三件事全部可以直接钉住。
    /// 没有这条，Important 2 的修复在测试层面是零守护的：三个失败出口都不可达，界面上「按钮说失败、
    /// 事件说没有」只能等 Task 12 真机才发现。
    /// `events.len() == 1` 挡的是「重复发事件」；`total == 0` 挡的是「把上一轮的计数塞进 error 事件」。
    #[test]
    fn report_failure_leaves_a_reason_and_emits_one_error_event() {
        let shared = IndexShared::new();
        {
            // 先放一份「上一轮成功」的摘要：失败轮必须把它冲掉，否则界面会拿旧摘要当本轮结果。
            let mut saved = shared.summary.lock().unwrap();
            *saved = Some(RunSummary {
                state: "done",
                results: vec![ProjectResult {
                    project_id: "p1".into(), project_name: "一号".into(),
                    scanned_total: 3, ok: 3, skipped: 0, failed: 0, unchanged: 0,
                    walk_errors: 0, capped: false, root_missing: false,
                }],
                started_at: "2026-09-30T00:00:00Z".into(),
                finished_at: "2026-09-30T00:01:00Z".into(),
            });
        }
        let mut events: Vec<Progress> = Vec::new();
        report_failure(&mut |p| events.push(p), &shared, "[db_open_failed] 打不开库".to_owned());

        assert_eq!(events.len(), 1, "只发一条轮级事件，发两遍 Task 11 的进度条会抖");
        assert_eq!(events[0].state, "error", "{:?}", events[0]);
        assert_eq!(events[0].error.as_deref(), Some("[db_open_failed] 打不开库"),
            "事件里的原因必须和 last_error 是同一个串");
        assert_eq!(events[0].project_id, "");
        assert_eq!(events[0].total, 0, "失败轮的轮级计数是 0，不是上一轮的残留");
        assert!(shared.summary.lock().unwrap().is_none(), "失败轮不该留下半截摘要");
        assert_eq!(shared.last_error.lock().unwrap().as_deref(), Some("[db_open_failed] 打不开库"),
            "Task 10 的 IPC 全靠这一条才分得清「从没跑过」和「跑挂了」");
    }

    /// panic 边界的直测（终审 I1）。动机写在这里：`extract_text` 四条路径里只有 `pdf_text`
    /// 在抽取层自己兜了 panic，docx/pptx/xlsx/txt 三条原本会直接炸穿本轮。
    /// **少这一条，把 `panic_to_err` 里的 `catch_unwind` 删掉也不会红** —— 真实文件在本机
    /// 构造不出 panic（那是 `开发进度.md` 的缺口 1），能构造的地方只有这里，所以这条直测不可替代。
    /// 只断 code / 文案前缀 / 透传值，不回显任何抽取正文。
    #[test]
    fn panic_to_err_turns_a_panic_into_an_extract_failure() {
        let boom = panic_to_err(
            || panic!("抽取器在畸形文件上炸了 —— 这条用例的前提就是它要炸"),
            "C:/坏文档.docx",
        )
        .unwrap_err();
        assert_eq!(boom.code, "extract_failed", "panic 必须落成 extract_failed，才能走 pass 那条失败 arm：{}", boom.code);
        assert!(boom.message.contains("panic"), "message 要能看出是 panic 被兜住的：{}", boom.message);
        assert!(boom.message.contains("C:/坏文档.docx"), "message 要带上是哪个文件，否则 /index 清单无法定位：{}", boom.message);
        assert!(boom.hint.is_some(), "要给界面一句可行动的建议");

        // 正常路径原样透传：兜底不许把值吃掉（吃掉的话整轮会变成「全都 failed」而照样报 done）。
        assert_eq!(panic_to_err(|| "正文两个词".to_owned(), "C:/好文件.txt").unwrap(), "正文两个词");
        assert_eq!(panic_to_err(|| 7u8, "C:/好文件.txt").unwrap(), 7);
    }

    /// 造一个「扩展名在 SUPPORTED 清单内、内容畸形到抽取层必然回 Err(extract_failed)」的 docx：
    /// zip 是真的，`word/document.xml` 的第一个标记就是一枚**悬空闭标签，且标签名长达 400 字**。
    /// 为什么不用简报里建议的「往 .docx 里写非 zip 的垃圾字节」：垃圾字节走 `ZipArchive::new` 的
    /// `解包失败：{e}`，而 zip 的 `ZipError::InvalidArchive(&'static str)` 不带任何动态内容，
    /// 整条 message 只有几十字，于是 `length(error_msg) <= 300` 会退化成恒真 —— 删掉 `truncate` 也不会红。
    /// 这里 quick-xml 会把那 400 字标签名回显进 `IllFormedError::UnmatchedEndTag` 的 Display
    /// （`check_end_names = false` 关不掉它：它走的是「栈空时遇到闭标签」那条分支），
    /// 原始 message 稳过 300 字，`truncate(&e.message, 300)` 因此被真的钉住。
    fn broken_docx(dir: &Path, name: &str) -> PathBuf {
        use std::io::Write;
        let p = dir.join(name);
        let mut w = zip::write::ZipWriter::new(std::fs::File::create(&p).unwrap());
        let zopts = zip::write::SimpleFileOptions::default();
        w.start_file("[Content_Types].xml", zopts).unwrap();
        w.write_all(br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#)
            .unwrap();
        w.start_file("word/document.xml", zopts).unwrap();
        let xml = format!(
            "</{}><w:document xmlns:w=\"http://x\"><w:body><w:r><w:t>畸形正文</w:t></w:r></w:body></w:document>",
            "y".repeat(400)
        );
        w.write_all(xml.as_bytes()).unwrap();
        w.finish().unwrap();
        p
    }

    /// 里程碑级红线「一个坏文件只废它自己、不打挂整轮」的 **pass 级**测试（终审 I2）。
    /// 既有夹具 `tree()` 里没有任何文件能让 `extract_text` 返回 Err，所以 `:529` 的
    /// `assert_eq!(one.failed, 0)` 恒真：把 `run_pass` 的失败 arm 整段删掉改成 `?` 上抛，
    /// 原来的 96 条一个都不会红 —— 这条测试存在的全部理由就是让那段代码有东西守着。
    /// 断言全部写具体数字，且额外钉住三件事：坏文件**照样有一行**（不静默丢弃）、
    /// `error_msg` 被 `truncate` 截到 300 字以内、终态事件计数与摘要一致。
    #[test]
    fn a_failing_file_only_kills_itself() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("验收.txt"), "甲方要求验收指标".as_bytes()).unwrap();
        std::fs::write(dir.path().join("维保.txt"), "维保期为十二个月".as_bytes()).unwrap();
        let bad = broken_docx(dir.path(), "坏文档.docx");
        // 前提：坏文件确实是 Err(extract_failed)，而不是 Err(extract_unsupported)（那会落 Skipped）。
        let probe = extract_text(&bad).unwrap_err();
        assert_eq!(probe.code, "extract_failed", "夹具要落进失败 arm 而不是 skipped：{}", probe.code);
        assert!(probe.message.chars().count() > 300,
            "夹具的原始 message 必须长于 300 字，否则长度断言对 `truncate` 是恒真的：{}",
            probe.message.chars().count());

        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        let mut events = Vec::new();
        let summary = run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |p| events.push(p.clone())).unwrap();

        assert_eq!(summary.state, "done", "一个坏文件不能把本轮变成 error/取消：{}", summary.state);
        let one = &summary.results[0];
        assert_eq!(one.failed, 1, "{one:?}");
        assert_eq!(one.ok, 2, "坏文件旁边的好文件要照常索引：{one:?}");
        assert_eq!(one.scanned_total, 3, "{one:?}");
        assert_eq!(one.skipped, 0, "{one:?}");
        assert_eq!(doc_rows(&conn, "p1"), 3, "坏文件照样要有一行 —— 失败要能被 /index 清单回答，不静默丢弃");

        let stored: (String, Option<String>, i64) = conn
            .query_row(
                "SELECT index_status, error_msg, length(error_msg) FROM index_docs WHERE path LIKE '%坏文档.docx'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(stored.0, "failed", "{:?}", stored.0);
        let msg = stored.1.unwrap_or_default();
        assert!(msg.starts_with("XML 解析失败"),
            "error_msg 要留下原因文案的前缀，界面上才念得出来：{}",
            msg.chars().take(24).collect::<String>());
        assert!(!msg.is_empty(), "失败行不许只有状态没有原因");
        assert!(stored.2 <= 300, "error_msg 要显示在 /index 清单里，必须被截断：{}", stored.2);
        assert_eq!(stored.2, 300,
            "夹具原始 message 实测 473 字（那 400 字标签名被 quick-xml 回显进 Display），所以这条才真的钉住 truncate(&e.message, 300)：删掉它这里会变成 473");

        // 终态事件的计数与摘要必须同一个口径（Task 11 的摘要条数全靠它）。
        let term = terminal(summary.state, &summary.results, None);
        assert_eq!((term.total, term.done, term.ok, term.skipped, term.failed), (3, 3, 2, 0, 1), "{term:?}");
        assert_eq!(term.error, None, "本轮没整轮失败，终止事件不该带原因");
        let last = events.last().unwrap();
        assert_eq!((last.done, last.ok, last.failed), (3, 2, 1), "项目边界的进度事件也要对得上：{last:?}");
        assert_eq!(crate::index_store::doc_hits(&conn, "验收", 20).unwrap().len(), 1, "其余文件仍可检索");
        assert_eq!(crate::index_store::fts_orphan_rows(&conn), 0, "一轮里有失败行也不该留下对不上主表的虚表行");
    }

    /// 不经磁盘扫描、直接给 `write_doc` 造入参。`path` 在这里只是**字符串键**（一行归属哪个路径），
    /// 测试不会去创建或读取它 —— 红线要求的夹具落点只有 `tempfile::tempdir()` 与 `db::open_in_memory()`。
    fn scanned(path: &str) -> crate::index_scan::ScannedFile {
        crate::index_scan::ScannedFile {
            path: path.to_owned(),
            file_name: path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned(),
            ext: "txt".to_owned(),
            size: 128,
            mtime: 1_700_000_000,
        }
    }

    /// 无根项目的残留行必须在轮末被自动回收（定向复审 I-1 的成因修法）。
    /// p2 刻意做成**只有行、没有 root 行** —— 那正是「作业运行中注销根目录、`clear_project` 已经删过一遍，
    /// 本轮却仍拿着 `targets()` 旧快照把行写回来」之后的形态；而 `targets()` 里不再有 p2，
    /// 重建循环连一次都够不着它（「点一次全量重建就能回收」是错的），所以只能由轮末的 sweep 收敛。
    /// 最后那条检索断言是关键项：只数行数会漏掉「虚表侧没删干净所以照样搜得到」。
    #[test]
    fn a_pass_reclaims_rows_left_by_an_unrooted_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("验收.txt"), "甲方要求验收指标".as_bytes()).unwrap();
        std::fs::write(dir.path().join("报价.txt"), "报价单里写着验收流程".as_bytes()).unwrap();
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        conn.execute("INSERT INTO projects (id, name, status) VALUES ('p2', '已注销根目录的项目', '立项')", []).unwrap();
        // 两行里一行带正文（会产生虚表行）、一行是 skipped（只有状态行）—— 两种形态都要被回收。
        write_doc(&conn, "p2", &scanned("C:/已注销/遗留说明.txt"), DocOutcome::Ok("遗留正文 付款条件".into())).unwrap();
        write_doc(&conn, "p2", &scanned("C:/已注销/遗留清单.txt"), DocOutcome::Skipped("too_large")).unwrap();

        // 正面照控（跑之前）：p2 的行真的在库里、正文真的搜得到，否则下面两条「没了」是恒真的。
        assert_eq!(doc_rows(&conn, "p2"), 2, "前提：p2 的两行都真的在库里");
        assert_eq!(crate::index_store::doc_hits(&conn, "付款条件", 20).unwrap().len(), 1,
            "前提：p2 的正文此刻确实可被搜到，否则末尾那条「搜不到」什么都没证明");

        let summary = run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(summary.state, "done", "{:?}", summary.state);

        assert_eq!(doc_rows(&conn, "p1"), 2, "有根项目的行一格不许动");
        assert_eq!(crate::index_store::doc_hits(&conn, "验收", 20).unwrap().len(), 2, "p1 的检索结果不许受影响");
        assert_eq!(doc_rows(&conn, "p2"), 0, "无根项目的残留行必须在轮末被回收");
        assert_eq!(
            conn.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get::<_, i64>(0)).unwrap(),
            2,
            "虚表侧只剩 p1 的那两行"
        );
        assert_eq!(crate::index_store::fts_orphan_rows(&conn), 0, "回收必须双侧对齐，不许留下孤儿虚表行");
        assert!(crate::index_store::doc_hits(&conn, "付款条件", 20).unwrap().is_empty(),
            "还搜得到就说明行没删干净 —— 这一格比行数断言更接近用户看到的失效形态");
    }

    /// sweep 不许顺手清掉软删项目的行：`p.deleted_at IS NULL` 这一条件的专属断言。
    /// 软删是「先单人、预留多人」的删除语义，行留着才谈得上恢复；`index_store::SELECT_SQL` 已经靠
    /// 同一个条件把它们挡在检索外，所以「搜不到」根本不需要靠删行来达成 —— 清掉只会把恢复变成重抽几十 GB。
    /// 两个项目都软删、都留有行，但只有一个能守住那条变异：
    /// - `p2` 留着 root 行（复审简报字面写的形状）：它靠 `NOT EXISTS (… kind = 'root')` 就被放行，
    ///   把 `deleted_at IS NULL` 删掉它照样不动 ⇒ 对那次变异是恒绿的；
    /// - `p3` 已经没有 root 行（先注销根、行被旧快照写回来、随后项目被软删）：只有它让
    ///   `deleted_at IS NULL` 承重 —— 删掉那个条件，p3 当场被 sweep 清成 0 行，本用例红。
    ///
    /// 两条都走 `crate::project` 的既有函数（`remove_dir` / `delete_project`），不手写软删 SQL。
    #[test]
    fn the_sweep_spares_a_soft_deleted_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("验收.txt"), "甲方要求验收指标".as_bytes()).unwrap();
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());

        conn.execute("INSERT INTO projects (id, name, status) VALUES ('p2', '软删但留着根目录', '立项')", []).unwrap();
        conn.execute("INSERT INTO project_dirs (id, project_id, kind, path) VALUES ('p2-d', 'p2', 'root', 'Z:/没挂载/根目录')", [])
            .unwrap();
        write_doc(&conn, "p2", &scanned("C:/软删/合同说明.txt"), DocOutcome::Ok("软删项目遗留 付款条件".into())).unwrap();
        crate::project::delete_project(&conn, "p2").unwrap();

        seeded_project(&conn, "p3", dir.path());
        crate::project::remove_dir(&conn, "p3", "p3-d").unwrap(); // root 行没了（此刻库里还没行）
        write_doc(&conn, "p3", &scanned("C:/软删/维保说明.txt"), DocOutcome::Ok("软删且无根遗留 付款条件".into())).unwrap();
        crate::project::delete_project(&conn, "p3").unwrap();

        assert_eq!(doc_rows(&conn, "p2"), 1, "前提：p2 的行在库里");
        assert_eq!(doc_rows(&conn, "p3"), 1, "前提：p3 的行在库里");
        let fts_before: i64 = conn
            .query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get(0))
            .unwrap();

        let summary = run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(summary.state, "done", "{:?}", summary.state);
        assert_eq!(summary.results.iter().filter(|r| r.project_id == "p1").count(), 1, "p1 照常进了本轮");
        assert_eq!(doc_rows(&conn, "p1"), 1, "对照：活着的有根项目该有一行");

        assert_eq!(doc_rows(&conn, "p2"), 1, "软删且留着 root 行：行不许动");
        assert_eq!(doc_rows(&conn, "p3"), 1, "软删且没有 root 行：行同样不许被 sweep 清掉，那是留给恢复的");
        assert_eq!(
            conn.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get::<_, i64>(0)).unwrap(),
            fts_before + 1,
            "虚表侧只该多出 p1 那一行，软删项目的两行都还在原地"
        );
        assert!(crate::index_store::doc_hits(&conn, "付款条件", 20).unwrap().is_empty(),
            "软删项目的内容靠 deleted_at 过滤挡住，不是靠删行 —— 行在库里而搜不到才是对的");
        assert_eq!(crate::index_store::doc_hits(&conn, "验收", 20).unwrap().len(), 1, "活项目照常可搜");
    }
}
