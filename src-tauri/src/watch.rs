//! 文件监听（M5.5 唯一新机制）。这一层**只置一个脏布尔**，绝不碰 SQLite：
//! notify 回调跑在 notify 内部线程，若在那儿直接写库就绕过了 index_job 的 running 单飞闸门。
//! 真正的追平仍由 ticker 调既有 index_job::start(None, rebuild=false) 走那一整轮（spec D1/D3）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{AppError, AppResult};

/// 「自上次追平之后有没有过文件事件」的唯一位点。D2：不是项目集合，就是一个布尔
/// （index_job::start 只吃单个 Option<String> project_id、targets() 追平轮本就该扫全部）。
///
/// 这里**只放 dirty 一位**：「有没有轮在跑」不在这里存 —— `IndexShared.running` 是裸
/// `AtomicBool` 字段（Arc 套在 `IndexShared` 外，`index_job.rs:397`），ticker 手里本就持
/// `Arc<IndexShared>`，直接把 `index.running.load()` 作为实参传给 `due_for_pass` 即可
/// （Task 7 评审裁定 1：`index_job.rs` 零改动，也不去把 `IndexShared.running` 改成 `Arc`）。
pub struct WatchShared {
    pub dirty: AtomicBool,
}

impl WatchShared {
    pub fn new() -> Self {
        Self {
            dirty: AtomicBool::new(false),
        }
    }
}

pub fn mark_dirty(shared: &WatchShared) {
    shared.dirty.store(true, Ordering::SeqCst);
}

/// ticker 的决策：见脏且空闲 ⇒ 清脏并返回 true。
/// running 判定必须在 swap **之前**（spec 第四节①）：running 由调用方（ticker）从
/// `IndexShared.running` 现读现传（裁定 1），不吞脏位。
/// 「先起轮后清脏」会让轮跑期间进来的新事件被下一轮重复起，而 running 时消费脏位
/// 会让轮结束后再也追不平这些事件（那是真漏）。
pub fn due_for_pass(shared: &WatchShared, running_now: bool) -> bool {
    // running 时**不消费**脏位（否则自动轮跑了几分钟、期间的事件被 swap 清空，轮结束后
    // 这些事件永不触发追平 —— 那是真漏）。空闲时才 swap-and-clear 起轮。
    if running_now {
        return false;
    }
    shared.dirty.swap(false, Ordering::SeqCst)
}

/// 列出所有「未软删项目」的主根目录路径（`kind = 'root'`）。
/// 一条 SQL 只回明文字符串，不碰任何敏感列，也不写库。
/// entry 目录与软删项目的 root 都不进来（评审裁定：只监听项目主根，递归覆盖其下全部内容）。
fn list_watchable_roots(conn: &rusqlite::Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT d.path FROM project_dirs d JOIN projects p ON p.id = d.project_id
          WHERE d.kind = 'root' AND p.deleted_at IS NULL",
    )?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 起 notify watcher：监听所有「未软删且有 root 且 root 真实存在」的项目目录，
/// 事件回调只做 `mark_dirty`（红线 1：绝不在回调里写库）。
/// 单个 root 注册失败（不可达是常态用法）静默跳过、不写 last_error、不 panic、不阻断启动
/// （spec 第六节）。watcher 由一个 `.name("file-watch")` 常驻线程持有并阻塞存活
/// —— 它必须活过整个进程，交给线程托管而非 mem::forget；进程退出即回收。
pub fn spawn_watcher(data_dir: &std::path::Path, shared: Arc<WatchShared>) -> AppResult<()> {
    // 只读列 root：开一条自己的连接、读完即 drop，不占用 AppState 那把全局锁。
    let conn = crate::db::open(data_dir)?;
    let mut roots = list_watchable_roots(&conn)?;
    // 过滤掉当前不可达的（is_dir 为假的不注册，避免噪声）。
    roots.retain(|p| std::path::Path::new(p).is_dir());
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            if res.is_ok() {
                mark_dirty(&shared); // ← 唯一副作用：置脏。绝不在回调里写库。
            }
        },
        Config::default(),
    )
    .map_err(|e| {
        AppError::new(
            "watch_failed",
            &format!("文件监听初始化失败：{e}"),
            Some("确认数据目录可读；监听失败不影响手动索引"),
        )
    })?;
    for r in roots {
        // 单个注册失败静默跳过（不可达 / 权限 / 平台限制都不该炸掉整个监听或启动）。
        let _ = watcher.watch(std::path::Path::new(&r), RecursiveMode::Recursive);
    }
    // watcher 必须活过整个进程 —— 交给常驻线程持有并阻塞（线程存活期间 _keep 不被 drop）。
    std::thread::Builder::new()
        .name("file-watch".into())
        .spawn(move || {
            let _keep = watcher;
            loop {
                std::thread::park();
            }
        })
        .map_err(|e| {
            AppError::new(
                "watch_failed",
                &format!("文件监听线程启动失败：{e}"),
                Some("确认系统资源充足；监听失败不影响手动索引"),
            )
        })?;
    Ok(())
}

/// ticker：每 5 s 看一次，脏且空闲就起一轮整库追平（复用 index_job::start 的全部语义）。
/// 它同时是去抖器 —— 5 s 窗口内几百条事件只起一轮（spec D3）。
/// running 判据现读现传自 `IndexShared.running`（裁定 1）。常驻线程生命周期本机不可测，交真机。
pub fn spawn_ticker(
    app: tauri::AppHandle,
    data_dir: std::path::PathBuf,
    shared: Arc<WatchShared>,
    index: Arc<crate::index_job::IndexShared>,
) {
    std::thread::Builder::new()
        .name("watch-ticker".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            // running_now 直接从 index.running 读，不新增共享字段（裁定 1）。
            if due_for_pass(&shared, index.running.load(Ordering::SeqCst)) {
                match crate::index_job::start(
                    app.clone(),
                    data_dir.clone(),
                    Arc::clone(&index),
                    None,
                    false,
                ) {
                    Ok(_) => {}
                    // 裁定 2：把脏位放回去。从 due_for_pass 判空闲、swap 清脏，到 start 内部
                    // 那次 `running.swap(true)` 之间有一个毫秒窗口 —— 若这期间用户手动抢先起了
                    // 一轮，start 返回 index_running，而我们已把脏位清掉了，那批文件事件就永久丢
                    // 了。放回脏位，下一拍 ticker 会重试（真跑起来的那轮若失败由 index_job 自写 last_error）。
                    Err(_) => mark_dirty(&shared),
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_shared_is_not_dirty() {
        let w = WatchShared::new();
        assert!(!due_for_pass(&w, false), "初值必须未占用（M5 同款钉法）");
    }

    #[test]
    fn mark_dirty_then_due_consumes_the_flag_once() {
        let w = WatchShared::new();
        mark_dirty(&w);
        assert!(due_for_pass(&w, false), "置脏且空闲 → 该起一轮");
        assert!(!due_for_pass(&w, false), "起过就该清脏，第二拍不该再起（除非又来事件）");
    }

    #[test]
    fn running_suppresses_pass_even_when_dirty() {
        let w = WatchShared::new();
        mark_dirty(&w);
        assert!(!due_for_pass(&w, true), "running 时不起新轮");
        // running 期间的脏位不能被吞：轮结束后仍要靠它追平一次。
        // 变异防护 —— 若 due_for_pass 把 swap 放到了 running 判定之前，上一条断言就已经
        // 把脏位消费掉，这条 will 拿到 false 而当场变红。
        assert!(due_for_pass(&w, false), "running 期间的脏位不能被吞，空闲后仍要能追平");
    }

    #[test]
    fn list_watchable_roots_only_returns_live_root_dirs() {
        // 用内存库造行：证伪 SQL 的两个 WHERE（只回 kind='root'、只回未软删项目）。
        let conn = crate::db::open_in_memory().unwrap();
        // 活项目：1 个 root（要回）+ 1 个 entry（不该回）。
        conn.execute("INSERT INTO projects(id,name) VALUES('a','A')", []).unwrap();
        conn.execute(
            "INSERT INTO project_dirs(id,project_id,kind,path) VALUES('d1','a','root','/live/root')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_dirs(id,project_id,kind,path) VALUES('d2','a','entry','/live/entry')",
            [],
        )
        .unwrap();
        // 软删项目：它的 root 不该回。
        conn.execute(
            "INSERT INTO projects(id,name,deleted_at) VALUES('b','B',datetime('now'))",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_dirs(id,project_id,kind,path) VALUES('d3','b','root','/dead/root')",
            [],
        )
        .unwrap();
        let mut roots = list_watchable_roots(&conn).unwrap();
        roots.sort();
        assert_eq!(
            roots,
            vec!["/live/root".to_string()],
            "只回未软删项目的 kind='root'；entry 与软删项目的 root 都不该进来"
        );
    }
}
