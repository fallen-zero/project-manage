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
        // 请求 WAL 之后必须读回来判（pragma_update 的 Ok 不代表真落到 WAL）。拆出去的那半段
        // 见 assert_wal —— 本行的 side effect 留在 after_open，判据留在 assert_wal，M8 只调判据。
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        assert_wal(conn)?;
    } else {
        // 内存库的 journal_mode 只能是 MEMORY/OFF，这次请求会被 SQLite 直接忽略，拿不到 WAL 是无害的。
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
    }

    pragma(conn, "synchronous", "NORMAL")?;
    pragma(conn, "foreign_keys", "ON")?;
    pragma(conn, "busy_timeout", "5000")?;
    migrate(conn)
}

/// 现读 `PRAGMA journal_mode` 并断它等于 wal。M0 那条「不信 `pragma_update` 的 Ok、必须读回」
/// 硬门的读回半段；从 `after_open` 拆出来是给 M8 导入收尾复用的 —— `Connection::restore` 灌的是
/// **已打开**的连接，根本不经过 `after_open`，不现读一次就留下「库内容整库换过、WAL 从没在这份
/// 内容上被断过」的状态（spec §七 那条判据）。错误码与措辞一字不改，`durable_open_really_runs_in_wal`
/// 与 `side_file_unopenable_pins_the_readback_invariant` 钉的就是这两条错误的形状。
pub fn assert_wal(conn: &Connection) -> AppResult<()> {
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
    Ok(())
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
/// 提 `pub(crate)` 是给 M8 的验包判据比：包里的库比本机新就该拒（`backup::read_package`）。
pub(crate) const TARGET_VERSION: i64 = 7;

/// `pub(crate)` 是 M8 加的：`backup_import` 灌完包内库要能重放迁移（包里可能是旧版本）。
pub(crate) fn migrate(conn: &Connection) -> AppResult<()> {
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
        let ddl = match target {
            1 => V1,
            2 => V2,
            3 => V3,
            4 => V4,
            5 => V5,
            6 => V6,
            7 => V7,
            _ => "",
        };
        apply_version(conn, target, ddl)?;
    }
    Ok(())
}

/// 一个版本 = 一个事务：DDL 与版本行同生同死。
///
/// 不能拆成两条自动提交语句：`execute_batch` 成功不代表版本已登记，中间出任何事
/// （报错、断电、进程被杀）都会留下「对象建了一半、`schema_meta` 还停在旧版本」的库，
/// 重放同一个版本时 `CREATE TABLE`（V4 没写 `IF NOT EXISTS`）直接失败，而本项目对唯一
/// 持久库是拒绝启动的，于是应用永久起不来。`Transaction` 的 drop 默认就是回滚
/// （rusqlite 0.40.2 `src/transaction.rs:25` 的 `DropBehavior::Rollback` 注释写明
/// "This is the default"），所以这里不需要手写 rollback 分支；用 `unchecked_transaction`
/// 是因为 `migrate` 只拿到 `&Connection`，`transaction()` 要 `&mut self`。
fn apply_version(conn: &Connection, target: i64, ddl: &str) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(ddl)?;
    tx.execute(
        "INSERT INTO schema_meta (version, applied_at) VALUES (?1, datetime('now'))",
        [target],
    )?;
    // commit 自己返回 rusqlite::Error，和上面两处 `?` 一样过 From 收敛成 AppError。
    Ok(tx.commit()?)
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

/// v5：待办清单 + 项目里程碑（M9，spec §三）。
/// 三处是「照需求写」而不是「照方便写」，别顺手改：
/// 1. `stage` 的五个词是 `思维导图.md:35-36` 原文（立项/评审/联调/上线/验收）。它与
///    `projects.status` 那八段**是两套词**、共有「立项」「上线」两个同形词而语义不同（spec D4）：
///    status 是「项目当前在哪」，stage 是「过程节点」。别统一，别让一个下拉喂两个字段。
/// 2. `done` 用 INTEGER + CHECK 而不是裸布尔约定：SQLite 没有布尔列，而 CHECK 挡住「直写库的人」，
///    serde 侧仍然出 `bool`（`rusqlite` 的 `bool` ToSql/FromSql 走 0/1）。
/// 3. 日期列**只由写入侧保证形状**（`todo::is_iso_date`），SQL 里不做任何日期解释：
///    `strftime('now')` 是 UTC，用它比日期会把本机「今天到期」算错一天（spec D1）。
const V5: &str = "
CREATE TABLE todos (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    due_date   TEXT,
    done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0,1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at TEXT
);

CREATE TABLE milestones (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    stage      TEXT NOT NULL CHECK (stage IN ('立项','评审','联调','上线','验收')),
    plan_date  TEXT,
    note       TEXT NOT NULL DEFAULT '',
    done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0,1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at TEXT
);

CREATE INDEX ix_todo_project ON todos(project_id);
CREATE INDEX ix_milestone_project ON milestones(project_id);
";

/// V6 = 日报表（M10 spec §五.1）。三条口径延续 V5 的注释：
/// 1. 软删沿用 `deleted_at`；`ON DELETE CASCADE` 因项目是软删（`project.rs` 的 UPDATE）而永不触发，
///    所以「跨项目读要自带 p.deleted_at IS NULL」这条规矩照旧适用于本表。
/// 2. `hours` 用 REAL 且**不加 CHECK**：表由本程序唯一写入，`0<h<=24` 与两位小数的判据在 Rust 侧
///    （`daily_report::check_hours`）。CHECK 违例冒出来是 `db_failed`，会把「校验失败」伪装成库故障。
/// 3. `report_date` 的形状与必填**只由写入侧保证**（`todo::req_date`）：SQL 里不做任何日期解释，
///    `strftime`/`date` 是 UTC，用它比较会把本机日期算错（M9 D7 同一个理由，spec D9 沿用）。
const V6: &str = "
CREATE TABLE daily_reports (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    report_date TEXT NOT NULL,
    content     TEXT NOT NULL,
    hours       REAL NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at  TEXT
);

CREATE INDEX ix_daily_report_project ON daily_reports(project_id);
";

/// V7 = 到期事项表（M11-a spec §四）。四条口径延续 V5/V6 的注释：
/// 1. 软删沿用 `deleted_at`；`ON DELETE CASCADE` 因项目是软删（`project.rs` 的 UPDATE）而永不触发，
///    所以「跨项目读要自带 p.deleted_at IS NULL」这条规矩照旧适用于本表（Task 2/3 各落一次）。
/// 2. `kind` 这里留 CHECK 只是**直写库的最后闸门**：命令路径的合法值判据在 `expiry::check_kind`，
///    因为 CHECK 违例冒出来是 `db_failed`，会把「你选的种类不在词表里」伪装成库故障（V5 的 stage 同形）。
/// 3. `expire_date` **必填但不加形状 CHECK**：形状与空白由写入侧的 `todo::req_date` 判（D2 的语义是
///    「这一行的有效期到 X」，没 X 就没意义）；SQL 里不做任何日期解释（`strftime`/`date` 是 UTC）。
/// 4. **零回填**：老项目的维保/域名/证书信息不由迁移猜，由用户自己录（延续 M9 为 V5 立的规矩）。
const V7: &str = "
CREATE TABLE project_expiries (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('维保','域名','证书','其他')),
    label       TEXT NOT NULL,
    expire_date TEXT NOT NULL,
    note        TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at  TEXT
);

CREATE INDEX ix_expiry_project ON project_expiries(project_id);
";

#[cfg(test)]
mod tests {
    use super::*;
    // 测试体按简报走 `db::` 全路径调用（与 M8 `backup_import` 实际调用 `db::assert_wal` 的形状一致），
    // 而 edition 2021 下模块自身不进入子模块作用域，故显式把 `crate::db` 引进来。
    use crate::db;
    use rusqlite::params;

    /// index_docs.project_id 有 REFERENCES projects(id) 外键，而 after_open 里
    /// PRAGMA foreign_keys=ON，所以必须先落一行父项目，插子表才走得通。
    fn seed_project(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name) VALUES (?1, '索引测试项目')",
            params![id],
        )
        .unwrap();
    }

    /// v4 建表必须真的成功：FTS5 虚表在 bundled 构建里不是「假设可用」，这里直接建一次。
    #[test]
    fn v4_creates_index_tables_and_fts_is_writable() {
        let conn = open_in_memory().unwrap();
        seed_project(&conn, "p-1");
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
        seed_project(&conn, "p-1");
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
        // 引用 TARGET_VERSION 而非写字面数字：本断言钉的是「迁移链真的跑到了链头并写下版本号」，
        // 写死数字会在每次加版本时假红（与 lib.rs 的 `bootstrap_data...` 同一形制）。链头**具体等于几**
        // 由本文件末尾那条 `..._schema_meta_reaches_target_version_...` 钉死，这里不与之重复；
        // 那条测试每加一个版本都会改名（V5→v5_、V6→v6_、V7→v7_），所以这里只写它的名字形状，不写死版本号。
        assert_eq!(
            conn.query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_meta",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            TARGET_VERSION
        );
    }

    /// 一个版本 = 一个事务：失败的 batch 既不留半成品表，也不留版本号行。
    /// 守的是「断电后下次启动还起得来」：这个库是唯一副本，没有回退路径。
    #[test]
    fn a_failed_version_leaves_no_partial_objects_and_no_version_row() {
        let conn = open_in_memory().unwrap();
        apply_version(&conn, 99, "CREATE TABLE atomic_probe(x); CREATE TABLE atomic_probe(x);")
            .expect_err("同名 CREATE 的第二条必须让整个 batch 失败");
        let objs: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'atomic_probe'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(objs, 0, "事务回滚后不该留下第一条已建好的表");
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM schema_meta WHERE version = 99", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "失败的版本不该写下版本号");
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

    /// 拆分前后同红同绿：`assert_wal` 只对**读回**负责。这里刻意不用 `open` 那条路径（它自己会
    /// 先 `pragma_update`），而是把一份已 WAL 的文件库改回 DELETE 模式再喂给 `assert_wal` ——
    /// 于是这条测试钉的是「读回不是 wal 就必须红」，而不是「open 之后一般是 wal」。
    /// M8 的 `backup_import` 在 `restore` 之后现读它（`restore` 绕过 `after_open`，spec §七），
    /// 这条门没有测试就等于 spec §十一.8 那条风险没人守。
    #[test]
    fn assert_wal_rejects_a_db_that_fell_out_of_wal() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open(dir.path()).unwrap();
        // 先证正对照：刚 open 完必须过（否则下一条断言可能是恒红）。
        db::assert_wal(&conn).expect("open 之后 assert_wal 必须过");
        conn.pragma_update(None, "journal_mode", "DELETE").unwrap();
        let err = db::assert_wal(&conn).expect_err("掉回 DELETE 必须红，否则 M8 的导入收尾门什么都没钉住");
        assert_eq!(err.code, "db_pragma_failed");
        // 读回的模式是 SQLite 原样给的（本机实测为小写 delete），所以这里大小写不敏感地断「错误里
        // 念出了实际读到的模式」—— 与 assert_wal 用 eq_ignore_ascii_case 同一个理由，别改成硬编码小写。
        assert!(
            err.message.to_uppercase().contains("DELETE"),
            "错误里要念出实际读到的模式：{}",
            err.message
        );
    }

    #[test]
    fn the_three_20mb_defaults_are_one_number() {
        // M5 终审发现的「四个家」：种子值 / get 默认串 / parse 回退值 三处各写一遍 20 MB，
        // 没有一致性断言 ⇒ 改一处就悄悄漂。本任务把后两处收敛到 index_scan 的一个 pub const，
        // 本断言钉「种子 == 那个 const」。
        let conn = open_in_memory().unwrap();
        let seeded: String = conn
            .query_row("SELECT value FROM settings WHERE key = 'index_max_file_bytes'", [], |r| r.get(0))
            .unwrap();
        let seeded: u64 = seeded.parse().unwrap();
        assert_eq!(
            seeded,
            crate::index_scan::DEFAULT_MAX_FILE_BYTES,
            "settings 种子值与 ScanOptions 的默认上限必须是同一个数"
        );
    }

    /// V5 必须真把两张表和两个索引建出来（`CREATE TABLE` 没写 `IF NOT EXISTS`，
    /// 所以「对象建了一半、schema_meta 停在旧版本」这个失败形状在事务保护下不会发生，
    /// 但建了表漏了索引是看得见的，这里一起钉）。
    #[test]
    fn v5_creates_todo_and_milestone_tables_and_indexes() {
        let conn = open_in_memory().unwrap();
        let names: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master
                  WHERE name IN ('todos','milestones','ix_todo_project','ix_milestone_project')
                  ORDER BY name",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            names,
            vec![
                "ix_milestone_project",
                "ix_todo_project",
                "milestones",
                "todos"
            ],
            "V5 的四个对象必须全在：{names:?}"
        );
    }

    #[test]
    fn v7_schema_meta_reaches_target_version_seven() {
        let conn = open_in_memory().unwrap();
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 7, "迁移必须跑到 7");
        assert_eq!(TARGET_VERSION, 7, "= 上一条，两处别各写各的");
    }

    /// D4：`stage` 是 CHECK 枚举。这五个词是需求原文那五个（思维导图.md:35-36），
    /// 词表外的值必须被挡；报错文本要确认是 CHECK 而不是别的约束。
    #[test]
    fn v5_milestone_stage_check_refuses_words_outside_the_five() {
        let conn = open_in_memory().unwrap();
        seed_project(&conn, "p-stage");
        for stage in ["立项", "评审", "联调", "上线", "验收"] {
            conn.execute(
                "INSERT INTO milestones (id, project_id, stage) VALUES (?1, 'p-stage', ?2)",
                params![format!("m-{stage}"), stage],
            )
            .unwrap_or_else(|e| panic!("五段词表里的「{stage}」必须插得进去：{e}"));
        }
        let err = conn
            .execute(
                "INSERT INTO milestones (id, project_id, stage) VALUES ('m-bad', 'p-stage', '初验')",
                [],
            )
            .expect_err("词表外的阶段名必须被 CHECK 拒掉");
        assert!(
            err.to_string().contains("CHECK constraint failed"),
            "要的是 CHECK 那条错，实际：{err}"
        );
    }

    /// D5：`done` 只吃 0/1。写 2 必须报错 —— 否则「布尔」只是约定。
    #[test]
    fn v5_done_column_check_refuses_values_other_than_zero_and_one() {
        let conn = open_in_memory().unwrap();
        seed_project(&conn, "p-done");
        // 两张表的列集不同（todos 无 stage、milestones 无 title），各写各的 INSERT 才只撞到
        // done 那一条 CHECK；共用一支含全部列的 INSERT 会先报「no such column」而绕开 CHECK。
        let cases = [
            (
                "todos",
                "INSERT INTO todos (id, project_id, title, done) VALUES ('x', 'p-done', 't', 2)",
            ),
            (
                "milestones",
                "INSERT INTO milestones (id, project_id, stage, done) VALUES ('x', 'p-done', '立项', 2)",
            ),
        ];
        for (table, sql) in cases {
            let err = conn
                .execute(sql, [])
                .expect_err("两张表的 done 都不许收 2");
            assert!(
                err.to_string().contains("CHECK constraint failed"),
                "{table} 的 done 要的是 CHECK，实际：{err}"
            );
        }
    }

    /// D8 的前提事实：`PRAGMA foreign_keys=ON` 真的在挡外键。
    /// 这条不是装饰 —— 它是「软删项目不会被 CASCADE 删掉子行」的另一半证明
    /// （另一半在 T2/T3 的 cross_project 测试里：行还在、只是读不到）。
    #[test]
    fn v5_both_new_tables_enforce_the_project_foreign_key() {
        let conn = open_in_memory().unwrap();
        let err = conn
            .execute(
                "INSERT INTO todos (id, project_id, title) VALUES ('t-x', 'no-such-project', '甲')",
                [],
            )
            .expect_err("不存在的项目必须挡住");
        assert!(
            err.to_string().contains("FOREIGN KEY constraint failed"),
            "要的是 FK 那条错，实际：{err}"
        );
        let err2 = conn
            .execute(
                "INSERT INTO milestones (id, project_id, stage) VALUES ('m-x', 'no-such-project', '立项')",
                [],
            )
            .expect_err("milestones 同理");
        assert!(
            err2.to_string().contains("FOREIGN KEY constraint failed"),
            "要的是 FK 那条错，实际：{err2}"
        );
    }

    /// 默认值是线格式的一部分：日期列可空、done 默认 0、note 默认空串。
    #[test]
    fn v5_dates_default_to_null_and_done_defaults_to_zero() {
        let conn = open_in_memory().unwrap();
        seed_project(&conn, "p-def");
        conn.execute(
            "INSERT INTO todos (id, project_id, title) VALUES ('t-1', 'p-def', '只填标题')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO milestones (id, project_id, stage) VALUES ('m-1', 'p-def', '立项')",
            [],
        )
        .unwrap();
        let (due, done): (Option<String>, i64) = conn
            .query_row(
                "SELECT due_date, done FROM todos WHERE id = 't-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(due, None, "due_date 不填就是 NULL，不是空串");
        assert_eq!(done, 0, "新行默认未完成");
        let (plan, note, mdone): (Option<String>, String, i64) = conn
            .query_row(
                "SELECT plan_date, note, done FROM milestones WHERE id = 'm-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(plan, None, "plan_date 不填就是 NULL");
        assert_eq!(note, "", "里程碑 note 默认空串（需求原文把它当可选项，不是必填）");
        assert_eq!(mdone, 0);
    }

    /// 列集合与顺序一起钉：顺序漂了 `SELECT_COLUMNS`（Task 2）会静默错位读，
    /// 而改名会同时打掉 `src/types/daily-report.ts` 与 xlsx 表头两处。
    #[test]
    fn v6_creates_daily_reports_with_documented_columns() {
        let conn = db::open_in_memory().unwrap();
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('daily_reports') ORDER BY cid")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(
            cols.join(","),
            "id,project_id,report_date,content,hours,created_at,updated_at,deleted_at",
            "V6 的列集合/顺序变了要同步 daily_report.rs 的 SELECT_COLUMNS 与 xlsx 表头"
        );
    }

    /// 这一格钉的是**为什么必填日期必须走 `req_date` 而不是 `norm_date`**（spec §五.2）：
    /// 缺列/NULL 走到 SQL 才有约束来挡，而那一挡冒出来是 SQLite 的 NOT NULL，不是我们的 invalid_input。
    #[test]
    fn v6_report_date_not_null_is_enforced_at_sqlite_layer() {
        let conn = db::open_in_memory().unwrap();
        seed_project(&conn, "p1");
        let err = conn
            .execute(
                "INSERT INTO daily_reports (id, project_id, content, hours) VALUES ('d1','p1','甲',1.0)",
                [],
            )
            .unwrap_err();
        assert!(
            format!("{err}").contains("NOT NULL"),
            "预期是 NOT NULL 挡住（而不是 CHECK 或外键），实际：{err}"
        );
    }

    /// 本轮新立的规矩：**V6 零回填**（延续 M9 为 V5 立的那条）。迁移里不许塞业务行。
    #[test]
    fn v6_backfills_nothing() {
        let conn = db::open_in_memory().unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_reports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "新建库跑完迁移后本表必须是空的");
    }

    #[test]
    fn v7_creates_project_expiries_with_documented_columns() {
        let conn = db::open_in_memory().unwrap();
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('project_expiries') ORDER BY cid")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(
            cols.join(","),
            "id,project_id,kind,label,expire_date,note,created_at,updated_at,deleted_at",
            "V7 的列集合/顺序变了要同步 expiry.rs 的 SELECT_COLUMNS"
        );
        // `sql IS NOT NULL` 不是洁癖：TEXT 主键会让 SQLite 自带一支 `sqlite_autoindex_*`（`sql` 为 NULL），
        // 每张表都有（daily_reports 同理）。不排掉它，下面那句「索引必须恰好只有 ix_expiry_project」
        // 在任何正确 DDL 下都恒红；排掉后它钉的才是**本迁移自己写下的索引**：多建一条日期索引
        // （spec §四 明令不建）就会红，漏建 ix_expiry_project 也会红。
        let idx: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'project_expiries' AND sql IS NOT NULL ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(idx, vec!["ix_expiry_project"], "只建项目维度的索引，**没有日期索引**（spec §四）：{idx:?}");
    }

    /// D2 的 SQL 层兜底：`expire_date` NOT NULL 真的挡得住直写库。
    /// 这条钉的是「为什么必填日期走 `req_date` 而不是 `norm_date`」的同一条理由（延续 v6 那格）。
    #[test]
    fn v7_expire_date_not_null_is_enforced_at_sqlite_layer() {
        let conn = db::open_in_memory().unwrap();
        seed_project(&conn, "p1");
        let err = conn
            .execute(
                "INSERT INTO project_expiries (id, project_id, kind, label) VALUES ('x1','p1','域名','a.gov.cn')",
                [],
            )
            .unwrap_err();
        assert!(
            format!("{err}").contains("NOT NULL"),
            "预期是 NOT NULL 挡住（而不是 CHECK 或外键），实际：{err}"
        );
    }

    /// 四类词表外的值必须被 SQL 的 CHECK 挡住：这是绕过命令直写库的那条路的最后一道闸门。
    /// 报错要确认是 CHECK 而不是别的约束（延续 `v5_milestone_stage_check_...` 的判法）。
    #[test]
    fn v7_kind_check_refuses_words_outside_the_four() {
        let conn = db::open_in_memory().unwrap();
        seed_project(&conn, "p-kind");
        for kind in ["维保", "域名", "证书", "其他"] {
            conn.execute(
                "INSERT INTO project_expiries (id, project_id, kind, label, expire_date)
                  VALUES (?1, 'p-kind', ?2, '一行', '2027-01-31')",
                rusqlite::params![format!("e-{kind}"), kind],
            )
            .unwrap_or_else(|e| panic!("四类词表里的「{kind}」必须插得进去：{e}"));
        }
        let err = conn
            .execute(
                "INSERT INTO project_expiries (id, project_id, kind, label, expire_date)
                  VALUES ('e-bad', 'p-kind', '备案', '一行', '2027-01-31')",
                [],
            )
            .expect_err("词表外的种类名必须被 CHECK 拒掉（需求原文只有三类，「其他」是 D7 加的那一格）");
        assert!(
            err.to_string().contains("CHECK constraint failed"),
            "要的是 CHECK 那条错，实际：{err}"
        );
    }

    #[test]
    fn v7_backfills_nothing() {
        let conn = db::open_in_memory().unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM project_expiries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "新建库跑完迁移后本表必须是空的（迁移里不许塞业务行）");
    }
}
