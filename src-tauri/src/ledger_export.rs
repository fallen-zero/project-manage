//! 台账导出（M11-b spec）：把五张台账表摊成「导出行模型」，再由两个渲染器写成 xlsx / Markdown。
//!
//! 红线（spec D3）：**默认路径不读任何密文列** —— `mk` 为 `None` 时下面的 SQL 里根本不出现
//! `*_cipher` / `*_nonce`；只有调用方显式传 `Some(mk)` 才解密（Task 2）。变异 M② 打的就是这条。
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::ledger::ENV_ORDER_SQL;
use crate::vault::{self, EncryptedField, MasterKey};

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查导出参数"))
}

fn not_found(id: &str) -> AppError {
    AppError::new("not_found", &format!("记录不存在：{id}"), Some("刷新后重试"))
}

/// 写盘失败按文件域报（spec §五）：码与文案对齐 `daily_report::write_fail`（`fs_failed` + 可操作 hint）。
/// 两个渲染器共用，模块内私有。
fn write_fail(path: &Path, e: impl std::fmt::Display) -> AppError {
    AppError::new(
        "fs_failed",
        &format!("写 {} 失败：{e}", path.display()),
        Some("确认该路径存在且可读写"),
    )
}

#[derive(Debug)]
pub struct Sheet {
    pub name: &'static str,
    pub headers: &'static [&'static str],
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug)]
pub struct ExportDoc {
    pub project_name: String,
    pub generated_at: String,
    pub included_secrets: bool,
    pub sheets: Vec<Sheet>,
}

/// IPC 回执（spec §四）：`sheets` 恒 5、`rows` 是五表行数合计，供界面念「5 张表 / 共 N 行」。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerExportSummary {
    pub path: String,
    pub sheets: usize,
    pub rows: usize,
    pub included_secrets: bool,
}

/// 五张表的固定顺序与名字（spec §三）。xlsx 的工作表名与 Markdown 的小节名共用这一份。
pub const SHEET_NAMES: [&str; 5] = ["环境", "凭据", "服务器", "链接", "备注"];

const ENV_HEADERS: &[&str] = &["环境", "名称", "地址", "端口", "备注"];
const CRED_HEADERS: &[&str] = &["标题", "所属环境", "账号", "密码", "地址", "备注"];
const SERVER_HEADERS: &[&str] = &["名称", "IP", "堡垒机", "账号", "密码", "备注"];
const LINK_HEADERS: &[&str] = &["类型", "标题", "地址", "备注"];
const NOTE_HEADERS: &[&str] = &["标题", "标签", "正文"];

/// 只收 xlsx / md（spec §四 的第一道前置）。
pub fn format_of(format: &str) -> AppResult<()> {
    match format {
        "xlsx" | "md" => Ok(()),
        other => Err(invalid(&format!(
            "导出格式只支持 xlsx 与 md，收到「{other}」"
        ))),
    }
}

pub fn collect(conn: &Connection, mk: Option<&MasterKey>, project_id: &str) -> AppResult<ExportDoc> {
    let project_name: String = conn
        .query_row(
            "SELECT name FROM projects WHERE id = ?1 AND deleted_at IS NULL",
            [project_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| not_found(project_id))?;
    let generated_at: String = conn.query_row("SELECT datetime('now','localtime')", [], |r| r.get(0))?;

    let envs = collect_envs(conn, project_id)?;
    let credentials = collect_credentials(conn, mk, project_id)?;
    let servers = collect_servers(conn, mk, project_id)?;
    let links = collect_links(conn, project_id)?;
    let notes = collect_notes(conn, project_id)?;

    Ok(ExportDoc {
        project_name,
        generated_at,
        included_secrets: mk.is_some(),
        sheets: vec![
            Sheet { name: SHEET_NAMES[0], headers: ENV_HEADERS, rows: envs },
            Sheet { name: SHEET_NAMES[1], headers: CRED_HEADERS, rows: credentials },
            Sheet { name: SHEET_NAMES[2], headers: SERVER_HEADERS, rows: servers },
            Sheet { name: SHEET_NAMES[3], headers: LINK_HEADERS, rows: links },
            Sheet { name: SHEET_NAMES[4], headers: NOTE_HEADERS, rows: notes },
        ],
    })
}

fn collect_envs(conn: &Connection, project_id: &str) -> AppResult<Vec<Vec<String>>> {
    // `created_at` 只到秒；`id` 是导出可复现性的兜底（`ledger::list` 的既有顺序不带它，导出层补上）。
    let mut stmt = conn.prepare(&format!(
        "SELECT env, name, COALESCE(url,''), COALESCE(port,''), COALESCE(note,'')
           FROM ledger_envs
          WHERE project_id = ?1 AND deleted_at IS NULL
          ORDER BY {ENV_ORDER_SQL}, created_at, id",
    ))?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(vec![r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?])
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 含明文路径的一列：cipher 与 nonce 两格都在才解密；任一为空即「这一列没登记过」，导出空串。
/// 密文坏了由 `vault::decrypt_field` 报 `field_decrypt_failed`，整份 collect 随之失败。
fn decrypt_or_empty(
    mk: &MasterKey,
    row_id: &str,
    field: &str,
    cipher: Option<Vec<u8>>,
    nonce: Option<Vec<u8>>,
) -> AppResult<String> {
    match (cipher, nonce) {
        (Some(cipher), Some(nonce)) => {
            vault::decrypt_field(mk, row_id, field, &EncryptedField { cipher, nonce })
        }
        _ => Ok(String::new()),
    }
}

fn collect_credentials(
    conn: &Connection,
    mk: Option<&MasterKey>,
    project_id: &str,
) -> AppResult<Vec<Vec<String>>> {
    // 默认路径：SQL 里**没有** username_cipher / password_cipher —— 这一条是 D3 的可证伪形态。
    let Some(mk) = mk else {
        let mut stmt = conn.prepare(
            "SELECT c.title, COALESCE(e.name,''), COALESCE(c.url,''), COALESCE(c.note,'')
               FROM ledger_credentials c
               LEFT JOIN ledger_envs e ON e.id = c.env_id
              WHERE c.project_id = ?1 AND c.deleted_at IS NULL
              ORDER BY c.created_at, c.id",
        )?;
        let rows = stmt.query_map([project_id], |r| {
            Ok(vec![
                r.get(0)?,
                r.get(1)?,
                String::new(),
                String::new(),
                r.get(2)?,
                r.get(3)?,
            ])
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        return Ok(out);
    };

    // 显式含明文路径独立一条 SQL（spec D3）：只有调用方传了 mk，密文列才会被读出来。
    // `column` 传逻辑列名（AAD 拼的是 `aad(row_id, column)`，与 ledger::reveal 一致）。
    let mut stmt = conn.prepare(
        "SELECT c.id, c.title, COALESCE(e.name,''), COALESCE(c.url,''), COALESCE(c.note,''),
                c.username_cipher, c.username_nonce, c.password_cipher, c.password_nonce
           FROM ledger_credentials c
           LEFT JOIN ledger_envs e ON e.id = c.env_id
          WHERE c.project_id = ?1 AND c.deleted_at IS NULL
          ORDER BY c.created_at, c.id",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<Vec<u8>>>(5)?,
            r.get::<_, Option<Vec<u8>>>(6)?,
            r.get::<_, Option<Vec<u8>>>(7)?,
            r.get::<_, Option<Vec<u8>>>(8)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, title, env_name, url, note, uc, un, pc, pn) = row?;
        let username = decrypt_or_empty(mk, &id, "username", uc, un)?;
        let password = decrypt_or_empty(mk, &id, "password", pc, pn)?;
        out.push(vec![title, env_name, username, password, url, note]);
    }
    Ok(out)
}

fn collect_servers(
    conn: &Connection,
    mk: Option<&MasterKey>,
    project_id: &str,
) -> AppResult<Vec<Vec<String>>> {
    // 默认路径：同样不出现 account_cipher / password_cipher（D3）。
    let Some(mk) = mk else {
        let mut stmt = conn.prepare(
            "SELECT name, COALESCE(ip,''), COALESCE(bastion,''), COALESCE(note,'')
               FROM ledger_servers
              WHERE project_id = ?1 AND deleted_at IS NULL
              ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([project_id], |r| {
            Ok(vec![
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                String::new(),
                String::new(),
                r.get(3)?,
            ])
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        return Ok(out);
    };

    let mut stmt = conn.prepare(
        "SELECT id, name, COALESCE(ip,''), COALESCE(bastion,''), COALESCE(note,''),
                account_cipher, account_nonce, password_cipher, password_nonce
           FROM ledger_servers
          WHERE project_id = ?1 AND deleted_at IS NULL
          ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<Vec<u8>>>(5)?,
            r.get::<_, Option<Vec<u8>>>(6)?,
            r.get::<_, Option<Vec<u8>>>(7)?,
            r.get::<_, Option<Vec<u8>>>(8)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, name, ip, bastion, note, ac, an, pc, pn) = row?;
        let account = decrypt_or_empty(mk, &id, "account", ac, an)?;
        let password = decrypt_or_empty(mk, &id, "password", pc, pn)?;
        out.push(vec![name, ip, bastion, account, password, note]);
    }
    Ok(out)
}

fn collect_links(conn: &Connection, project_id: &str) -> AppResult<Vec<Vec<String>>> {
    let mut stmt = conn.prepare(
        "SELECT kind, title, url, COALESCE(note,'')
           FROM ledger_links
          WHERE project_id = ?1 AND deleted_at IS NULL
          ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(vec![r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?])
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn collect_notes(conn: &Connection, project_id: &str) -> AppResult<Vec<Vec<String>>> {
    let mut stmt = conn.prepare(
        "SELECT title, COALESCE(tag,''), body
           FROM ledger_notes
          WHERE project_id = ?1 AND deleted_at IS NULL
          ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(vec![r.get(0)?, r.get(1)?, r.get(2)?])
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Excel 的单格文本上限（`rust_xlsxwriter` 会在 `save` 时抛 `MaxStringLengthExceeded`）。
/// 写盘**之前**先查：台账导出是给用户拿走的文件，静默截断一段正文比明确失败更糟；
/// 而把它落成 `fs_failed` 会让 hint 指向「检查路径可读写」，方向是错的。
const EXCEL_CELL_LIMIT: usize = 32_767;

fn check_cell_lengths(doc: &ExportDoc) -> AppResult<()> {
    for sheet in &doc.sheets {
        for (i, row) in sheet.rows.iter().enumerate() {
            for cell in row {
                if cell.chars().count() > EXCEL_CELL_LIMIT {
                    return Err(AppError::new(
                        "invalid_input",
                        &format!(
                            "「{}」第 {} 行的单元格超过 Excel 的 {EXCEL_CELL_LIMIT} 字符上限",
                            sheet.name,
                            i + 1
                        ),
                        Some("改用 Markdown 导出（没有这个上限），或先把那一段拆短"),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// 把 `ExportDoc` 写成单文件五工作表的 `.xlsx`：表头加粗、首行冻结、正文换行保留（spec D2/D6）。
/// 表名与列头一律取 `doc.sheets`（源头是 `SHEET_NAMES` / `*_HEADERS`），渲染器不另抄一份名字。
pub fn write_xlsx(path: &Path, doc: &ExportDoc) -> AppResult<()> {
    use rust_xlsxwriter::{Format, Workbook};

    check_cell_lengths(doc)?;
    let mut wb = Workbook::new();
    let bold = Format::new().set_bold();
    // 「备注」的正文带换行；只有 wrap 格式才会让 Excel 把 `\n` 显示成多行，值本身原样写进单元格。
    let wrap = Format::new().set_text_wrap();
    for sheet in &doc.sheets {
        let ws = wb.add_worksheet();
        ws.set_name(sheet.name).map_err(|e| write_fail(path, e))?;
        ws.set_freeze_panes(1, 0).map_err(|e| write_fail(path, e))?;
        for (col, head) in sheet.headers.iter().enumerate() {
            ws.write_with_format(0, col as u16, *head, &bold)
                .map_err(|e| write_fail(path, e))?;
        }
        for (i, row) in sheet.rows.iter().enumerate() {
            let r = (i + 1) as u32;
            for (col, cell) in row.iter().enumerate() {
                ws.write_with_format(r, col as u16, cell.as_str(), &wrap)
                    .map_err(|e| write_fail(path, e))?;
            }
        }
    }
    wb.save(path).map_err(|e| write_fail(path, e))?;
    Ok(())
}

/// GFM 表格单元格转义（spec D9）：`|` 会截断单元格、换行会截断表格行，两者都必须在
/// 渲染时变成表格能承载的形态；`\r` 直接丢掉（库里存的是 `\n`，这里只防手改库带进 CRLF）。
pub fn escape_md(s: &str) -> String {
    // 反斜杠必须**最先**翻倍：否则输入里的 `\|` 经管道替换后是 `\\|`，
    // GFM 会把 `\\` 读成「转义的反斜杠」，后面的 `|` 重新变回单元格分隔符（整行多一格、行尾格丢失）。
    // 顺序是承重的：先转 `|` 会把我们新加的反斜杠再翻倍。
    s.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\r', "")
        .replace('\n', "<br>")
}

/// 把 `ExportDoc` 写成单文件五小节的 Markdown（spec D9）：标题、导出时间、含明文标记，
/// 五节各一张 GFM 表；空表写「（无记录）」。小节名与列头一律取 `doc.sheets`。
pub fn write_markdown(path: &Path, doc: &ExportDoc) -> AppResult<()> {
    let mut out = String::new();
    out.push_str(&format!("# 台账 — {}\n\n", doc.project_name));
    out.push_str(&format!("- 导出时间：{}\n", doc.generated_at));
    // spec D9：这一行是 D3「含明文路径」的文件内回显 —— 拿到这份文件的人，不回头问也看得出
    // 里面有没有明文，故不随之省略。
    out.push_str(&format!(
        "- 含明文：{}\n\n",
        if doc.included_secrets { "是" } else { "否" }
    ));
    for sheet in &doc.sheets {
        out.push_str(&format!("## {}\n\n", sheet.name));
        if sheet.rows.is_empty() {
            out.push_str("（无记录）\n\n");
            continue;
        }
        out.push_str("| ");
        out.push_str(
            &sheet
                .headers
                .iter()
                .map(|h| escape_md(h))
                .collect::<Vec<_>>()
                .join(" | "),
        );
        out.push_str(" |\n|");
        out.push_str(&sheet.headers.iter().map(|_| " --- ").collect::<Vec<_>>().join("|"));
        out.push_str("|\n");
        for row in &sheet.rows {
            out.push_str("| ");
            out.push_str(&row.iter().map(|c| escape_md(c)).collect::<Vec<_>>().join(" | "));
            out.push_str(" |\n");
        }
        out.push('\n');
    }
    std::fs::write(path, out).map_err(|e| AppError::io(path, &e))
}

/// 按 `format` 分派到两个渲染器，并回执（spec §四）。**这里不判落点**：落点守卫在命令层、
/// 且早于本函数（spec §六：判据只在命令层落一次）。`format_of` 在这里再校一次是给公开入口
/// 自带边界 —— 命令层先校验是为了「格式不合法时连数据目录都不碰」，两层判据同一份。
pub fn write(path: &str, format: &str, doc: &ExportDoc) -> AppResult<LedgerExportSummary> {
    format_of(format)?;
    let target = Path::new(path);
    match format {
        "xlsx" => write_xlsx(target, doc)?,
        _ => write_markdown(target, doc)?,
    }
    Ok(LedgerExportSummary {
        path: path.to_owned(),
        sheets: doc.sheets.len(),
        rows: doc.sheets.iter().map(|s| s.rows.len()).sum(),
        included_secrets: doc.included_secrets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::ledger;

    fn fixture() -> (Connection, String) {
        let conn = db::open_in_memory().unwrap();
        let pid = crate::project::create_project(
            &conn,
            &crate::project::ProjectInput {
                name: "政务云迁移".to_owned(),
                code: Some("GOV-01".to_owned()),
                manager: None,
                customer: None,
                contact_name: None,
                contact_phone: None,
                contract_no: None,
                contract_period: None,
                delivery_deadline: None,
                status: "开发".to_owned(),
                tags: vec![],
            },
        )
        .unwrap()
        .id;
        (conn, pid)
    }

    /// 在 fixture 之上初始化主密钥（凭据/服务器的密文列需要它）。
    fn fixture_with_secret() -> (Connection, String, MasterKey) {
        let (conn, pid) = fixture();
        let code = crate::vault::generate_recovery_code().unwrap();
        let mk = crate::vault::initialize(&conn, "主密码-至少八位", &code).unwrap();
        (conn, pid, mk)
    }

    #[test]
    fn collect_always_returns_five_sheets_in_fixed_order() {
        let (conn, pid, mk) = fixture_with_secret();
        let env = ledger::create_env(&conn, &pid, &ledger::EnvInput {
            env: "dev".to_owned(),
            name: "开发环境".to_owned(),
            url: Some("https://dev.example.gov.cn".to_owned()),
            port: Some("8080".to_owned()),
            note: Some("环境备注".to_owned()),
        })
        .unwrap();
        ledger::create_credential(&conn, &mk, &pid, &ledger::CredentialInput {
            env_id: Some(env.id),
            title: "数据库密码".to_owned(),
            username: Some("gov-admin".to_owned()),
            password: Some("S3cret-值".to_owned()),
            url: Some("https://db.example.gov.cn".to_owned()),
            note: Some("凭据备注".to_owned()),
        })
        .unwrap();
        // 第二条凭据故意不挂环境：LEFT JOIN（而非 INNER JOIN）才会保留它。
        ledger::create_credential(&conn, &mk, &pid, &ledger::CredentialInput {
            env_id: None,
            title: "无环境的账号".to_owned(),
            username: Some("散账号".to_owned()),
            password: Some("N0-env-密".to_owned()),
            url: Some("https://orphan.example.gov.cn".to_owned()),
            note: Some("无环境备注".to_owned()),
        })
        .unwrap();
        ledger::create_server(&conn, &mk, &pid, &ledger::ServerInput {
            name: "应用节点".to_owned(),
            ip: Some("10.0.0.8".to_owned()),
            bastion: Some("跳板机".to_owned()),
            account: Some("root".to_owned()),
            password: Some("P@ss-服务器".to_owned()),
            note: Some("服务器备注".to_owned()),
        })
        .unwrap();
        ledger::create_link(&conn, &pid, &ledger::LinkInput {
            kind: "repo".to_owned(),
            title: "代码仓库".to_owned(),
            url: "https://git.example.gov.cn".to_owned(),
            note: Some("链接备注".to_owned()),
        })
        .unwrap();
        ledger::create_note(&conn, &pid, &ledger::NoteInput {
            title: "上线检查单".to_owned(),
            body: Some("上线检查单正文".to_owned()),
            tag: Some("发布".to_owned()),
        })
        .unwrap();

        let doc = collect(&conn, None, &pid).unwrap();

        let names: Vec<&str> = doc.sheets.iter().map(|s| s.name).collect();
        assert_eq!(names, ["环境", "凭据", "服务器", "链接", "备注"], "五张表的名字与顺序是线格式契约");
        let headers: Vec<Vec<&str>> = doc.sheets.iter().map(|s| s.headers.to_vec()).collect();
        assert_eq!(
            headers,
            vec![
                vec!["环境", "名称", "地址", "端口", "备注"],
                vec!["标题", "所属环境", "账号", "密码", "地址", "备注"],
                vec!["名称", "IP", "堡垒机", "账号", "密码", "备注"],
                vec!["类型", "标题", "地址", "备注"],
                vec!["标题", "标签", "正文"],
            ],
            "五组列头逐字（5/6/6/4/3 列），是线格式契约"
        );
        assert_eq!(
            doc.sheets[1].rows.len(),
            2,
            "凭据表应有 2 行：一条挂环境 + 一条 env_id 为 None，INNER JOIN 会少一行"
        );
        for idx in [0, 2, 3, 4] {
            assert_eq!(doc.sheets[idx].rows.len(), 1, "{} 表应有一行种子数据", doc.sheets[idx].name);
        }
        for sheet in &doc.sheets {
            for (i, row) in sheet.rows.iter().enumerate() {
                assert_eq!(
                    row.len(),
                    sheet.headers.len(),
                    "行宽必须等于列头数：{} 第 {} 行",
                    sheet.name,
                    i + 1
                );
            }
        }
        assert_eq!(
            doc.sheets[0].rows[0],
            ["dev", "开发环境", "https://dev.example.gov.cn", "8080", "环境备注"]
        );
        // 两条凭据同秒落库时 `ORDER BY created_at, id` 的先后由随机 id 决定，故按标题定位。
        let cred_rows = &doc.sheets[1].rows;
        let linked_idx = cred_rows
            .iter()
            .position(|r| r[0] == "数据库密码")
            .expect("挂环境的凭据在导出行里");
        assert_eq!(
            cred_rows[linked_idx],
            ["数据库密码", "开发环境", "", "", "https://db.example.gov.cn", "凭据备注"],
            "所属环境按 env_id 关联出环境名（LEFT JOIN 保留）；D3 默认账号/密码留空串"
        );
        let orphan_idx = cred_rows
            .iter()
            .position(|r| r[0] == "无环境的账号")
            .expect("env_id 为 None 的凭据也必须导出（LEFT JOIN 语义）");
        assert_eq!(
            cred_rows[orphan_idx][1], "",
            "env_id 为 NULL 时「所属环境」是空串 —— LEFT JOIN 与 INNER JOIN 的分水岭"
        );
        assert_eq!(
            (cred_rows[orphan_idx][2].as_str(), cred_rows[orphan_idx][3].as_str()),
            ("", ""),
            "D3 默认账号/密码留空串（无环境凭据同样适用）"
        );
        assert_eq!(
            cred_rows[orphan_idx],
            ["无环境的账号", "", "", "", "https://orphan.example.gov.cn", "无环境备注"],
            "env_id 为 NULL 的行其余列照常导出"
        );
        assert_eq!(
            doc.sheets[2].rows[0],
            ["应用节点", "10.0.0.8", "跳板机", "", "", "服务器备注"],
            "D3 默认账号/密码留空串"
        );
        assert_eq!(
            doc.sheets[3].rows[0],
            ["repo", "代码仓库", "https://git.example.gov.cn", "链接备注"]
        );
        assert_eq!(doc.sheets[4].rows[0], ["上线检查单", "发布", "上线检查单正文"]);
        assert!(!doc.included_secrets, "默认路径不算含明文");
        assert_eq!(doc.project_name, "政务云迁移");
    }

    #[test]
    fn collect_orders_envs_dev_test_pre_prod_regardless_of_insert_order() {
        let (conn, pid) = fixture();
        for env in ["prod", "dev", "test", "pre"] {
            ledger::create_env(&conn, &pid, &ledger::EnvInput {
                env: env.to_owned(),
                name: format!("{env} 环境"),
                url: None,
                port: None,
                note: None,
            })
            .unwrap();
        }
        let doc = collect(&conn, None, &pid).unwrap();
        let envs: Vec<&str> = doc.sheets[0].rows.iter().map(|r| r[0].as_str()).collect();
        assert_eq!(envs, vec!["dev", "test", "pre", "prod"], "排序复用 ledger::list 的同一套 CASE");
    }

    #[test]
    fn collect_skips_soft_deleted_rows() {
        let (conn, pid, mk) = fixture_with_secret();
        let env = ledger::create_env(&conn, &pid, &ledger::EnvInput {
            env: "dev".to_owned(),
            name: "要删的环境".to_owned(),
            url: None,
            port: None,
            note: None,
        })
        .unwrap();
        let cred = ledger::create_credential(&conn, &mk, &pid, &ledger::CredentialInput {
            env_id: Some(env.id.clone()),
            title: "要删的凭据".to_owned(),
            username: None,
            password: None,
            url: None,
            note: None,
        })
        .unwrap();
        let server = ledger::create_server(&conn, &mk, &pid, &ledger::ServerInput {
            name: "要删的服务器".to_owned(),
            ip: None,
            bastion: None,
            account: None,
            password: None,
            note: None,
        })
        .unwrap();
        let link = ledger::create_link(&conn, &pid, &ledger::LinkInput {
            kind: "other".to_owned(),
            title: "要删的链接".to_owned(),
            url: "https://example.gov.cn".to_owned(),
            note: None,
        })
        .unwrap();
        let note = ledger::create_note(&conn, &pid, &ledger::NoteInput {
            title: "要删的备注".to_owned(),
            body: None,
            tag: None,
        })
        .unwrap();

        ledger::delete_env(&conn, &env.id).unwrap();
        ledger::delete_credential(&conn, &cred.id).unwrap();
        ledger::delete_server(&conn, &server.id).unwrap();
        ledger::delete_link(&conn, &link.id).unwrap();
        ledger::delete_note(&conn, &note.id).unwrap();

        let doc = collect(&conn, None, &pid).unwrap();
        for sheet in &doc.sheets {
            assert!(sheet.rows.is_empty(), "软删行不导（D4）：{}", sheet.name);
        }
    }

    #[test]
    fn collect_without_secrets_leaves_secret_cells_empty() {
        let (conn, pid, mk) = fixture_with_secret();
        ledger::create_credential(&conn, &mk, &pid, &ledger::CredentialInput {
            env_id: None,
            title: "运维账号".to_owned(),
            username: Some("gov-admin".to_owned()),
            password: Some("S3cret-值".to_owned()),
            url: None,
            note: None,
        })
        .unwrap();
        let doc = collect(&conn, None, &pid).unwrap();
        let row = &doc.sheets[1].rows[0];
        assert_eq!(row[2], "", "默认路径账号列留空");
        assert_eq!(row[3], "", "默认路径密码列留空");
        assert!(!doc.included_secrets);
    }

    #[test]
    fn collect_with_secrets_decrypts_and_matches_reveal() {
        let (conn, pid, mk) = fixture_with_secret();
        let cred = ledger::create_credential(&conn, &mk, &pid, &ledger::CredentialInput {
            env_id: None,
            title: "运维账号".to_owned(),
            username: Some("gov-admin".to_owned()),
            password: Some("S3cret-值".to_owned()),
            url: None,
            note: None,
        })
        .unwrap();
        let doc = collect(&conn, Some(&mk), &pid).unwrap();
        let row = &doc.sheets[1].rows[0];
        assert_eq!(row[2], ledger::reveal(&conn, &mk, "credential", &cred.id, "username").unwrap());
        assert_eq!(row[3], ledger::reveal(&conn, &mk, "credential", &cred.id, "password").unwrap());
        assert!(doc.included_secrets);
    }

    #[test]
    fn collect_with_secrets_decrypts_server_account_and_password() {
        let (conn, pid, mk) = fixture_with_secret();
        let srv = ledger::create_server(&conn, &mk, &pid, &ledger::ServerInput {
            name: "跳板机".to_owned(),
            ip: Some("10.0.0.8".to_owned()),
            bastion: None,
            account: Some("root".to_owned()),
            password: Some("P@ss-服务器".to_owned()),
            note: None,
        })
        .unwrap();
        let doc = collect(&conn, Some(&mk), &pid).unwrap();
        let row = &doc.sheets[2].rows[0];
        assert_eq!(row[3], ledger::reveal(&conn, &mk, "server", &srv.id, "account").unwrap());
        assert_eq!(row[4], ledger::reveal(&conn, &mk, "server", &srv.id, "password").unwrap());
    }

    #[test]
    fn collect_with_secrets_keeps_empty_secret_columns_empty() {
        let (conn, pid, mk) = fixture_with_secret();
        ledger::create_credential(&conn, &mk, &pid, &ledger::CredentialInput {
            env_id: None,
            title: "只有标题".to_owned(),
            username: None,
            password: None,
            url: None,
            note: None,
        })
        .unwrap();
        let doc = collect(&conn, Some(&mk), &pid).unwrap();
        assert_eq!(doc.sheets[1].rows[0][2], "");
        assert_eq!(doc.sheets[1].rows[0][3], "");
    }

    #[test]
    fn collect_returns_field_decrypt_failed_on_tampered_cipher() {
        let (conn, pid, mk) = fixture_with_secret();
        let cred = ledger::create_credential(&conn, &mk, &pid, &ledger::CredentialInput {
            env_id: None,
            title: "会被改坏".to_owned(),
            username: Some("u".to_owned()),
            password: Some("p".to_owned()),
            url: None,
            note: None,
        })
        .unwrap();
        conn.execute(
            "UPDATE ledger_credentials SET password_cipher = X'00' WHERE id = ?1",
            [&cred.id],
        )
        .unwrap();
        let err = collect(&conn, Some(&mk), &pid).unwrap_err();
        assert_eq!(err.code, "field_decrypt_failed", "密文坏掉要整份失败，不许静默导出半份");
    }

    fn read_sheets(path: &std::path::Path) -> Vec<(String, Vec<Vec<String>>)> {
        use calamine::Reader as _;
        let mut book = calamine::open_workbook_auto(path).unwrap();
        let names = book.sheet_names().to_vec();
        names
            .into_iter()
            .map(|n| {
                let range = book.worksheet_range(&n).unwrap();
                let rows = range
                    .rows()
                    .map(|r| r.iter().map(|c| c.to_string()).collect::<Vec<String>>())
                    .collect::<Vec<Vec<String>>>();
                (n, rows)
            })
            .collect()
    }

    #[test]
    fn write_xlsx_emits_five_worksheets_with_headers_and_multiline_note() {
        let (conn, pid) = fixture();
        ledger::create_note(&conn, &pid, &ledger::NoteInput {
            title: "上线步骤".to_owned(),
            body: Some("第一步\n第二步".to_owned()),
            tag: Some("运维".to_owned()),
        })
        .unwrap();
        let doc = collect(&conn, None, &pid).unwrap();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("m11b-xlsx-{}.xlsx", std::process::id()));
        write_xlsx(&path, &doc).unwrap();
        let sheets = read_sheets(&path);
        let names: Vec<String> = sheets.iter().map(|(n, _)| n.clone()).collect();
        assert_eq!(names, vec!["环境", "凭据", "服务器", "链接", "备注"]);
        assert_eq!(sheets[3].1[0], vec!["类型", "标题", "地址", "备注"], "表头逐字是契约");
        let note = &sheets[4].1[1];
        assert_eq!(note[0], "上线步骤");
        assert_eq!(note[2], "第一步\n第二步", "多行正文要连换行一起写进单元格");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn write_xlsx_with_zero_rows_still_writes_headers() {
        let (conn, pid) = fixture();
        let doc = collect(&conn, None, &pid).unwrap();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("m11b-xlsx-empty-{}.xlsx", std::process::id()));
        write_xlsx(&path, &doc).unwrap();
        let sheets = read_sheets(&path);
        assert_eq!(sheets.len(), 5);
        assert_eq!(sheets[0].1.len(), 1, "零行也要有表头一行");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn write_xlsx_refuses_cells_over_excel_limit_naming_the_sheet() {
        // 恰好 32767 个「长」字：字符数在上限内、UTF-8 字节数约 98KB。
        // 能写成功同时证明「32767 通过」与「按字符而非字节计」（误用 len() 会拒收）。
        let (conn, pid) = fixture();
        ledger::create_note(&conn, &pid, &ledger::NoteInput {
            title: "临界正文".to_owned(),
            body: Some("长".repeat(EXCEL_CELL_LIMIT)),
            tag: None,
        })
        .unwrap();
        let doc = collect(&conn, None, &pid).unwrap();
        let ok_path =
            std::env::temp_dir().join(format!("m11b-xlsx-limit-ok-{}.xlsx", std::process::id()));
        write_xlsx(&ok_path, &doc).expect("恰好 32767 个字符必须写成功");
        std::fs::remove_file(&ok_path).ok();

        // 新库里的 32768 是第一行数据：拒收并点名表与数据行号。
        let (conn, pid) = fixture();
        ledger::create_note(&conn, &pid, &ledger::NoteInput {
            title: "超长正文".to_owned(),
            body: Some("长".repeat(EXCEL_CELL_LIMIT + 1)),
            tag: None,
        })
        .unwrap();
        let doc = collect(&conn, None, &pid).unwrap();
        let path = std::env::temp_dir().join(format!("m11b-xlsx-long-{}.xlsx", std::process::id()));
        let err = write_xlsx(&path, &doc).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("备注"), "要点名是哪张表：{}", err.message);
        assert!(err.message.contains("第 1 行"), "要点名数据行号：{}", err.message);
        assert!(err.hint.as_deref().unwrap_or("").contains("Markdown"), "要给出路：{:?}", err.hint);
        assert!(!path.exists(), "拒收时不许留下半份文件");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn write_markdown_escapes_pipe_and_converts_newlines() {
        // I-1：反斜杠必须先翻倍，否则 `a\|b` 会输出 `a\\|b`，GFM 把 `\\` 当转义的反斜杠、
        // 后面的 `|` 重新成为单元格分隔符。逐字比，顺序颠倒（先转 `|`）也过不了这条。
        assert_eq!(escape_md(r"a\|b"), r"a\\\|b", "反斜杠要先转义，再转管道");
        let (conn, pid) = fixture();
        ledger::create_note(&conn, &pid, &ledger::NoteInput {
            title: "含|竖线".to_owned(),
            body: Some("第一行\r\n第二行".to_owned()),
            tag: None,
        })
        .unwrap();
        let doc = collect(&conn, None, &pid).unwrap();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("m11b-md-{}.md", std::process::id()));
        write_markdown(&path, &doc).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# 台账 — "), "标题行");
        assert!(text.contains("- 导出时间："), "导出时间行");
        assert!(text.contains("- 含明文：否"), "默认路径要在文件内回显「含明文：否」");
        for name in SHEET_NAMES {
            assert!(text.contains(&format!("## {name}")), "五个小节都要在：缺「{name}」");
        }
        assert!(text.contains("含\\|竖线"), "单元格里的 | 必须转义：{text}");
        assert!(text.contains("第一行<br>第二行"), "CRLF 要折成单个 <br>");
        assert!(!text.contains('\r'), "\\r 必须清掉：{text}");
        std::fs::remove_file(&path).ok();

        // D9 的「含明文」是 D3 的文件内回显：含明文路径写「是」，同一支测试内换个临时文件名再验一次。
        let (conn, pid, mk) = fixture_with_secret();
        let doc = collect(&conn, Some(&mk), &pid).unwrap();
        let path = dir.join(format!("m11b-md-secret-{}.md", std::process::id()));
        write_markdown(&path, &doc).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("- 含明文：是"), "含明文路径要在文件内回显「含明文：是」");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn format_of_accepts_only_xlsx_and_md() {
        assert!(format_of("xlsx").is_ok());
        assert!(format_of("md").is_ok());
        let err = format_of("csv").unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("xlsx") && err.message.contains("md"), "点名两个合法值：{}", err.message);
        // N-5：`write` 的分派本身也要有牙 —— 分派先过 `format_of`，在任何写盘之前就拒；
        // 落点指向不存在的目录，拒绝与否与路径无关。
        let (conn, pid) = fixture();
        let doc = collect(&conn, None, &pid).unwrap();
        let err = write("/nonexistent-dir/x.csv", "csv", &doc).unwrap_err();
        assert_eq!(err.code, "invalid_input", "非法格式不许进任何渲染器");
    }

    #[test]
    fn write_markdown_marks_empty_sheet_as_no_records() {
        let (conn, pid) = fixture();
        let doc = collect(&conn, None, &pid).unwrap();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("m11b-md-empty-{}.md", std::process::id()));
        write_markdown(&path, &doc).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("（无记录）").count(), 5, "五张空表各写一次：{text}");
        std::fs::remove_file(&path).ok();
    }
}
