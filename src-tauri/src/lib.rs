mod db;
mod doc_preview;
mod error;
mod extract;
mod index_job;
mod index_scan;
mod index_store;
mod ledger;
mod project;
mod search;
mod tokenize;
mod vault;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use tauri::{Manager, State};

use crate::error::{AppError, AppResult};
use crate::ledger::{
    CredentialInput, EnvInput, Ledger, LinkInput, NoteInput, ServerInput,
};
use crate::project::{DirInput, Project, ProjectInput};
use crate::vault::MasterKey;

struct AppState {
    conn: Mutex<rusqlite::Connection>,
    data_dir: PathBuf,
    /// 解锁后的主密钥只活在进程内存里：锁屏/退出即丢，重新解锁靠主密码或恢复码。
    mk: Mutex<Option<MasterKey>>,
    /// 索引作业跨线程共享：running/cancel 是原子量，summary 是上一轮的结论。
    index: Arc<index_job::IndexShared>,
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
}

#[tauri::command]
fn index_overview(state: State<'_, AppState>) -> AppResult<IndexOverview> {
    // 持锁段：只做库内的读。文件系统探测一条都不放进来 ——
    // `AppState.conn` 是全 crate 唯一那把 `Mutex<Connection>`，`db_status` / `project_list` /
    // `ledger_list` / `index_docs` / `search_docs` 全排在它后面；而未挂载的 SMB/网络盘上的
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

    // 出锁段：`is_dir()` 可以慢，但此时别的 IPC 进得来。
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
    })
}

/// 某一项目的文件清单（分页）。`limit` 的 clamp 是本路径唯一的边界校验：`list_docs`
/// （`index_store.rs:156-183`，只有一条 prepared statement）拿到负数时，SQLite 的语义是「不限行」，
/// 于是一次拉出整个项目（上限 50000 行）进 IPC 载荷。别把 clamp 挪进 `index_store.rs`：
/// `doc_hits` 的既有测试正靠 `limit=0`/`limit=-1` 钉住 SQLite 的原语义
/// （`index_store.rs:623`「负数 = 不限行」、`:624`「0 就是 0 行，不许悄悄变成默认值」），
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
/// 握着 `state.conn` 去读盘会让其他每条命令都等一次慢 IO（网络盘上一个大文件就是秒级卡顿，
/// 而首屏正是最容易连点的地方），因此这里刻意不写成 `let conn = db(&state)?; preview(&conn, ..)`。
#[tauri::command]
fn doc_preview(
    state: State<'_, AppState>,
    doc_id: String,
    query: String,
) -> AppResult<doc_preview::DocPreview> {
    let row = {
        let conn = db(&state)?;
        doc_preview::row_for_preview(&conn, &doc_id)?
    };
    doc_preview::render_preview(&doc_id, &row, &query)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let conn = db::open(&data_dir)?;
            app.manage(AppState {
                conn: Mutex::new(conn),
                data_dir,
                mk: Mutex::new(None),
                index: Arc::new(index_job::IndexShared::new()),
            });
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
            doc_preview
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}
