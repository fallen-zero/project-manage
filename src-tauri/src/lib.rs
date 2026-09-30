mod db;
mod error;
mod ledger;
mod project;
mod search;
mod tokenize;
mod vault;

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

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
    let fts5_available = conn
        .query_row(
            "SELECT 1 FROM pragma_compile_options WHERE compile_options LIKE '%FTS5%'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .is_ok();

    Ok(DbStatus {
        data_dir: state.data_dir.display().to_string(),
        db_file: db::db_path(&state.data_dir).display().to_string(),
        journal_mode,
        schema_version,
        fts5_available,
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
            ledger_note_delete
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}
