//! 信息台账：多环境网址、账号凭据、服务器、外部链接、备注。
//!
//! 读接口一律不回传敏感字段明文，只回传「这一列有没有值」；要看明文得单独走
//! [`reveal`]，它按需解密一个字段。这样列表 JSON 里永远不会躺着别人的密码，
//! 日志、DevTools、崩溃转储都少一处泄漏面。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::vault::{self, MasterKey};

const ENVS: [&str; 4] = ["dev", "test", "pre", "prod"];

fn invalid(msg: &str) -> AppError {
    AppError::new("invalid_input", msg, Some("检查表单必填项"))
}

fn not_found(id: &str) -> AppError {
    AppError::new("not_found", &format!("记录不存在：{id}"), Some("刷新后重试"))
}

fn norm(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Env {
    pub id: String,
    pub env: String,
    pub name: String,
    pub url: Option<String>,
    pub port: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub id: String,
    pub env_id: Option<String>,
    pub title: String,
    pub url: Option<String>,
    pub note: Option<String>,
    /// 只告诉前端这一列有没有值；明文走 reveal。
    pub username_set: bool,
    pub password_set: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: String,
    pub name: String,
    pub ip: Option<String>,
    pub bastion: Option<String>,
    pub note: Option<String>,
    pub account_set: bool,
    pub password_set: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub url: String,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    pub tag: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ledger {
    pub envs: Vec<Env>,
    pub credentials: Vec<Credential>,
    pub servers: Vec<Server>,
    pub links: Vec<Link>,
    pub notes: Vec<Note>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EnvInput {
    pub env: String,
    pub name: String,
    pub url: Option<String>,
    pub port: Option<String>,
    pub note: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInput {
    pub env_id: Option<String>,
    pub title: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub url: Option<String>,
    pub note: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServerInput {
    pub name: String,
    pub ip: Option<String>,
    pub bastion: Option<String>,
    pub account: Option<String>,
    pub password: Option<String>,
    pub note: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LinkInput {
    pub kind: String,
    pub title: String,
    pub url: String,
    pub note: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NoteInput {
    pub title: String,
    pub body: Option<String>,
    pub tag: Option<String>,
}

/// 前端传来的定位信息：哪张表的哪一列。写成常量表而不是让前端拼 SQL 片段，
/// 避免把可拼接的表名/列名暴露在 IPC 入参里。
pub const SECRET_FIELDS: [(&str, &str, &str); 4] = [
    ("credential", "username", "username_cipher"),
    ("credential", "password", "password_cipher"),
    ("server", "account", "account_cipher"),
    ("server", "password", "password_cipher"),
];
const SECRET_TABLES: &[(&str, &str)] = &[("credential", "ledger_credentials"), ("server", "ledger_servers")];

fn secret_column(kind: &str, field: &str) -> AppResult<(&'static str, &'static str, &'static str)> {
    for (k, f, col) in SECRET_FIELDS {
        if k == kind && f == field {
            let table = SECRET_TABLES
                .iter()
                .find(|(k2, _)| *k2 == k)
                .map(|(_, t)| *t)
                .expect("SECRET_FIELDS 里的 kind 必然在 SECRET_TABLES 里");
            return Ok((table, f, col));
        }
    }
    // 清单从表本身拼出来：新增敏感列时这里不会漏（漏了就是上面那张表没写，测试会红）。
    let allowed = SECRET_FIELDS
        .iter()
        .map(|(k, f, _)| format!("{k}.{f}"))
        .collect::<Vec<_>>()
        .join(" / ");
    Err(invalid(&format!("「{kind}.{field}」不是可解密的敏感字段，可解密的是 {allowed}")))
}

fn nonce_column(cipher_column: &str) -> String {
    cipher_column.replace("_cipher", "_nonce")
}

/// 一份密文只能拆一次：`params!` 里的表达式按值取用，cipher 与 nonce 得先落到两个局部变量。
fn cipher_cols(
    field: Option<vault::EncryptedField>,
) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    match field {
        Some(f) => (Some(f.cipher), Some(f.nonce)),
        None => (None, None),
    }
}

/// 一列敏感字段的两块料：密文与 nonce，任一为空即「这一列没登记过」。
type CipherCells = (Option<Vec<u8>>, Option<Vec<u8>>);

/// 按需解密一个敏感字段。AAD 用「行 id + 字段名」，与写入时一致。
pub fn reveal(conn: &Connection, mk: &MasterKey, kind: &str, id: &str, field: &str) -> AppResult<String> {
    let (table, col_name, cipher_col) = secret_column(kind, field)?;
    let sql = format!(
        "SELECT {cipher_col}, {} FROM {table} WHERE id = ?1 AND deleted_at IS NULL",
        nonce_column(cipher_col)
    );
    let mut stmt = conn.prepare(&sql)?;
    let cells: Option<CipherCells> = stmt
        .query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok();
    drop(stmt);
    let Some((Some(cipher), Some(nonce))) = cells else {
        return Err(invalid("没有可解密的内容（这一列没登记过，或记录已删除）"));
    };
    let value = vault::decrypt_field(mk, id, col_name, &vault::EncryptedField { cipher, nonce })?;
    Ok(value)
}

fn soft_delete(conn: &Connection, table: &str, id: &str) -> AppResult<()> {
    let changed = conn.execute(
        &format!("UPDATE {table} SET deleted_at = datetime('now') WHERE id = ?1 AND deleted_at IS NULL"),
        [id],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

fn project_exists(conn: &Connection, project_id: &str) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE id = ?1 AND deleted_at IS NULL",
        [project_id],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(not_found(project_id));
    }
    Ok(())
}

pub fn list(conn: &Connection, project_id: &str) -> AppResult<Ledger> {
    let mut envs = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT id, env, name, url, port, note FROM ledger_envs
         WHERE project_id = ?1 AND deleted_at IS NULL ORDER BY CASE env WHEN 'dev' THEN 0 WHEN 'test' THEN 1 WHEN 'pre' THEN 2 ELSE 3 END",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(Env {
            id: r.get(0)?,
            env: r.get(1)?,
            name: r.get(2)?,
            url: r.get(3)?,
            port: r.get(4)?,
            note: r.get(5)?,
        })
    })?;
    for row in rows {
        envs.push(row?);
    }
    drop(stmt);

    let mut credentials = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT id, env_id, title, url, note, username_cipher IS NOT NULL, password_cipher IS NOT NULL
         FROM ledger_credentials WHERE project_id = ?1 AND deleted_at IS NULL ORDER BY created_at",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(Credential {
            id: r.get(0)?,
            env_id: r.get(1)?,
            title: r.get(2)?,
            url: r.get(3)?,
            note: r.get(4)?,
            username_set: r.get(5)?,
            password_set: r.get(6)?,
        })
    })?;
    for row in rows {
        credentials.push(row?);
    }
    drop(stmt);

    let mut servers = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT id, name, ip, bastion, note, account_cipher IS NOT NULL, password_cipher IS NOT NULL
         FROM ledger_servers WHERE project_id = ?1 AND deleted_at IS NULL ORDER BY created_at",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(Server {
            id: r.get(0)?,
            name: r.get(1)?,
            ip: r.get(2)?,
            bastion: r.get(3)?,
            note: r.get(4)?,
            account_set: r.get(5)?,
            password_set: r.get(6)?,
        })
    })?;
    for row in rows {
        servers.push(row?);
    }
    drop(stmt);

    let mut links = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT id, kind, title, url, note FROM ledger_links
         WHERE project_id = ?1 AND deleted_at IS NULL ORDER BY kind, created_at",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(Link {
            id: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            url: r.get(3)?,
            note: r.get(4)?,
        })
    })?;
    for row in rows {
        links.push(row?);
    }
    drop(stmt);

    let mut notes = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT id, title, body, tag FROM ledger_notes
         WHERE project_id = ?1 AND deleted_at IS NULL ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok(Note {
            id: r.get(0)?,
            title: r.get(1)?,
            body: r.get(2)?,
            tag: r.get(3)?,
        })
    })?;
    for row in rows {
        notes.push(row?);
    }
    drop(stmt);

    Ok(Ledger {
        envs,
        credentials,
        servers,
        links,
        notes,
    })
}

pub fn create_env(conn: &Connection, project_id: &str, input: &EnvInput) -> AppResult<Env> {
    project_exists(conn, project_id)?;
    if input.name.trim().is_empty() {
        return Err(invalid("环境名称不能为空"));
    }
    if !ENVS.contains(&input.env.as_str()) {
        return Err(invalid(&format!(
            "环境「{}」不在 dev/test/pre/prod 之内",
            input.env
        )));
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO ledger_envs (id, project_id, env, name, url, port, note)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            id,
            project_id,
            input.env,
            input.name.trim(),
            norm(input.url.clone()),
            norm(input.port.clone()),
            norm(input.note.clone())
        ],
    )?;
    Ok(list(conn, project_id)?
        .envs
        .into_iter()
        .find(|e| e.id == id)
        .expect("刚插入的行一定读得到"))
}

pub fn update_env(conn: &Connection, id: &str, input: &EnvInput) -> AppResult<()> {
    if input.name.trim().is_empty() || !ENVS.contains(&input.env.as_str()) {
        return Err(invalid("环境名称或取值不合法"));
    }
    let changed = conn.execute(
        "UPDATE ledger_envs SET env=?2, name=?3, url=?4, port=?5, note=?6, updated_at=datetime('now')
         WHERE id=?1 AND deleted_at IS NULL",
        params![
            id,
            input.env,
            input.name.trim(),
            norm(input.url.clone()),
            norm(input.port.clone()),
            norm(input.note.clone())
        ],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

pub fn delete_env(conn: &Connection, id: &str) -> AppResult<()> {
    soft_delete(conn, "ledger_envs", id)
}

pub fn create_credential(
    conn: &Connection,
    mk: &MasterKey,
    project_id: &str,
    input: &CredentialInput,
) -> AppResult<Credential> {
    project_exists(conn, project_id)?;
    if input.title.trim().is_empty() {
        return Err(invalid("凭据标题不能为空"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    write_credential(conn, mk, &id, project_id, input)?;
    Ok(list(conn, project_id)?
        .credentials
        .into_iter()
        .find(|c| c.id == id)
        .expect("刚插入的行一定读得到"))
}

/// 敏感列在这里加密后才进库；传空表示清空这一列，而不是保留旧值。
fn write_credential(
    conn: &Connection,
    mk: &MasterKey,
    id: &str,
    project_id: &str,
    input: &CredentialInput,
) -> AppResult<()> {
    let enc_u = norm(input.username.clone())
        .map(|v| vault::encrypt_field(mk, id, "username", &v))
        .transpose()?;
    let enc_p = norm(input.password.clone())
        .map(|v| vault::encrypt_field(mk, id, "password", &v))
        .transpose()?;
    let (u_cipher, u_nonce) = cipher_cols(enc_u);
    let (p_cipher, p_nonce) = cipher_cols(enc_p);
    conn.execute(
        "INSERT INTO ledger_credentials (id, project_id, env_id, title, username_cipher, username_nonce,
                                         password_cipher, password_nonce, url, note)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(id) DO UPDATE SET
           env_id=excluded.env_id, title=excluded.title,
           username_cipher=excluded.username_cipher, username_nonce=excluded.username_nonce,
           password_cipher=excluded.password_cipher, password_nonce=excluded.password_nonce,
           url=excluded.url, note=excluded.note, updated_at=datetime('now')",
        params![
            id,
            project_id,
            input.env_id.clone(),
            input.title.trim(),
            u_cipher,
            u_nonce,
            p_cipher,
            p_nonce,
            norm(input.url.clone()),
            norm(input.note.clone())
        ],
    )?;
    Ok(())
}

/// 更新时前端不再传归属项目，所以 project_id 从原有行读回：写路径是 UPSERT，
/// 少读这一步就会把项目的归属覆盖成空。顺带用它当存在性检查。
pub fn update_credential(
    conn: &Connection,
    mk: &MasterKey,
    id: &str,
    input: &CredentialInput,
) -> AppResult<()> {
    if input.title.trim().is_empty() {
        return Err(invalid("凭据标题不能为空"));
    }
    let project_id: String = conn
        .query_row(
            "SELECT project_id FROM ledger_credentials WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |r| r.get(0),
        )
        .map_err(|_| not_found(id))?;
    write_credential(conn, mk, id, &project_id, input)
}

pub fn delete_credential(conn: &Connection, id: &str) -> AppResult<()> {
    soft_delete(conn, "ledger_credentials", id)
}

pub fn create_server(
    conn: &Connection,
    mk: &MasterKey,
    project_id: &str,
    input: &ServerInput,
) -> AppResult<Server> {
    project_exists(conn, project_id)?;
    if input.name.trim().is_empty() {
        return Err(invalid("服务器名称不能为空"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    write_server(conn, mk, &id, project_id, input)?;
    Ok(list(conn, project_id)?
        .servers
        .into_iter()
        .find(|s| s.id == id)
        .expect("刚插入的行一定读得到"))
}

fn write_server(
    conn: &Connection,
    mk: &MasterKey,
    id: &str,
    project_id: &str,
    input: &ServerInput,
) -> AppResult<()> {
    let enc_a = norm(input.account.clone())
        .map(|v| vault::encrypt_field(mk, id, "account", &v))
        .transpose()?;
    let enc_p = norm(input.password.clone())
        .map(|v| vault::encrypt_field(mk, id, "password", &v))
        .transpose()?;
    let (a_cipher, a_nonce) = cipher_cols(enc_a);
    let (p_cipher, p_nonce) = cipher_cols(enc_p);
    conn.execute(
        "INSERT INTO ledger_servers (id, project_id, name, ip, bastion, account_cipher, account_nonce,
                                     password_cipher, password_nonce, note)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(id) DO UPDATE SET
           name=excluded.name, ip=excluded.ip, bastion=excluded.bastion,
           account_cipher=excluded.account_cipher, account_nonce=excluded.account_nonce,
           password_cipher=excluded.password_cipher, password_nonce=excluded.password_nonce,
           note=excluded.note, updated_at=datetime('now')",
        params![
            id,
            project_id,
            input.name.trim(),
            norm(input.ip.clone()),
            norm(input.bastion.clone()),
            a_cipher,
            a_nonce,
            p_cipher,
            p_nonce,
            norm(input.note.clone())
        ],
    )?;
    Ok(())
}

pub fn update_server(
    conn: &Connection,
    mk: &MasterKey,
    id: &str,
    input: &ServerInput,
) -> AppResult<()> {
    if input.name.trim().is_empty() {
        return Err(invalid("服务器名称不能为空"));
    }
    let project_id: String = conn
        .query_row(
            "SELECT project_id FROM ledger_servers WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |r| r.get(0),
        )
        .map_err(|_| not_found(id))?;
    write_server(conn, mk, id, &project_id, input)
}

pub fn delete_server(conn: &Connection, id: &str) -> AppResult<()> {
    soft_delete(conn, "ledger_servers", id)
}

pub fn create_link(conn: &Connection, project_id: &str, input: &LinkInput) -> AppResult<Link> {
    project_exists(conn, project_id)?;
    if input.title.trim().is_empty() || input.url.trim().is_empty() {
        return Err(invalid("链接标题与地址都不能为空"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO ledger_links (id, project_id, kind, title, url, note) VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            id,
            project_id,
            normalize_link_kind(&input.kind),
            input.title.trim(),
            input.url.trim(),
            norm(input.note.clone())
        ],
    )?;
    Ok(list(conn, project_id)?
        .links
        .into_iter()
        .find(|l| l.id == id)
        .expect("刚插入的行一定读得到"))
}

/// 未知归类落到 other， 而不是拒绝：链接分类是用户随口起的， 卡住表单没有收益。
fn normalize_link_kind(kind: &str) -> String {
    match kind {
        "jira" | "zentao" | "wiki" | "doc" | "repo" => kind.to_owned(),
        _ => "other".to_owned(),
    }
}

pub fn update_link(conn: &Connection, id: &str, input: &LinkInput) -> AppResult<()> {
    if input.title.trim().is_empty() || input.url.trim().is_empty() {
        return Err(invalid("链接标题与地址都不能为空"));
    }
    let changed = conn.execute(
        "UPDATE ledger_links SET kind=?2, title=?3, url=?4, note=?5, updated_at=datetime('now')
         WHERE id=?1 AND deleted_at IS NULL",
        params![
            id,
            normalize_link_kind(&input.kind),
            input.title.trim(),
            input.url.trim(),
            norm(input.note.clone())
        ],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

pub fn delete_link(conn: &Connection, id: &str) -> AppResult<()> {
    soft_delete(conn, "ledger_links", id)
}

pub fn create_note(conn: &Connection, project_id: &str, input: &NoteInput) -> AppResult<Note> {
    project_exists(conn, project_id)?;
    if input.title.trim().is_empty() {
        return Err(invalid("备注标题不能为空"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO ledger_notes (id, project_id, title, body, tag) VALUES (?1,?2,?3,?4,?5)",
        params![
            id,
            project_id,
            input.title.trim(),
            input.body.clone().unwrap_or_default(),
            norm(input.tag.clone())
        ],
    )?;
    Ok(list(conn, project_id)?
        .notes
        .into_iter()
        .find(|n| n.id == id)
        .expect("刚插入的行一定读得到"))
}

pub fn update_note(conn: &Connection, id: &str, input: &NoteInput) -> AppResult<()> {
    if input.title.trim().is_empty() {
        return Err(invalid("备注标题不能为空"));
    }
    let changed = conn.execute(
        "UPDATE ledger_notes SET title=?2, body=?3, tag=?4, updated_at=datetime('now')
         WHERE id=?1 AND deleted_at IS NULL",
        params![id, input.title.trim(), input.body.clone().unwrap_or_default(), norm(input.tag.clone())],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

pub fn delete_note(conn: &Connection, id: &str) -> AppResult<()> {
    soft_delete(conn, "ledger_notes", id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, project};
    use rusqlite::Connection;

    const USER: &str = "zhangwei.admin";
    const PW: &str = "Gov@2026#Passw0rd";
    const PW2: &str = "Gov@2027#Replaced";

    /// 每个用例一套内存库 + 已初始化的主密钥 + 一个项目，避免用例之间互相依赖。
    fn fixture() -> (Connection, String, MasterKey) {
        let conn = db::open_in_memory().unwrap();
        let code = vault::generate_recovery_code().unwrap();
        let mk = vault::initialize(&conn, "主密码-至少八位", &code).unwrap();
        let pid = seed_project(&conn, "政务云迁移");
        (conn, pid, mk)
    }

    fn seed_project(conn: &Connection, name: &str) -> String {
        project::create_project(
            conn,
            &project::ProjectInput {
                name: name.to_owned(),
                code: None,
                manager: None,
                customer: None,
                contact_name: None,
                contact_phone: None,
                contract_no: None,
                contract_period: None,
                delivery_deadline: None,
                status: "立项".to_owned(),
                tags: vec![],
            },
        )
        .unwrap()
        .id
    }

    fn cred_input(title: &str, username: Option<&str>, password: Option<&str>) -> CredentialInput {
        CredentialInput {
            env_id: None,
            title: title.to_owned(),
            username: username.map(str::to_owned),
            password: password.map(str::to_owned),
            url: None,
            note: None,
        }
    }

    fn server_input(name: &str, account: Option<&str>) -> ServerInput {
        ServerInput {
            name: name.to_owned(),
            ip: Some("10.20.30.40".to_owned()),
            bastion: None,
            account: account.map(str::to_owned),
            password: Some(PW.to_owned()),
            note: None,
        }
    }

    fn env_input(env: &str, name: &str) -> EnvInput {
        EnvInput {
            env: env.to_owned(),
            name: name.to_owned(),
            url: None,
            port: None,
            note: None,
        }
    }

    fn link_input(kind: &str, title: &str) -> LinkInput {
        LinkInput {
            kind: kind.to_owned(),
            title: title.to_owned(),
            url: "https://jira.example.gov.cn/browse/PROJ-1".to_owned(),
            note: None,
        }
    }

    fn json(conn: &Connection, pid: &str) -> String {
        serde_json::to_string(&list(conn, pid).unwrap()).unwrap()
    }

    #[test]
    fn list_only_says_whether_a_secret_column_has_a_value() {
        let (conn, pid, mk) = fixture();
        let created =
            create_credential(&conn, &mk, &pid, &cred_input("审批后台", Some(USER), Some(PW)))
                .unwrap();
        assert!(created.username_set && created.password_set);

        let raw = json(&conn, &pid);
        // 列表是前端拿到的东西：这里出现明文，DevTools、日志、崩溃转储就都跟着漏。
        assert!(!raw.contains(USER), "列表 JSON 泄漏了账号明文：{raw}");
        assert!(!raw.contains(PW), "列表 JSON 泄漏了密码明文：{raw}");
        assert!(raw.contains("\"usernameSet\":true"));
    }

    #[test]
    fn plaintext_never_reaches_the_database_file() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open(dir.path()).unwrap();
        let code = vault::generate_recovery_code().unwrap();
        let mk = vault::initialize(&conn, "主密码-至少八位", &code).unwrap();
        let pid = seed_project(&conn, "政务云迁移");
        create_credential(&conn, &mk, &pid, &cred_input("审批后台", Some(USER), Some(PW))).unwrap();
        create_server(&conn, &mk, &pid, &server_input("应用节点 1", Some(USER))).unwrap();

        // WAL 里躺着尚未 checkpoint 的页，两个文件都要查，否则这个断言会假绿。
        let mut bytes = Vec::new();
        for name in ["ledger.db", "ledger.db-wal"] {
            if let Ok(extra) = std::fs::read(dir.path().join(name)) {
                bytes.extend_from_slice(&extra);
            }
        }
        let haystack = String::from_utf8_lossy(&bytes);
        assert!(!haystack.contains(USER), "库文件里出现了账号明文");
        assert!(!haystack.contains(PW), "库文件里出现了密码明文");
    }

    #[test]
    fn reveal_roundtrips_every_secret_field() {
        let (conn, pid, mk) = fixture();
        let c = create_credential(&conn, &mk, &pid, &cred_input("审批后台", Some(USER), Some(PW)))
            .unwrap();
        let s = create_server(&conn, &mk, &pid, &server_input("应用节点 1", Some("root")))
            .unwrap();

        assert_eq!(reveal(&conn, &mk, "credential", &c.id, "username").unwrap(), USER);
        assert_eq!(reveal(&conn, &mk, "credential", &c.id, "password").unwrap(), PW);
        assert_eq!(reveal(&conn, &mk, "server", &s.id, "account").unwrap(), "root");
        assert_eq!(reveal(&conn, &mk, "server", &s.id, "password").unwrap(), PW);
    }

    #[test]
    fn reveal_refuses_anything_outside_the_secret_field_table() {
        let (conn, pid, mk) = fixture();
        let c = create_credential(&conn, &mk, &pid, &cred_input("审批后台", None, Some(PW)))
            .unwrap();
        let n = create_note(&conn, &pid, &NoteInput {
            title: "坑点".to_owned(),
            body: Some("验证码接口有 IP 白名单".to_owned()),
            tag: None,
        })
        .unwrap();

        for (kind, id, field) in [
            ("credential", c.id.as_str(), "title"),
            ("credential", c.id.as_str(), "url"),
            ("note", n.id.as_str(), "body"),
            ("envs", c.id.as_str(), "password"),
        ] {
            let err = reveal(&conn, &mk, kind, id, field).unwrap_err();
            assert_eq!(err.code, "invalid_input", "{kind}.{field} 竟然可解密");
        }
    }

    #[test]
    fn updating_a_credential_replaces_the_ciphertext_in_place() {
        let (conn, pid, mk) = fixture();
        let c = create_credential(&conn, &mk, &pid, &cred_input("审批后台", Some(USER), Some(PW)))
            .unwrap();

        update_credential(&conn, &mk, &c.id, &cred_input("审批后台", Some(USER), Some(PW2)))
            .unwrap();

        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM ledger_credentials", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1, "更新写成了第二行");
        assert_eq!(reveal(&conn, &mk, "credential", &c.id, "password").unwrap(), PW2);
        assert!(!json(&conn, &pid).contains(PW));
    }

    #[test]
    fn clearing_a_secret_column_drops_the_ciphertext() {
        let (conn, pid, mk) = fixture();
        let c = create_credential(&conn, &mk, &pid, &cred_input("审批后台", Some(USER), Some(PW)))
            .unwrap();
        update_credential(&conn, &mk, &c.id, &cred_input("审批后台", None, Some(PW))).unwrap();

        let after = list(&conn, &pid).unwrap().credentials.remove(0);
        assert!(!after.username_set && after.password_set);
        let err = reveal(&conn, &mk, "credential", &c.id, "username").unwrap_err();
        assert_eq!(err.code, "invalid_input");
    }

    #[test]
    fn update_keeps_the_row_in_its_project_and_across_project_lists() {
        let (conn, pid, mk) = fixture();
        let other = seed_project(&conn, "另一件事");
        let c = create_credential(&conn, &mk, &pid, &cred_input("审批后台", Some(USER), Some(PW)))
            .unwrap();

        update_credential(&conn, &mk, &c.id, &cred_input("改名了", Some(USER), Some(PW2))).unwrap();

        // 更新路径若不把原 project_id 读回来，UPSERT 会把归属写空（FK 直接拒绝）或写到别的项目。
        assert_eq!(list(&conn, &pid).unwrap().credentials.len(), 1);
        assert!(list(&conn, &other).unwrap().credentials.is_empty());
        let stored: String = conn
            .query_row(
                "SELECT project_id FROM ledger_credentials WHERE id = ?1",
                [c.id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, pid);
    }

    #[test]
    fn deleted_rows_leave_the_list_and_stop_resolving() {
        let (conn, pid, mk) = fixture();
        let c = create_credential(&conn, &mk, &pid, &cred_input("审批后台", Some(USER), Some(PW)))
            .unwrap();
        delete_credential(&conn, &c.id).unwrap();

        assert!(list(&conn, &pid).unwrap().credentials.is_empty());
        assert_eq!(reveal(&conn, &mk, "credential", &c.id, "password").unwrap_err().code, "invalid_input");
        // 再删一次是「记录不存在」，不是静默成功。
        assert_eq!(delete_credential(&conn, &c.id).unwrap_err().code, "not_found");
    }

    #[test]
    fn env_is_a_closed_set_and_a_substring_is_not_a_member() {
        let (conn, pid, _mk) = fixture();
        for ok in ["dev", "test", "pre", "prod"] {
            assert!(create_env(&conn, &pid, &env_input(ok, "环境")).is_ok(), "{ok} 应合法");
        }
        // 曾经用 "dev|test|pre|prod".contains(输入) 判断，这两个都会通过。
        for bad in ["staging", "dev|test", "prod\n", ""] {
            let err = create_env(&conn, &pid, &env_input(bad, "环境")).unwrap_err();
            assert_eq!(err.code, "invalid_input", "「{bad}」竟然被当成合法环境");
        }
    }

    #[test]
    fn note_title_and_body_keep_their_own_columns() {
        let (conn, pid, _mk) = fixture();
        let n = create_note(
            &conn,
            &pid,
            &NoteInput {
                title: "接口变更".to_owned(),
                body: Some("GET /sso/token 改成 POST".to_owned()),
                tag: Some("对接".to_owned()),
            },
        )
        .unwrap();
        let read = list(&conn, &pid).unwrap().notes.remove(0);
        assert_eq!(read.id, n.id);
        assert_eq!(read.title, "接口变更");
        assert_eq!(read.body, "GET /sso/token 改成 POST");
        assert_eq!(read.tag.as_deref(), Some("对接"));
    }

    #[test]
    fn unknown_link_kind_falls_back_to_other() {
        let (conn, pid, _mk) = fixture();
        assert_eq!(create_link(&conn, &pid, &link_input("wiki", "Wiki")).unwrap().kind, "wiki");
        assert_eq!(create_link(&conn, &pid, &link_input("gitea", "自建 Git")).unwrap().kind, "other");
    }

    #[test]
    fn writes_require_an_existing_project_and_a_valid_env() {
        let (conn, pid, mk) = fixture();
        let err = create_credential(&conn, &mk, "no-such-project", &cred_input("x", None, None))
            .unwrap_err();
        assert_eq!(err.code, "not_found");

        // env_id 由外键兜住：指向不存在的环境应当被拒，而不是留下一条挂不到的关联。
        let bad = CredentialInput {
            env_id: Some("no-such-env".to_owned()),
            ..cred_input("x", None, Some(PW))
        };
        assert!(create_credential(&conn, &mk, &pid, &bad).is_err());
    }
}
