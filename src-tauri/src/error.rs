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
