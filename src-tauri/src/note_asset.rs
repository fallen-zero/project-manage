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
        let data_url = format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(data)
        );
        Ok(NoteAsset {
            id: r.get(0)?,
            note_id: r.get(1)?,
            size: r.get(3)?,
            created_at: r.get(4)?,
            mime,
            data_url,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
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
