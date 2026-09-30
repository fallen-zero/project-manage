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
const TARGET_VERSION: i64 = 4;

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
            3 => V3,
            4 => V4,
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

/// v3：密钥库 + 信息台账。台账里的敏感列一律 `*_cipher` + `*_nonce` 成对出现，
/// 明文永不入库，也就永远不会被 M3 的全文索引捞到。
const V3: &str = "
CREATE TABLE vault_meta (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    kdf_m_cost INTEGER NOT NULL,
    kdf_t_cost INTEGER NOT NULL,
    kdf_p_cost INTEGER NOT NULL,
    pw_salt    BLOB NOT NULL,
    pw_cipher  BLOB NOT NULL,
    pw_nonce   BLOB NOT NULL,
    rc_salt    BLOB NOT NULL,
    rc_cipher  BLOB NOT NULL,
    rc_nonce   BLOB NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE ledger_envs (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    env        TEXT NOT NULL CHECK (env IN ('dev','test','pre','prod')),
    name       TEXT NOT NULL,
    url        TEXT,
    port       TEXT,
    note       TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at TEXT
);

CREATE TABLE ledger_credentials (
    id                 TEXT PRIMARY KEY,
    project_id         TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    env_id             TEXT REFERENCES ledger_envs(id) ON DELETE SET NULL,
    title              TEXT NOT NULL,
    username_cipher    BLOB,
    username_nonce     BLOB,
    password_cipher    BLOB,
    password_nonce     BLOB,
    url                TEXT,
    note               TEXT,
    created_at         TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at         TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at         TEXT
);

CREATE TABLE ledger_servers (
    id                    TEXT PRIMARY KEY,
    project_id            TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name                  TEXT NOT NULL,
    ip                    TEXT,
    bastion               TEXT,
    account_cipher        BLOB,
    account_nonce         BLOB,
    password_cipher       BLOB,
    password_nonce        BLOB,
    note                  TEXT,
    created_at            TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at            TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at            TEXT
);

CREATE TABLE ledger_links (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL DEFAULT 'other'
               CHECK (kind IN ('jira','zentao','wiki','doc','repo','other')),
    title      TEXT NOT NULL,
    url        TEXT NOT NULL,
    note       TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at TEXT
);

CREATE TABLE ledger_notes (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    body       TEXT NOT NULL DEFAULT '',
    tag        TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at TEXT
);

CREATE INDEX ix_ledger_project ON ledger_envs(project_id);
CREATE INDEX ix_ledger_cred_project ON ledger_credentials(project_id);
CREATE INDEX ix_ledger_srv_project ON ledger_servers(project_id);
CREATE INDEX ix_ledger_link_project ON ledger_links(project_id);
CREATE INDEX ix_ledger_note_project ON ledger_notes(project_id);
";

/// v4：全文索引。三处设计与 spec 有意不同，都是为了实测结果让路：
/// 1. `doc_rowid` 是对齐锚。FTS5 的 rowid 只能是整数，而业务主键是 TEXT uuid，所以主表额外
///    带一个 AUTOINCREMENT 整数列给虚表当 rowid；按 `doc_id` 列删虚表是全表扫，按 rowid 是点删。
/// 2. 虚表不带 `content=`，是独立存储。正文不另存一份原文：磁盘上本来就有一份。
/// 3. `missing` 现在就进 CHECK。CHECK 约束无法 ALTER，M5 的启动对账要它就得重建整张表。
const V4: &str = "
CREATE TABLE index_docs (
    doc_rowid    INTEGER PRIMARY KEY AUTOINCREMENT,
    id           TEXT    NOT NULL UNIQUE,
    project_id   TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path         TEXT    NOT NULL,
    ext          TEXT    NOT NULL,
    size         INTEGER NOT NULL DEFAULT 0,
    mtime        INTEGER NOT NULL DEFAULT 0,
    index_status TEXT    NOT NULL DEFAULT 'pending'
                 CHECK (index_status IN ('pending','ok','skipped','failed','missing')),
    skip_reason  TEXT,
    error_msg    TEXT,
    indexed_at   TEXT,
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (project_id, path)
);

CREATE INDEX ix_index_docs_project ON index_docs(project_id);
CREATE INDEX ix_index_docs_status  ON index_docs(index_status);

CREATE VIRTUAL TABLE index_docs_fts USING fts5(
    name_tokens,
    body_tokens,
    tokenize = 'unicode61'
);

INSERT OR IGNORE INTO settings (key, value) VALUES
    ('index_exclude_dirs', 'node_modules,dist,build,target,__pycache__,.git'),
    ('index_max_file_bytes', '20971520'),
    ('index_max_files_per_project', '50000');
";

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    /// v4 建表必须真的成功：FTS5 虚表在 bundled 构建里不是「假设可用」，这里直接建一次。
    #[test]
    fn v4_creates_index_tables_and_fts_is_writable() {
        let conn = open_in_memory().unwrap();
        // index_docs.project_id 带外键，父行必须先存在。
        conn.execute(
            "INSERT INTO projects (id, name) VALUES ('p-1', '索引测试项目')",
            [],
        )
        .unwrap();
        // 表存在
        let names: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master
                  WHERE name IN ('index_docs', 'index_docs_fts') ORDER BY name",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(names, vec!["index_docs", "index_docs_fts"]);

        // rowid 对齐：先插主表拿 rowid，再用同一个 rowid 插 FTS，JOIN 必须回得来
        conn.execute(
            "INSERT INTO index_docs (id, project_id, path, ext, index_status)
             VALUES ('u-1', 'p-1', 'C:/x/合同验收.docx', 'docx', 'ok')",
            [],
        )
        .unwrap();
        let rowid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO index_docs_fts (rowid, name_tokens, body_tokens) VALUES (?1, ?2, ?3)",
            params![rowid, "合同 验收", "甲方 要求 验收 指标"],
        )
        .unwrap();
        let hit: String = conn
            .prepare(
                "SELECT d.id FROM index_docs_fts f
                   JOIN index_docs d ON d.doc_rowid = f.rowid
                  WHERE index_docs_fts MATCH ?1",
            )
            .unwrap()
            .query_row(params!["\"验收\""], |r| r.get(0))
            .unwrap();
        assert_eq!(hit, "u-1");
    }

    /// status 是 CHECK 约束，写错值必须在库层就挡住，而不是靠每个调用方自觉；
    /// `missing` 也要现在就进枚举——CHECK 改不动，M5 的对账再补就得重建整张表。
    #[test]
    fn index_status_check_accepts_the_five_documented_states() {
        let conn = open_in_memory().unwrap();
        // index_docs.project_id 有 REFERENCES projects(id) 外键，而 after_open 里
        // PRAGMA foreign_keys=ON，所以必须先落一行父项目，插入才走得通。
        conn.execute(
            "INSERT INTO projects (id, name) VALUES ('p-1', '索引测试项目')",
            [],
        )
        .unwrap();
        for (i, status) in ["pending", "ok", "skipped", "failed", "missing"]
            .iter()
            .enumerate()
        {
            conn.execute(
                "INSERT INTO index_docs (id, project_id, path, ext, index_status)
                 VALUES (?1, ?2, ?3, 'txt', ?4)",
                params![format!("u-{i}"), "p-1", format!("C:/x/{i}.txt"), status],
            )
            .unwrap_or_else(|e| panic!("{status} 应该在枚举内：{e}"));
        }
        let e = conn
            .execute(
                "INSERT INTO index_docs (id, project_id, path, ext, index_status)
                 VALUES ('u-x', 'p-1', 'C:/x/xx.txt', 'txt', 'done')",
                [],
            )
            .unwrap_err();
        assert!(
            e.to_string().contains("CHECK"),
            "未登记的状态必须被 CHECK 约束拒绝，实际：{e}"
        );
    }

    /// 上限与排除规则要能被 UI 改，所以 v4 必须落种子值；否则 Task 6 的 load() 拿到空字符串，
    /// 会把「没有排除目录」当成默认行为，把 node_modules 全灌进索引。
    #[test]
    fn v4_seeds_index_settings_with_the_documented_limits() {
        let conn = open_in_memory().unwrap();
        let read = |key: &str| -> String {
            conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(
            read("index_exclude_dirs"),
            "node_modules,dist,build,target,__pycache__,.git"
        );
        assert_eq!(read("index_max_file_bytes"), "20971520", "spec 定的单文件 20 MB");
        assert_eq!(read("index_max_files_per_project"), "50000");
        assert_eq!(
            conn.query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_meta",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            4
        );
    }

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
