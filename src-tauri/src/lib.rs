mod db;
mod error;
mod project;

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use serde::Serialize;
use tauri::{Manager, State};

use crate::error::{AppError, AppResult};
use crate::project::{DirInput, Project, ProjectInput};

struct AppState {
    conn: Mutex<rusqlite::Connection>,
    data_dir: PathBuf,
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
            project_status_options
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}
