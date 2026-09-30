//! 主密钥与列级加密。
//!
//! 需求里「软件开机主密码解锁 + 敏感字段脱敏」落到这套结构上：
//! 32 字节主密钥 MK 随机生成、从不明文落盘；MK 同时被两把钥匙各包裹一份存
//! `vault_meta` —— 主密码派生的 KEK 与一次性恢复码派生的 KEK。解锁时两枚包裹
//! 都试一遍，能解开 MK 就算通过，因此不需要单独存密码哈希（多存一份哈希就多
//! 一处可被离线爆破的靶子）。
//!
//! 刻意不引入 DPAPI：它的产物绑定机器与用户 profile，重装系统即不可解，而需求
//! 没有免密诉求。代价是每次开机要输主密码，这是明确选过的取舍。

use argon2::{Algorithm, Argon2, Params, Version};
use rusqlite::Connection;
use serde::Serialize;

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit};

use crate::error::{AppError, AppResult};

/// `Aes256Gcm = AesGcm<Aes256, U12>`， nonce 尺寸由 cipher 决定， 这里显式写死避免每处标注。
type GcmNonce = aes_gcm::Nonce<aes_gcm::aead::consts::U12>;

const MK_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const SALT_LEN: usize = 16;
/// 恢复码取 10 字节 = 80 位熵，够长且手抄得完。
const RECOVERY_BYTES: usize = 10;
/// 去掉 I/O/0/1 这些在屏幕和纸面上容易看错的字符。
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// 主密钥。解锁后驻留在应用状态里， 全程不以明文进过数据库。
pub struct MasterKey([u8; MK_LEN]);

/// 手写 Debug 而不是 derive：derive 会把 32 字节密钥打进日志与 panic 输出，
/// 而「显示明文不写日志」是这条链路上明确的约束。
impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MasterKey(<已隐藏>)")
    }
}

/// 一列密文。库里对应 `*_cipher` 与 `*_nonce` 两个 BLOB 列。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedField {
    pub cipher: Vec<u8>,
    pub nonce: Vec<u8>,
}

fn vault_err(code: &'static str, msg: &str, hint: Option<&str>) -> AppError {
    AppError::new(code, msg, hint)
}

fn random_bytes<const N: usize>() -> AppResult<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf)
        .map_err(|e| vault_err("rng_failed", &format!("生成随机数失败：{e}"), None))?;
    Ok(buf)
}

/// 表里有没有 vault_meta 那一行 —— 决定首启是走「设置主密码」还是「解锁」。
pub fn is_initialized(conn: &Connection) -> AppResult<bool> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM vault_meta", [], |r| r.get(0))?;
    Ok(n > 0)
}

pub fn generate_recovery_code() -> AppResult<String> {
    let bytes = random_bytes::<RECOVERY_BYTES>()?;
    // 每 5 位一个字符：80 位正好 16 个字符，按 4 个一组分开方便抄写
    let mut chars = String::new();
    let mut acc: u32 = 0;
    let mut bits = 0;
    for b in bytes {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            chars.push(CODE_ALPHABET[((acc >> bits) & 0x1f) as usize] as char);
        }
    }
    Ok(chars
        .as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join("-"))
}

/// Argon2id 派生 KEK。参数存进库里，换机器恢复时要用同一组参数才能重算出同样的密钥。
fn derive_kek(secret: &str, salt: &[u8], m_cost: u32, t_cost: u32, p_cost: u32) -> AppResult<[u8; MK_LEN]> {
    let params = Params::new(m_cost, t_cost, p_cost, Some(MK_LEN)).map_err(|e| {
        vault_err("kdf_params_invalid", &format!("KDF 参数不合法：{e}"), None)
    })?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = [0u8; MK_LEN];
    argon
        .hash_password_into(secret.as_bytes(), salt, &mut out)
        .map_err(|e| vault_err("kdf_failed", &format!("密钥派生失败：{e}"), None))?;
    Ok(out)
}

/// nonce 长度必须正好 12 字节。不用 `GcmNonce::from_slice`：它在长度不符时 panic，
/// 而 nonce 有一路是从库里读回来的（手改过的库、半截写入都会给出错误长度），
/// 解密路径不该被一个坏字段打崩整个进程。
fn nonce_of(code: &'static str, bytes: &[u8]) -> AppResult<GcmNonce> {
    GcmNonce::try_from(bytes).map_err(|_| {
        vault_err(
            code,
            &format!("nonce 长度应为 {NONCE_LEN} 字节，实际 {} 字节", bytes.len()),
            Some("该字段可能来自其它版本或被手工改动，必要时从备份包恢复"),
        )
    })
}

fn wrap(kek: &[u8; MK_LEN], mk: &[u8; MK_LEN], aad: &[u8]) -> AppResult<EncryptedField> {
    let cipher = Aes256Gcm::new_from_slice(kek).expect("KEK 长度固定为 32 字节");
    let nonce_bytes = random_bytes::<NONCE_LEN>()?;
    let nonce = nonce_of("vault_wrap_failed", &nonce_bytes)?;
    let body = cipher
        .encrypt(&nonce, Payload { msg: mk.as_slice(), aad })
        .map_err(|e| vault_err("vault_wrap_failed", &format!("主密钥包裹失败：{e}"), None))?;
    Ok(EncryptedField {
        cipher: body,
        nonce: nonce_bytes.to_vec(),
    })
}

fn unwrap(kek: &[u8; MK_LEN], field: &EncryptedField, aad: &[u8]) -> Option<[u8; MK_LEN]> {
    let cipher = Aes256Gcm::new_from_slice(kek).ok()?;
    let nonce = nonce_of("vault_bad_secret", &field.nonce).ok()?;
    let plain = cipher
        .decrypt(&nonce, Payload { msg: field.cipher.as_slice(), aad })
        .ok()?;
    plain.try_into().ok()
}

const AAD_PW: &[u8] = b"pfm:vault:mk:main-password";
const AAD_RC: &[u8] = b"pfm:vault:mk:recovery-code";

struct Meta {
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
    pw_salt: Vec<u8>,
    pw_wrap: EncryptedField,
    rc_salt: Vec<u8>,
    rc_wrap: EncryptedField,
}

fn read_meta(conn: &Connection) -> AppResult<Option<Meta>> {
    let mut stmt = conn.prepare(
        "SELECT kdf_m_cost, kdf_t_cost, kdf_p_cost, pw_salt, pw_cipher, pw_nonce,
                rc_salt, rc_cipher, rc_nonce
         FROM vault_meta WHERE id = 1",
    )?;
    let mut rows = stmt.query_map([], |r| {
        Ok(Meta {
            m_cost: r.get(0)?,
            t_cost: r.get(1)?,
            p_cost: r.get(2)?,
            pw_salt: r.get(3)?,
            pw_wrap: EncryptedField {
                cipher: r.get(4)?,
                nonce: r.get(5)?,
            },
            rc_salt: r.get(6)?,
            rc_wrap: EncryptedField {
                cipher: r.get(7)?,
                nonce: r.get(8)?,
            },
        })
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

fn write_meta(conn: &Connection, m: &Meta) -> AppResult<()> {
    conn.execute(
        "INSERT INTO vault_meta (id, kdf_m_cost, kdf_t_cost, kdf_p_cost,
                                 pw_salt, pw_cipher, pw_nonce, rc_salt, rc_cipher, rc_nonce)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET
           kdf_m_cost=excluded.kdf_m_cost, kdf_t_cost=excluded.kdf_t_cost,
           kdf_p_cost=excluded.kdf_p_cost, pw_salt=excluded.pw_salt,
           pw_cipher=excluded.pw_cipher, pw_nonce=excluded.pw_nonce,
           rc_salt=excluded.rc_salt, rc_cipher=excluded.rc_cipher, rc_nonce=excluded.rc_nonce",
        rusqlite::params![
            m.m_cost as i64,
            m.t_cost as i64,
            m.p_cost as i64,
            m.pw_salt,
            m.pw_wrap.cipher,
            m.pw_wrap.nonce,
            m.rc_salt,
            m.rc_wrap.cipher,
            m.rc_wrap.nonce
        ],
    )?;
    Ok(())
}

/// 首次设置。恢复码由调用方先生成并展示给用户后才能写库， 因为这一步之后
/// 再也拿不到明文恢复码。
pub fn initialize(
    conn: &Connection,
    main_password: &str,
    recovery_code: &str,
) -> AppResult<MasterKey> {
    if is_initialized(conn)? {
        return Err(vault_err(
            "vault_already_initialized",
            "密钥库已经初始化过了",
            Some("如需更换主密码，请用「修改主密码」；只有忘记主密码时才用恢复码"),
        ));
    }
    if main_password.chars().count() < 8 {
        return Err(vault_err(
            "invalid_input",
            "主密码至少 8 个字符",
            Some("这是本机数据唯一的防线，太短等于没有"),
        ));
    }
    let mk = MasterKey(random_bytes::<MK_LEN>()?);
    // 默认参数：19 MiB / 2 轮 / 1 并行度， 开机解锁约几十毫秒， 够挡住离线爆破又不让人等
    let (m_cost, t_cost, p_cost) = (19_456u32, 2u32, 1u32);
    let pw_salt = random_bytes::<SALT_LEN>()?.to_vec();
    let rc_salt = random_bytes::<SALT_LEN>()?.to_vec();
    let kek_pw = derive_kek(main_password, &pw_salt, m_cost, t_cost, p_cost)?;
    let kek_rc = derive_kek(recovery_code, &rc_salt, m_cost, t_cost, p_cost)?;
    let meta = Meta {
        m_cost,
        t_cost,
        p_cost,
        pw_salt,
        pw_wrap: wrap(&kek_pw, &mk.0, AAD_PW)?,
        rc_salt,
        rc_wrap: wrap(&kek_rc, &mk.0, AAD_RC)?,
    };
    write_meta(conn, &meta)?;
    Ok(mk)
}

/// 解锁：两枚包裹都试。AEAD 认证标签不匹配就是密码错， 不需要另存校验值。
pub fn unlock(conn: &Connection, secret: &str) -> AppResult<MasterKey> {
    let meta = read_meta(conn)?.ok_or_else(|| {
        vault_err(
            "vault_not_initialized",
            "密钥库尚未初始化",
            Some("先设置主密码， 系统会同时给出一次性恢复码"),
        )
    })?;
    let kek_pw = derive_kek(secret, &meta.pw_salt, meta.m_cost, meta.t_cost, meta.p_cost)?;
    if let Some(bytes) = unwrap(&kek_pw, &meta.pw_wrap, AAD_PW) {
        return Ok(MasterKey(bytes));
    }
    let kek_rc = derive_kek(secret, &meta.rc_salt, meta.m_cost, meta.t_cost, meta.p_cost)?;
    if let Some(bytes) = unwrap(&kek_rc, &meta.rc_wrap, AAD_RC) {
        return Ok(MasterKey(bytes));
    }
    Err(vault_err(
        "vault_bad_secret",
        "主密码或恢复码不正确",
        Some("连续输错不会锁定或重置数据；忘记主密码时改用恢复码解锁"),
    ))
}

/// 换主密码：只换外层包裹， MK 不变， 所以已加密的字段一条都不用重写。
pub fn change_main_password(
    conn: &Connection,
    mk: &MasterKey,
    old_password: &str,
    new_password: &str,
) -> AppResult<()> {
    if new_password.chars().count() < 8 {
        return Err(vault_err("invalid_input", "新主密码至少 8 个字符", None));
    }
    let mut meta = read_meta(conn)?.ok_or_else(|| {
        vault_err("vault_not_initialized", "密钥库尚未初始化", None)
    })?;
    let old_kek = derive_kek(old_password, &meta.pw_salt, meta.m_cost, meta.t_cost, meta.p_cost)?;
    let verified = unwrap(&old_kek, &meta.pw_wrap, AAD_PW).is_some_and(|b| b == mk.0);
    if !verified {
        return Err(vault_err(
            "vault_bad_secret",
            "当前主密码不正确",
            Some("修改主密码必须先验证旧密码"),
        ));
    }
    meta.pw_salt = random_bytes::<SALT_LEN>()?.to_vec();
    let new_kek = derive_kek(new_password, &meta.pw_salt, meta.m_cost, meta.t_cost, meta.p_cost)?;
    meta.pw_wrap = wrap(&new_kek, &mk.0, AAD_PW)?;
    write_meta(conn, &meta)
}

/// 用恢复码解锁后换上新恢复码：旧恢复码随即失效， 否则它就成了一把永不轮换的万能钥匙。
pub fn rotate_recovery_code(conn: &Connection, mk: &MasterKey) -> AppResult<String> {
    let mut meta = read_meta(conn)?.ok_or_else(|| {
        vault_err("vault_not_initialized", "密钥库尚未初始化", None)
    })?;
    let code = generate_recovery_code()?;
    meta.rc_salt = random_bytes::<SALT_LEN>()?.to_vec();
    let kek = derive_kek(&code, &meta.rc_salt, meta.m_cost, meta.t_cost, meta.p_cost)?;
    meta.rc_wrap = wrap(&kek, &mk.0, AAD_RC)?;
    write_meta(conn, &meta)?;
    Ok(code)
}

/// AAD 绑定「行 id + 列名」：把 A 行某列的密文搬到 B 行会直接解不开，
/// 数据库被局部篡改时不至于把某个账号的密码挪到另一个账号上。
fn aad(row_id: &str, column: &str) -> Vec<u8> {
    format!("{row_id}:{column}").into_bytes()
}

pub fn encrypt_field(
    mk: &MasterKey,
    row_id: &str,
    column: &str,
    plaintext: &str,
) -> AppResult<EncryptedField> {
    let cipher = Aes256Gcm::new_from_slice(&mk.0).expect("MK 长度固定为 32 字节");
    let nonce = random_bytes::<NONCE_LEN>()?;
    let body = cipher
        .encrypt(
            &nonce_of("field_encrypt_failed", &nonce)?,
            Payload {
                msg: plaintext.as_bytes(),
                aad: &aad(row_id, column),
            },
        )
        .map_err(|e| vault_err("field_encrypt_failed", &format!("字段加密失败：{e}"), None))?;
    Ok(EncryptedField {
        cipher: body,
        nonce: nonce.to_vec(),
    })
}

/// 解不开就报错， 不返回空串：把「密文被篡改/搬动」伪装成「用户没填」是最坏的诊断体验。
pub fn decrypt_field(
    mk: &MasterKey,
    row_id: &str,
    column: &str,
    field: &EncryptedField,
) -> AppResult<String> {
    let cipher = Aes256Gcm::new_from_slice(&mk.0).expect("MK 长度固定为 32 字节");
    let nonce = nonce_of("field_decrypt_failed", &field.nonce)?;
    let plain = cipher
        .decrypt(
            &nonce,
            Payload {
                msg: field.cipher.as_slice(),
                aad: &aad(row_id, column),
            },
        )
        .map_err(|_| {
            vault_err(
                "field_decrypt_failed",
                "该字段无法解密",
                Some("密文可能被改动， 或数据来自另一个密钥库；必要时从备份包恢复"),
            )
        })?;
    String::from_utf8(plain).map_err(|e| {
        vault_err("field_not_utf8", &format!("解密结果不是文本：{e}"), None)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PW: &str = "主密码-2026";
    const RC: &str = "ABCD-EFGH-JKMN-PQRS";

    fn init(conn: &Connection) -> MasterKey {
        initialize(conn, PW, RC).unwrap()
    }

    #[test]
    fn unlock_accepts_either_wrap_and_rejects_wrong_secret() {
        let conn = crate::db::open_in_memory().unwrap();
        let mk = init(&conn);
        assert!(is_initialized(&conn).unwrap());
        assert_eq!(unlock(&conn, PW).unwrap().0, mk.0);
        assert_eq!(unlock(&conn, RC).unwrap().0, mk.0);
        assert_eq!(
            unlock(&conn, "猜错的").unwrap_err().code,
            "vault_bad_secret"
        );
    }

    #[test]
    fn double_wrap_survives_a_password_change() {
        let conn = crate::db::open_in_memory().unwrap();
        let mk = init(&conn);
        change_main_password(&conn, &mk, PW, "新主密码-8888").unwrap();

        assert_eq!(unlock(&conn, "新主密码-8888").unwrap().0, mk.0);
        assert_eq!(unlock(&conn, PW).unwrap_err().code, "vault_bad_secret");
        // 恢复码这一路没被动过， 换密码不该把它弄失效
        assert_eq!(unlock(&conn, RC).unwrap().0, mk.0);
    }

    #[test]
    fn rotating_the_recovery_code_retires_the_old_one() {
        let conn = crate::db::open_in_memory().unwrap();
        let mk = init(&conn);
        let fresh = rotate_recovery_code(&conn, &mk).unwrap();
        assert_ne!(fresh, RC);
        assert_eq!(unlock(&conn, &fresh).unwrap().0, mk.0);
        assert_eq!(unlock(&conn, RC).unwrap_err().code, "vault_bad_secret");
    }

    #[test]
    fn master_key_never_hits_the_database_in_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open(dir.path()).unwrap();
        let mk = initialize(&conn, PW, RC).unwrap();
        drop(conn);

        let raw = std::fs::read(crate::db::db_path(dir.path())).unwrap();
        assert!(!contains(&raw, &mk.0), "主密钥明文不应出现在库文件里");
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn field_roundtrip_and_aad_binding() {
        let conn = crate::db::open_in_memory().unwrap();
        let mk = init(&conn);
        let field = encrypt_field(&mk, "row-a", "password", "P@ssw0rd 中文").unwrap();
        assert_eq!(decrypt_field(&mk, "row-a", "password", &field).unwrap(), "P@ssw0rd 中文");

        // 跨行挪用与跨列挪用都必须解不开
        assert_eq!(
            decrypt_field(&mk, "row-b", "password", &field).unwrap_err().code,
            "field_decrypt_failed"
        );
        assert_eq!(
            decrypt_field(&mk, "row-a", "account", &field).unwrap_err().code,
            "field_decrypt_failed"
        );
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let conn = crate::db::open_in_memory().unwrap();
        let mk = init(&conn);
        let mut field = encrypt_field(&mk, "row-a", "password", "secret").unwrap();
        field.cipher[3] ^= 0b0000_0001;
        assert_eq!(
            decrypt_field(&mk, "row-a", "password", &field).unwrap_err().code,
            "field_decrypt_failed"
        );
    }

    #[test]
    fn wrong_master_key_cannot_decrypt() {
        let conn = crate::db::open_in_memory().unwrap();
        let mk = init(&conn);
        let field = encrypt_field(&mk, "row-a", "password", "secret").unwrap();
        let other = MasterKey(random_bytes::<MK_LEN>().unwrap());
        assert_eq!(
            decrypt_field(&other, "row-a", "password", &field).unwrap_err().code,
            "field_decrypt_failed"
        );
    }

    #[test]
    fn short_password_and_double_init_are_refused() {
        let conn = crate::db::open_in_memory().unwrap();
        assert_eq!(
            initialize(&conn, "1234567", RC).unwrap_err().code,
            "invalid_input"
        );
        let _mk = init(&conn);
        assert_eq!(
            initialize(&conn, PW, RC).unwrap_err().code,
            "vault_already_initialized"
        );
    }

    #[test]
    fn recovery_code_is_readable_and_uses_an_unambiguous_alphabet() {
        let code = generate_recovery_code().unwrap();
        assert_eq!(code.matches('-').count(), 3);
        assert_eq!(code.chars().filter(|c| *c != '-').count(), 16);
        assert!(
            code.chars()
                .all(|c| CODE_ALPHABET.contains(&(c as u8)) || c == '-'),
            "恢复码里不能出现 I/O/0/1：{code}"
        );
    }
}
