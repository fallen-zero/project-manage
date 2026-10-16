use std::fmt;

/// 统一的应用错误。三段式：机器可解析的 code、给用户看的 message、可行动的 hint。
/// code 是给前端分支判断用的稳定契约，message/hint 随文案变化不影响调用方。
#[derive(Debug)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
    pub hint: Option<String>,
}

impl AppError {
    pub fn new(code: &'static str, message: &str, hint: Option<&str>) -> Self {
        Self {
            code,
            message: message.to_owned(),
            hint: hint.map(|h| h.to_owned()),
        }
    }

    pub fn io(path: &std::path::Path, e: &std::io::Error) -> Self {
        Self::new(
            "fs_failed",
            &format!("访问 {} 失败：{e}", path.display()),
            Some("确认该路径存在且可读写"),
        )
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, "（{hint}）")?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        // M5.5 D7：只给 busy（5 s `busy_timeout` 耗尽）单拉一句人话 + hint，**不新增 code**
        // （`hint` 本就在回线上、界面已拼进红字，见 spec D7）。其余 rusqlite 错原文照旧。
        if let rusqlite::Error::SqliteFailure(ffi_err, _) = &e {
            if ffi_err.code == rusqlite::ffi::ErrorCode::DatabaseBusy {
                return Self::new(
                    "db_failed",
                    "数据库正被占用（索引正在进行中），已等待约 5 秒仍未排上，请重试",
                    Some("这不是数据出错；等这一轮索引跑完（或点「取消」）再保存通常就好"),
                );
            }
        }
        Self::new(
            "db_failed",
            &format!("数据库操作失败：{e}"),
            Some("若提示库文件损坏，请从备份包恢复数据"),
        )
    }
}

// Tauri 要求命令的 Err 可序列化并能在前端还原
impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("AppError", 3)?;
        st.serialize_field("code", self.code)?;
        st.serialize_field("message", &self.message)?;
        st.serialize_field("hint", &self.hint)?;
        st.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::ffi;

    fn sqlite_err(code: ffi::ErrorCode, msg: &str) -> rusqlite::Error {
        // bundled libsqlite3-sys 的 Error{code, extended_code} 两字段皆 pub；
        // 直接构造，不必为造 busy 开真连接（本机 WAL 下第二连接会打不开）。
        rusqlite::Error::SqliteFailure(ffi::Error { code, extended_code: 0 }, Some(msg.to_owned()))
    }

    #[test]
    fn busy_error_gets_an_actionable_message_and_hint() {
        // 5 s busy_timeout 耗尽后 SQLite 回的就是 SQLITE_BUSY。
        let e: AppError = sqlite_err(ffi::ErrorCode::DatabaseBusy, "database is locked").into();
        // code 仍是 db_failed（不扩码表；前端按 code 分支的既有逻辑不受影响）。
        assert_eq!(e.code, "db_failed");
        assert!(
            e.message.starts_with("数据库正被占用"),
            "busy 要给一句人话，实际回的是：{}",
            e.message
        );
        assert!(e.hint.is_some(), "busy 必须带可行动 hint（界面已把 hint 拼进红字）");
    }

    #[test]
    fn non_busy_sqlite_errors_keep_the_original_wording() {
        let e: AppError = sqlite_err(ffi::ErrorCode::ConstraintViolation, "constraint failed").into();
        assert_eq!(e.code, "db_failed");
        assert!(e.message.starts_with("数据库操作失败"), "非 busy 不该被新文案覆盖：{}", e.message);
    }
}
