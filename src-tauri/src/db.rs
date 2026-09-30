use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

/// 用户数据的唯一持久库。丢了它等于丢了全部台账，所以它以 WAL 运行为启动前提。
pub const DB_FILE_NAME: &str = "ledger.db";

pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join(DB_FILE_NAME)
}

/// 打开数据目录下的持久库。
pub fn open(data_dir: &Path) -> AppResult<Connection> {
    std::fs::create_dir_all(data_dir).map_err(|e| AppError::io(data_dir, &e))?;
    let path = db_path(data_dir);
    let conn = Connection::open(&path).map_err(|e| {
        AppError::new(
            "db_open_failed",
            &format!("打开 {} 失败：{e}", path.display()),
            Some("确认数据目录可写、库文件未被其它进程独占"),
        )
    })?;
    after_open(&conn, true)?;
    Ok(conn)
}

/// 仅测试使用的可丢弃库。M1 起 service 层单测全部基于它，故此时 lib 目标内暂无调用点。
#[allow(dead_code)]
pub fn open_in_memory() -> AppResult<Connection> {
    let conn = Connection::open_in_memory()?;
    after_open(&conn, false)?;
    Ok(conn)
}

/// 「请求切 WAL」和「真的跑在 WAL 上」是两件事，所以这里不信 Ok/Err 而是读回断言：
/// rusqlite 0.40.2 的 `pragma_update` 就是 `execute_batch`，而后者把 pragma 返回的行丢弃
/// （`Error::ExecuteReturnedResults` 被 `if false` 关掉）；SQLite 拒绝切换时是**成功返回原模式**，
/// 网络盘与被安全软件拦截的路径正走这条静默分支。只判 Err 会留下「无持久回滚日志 +
/// synchronous=NORMAL」的连接照常启动，断电即损坏用户唯一的库且无任何信号。
///
/// 只有内存库允许没有 WAL：它不承载任何用户数据。
fn after_open(conn: &Connection, durable: bool) -> AppResult<()> {
    if durable {
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .map_err(|e| {
                AppError::new(
                    "db_pragma_failed",
                    &format!("读取 journal_mode 失败：{e}"),
                    Some("确认数据目录可写、ledger.db 未损坏（必要时从备份恢复），或把数据目录换到本地磁盘"),
                )
            })?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(AppError::new(
                "db_pragma_failed",
                &format!("无法把数据库切换为 WAL 模式（当前为 {mode}）"),
                Some("数据库所在路径不支持 WAL（网络盘或被安全软件拦截常见），请把数据目录换到本地磁盘"),
            ));
        }
    } else {
        // 内存库的 journal_mode 只能是 MEMORY/OFF，这次请求会被 SQLite 直接忽略，拿不到 WAL 是无害的。
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
    }

    pragma(conn, "synchronous", "NORMAL")?;
    pragma(conn, "foreign_keys", "ON")?;
    pragma(conn, "busy_timeout", "5000")?;
    migrate(conn)
}

fn pragma(conn: &Connection, name: &str, value: &str) -> AppResult<()> {
    conn.pragma_update(None, name, value).map_err(|e| {
        AppError::new(
            "db_pragma_failed",
            &format!("设置 {name}={value} 失败：{e}"),
            Some("确认数据目录所在文件系统支持该设置"),
        )
    })
}

/// 线性版本迁移：一个版本一段 DDL，只追加不改历史，便于备份包跨版本恢复。
const TARGET_VERSION: i64 = 2;

fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta (
             version    INTEGER NOT NULL,
             applied_at TEXT    NOT NULL
         );",
    )?;

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_meta",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    for target in (current + 1)..=TARGET_VERSION {
        conn.execute_batch(match target {
            1 => V1,
            2 => V2,
            _ => "",
        })?;
        conn.execute(
            "INSERT INTO schema_meta (version, applied_at) VALUES (?1, datetime('now'))",
            [target],
        )?;
    }
    Ok(())
}

const V1: &str = "CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);";

const V2: &str = "
CREATE TABLE projects (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    code              TEXT,
    manager           TEXT,
    customer          TEXT,
    contact_name      TEXT,
    contact_phone     TEXT,
    contract_no       TEXT,
    contract_period   TEXT,
    delivery_deadline TEXT,
    status            TEXT NOT NULL DEFAULT '立项'
                      CHECK (status IN ('立项','需求','开发','测试','预发','上线','运维','归档')),
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at        TEXT
);

CREATE TABLE project_tags (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    tag        TEXT NOT NULL,
    UNIQUE (project_id, tag)
);

CREATE TABLE project_dirs (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('root','entry')),
    path       TEXT NOT NULL,
    label      TEXT,
    sort       INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (project_id, path)
);

-- 一个项目只有一个主根目录，这是索引器「一个根一个扫描任务」的前提
CREATE UNIQUE INDEX ux_project_root ON project_dirs(project_id) WHERE kind = 'root';
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_open_really_runs_in_wal() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        assert!(db_path(dir.path()).exists());
    }

    #[test]
    fn corrupt_header_is_reported_as_read_failure_not_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = db_path(dir.path());
        std::fs::write(&path, b"this is definitely not a sqlite database header").unwrap();

        let err = open(dir.path()).unwrap_err();
        assert_eq!(err.code, "db_pragma_failed");
        assert!(
            err.message.starts_with("读取 journal_mode 失败"),
            "损坏库必须走读回失败的文案，不能被误诊成「换本地磁盘」：{}",
            err.message
        );
        assert!(err.hint.unwrap().contains("损坏"));
    }

    #[test]
    fn side_file_unopenable_pins_the_readback_invariant() {
        // 先建好一个干净 WAL 库：最后一个连接关闭时 SQLite 会 checkpoint 并删除 -wal 侧文件，
        // 于是把 <db>-wal 做成目录后，第二次打开必然在读回处失败。这个前缀只可能由读回的
        // map_err 产生——谁把读回断言删掉或退化回只看 Err，这条就会红。
        let dir = tempfile::tempdir().unwrap();
        let path = db_path(dir.path());
        {
            let conn = open(dir.path()).unwrap();
            conn.execute("INSERT INTO schema_meta (version, applied_at) VALUES (0, 'x')", [])
                .unwrap();
            drop(conn);
        }
        std::fs::create_dir(format!("{}-wal", path.display())).unwrap();

        let err = open(dir.path()).unwrap_err();
        assert_eq!(err.code, "db_pragma_failed");
        assert!(err.message.starts_with("读取 journal_mode 失败"));
    }

    #[test]
    fn in_memory_open_is_allowed_without_wal() {
        let conn = open_in_memory().unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "memory");
    }
}
