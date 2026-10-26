# M16-b 图文笔记与截图 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 备注编辑对话框能粘贴 PNG/JPEG/WebP 截图，备注展开行显示缩略图并可单张删除；图片以 BLOB 存库，随整库快照进备份。

**Architecture:** 新增 V13 `note_assets`（BLOB、无软删、FK 级联）与三条同步命令；所有读写挂在宿主备注的工作区守卫上；IPC 用 base64 data URL；备份走既有整库快照，包形状与 Counts 不变；前端只在备注展开行与编辑对话框各加一小块。

**Tech Stack:** Tauri 2 + Rust（rusqlite 0.40.2、base64 0.22）+ React 19 / TypeScript / Vite / Tailwind / zustand。

**Spec:** `docs/superpowers/specs/2026-10-24-m16b-note-images-design.md`

## Global Constraints

- `TARGET_VERSION` 12→**13**；备份包 schema **13**；`Counts` 仍 **7** 键。
- 命令 79→**82**；`tauri::command(async)` 仍 **7**；`dead_code` 豁免 **1**；`Cargo.lock` `[[package]]` **616**；`collect` **9**。
- 只收 `image/png` / `image/jpeg` / `image/webp`，按 magic bytes 判；单张 ≤ **5,242,880 字节**，每备注 ≤ **10 张**；`invalid_input`。
- 工作区守卫一律挂宿主 `ledger_notes`；跨区与不存在同走 `not_found`。**不许**对 `note_assets` 调 `ensure_active_row`（该表没有 `deleted_at`）。
- 图片硬删、无 `deleted_at`；备注软删时图片保留；硬删备注靠 FK `ON DELETE CASCADE`。
- 图片不进检索、不写审计、不进 Counts。
- 不新增 npm 依赖；`base64 = "0.22"` 提为直接依赖（锁文件已有 0.22.1），不新增 `[[package]]`。
- 错误码只用 `invalid_input` / `not_found` / `db_failed`。
- 前端不新增 zustand 字段；图片留在组件局部 state。
- 所有文案用中文；`docs/思维导图.md` 是需求源。
- 收口前别 `git clean -fdx`：`.superpowers/sdd/` 是 git-ignored 的证据目录。

---

## File Structure

| 文件 | 责任 |
|---|---|
| `src-tauri/src/db.rs` | V13 DDL、`TARGET_VERSION`、迁移测试 |
| `src-tauri/src/note_asset.rs` | 新建：`NoteAsset`、`list/save/delete`、data URL/magic/上限校验、8 条模块测试 |
| `src-tauri/Cargo.toml` | 提 `base64 = "0.22"` 为直接依赖 |
| `src-tauri/src/lib.rs` | `mod note_asset;`、三条命令壳、`generate_handler!` |
| `src-tauri/src/backup.rs` | schema 13、告警句、备份往返测试 |
| `src/types/ledger.ts` | `NoteAsset` / `NoteAssetMime` 线格式 |
| `src/lib/api.ts` | 三条 invoke 包装 |
| `src/components/ledger-panel.tsx` | 展开行缩略图/删除、编辑对话框 paste、刷新计数 |
| `docs/技术方案.md` / `docs/开发进度.md` / `docs/HANDOFF.md` | 活文档回写 |

---

## Task 1: V13 note_assets 迁移与备份口径

**Files:**
- Modify: `src-tauri/src/db.rs:98`、`:487`（V12 后插 V13）、`:130`（match）、`:772-781`（版本测试改名）、`:1391` 附近（追加回填测试）
- Modify: `src-tauri/src/backup.rs:22-24`（告警句）、`:665-680`（告警测试）、`:871`/`:898`/`:921`/`:1535`（schema 断言）、文件末尾（新测试）

**Interfaces:**
- Produces: `note_assets` 表（六列）、`TARGET_VERSION = 13`、备份 schema 13、告警句含「备注图片（二进制」。
- 后续 Task 2 只依赖表名与列名，不依赖本任务的任何 Rust 符号。

- [ ] **Step 1: 写失败的迁移/备份测试**

在 `db.rs` 的 `mod tests` 里，把既有 `v12_schema_meta_reaches_target_version_twelve`（`:774`）整段替换成：

```rust
    #[test]
    fn v13_schema_meta_reaches_target_version_thirteen() {
        let conn = open_in_memory().unwrap();
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 13, "迁移必须跑到 13");
        assert_eq!(TARGET_VERSION, 13, "= 上一条，两处别各写各的");
    }

    /// V13 必须建出截图表、索引，且表上没有软删列（硬删口径，spec §三）。
    #[test]
    fn v13_note_assets_table_and_index_exist() {
        let conn = open_in_memory().unwrap();
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('note_assets') ORDER BY cid")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(
            cols.join(","),
            "id,note_id,mime,size,data,created_at",
            "V13 的列集合/顺序变了要同步 note_asset.rs 的 SELECT"
        );
        let idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                  WHERE type='index' AND name='ix_note_asset_note'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 1, "ix_note_asset_note 必须存在");
        let soft: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('note_assets') WHERE name='deleted_at'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(soft, 0, "截图是硬删，不该有 deleted_at");
    }
```

在 `v12_backfills_nothing`（`:1391`）之后追加：

```rust
    /// V13 只建表：存量备注一字不动，截图表 0 行（spec §三）。
    #[test]
    fn v13_backfills_nothing() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_meta (version INTEGER NOT NULL, applied_at TEXT NOT NULL);",
        )
        .unwrap();
        for (target, ddl) in [
            (1, V1), (2, V2), (3, V3), (4, V4), (5, V5), (6, V6),
            (7, V7), (8, V8), (9, V9), (10, V10), (11, V11), (12, V12),
        ] {
            apply_version(&conn, target, ddl).unwrap();
        }
        conn.execute(
            "INSERT INTO projects (id, name) VALUES ('p-old', '存量项目')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ledger_notes (id, project_id, title, body)
             VALUES ('n-old', 'p-old', '旧备注', '旧正文')",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap();

        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM note_assets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "V13 不许回填任何截图行");
        let note: String = conn
            .query_row(
                "SELECT title || '/' || body FROM ledger_notes WHERE id = 'n-old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(note, "旧备注/旧正文", "V13 只建表，不许碰存量备注");
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, TARGET_VERSION);
    }
```

在 `backup.rs` 的 `plaintext_warning_names_every_plain_text_table_since_m9`（`:665`）末尾追加：

```rust
        // M16-b：截图是明文二进制，库与包都不加密。
        assert!(warn.contains("备注图片（二进制"), "漏了备注图片的明文：{warn}");
```

在 `backup.rs` 的 `mod tests` 末尾（`backup_package_carries_project_versions_without_counting_them` 之后）追加：

```rust
    /// M16-b 的备份半边：截图字节随整库快照进包，Counts 仍是七张表。
    #[test]
    fn backup_package_carries_note_assets_without_counting_them() {
        let live = crate::db::open_in_memory().unwrap();
        live.execute("INSERT INTO projects (id, name) VALUES ('p1', '甲项目')", [])
            .unwrap();
        live.execute(
            "INSERT INTO ledger_notes (id, project_id, title, body)
             VALUES ('n1', 'p1', '截图备注', '')",
            [],
        )
        .unwrap();
        let bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
        live.execute(
            "INSERT INTO note_assets (id, note_id, mime, size, data)
             VALUES ('a1', 'n1', 'image/png', ?1, ?2)",
            rusqlite::params![bytes.len() as i64, bytes.clone()],
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("assets.zip");
        let sum = export(&live, &dst).unwrap();
        assert_eq!(sum.schema_version, crate::db::TARGET_VERSION);
        assert_eq!(sum.schema_version, 13, "M16-b 起包内 schema 必须是 13");
        assert_eq!(
            serde_json::to_value(sum.counts).unwrap().as_object().unwrap().len(),
            7,
            "note_assets 是新表，不进 Counts"
        );

        let (_unpacked, got) = unpack_db(&dst);
        let (mime, size, data): (String, i64, Vec<u8>) = got
            .query_row(
                "SELECT mime, size, data FROM note_assets WHERE id = 'a1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(size, bytes.len() as i64);
        assert_eq!(data, bytes, "截图字节必须原样随包");
    }
```

同时把 `backup.rs` 四处 `12` 断言改成 `13`：
- `:871`、`:921`、`:1535` 的消息改成 `"M16-b 起包内 schema 必须是 13"`；
- `:898` 的裸 `assert_eq!(sum.schema_version, 12);` 改 `13`。

- [ ] **Step 2: 跑测试确认先红**

```bash
cd src-tauri && cargo test -j 1 --lib db::tests::v13; echo "exit=$?"
cd src-tauri && cargo test -j 1 --lib backup::tests::backup_package_carries_note_assets; echo "exit=$?"
cd src-tauri && cargo test -j 1 --lib backup::tests::plaintext_warning_names; echo "exit=$?"
```

Expected: 第一条红在 `no such table: note_assets` 或 `assert_eq!(ver, 13)`；备份测试红在 `12 != 13`；告警测试红在缺「备注图片」。

- [ ] **Step 3: 实现 V13**

`db.rs`：

```rust
pub(crate) const TARGET_VERSION: i64 = 13;
```

在 V12 常量之后插入：

```rust
/// v13：备注截图（M16-b spec §三）。图片字节直接存库（BLOB），无附件目录、无软删；
/// 软删备注靠宿主行的 `deleted_at` 挡读，硬删备注由 FK 级联清图。
const V13: &str = "
CREATE TABLE note_assets (
    id         TEXT PRIMARY KEY,
    note_id    TEXT NOT NULL REFERENCES ledger_notes(id) ON DELETE CASCADE,
    mime       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    data       BLOB NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX ix_note_asset_note ON note_assets(note_id, created_at);
";
```

迁移 match 加一行：

```rust
            12 => V12,
            13 => V13,
```

`backup.rs` 的告警句改成（只加最后一小段）：

```rust
    "包内非 *_cipher 列为明文：项目名/编号/负责人/客户/联系人/合同号（含项目模板的同类字段）、工作区名称、服务器 ip 与堡垒机、各类 url、各表 note、待办内容、里程碑备注、日报内容、到期事项名称与备注、版本号与上线内容、备注图片（二进制，截图内容未加密）、项目修改日志的字段旧值/新值（含联系人、联系方式）。"
```

- [ ] **Step 4: 跑本任务测试转绿**

```bash
cd src-tauri && cargo test -j 1 --lib db::tests::v13; echo "exit=$?"
cd src-tauri && cargo test -j 1 --lib backup::tests::backup_package_carries_note_assets; echo "exit=$?"
cd src-tauri && cargo test -j 1 --lib backup::tests::plaintext_warning_names; echo "exit=$?"
cd src-tauri && cargo test -j 1 --lib backup::tests; echo "exit=$?"
```

Expected: 全部 `exit=0`；`backup::tests` 里四条旧 schema 断言也随 13 转绿。

- [ ] **Step 5: clippy + build**

```bash
cd src-tauri && cargo clippy -j 1 --lib -- -D warnings; echo "exit=$?"
cd src-tauri && cargo clippy -j 1 --lib --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo build -j 1 --lib; echo "exit=$?"
```

Expected: 三条 `exit=0`。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/db.rs src-tauri/src/backup.rs
git commit -m "feat(m16b): V13 note_assets 迁移与备份 schema 13"
```

---

## Task 2: note_asset 模块、三条命令与 base64

**Files:**
- Create: `src-tauri/src/note_asset.rs`
- Modify: `src-tauri/Cargo.toml:56` 之后
- Modify: `src-tauri/src/lib.rs:14` 附近（`mod note_asset;`）、`:480` 之后（三条命令）、`:1106` 之后（handler）

**Interfaces:**
- Consumes: `crate::workspace::ensure_active_row(conn, "ledger_notes", note_id)`；表 `note_assets(id, note_id, mime, size, data, created_at)`。
- Produces:
  - `note_asset::NoteAsset { id: String, note_id: String, mime: String, size: i64, created_at: String, data_url: String }`（Serialize camelCase）。
  - `note_asset::list(conn: &Connection, note_id: &str) -> AppResult<Vec<NoteAsset>>`
  - `note_asset::save(conn: &Connection, note_id: &str, data_url: &str) -> AppResult<()>`
  - `note_asset::delete(conn: &Connection, id: &str) -> AppResult<()>`
  - 命令 `note_asset_list(note_id)` / `note_asset_save(note_id, data_url)` / `note_asset_delete(id)`。

- [ ] **Step 1: 加依赖、建模块与失败测试**

`Cargo.toml` 在 `sha2 = "0.11"` 之后加：

```toml
# M16-b：备注截图的 data URL 编解码。0.22.1 已在 Cargo.lock（传递依赖），提为直接依赖不新增
# `[[package]]`、不新增编译单元；手写 base64 是红线。
base64 = "0.22"
```

新建 `src-tauri/src/note_asset.rs`，先写**类型 + 空实现 + 全部测试**（`unimplemented!()` 让测试先红）：

```rust
//! 备注截图（M16-b spec §三/§四）：图片字节存库（BLOB），随整库快照进备份。

use base64::Engine as _;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::error::{AppError, AppResult};

const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const MAX_ASSETS_PER_NOTE: i64 = 10;
const MAX_DATA_URL_LEN: usize = 7 * 1024 * 1024;
const PNG: &str = "image/png";
const JPEG: &str = "image/jpeg";
const WEBP: &str = "image/webp";

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查截图格式与大小"))
}

fn not_found(id: &str) -> AppError {
    AppError::new("not_found", &format!("记录不存在：{id}"), Some("刷新后重试"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteAsset {
    pub id: String,
    pub note_id: String,
    pub mime: String,
    pub size: i64,
    pub created_at: String,
    pub data_url: String,
}

pub fn list(_conn: &Connection, _note_id: &str) -> AppResult<Vec<NoteAsset>> {
    unimplemented!()
}

pub fn save(_conn: &Connection, _note_id: &str, _data_url: &str) -> AppResult<()> {
    unimplemented!()
}

pub fn delete(_conn: &Connection, _id: &str) -> AppResult<()> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use base64::Engine as _;
    use rusqlite::Connection;

    const PNG_DATA_URL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    fn fixture() -> (Connection, String) {
        let conn = db::open_in_memory().unwrap();
        conn.execute("INSERT INTO projects (id, name) VALUES ('p1', '甲项目')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO ledger_notes (id, project_id, title, body)
             VALUES ('n1', 'p1', '坑点', '正文')",
            [],
        )
        .unwrap();
        (conn, "n1".to_owned())
    }

    fn data_url(mime: &str, bytes: &[u8]) -> String {
        format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }

    fn png_bytes() -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(PNG_DATA_URL.split_once(',').unwrap().1)
            .unwrap()
    }

    fn jpeg_bytes() -> Vec<u8> {
        vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00]
    }

    fn count(conn: &Connection, note_id: &str) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM note_assets WHERE note_id = ?1",
            [note_id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn save_and_list_assets_roundtrip() {
        let (conn, note) = fixture();
        save(&conn, &note, PNG_DATA_URL).unwrap();
        save(&conn, &note, &data_url("image/jpeg", &jpeg_bytes())).unwrap();

        let rows = list(&conn, &note).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].mime, "image/png");
        assert_eq!(rows[0].size, png_bytes().len() as i64);
        assert_eq!(rows[0].data_url, PNG_DATA_URL, "规范 base64 往返必须逐字相等");
        assert_eq!(rows[1].mime, "image/jpeg", "同秒两张按插入序");

        let obj = serde_json::to_value(&rows[0]).unwrap();
        let keys: Vec<&String> = obj.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 6, "NoteAsset 键集合变了：{keys:?}");
        for key in ["id", "noteId", "mime", "size", "createdAt", "dataUrl"] {
            assert!(obj.get(key).is_some(), "缺 {key}");
        }
        assert!(obj.get("data").is_none() && obj.get("blob").is_none(), "不许回裸字节");
    }

    #[test]
    fn save_rejects_non_image_payloads() {
        let (conn, note) = fixture();
        let bad = [
            "data:text/plain;base64,aGVsbG8=".to_owned(),
            data_url("image/gif", b"GIF89a-fake-bytes"),
            data_url("image/svg+xml", b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
            data_url("image/png", &jpeg_bytes()),
            "data:image/png;base64,!!!!".to_owned(),
        ];
        for input in &bad {
            assert_eq!(
                save(&conn, &note, input).unwrap_err().code,
                "invalid_input",
                "「{}」竟然被收下",
                &input[..input.len().min(32)]
            );
        }
        assert_eq!(count(&conn, &note), 0, "被拒的一张都不许入库");
    }

    #[test]
    fn save_refuses_oversized_and_empty_images() {
        let (conn, note) = fixture();
        assert_eq!(
            save(&conn, &note, "data:image/png;base64,").unwrap_err().code,
            "invalid_input"
        );

        let mut big = vec![0u8; MAX_IMAGE_BYTES + 1];
        big[..PNG_MAGIC.len()].copy_from_slice(&PNG_MAGIC);
        assert_eq!(
            save(&conn, &note, &data_url("image/png", &big)).unwrap_err().code,
            "invalid_input",
            "5 MiB+1 必须被大小门挡住"
        );

        let huge = format!("data:image/png;base64,{}", "A".repeat(MAX_DATA_URL_LEN));
        assert_eq!(
            save(&conn, &note, &huge).unwrap_err().code,
            "invalid_input",
            "超长串要在解码前被拒"
        );
        assert_eq!(count(&conn, &note), 0);
    }

    #[test]
    fn save_refuses_an_eleventh_image() {
        let (conn, note) = fixture();
        for i in 0..10 {
            save(&conn, &note, PNG_DATA_URL).unwrap_or_else(|e| panic!("第 {} 张失败：{e:?}", i + 1));
        }
        let err = save(&conn, &note, PNG_DATA_URL).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("10"), "文案要点名上限：{}", err.message);
        assert_eq!(count(&conn, &note), 10);
    }

    #[test]
    fn note_asset_list_and_save_are_scoped() {
        let (conn, note) = fixture();
        save(&conn, &note, PNG_DATA_URL).unwrap();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        conn.execute(
            "UPDATE projects SET workspace_id = ?1 WHERE id = 'p1'",
            [&b.id],
        )
        .unwrap();

        assert_eq!(list(&conn, &note).unwrap_err().code, "not_found");
        assert_eq!(save(&conn, &note, PNG_DATA_URL).unwrap_err().code, "not_found");
        assert_eq!(count(&conn, &note), 1, "跨区被拒后一行都不许多");

        crate::workspace::switch(&conn, &b.id).unwrap();
        assert_eq!(list(&conn, &note).unwrap().len(), 1, "切到新区后原样可见");
        save(&conn, &note, PNG_DATA_URL).unwrap();
        assert_eq!(list(&conn, &note).unwrap().len(), 2);
    }

    #[test]
    fn note_asset_delete_is_scoped() {
        let (conn, note) = fixture();
        save(&conn, &note, PNG_DATA_URL).unwrap();
        let id = list(&conn, &note).unwrap()[0].id.clone();
        let b = crate::workspace::create(&conn, "客户乙").unwrap();
        conn.execute(
            "UPDATE projects SET workspace_id = ?1 WHERE id = 'p1'",
            [&b.id],
        )
        .unwrap();

        assert_eq!(delete(&conn, &id).unwrap_err().code, "not_found");
        assert_eq!(count(&conn, &note), 1, "跨区删除不许真删");

        crate::workspace::switch(&conn, &b.id).unwrap();
        delete(&conn, &id).unwrap();
        assert!(list(&conn, &note).unwrap().is_empty());
    }

    #[test]
    fn delete_removes_the_row_and_missing_is_not_found() {
        let (conn, note) = fixture();
        save(&conn, &note, PNG_DATA_URL).unwrap();
        save(&conn, &note, PNG_DATA_URL).unwrap();
        let id = list(&conn, &note).unwrap()[0].id.clone();

        delete(&conn, &id).unwrap();
        assert_eq!(list(&conn, &note).unwrap().len(), 1);
        assert_eq!(delete(&conn, &id).unwrap_err().code, "not_found");
    }

    #[test]
    fn hard_deleted_note_cascades_its_assets() {
        let (conn, note) = fixture();
        save(&conn, &note, PNG_DATA_URL).unwrap();
        conn.execute("DELETE FROM ledger_notes WHERE id = 'n1'", []).unwrap();
        assert_eq!(count(&conn, "n1"), 0, "硬删备注必须级联清图");

        conn.execute(
            "INSERT INTO ledger_notes (id, project_id, title, body)
             VALUES ('n2', 'p1', '软删备注', '')",
            [],
        )
        .unwrap();
        save(&conn, "n2", PNG_DATA_URL).unwrap();
        conn.execute(
            "UPDATE ledger_notes SET deleted_at = datetime('now') WHERE id = 'n2'",
            [],
        )
        .unwrap();
        assert_eq!(count(&conn, "n2"), 1, "软删不级联，图片行要留着");
        assert_eq!(list(&conn, "n2").unwrap_err().code, "not_found");
    }
}
```

`lib.rs` 顶部 `mod milestone;` 之后加：

```rust
mod note_asset;
```

- [ ] **Step 2: 跑测试确认先红**

```bash
cd src-tauri && cargo test -j 1 --lib note_asset::tests; echo "exit=$?"
```

Expected: 8 条全部 fail / panic 在 `not implemented`。

- [ ] **Step 3: 实现模块**

把 `list` / `save` / `delete` 三个空实现之间插入校验函数，并替换三个函数体：

```rust
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(PNG);
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(JPEG);
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(WEBP);
    }
    None
}

fn parse(data_url: &str) -> AppResult<(&'static str, Vec<u8>)> {
    if data_url.len() > MAX_DATA_URL_LEN {
        return Err(invalid("单张截图不能超过 5 MB"));
    }
    let (header, payload) = data_url
        .split_once(',')
        .ok_or_else(|| invalid("截图不是合法的 data URL"))?;
    let declared = match header {
        "data:image/png;base64" => PNG,
        "data:image/jpeg;base64" => JPEG,
        "data:image/webp;base64" => WEBP,
        _ => return Err(invalid("只支持 PNG / JPEG / WebP 截图")),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|_| invalid("截图不是合法的 base64 数据"))?;
    if bytes.is_empty() {
        return Err(invalid("截图内容为空"));
    }
    let mime = sniff(&bytes).ok_or_else(|| invalid("只支持 PNG / JPEG / WebP 截图"))?;
    if mime != declared {
        return Err(invalid("图片内容与声明的格式不符"));
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(invalid("单张截图不能超过 5 MB"));
    }
    Ok((mime, bytes))
}

pub fn list(conn: &Connection, note_id: &str) -> AppResult<Vec<NoteAsset>> {
    crate::workspace::ensure_active_row(conn, "ledger_notes", note_id)?;
    let mut stmt = conn.prepare(
        "SELECT id, note_id, mime, size, created_at, data FROM note_assets
          WHERE note_id = ?1 ORDER BY created_at, rowid",
    )?;
    let rows = stmt.query_map([note_id], |r| {
        let mime: String = r.get(2)?;
        let data: Vec<u8> = r.get(5)?;
        Ok(NoteAsset {
            id: r.get(0)?,
            note_id: r.get(1)?,
            size: r.get(3)?,
            created_at: r.get(4)?,
            mime,
            data_url: format!(
                "data:{mime};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(data)
            ),
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn save(conn: &Connection, note_id: &str, data_url: &str) -> AppResult<()> {
    crate::workspace::ensure_active_row(conn, "ledger_notes", note_id)?;
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM note_assets WHERE note_id = ?1",
        [note_id],
        |r| r.get(0),
    )?;
    if n >= MAX_ASSETS_PER_NOTE {
        return Err(invalid("单条备注最多 10 张截图"));
    }
    let (mime, bytes) = parse(data_url)?;
    let size = bytes.len() as i64;
    conn.execute(
        "INSERT INTO note_assets (id, note_id, mime, size, data) VALUES (?1,?2,?3,?4,?5)",
        params![uuid::Uuid::new_v4().to_string(), note_id, mime, size, bytes],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let note_id: String = conn
        .query_row("SELECT note_id FROM note_assets WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| not_found(id))?;
    crate::workspace::ensure_active_row(conn, "ledger_notes", &note_id)?;
    let changed = conn.execute("DELETE FROM note_assets WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}
```

- [ ] **Step 4: 跑模块测试转绿**

```bash
cd src-tauri && cargo test -j 1 --lib note_asset::tests; echo "exit=$?"
```

Expected: `8 passed; 0 failed`，`exit=0`。

- [ ] **Step 5: 接命令与 handler**

`lib.rs` 的 `ledger_note_delete`（`:478`）之后插入：

```rust
/// 备注截图三条命令（M16-b spec §四）：同步、函数体不写 await 点。
/// 图片不经文件系统，备份里跟着整库快照走，所以 async 标记那扇门保持 7 条不动。
#[tauri::command]
fn note_asset_list(
    state: State<'_, AppState>,
    note_id: String,
) -> AppResult<Vec<note_asset::NoteAsset>> {
    let conn = db(&state)?;
    note_asset::list(&conn, &note_id)
}

#[tauri::command]
fn note_asset_save(state: State<'_, AppState>, note_id: String, data_url: String) -> AppResult<()> {
    let conn = db(&state)?;
    note_asset::save(&conn, &note_id, &data_url)
}

#[tauri::command]
fn note_asset_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = db(&state)?;
    note_asset::delete(&conn, &id)
}
```

`generate_handler!` 里 `ledger_note_delete,`（`:1106`）之后插入：

```rust
            note_asset_list,
            note_asset_save,
            note_asset_delete,
```

- [ ] **Step 6: 结构门 + clippy + build + 全量 cargo**

```bash
cd src-tauri && awk '/generate_handler!\[/,/\]\)/' src/lib.rs | grep -cE '^            [a-z_]+,?$'; echo "commands_exit=$?"
cd src-tauri && grep -c "tauri::command(async)" src/lib.rs; echo "async_exit=$?"
cd src-tauri && grep -c "TARGET_VERSION: i64 = 13" src/db.rs; echo "target_exit=$?"
cd src-tauri && grep -c "hits.append(&mut collect(" src/search.rs; echo "collect_exit=$?"
cd src-tauri && grep -rc "allow(dead_code)" src/ | grep -v ":0"; echo "exempt_exit=$?"
cd src-tauri && grep -c '^\[\[package\]\]' Cargo.lock; echo "lock_exit=$?"
cd src-tauri && cargo test -j 1 --lib; echo "exit=$?"
cd src-tauri && cargo clippy -j 1 --lib --all-targets -- -D warnings; echo "exit=$?"
```

Expected: 命令 **82**、async **7**、TARGET 行 **1**、collect **9**、豁免只有 `db.rs:30` 一条、`Cargo.lock` **616**、cargo **426 passed**、clippy exit 0。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/note_asset.rs src-tauri/src/lib.rs
git commit -m "feat(m16b): 备注截图模块、三条命令与 base64 校验"
```

---

## Task 3: 前端类型、API、缩略图与粘贴

**Files:**
- Modify: `src/types/ledger.ts`（`Note` 之后）
- Modify: `src/lib/api.ts:16-27`（类型导入）、`:243` 之后（三个包装）
- Modify: `src/components/ledger-panel.tsx:3`（React 类型导入）、`:22`（api 导入）、`:27-40`（ledger 类型导入）、`:152-209`（Panel）、`:415-450`（NotesSection/NoteRow）、`:565-722`（对话框）

**Interfaces:**
- Consumes: 命令 `note_asset_list/save/delete`；`NoteAsset` 线格式。
- Produces: 备注展开行缩略图/删除；编辑对话框 paste；`assetsTick` 刷新机制。

- [ ] **Step 1: 加线格式**

`src/types/ledger.ts` 的 `Note` 接口之后加：

```ts
export type NoteAssetMime = "image/png" | "image/jpeg" | "image/webp";

export interface NoteAsset {
  id: string;
  noteId: string;
  mime: NoteAssetMime;
  size: number;
  createdAt: string;
  dataUrl: string;
}
```

- [ ] **Step 2: 加 API 包装**

`src/lib/api.ts` 的 `@/types/ledger` 导入块加 `NoteAsset`：

```ts
  NoteAsset,
  NoteInput,
```

`deleteNote` 之后加：

```ts
export const listNoteAssets = (noteId: string) =>
  invoke<NoteAsset[]>("note_asset_list", { noteId });
export const saveNoteAsset = (noteId: string, dataUrl: string) =>
  invoke<void>("note_asset_save", { noteId, dataUrl });
export const deleteNoteAsset = (id: string) => invoke<void>("note_asset_delete", { id });
```

- [ ] **Step 3: 备注展开行**

`ledger-panel.tsx` 顶部：

```ts
import { useEffect, useState, type ClipboardEvent, type ReactNode } from "react";
```

api 导入改成：

```ts
import { copyText, deleteNoteAsset, ledgerReveal, listNoteAssets, saveNoteAsset } from "@/lib/api";
```

类型导入加 `NoteAsset`。

`LedgerPanel` 里 `editing` state 之后加：

```ts
  const [assetsTick, setAssetsTick] = useState(0);
```

`NotesSection` 与对话框的调用改成：

```tsx
      <NotesSection
        items={data.notes}
        reloadKey={assetsTick}
        onAdd={() => setEditing({ entity: "notes", id: null })}
        onEdit={(id) => setEditing({ entity: "notes", id })}
      />

      {editing && (
        <LedgerFormDialog
          entity={editing.entity}
          id={editing.id}
          envs={data.envs}
          saving={saving}
          onAssetsChanged={() => setAssetsTick((v) => v + 1)}
          onClose={() => setEditing(null)}
        />
      )}
```

`NotesSection` / `NoteRow` 整段替换成：

```tsx
function NotesSection({
  items,
  reloadKey,
  onAdd,
  onEdit,
}: {
  items: Note[];
  reloadKey: number;
  onAdd: () => void;
  onEdit: (id: string) => void;
}) {
  const { drop } = useLedgerStore();
  return (
    <div className="grid gap-1.5">
      <SectionHeader title="备注台账" count={items.length} onAdd={onAdd} addLabel="加备注" />
      {items.length === 0 ? (
        <Empty text="接口坑点、遗留问题、需求变更这类「当时知道、三个月后想不起来」的事。" />
      ) : (
        items.map((n) => (
          <NoteRow
            key={n.id}
            note={n}
            reloadKey={reloadKey}
            onEdit={() => onEdit(n.id)}
            onDelete={() => void drop("notes", n.id)}
          />
        ))
      )}
    </div>
  );
}

function NoteRow({
  note,
  reloadKey,
  onEdit,
  onDelete,
}: {
  note: Note;
  reloadKey: number;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [assets, setAssets] = useState<NoteAsset[]>([]);
  const [assetsError, setAssetsError] = useState<AppErrorShape | null>(null);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    if (!open) return;
    let alive = true;
    void listNoteAssets(note.id)
      .then((rows) => {
        if (!alive) return;
        setAssets(rows);
        setAssetsError(null);
      })
      .catch((e) => {
        if (alive) setAssetsError(toAppError(e));
      });
    return () => {
      alive = false;
    };
  }, [open, note.id, reloadKey, tick]);

  const removeAsset = async (id: string) => {
    try {
      await deleteNoteAsset(id);
      setTick((v) => v + 1);
    } catch (e) {
      setAssetsError(toAppError(e));
    }
  };

  return (
    <div className="rounded-md border bg-background px-2 py-1.5 text-sm">
      <div className="flex items-center gap-2">
        <button className="flex-1 text-left" onClick={() => setOpen((v) => !v)}>
          {note.title}
          {note.tag && <Badge variant="secondary" className="ml-2">{note.tag}</Badge>}
        </button>
        <Button variant="ghost" size="icon" onClick={onEdit}>
          <Pencil className="size-4" />
        </Button>
        <Button variant="ghost" size="icon" onClick={onDelete}>
          <Trash2 className="size-4" />
        </Button>
      </div>
      {open && (
        <div className="mt-2 grid gap-2">
          {note.body && (
            <p className="whitespace-pre-wrap text-xs text-muted-foreground">{note.body}</p>
          )}
          {assetsError && (
            <p className="text-xs text-destructive" title={assetsError.hint ?? undefined}>
              {assetsError.message}
            </p>
          )}
          {assets.length === 0 ? (
            <p className="text-xs text-muted-foreground">还没有截图。编辑这条备注时可粘贴截图。</p>
          ) : (
            <div className="grid grid-cols-4 gap-2">
              {assets.map((a) => (
                <div key={a.id} className="relative w-16">
                  <img
                    src={a.dataUrl}
                    alt="备注图片"
                    title={`${Math.round(a.size / 1024)} KB`}
                    className="h-16 w-16 rounded border object-cover"
                  />
                  <Button
                    variant="secondary"
                    size="icon"
                    className="absolute -right-1 -top-1 size-5"
                    title="删除截图"
                    onClick={() => void removeAsset(a.id)}
                  >
                    <Trash2 className="size-3" />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 4: 编辑对话框粘贴**

`LedgerFormDialog` 的 props 增加 `onAssetsChanged`：

```tsx
function LedgerFormDialog({
  entity,
  id,
  envs,
  saving,
  onAssetsChanged,
  onClose,
}: {
  entity: Entity;
  id: string | null;
  envs: LedgerEnv[];
  saving: boolean;
  onAssetsChanged: () => void;
  onClose: () => void;
}) {
```

解构后加两个 state（`error` 旁边）：

```ts
  const [notice, setNotice] = useState<string | null>(null);
  const [uploading, setUploading] = useState(false);
```

`submit` 之前加：

```tsx
  const uploadPasted = async (e: ClipboardEvent<HTMLDivElement>) => {
    if (entity !== "notes" || !isEdit || uploading) return;
    const files = Array.from(e.clipboardData.items)
      .filter((item) => item.kind === "file")
      .map((item) => item.getAsFile())
      .filter((file): file is File => file !== null);
    if (files.length === 0) return;
    e.preventDefault();
    setNotice(null);
    setError(null);
    setUploading(true);
    let added = 0;
    for (const file of files) {
      if (!NOTE_IMAGE_TYPES.includes(file.type)) {
        setError({ code: "invalid_input", message: "只支持 PNG / JPEG / WebP 截图", hint: null });
        break;
      }
      try {
        await saveNoteAsset(id as string, await readAsDataUrl(file));
        added += 1;
      } catch (err) {
        setError(toAppError(err));
        break;
      }
    }
    setUploading(false);
    if (added > 0) {
      onAssetsChanged();
      setNotice(`已添加 ${added} 张截图，可在备注展开处查看`);
    }
  };
```

`DialogDescription` 整段替换成：

```tsx
          <DialogDescription>
            {entity === "notes" && !isEdit
              ? "截图请先保存备注，再在编辑时粘贴。正文是明文列，会被全文检索捞到。"
              : entity === "credentials" || entity === "servers"
                ? "账号与密码会加密后入库，明文只在解锁后按需解出单个字段。"
                : "这些字段是明文列，会被全文检索捞到；密码请填在加密字段里。"}
          </DialogDescription>
```

表单容器加 `onPaste`（`:638`）：

```tsx
        <div
          className="grid gap-3"
          onPaste={entity === "notes" && isEdit ? (e) => void uploadPasted(e) : undefined}
        >
```

`error` 提示块之后加：

```tsx
        {notice && <p className="text-sm text-emerald-700">{notice}</p>}
        {uploading && <p className="text-xs text-muted-foreground">图片保存中…</p>}
```

文件末尾 `initialForm` 之前加常量与辅助函数：

```ts
const NOTE_IMAGE_TYPES = ["image/png", "image/jpeg", "image/webp"];

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("读取剪贴板图片失败"));
    reader.readAsDataURL(file);
  });
}
```

> `NOTE_IMAGE_TYPES.includes(file.type)` 之所以能编过：数组被推断成 `string[]`，不是字面量元组；别给它加 `as const`。

- [ ] **Step 5: 前端门**

```bash
npm run build 2>&1 | tail -8; echo "exit=${PIPESTATUS[0]}"
node --test "tests/**/*.test.ts" 2>&1 | tail -8
```

Expected: `npm run build` exit 0，输出 `error TS` 计数 **0**；node `tests 75 / pass 75 / fail 0`。

> 没有 jsdom：粘贴/缩略图/删除的真机证据在 Task 4 的 C-2/C-4/C-5。

- [ ] **Step 6: 提交**

```bash
git add src/types/ledger.ts src/lib/api.ts src/components/ledger-panel.tsx
git commit -m "feat(m16b): 备注展开缩略图与编辑对话框粘贴截图"
```

---

## Task 4: 全量门禁、变异取证、真机八格（控制方跑，不派实现者）

**Files:**
- Create（证据，全在 git-ignored 的账本目录，**收口前不要 `git clean -fdx`**）：
  `.superpowers/sdd/2026-10-26-m16b-note-images/{gates.txt,mutation-evidence.txt,real-machine-readings.md,spec-plan-mapping.md,m16b-cdp.mjs,m16b-cdp.stdout.txt,m16b-backup.zip,realdir-before.txt,realdir-after.txt}`
- Modify: 无生产代码（变异下靶后必须逐条还原并 `git diff --exit-code` 验 0）。

- [ ] **Step 1: 整套门禁（终态树现跑，逐个真实退出码）**

```bash
cd src-tauri && cargo test -j 1 --lib; echo "exit=$?"
cd src-tauri && cargo clippy -j 1 --lib -- -D warnings; echo "exit=$?"
cd src-tauri && cargo clippy -j 1 --lib --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo build -j 1 --lib; echo "exit=$?"
node --test "tests/**/*.test.ts" 2>&1 | tail -8
npm run build 2>&1 | tail -5
```

结构门（仓库根）：

```bash
grep -c "TARGET_VERSION: i64 = 13" src-tauri/src/db.rs
awk '/generate_handler!\[/,/\]\)/' src-tauri/src/lib.rs | grep -cE '^            [a-z_]+,?$'   # 82
grep -c "tauri::command(async)" src-tauri/src/lib.rs                                        # 7
grep -rc "allow(dead_code)" src-tauri/src/ | grep -v ":0"                                   # 只有 db.rs:30
grep -c '^\[\[package\]\]' src-tauri/Cargo.lock                                             # 616
grep -c "hits.append(&mut collect(" src-tauri/src/search.rs                                 # 9
git diff --stat master..HEAD -- package.json                                                # 空
git diff --stat master..HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock                   # 只有 base64 那一行 + 根包依赖清单
git rev-list --merges --count --all                                                         # 0
```

Expected: cargo **426 passed**、node **75/75/0**、`npm run build` `error TS` **0**、两条 clippy 0、build 0；结构门 **82 / 7 / 13 / 1 / 616 / 9**；`package.json` diff 空；merge **0**。
退出码一律单独 `echo "exit=$?"` 取，管道 `tail` 会吞码（本仓前科）。

- [ ] **Step 2: 九处变异，逐处下靶 → 跑指定测试 → 记退出码 → 还原**

记录格式（写进 `mutation-evidence.txt`）：靶名、文件:行、原语句 → 改后语句、`command`、`exit=`、`expected_nonzero=True`、失败测试名、`restore_cmp=identical`、`git_diff_after_restore=clean`。

| # | 靶 | 怎么改 | 必须红的测试 |
|---|---|---|---|
| M① | list 不判区 | `note_asset.rs` `list` 首行 `ensure_active_row(...)` 整句删 | `note_asset::tests::note_asset_list_and_save_are_scoped` |
| M② | save 不判区 | `note_asset.rs` `save` 首行 `ensure_active_row(...)` 整句删 | `note_asset::tests::note_asset_list_and_save_are_scoped` |
| M③ | delete 不判区 | `note_asset.rs` `delete` 的 `ensure_active_row(...)` 整句删 | `note_asset::tests::note_asset_delete_is_scoped` |
| M④ | 没有张数上限 | 删掉 `if n >= MAX_ASSETS_PER_NOTE { return Err(...); }` 整块 | `note_asset::tests::save_refuses_an_eleventh_image` |
| M⑤ | 没有大小上限 | 删掉 `if bytes.len() > MAX_IMAGE_BYTES { return Err(...); }` 整块 | `note_asset::tests::save_refuses_oversized_and_empty_images` |
| M⑥ | 只信声明头 | `parse` 里把 `sniff + 不一致判断` 两段换成 `let mime = declared;` | `note_asset::tests::save_rejects_non_image_payloads` |
| M⑦ | 不级联 | `db.rs` V13 DDL 的 ` ON DELETE CASCADE` 删掉 | `note_asset::tests::hard_deleted_note_cascades_its_assets` |
| M⑧ | V13 没进迁移链 | `db.rs` match 里 `13 => V13,` 删掉（落到 `_ => \"\"`） | `db::tests::v13_note_assets_table_and_index_exist` |
| M⑨ | 告警漏截图 | `backup.rs` 告警句删掉「备注图片（二进制，截图内容未加密）、」 | `backup::tests::plaintext_warning_names_every_plain_text_table_since_m9` |

命令一律 `cd src-tauri && cargo test -j 1 --lib <单个筛选词>`；每处还原后：

```bash
git diff --exit-code src-tauri/src/<file>; echo "restore_exit=$?"
sha256sum src-tauri/src/<file>
```

Expected: 每处变异 `exit!=0`；还原后 `restore_exit=0` 且 sha256 前后一致。

- [ ] **Step 3: 真机八格（CDP，一次性 identifier 沙盒）**

隔离口径照 M15/M16-a（HANDOFF 坑 51）：

1. 先对真实数据目录取读数落盘：`ls -l --time-style=full-iso %APPDATA%\dev.zero.pfm\ledger.db{,-wal,-shm}` + `sha256sum`（记前 12 位）。
2. 临时把 `src-tauri/tauri.conf.json` 的 `identifier` 改成 `dev.zero.pfm.m16btest`；证据取完后 `git checkout -- src-tauri/tauri.conf.json` 并按原字节还原。
3. `npm run tauri dev` 由控制方走受管后台任务；CDP 端口 **9333**（9222 被 Qoder 占用）。
4. 进程只按自己记下的 PID 精确回收（`taskkill //PID <pid> //T //F`），绝不按映像名通杀。
5. 跑完删净沙盒目录，再取一次真实目录三项读数，与第 1 步逐字一致才算过；不一致立刻停手报告。

`m16b-cdp.mjs` 的取数口径：只回布尔/条数/字段名/长度/错误码名，不回备注正文、不回截图字节。`waitFor` 必须 try/catch 包 `document.body` 为 null 的瞬间。

| 格 | 动作 | 必须落盘的读数 |
|---|---|---|
| C-1 | 沙盒里建项目+备注，打开详情/台账，展开备注 | `emptyHint=true`、`imgCount=0` |
| C-2 | 编辑该备注，合成 paste 塞一张 canvas PNG | `imgCount=1`、`srcPrefix="data:image/png;base64,"`、`naturalWidth=1`、`dialogNotice` 含「已添加 1 张」 |
| C-3 | 新建备注对话框里合成 paste | 提示文案含「先保存备注」、C-1 那条备注的图片数仍是 1（没有误挂到别的备注） |
| C-4 | 点缩略图删除按钮 | `imgCount=0`、重新展开仍 0 |
| C-5 | 同一备注连塞 11 张 | 前 10 张在；第 11 张界面错误含「最多 10 张」；库内 10 行 |
| C-6 | 建乙区并切过去，打开乙区项目；再切回默认区 | 乙区备注 0 条；默认区原备注与缩略图仍在 |
| C-7 | 直调 `backup_export({path})` → 删图 → `backup_import({path})` | 导入后 `imgCount=1`、`srcPrefix="data:image/png;base64,"`、`schemaVersion=13` |
| C-8 | 直调 `note_asset_list` | 键集合恰为 `id/noteId/mime/size/createdAt/dataUrl`，无 `data`/`blob` 键 |

C-1 的种子数据（页面上下文直调，避免手点）：

```js
const project = await invoke("project_create", {
  input: {
    name: "截图项目", code: null, manager: null, customer: null,
    contactName: null, contactPhone: null, contractNo: null,
    contractPeriod: null, deliveryDeadline: null, status: "立项",
    tags: [], parentId: null, workspaceId: null,
  },
});
await invoke("ledger_note_create", {
  projectId: project.id,
  input: { title: "截图备注", body: "正文", tag: null },
});
```

C-2 的合成 paste（页面上下文；靶子必须**限定在对话框内**，别用全页 `div`）：

```js
const canvas = document.createElement("canvas");
canvas.width = 1;
canvas.height = 1;
const blob = await new Promise((r) => canvas.toBlob(r, "image/png"));
const dt = new DataTransfer();
dt.items.add(new File([blob], "shot.png", { type: "image/png" }));
const target = document.querySelector('[role="dialog"] .grid.gap-3');
target.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
```

C-6 的区切换：

```js
const zone = await invoke("workspace_create", { name: "客户乙" });
await invoke("workspace_switch", { id: zone.id });
// reload 后打开乙区项目；再 workspace_switch({ id: "default" }) 切回
```

C-7 的包落点用 Node 的 `os.tmpdir()` 拼出数据目录之外的路径（如 `path.join(os.tmpdir(), "m16b-backup.zip")`），
跑完把同一份包复制进证据目录留档：

```js
await invoke("backup_export", { path: pkg });
await invoke("note_asset_delete", { id: firstAssetId });
await invoke("backup_import", { path: pkg });
```

跑完脚本：

```bash
node .superpowers/sdd/2026-10-26-m16b-note-images/m16b-cdp.mjs \
  > .superpowers/sdd/2026-10-26-m16b-note-images/m16b-cdp.stdout.txt 2>&1
echo "exit=$?"
```

命名缺口（必须写进 `real-machine-readings.md`）：
- 合成的是 `ClipboardEvent`，证应用内处理器；系统剪贴板 → WebView 事件不自动验。
- C-7 绕开原生文件对话框，验的是数据往返；对话框接线归 M8。

Expected: `M16B-RESULT: ALL-PASS`（八格全过才写；任何一格 pending 就写 `M16B-RESULT: NOT-ALL-PASS`，不许用别的字样冒充）。

- [ ] **Step 4: spec↔计划映射表**

`spec-plan-mapping.md`：spec §一…§十二 每节 → 本计划哪一 Task 哪一 Step → 测试名 / 真机格 / 变异靶。没有落点的句子显式写「本轮不做」或「有名缺口」，不许留空。至少覆盖 D1–D12、T-1…T-11、C-1…C-8、M①…M⑨、§十 九条不做。

- [ ] **Step 5: 证据不入 git**

`.superpowers/` 已在 `.gitignore`；不 `git add -f`。把证据目录路径与关键读数写进 Task 5 的活文档。

---

## Task 5: 活文档回写与收口

**Files:**
- Modify: `docs/技术方案.md`（`note_assets` 行、M8 附件附注、里程碑表 M16-b 行）
- Modify: `docs/开发进度.md`（新增 M16-b 验收证据 + 变更日志）
- Modify: `docs/HANDOFF.md`（§3 快照、§4 门禁串、下一格）

- [ ] **Step 1: `技术方案.md`**

把数据模型里这一行：

```
note_assets         粘贴进来的截图，文件存 app data 附件目录，库里只存相对路径
```

改成：

```
note_assets         粘贴进来的截图（M16-b 新增，V13）：id / note_id→ledger_notes（ON DELETE CASCADE）/ mime /
                    size（解码后字节数）/ data（BLOB）/ created_at —— 图像字节直接存库，**无外部附件目录**、不存路径；
                    只收 PNG/JPEG/WebP（magic bytes）、单张 ≤5 MiB、每备注 ≤10 张；无软删（硬删），软删备注时图片保留。
```

把 M8 段里「附件（截图）单独打包或一并放进 zip —— 定为一并放入」那条附注的旧结论改成：

```
附件（截图）**M16-b 起以 BLOB 存库，随 `backup.db` 整库快照进包**；不建附件目录、zip 条目仍是 `manifest.json` + `backup.db`，`Counts` 仍七键。
```

在里程碑表 `M16-a` 行之后加：

```
| M16-b | 图文笔记与截图：V13 `note_assets`（BLOB、硬删、FK 级联）+ 三条同步命令（79→82）+ 备注展开缩略图/单张删除 + 编辑对话框粘贴截图 | ✅ 覆盖「图文笔记、支持粘贴项目截图」 | 代码 + 9 处变异 + 真机 C-1…C-8（2026-10-26）；`TARGET_VERSION` 12→13、cargo 426 / node 75 / 命令 82 / async 7 / 豁免 1 / `Cargo.lock` 616 / collect 9；证据见 `docs/开发进度.md`「## M16-b 验收证据」 |
```

- [ ] **Step 2: `开发进度.md`**

在 M16-a 段之后新增 `## M16-b 验收证据（图文笔记与截图，…）`，写：分支/提交、V13、三条命令、备份 schema 13、门禁读数（426/75/82/7/13/1/616/9）、九处变异逐条退出码、真机八格逐格读数（含真实目录前后 sha256 一致那句）、命名缺口（合成 paste / 直调备份）。变更日志加一行 `2026-10-26：M16-b …`。

- [ ] **Step 3: `HANDOFF.md`**

- §3 快照：当前分支/提交、M16-b 完成状态、证据目录路径。
- §4-A 门禁串：cargo 426 / node 75 / 命令 82 / async 7 / TARGET 13 / 豁免 1 / Cargo.lock 616 / collect 9。
- §4 下一格：说明 M16-b 已完，下一格待需求方指定；`docs/思维导图.md` 的 M16 三项（版本台账、截图、子文件选择器）全部落地。

- [ ] **Step 4: 提交**

```bash
git add docs/技术方案.md docs/开发进度.md docs/HANDOFF.md
git commit -m "docs(m16b): 活文档回写 —— V13、命令 82、备份 schema 13"
```

---

## Self-Review

**Spec coverage:** §一 → Task 2/3；§二 D1–D12 → Global Constraints + Task 1/2/3；§三 V13 → Task 1；§四命令 → Task 2；§五校验 → Task 2 Step 3；§六工作区 → Task 2 测试 T-7/T-8 + M①–M③；§七备份 → Task 1；§八前端 → Task 3；§九验收 → Task 1/2/4；§十不做 → 无任务（刻意）；§十一回写 → Task 5；§十二风险 → 门禁与真机格。

**Placeholder scan:** 无 TBD/TODO；每个代码步都有完整片段；真机格与控制方变异是逐条动作，不是「补充测试」。

**Type consistency:** `NoteAsset` 六字段、`note_asset::list/save/delete` 三签名、命令名 `note_asset_list/save/delete`、TS 包装同名、`assetsTick`/`reloadKey` 前后一致。`save` 回 `()`，前端只递增计数，不接收整行。
