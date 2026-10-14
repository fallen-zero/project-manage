//! 备份包（M8）。三个动作：导出（快照 → 剥索引 → 打 zip）、验包（只读校验）、导入（验完 → 存应急包
//! → 把包内库灌回**当前已打开**的连接）。本模块**不碰 IPC、不碰 AppState、不读任何全局**：
//! 全部喂现成的 `&Connection` / `&mut Connection`，所以三档测试都能在没有 tauri 的情况下跑。
//!
//! 为什么不用 `Connection::open` 覆盖 `ledger.db`：那个文件正被应用自己开着（`AppState.conn`），
//! Windows 上覆盖它是整条路最脏的一笔；`restore` 灌活连接同时免掉「重启应用」这一步（spec D2）。
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

/// 包格式版本。只承诺「同版本读写 + 拒绝更高 `schemaVersion` 的包」，不做跨版本兼容矩阵（spec §五）。
pub const FORMAT_VERSION: u32 = 1;
/// 包内条目名。**故意不叫 `ledger.db`**：免得有人把它双击或拷回数据目录当活库用（spec §五）。
pub const DB_ENTRY: &str = "backup.db";
pub const MANIFEST_ENTRY: &str = "manifest.json";

/// D6 的代价不粉饰：库里只封四列 `*_cipher`，其余全是明文列，一个没加密的包 ≈ 一份内网入口清单。
/// 界面念的是这一句，manifest 里存的也是这一句 —— 一份文本，两处出（别抄两遍再漂）。
pub fn warn_plaintext_columns() -> &'static str {
    "包内非 *_cipher 列为明文：项目名/编号/负责人/客户/联系人/合同号、服务器 ip 与堡垒机、各类 url、各表 note。"
}

/// manifest 里的一条文件记录 `{name,size,sha256}`：`size`/`sha256` 都打在条目的**原始字节**上。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

/// 台账侧七张表的行数。导出时从**快照连接**现数（不是从活库数），这样 inspect 念出来的数字
/// 与包内容必然同源（spec §五）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub projects: i64,
    pub ledger_envs: i64,
    pub ledger_credentials: i64,
    pub ledger_servers: i64,
    pub ledger_links: i64,
    pub ledger_notes: i64,
    pub settings: i64,
}

/// 明文列清单那句走 `plaintext_warning`，所以 `Manifest` 的字段数 = spec §五 那 8 个。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format_version: u32,
    pub app_version: String,
    pub schema_version: i64,
    pub created_at: String,
    pub index_excluded: bool,
    pub counts: Counts,
    pub files: Vec<FileEntry>,
    pub plaintext_warning: String,
}

/// 出参不含 sha256、不含任何包内内容（红线④：验证脚本与错误面只回路径/条数/大小/条目名）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    pub path: String,
    pub entries: Vec<String>,
    pub bytes: u64,
    pub counts: Counts,
    pub schema_version: i64,
}

/// 十六进制小写。手写，不为 hex 再加一个 crate（`Cargo.lock` 的门：不新增 `[[package]]`）。
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn count_one(conn: &rusqlite::Connection, table: &str) -> AppResult<i64> {
    // 表名是本函数内写死的字面量，不是入参：拼 SQL 在这里没有注入面（`counts` 的七张表都是 DDL 里定死的）。
    let n: i64 = conn
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .map_err(|e| {
            AppError::new(
                "backup_count_failed",
                &format!("数 {table} 失败：{e}"),
                Some("这份库的表结构与预期不符；换一份备份包或从数据目录的应急包恢复"),
            )
        })?;
    Ok(n)
}

/// 七张表逐张现数。**任一失败就整份导出中止**（spec §五：宁可导出失败，也不产出数字不全的 manifest ——
/// 「看得见包内有什么」是 D5 花一条命令换来的，不能是空的）。
pub fn read_counts(conn: &rusqlite::Connection) -> AppResult<Counts> {
    Ok(Counts {
        projects: count_one(conn, "projects")?,
        ledger_envs: count_one(conn, "ledger_envs")?,
        ledger_credentials: count_one(conn, "ledger_credentials")?,
        ledger_servers: count_one(conn, "ledger_servers")?,
        ledger_links: count_one(conn, "ledger_links")?,
        ledger_notes: count_one(conn, "ledger_notes")?,
        settings: count_one(conn, "settings")?,
    })
}

/// 时间戳走 SQLite 的 `strftime`，零新依赖（不引 chrono；仓库现在也没有它）。
/// 用 `now`（UTC）而不是本地时间：包被带到别的机器上也不会因为时区说得含糊。
pub fn now_utc(conn: &rusqlite::Connection) -> AppResult<String> {
    conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| r.get(0))
        .map_err(|e| {
            AppError::new(
                // 码名按故障定：`backup_count_failed` 是数表失败（见 `count_one`），取时间戳失败
                // 是另一件事 —— T8/T9 若按码分支不能把两种故障并进同一支。
                "backup_timestamp_failed",
                &format!("取时间戳失败：{e}"),
                Some("这份库连 strftime 都用不了，多半不是本工具的库"),
            )
        })
}

/// 导出一次备份包：快照 → 剥索引 → VACUUM → **先关快照连接** → 读字节 → 打 zip。
///
/// 收 `&Connection` 而不开自己的连接：谁持有锁由调用方决定（spec D12）—— 普通导出用
/// `data_dir` 现开的第二条连接（不占 `AppState.conn`，免得 VACUUM 期间界面每个写命令都在锁上排队），
/// 导入前的应急包则用 `AppState.conn` 本身（它要抓「即将被覆写的那份库」的此刻状态）。
///
/// 快照那份文件**必须走 `rusqlite::Connection::open`，不能走 `db::open`**：后者会把快照也
/// 切进 WAL 并跑 `migrate`，于是包里的 `schema_version` 会被本机版本悄悄改写 —— manifest 那句
/// 「比本机新就拒」就失去意义（spec §六.4）。
///
/// 本模块**唯一**还留着的 `dead_code` 豁免挂在这里，归属是 T7：`mod backup` 要到三条 IPC 命令
/// 进 `generate_handler!` 才从 crate 根可达，在那之前 `export` 就是整棵树唯一的外部入口。它下面
/// 那 11 件（两个条目名常量、`FORMAT_VERSION`、`warn_plaintext_columns`、`FileEntry`/`Counts`/
/// `Manifest`/`BackupSummary`、`sha256_hex`/`count_one`/`read_counts`/`now_utc`）已被本函数接上，
/// 而 `dead_code` 不沿调用链往下传（实测：只留这一条时 `cargo clippy --lib -- -D warnings` 仍 exit=0），
/// 所以 T3 那 11 条逐件豁免可以删净，不必再抄一遍。
#[allow(dead_code)] // T7 挂 IPC 后删
pub fn export(conn: &rusqlite::Connection, dst: &Path) -> AppResult<BackupSummary> {
    let scratch = crate::scratch::Scratch::new("pfm-m8", "export")?;
    let snap = scratch.dir().join(DB_ENTRY);
    conn.backup(rusqlite::MAIN_DB, &snap, None).map_err(|e| {
        AppError::new(
            "backup_snapshot_failed",
            &format!("建运行时快照失败：{e}"),
            Some("确认临时目录可写、库未损坏（必要时从备份恢复）；网络盘上跑不动就换本地盘"),
        )
    })?;

    // 剥索引 + VACUUM + 现数 counts 全在这条短连接里做完，然后 drop ——
    // 最后关闭时 SQLite 才把 WAL 收干净（`db.rs:479` 注释记的就是这条机制），
    // 不关就打包会拿到一个引用不存在 -wal 的 db（技术方案 line 175 明令避免的那件事）。
    let (db_bytes, counts, schema_version, created_at) = {
        // 快照走 `rusqlite::Connection::open`，**不走 `db::open`**（后者会切 WAL 并跑 migrate）。
        let s = rusqlite::Connection::open(&snap).map_err(|e| {
            AppError::new(
                "backup_snapshot_failed",
                &format!("打开快照 {} 失败：{e}", snap.display()),
                Some("确认临时目录可写、库未损坏（必要时从备份恢复）；网络盘上跑不动就换本地盘"),
            )
        })?;
        strip_index(&s)?;
        s.execute_batch("VACUUM").map_err(|e| {
            AppError::new(
                "backup_vacuum_failed",
                &format!("VACUUM 快照失败：{e}"),
                Some("临时目录所在盘满了？包体积≈台账数据量，索引已被剥掉"),
            )
        })?;
        let counts = read_counts(&s)?;
        let schema_version = schema_version_of(&s)?;
        let created_at = now_utc(&s)?;
        // 必须在这里 drop：最后一个连接关闭时 SQLite 才把 WAL 收干净（`db.rs:479` 注释记的机制），
        // 不关就 `fs::read` 打包会拿到一个引用不存在 -wal 的 db（技术方案 line 175 明令避免的事）。
        drop(s);
        (std::fs::read(&snap).map_err(|e| AppError::io(&snap, &e))?, counts, schema_version, created_at)
    };

    let sha = sha256_hex(&db_bytes);
    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        schema_version,
        created_at,
        index_excluded: true,
        counts,
        files: vec![FileEntry { name: DB_ENTRY.to_owned(), size: db_bytes.len() as u64, sha256: sha }],
        plaintext_warning: warn_plaintext_columns().to_owned(),
    };
    zip_out(dst, &manifest, &db_bytes)?;

    Ok(BackupSummary {
        path: dst.display().to_string(),
        entries: vec![MANIFEST_ENTRY.to_owned(), DB_ENTRY.to_owned()],
        bytes: db_bytes.len() as u64,
        counts,
        schema_version,
    })
}

/// 剥索引 = 删两张表的**行**，表结构照留（spec D4）。FTS 侧先删再删主表，顺序反了也不会漏行
/// （两张表之间没有触发器，映射由 `index_store` 手动维持 —— 这里要的只是「两张都空」）。
fn strip_index(conn: &rusqlite::Connection) -> AppResult<()> {
    conn.execute_batch("DELETE FROM index_docs_fts; DELETE FROM index_docs;")
        .map_err(|e| {
            AppError::new(
                "backup_strip_failed",
                &format!("剥索引失败：{e}"),
                Some("这份库的索引表结构与预期不符；先别导包，确认库没损坏"),
            )
        })
}

/// `schema_meta` 里的当前版本。注意**不碰 `db::TARGET_VERSION`**：包里的库是它自己那一刻的版本，
/// 与本机目标版本是两件事（T5 的拒包判据比的是这两个）。
fn schema_version_of(conn: &rusqlite::Connection) -> AppResult<i64> {
    conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_meta", [], |r| r.get(0))
        .map_err(|e| {
            AppError::new(
                "backup_schema_unreadable",
                &format!("读库版本失败：{e}"),
                Some("这份库没有 schema_meta，多半不是本工具建出来的"),
            )
        })
}

/// 写 zip：两个条目，manifest 在前、db 在后。
/// **`add_data` 在本仓的 zip 8.6.0 里不存在**（实读 `src/write.rs`：只有 `start_file` +
/// `impl Write for ZipWriter`），所以字节走 `std::io::Write::write_all`。
fn zip_out(dst: &Path, manifest: &Manifest, db_bytes: &[u8]) -> AppResult<()> {
    use std::io::Write as _;
    let json = serde_json::to_vec_pretty(manifest).map_err(|e| {
        AppError::new("backup_manifest_failed", &format!("序列化 manifest 失败：{e}"), None)
    })?;
    let file = std::fs::File::create(dst).map_err(|e| AppError::io(dst, &e))?;
    let mut z = zip::ZipWriter::new(file);
    // options 按值移动（实读 `write.rs:1501`）⇒ 两个条目各现造一份，别指望它是 Copy。
    // 变体名是 `Deflated`（实读 `compression.rs:22`），不是 `Deflate`。
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    z.start_file(MANIFEST_ENTRY, opts).map_err(|e| zip_fail(dst, e))?;
    z.write_all(&json).map_err(|e| io_fail(dst, &e))?;
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    z.start_file(DB_ENTRY, opts).map_err(|e| zip_fail(dst, e))?;
    z.write_all(db_bytes).map_err(|e| io_fail(dst, &e))?;
    z.finish().map_err(|e| zip_fail(dst, e))?;
    Ok(())
}

fn zip_fail(dst: &Path, e: zip::result::ZipError) -> AppError {
    AppError::new(
        "backup_zip_failed",
        &format!("写 {} 失败：{e}", dst.display()),
        Some("目标位置可写吗？U 盘/网盘同步中的目录常拒绝；文件名里别带冒号"),
    )
}

fn io_fail(dst: &Path, e: &std::io::Error) -> AppError {
    AppError::io(dst, e)
}

#[cfg(test)]
impl Counts {
    /// 只给测试用（生产路径的 counts 必须来自 `read_counts`）。
    /// 私有 `fn` 就够：`impl` 挂在父模块，`mod tests` 经 `use super::*` 本就能看见私有项。
    /// 唯一的真实约束是非 test 配置下**绝不**给 `Counts` derive `Default` ——
    /// 能默认出 0 的计数结构会被误用成「0 也算数」。
    fn default_for_test() -> Counts {
        Counts {
            projects: 0,
            ledger_envs: 0,
            ledger_credentials: 0,
            ledger_servers: 0,
            ledger_links: 0,
            ledger_notes: 0,
            settings: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// manifest 出参的键集合是**本轮新增线格式**，本仓形制是把它钉死（`SearchBundle` 七键、
    /// `DocPreview` 六键两条断言就是先例）。少一个键 = 前端读到 undefined，多一个键 = 白送。
    #[test]
    fn manifest_serializes_to_the_documented_camel_case_keys() {
        let m = Manifest {
            format_version: FORMAT_VERSION,
            app_version: "0.1.0".to_owned(),
            schema_version: 4,
            created_at: "2026-10-13T21:30:12Z".to_owned(),
            index_excluded: true,
            counts: Counts::default_for_test(),
            files: vec![FileEntry {
                name: DB_ENTRY.to_owned(),
                size: 4096,
                sha256: "ab".repeat(32),
            }],
            plaintext_warning: warn_plaintext_columns().to_owned(),
        };
        let obj: serde_json::Value = serde_json::to_value(&m).unwrap();
        let keys: Vec<&str> = obj.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        for key in [
            "formatVersion", "appVersion", "schemaVersion", "createdAt",
            "indexExcluded", "counts", "files", "plaintextWarning",
        ] {
            assert!(obj.get(key).is_some(), "缺 {key}：{keys:?}");
        }
        assert_eq!(keys.len(), 8, "manifest 的键集合就是 spec §五 那八个，多一个都要红");
        let c = obj["counts"].as_object().unwrap();
        assert_eq!(
            c.keys().collect::<Vec<_>>().len(),
            7,
            "counts 七张表：{:?}", c.keys().collect::<Vec<_>>()
        );
        assert!(c.contains_key("ledgerCredentials"), "counts 必须出 camelCase：{:?}", c.keys().collect::<Vec<_>>());
    }

    /// 计数从库里现数，且必须与 V2/V3 建的表**逐张对上**。
    /// 刻意用内存库 + 手写 INSERT：`counts` 一旦漏一张表（以后加表时很容易），
    /// 这条断言不会红 —— 所以这里钉的是「数出来的值等于种进去的值」，不是「非零」。
    #[test]
    fn read_counts_reports_what_is_actually_in_the_db() {
        let conn = crate::db::open_in_memory().unwrap();
        // 控制方裁定：index_job::tests::seed_project 是私有 fn，跨模块拿不到，
        // 按简报注 2 手写 INSERT（不为测试提可见性、不造新缝）。
        conn.execute(
            "INSERT INTO projects (id, name, status) VALUES ('p1','甲项目','立项')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO ledger_envs(id, project_id, env, name) VALUES('e1','p1','dev','开发')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO ledger_credentials(id, project_id, title) VALUES('c1','p1','入口')",
            [],
        ).unwrap();
        let c = read_counts(&conn).unwrap();
        assert_eq!(c.projects, 1, "项目数：{c:?}");
        assert_eq!(c.ledger_envs, 1, "环境数：{c:?}");
        assert_eq!(c.ledger_credentials, 1, "凭据数：{c:?}");
        assert_eq!(c.ledger_servers, 0);
        assert!(c.settings >= 3, "V4 种了三条 index_* 设置：{c:?}");
    }

    /// 已知向量钉 sha256_hex：不能只断「长度 64」。向量是公开的 FIPS 180-4 值。
    #[test]
    fn sha256_hex_matches_the_public_vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = sha256_hex(&"合同验收哨兵串".to_string().into_bytes());
        assert_eq!(long.len(), 64, "中文进哈希也要出 64 个十六进制字符");
        assert!(long.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    /// `now_utc` 走 SQLite 的 strftime，不引 chrono（零新依赖红线）。断形状不断值。
    #[test]
    fn now_utc_is_an_sqlite_produced_iso_stamp() {
        let conn = crate::db::open_in_memory().unwrap();
        let s = now_utc(&conn).unwrap();
        assert_eq!(s.len(), 20, "YYYY-MM-DDTHH:MM:SSZ：{s}");
        assert!(s.ends_with('Z') && s.chars().nth(10) == Some('T'), "形状不对：{s}");
    }

    // ---- T4：export（快照 → 剥两张索引表 → VACUUM → 先关连接再打 zip）----

    /// 测试辅助件：手写 INSERT 种 1 行 `projects` + n 行 `ledger_credentials`。
    /// 不依赖任何别的模块的私有 seed（`index_job::tests::seed_project` 之类跨不过去）。
    fn seed_ledger(conn: &rusqlite::Connection, n: i64) {
        conn.execute(
            "INSERT INTO projects (id, name, status) VALUES ('p1','甲项目','立项')",
            [],
        )
        .unwrap();
        for i in 1..=n {
            conn.execute(
                "INSERT INTO ledger_credentials (id, project_id, title) VALUES (?1, 'p1', ?2)",
                rusqlite::params![format!("c{i}"), format!("凭据{i}")],
            )
            .unwrap();
        }
    }

    /// 测试辅助件：往 `index_docs` 与 `index_docs_fts` **各**写 n 行，正文塞 ~4 KB 文本。
    /// FTS 侧必须真写 —— 否则第一条测试的 `fts == 0` 是空的对空的、恒真。
    fn seed_index(conn: &rusqlite::Connection, n: i64) {
        // 18 字 × 3 字节 × 100 ≈ 5.4 KB/行：40 行足够双臂对照分出差。
        let body = "合同验收哨兵正文段，用于把快照撑大。".repeat(100);
        for i in 1..=n {
            conn.execute(
                "INSERT INTO index_docs (id, project_id, path, ext, size, index_status)
                 VALUES (?1, 'p1', ?2, 'txt', 0, 'ok')",
                rusqlite::params![format!("d{i}"), format!("/srv/docs/{i}.txt")],
            )
            .unwrap();
            let rowid: i64 = conn
                .query_row(
                    "SELECT doc_rowid FROM index_docs WHERE id = ?1",
                    rusqlite::params![format!("d{i}")],
                    |r| r.get(0),
                )
                .unwrap();
            conn.execute(
                "INSERT INTO index_docs_fts (rowid, name_tokens, body_tokens) VALUES (?1, ?2, ?3)",
                rusqlite::params![rowid, format!("报告{i}"), body],
            )
            .unwrap();
        }
    }

    /// 测试辅助件：从包里解出 `backup.db` 到**新临时目录**并用普通 `Connection::open` 打开
    /// （快照是 delete 模式，普通 open 即可）。**必须把 TempDir 与 Connection 一起返回**：
    /// 临时目录若在本函数里被就地 drop，目录连带文件在断言前就没了（Windows 上还有句柄问题）。
    fn unpack_db(zip_path: &Path) -> (tempfile::TempDir, rusqlite::Connection) {
        use std::io::Read as _;
        let dir = tempfile::tempdir().unwrap();
        let file = std::fs::File::open(zip_path).unwrap();
        let mut z = zip::ZipArchive::new(file).unwrap();
        let mut entry = z.by_name(DB_ENTRY).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        let db_path = dir.path().join(DB_ENTRY);
        std::fs::write(&db_path, bytes).unwrap();
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        (dir, conn)
    }

    /// 测试辅助件：包内条目名清单。
    fn zip_names(path: &Path) -> Vec<String> {
        let file = std::fs::File::open(path).unwrap();
        let mut z = zip::ZipArchive::new(file).unwrap();
        (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().to_owned())
            .collect()
    }

    /// 测试辅助件：读回 (manifest, backup.db 原始字节)。哈希断言必须打在**原始字节**上。
    /// 两个 `by_name` 各自放在块里：`ZipFile` 借用了 `z`（且 Drop 时还要用它），
    /// 不先释放第一个条目就取第二个会撞 E0499。
    fn zip_read_all(path: &Path) -> (Manifest, Vec<u8>) {
        use std::io::Read as _;
        let file = std::fs::File::open(path).unwrap();
        let mut z = zip::ZipArchive::new(file).unwrap();
        let mut json = String::new();
        {
            let mut entry = z.by_name(MANIFEST_ENTRY).unwrap();
            entry.read_to_string(&mut json).unwrap();
        }
        let manifest: Manifest = serde_json::from_str(&json).unwrap();
        let mut db_bytes = Vec::new();
        {
            let mut entry = z.by_name(DB_ENTRY).unwrap();
            entry.read_to_end(&mut db_bytes).unwrap();
        }
        (manifest, db_bytes)
    }

    /// 主承诺的正面断言：**台账在、索引空**。只断「台账在」不算 —— 把索引一起打包也能让台账在（spec §七）。
    /// 夹具刻意做成「快照里索引行本来就非空」，否则空对空，`index_docs == 0` 恒真。
    #[test]
    fn export_keeps_the_ledger_and_drops_both_index_tables() {
        let live = crate::db::open_in_memory().unwrap();
        seed_ledger(&live, 2); // 手写 INSERT：projects + 2 条 ledger_credentials
        seed_index(&live, 3); // 往 index_docs 与 index_docs_fts 各写 3 行（证"真的删了"）
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("out.zip");

        let sum = export(&live, &dst).unwrap();

        // 包内只有两条目，名字是本模块的常量（不是文件名派生）。
        assert_eq!(sum.entries, vec![MANIFEST_ENTRY.to_owned(), DB_ENTRY.to_owned()], "{:?}", sum.entries);
        assert!(dst.exists());
        // 从包里解出 backup.db，开它，断索引两张表空 + 台账两行在。
        let (unpacked_dir, got) = unpack_db(&dst);
        let idx: i64 = got.query_row("SELECT COUNT(*) FROM index_docs", [], |r| r.get(0)).unwrap();
        let fts: i64 = got.query_row("SELECT COUNT(*) FROM index_docs_fts", [], |r| r.get(0)).unwrap();
        assert_eq!(idx, 0, "索引主表必须空（这是 D4 的全部兑现）");
        assert_eq!(fts, 0, "FTS 侧必须空");
        assert_eq!(got.query_row::<i64,_,_>("SELECT COUNT(*) FROM ledger_credentials", [], |r| r.get(0)).unwrap(), 2);
        // 表结构照留：DROP 掉会让导入端 schema 与 TARGET_VERSION 对不上（spec D4 第③条）。
        assert!(got.query_row::<i64,_,_>("SELECT COUNT(*) FROM sqlite_master WHERE name='index_docs'", [], |r| r.get(0)).unwrap() > 0,
            "结构必须还在，只删行");
        drop(got); // 先关连接再让临时目录出手（Windows 句柄）
        drop(unpacked_dir);
    }

    /// 双臂对照：同一份夹具、同样 `conn.backup` 到两个临时文件，一臂剥索引 + VACUUM、一臂原样。
    /// **不**拿活库 `ledger.db` 的文件大小作对照（WAL 下最近改动都在 `-wal` 里，主库文件可以小到
    /// 没有比较意义，见 spec §八）。两臂之差还要 > 一个下限，否则夹具规模不足要重造。
    #[test]
    fn stripping_the_index_actually_shrinks_the_snapshot() {
        let live = crate::db::open_in_memory().unwrap();
        seed_ledger(&live, 1);
        seed_index(&live, 40); // 40 行、每行塞 ~4 KB 正文，凑出可分辨的差
        let dir = tempfile::tempdir().unwrap();
        let stripped = dir.path().join("stripped.db");
        let raw = dir.path().join("raw.db");

        live.backup(rusqlite::MAIN_DB, &stripped, None).unwrap();
        live.backup(rusqlite::MAIN_DB, &raw, None).unwrap();
        let s = rusqlite::Connection::open(&stripped).unwrap();
        strip_index(&s).unwrap();
        s.execute_batch("VACUUM").unwrap();
        let stripped_bytes = stripped.metadata().unwrap().len();
        let raw_bytes = raw.metadata().unwrap().len();
        assert!(stripped_bytes < raw_bytes, "剥过要小于原样：{stripped_bytes} vs {raw_bytes}");
        assert!(raw_bytes.saturating_sub(stripped_bytes) > 8 * 1024,
            "差值下限 8 KB，夹具分不出差就重造：{stripped_bytes} vs {raw_bytes}");
    }

    /// spec §七 那条判据：快照连接必须**先关**再把字节读进 zip。
    /// 这里钉的是结果形状：包里的 `backup.db` 单独打开就 `integrity_check` = ok，
    /// 且临时目录里没有 `-wal` 被一起打进去（条目数恒等于 2 已经覆盖了这一半）。
    #[test]
    fn packaged_db_is_self_contained_and_checks_clean() {
        let live = crate::db::open_in_memory().unwrap();
        seed_ledger(&live, 1);
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("x.zip");
        export(&live, &dst).unwrap();
        let names = zip_names(&dst);
        assert_eq!(names.len(), 2, "包里只该两条目：{names:?}");
        assert!(names.iter().all(|n| n != "backup.db-wal" && n != "backup.db-shm"),
            "WAL 侧文件绝不能进包（那会得到一个引用不存在 -wal 的 db）：{names:?}");
        let (unpacked_dir, got) = unpack_db(&dst);
        let ok: String = got.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
        assert_eq!(ok, "ok");
        drop(got);
        drop(unpacked_dir);
    }

    /// manifest 与包内字节同源：哈希对得上、counts 等于包内库现数、schema 版本落在 manifest 上。
    #[test]
    fn manifest_hash_and_counts_match_the_bytes_it_ships() {
        let live = crate::db::open_in_memory().unwrap();
        seed_ledger(&live, 3);
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("y.zip");
        let sum = export(&live, &dst).unwrap();
        let (manifest, db_bytes) = zip_read_all(&dst);
        assert_eq!(manifest.files.len(), 1);
        assert_eq!(manifest.files[0].name, DB_ENTRY);
        assert_eq!(manifest.files[0].size as usize, db_bytes.len());
        assert_eq!(manifest.files[0].sha256, sha256_hex(&db_bytes), "哈希必须对得上条目原始字节");
        assert_eq!(manifest.counts, sum.counts);
        assert!(manifest.index_excluded);
        assert_eq!(manifest.schema_version, sum.schema_version);
        assert_eq!(manifest.format_version, FORMAT_VERSION);
        assert_eq!(manifest.app_version, env!("CARGO_PKG_VERSION"));
    }
}
