//! 开机自启（Windows 用户级 Run 键）。
//! shortcut: Windows-only via HKCU Run; upgrade to tauri-plugin-autostart if cross-platform is needed.

use crate::error::{AppError, AppResult};

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "项目资料管理系统";

fn failed(action: &str, detail: impl std::fmt::Display) -> AppError {
    AppError::new(
        "autostart_failed",
        &format!("{action}开机自启失败：{detail}"),
        Some("检查安全软件是否拦截注册表写入；也可手动查看 HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
    )
}

fn reg_data(exe: &std::path::Path) -> String {
    format!("\"{}\"", exe.display())
}

#[cfg(windows)]
fn reg(args: &[&str]) -> std::io::Result<std::process::ExitStatus> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg")
        .args(args)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW：每次读状态都不许闪黑框
        .status()
}

#[cfg(windows)]
pub fn is_enabled() -> AppResult<bool> {
    let status = reg(&["query", RUN_KEY, "/v", VALUE]).map_err(|e| failed("读取", e))?;
    Ok(status.success())
}

#[cfg(not(windows))]
pub fn is_enabled() -> AppResult<bool> {
    Err(AppError::new("invalid_input", "开机自启仅支持 Windows", None))
}

#[cfg(windows)]
pub fn set_enabled(enabled: bool) -> AppResult<()> {
    if enabled {
        let exe = std::env::current_exe().map_err(|e| failed("设置", e))?;
        let data = reg_data(&exe);
        let status = reg(&["add", RUN_KEY, "/v", VALUE, "/t", "REG_SZ", "/d", &data, "/f"])
            .map_err(|e| failed("设置", e))?;
        if !status.success() {
            return Err(failed("设置", format!("reg add 退出码 {:?}", status.code())));
        }
        return Ok(());
    }
    if !is_enabled()? {
        return Ok(());
    }
    let status = reg(&["delete", RUN_KEY, "/v", VALUE, "/f"]).map_err(|e| failed("关闭", e))?;
    if !status.success() {
        return Err(failed("关闭", format!("reg delete 退出码 {:?}", status.code())));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn set_enabled(_enabled: bool) -> AppResult<()> {
    Err(AppError::new("invalid_input", "开机自启仅支持 Windows", None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn quotes_exe_path_for_the_run_value() {
        let data = reg_data(Path::new(r"C:\Program Files\项目资料管理系统\项目资料管理系统.exe"));
        assert_eq!(
            data,
            "\"C:\\Program Files\\项目资料管理系统\\项目资料管理系统.exe\""
        );
        assert!(data.starts_with('"') && data.ends_with('"'), "路径必须带引号：{data}");
    }
}
