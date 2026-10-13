//! 备份包（M8）。三个动作：导出（快照 → 剥索引 → 打 zip）、验包（只读校验）、导入（验完 → 存应急包
//! → 把包内库灌回**当前已打开**的连接）。本模块**不碰 IPC、不碰 AppState、不读任何全局**：
//! 全部喂现成的 `&Connection` / `&mut Connection`，所以三档测试都能在没有 tauri 的情况下跑。
//!
//! 为什么不用 `Connection::open` 覆盖 `ledger.db`：那个文件正被应用自己开着（`AppState.conn`），
//! Windows 上覆盖它是整条路最脏的一笔；`restore` 灌活连接同时免掉「重启应用」这一步（spec D2）。
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

/// 包格式版本。只承诺「同版本读写 + 拒绝更高 `schemaVersion` 的包」，不做跨版本兼容矩阵（spec §五）。
#[allow(dead_code)] // T4 接 export 后删
pub const FORMAT_VERSION: u32 = 1;
/// 包内条目名。**故意不叫 `ledger.db`**：免得有人把它双击或拷回数据目录当活库用（spec §五）。
#[allow(dead_code)] // T4 接 export 后删
pub const DB_ENTRY: &str = "backup.db";
#[allow(dead_code)] // T4 接 export 后删
pub const MANIFEST_ENTRY: &str = "manifest.json";

/// D6 的代价不粉饰：库里只封四列 `*_cipher`，其余全是明文列，一个没加密的包 ≈ 一份内网入口清单。
/// 界面念的是这一句，manifest 里存的也是这一句 —— 一份文本，两处出（别抄两遍再漂）。
#[allow(dead_code)] // T4 接 export 后删
pub fn warn_plaintext_columns() -> &'static str {
    "包内非 *_cipher 列为明文：项目名/编号/负责人/客户/联系人/合同号、服务器 ip 与堡垒机、各类 url、各表 note。"
}

#[allow(dead_code)] // T4 接 export 后删
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

/// 台账侧七张表的行数。导出时从**快照连接**现数（不是从活库数），这样 inspect 念出来的数字
/// 与包内容必然同源（spec §五）。
#[allow(dead_code)] // T4 接 export 后删（读侧 T5 inspect 也用它，但第一个接线点是 export）
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
#[allow(dead_code)] // T4 接 export 后删（读侧 T5 inspect 也用它，但第一个接线点是 export）
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

/// 十六进制小写。手写，不为 hex 再加一个 crate（`Cargo.lock` 的门：不新增 `[[package]]`）。
#[allow(dead_code)] // T4 接 export 后删（T5 验包哈希同样要用，第一个接线点是 export）
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[allow(dead_code)] // T4 接 export 后删（随 read_counts 一起接线）
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
#[allow(dead_code)] // T4 接 export 后删
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
#[allow(dead_code)] // T4 接 export 后删
pub fn now_utc(conn: &rusqlite::Connection) -> AppResult<String> {
    conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| r.get(0))
        .map_err(|e| {
            AppError::new(
                "backup_count_failed",
                &format!("取时间戳失败：{e}"),
                Some("这份库连 strftime 都用不了，多半不是本工具的库"),
            )
        })
}

#[cfg(test)]
impl Counts {
    /// 只给测试用（见 Step 1 注 1：生产路径的 counts 必须来自 `read_counts`）。
    ///
    /// `pub(crate)` 是裁定件：`impl` 整体在 `#[cfg(test)]` 里，而 `Manifest` 的生产字段
    /// `pub counts: Counts` 要能在测试形状里被构造；非 test 配置下**绝不**给 `Counts`
    /// derive `Default`（能默认出 0 的计数结构会被误用成「0 也算数」）。
    pub(crate) fn default_for_test() -> Counts {
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
}
