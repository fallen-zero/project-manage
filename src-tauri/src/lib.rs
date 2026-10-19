mod backup;
mod daily_report;
mod db;
mod doc_preview;
mod error;
mod expiry;
mod extract;
mod index_job;
mod index_scan;
mod index_store;
mod ledger;
mod legacy_office;
mod milestone;
mod ocr;
mod project;
mod scratch;
mod search;
mod tokenize;
mod todo;
mod vault;
mod watch;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use tauri::{Manager, State};

use crate::daily_report::{DailyReport, DailyReportEdit, DailyReportInput, ExportRow, ImportOutcome, ImportPreview};
use crate::error::{AppError, AppResult};
use crate::expiry::{Expiry, ExpiryInput, ExpiryUpdate};
use crate::ledger::{
    CredentialInput, EnvInput, Ledger, LinkInput, NoteInput, ServerInput,
};
use crate::milestone::{Milestone, MilestoneEdit, MilestoneInput};
use crate::project::{DirInput, Project, ProjectInput};
use crate::todo::{Todo, TodoEdit, TodoInput};
use crate::vault::MasterKey;

struct AppState {
    conn: Mutex<rusqlite::Connection>,
    data_dir: PathBuf,
    /// 解锁后的主密钥只活在进程内存里：锁屏/退出即丢，重新解锁靠主密码或恢复码。
    mk: Mutex<Option<MasterKey>>,
    /// 索引作业跨线程共享：running/cancel 是原子量，summary 是上一轮的结论。
    index: Arc<index_job::IndexShared>,
    /// 预览的抽盘单飞闸门（终审 I-5）：占用中第二条预览直接拒，不再占 tokio worker（spec D9）。
    preview: Arc<doc_preview::PreviewShared>,
}

fn db(state: &AppState) -> AppResult<MutexGuard<'_, rusqlite::Connection>> {
    state.conn.lock().map_err(|_| {
        AppError::new(
            "db_poisoned",
            "数据库连接锁被此前的 panic 污染",
            Some("重启应用；若反复出现请把现象反馈给维护者"),
        )
    })
}

/// 台账的敏感列要解密才能看，所以走这条的命令必须先解锁。
/// 主密钥放在 `Option` 里而不是每次现推：Argon2id 派生是百毫秒级的事，
/// 且解锁动作应当由用户显式发起，不该在读取时偷偷弹窗。
fn unlocked_mk(state: &AppState) -> AppResult<MutexGuard<'_, Option<MasterKey>>> {
    let guard = state.mk.lock().map_err(|_| {
        AppError::new(
            "vault_poisoned",
            "主密钥锁被此前的 panic 污染",
            Some("重启应用后重新解锁"),
        )
    })?;
    if guard.is_none() {
        return Err(AppError::new(
            "vault_locked",
            "主密码库尚未解锁",
            Some("先在「安全」里输入主密码解锁，再查看账号与密码"),
        ));
    }
    Ok(guard)
}

#[derive(Serialize)]
struct DbStatus {
    data_dir: String,
    db_file: String,
    journal_mode: String,
    schema_version: i64,
    fts5_available: bool,
}

/// 探测本构建有没有编进 FTS5。从 `db_status` 里抽出来复用：
/// `index_overview` 也要在同一个口径上回答「这台机器能不能建全文索引」。
fn fts5_available(conn: &rusqlite::Connection) -> bool {
    conn.query_row(
        "SELECT 1 FROM pragma_compile_options WHERE compile_options LIKE '%FTS5%'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .is_ok()
}

/// 供前端展示的运行态，也是 IPC 往返的最小验证。
#[tauri::command]
fn db_status(state: State<'_, AppState>) -> AppResult<DbStatus> {
    let conn = db(&state)?;
    let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
    let schema_version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_meta",
        [],
        |r| r.get(0),
    )?;
    // FTS5 是全文检索的地基，这里显式探测而不是假设 bundled 构建一定带它
    let has_fts = fts5_available(&conn);

    Ok(DbStatus {
        data_dir: state.data_dir.display().to_string(),
        db_file: db::db_path(&state.data_dir).display().to_string(),
        journal_mode,
        schema_version,
        fts5_available: has_fts,
    })
}

#[tauri::command]
fn project_list(state: State<'_, AppState>) -> AppResult<Vec<Project>> {
    let conn = db(&state)?;
    project::list_projects(&conn)
}

#[tauri::command]
fn project_get(state: State<'_, AppState>, id: String) -> AppResult<Project> {
    let conn = db(&state)?;
    project::get_project(&conn, &id)
}

#[tauri::command]
fn project_create(state: State<'_, AppState>, input: ProjectInput) -> AppResult<Project> {
    let conn = db(&state)?;
    project::create_project(&conn, &input)
}

#[tauri::command]
fn project_update(
    state: State<'_, AppState>,
    id: String,
    input: ProjectInput,
) -> AppResult<Project> {
    let conn = db(&state)?;
    project::update_project(&conn, &id, &input)
}

#[tauri::command]
fn project_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    project::delete_project(&conn, &id)
}

#[tauri::command]
fn project_set_root(
    state: State<'_, AppState>,
    id: String,
    input: DirInput,
) -> AppResult<Project> {
    let conn = db(&state)?;
    project::set_root_dir(&conn, &id, &input)?;
    project::get_project(&conn, &id)
}

#[tauri::command]
fn project_add_entry(
    state: State<'_, AppState>,
    id: String,
    input: DirInput,
) -> AppResult<Project> {
    let conn = db(&state)?;
    project::add_entry_dir(&conn, &id, &input)?;
    project::get_project(&conn, &id)
}

#[tauri::command]
fn project_remove_dir(state: State<'_, AppState>, id: String, dir_id: String) -> AppResult<()> {
    let conn = db(&state)?;
    project::remove_dir(&conn, &id, &dir_id)
}

#[tauri::command]
fn project_status_options() -> Vec<&'static str> {
    project::STATUSES.to_vec()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultStatus {
    initialized: bool,
    unlocked: bool,
}

#[tauri::command]
fn vault_status(state: State<'_, AppState>) -> AppResult<VaultStatus> {
    let conn = db(&state)?;
    Ok(VaultStatus {
        initialized: vault::is_initialized(&conn)?,
        unlocked: state.mk.lock().map(|g| g.is_some()).unwrap_or(false),
    })
}

/// 初始化主密钥库。恢复码在这里生成并**只返回这一次**，前端展示后不再持久化。
#[tauri::command]
fn vault_init(state: State<'_, AppState>, main_password: String) -> AppResult<String> {
    let conn = db(&state)?;
    let code = vault::generate_recovery_code()?;
    let mk = vault::initialize(&conn, &main_password, &code)?;
    if let Ok(mut g) = state.mk.lock() {
        *g = Some(mk);
    }
    Ok(code)
}

/// 解锁用的凭据可以是主密码，也可以是恢复码：两者都尝试，由 vault 判定。
#[tauri::command]
fn vault_unlock(state: State<'_, AppState>, secret: String) -> AppResult<()> {
    let conn = db(&state)?;
    let mk = vault::unlock(&conn, &secret)?;
    let Some(mut g) = state.mk.lock().ok() else {
        return Err(AppError::new(
            "vault_poisoned",
            "主密钥锁被此前的 panic 污染",
            Some("重启应用后重新解锁"),
        ));
    };
    *g = Some(mk);
    Ok(())
}

#[tauri::command]
fn vault_lock(state: State<'_, AppState>) -> AppResult<()> {
    let Some(mut g) = state.mk.lock().ok() else {
        return Ok(());
    };
    *g = None;
    Ok(())
}

#[tauri::command]
fn vault_change_password(
    state: State<'_, AppState>,
    old_password: String,
    new_password: String,
) -> AppResult<()> {
    let conn = db(&state)?;
    let guard = unlocked_mk(&state)?;
    let mk = guard.as_ref().expect("unlocked_mk 已确认是 Some");
    vault::change_main_password(&conn, mk, &old_password, &new_password)
}

#[tauri::command]
fn vault_rotate_recovery_code(state: State<'_, AppState>) -> AppResult<String> {
    let conn = db(&state)?;
    let guard = unlocked_mk(&state)?;
    let mk = guard.as_ref().expect("unlocked_mk 已确认是 Some");
    vault::rotate_recovery_code(&conn, mk)
}

/// 台账列表不需要解锁：读接口本来就只回「这一列有没有值」，密文不出库。
#[tauri::command]
fn ledger_list(state: State<'_, AppState>, project_id: String) -> AppResult<Ledger> {
    let conn = db(&state)?;
    ledger::list(&conn, &project_id)
}

/// 库内字段检索（项目 + 台账的明文列）。不需要解锁：SQL 里压根没有密文列。
#[tauri::command]
fn search_local(state: State<'_, AppState>, query: String) -> AppResult<Vec<search::FieldHit>> {
    let conn = db(&state)?;
    search::field_hits(&conn, &query)
}

/// 首屏统一检索：一次 IPC 带回三段与三个截断计数。这一格只持锁查库，不碰磁盘。
#[tauri::command]
fn search_all(state: State<'_, AppState>, query: String) -> AppResult<search::SearchBundle> {
    let conn = db(&state)?;
    search::unified_bundle(&conn, &query)
}

#[tauri::command]
fn ledger_reveal(
    state: State<'_, AppState>,
    kind: String,
    id: String,
    field: String,
) -> AppResult<String> {
    let conn = db(&state)?;
    let guard = unlocked_mk(&state)?;
    let mk = guard.as_ref().expect("unlocked_mk 已确认是 Some");
    ledger::reveal(&conn, mk, &kind, &id, &field)
}

#[tauri::command]
fn ledger_env_create(
    state: State<'_, AppState>,
    project_id: String,
    input: EnvInput,
) -> AppResult<ledger::Env> {
    let conn = db(&state)?;
    ledger::create_env(&conn, &project_id, &input)
}

#[tauri::command]
fn ledger_env_update(state: State<'_, AppState>, id: String, input: EnvInput) -> AppResult<()> {
    let conn = db(&state)?;
    ledger::update_env(&conn, &id, &input)
}

#[tauri::command]
fn ledger_env_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    ledger::delete_env(&conn, &id)
}

#[tauri::command]
fn ledger_credential_create(
    state: State<'_, AppState>,
    project_id: String,
    input: CredentialInput,
) -> AppResult<ledger::Credential> {
    let conn = db(&state)?;
    let guard = unlocked_mk(&state)?;
    let mk = guard.as_ref().expect("unlocked_mk 已确认是 Some");
    ledger::create_credential(&conn, mk, &project_id, &input)
}

#[tauri::command]
fn ledger_credential_update(
    state: State<'_, AppState>,
    id: String,
    input: CredentialInput,
) -> AppResult<()> {
    let conn = db(&state)?;
    let guard = unlocked_mk(&state)?;
    let mk = guard.as_ref().expect("unlocked_mk 已确认是 Some");
    ledger::update_credential(&conn, mk, &id, &input)
}

#[tauri::command]
fn ledger_credential_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    ledger::delete_credential(&conn, &id)
}

#[tauri::command]
fn ledger_server_create(
    state: State<'_, AppState>,
    project_id: String,
    input: ServerInput,
) -> AppResult<ledger::Server> {
    let conn = db(&state)?;
    let guard = unlocked_mk(&state)?;
    let mk = guard.as_ref().expect("unlocked_mk 已确认是 Some");
    ledger::create_server(&conn, mk, &project_id, &input)
}

#[tauri::command]
fn ledger_server_update(
    state: State<'_, AppState>,
    id: String,
    input: ServerInput,
) -> AppResult<()> {
    let conn = db(&state)?;
    let guard = unlocked_mk(&state)?;
    let mk = guard.as_ref().expect("unlocked_mk 已确认是 Some");
    ledger::update_server(&conn, mk, &id, &input)
}

#[tauri::command]
fn ledger_server_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    ledger::delete_server(&conn, &id)
}

#[tauri::command]
fn ledger_link_create(
    state: State<'_, AppState>,
    project_id: String,
    input: LinkInput,
) -> AppResult<ledger::Link> {
    let conn = db(&state)?;
    ledger::create_link(&conn, &project_id, &input)
}

#[tauri::command]
fn ledger_link_update(state: State<'_, AppState>, id: String, input: LinkInput) -> AppResult<()> {
    let conn = db(&state)?;
    ledger::update_link(&conn, &id, &input)
}

#[tauri::command]
fn ledger_link_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    ledger::delete_link(&conn, &id)
}

#[tauri::command]
fn ledger_note_create(
    state: State<'_, AppState>,
    project_id: String,
    input: NoteInput,
) -> AppResult<ledger::Note> {
    let conn = db(&state)?;
    ledger::create_note(&conn, &project_id, &input)
}

#[tauri::command]
fn ledger_note_update(state: State<'_, AppState>, id: String, input: NoteInput) -> AppResult<()> {
    let conn = db(&state)?;
    ledger::update_note(&conn, &id, &input)
}

#[tauri::command]
fn ledger_note_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    ledger::delete_note(&conn, &id)
}

/// 起一轮索引。线程壳与 `app.emit` 的连通性只能由真机验证（Task 12），
/// 这里只保证两个错误码能被界面念出来：`index_running` / `index_spawn_failed`。
#[tauri::command]
fn index_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: Option<String>,
    rebuild: bool,
) -> AppResult<()> {
    index_job::start(app, state.data_dir.clone(), state.index.clone(), project_id, rebuild)
}

/// 只置 cancel 标志，不做复位：`start` 开头有 `cancel.store(false)`，
/// 所以「空闲时点了取消」不会把下一轮也取消掉。
#[tauri::command]
fn index_cancel(state: State<'_, AppState>) -> AppResult<()> {
    state.index.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectState {
    project_id: String,
    project_name: String,
    root_path: String,
    root_exists: bool,
    ok: i64,
    skipped: i64,
    failed: i64,
    pending: i64,
    missing: i64,
    total: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IndexOverview {
    running: bool,
    fts5_available: bool,
    last_run: Option<index_job::RunSummary>,
    /// 一轮彻底失败的原因串（`[code] message（hint）`）。Task 9 加 `IndexShared::last_error`
    /// 就是为了这一刻 —— 没有这个字段，`report_failure` 写的缘由永远读不出来，
    /// 界面上「按钮说失败了」和「这一轮为什么失败」就接不上。成功轮由 `start` 置回 None，
    /// 所以这里不需要自己判断陈旧性。
    last_error: Option<String>,
    projects: Vec<ProjectState>,
    exclude_dirs: Vec<String>,
    max_file_bytes: i64,
    max_files_per_project: i64,
    supported_exts: Vec<&'static str>,
    /// 本机 Windows OCR 是否可用（`ocr::ocr_available()` 的活探测值）。
    /// 不透出 `ScanOptions::load` 的 `opts.ocr_available`——那是 load 的惰性默认 `true`，
    /// 在这条路径上从不喂给 `scan_root`，set-but-never-read；UI 必须看到真实探测。
    ocr_available: bool,
    /// 本机有没有 LibreOffice 可用（`legacy_office::soffice_available()` 的**活探测**值）。
    /// 与 `ocr_available` 同一措辞理由：不透出 `ScanOptions::load` 的惰性默认 —— 那两条消费路径
    /// 从不喂给 `scan_root`，set-but-never-read；UI 必须看到真实探测，否则「装完不用重启」这句承诺会假。
    soffice_available: bool,
}

#[tauri::command]
fn index_overview(state: State<'_, AppState>) -> AppResult<IndexOverview> {
    // 持锁段：只做库内的读。文件系统探测一条都不放进来 ——
    // `AppState.conn` 是全 crate 唯一那把 `Mutex<Connection>`，`db_status` / `project_list` /
    // `ledger_list` / `index_docs` / `search_docs` 全排在它后面，**排队的时段只是下面那个取锁块
    // （`let conn = db(&state)?` 到它所属作用域结束）**，出锁后锁就空出来了；而未挂载的 SMB/网络盘上的
    // `Path::is_dir()` 在 Windows 上能阻塞几十秒。持着它去 stat N 个根目录 = 一次界面刷新
    // 卡住整条 IPC（Task 10 评审的 Important 1，代价实测过口径，不是推测）。
    let (opts, targets, counts, has_fts) = {
        let conn = db(&state)?;
        let opts = index_scan::ScanOptions::load(&conn)?;
        let targets = index_job::targets(&conn, None)?;
        // 与 targets 同序收集，出锁后用 zip 配对，不另存一个 project_id 做查表。
        let mut counts = Vec::with_capacity(targets.len());
        for t in &targets {
            counts.push(index_store::status_counts(&conn, &t.project_id)?);
        }
        (opts, targets, counts, fts5_available(&conn))
    }; // conn 守卫到此离开作用域被 drop，下面再碰文件系统已经不持锁

    // 出锁探测段（上面那个取锁块结束后到 `collect()` 之前）：`is_dir()` 可以慢，锁也确实空出来了，
    // 但命令本身仍是 sync，这段时间消息泵还被它占着 —— **连那些排在上面那块锁后面、此刻已经拿得到锁的
    // 别的 IPC 也一样进不来**（两条时段各有各的堵法，别把上面那句「全排在它后面」读到这里，
    // 也别把这句读成持锁段；Task 6 复审 Important-1 核到的口径）。
    let projects = targets
        .into_iter()
        .zip(counts)
        .map(|(t, counts)| {
            let pick = |s: &str| counts.iter().find(|c| c.status == s).map(|c| c.count).unwrap_or(0);
            ProjectState {
                total: counts.iter().map(|c| c.count).sum(),
                project_id: t.project_id,
                project_name: t.project_name,
                root_exists: std::path::Path::new(&t.root_path).is_dir(),
                root_path: t.root_path,
                ok: pick("ok"),
                skipped: pick("skipped"),
                failed: pick("failed"),
                pending: pick("pending"),
                missing: pick("missing"),
            }
        })
        .collect();

    Ok(IndexOverview {
        running: state.index.running.load(std::sync::atomic::Ordering::SeqCst),
        fts5_available: has_fts,
        // 上一轮结论只是展示用，拿不到锁就回 None，不因为 UI 刷新阻塞作业线程。
        // summary/last_error 是 IndexShared 自己的两把 Mutex。作业线程自持一条 `db::open` 的连接、
        // 从不反向拿 AppState 的 conn，所以这里既不构成环路，也不再持着 conn 守卫。
        last_run: state.index.summary.lock().ok().and_then(|g| g.clone()),
        last_error: state.index.last_error.lock().ok().and_then(|g| g.clone()),
        projects,
        exclude_dirs: opts.exclude_dirs,
        max_file_bytes: opts.max_file_bytes as i64,
        max_files_per_project: opts.max_files_per_project,
        supported_exts: extract::supported_exts().to_vec(),
        ocr_available: crate::ocr::ocr_available(),
        soffice_available: crate::legacy_office::soffice_available(),
    })
}

/// 某一项目的文件清单（分页）。`limit` 的 clamp 是本路径唯一的边界校验：`index_store::list_docs`
/// （只有一条 prepared statement，不做二次校验）拿到负数时，SQLite 的语义是「不限行」，
/// 于是一次拉出整个项目（上限 50000 行）进 IPC 载荷。别把 clamp 挪进 `index_store.rs`：
/// `doc_hits` 的既有测试正靠 `limit=0`/`limit=-1` 钉住 SQLite 的原语义
/// （`index_store` 测试 `query_guards_and_limit_pass_straight_through` 里那两条断言：「负数 limit
/// 原样交给 SQL（SQLite：不限行）」与「0 就是 0 行，不许悄悄变成默认值」），
/// clamp 一旦下沉，那两条断言当场失去对象。
#[tauri::command]
fn index_docs(
    state: State<'_, AppState>,
    project_id: String,
    status: Option<String>,
    limit: i64,
    offset: i64,
) -> AppResult<Vec<index_store::DocRow>> {
    let conn = db(&state)?;
    index_store::list_docs(
        &conn,
        &project_id,
        status.as_deref().filter(|s| !s.is_empty()),
        limit.clamp(1, 500),
        offset.max(0),
    )
}

/// 正文检索。密文列不参与：SQL 全在 `index_store::doc_hits` 里，这里不拼任何语句。
#[tauri::command]
fn search_docs(
    state: State<'_, AppState>,
    query: String,
    limit: Option<i64>,
) -> AppResult<Vec<index_store::DocHit>> {
    let conn = db(&state)?;
    index_store::doc_hits(&conn, &query, limit.unwrap_or(50).clamp(1, 200))
}

/// 原文预览：**锁内查库、锁外抽盘**，所以必须分两步。
/// 握着 `state.conn` 去读盘会让别的命令等一次慢 IO（网络盘上一个大文件就是秒级卡顿，而首屏正是
/// 最容易连点的地方），所以这里刻意不写成 `let conn = db(&state)?; preview(&conn, ..)`。
/// 光出锁还不够：`async` 那一行把命令体挪出 IPC 所在线程，见下面那段「为什么必须有 async」。
#[tauri::command(async)]
fn doc_preview(
    state: State<'_, AppState>,
    doc_id: String,
    query: String,
) -> AppResult<doc_preview::DocPreview> {
    // 单飞闸门（I-5）：_guard 必须活到函数末尾，所以绑下划线**带名字**（`let _ =` 会当场释放）。
    let _guard = doc_preview::PreviewShared::try_enter(&state.preview).ok_or_else(doc_preview::busy)?;
    let (row, max_file_bytes) = {
        let conn = db(&state)?;
        (
            doc_preview::row_for_preview(&conn, &doc_id)?,
            index_scan::ScanOptions::load(&conn)?.max_file_bytes,
        )
    };
    doc_preview::render_preview(&doc_id, &row, &query, max_file_bytes)
}

/// 一键安装 LibreOffice（M7 D8）。**零参数**：包 ID 与全部 argv 都硬编码在 `legacy_office`。
/// 返回 `startedViaWinget=false` 表示本机没有 winget，前端要退到「打开官网下载页」。
/// 这是全仓第 42 条命令，也是唯一一条会启动外部安装器的命令 —— spec §四 单列了理由与两条守护测试。
#[tauri::command]
fn libreoffice_install() -> AppResult<legacy_office::InstallOutcome> {
    legacy_office::start_install()
}

/// 索引轮在跑就不给导（两个写者争同一库的后果见 `backup_import_refuses_while_a_pass_is_running` 的注释：
/// `restore` 灌页时索引 worker 正持着**第二条连接**写 `index_docs`，会出现「灌到一半被写回」，比报错难查）。
/// 抽成独立可测函数：命令层用它，测试直接喂 `IndexShared` 置位后的旗就能验，不必起真机。
fn not_indexing(shared: &index_job::IndexShared) -> AppResult<()> {
    if shared.running.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(AppError::new(
            "backup_busy",
            "索引轮正在进行，此刻导入会两个写者争同一个库",
            Some("先等这一轮结束，或在索引页点「取消」，再导入"),
        ));
    }
    Ok(())
}

/// 导出备份包（M8 D1–D4）。**用现开的第二条连接**（D12）：真正会占住 `state.conn`、让界面上每个读写
/// 命令在那把 std Mutex 上无声排队（不报错、纯等）的是 **import** 那一长串（读包 + 全库哈希 + 应急包导出 +
/// restore），不是 `VACUUM` —— `VACUUM` 只在一次性快照连接上跑（见 `backup::export`），从不碰调用方连接。
/// 这里开第二条连接的用意是：整条导出（快照 → 剥索引 → VACUUM → 读字节 → 打 zip 文件 I/O）压根不占用户锁。
/// 仓库本就有第二条连接的生产先例（`index_job.rs:468` worker、`watch.rs:70` watcher）。只读快照 ⇒ 与索引轮并发无害。
#[tauri::command(async)]
fn backup_export(state: State<'_, AppState>, path: String) -> AppResult<backup::BackupSummary> {
    // 落点守卫必须早于任何文件写：`zip_out` 的 `File::create` 会 CREATE_ALWAYS 截断，把落点指进
    // 数据目录（活库/应急包所在）等于毁掉用户唯一的持久库；判据与理由见 `backup::guard_destination`。
    backup::guard_destination(&state.data_dir, std::path::Path::new(&path))?;
    let conn = db::open(&state.data_dir)?;
    backup::export(&conn, std::path::Path::new(&path))
}

/// 覆盖前「看得见包里是什么」（D5 花第三条命令买的这一格）。不碰任何连接、不落任何盘。
#[tauri::command(async)]
fn backup_inspect(path: String) -> AppResult<backup::PackageInfo> {
    backup::inspect(std::path::Path::new(&path))
}

/// 判定导入失败后要不要清 `AppState.mk`：只有**库已被覆写之后**才可能报的三支码才清（此时 `mk` 里
/// 那把旧密钥对包里的 `vault_meta` 已无意义，甚至可能出自换主密码之前）。`backup_restore_failed` 一并
/// 算上 —— 控制方裁定：`restore` 中断大概率被 SQLite 自己的 `backup_finish` 回滚，但本机造不出这一支来
/// 证实，而重锁是安全方向（三支 hint 都已叫用户重导应急包）。其余码都在动库**之前**就失败（闸门、验包、
/// 写应急包），清 mk 只是白白锁一遍用户，故一律 false。抽成纯函数，好把「判真/判假」两个方向都单测到位。
fn should_clear_mk_after_import(code: &str) -> bool {
    matches!(code, "backup_migrate_failed" | "backup_wal_lost" | "backup_restore_failed")
}

/// 导入 = 整库覆写当前数据。D13 顺序（验包四道 → 应急包 → restore → migrate → WAL 现读）里，
/// **结构**保证的是「应急包早于覆写」：应急包与 restore 同处 `backup::import` 内、且 restore 只在
/// 四道验包与应急包写出都成功后才发起 —— 前三道任一失败库一字未动、也不落应急包（见 backup.rs import 注）。
///
/// `not_indexing` 这道闸门则**只是大幅收窄、不是互斥**：它在 `db(&state)?` 取写锁**之前**就判过了，
/// 且 `index_job::start` 从不碰 `state.conn`（worker 在 `index_job.rs:468` 自开一条连接），所以把闸门
/// 挪进锁里也关不掉这个窗口（spec D13 / HANDOFF 坑 7 的既有裁定）。后果有界：竞态只可能把索引行重新
/// 写回，台账/`vault_meta`/`settings` 三张表 worker 从不写，下一轮索引自愈。
///
/// 收尾的 `AppState.mk`：成功路径无条件清；失败路径按 `should_clear_mk_after_import` 只清覆写之后的那几支。
/// 包里那份 `vault_meta` 的包裹可能出自换主密码之前，用旧密钥解新库只会当场报错 —— 让它明着要一次解锁，
/// 比悄悄错好（spec §三 密钥侧）。本机无独占断言可钉「界面回未解锁态」（真机 §九.3 才看得到），故不为它编假断言。
#[tauri::command(async)]
fn backup_import(state: State<'_, AppState>, path: String) -> AppResult<backup::ImportOutcome> {
    not_indexing(&state.index)?;
    let mut conn = db(&state)?;
    match backup::import(&mut conn, std::path::Path::new(&path), &state.data_dir) {
        Ok(out) => {
            drop(conn);
            if let Ok(mut g) = state.mk.lock() {
                *g = None;
            }
            Ok(out)
        }
        Err(e) => {
            // 库已被覆写后才失败的臂（migrate/wal_lost/restore_failed）：mk 里那把旧钥对新库已无意义，
            // 不清就是让界面继续报 unlocked:true（vault.rs:357-361 那句 field_decrypt_failed 会替它喊冤）。
            // 其余臂库根本未动，原样返回、绝不清（白锁用户）。
            if should_clear_mk_after_import(e.code) {
                if let Ok(mut g) = state.mk.lock() {
                    *g = None;
                }
            }
            Err(e)
        }
    }
}

/// 待办四条命令（M9 Task 2，形制照 `ledger_env_create`）：全部同步、函数体不写 await 点。
/// **本注释原先写的是「给它们加 async 标记会直接编不过」——那是句假话，M10 更正**：
/// 真取 `db(&state)?` 的 `MutexGuard` 又挂着 async 属性的是 `backup_import` 与 `doc_preview` 这两条，
/// 它们今天就编得过（`backup_export` 开的是另一条连接、`backup_inspect` 根本不碰库，不能当证据）。
/// 真编不过的是 `async fn` 里让 guard 横跨 await 点；只加属性、函数体不 await，
/// 得到的正是 M10 Task 6/7 那三条导出/导入命令用的形态（spec §五.5）。
/// 勾选完成走 `todo_update` 的 `done` 字段（D12：不单开一条命令）。
/// `project_id` 的 JS 侧要显式传 `null`（缺键与 null 的行为别赌）。
#[tauri::command]
fn todo_list(state: State<'_, AppState>, project_id: Option<String>) -> AppResult<Vec<todo::Todo>> {
    let conn = db(&state)?;
    todo::list(&conn, project_id.as_deref())
}

#[tauri::command]
fn todo_create(state: State<'_, AppState>, input: TodoInput) -> AppResult<Todo> {
    let conn = db(&state)?;
    todo::create(&conn, &input)
}

#[tauri::command]
fn todo_update(state: State<'_, AppState>, id: String, input: TodoEdit) -> AppResult<()> {
    let conn = db(&state)?;
    todo::update(&conn, &id, &input)
}

#[tauri::command]
fn todo_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    todo::delete(&conn, &id)
}

/// 里程碑四条命令（M9 Task 3，形制照上面 `todo_*` 那四条）：全部同步、函数体不写 await 点。
/// 这里原先也写着「加 async 标记会直接编不过」，与上面 `todo_*` 那处是**同一句假话**（M10 一起更正）：
/// 挂着 async 属性又取 `MutexGuard` 的 `backup_import`/`doc_preview` 今天就编得过；编不过的是让 guard
/// 横跨 await 点的 `async fn`（M8 spec D11 说的其实是这件事）。
/// 勾选完成走 `milestone_update` 的 `done` 字段（D12：不单开命令）；`MilestoneEdit` 不含
/// `project_id`，改归属不在本轮（spec §十一）。`project_id` 的 JS 侧要显式传 `null`（缺键与 null 的行为别赌）。
#[tauri::command]
fn milestone_list(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> AppResult<Vec<milestone::Milestone>> {
    let conn = db(&state)?;
    milestone::list(&conn, project_id.as_deref())
}

#[tauri::command]
fn milestone_create(state: State<'_, AppState>, input: MilestoneInput) -> AppResult<Milestone> {
    let conn = db(&state)?;
    milestone::create(&conn, &input)
}

#[tauri::command]
fn milestone_update(state: State<'_, AppState>, id: String, input: MilestoneEdit) -> AppResult<()> {
    let conn = db(&state)?;
    milestone::update(&conn, &id, &input)
}

#[tauri::command]
fn milestone_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    milestone::delete(&conn, &id)
}

/// 到期事项四条命令（M11-a Task 2，形制照 `milestone_*`）：同步、函数体不写 await 点。
/// `db(&state)?` 取回的 `MutexGuard` 不是 `Send`，在 async fn 里让它横跨 await 点才编不过（M8 D11）；
/// 本批四条不碰文件，所以 async 标记那扇门**保持 7 条不动**（spec §六）。
/// 勾选态那一列本表没有（D2：续期 = 改日期），所以这里没有 `done` 那一条形状。
#[tauri::command]
fn expiry_list(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> AppResult<Vec<Expiry>> {
    let conn = db(&state)?;
    expiry::list(&conn, project_id.as_deref())
}

#[tauri::command]
fn expiry_create(state: State<'_, AppState>, input: ExpiryInput) -> AppResult<Expiry> {
    let conn = db(&state)?;
    expiry::create(&conn, &input)
}

#[tauri::command]
fn expiry_update(state: State<'_, AppState>, id: String, input: ExpiryUpdate) -> AppResult<()> {
    let conn = db(&state)?;
    expiry::update(&conn, &id, &input)
}

#[tauri::command]
fn expiry_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    expiry::delete(&conn, &id)
}

/// 日报四条命令（M10 Task 2，形制照 `todo_*`）：同步、函数体不写 await 点。
/// `db(&state)?` 取回的 `MutexGuard` 不是 `Send`，**在 async fn 里让它横跨 await 点**才编不过（M8 D11）。
/// 只挂 async 属性、函数体照常同步是合法的 —— `backup_import` 与 `doc_preview` 就是这个形态，
/// M10 Task 6/7 的导出与导入三条也走这一形（spec §五.5）。
#[tauri::command]
fn daily_report_list(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> AppResult<Vec<DailyReport>> {
    let conn = db(&state)?;
    daily_report::list(&conn, project_id.as_deref())
}

#[tauri::command]
fn daily_report_create(state: State<'_, AppState>, input: DailyReportInput) -> AppResult<DailyReport> {
    let conn = db(&state)?;
    daily_report::create(&conn, &input)
}

#[tauri::command]
fn daily_report_update(state: State<'_, AppState>, id: String, input: DailyReportEdit) -> AppResult<()> {
    let conn = db(&state)?;
    daily_report::update(&conn, &id, &input)
}

#[tauri::command]
fn daily_report_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    daily_report::delete(&conn, &id)
}

/// 日报导出（M10 Task 6，spec §六.1）：挂 async 属性、函数体不写 await 点 —— 写整个 xlsx 是文件 IO，
/// 不该压在消息泵那一个线程上（与备份那三条同一个理由，见本文件上方那条备份命令）。
/// **不取 `db(&state)?`**：要导出的行由前端传回来（判据只有一处），所以这条与索引 job、
/// 与其它读写完全并行，不占那把全局 Mutex。
#[tauri::command(async)]
fn daily_report_export(
    state: State<'_, AppState>,
    path: String,
    rows: Vec<ExportRow>,
) -> AppResult<usize> {
    // 落点守卫必须早于任何文件写：`File::create` 是 CREATE_ALWAYS 截断，把落点指进数据目录
    // 会覆掉活库或那次导入的撤销键（`backup_export` 里同一条注释）。
    backup::guard_destination(&state.data_dir, std::path::Path::new(&path))?;
    daily_report::write_xlsx(std::path::Path::new(&path), &rows)
}

/// 导入预检（M10 Task 7，spec §六.2）：**只读、不写库**（D5），界面先看清「几行能进、哪几行为什么不进」。
/// 文件解析与项目匹配/查重在同一次调用里做完 ⇒ 这一格**全程持库锁**（`preview` 收 `&Connection` 与路径，
/// 读表在锁内）。`MAX_IMPORT_ROWS` 就是给这一格的等待时间兜底的；async 属性只保证不压在消息泵那一个线程上。
#[tauri::command(async)]
fn daily_report_import_preview(state: State<'_, AppState>, path: String) -> AppResult<ImportPreview> {
    let p = std::path::Path::new(&path);
    let conn = db(&state)?;
    daily_report::preview(&conn, p)
}

/// 导入落库（M10 Task 7）：**重新解析同一个路径**（不缓存预检结果、两步之间不锁文件，那是 D5 当次接受的
/// 代价），单事务全成全败。命令体同样不写 await 点；`db(&state)?` 取回的 `MutexGuard` 也不是 `Send`，
/// 这正是「挂 async 属性但函数体保持同步」的形态（与 `backup_export`、`daily_report_export` 同形，
/// 理由见本文件上方 `daily_report_list` 那条注释）。
#[tauri::command(async)]
fn daily_report_import_apply(state: State<'_, AppState>, path: String) -> AppResult<ImportOutcome> {
    let p = std::path::Path::new(&path);
    let mut conn = db(&state)?;
    daily_report::apply(&mut conn, p)
}

/// 建库并在**指定目录**打开（M5.5 D8）。把「开库 + WAL 读回」从 setup 里拆出来，是为了能喂 tempdir
/// 单测它——`index_job::start` 要 AppHandle、本批不开 tauri::test，所以可测切分只能到这一步；
/// 取默认数据目录的路径与起轮仍留在 setup。`db::open` 内部已做 `create_dir_all` 与 WAL 读回断言
/// （`db.rs:16` 建目录、`db::after_open` 里 M0 那条 WAL 读回硬门），所以本函数只是薄转发——
/// 价值全在「data_dir 是参数、不是 app.path()」这条可注入的边界上。
fn bootstrap_data(data_dir: &std::path::Path) -> AppResult<rusqlite::Connection> {
    db::open(data_dir)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let conn = bootstrap_data(&data_dir)?;
            let index = Arc::new(index_job::IndexShared::new());
            let watch = Arc::new(watch::WatchShared::new());
            app.manage(AppState {
                conn: Mutex::new(conn),
                data_dir: data_dir.clone(),
                mk: Mutex::new(None),
                index: Arc::clone(&index),
                preview: Arc::new(doc_preview::PreviewShared::new()),
            });
            // M5.5 D4：把文件监听接进运行态 —— watcher 只置脏，ticker 每 5 s 在空闲时起一轮追平。
            // 都必须在 `manage` 之后（ticker 要 app.handle() 且要 index/watch 已就绪），在启动轮之前
            // 之后皆可（二者共用 index.running 这同一个闸门，先后不影响正确性）。
            // watcher 初始化失败（极少：库打不开 / 系统起不了线程）不阻断启动，界面仍有手动索引兜底。
            let _ = watch::spawn_watcher(&data_dir, Arc::clone(&watch));
            watch::spawn_ticker(
                app.handle().clone(),
                data_dir.clone(),
                Arc::clone(&watch),
                Arc::clone(&index),
            );
            // M5 D1：启动即追平一轮。rebuild=false 走既有增量判据（size + mtime + ok 相同 ⇒ 连文件都
            // 不打开），所以常态代价是一次目录遍历 + stat，不是重抽；root 不可达的项目连 walkdir 都不发起。
            // 与用户手动点撞车的主防线其实是两个 start 按钮的 `disabled={running}`（口径来自
            // `overview.running`）；既有那条 amber 提示（「这不是索引出错，只是上一轮还没结束」）
            // 只在「`overview` 还没到货、页面以为空闲」那一个挂载窗口里可达 —— 真机要按这个窗口造前提才采得到（Task 10 断言 11；2026-10-12 终审 Minor-2 把原来误写的「断言 3」改准，3 是「主承诺」那一格）。
            // 自动轮的失败不拦启动（应用打不开比没追平严重），但也不许悄悄消失：起不来时把原因写进
            // `last_error` —— 它存在的全部理由就是「三个失败出口都要留下一句话」（`index_job.rs` 头注释），
            // `index_overview` 已透出这一格、/index 已整段渲染它，所以零线格式改动（Task 8 评审的 I 级结论）。
            // 头几秒前端还没装 index://progress 监听，那段进度事件会丢；用户进 /index 看到的是
            // 徽标「索引进行中」+ 既有那句「本轮已开始，正在等第一条进度事件」的空态文案，M3 已实测过这一档
            // （评审另核到时序依据：tauri 2.12.0 `app.rs:2689-2699` 是先建窗口配置、再跑 setup 钩子）。
            if let Err(e) = index_job::start(app.handle().clone(), data_dir, Arc::clone(&index), None, false) {
                if let Ok(mut g) = index.last_error.lock() {
                    *g = Some(format!("{e}"));
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            db_status,
            project_list,
            project_get,
            project_create,
            project_update,
            project_delete,
            project_set_root,
            project_add_entry,
            project_remove_dir,
            project_status_options,
            vault_status,
            vault_init,
            vault_unlock,
            vault_lock,
            vault_change_password,
            vault_rotate_recovery_code,
            ledger_list,
            search_local,
            ledger_reveal,
            ledger_env_create,
            ledger_env_update,
            ledger_env_delete,
            ledger_credential_create,
            ledger_credential_update,
            ledger_credential_delete,
            ledger_server_create,
            ledger_server_update,
            ledger_server_delete,
            ledger_link_create,
            ledger_link_update,
            ledger_link_delete,
            ledger_note_create,
            ledger_note_update,
            ledger_note_delete,
            index_start,
            index_cancel,
            index_overview,
            index_docs,
            search_docs,
            search_all,
            doc_preview,
            libreoffice_install,
            backup_export,
            backup_inspect,
            backup_import,
            todo_list,
            todo_create,
            todo_update,
            todo_delete,
            milestone_list,
            milestone_create,
            milestone_update,
            milestone_delete,
            daily_report_list,
            daily_report_create,
            daily_report_update,
            daily_report_delete,
            daily_report_export,
            daily_report_import_preview,
            daily_report_import_apply,
            expiry_list,
            expiry_create,
            expiry_update,
            expiry_delete
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_data_opens_a_wal_db_at_the_given_dir() {
        let dir = tempfile::tempdir().unwrap();
        let conn = bootstrap_data(dir.path()).unwrap();
        // 传进去的目录被真用了（红线二的守护：绝不回落到 app.path() 默认目录）。
        assert!(dir.path().join("ledger.db").exists(), "库必须落在喂进来的 data_dir");
        let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        // 提 pub(crate) 是给 M8 的验包判据用的，这条顺带改成直接引用它 ——
        // 原来写字面 4 的那两句注释（「lib.rs 够不着」）已经不成立。
        assert_eq!(ver, crate::db::TARGET_VERSION, "= db.rs 的 TARGET_VERSION，链头已随 M9 V5 走到 5");
    }

    /// 第 42 条命令必须是**零参数**的（spec §四：这条命令能启动外部安装器，
    /// 任何入参都是任意命令执行的入口）。签名一变，这一行编不过。
    #[test]
    fn libreoffice_install_command_takes_no_arguments() {
        const _: fn() -> crate::error::AppResult<crate::legacy_office::InstallOutcome> = libreoffice_install;
    }

    /// D13 的闸门单独抽成可测函数：`restore` 灌整库的时候，索引 worker 正持着**第二条连接**
    /// （`index_job.rs:468`）写 `index_docs` —— 两个写者争同一库会出现「灌到一半被写回」，
    /// 那比报错难查得多。所以 running 为真时必须当场拒，且错误码固定。
    ///
    /// 置位方式（控制方裁定 3）：`IndexShared.running` 是 pub 原子量，测试里直接
    /// `shared.running.store(true, SeqCst)` 用的就是 `start` 里 swap 的同一把旗；**不**为此新增
    /// `IndexShared::hold_for_test` 之类的 pub 函数（那等于给生产 API 开一个没人用的入口）。
    #[test]
    fn backup_import_refuses_while_a_pass_is_running() {
        use std::sync::atomic::Ordering;
        let shared = index_job::IndexShared::new();
        assert!(not_indexing(&shared).is_ok(), "空闲时必须放行，否则上一条测试是恒红摆设");
        shared.running.store(true, Ordering::SeqCst);
        let e = not_indexing(&shared).expect_err("running 时必须红");
        assert_eq!(e.code, "backup_busy");
        // 简报原行 `e.hint.is_some_and(|h| ..)` 与同一断言消息里的 `e.hint` 撞 E0382（部分移动后复用），
        // 与 T5 同形制改成 match：判据不变，回显面也不含包内字段值（红线④）。
        match &e.hint {
            Some(h) => assert!(h.contains("取消"), "hint 要指路到取消按钮：{h}"),
            None => panic!("hint 要指路到取消按钮，实得 None"),
        }
    }

    /// 三条命令的入参形状：**只有一个 `path: String`**，第二个参数都不许有。
    /// M7 给 `libreoffice_install` 钉的是同一条守护（那格守的是「不给前端任意命令执行」），
    /// 本轮守的是同一件事的另一面：命令只写用户当场选中的那一个路径（红线⑤）。
    /// 给任何一条命令加参数（哪怕是「要不要剥索引」这种看着无害的开关）都会让这一行编不过。
    #[test]
    fn backup_commands_take_exactly_one_path_argument() {
        const _: fn(String) -> crate::error::AppResult<backup::PackageInfo> = backup_inspect;
        const _: for<'a> fn(
            tauri::State<'a, AppState>,
            String,
        ) -> crate::error::AppResult<backup::BackupSummary> = backup_export;
        const _: for<'a> fn(
            tauri::State<'a, AppState>,
            String,
        ) -> crate::error::AppResult<backup::ImportOutcome> = backup_import;
    }

    /// I1：`should_clear_mk_after_import` 的判真/判假两个方向都要钉死 —— 覆写之后的三支必须清、
    /// 动库之前的各支必须不清。用 `assert!`/`assert!(!)` 而非 `assert_eq!(.., true/false)`：
    /// 后者会撞 `clippy::bool_assert_comparison`（`-D warnings` 下打成红），前者同样是逐支精确判值。
    #[test]
    fn only_post_overwrite_import_failures_clear_the_master_key() {
        // 覆写之后才可能报的三支：mk 里旧钥对新库已无意义，必须清（否则界面继续报 unlocked:true）。
        for code in ["backup_migrate_failed", "backup_wal_lost", "backup_restore_failed"] {
            assert!(should_clear_mk_after_import(code), "{code} 在库已覆写之后，判 false 就漏清、界面撒谎");
        }
        // 动库之前就失败的臂：库一字未动，清 mk 只是白白锁一遍用户，必须不清。
        for code in [
            "backup_busy",
            "backup_schema_too_new",
            "backup_hash_mismatch",
            "backup_package_invalid",
            "fs_failed",
        ] {
            assert!(!should_clear_mk_after_import(code), "{code} 发生在覆写之前，判 true 就是无谓锁定用户");
        }
        // wrong-value 型钉法（让 matches! 那支真的能变红）：把 restore_failed 从三支里漏掉 → 第一条红；
        // 把 busy 误加进三支 → 第二条红。
        assert!(
            should_clear_mk_after_import("backup_restore_failed"),
            "restore_failed 若从 matches! 那支漏掉，本行必须红（裁定：重锁是安全方向）"
        );
        assert!(
            !should_clear_mk_after_import("backup_busy"),
            "busy 若在 matches! 里被误加，本行必须红（闸门失败时库根本未动）"
        );
    }

    /// M8.1 补钉：`backup_export` 里「守卫先于任何写盘」这条**接线**的落点。
    ///
    /// 为什么只能这么钉：判据本身有单测（`backup::guard_destination` 的落点各档 + 「既有文件不被覆写」），
    /// 但命令层的调用点在这套门禁里开不出真测试 —— 本仓 `tauri` 没开 `test` feature（「`Cargo.toml`
    /// 一行不动」是硬边界），而 mock 的空 identifier 会把 `app_data_dir()` 解析到**真实用户目录**、
    /// 撞红线②（M5 终审把这条机制读过，见 HANDOFF 坑 101）。所以照本仓现成的**文本门**形制钉它
    /// （先例：`tokenize.rs` 的 `query_side_has_exactly_one_cut`、`extract.rs` 的
    /// `release_profile_does_not_abort_so_panic_guards_work`）：谁把守卫调用删掉、或把它挪到
    /// `backup::export` 之后，本条就红。
    ///
    /// needle 一律 `concat!` 拼 —— 写成字面量会把本测试自己数进去（`tokenize.rs` 那条同款坑）。
    #[test]
    fn backup_export_wires_the_guard_before_any_write() {
        let src = include_str!("lib.rs");
        let start = src
            .find(concat!("fn backup_", "export("))
            .expect("backup_export 必须还在 lib.rs 里");
        let body = &src[start..start + src[start..].find("\n}").expect("backup_export 的函数体收尾")];
        let guard = concat!("backup::guard_", "destination(");
        let write = concat!("backup::exp", "ort(");
        assert_eq!(
            body.matches(guard).count(),
            1,
            "命令层必须恰好调用一次落点守卫（两次就是有人加了第二条导出路径）"
        );
        let guard_at = body.find(guard).expect("守卫调用不见了");
        let write_at = body.find(write).expect("写盘调用不见了");
        assert!(
            guard_at < write_at,
            "守卫必须早于任何写盘：本函数里守卫在第 {guard_at} 字节、写盘在第 {write_at} 字节"
        );
    }

    /// 五个阶段词在三个地方各存一份（V5 的 CHECK、`milestone.rs` 的 `STAGES`、
    /// `src/types/milestone.ts` 的 `MILESTONE_STAGES`）。CHECK 那条已由
    /// `v5_milestone_stage_check_refuses_words_outside_the_five` 按行为钉死；剩下两份是**文本**，
    /// 最容易漂，漂一个字的后果不是编译错而是「前端能选、后端当场拒」。这条把 Rust 与前端钉在一起。
    /// 坑 33 说 include_str! 的 needle 要用 concat! 拼，那是针对「在同一文件里数出现次数」的写法；
    /// 这里是拿 lib.rs 的门去比**另一个文件**的字面量，数不到自己头上，所以用普通字面量。
    #[test]
    fn stage_words_are_identical_in_rust_and_the_frontend_type() {
        assert_eq!(
            crate::milestone::STAGES,
            ["立项", "评审", "联调", "上线", "验收"],
            "Rust 侧词表被改了：V5 的 CHECK 与前端常量要一起改"
        );
        let ts = include_str!("../../src/types/milestone.ts");
        assert!(
            ts.contains("立项\", \"评审\", \"联调\", \"上线\", \"验收"),
            "前端 MILESTONE_STAGES 那五个词的引号/顺序/字面与 Rust 侧不一致"
        );
    }

    /// 四类词表三处同源：`expiry::KINDS`、`src/types/expiry.ts` 的 `EXPIRY_KINDS`、V7 的 CHECK。
    /// 这一条是 include_str! 文本门，钉的是**源码文本形状**（HANDOFF 坑 33：它挡得住「有人删了/改了那一行」，
    /// 挡不住参数写错）—— 判据本体在 expiry.rs 的 create_refuses_kind_words_outside_the_four 与
    /// db.rs 的 v7_kind_check_refuses_words_outside_the_four 两处，各有真断言。
    #[test]
    fn kind_words_are_identical_in_rust_and_the_frontend_type() {
        assert_eq!(
            crate::expiry::KINDS,
            ["维保", "域名", "证书", "其他"],
            "Rust 侧词表被改了：V7 的 CHECK 与前端常量要一起改"
        );
        let ts = include_str!("../../src/types/expiry.ts");
        assert!(
            ts.contains("维保\", \"域名\", \"证书\", \"其他"),
            "前端 EXPIRY_KINDS 那四个词的引号/顺序/字面与 Rust 侧不一致"
        );
        let ddl = include_str!("db.rs");
        assert!(
            ddl.contains("CHECK (kind IN ('维保','域名','证书','其他'))"),
            "V7 的 CHECK 词表与 Rust/前端两份不一致（注意 SQL 里是单引号、无空格）"
        );
    }
}
