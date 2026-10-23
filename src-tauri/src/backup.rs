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
    "包内非 *_cipher 列为明文：项目名/编号/负责人/客户/联系人/合同号（含项目模板的同类字段）、工作区名称、服务器 ip 与堡垒机、各类 url、各表 note、待办内容、里程碑备注、日报内容、到期事项名称与备注、项目修改日志的字段旧值/新值（含联系人、联系方式）。"
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

/// 覆盖前「看得见包里是什么」的兑现物（D5 花第三条命令买的就是这一格）。
/// 刻意**不**带 sha256 字符串：界面只念「校验通过」，念一串十六进制没有决策价值（红线④）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInfo {
    pub path: String,
    pub created_at: String,
    pub app_version: String,
    pub schema_version: i64,
    pub counts: Counts,
    pub bytes: u64,
    pub plaintext_warning: String,
}

/// `import` 的出参。`counts` 是**覆写前**那份现状的计数（等于应急包里念出来的数字），不是导入后的 ——
/// 导入后的数字由前端重新拉一次 `index_overview`/`project_list` 得到，这里不做第二份真值。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub schema_version: i64,
    pub pre_restore_path: String,
    pub counts: Counts,
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

/// 挡住「把导出落点指进应用自己的数据目录」这类自毁操作。
///
/// 判据不是「只有 `ledger.db` 危险」，而是**落在数据目录里的任何位置都拒**：那目录里除活库外还有
/// `pre-restore-*.zip`（那是某次导入的撤销键，被覆掉等于毁掉别人的回滚），而且用户丢进去的包与应用
/// 自己写出的包在形状上无法区分。用户面入口（`backup_export`）才守这一道 —— `import` 内部写应急包
/// 到数据目录是合法路径（见 `import`，走的是同一条 `export`），不能套本守卫，否则撤销键就写不出去。
///
/// `zip_out` 用的是 `std::fs::File::create`（CREATE_ALWAYS + 截断）：Windows 共享规则**保护不了**
/// 正被 SQLite 开着的 `ledger.db`，实测把落点指向活库会让文件长度当场归零、随即被 zip 覆掉，
/// 用户唯一的持久库就此没了。所以必须在真正打开文件之前把这种落点拒掉。
pub fn guard_destination(data_dir: &Path, dst: &Path) -> AppResult<()> {
    // 相对形状先按**当前目录**展开：Windows 下 `File::create` 也是相对 CWD 解析的，不展开就无从判断
    // 一条相对落点到底落不落进数据目录（留相对串直接做裸前缀比对的话，它永远"不像"在目录里 ⇒ 漏拒）。
    let abs = if dst.is_relative() {
        std::env::current_dir()
            .map(|cwd| cwd.join(dst))
            .unwrap_or_else(|_| dst.to_path_buf())
    } else {
        dst.to_path_buf()
    };
    // 规范化的是**父目录**而不是文件本身：一次全新导出时目标文件还不存在，canonicalize(文件) 会 ENOENT；
    // 父目录存在时规范化又顺带吃掉 `..\`/`.` 回溯段，让「指向数据目录内的相对/回溯形状」与绝对路径同判据。
    let parent = abs.parent().unwrap_or(&abs);
    let canonical_parent = std::fs::canonicalize(parent).ok();
    let canonical_dir = std::fs::canonicalize(data_dir).ok();
    // 三档，每档只在自己那档里可比（两种形状 `C:\…` 与 `\\?\C:\…` 按组件比对**永不命中**，混档比=假阴性）：
    // ① 父子都能规范化 ⇒ 规范化 vs 规范化：挡住 `..\` 回溯、junction 指回来的形状。
    // ② 父能规范化、数据目录不能（还没建出来）⇒ 退回裸串 vs 裸串。
    // ③ 父规范化不了（那条路径本身不存在）⇒ 只能裸串 vs 裸串。
    // 为什么不能把裸串比对无条件并列进去：`data\sub\..\..\x.zip` 折叠后其实**在目录外**，裸串前缀却看得见
    // `data\` —— 无脑并列会把这种合法落点误拒（第 (e) 档就是钉这一条的，控制方第一版就是这么写、被这条
    // 测试当场打回来的）。
    // **残余窗口要如实写**：②③ 两档里如果父子其中一边规范化不了、而另一边的形状又不是同一根串，判据会
    // 落到「放行」。真机上那两档同时意味着目标不可创建（父目录不存在 / 数据目录不存在）⇒ 不是可利用的
    // 毁库路径；但**别把这一条读成已经全档兜住**（`\\?\` 前缀种类不同的那一档只能退回放行）。
    let inside = match (&canonical_parent, abs.file_name()) {
        (Some(p), Some(name)) => match &canonical_dir {
            Some(d) => p.join(name).starts_with(d),
            None => abs.starts_with(data_dir),
        },
        (Some(p), None) => match &canonical_dir {
            Some(d) => p.starts_with(d),
            None => abs.starts_with(data_dir),
        },
        (None, _) => abs.starts_with(data_dir),
    };
    if inside {
        return Err(AppError::new(
            "backup_destination_unsafe",
            &format!("导出目标 {} 落在应用的数据目录里，会被拒掉以免覆掉活库或应急包", dst.display()),
            Some("请把导出文件保存到数据目录以外的位置（数据目录里是活库和应急包，覆盖会毁掉它们）"),
        ));
    }
    Ok(())
}

/// 导出一次备份包：快照 → 剥索引 → VACUUM → **先关快照连接** → 读字节 → 打 zip。
///
/// 收 `&Connection` 而不开自己的连接：谁持有锁由调用方决定（spec D12）—— 普通导出用
/// `data_dir` 现开的第二条连接（不占 `AppState.conn`，免得整条快照/剥索引/写 zip 的期间让界面上每个读写
/// 命令在那把 std Mutex 上无声排队），导入前的应急包则用 `AppState.conn` 本身（它要抓「即将被覆写
/// 的那份库」的此刻状态）。占锁真正久的是 **import** 那一长串（读包 + 全库哈希 + 应急包导出 + restore），
/// 不是 `VACUUM` —— `VACUUM` 只在一次性快照连接上跑（见下方块内），从不碰调用方连接。
///
/// 快照那份文件**必须走 `rusqlite::Connection::open`，不能走 `db::open`**：后者会把快照也
/// 切进 WAL 并跑 `migrate`，于是包里的 `schema_version` 会被本机版本悄悄改写 —— manifest 那句
/// 「比本机新就拒」就失去意义（spec §六.4）。
///
/// `dead_code` 的机制结论（T3–T7 实测出来的，留给下一个读者）：它**不沿调用链下传** ⇒ 未接线的
/// 入口件才是豁免的粒度，一条入口件 = 一条豁免，而不是它调用的每件各一条。它下面那 11 件
/// （两个条目名常量、`FORMAT_VERSION`、`warn_plaintext_columns`、`FileEntry`/`Counts`/`Manifest`/
/// `BackupSummary`、`sha256_hex`/`count_one`/`read_counts`/`now_utc`）在 `export` 接线后一条豁免都不
/// 需要（T3 期间逐件挂的那 11 条即因此删净，实测：只留入口件豁免时 `cargo clippy --lib -- -D warnings`
/// 仍 exit=0）。所以中间态只给未接线的**入口件**挂豁免就够。
/// 本模块那三条（这条 + T5 的 `inspect` + T6 的 `import`）已随 T7 把三条 IPC 挂上 `generate_handler!`
/// （`lib.rs:751-753`）删净；全仓现在只剩 `db.rs:30` 那一条既有豁免（结构门读数：`dead_code` 豁免 = 1）。
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
    // 最后关闭时 SQLite 才把 WAL 收干净（`db.rs` 的 `side_file_unopenable_pins_the_readback_invariant`
    // 注释记的就是这条机制；本仓行号会漂，故按名引、不按号引），
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
        // 必须在这里 drop：最后一个连接关闭时 SQLite 才把 WAL 收干净（`db.rs` 的
        // `side_file_unopenable_pins_the_readback_invariant` 注释记的机制；行号会漂故按名引），
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

/// 读**这条连接所指的那个库自己**的 `schema_meta` 现值（`MAX(version)`），不读任何本机常量。
/// 本机上限是 `db::TARGET_VERSION`，两者是两件事：拒包的判据（T5）拿「包里这个数」去比「本机上限」，
/// 而本函数只负责交出前一个数 —— 传进来的是活连接还是快照，就读出谁的版本，跟上限无关。
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

/// 解包 → 读 manifest → 比 `formatVersion` → 比 `schemaVersion` → **比对包内条目集合** → 验 db 大小与哈希。
/// **顺序是判据的一部分**（见任务注）：版本判据必须先于哈希判据 —— 对同时违反版本与哈希的包，
/// 必须拒在版本上（`inspect_refuses_a_package_from_a_newer_schema` 的双违反臂直钉这条顺序）；
/// 顺序反过来会把错因推给 mismatch，版本拦截失去出手机会，而「只改 manifest」的包（db 原样、
/// 哈希仍相符）两种顺序都能走到版本判据，单独用它测不出顺序。
/// 条目集合那一道（M8.1 ④b）排在版本判据之后、大小/哈希之前：它同样是「结构不对就先报结构」，
/// 且必须早于 `import` 里的应急包写入 —— 被拒的包一条文件都不许多写。
/// T6 的 import 也用它 —— 验包逻辑一份，不抄两遍。
fn read_package(src: &Path) -> AppResult<(Manifest, Vec<u8>)> {
    let file = std::fs::File::open(src).map_err(|e| AppError::io(src, &e))?;
    let mut z = zip::ZipArchive::new(file).map_err(|e| {
        AppError::new(
            "backup_package_invalid",
            &format!("解不开 {}：{e}", src.display()),
            Some("它不是备份包，或者下载/拷贝时被截断了"),
        )
    })?;
    let json = zip_read_entry(&mut z, MANIFEST_ENTRY)?;
    let manifest: Manifest = serde_json::from_slice(&json).map_err(|e| {
        AppError::new(
            "backup_package_invalid",
            &format!("读 {} 失败：{e}", MANIFEST_ENTRY),
            Some("这份包的 manifest 坏了，不能用它导入"),
        )
    })?;
    if manifest.format_version != FORMAT_VERSION {
        return Err(AppError::new(
            "backup_package_invalid",
            &format!("这份包的格式版本是 {}，本版本只读得动 {}", manifest.format_version, FORMAT_VERSION),
            Some("用导出它的那个版本打开，或在新版本应用里导入"),
        ));
    }
    if manifest.schema_version > crate::db::TARGET_VERSION {
        return Err(AppError::new(
            "backup_schema_too_new",
            &format!("包里的库是 schema v{}，本机只认到 v{}", manifest.schema_version, crate::db::TARGET_VERSION),
            Some("先升级应用再导入；别拿旧版本覆盖新版本的库"),
        ));
    }
    // M8.1 ④b（需求方当次批准，spec §五 的判据随之从「静默忽略」改成「硬拒」）：
    // 包内条目集合必须**恰好等于** manifest 点名的集合（`manifest.json` 自己也算一条）。
    // 原先只做到「按点名的取，其它一律忽略」——那确实让野条目落不了盘（T9 实测钉的就是那一句），
    // 但它同时会**静默接受一份被人改过的包**；响亮地拒才对得起 D6 那句「包可能被改过」。
    // 顺序上放在版本判据之后、大小/哈希之前：与 T5 定下的「同时违反多道的包必须有确定的那一条错误」一致，
    // 版本不符的包仍然先报版本，不会因为这一步变成结构错。
    let mut got_names: Vec<String> = z.file_names().map(str::to_owned).collect();
    let mut want_names: Vec<String> = manifest.files.iter().map(|f| f.name.clone()).collect();
    want_names.push(MANIFEST_ENTRY.to_owned());
    got_names.sort();
    want_names.sort();
    if got_names != want_names {
        return Err(AppError::new(
            "backup_package_invalid",
            &format!(
                "{} 里的条目与 manifest 点名的对不上（包内 {} 条，manifest 点名 {} 条）",
                src.display(),
                got_names.len(),
                want_names.len()
            ),
            Some("这份包被改过，或者不是本工具导出的；别用它导入。只想看看里面有什么就用「检查备份包」"),
        ));
    }
    // 只按 manifest 点名的那一个条目取（现在上一句已经保证没有别的条目了）⇒ zip-slip 的两道防线（spec §五）。
    let want = manifest
        .files
        .iter()
        .find(|f| f.name == DB_ENTRY)
        .ok_or_else(|| AppError::new(
            "backup_package_invalid",
            &format!("manifest 里没有 {DB_ENTRY} 这个条目"),
            Some("这份包不完整，重新导出一次"),
        ))?;
    let db_bytes = zip_read_entry(&mut z, DB_ENTRY)?;
    if db_bytes.len() as u64 != want.size {
        return Err(AppError::new(
            "backup_hash_mismatch",
            &format!("{DB_ENTRY} 大小不符（manifest 说 {}，实得 {}）", want.size, db_bytes.len()),
            Some("包已损坏"),
        ));
    }
    let got = sha256_hex(&db_bytes);
    if got != want.sha256 {
        return Err(AppError::new(
            "backup_hash_mismatch",
            &format!("{DB_ENTRY} 的 SHA-256 与 manifest 不符（本机算出 {got}）"),
            Some("包已损坏或被改过；别用它导入。若这是你刚导出的包，重导一次"),
        ));
    }
    Ok((manifest, db_bytes))
}

/// 只读校验：不碰数据库、不落任何盘（临时文件只在 import 那条路上写，spec D3）。
/// （曾挂在这条上的 `dead_code` 豁免随 T7 接上 IPC 已删，机制见 `export` 的 doc。）
pub fn inspect(src: &Path) -> AppResult<PackageInfo> {
    let (m, db_bytes) = read_package(src)?;
    Ok(PackageInfo {
        path: src.display().to_string(),
        created_at: m.created_at,
        app_version: m.app_version,
        schema_version: m.schema_version,
        counts: m.counts,
        bytes: db_bytes.len() as u64,
        plaintext_warning: m.plaintext_warning,
    })
}

/// 按条目名取字节。包内名字**永远是本模块写死的两个常量之一**，绝不拿它拼路径。
fn zip_read_entry(z: &mut zip::ZipArchive<std::fs::File>, name: &str) -> AppResult<Vec<u8>> {
    use std::io::Read as _;
    let mut entry = z.by_name(name).map_err(|e| {
        AppError::new(
            "backup_package_invalid",
            &format!("包里没有 {name}：{e}"),
            Some("这份包不完整，重新导出一次"),
        )
    })?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).map_err(|e| AppError::io(Path::new(name), &e))?;
    Ok(buf)
}

/// 应急包的文件名。**冒号换成 `-`**：`:` 在 Windows 文件名里非法（M7 的 profile URL `%20` 是同一类
/// 「路径形状要到本机量」的教训，spec §三.17）。
///
/// 除 `stamp` 外还要带一个**进程内唯一后缀**（T7 Step 0，控制方把 T6 评审 Minor 升级为必修）：
/// `stamp` 只有秒级粒度，同秒连导两次时第二次会把第一次的撤销键 `File::create` 静默 truncate 掉 ——
/// 那正好破掉 D1「覆盖永远可撤销」。唯一后缀复用 `Scratch` 那套的**三段**写法（`std::process::id()`
/// + 进程内自增 seq + epoch 纳秒，见 `scratch.rs:20-28`），不新造随机源。
pub fn pre_restore_name(stamp: &str, unique: &str) -> String {
    format!("pre-restore-{stamp}-{unique}.zip")
}

/// 本次覆写的时刻（不是包的导出时刻！manifest 的 `created_at` 走的是 `now_utc`，那个带冒号的形态
/// 进得了 manifest、进不了文件名），形态已按 `pre_restore_name` 的要求去掉冒号（注 1）。
fn stamp(conn: &rusqlite::Connection) -> AppResult<String> {
    conn.query_row("SELECT strftime('%Y-%m-%dT%H-%M-%SZ','now')", [], |r| r.get(0))
        .map_err(|e| {
            AppError::new(
                "backup_timestamp_failed",
                &format!("取时间戳失败：{e}"),
                Some("这份库连 strftime 都用不了，多半不是本工具的库"),
            )
        })
}

/// 把包里的库灌回**当前已打开**的连接。顺序即判据（spec D13 + §三）：
/// ① 解包读 manifest ②比格式版本 ③比 schema 版本 ④验 db 哈希（①–④全在 `read_package` 里，
/// 一道不过就返回，**库未动**、也不写应急包）→ ⑤写应急包到数据目录（写不出去就中止，仍在覆写之前）→
/// ⑥`restore` 灌页 → ⑦`migrate` 补版本 → ⑧`assert_wal` 现读回。
///
/// ⑥ 之后没有任何回滚：`Connection::restore` 按页拷贝、rusqlite 对 Busy 只重试 3 次 ×100 ms
/// （实读 vendored `rusqlite-0.40.2/src/backup.rs:102-134`），真失败就是半新半旧 —— 所以 ⑤ 那份
/// 应急包是唯一的撤销键，⑥⑦⑧ 每条错误的 hint 都必须把它的**完整路径**念出来（spec §三 表）。
/// 本机无法廉价构造「灌到一半」的现场，那一档登记为有名缺口（spec §九），这里只保证失败点前移。
///
/// （曾挂在这条上的 `dead_code` 豁免随 T7 接上 IPC 已删；被本函数使用的 `stamp`/`pre_restore_name`
/// 本来就不需要各自挂豁免，理由见 `export` 的 doc 里那条「`dead_code` 不沿调用链下传」。）
pub fn import(
    conn: &mut rusqlite::Connection,
    src: &Path,
    data_dir: &Path,
) -> AppResult<ImportOutcome> {
    // ①–④：与 inspect 同一份逻辑，不抄两遍。
    let (manifest, db_bytes) = read_package(src)?;
    // ⑤ 应急包：用**即将被覆写的那条连接**导出（D12：它抓的必须正是这份状态 —— 覆写前的现数）。
    let when = stamp(conn)?;
    // 进程内唯一后缀：`stamp` 只到秒，同秒连导两次会把前一份撤销键静默 truncate（T7 Step 0 / D1）。
    // **三段**照 `scratch.rs:20-28`（pid + 进程内自增 seq + epoch 纳秒），不新造随机源。
    // seq 不是可选的装饰：`duration_since` 在系统时钟早于 epoch 时返 Err、纳秒只能退化成同一个常量，
    // 光靠 pid + 纳秒那两次就会拿到一模一样的串 —— 自增的 seq 才是「兜底也不可碰撞」那一段。
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let unique = format!("{}-{}-{}", std::process::id(), seq, nanos);
    let pre = data_dir.join(pre_restore_name(&when, &unique));
    let summary = export(conn, &pre)?;
    let counts_before = summary.counts;
    // 撤销键：⑥⑦⑧ 三支 hint 共用的那句，必须带应急包的完整路径（裁定 2）。
    let undo = format!("库里可能已是半新半旧的状态。用这份应急包再导入一次即可回到覆写前：{}", pre.display());

    // ⑥ 把包内 db 落进一次性目录，再 restore 进去（`restore` 只收路径，不收字节 —— 实读 backup.rs:102）。
    // `_scratch`：下划线前缀是必须的 —— 本函数只在末尾靠 Drop 清理，没有任何一处读它，
    // 绑成 `scratch` 会被 `unused_variables` 在 `-D warnings` 下打成红。
    let _scratch = crate::scratch::Scratch::new("pfm-m8", "import")?;
    let snap = _scratch.dir().join(DB_ENTRY);
    std::fs::write(&snap, &db_bytes).map_err(|e| AppError::io(&snap, &e))?;
    // 泛型 `F: Fn(Progress)` ⇒ `None` 必须显式标注类型（实读 backup.rs:102 签名）。
    conn.restore(rusqlite::MAIN_DB, &snap, None::<fn(rusqlite::backup::Progress)>)
        .map_err(|e| {
            AppError::new(
                "backup_restore_failed",
                &format!("灌入包内库失败：{e}"),
                Some(&undo),
            )
        })?;
    // 一次性目录由 `_scratch` 在函数末尾（含错误返回那条路）Drop 时整棵删掉，不在此处手写清理。

    // ⑦ 包里可能是旧版本 ⇒ 补迁移。
    crate::db::migrate(conn)
        .map_err(|e| {
            AppError::new(
                "backup_migrate_failed",
                &format!("导入后补迁移失败：{}", e.message),
                Some(&format!("这份包的库版本本机读得动但补不动迁移；用数据目录里的应急包导回去：{}", pre.display())),
            )
        })?;
    // ⑧ WAL 现读（`restore` 绕过 after_open，spec §七）。
    crate::db::assert_wal(conn)
        .map_err(|e| {
            AppError::new(
                "backup_wal_lost",
                &format!("导入后这份库不在 WAL 上了：{}", e.message),
                Some(&format!("这份库不再受断电保护。先用数据目录里的应急包导回去，并把现象反馈给维护者：{}", pre.display())),
            )
        })?;

    Ok(ImportOutcome {
        schema_version: manifest.schema_version,
        pre_restore_path: pre.display().to_string(),
        counts: counts_before,
    })
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

/// 只给测试用（T9 端到端）：把包内 `backup.db` 解到调用方给的一次性目录并开一条连接读它。
/// 挂在模块层而不是 `mod tests` 里，是为了让 `crate::backup::unpack_for_test` 这个路径可达
/// （`mod tests` 是私有模块，兄弟模块拿不到里面的东西，T6 注 2 撞过同一件事）。
///
/// **必须用 `rusqlite::Connection::open`，不能走 `db::open`**：后者会把解出来的库 WAL 化并跑
/// `migrate`，于是测的就不是「包里那份 db」而是本机现造的库了（spec §六.4 的判据会被悄悄改掉）。
/// 落点与生命周期归调用方（一次性 `tempfile::tempdir()`），本函数只返回连接。
#[cfg(test)]
pub(crate) fn unpack_for_test(zip_path: &Path, dir: &Path) -> rusqlite::Connection {
    use std::io::Read as _;
    let file = std::fs::File::open(zip_path).unwrap();
    let mut z = zip::ZipArchive::new(file).unwrap();
    let mut bytes = Vec::new();
    {
        let mut entry = z.by_name(DB_ENTRY).unwrap();
        entry.read_to_end(&mut bytes).unwrap();
    }
    let db_path = dir.join(DB_ENTRY);
    std::fs::write(&db_path, bytes).unwrap();
    rusqlite::Connection::open(&db_path).unwrap()
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

    /// 包里躺着什么明文，这句必须念全 —— M9 加了两张明文表、M10 加了日报、M11-a 加了到期事项。
    /// 这条是**文本门性质**的行为断言：判据本体（哪些列真明文）在 V7 的 DDL 与 `expiry::SELECT_COLUMNS` 里。
    #[test]
    fn plaintext_warning_names_every_plain_text_table_since_m9() {
        let warn = warn_plaintext_columns();
        assert!(warn.contains("待办内容"), "漏了待办的明文列：{warn}");
        assert!(warn.contains("里程碑备注"), "漏了里程碑的明文列：{warn}");
        // M15 的第四处明文命名：区名是按客户起的，「客户乙」这种就是业务命名。
        assert!(warn.contains("工作区名称"), "漏了工作区名称的明文：{warn}");
        // M10 的第三张：`daily_reports.content` 既随整库快照进包，又因 D4 进全局搜索结果页（M10 spec §九.2）。
        assert!(warn.contains("日报内容"), "漏了日报的明文列：{warn}");
        // M11-a 的第四张（spec §九.6）：到期事项的名称与备注都是明文，且**都进全局检索**（D6）。
        // 断言要断全短语（终审 N1）：只断「到期事项」时，把「名称与备注」删掉仍会绿。
        // M10 那次是这句话没被分配到任何一格任务，于是九次任务评审结构性看不见它 —— 本轮它是一格的必改文件。
        assert!(warn.contains("到期事项名称与备注"), "漏了到期事项的明文列：{warn}");
        assert!(warn.contains("项目模板"), "漏了项目模板的明文列：{warn}");
        assert!(warn.contains("项目修改日志"), "漏了项目修改日志的旧值/新值：{warn}");
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

    /// 测试辅助件：手写 INSERT 种 n 行 `projects`（id 落成 `p1..pN`）+ n 行 `ledger_credentials`。
    /// 不依赖任何别的模块的私有 seed（`index_job::tests::seed_project` 之类跨不过去）。
    /// T6 注 3：本格的导入测试用 `DELETE FROM projects WHERE id='p1'` 制造「回滚」差异，
    /// 项目 id 必须真落成 p1..pN 且行数等于 n —— 否则删掉 0 行、断言恒真。内部那条
    /// `assert_eq!(n, 实插行数)` 就是钉这件事。
    fn seed_ledger(conn: &rusqlite::Connection, n: i64) {
        for i in 1..=n {
            conn.execute(
                "INSERT INTO projects (id, name, status) VALUES (?1, ?2, '立项')",
                rusqlite::params![format!("p{i}"), format!("甲项目{i}")],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO ledger_credentials (id, project_id, title) VALUES (?1, 'p1', ?2)",
                rusqlite::params![format!("c{i}"), format!("凭据{i}")],
            )
            .unwrap();
        }
        let got: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(got, n, "seed_ledger 必须真落 n 行项目（id=p1..pN），否则 DELETE 制造的差异不存在：{got}");
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

    /// M12 格 11 的 cargo 半边：模板随整库快照进包，但 counts 仍是七张表。
    #[test]
    fn backup_package_carries_project_templates_without_counting_them() {
        let live = crate::db::open_in_memory().unwrap();
        live.execute(
            "INSERT INTO project_templates (id, name, status) VALUES ('tpl-1', '备份模板', '立项')",
            [],
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("templates.zip");
        let sum = export(&live, &dst).unwrap();
        assert_eq!(sum.schema_version, crate::db::TARGET_VERSION);
        assert_eq!(sum.schema_version, 11, "M15 起包内 schema 必须是 11");
        assert_eq!(sum.counts.projects, 0);
        let (_unpacked, got) = unpack_db(&dst);
        let n: i64 = got
            .query_row("SELECT COUNT(*) FROM project_templates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "模板行必须随整库快照进包");
        let name: String = got
            .query_row("SELECT name FROM project_templates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "备份模板");
    }

    /// M14 的备份半边：日志随整库快照进包，但 counts 仍是七张表。
    #[test]
    fn backup_package_carries_project_audit_logs_without_counting_them() {
        let live = crate::db::open_in_memory().unwrap();
        live.execute(
            "INSERT INTO project_audit_logs (id, project_id, project_name, action, changes)
             VALUES ('log-1', 'p1', '甲项目', 'update', '[]')",
            [],
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("audit.zip");
        let sum = export(&live, &dst).unwrap();
        assert_eq!(sum.schema_version, crate::db::TARGET_VERSION);
        assert_eq!(sum.schema_version, 11);
        assert_eq!(sum.counts.projects, 0);
        let (_unpacked, got) = unpack_db(&dst);
        let n: i64 = got
            .query_row("SELECT COUNT(*) FROM project_audit_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "日志行必须随整库快照进包");
        let action: String = got
            .query_row("SELECT action FROM project_audit_logs WHERE id = 'log-1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(action, "update");
    }

    /// M15 的备份半边：工作区行随整库快照进包，但 Counts 仍是七张表（沿用 M12/M13/M14 的裁定）。
    #[test]
    fn backup_package_carries_workspaces_without_counting_them() {
        let live = crate::db::open_in_memory().unwrap();
        live.execute("INSERT INTO workspaces (id, name) VALUES ('ws-2', '客户乙')", [])
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("workspaces.zip");
        let sum = export(&live, &dst).unwrap();
        assert_eq!(sum.schema_version, crate::db::TARGET_VERSION);
        assert_eq!(sum.schema_version, 11, "M15 起包内 schema 必须是 11");

        let (_unpacked, got) = unpack_db(&dst);
        let names: Vec<String> = got
            .prepare("SELECT name FROM workspaces ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(names, vec!["默认工作区", "客户乙"], "区行必须随整库快照进包：{names:?}");
        let keys = serde_json::to_value(&sum.counts).unwrap();
        assert_eq!(
            keys.as_object().unwrap().len(),
            7,
            "workspaces 是新表，不进 Counts：{keys:?}"
        );
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

    /// 钉的是**包的形状**：包里只有 `manifest.json` + `backup.db` 两条（`-wal`/`-shm` 一条都不许混进来），
    /// 且解出来的 `backup.db` 单独打开就 `integrity_check` = ok。
    ///
    /// **别把这条当 spec §七「先关快照连接再读字节」的守卫**（T9 评审把这句记账纠正过，同源订正留在这里）：
    /// 本夹具走 `db::open_in_memory()` ⇒ journal 是 MEMORY，全程没有 `-wal` 侧文件，把 `export` 里的
    /// `drop(s)` 挪到 `fs::read` 之后这三条断言**全都不会红**。§七 在本仓的可证伪载体是 T9 那条
    /// WAL 文件库端到端（`index_job.rs` 的往返测试，变异实测红在 `index_docs` 行数为 3 的陈旧影像上）。
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

    // ---- T5：inspect（只读验包：formatVersion → schemaVersion → 哈希，顺序即判据）----

    /// 测试辅助件：读出 src 的全部条目，只按 `keep` 列出的名字重写进 dst。
    /// 本格的 `keep = [DB_ENTRY]` 用来造「manifest 整个没了」的包。
    fn zip_write_entries(src: &Path, dst: &Path, keep: &[&str]) {
        use std::io::{Read as _, Write as _};
        let file = std::fs::File::open(src).unwrap();
        let mut z = zip::ZipArchive::new(file).unwrap();
        let out = std::fs::File::create(dst).unwrap();
        let mut w = zip::ZipWriter::new(out);
        for name in keep {
            // 条目借用 `z` 且 Drop 时还要用它 ⇒ 取完字节就出块，别横跨下一次 `by_name`。
            let mut bytes = Vec::new();
            {
                let mut entry = z.by_name(name).unwrap();
                entry.read_to_end(&mut bytes).unwrap();
            }
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file(*name, opts).unwrap();
            w.write_all(&bytes).unwrap();
        }
        w.finish().unwrap();
    }

    /// 测试辅助件：只翻 `backup.db` 一个字节、manifest **原样**重写（前置②的臂：
    /// 包里 db 与 manifest 的哈希说法不一致，其它判据都必须仍是通过的）。
    fn flip_one_db_byte(src: &Path, dst: &Path) {
        let (manifest, mut db_bytes) = zip_read_all(src);
        // 偏移 100 落在 SQLite 首页的数据区里，翻它不影响文件长度 ⇒ 大小判据仍过，红在哈希上。
        db_bytes[100] ^= 0xFF;
        zip_out(dst, &manifest, &db_bytes).unwrap();
    }

    /// 测试辅助件：只改 manifest 里的 `schemaVersion`、db 字节**原样**（前置③的臂）。
    /// db 原样 ⇒ manifest 记的哈希对这种包**仍然通过**（这正是「互不遮蔽」的要求），
    /// 所以它单独测不出两条判据的先后 —— 顺序由本测试末尾的双违反臂直钉（见 `read_package` 注释）。
    fn bump_schema_in_manifest(src: &Path, dst: &Path, version: i64) {
        let (mut manifest, db_bytes) = zip_read_all(src);
        manifest.schema_version = version;
        zip_out(dst, &manifest, &db_bytes).unwrap();
    }

    /// 测试辅助件：只改 manifest 里的 `formatVersion`、db 字节原样。
    fn set_format_version(src: &Path, dst: &Path, version: u32) {
        let (mut manifest, db_bytes) = zip_read_all(src);
        manifest.format_version = version;
        zip_out(dst, &manifest, &db_bytes).unwrap();
    }

    /// happy：T4 造出来的包必须能原样验通，且念出来的数字与导出时同源。
    #[test]
    fn inspect_reads_back_what_export_wrote() {
        let live = crate::db::open_in_memory().unwrap();
        seed_ledger(&live, 2);
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("a.zip");
        let sum = export(&live, &dst).unwrap();
        let info = inspect(&dst).unwrap();
        assert_eq!(info.counts, sum.counts, "inspect 念出来的数必须与导出时同源：{:?}", info.counts);
        assert_eq!(info.schema_version, sum.schema_version);
        assert!(!info.plaintext_warning.is_empty(), "明文告警必须随包走（D6 的兑现物）");
    }

    /// 前置①：不是 zip / manifest 缺失，都归 backup_package_invalid，且不许 panic。
    #[test]
    fn inspect_rejects_non_zip_and_missing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk.zip");
        std::fs::write(&junk, b"this is not a zip archive at all").unwrap();
        let e = inspect(&junk).expect_err("非 zip 字节必须红");
        assert_eq!(e.code, "backup_package_invalid");

        let good = dir.path().join("good.zip");
        let live = crate::db::open_in_memory().unwrap();
        seed_ledger(&live, 1);
        export(&live, &good).unwrap();
        // 辅助件 `zip_write_entries(src, dst, keep)`：读出 src 的全部条目，只按 `keep` 列出的
        // 名字重写进 dst（本格 keep = [DB_ENTRY]，于是 manifest 整个没了）。
        let no_manifest = dir.path().join("nm.zip");
        zip_write_entries(&good, &no_manifest, &[DB_ENTRY]);
        let e2 = inspect(&no_manifest).expect_err("缺 manifest 必须红");
        assert_eq!(e2.code, "backup_package_invalid");
    }

    /// 前置②：翻 db 一个字节 ⇒ 必须报**是哪个文件**损坏（技术方案 line 177 那句「说明是哪个文件」的兑现）。
    #[test]
    fn inspect_reports_which_file_is_corrupt() {
        let live = crate::db::open_in_memory().unwrap();
        seed_ledger(&live, 2);
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("g.zip");
        export(&live, &good).unwrap();
        let flipped = dir.path().join("f.zip");
        flip_one_db_byte(&good, &flipped);
        let e = inspect(&flipped).expect_err("哈希不符必须红");
        assert_eq!(e.code, "backup_hash_mismatch");
        assert!(e.message.contains(DB_ENTRY), "消息要点名是哪个文件：{}", e.message);
    }

    /// 前置③：包比本机新 ⇒ 拒在写库之前（D5 花第三条命令买到的就是这一格）。
    #[test]
    fn inspect_refuses_a_package_from_a_newer_schema() {
        let live = crate::db::open_in_memory().unwrap();
        seed_ledger(&live, 1);
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.zip");
        export(&live, &real).unwrap();
        let newer = dir.path().join("newer.zip");
        bump_schema_in_manifest(&real, &newer, crate::db::TARGET_VERSION + 1);
        let e = inspect(&newer).expect_err("更新的 schema 必须拒");
        assert_eq!(e.code, "backup_schema_too_new");
        // 简报原行 `e.hint.is_some_and(|h| h.contains("升级"))` 与同一断言里的 `{:?}, e.hint`
        // 撞 E0382（部分移动后复用），改成 match：判据不变，且回显面不含包内字段值（红线④）。
        match &e.hint {
            Some(h) => assert!(h.contains("升级"), "hint 要指路「先升级应用」"),
            None => panic!("hint 要指路「先升级应用」，实得 None"),
        }

        // 反面对照：同一份包把版本号改回 TARGET_VERSION 就验得通 —— 证明上一条不是恒红。
        let same = dir.path().join("same.zip");
        bump_schema_in_manifest(&real, &same, crate::db::TARGET_VERSION);
        assert!(inspect(&same).is_ok(), "等于本机版本必须放行（否则上一条断言是恒真摆设）");

        // 顺序的直钉臂：`newer.zip`（只改 manifest）里 db 字节原样、哈希仍相符 ⇒ 单独调成
        // 「先哈希后版本」它也过版本判据、测不出顺序（简报 Step 6② 的预期在自家夹具上不成立，
        // 见本格报告）。所以再加一份**同时违反版本与哈希**的包：按 ①②③④ 的正确顺序它必须
        // 红在版本判据上；谁把哈希挪到版本之前，这里就会红在 mismatch 上。
        let both = dir.path().join("both.zip");
        let (m_bumped, mut bytes) = zip_read_all(&newer);
        bytes[100] ^= 0xFF;
        zip_out(&both, &m_bumped, &bytes).unwrap();
        let e3 = inspect(&both).expect_err("双违反的包必须红");
        assert_eq!(e3.code, "backup_schema_too_new", "顺序即判据：版本判据必须先于哈希判据");
    }

    /// `formatVersion` 不是 1 的包不承诺读得动（spec §五 末尾那条 YAGNI），要指名道姓地拒。
    #[test]
    fn inspect_refuses_an_unknown_format_version() {
        let live = crate::db::open_in_memory().unwrap();
        seed_ledger(&live, 1);
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("r.zip");
        export(&live, &real).unwrap();
        let weird = dir.path().join("w.zip");
        set_format_version(&real, &weird, 7);
        let e = inspect(&weird).expect_err("未知包格式版本必须红");
        assert_eq!(e.code, "backup_package_invalid");
        assert!(e.message.contains("7"), "消息要念出它读到的版本号：{}", e.message);
    }

    // ---- T6：import（四道前置全过才动库 → 应急包 → restore 灌活连接 → migrate → assert_wal）----

    /// 主承诺的正反两面：导入把台账灌回来，同时**索引仍是空的**（包里就没有），
    /// 且数据目录里出现一份能读得通的应急包，其 counts 等于覆写前（restore 之前）那一刻的活库。
    #[test]
    fn import_restores_the_ledger_and_leaves_an_undo_copy_behind() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let mut live = crate::db::open(&data).unwrap();
        seed_ledger(&live, 3);
        let pkg = dir.path().join("p.zip");
        export(&live, &pkg).unwrap();

        // 导出时的基准数字（导入后应当逐张表回到这里）。
        let before = read_counts(&live).unwrap();
        // 改活库：删掉一条项目，制造「回滚」的现实场景。从这一刻到 restore 之前才是「覆写前」，
        // 应急包念出来的必须是这份现数（控制方裁定 4：counts 语义 = 覆写前那份现状）。
        live.execute("DELETE FROM projects WHERE id='p1'", []).unwrap();
        let after_delete = read_counts(&live).unwrap();
        assert_eq!(after_delete.projects, before.projects - 1, "夹具得先真的删掉一条，否则下面什么都没证");

        let out = import(&mut live, &pkg, &data).unwrap();
        let back = read_counts(&live).unwrap();
        assert_eq!(back.projects, before.projects, "被删的项目要回来，这是回滚的全部意义：{back:?}");
        assert_eq!(back, before, "逐张表对上：导出前 {before:?} vs 导入后 {back:?}");
        let idx: i64 = live.query_row("SELECT COUNT(*) FROM index_docs", [], |r| r.get(0)).unwrap();
        assert_eq!(idx, 0, "索引不进包 ⇒ 导入后必然是空的（spec §七 正面判据）");
        assert_eq!(out.counts, after_delete, "ImportOutcome.counts 是覆写前的计数，不是导入后的（裁定 4）：{:?}", out.counts);

        // 应急包：落在数据目录里、能被 inspect 读通、counts 等于覆写前那一刻的现数。
        let pre = std::path::Path::new(&out.pre_restore_path);
        assert!(pre.parent() == Some(data.as_path()), "应急包必须落在数据目录内，不在包旁边：{:?}", out.pre_restore_path);
        let fname = pre.file_name().unwrap().to_string_lossy().to_string();
        assert!(fname.starts_with("pre-restore-") && fname.ends_with(".zip"), "应急包名形状不对：{fname}");
        assert!(!fname.contains(':'), "Windows 文件名里不能有冒号：{fname}");
        let info = inspect(pre).unwrap();
        assert_eq!(info.counts, after_delete, "应急包念出来的数字必须就是覆写前（restore 之前）那份：{:?}", info.counts);
    }

    /// 覆写之后 WAL 这条硬门必须还在 —— `restore` 不经过 `after_open`，这格全靠 import 收尾现读。
    #[test]
    fn import_leaves_the_restored_db_in_wal_and_unlocked() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("d2");
        std::fs::create_dir_all(&data).unwrap();
        let mut live = crate::db::open(&data).unwrap();
        seed_ledger(&live, 1);
        let pkg = dir.path().join("q.zip");
        export(&live, &pkg).unwrap();
        import(&mut live, &pkg, &data).unwrap();
        // 文件库上现读 journal_mode：内存库这条断言是恒真的废断言，所以夹具必须是文件库（spec §八）。
        let mode: String = live.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode.to_lowercase(), "wal", "导入后必须仍是 WAL，否则断电就损坏用户唯一的库");
        // -wal 侧文件在不在由 SQLite 自己管，这里不钉（钉了就是拿实现细节当断言）。
        assert!(data.join("ledger.db").exists());
    }

    /// 前置①②③失败时**库必须逐字未动**（四道里前三道的共同期望，spec §三 表）。
    #[test]
    fn a_rejected_package_leaves_the_database_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("d3");
        std::fs::create_dir_all(&data).unwrap();
        let mut live = crate::db::open(&data).unwrap();
        seed_ledger(&live, 5);
        let untouched = read_counts(&live).unwrap();

        // ①非 zip
        let junk = dir.path().join("j.zip");
        std::fs::write(&junk, b"not a zip").unwrap();
        // ②翻 db 字节（哈希不符）
        let good = dir.path().join("g3.zip");
        export(&live, &good).unwrap();
        let flipped = dir.path().join("f3.zip");
        flip_one_db_byte(&good, &flipped);
        // ③包比本机新
        let newer = dir.path().join("n3.zip");
        bump_schema_in_manifest(&good, &newer, crate::db::TARGET_VERSION + 1);

        for (name, p) in [("junk", &junk), ("flipped", &flipped), ("newer", &newer)] {
            let err = import(&mut live, p, &data).expect_err("{name} 必须被拒");
            assert!(matches!(err.code, "backup_package_invalid" | "backup_hash_mismatch" | "backup_schema_too_new"),
                "{name} 报了个没登记过的码：{}", err.code);
            assert_eq!(read_counts(&live).unwrap(), untouched, "{name} 被拒之后库必须一字未动");
            // 更要紧：前三道失败**不许留下应急包**（应急包是第四道才写的）。
            assert_eq!(std::fs::read_dir(&data).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with("pre-restore-")).count(), 0,
                "{name} 被拒后不该在数据目录里写出应急包");
        }
    }

    /// 第二连接在册时导入必须仍成功（控制方在 T6 派发前补的一臂，spec §三 那条具名风险的替代取证）：
    /// 本仓生产路径**本来就存在**第二条常驻连接 —— watcher 线程在 `watch.rs:70` 开一条并长持。
    /// `restore` 往活连接灌页时它若在**读**，就是两个读者/一个写者争同一库；实测「它当前不持读事务
    /// 时 restore 照常成功」能钉住「导入不会把 watcher 打死」这件事，但**钉不住**「watcher 正在
    /// `list_watchable_roots` 读库的中间被 import 打断」那一档时序（本机不可廉价构造 ⇒ 有名缺口，
    /// 由 SQLite 自身的文件锁互斥兜，写进 §九 的没能自动验证）。
    #[test]
    fn import_succeeds_while_a_second_connection_holds_the_same_db() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("d5");
        std::fs::create_dir_all(&data).unwrap();
        let mut conn = crate::db::open(&data).unwrap();
        seed_ledger(&conn, 2);
        let pkg = dir.path().join("p5.zip");
        export(&conn, &pkg).unwrap();
        // 照 watcher 的形制长持第二条连接（同 `db::open`，不是只读打开 —— 只读打开 WAL 库在本机
        // 已被证伪过一次：`-shm` 需要恢复时会 `unable to open database file`）。
        let second = crate::db::open(&data).unwrap();
        let n: i64 = second.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2, "第二条连接先读一次，确认它真连到了同一个库：{n}");
        let out = import(&mut conn, &pkg, &data).expect("第二条连接在册时 import 必须成功");
        assert!(!out.pre_restore_path.is_empty());
        let m: i64 = second.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0)).unwrap();
        assert_eq!(m, 2, "另一条连接随后再读要看得见导入后的库（不是报错、也不是读到陈旧页）：{m}");
    }

    /// 前置④：应急包写不出去 ⇒ 必须在覆写之前失败，且 hint 里不许出现「已导入」。
    /// 造法：`data_dir` 指向一个**不存在且建不了**的位置（Windows 下用一个已有文件当目录名最稳）。
    #[test]
    fn an_unwritable_undo_copy_aborts_before_any_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("d4");
        std::fs::create_dir_all(&data).unwrap();
        let mut live = crate::db::open(&data).unwrap();
        seed_ledger(&live, 4);
        let pkg = dir.path().join("p4.zip");
        export(&live, &pkg).unwrap();
        let before = read_counts(&live).unwrap();
        // 删一条，制造「如果导成功了就会看到」的差异。
        live.execute("DELETE FROM projects WHERE id='p1'", []).unwrap();

        let block_file = dir.path().join("block");
        std::fs::write(&block_file, b"x").unwrap();
        // 把一个**普通文件**当数据目录传进去：create_dir_all / File::create 必然失败。
        let err = import(&mut live, &pkg, &block_file).expect_err("应急包写不出去就必须中止");
        assert!(!matches!(err.code, "backup_restore_failed" | "backup_wal_lost" | "backup_migrate_failed"),
            "报的是覆写之后的码，说明前置④没挡住覆写：{}", err.code);
        let now = read_counts(&live).unwrap();
        assert_eq!(now.projects, before.projects - 1, "库必须还是「删过一条」的状态，一点没被灌：{now:?}");
    }

    /// 应急包的名字不许带冒号（Windows 文件名里 `:` 非法，M7 的 `%20` 教训同类：路径形状是实测项）。
    /// T7 Step 0：名字多了进程内唯一后缀段（两参形状），仍必须不含 `:` 与其余非法字符。
    #[test]
    fn pre_restore_name_has_no_illegal_characters() {
        let n = pre_restore_name("2026-10-13T21-30-12Z", "4242-1728000000123456789");
        assert_eq!(n, "pre-restore-2026-10-13T21-30-12Z-4242-1728000000123456789.zip");
        for bad in ['"', '<', '>', '|', '?', '*'] {
            assert!(!n.contains(bad), "包名里不能有 {bad}：{n}");
        }
        assert!(!n.contains(':'), "包名里不能有冒号：{n}");
        assert!(n.starts_with("pre-restore-") && n.ends_with(".zip"));
    }

    /// T7 Step 0 收尾：应急包名带进程内唯一后缀 ⇒ 同一份数据目录上连导两次，两次的撤销键都必须在。
    /// 三半判据：① 剥掉 `pre-restore-<stamp>` 前缀后**唯一尾段非空且两次不同**（这条与「两次是否
    /// 跨过秒界」无关，所以唯一后缀不会因 `stamp` 自己把名字分开而空过）② 两个 `pre_restore_path`
    /// 不相等 ③ **两个文件都还在** —— 只断路径字符串不同证不到「第一次那份没被第二次静默 truncate 掉」
    /// （那正是本条要防的破 D1 事故）。
    #[test]
    fn two_imports_in_a_row_keep_both_undo_copies() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data-2x");
        std::fs::create_dir_all(&data).unwrap();
        let mut live = crate::db::open(&data).unwrap();
        seed_ledger(&live, 2);
        let pkg = dir.path().join("p2x.zip");
        export(&live, &pkg).unwrap();

        // 连跑两次 import：两次**可能**落在同一秒内，但也可能刚好跨过秒界 —— 那样光 `stamp`
        // 就把名字分开了，下面的 `assert_ne!` 会空过、唯一后缀那一段等于没被检验。
        let one = import(&mut live, &pkg, &data).unwrap();
        let two = import(&mut live, &pkg, &data).unwrap();
        // 所以先做一条与秒界无关的判据：把名字里 `pre-restore-<stamp>` 这段前缀剥掉，剩下的
        // 唯一尾段必须**非空**且**两次互不相同** —— 即「即使 `stamp` 完全相同，名字也必然不同」。
        let unique_tail = |p: &str| -> String {
            let file = std::path::Path::new(p)
                .file_name()
                .expect("应急包路径有文件名段")
                .to_string_lossy()
                .into_owned();
            let rest = file
                .strip_prefix("pre-restore-")
                .expect("应急包名以 pre-restore- 起头");
            // `stamp` 恒以 UTC 的 `Z` 收尾（strftime 里的字面量），`Z-` 之后那一段才是唯一后缀。
            let (s, tail) = rest
                .split_once("Z-")
                .expect("stamp 的 Z 收尾与唯一后缀之间有 - 分隔");
            assert!(!s.is_empty(), "stamp 段不该为空：{file}");
            assert!(s.contains('T'), "stamp 段该是 `%Y-%m-%dT%H-%M-%SZ` 那一条：{file}");
            let tail = tail.strip_suffix(".zip").expect("应急包名以 .zip 结尾");
            assert!(!tail.is_empty(), "剥掉 pre-restore-<stamp> 后必须有非空唯一尾段：{file}");
            tail.to_string()
        };
        assert_ne!(
            unique_tail(&one.pre_restore_path),
            unique_tail(&two.pre_restore_path),
            "剥掉 stamp 前缀后两次连导的唯一尾段仍必须不同，否则同秒连导两次时第二次会把第一次那份撤销键静默覆掉"
        );
        assert_ne!(
            one.pre_restore_path, two.pre_restore_path,
            "同秒连导两次的应急包名必须不同，否则第二次会把第一次那份静默覆掉"
        );
        assert!(
            std::path::Path::new(&one.pre_restore_path).exists(),
            "第一次那份撤销键必须还在（没被第二次 truncate）：{}",
            one.pre_restore_path
        );
        assert!(
            std::path::Path::new(&two.pre_restore_path).exists(),
            "第二次那份撤销键也要在：{}",
            two.pre_restore_path
        );
    }

    // ---- 终审 C1：guard_destination（导出落点绝不许落在应用自己的数据目录里）----

    /// (a)~(e) 五档落点判据。全部走 `tempfile::tempdir()`，绝不碰 `%APPDATA%\dev.zero.pfm\`（红线②）。
    #[test]
    fn guard_refuses_every_landing_spot_inside_the_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let sub = data.join("sub");
        std::fs::create_dir_all(&sub).unwrap();

        // (a) 活库本体：backup_export 若把落点指这里，`File::create` 会先把用户唯一的持久库截断。
        let ledger = data.join("ledger.db");
        let e = guard_destination(&data, &ledger).expect_err("(a) 数据目录里的 ledger.db 必须被拒");
        assert_eq!(e.code, "backup_destination_unsafe");
        // 消息要点名 offending path（只念路径，不回显任何文件内容 —— 红线④）。
        assert!(e.message.contains("ledger.db"), "消息必须念出被拒的落点：{}", e.message);
        match &e.hint {
            Some(h) => assert!(h.contains("数据目录以外"), "hint 要指路到数据目录之外：{h}"),
            None => panic!("hint 必须告诉用户换到数据目录以外的位置，实得 None"),
        }

        // (b) 数据目录里一个还不存在的新包：形状与应用自写的 `pre-restore-*.zip` 无从区分，且可能覆掉别人的撤销键。
        assert!(
            guard_destination(&data, &data.join("pkg.zip")).is_err(),
            "(b) 数据目录内的新包必须拒（撤销键覆写类，不只是 ledger.db）"
        );

        // (c) 数据目录的**子目录**里：同样在校内，一样拒。
        assert!(
            guard_destination(&data, &sub.join("pkg.zip")).is_err(),
            "(c) 数据目录子目录内的落点必须拒"
        );

        // (d) 独立临时目录：正常导出落点，必须放行。
        let other = tempfile::tempdir().unwrap();
        guard_destination(&data, &other.path().join("out.zip"))
            .expect("(d) 数据目录之外的落点必须放行");

        // (e) `..` 回溯形状：规范化吃回溯段后，落进数据目录 → 拒，逃到数据目录外 → 放行。
        // 用真实存在的 `sub` 当锚点：`sub\..\ledger.db` 规范化后正是 `data\ledger.db`。
        let back_inside = sub.join("..").join("ledger.db");
        assert!(
            guard_destination(&data, &back_inside).is_err(),
            "(e) 回溯后落进数据目录的相对形状必须拒"
        );
        let back_outside = sub.join("..").join("..").join("escape.zip");
        guard_destination(&data, &back_outside)
            .expect("(e) 回溯后逃到数据目录外的相对形状必须放行");

        // (f) 裸路径兜底比对那一支（终审复审带回的 Important）：`canonicalize(父目录)` 失败时
        // （这里父目录根本不存在），候选串保持 `C:\…` 原形，而 `data_dir` 规范化后是 `\\?\C:\…`，
        // 只留规范化比对会**永不命中** ⇒ 变成静默放行。第二条裸比对必须把它拒掉。
        // 注：这一支在真机上多半也会因 `File::create` 自身失败而无害，它钉的是**判据的方向**
        // （「宁可多拒不漏拒」这句注释的可证伪落点），不是那条窗口本身能不能被用户撞见。
        let deep_missing = data.join("no-such-dir").join("pkg.zip");
        assert!(
            guard_destination(&data, &deep_missing).is_err(),
            "(f) 父目录规范化失败时，裸路径仍落在数据目录内 ⇒ 必须拒（漏了裸串那一档就红）"
        );

        // (g) 数据目录本身规范化不了（还没建出来）那一档：三档里 ②/③ 都靠裸串比对兜，
        // 判据仍然要把「落在这条将来的数据目录里」的落点拒掉 —— 摘掉 None 分支里那句 starts_with 就红。
        let ghost_dir = dir.path().join("not-created-yet");
        assert!(
            guard_destination(&ghost_dir, &ghost_dir.join("ledger.db")).is_err(),
            "(g) 数据目录规范化不了时，裸串前缀那一档必须仍然拒"
        );
        // (h) 相对形状按 CWD 展开那一档：**读** `current_dir()` 不**改**它 —— `set_current_dir` 是进程级的，
        // cargo 把同批测试跑在同一进程的多个线程里，为造一条相对路径去改 CWD 会把别人脚下的路径换掉
        // （那会红得毫无道理）。相对 `dst` 会被展成 `CWD/filename`，而本进程的 CWD 就是 `src-tauri`，
        // 于是「以 CWD 当数据目录 + 只给一个裸文件名」这条组合是**确定**落在目录里的 ⇒ 必须拒。
        // 摘掉 `is_relative()` 那一段展开，本行就会红（裸串 `ledger.db` 对绝对基串永不命中 ⇒ 静默放行）。
        let cwd = std::env::current_dir().unwrap();
        assert!(
            guard_destination(&cwd, std::path::Path::new("ledger.db")).is_err(),
            "(h) 相对落点必须先按 CWD 展开再判，否则裸串对绝对基串静默放行"
        );
        // 反向那一半同样确定：CWD 下一个**不存在**的子目录当数据目录，裸文件名展开后落在它外面 ⇒ 必须放行
        // （挡住"凡是相对都拒"这种过度收紧 —— 它会连带拒掉合法的相对形状）。
        guard_destination(&cwd.join("not-the-cwd"), std::path::Path::new("out.zip"))
            .expect("(h) 展开后落在数据目录之外的相对形状必须放行");
    }

    /// C1 的收口断言：走 `backup_export` 那条「守卫在前、export 在后」的顺序，落在数据目录里的既有文件
    /// **绝不被动过**（守卫一旦 Err，`export`/`zip_out` 的 `File::create` 截断路径根本执行不到）。
    /// 若有人把守卫摘掉，`export` 会先把哨兵文件截断再写成 zip ⇒ 本行必红（wrong-value 型，能变红）。
    #[test]
    fn guarded_export_never_truncates_an_existing_file_in_the_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let ledger = data.join("ledger.db");
        // 哨兵是一串自造的测试字节，不是真实库/包内容（红线④：只比长度与相等，不回显字节）。
        let sentinel = b"SIMULATED-LIVE-DB-BYTES-PLEASINGLY-LONG-ENOUGH".to_vec();
        std::fs::write(&ledger, &sentinel).unwrap();
        let len_before = ledger.metadata().unwrap().len();

        // 复刻 lib.rs backup_export 的调用顺序：先 guard_destination，再 export。
        let guarded = |dst: &Path| -> AppResult<BackupSummary> {
            guard_destination(&data, dst)?;
            // 守卫一旦返回 Err，下面两行根本不执行 —— 也就绝不触发 zip_out 的截断。
            let live = crate::db::open_in_memory().unwrap();
            export(&live, dst)
        };

        let e = guarded(&ledger).expect_err("落点指进数据目录必须被拒");
        assert_eq!(e.code, "backup_destination_unsafe");
        // 断「文件一字未动」而不是打印字节（红线④）。
        let after = std::fs::read(&ledger).unwrap();
        assert!(after == sentinel, "被拒后哨兵字节必须一字未变（export 从未被调用，绝不截断活库）");
        assert_eq!(ledger.metadata().unwrap().len(), len_before, "被拒后长度必须不变");
    }

    /// M13 格 12 的 cargo 半边：父字段随整库快照进包。
    #[test]
    fn parent_id_survives_the_backup_snapshot() {
        let live = crate::db::open_in_memory().unwrap();
        live.execute(
            "INSERT INTO projects (id, name, status) VALUES ('p-parent', '总包', '立项')",
            [],
        )
        .unwrap();
        live.execute(
            "INSERT INTO projects (id, name, status, parent_id)
             VALUES ('p-child', '子项目', '立项', 'p-parent')",
            [],
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("hierarchy.zip");
        export(&live, &dst).unwrap();

        let (_unpacked, got) = unpack_db(&dst);
        let pid: Option<String> = got
            .query_row(
                "SELECT parent_id FROM projects WHERE id = 'p-child'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pid.as_deref(), Some("p-parent"));
    }
}
