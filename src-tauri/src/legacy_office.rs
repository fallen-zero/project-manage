//! 旧版 Office 二进制格式（.doc/.ppt，Office 97-2003）的正文抽取（M7）。
//! 纯 Rust 没有可靠方案（`calamine`/`dowse` 都只覆盖 docx/xlsx/pptx/xls），所以走**本机 LibreOffice
//! 无头转换**：转成 docx/pptx 后交给 `extract::office_text` 那条既有抽取链，产物用完即删（D2）。
//! 本模块只读源文件、只往自己的临时目录写；探不到 LibreOffice 时由 `index_scan` 的降级门整类不建行（D6）。
//!
//! 下面每一条关于 soffice 行为的论断都指向 spec §三 的实测读数，改动前先重跑
//! `.superpowers/sdd/2026-10-13-m7-legacy-office/probe5.sh`，别把脚本 bug 读成 LibreOffice 的行为。
//!
//! # 关于本文件里的 `#[allow(dead_code)]`（T1 记账）
//! 本任务只铺纯核，生产调用点全在本批后续任务：转换件（`CONVERT_TIMEOUT`、`win_slash`、`profile_url`、
//! `convert_args`、`product_path`、`profile_dir`、`Scratch`、`run_bounded`）由 T2 的 `legacy_text` 用、
//! T3 经 `extract_text` 调到；`soffice_candidates`/`soffice_path`/`soffice_available`/`LIBREOFFICE_SUBDIR`
//! 由 T2 与 T4 的降级门用；`WINGET_PACKAGE_ID`/`winget_path`/`winget_args`/`InstallOutcome` 由 T5 的
//! `start_install` 用。处理方式与 `db::open_in_memory`（`db.rs:30`）和 M6 的 ocr.rs 首提交同款：
//! **逐项显式放行，绝不模块级/全局关掉**（模块级豁免会连 T2 以后新写的没接线代码一起吞掉）。
//! 后续任务接线落地时**必须顺手删掉对应豁免**；T5 的收口结构门 `grep -rn "allow(dead_code)" src/`
//! 要求全仓只剩 `db.rs:30`，本文件届时要一条不剩。
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};

/// 单份文件的转换预算。实测稳态 1.03–1.19 s/份（spec §三.2），60 s 是「真卡死」与「慢盘」的分界。
/// 之所以必须有这一道：`run_pass` 只在**文件之间**查 cancel，子进程不返回就停不下来。
#[allow(dead_code)] // T2 接线（legacy_text）后删
pub const CONVERT_TIMEOUT: Duration = Duration::from_secs(60);

/// 安装用的包 ID：**硬编码在本机 Rust 侧，绝不取自任何入参**（spec §四 那条「不给前端任意命令执行」）。
#[allow(dead_code)] // T5 接线（start_install → winget_args）后删
pub const WINGET_PACKAGE_ID: &str = "TheDocumentFoundation.LibreOffice";
#[allow(dead_code)] // T2/T4 接线（soffice_candidates 进生产）后删
const LIBREOFFICE_SUBDIR: &str = "LibreOffice";

/// 三个候选位：`ProgramFiles` → `ProgramFiles(x86)` → PATH 各段。**探的是 `soffice.com`**：
/// 实测同一活儿 `.exe` 6.65 s、`.com` 1.27 s，且 `.exe --version` 一个字都不往控制台写（spec §三.1）。
/// 做成吃 `Option<&OsStr>` 的纯函数，是为了让「顺序」与「`.com` 这个文件名」能被单测打红。
#[allow(dead_code)] // T2/T4 接线（soffice_path 进生产）后删
pub fn soffice_candidates(
    program_files: Option<&OsStr>,
    program_files_x86: Option<&OsStr>,
    path_var: Option<&OsStr>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in [program_files, program_files_x86].into_iter().flatten() {
        out.push(Path::new(root).join(LIBREOFFICE_SUBDIR).join("program").join("soffice.com"));
    }
    if let Some(pv) = path_var {
        for seg in std::env::split_paths(pv) {
            out.push(seg.join("soffice.com"));
        }
    }
    out
}

/// 活探测：第一个真实存在的候选。调用方每轮一次（D5，**不记忆化**——三次 `is_file()` 便宜到可以现探，
/// 换来「一键装完不用重启应用」）。
#[allow(dead_code)] // T2/T4 接线后删
pub fn soffice_path() -> Option<PathBuf> {
    soffice_candidates(
        std::env::var_os("ProgramFiles").as_deref(),
        std::env::var_os("ProgramFiles(x86)").as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
    .into_iter()
    .find(|p| p.is_file())
}

#[allow(dead_code)] // T4 接线（index_scan 降级门）后删
pub fn soffice_available() -> bool {
    soffice_path().is_some()
}

/// Windows 路径 → **全正斜杠**形式，并剥掉 `\\?\` 那个扩展长度前缀（M6 在 WinRT 上吃过同一课，坑 13）。
/// 已知残缺与 M6 一致：`\\?\UNC\server\share` 剥完变成 `UNC/server/share`，那条路径会转换失败并优雅落
/// `failed`（不 panic）。见测试 `win_slash_on_unc_is_known_broken_like_m6`，别把它悄悄"修"成没测过的样子。
#[allow(dead_code)] // T2 接线（经 profile_url 进 convert_args 生产链）后删
pub fn win_slash(p: &Path) -> String {
    let s = p.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.replace('\\', "/")
}

/// `-env:UserInstallation` 要的 URL。**混合反斜杠的写法是慢失败**（实测 13.8–45 s 才报错，spec §三.4），
/// 所以这里只走 `win_slash`，拼成 `file:///C:/...`。
#[allow(dead_code)] // T2 接线后删
pub fn profile_url(profile_dir: &Path) -> String {
    format!("file:///{}", win_slash(profile_dir))
}

/// 一次转换的 argv。**顺序与形态都是实测钉死的**：
/// `--outdir` 要**空格分隔**（等号形被直接拒、并回吐整页 usage）；`--convert-to` **不写 filter 名**
/// （实测自动选 `MS Word 2007 XML` / `Impress Office Open XML`）；`-env:` 用等号（那一个是 URL 语法）。
#[allow(dead_code)] // T2 接线（legacy_text 拼 argv）后删
pub fn convert_args(target: &str, out_dir: &str, src: &str, profile_url: &str) -> Vec<String> {
    vec![
        "--headless".to_owned(),
        "--norestore".to_owned(),
        "--nolockcheck".to_owned(),
        "--nodefault".to_owned(),
        "--convert-to".to_owned(),
        target.to_owned(),
        "--outdir".to_owned(),
        out_dir.to_owned(),
        format!("-env:UserInstallation={profile_url}"),
        src.to_owned(),
    ]
}

/// LibreOffice 的产物落点：`<out_dir>/<源文件 stem>.<target>`（实测：名字跟着源 stem 走）。
/// 每个源文件配**独立**的 `out_dir` 正是为了避开「两份同名文件进同一目录互相覆盖」（spec §三.8）。
#[allow(dead_code)] // T2 接线（legacy_text 校验产物）后删
pub fn product_path(out_dir: &Path, src: &Path, target: &str) -> PathBuf {
    let stem = src.file_stem().unwrap_or(OsStr::new("doc"));
    out_dir.join(stem).with_extension(target)
}

/// 独立用户配置目录（每进程一个，**故意不删**：实测每次新建 profile 多花 ≈0.78 s/份，spec §三.3；
/// 代价记在 spec §十一.6——`%TEMP%\pfm-m7\` 下会留约 359 KB）。
#[allow(dead_code)] // T2 接线（legacy_text 取 profile）后删
pub fn profile_dir() -> PathBuf {
    let dir = std::env::temp_dir()
        .join("pfm-m7")
        .join(format!("profile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// 一次性输出目录（每文件一个），Drop 时整棵删掉——成功与失败路径都清（spec §五.6）。
/// 不用 `tempfile`：它在 `[dev-dependencies]`，生产代码拿不到。
#[allow(dead_code)] // T2 接线（legacy_text 的一次性 out_dir）后删
pub struct Scratch {
    dir: PathBuf,
}

#[allow(dead_code)] // 同上：new/dir 的首个生产调用点在 T2 的 legacy_text
impl Scratch {
    pub fn new(tag: &str) -> AppResult<Scratch> {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join("pfm-m7")
            .join(format!("{}-{}-{}", std::process::id(), n, tag));
        std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, &e))?;
        Ok(Scratch { dir })
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 起子进程并**带预算地等**：每 200 ms 轮询一次 `try_wait`，超时就 `kill`。
/// 退出码由调用方判断——**别信它**（spec §三.6：垃圾字节冒充 `.doc` 也是 exit 0）。
#[allow(dead_code)] // T2 接线（legacy_text 起子进程）后删
pub fn run_bounded(program: &Path, args: &[String], budget: Duration) -> AppResult<ExitStatus> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::new(
            "extract_failed",
            &format!("启动 LibreOffice 失败：{e}"),
            Some("确认本机装了 LibreOffice；/index 的「支持类型与上限」卡片里有一键安装入口"),
        ))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if started.elapsed() >= budget {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::new(
                        "extract_failed",
                        &format!("LibreOffice 未在 {} 秒内返回，已终止该文件这一笔", budget.as_secs()),
                        Some("文件可能损坏或过大；这一轮其余文件不受影响"),
                    ));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(AppError::new("extract_failed", &format!("等待 LibreOffice 失败：{e}"), None)),
        }
    }
}

/// winget 的所在（`WindowsApps` 在 PATH 里，所以复用 PATH 扫描；不硬编码版本号路径）。
#[allow(dead_code)] // T5 接线（start_install）后删
pub fn winget_path() -> Option<PathBuf> {
    std::env::var_os("PATH")
        .as_deref()
        .and_then(|pv| std::env::split_paths(&pv).find(|seg| seg.join("winget.exe").is_file()))
        .map(|seg| seg.join("winget.exe"))
}

/// 安装命令的参数。**包 ID 从这里硬编码**，`libreoffice_install` 那条 IPC 命令不吃任何前端入参。
#[allow(dead_code)] // T5 接线（start_install）后删
pub fn winget_args() -> Vec<String> {
    vec![
        "install".to_owned(),
        "--id".to_owned(),
        WINGET_PACKAGE_ID.to_owned(),
        "-e".to_owned(),
        "--accept-source-agreements".to_owned(),
        "--accept-package-agreements".to_owned(),
    ]
}

/// T5 起由 `start_install`（`libreoffice_install` IPC 命令）构造；**届时删除这条 `allow(dead_code)` 豁免**
/// （账本已记：T5 收口门要求本属性不残留）。
#[allow(dead_code)]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    /// true = 已在自己的控制台窗口里起起 `winget install`（UAC 与进度都在那个窗口，本命令不等它结束）；
    /// false = 本机探不到 winget，**由前端 `openUrl` 打开官网下载页**。
    pub started_via_winget: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soffice_candidates_order_is_program_files_then_x86_then_path() {
        let c = soffice_candidates(
            Some(OsStr::new("C:\\pf")),
            Some(OsStr::new("D:\\pf86")),
            Some(OsStr::new("E:\\bin;F:\\bin2")),
        );
        assert_eq!(c.len(), 4, "两个 ProgramFiles + PATH 两段 = 4 个候选：{c:?}");
        assert_eq!(c[0], PathBuf::from("C:\\pf").join("LibreOffice").join("program").join("soffice.com"));
        assert_eq!(c[1], PathBuf::from("D:\\pf86").join("LibreOffice").join("program").join("soffice.com"));
        assert_eq!(c[2], PathBuf::from("E:\\bin").join("soffice.com"));
        assert_eq!(c[3], PathBuf::from("F:\\bin2").join("soffice.com"));
    }

    #[test]
    fn soffice_candidates_never_offer_the_gui_launcher() {
        // 实测 `.exe` 同一活儿慢 5 倍且 `--version` 零输出（spec §三.1），探错文件是静默的性能窟窿。
        for c in soffice_candidates(Some(OsStr::new("C:\\pf")), None, Some(OsStr::new("X:\\bin"))) {
            assert_eq!(c.file_name().unwrap(), "soffice.com", "候选必须是 .com：{c:?}");
        }
    }

    #[test]
    fn convert_args_uses_space_form_outdir_and_no_filter_name() {
        let a = convert_args("docx", "C:/tmp/o", "C:/tmp/合同.doc", "file:///C:/tmp/p");
        assert_eq!(
            a,
            vec![
                "--headless", "--norestore", "--nolockcheck", "--nodefault",
                "--convert-to", "docx",
                "--outdir", "C:/tmp/o",
                "-env:UserInstallation=file:///C:/tmp/p",
                "C:/tmp/合同.doc",
            ].into_iter().map(String::from).collect::<Vec<String>>(),
            "argv 的形状就是实测能跑通的那一串：{a:?}"
        );
        assert!(a.iter().all(|x| !x.starts_with("--outdir=")), "--outdir 用等号形会被 LibreOffice 直接拒绝");
    }

    #[test]
    fn product_path_reuses_the_source_stem_with_the_target_extension() {
        let p = product_path(Path::new("C:/out"), Path::new("C:/src/合同 验收.doc"), "docx");
        assert_eq!(p, PathBuf::from("C:/out/合同 验收.docx"));
    }

    #[test]
    fn win_slash_flips_separators_and_strips_the_extended_length_prefix() {
        assert_eq!(win_slash(Path::new(r"C:\x\y")), "C:/x/y");
        assert_eq!(win_slash(Path::new(r"\\?\C:\Users\zero\合同.doc")), "C:/Users/zero/合同.doc");
    }

    #[test]
    fn win_slash_on_unc_is_known_broken_like_m6() {
        // 与 M6 的有名登记同形残缺：`\\?\UNC\...` 剥完只剩 UNC/...，网络盘上的旧格式会落 failed。
        // 这条断言的是**现状**，不是理想行为；真要修 UNC 支持时它会红，那时改判据而不是删断言。
        assert_eq!(win_slash(Path::new(r"\\?\UNC\server\share\合同.doc")), "UNC/server/share/合同.doc");
    }

    #[test]
    fn profile_url_is_forward_slashes_only() {
        let u = profile_url(Path::new(r"C:\Users\zero\AppData\Local\Temp\pfm-m7\profile-1"));
        assert!(u.starts_with("file:///C:/"), "要三斜杠 + 正斜杠盘符：{u}");
        assert!(!u.contains('\\'), "混合分隔符是 14–45 s 的慢失败（spec §三.4）：{u}");
    }

    #[test]
    fn scratch_dir_disappears_when_it_is_dropped() {
        let s = Scratch::new("t1").unwrap();
        let dir = s.dir().to_path_buf();
        std::fs::write(dir.join("x.txt"), b"y").unwrap();
        assert!(dir.is_dir(), "new 之后目录必须在");
        drop(s);
        assert!(!dir.exists(), "Drop 必须把临时目录整棵删掉（失败路径也走这一条）");
    }

    #[test]
    fn run_bounded_kills_a_child_that_overruns_the_budget() {
        // 不需要「真会卡死 LibreOffice 的坏 .doc」就能证伪超时分支：注入一条必然慢的命令即可。
        let cmd = std::env::var_os("COMSPEC").map(PathBuf::from).expect("Windows 上必有 cmd.exe");
        let r = run_bounded(
            &cmd,
            &[
                "/c".to_owned(),
                "ping -n 20 127.0.0.1 >nul".to_owned(),
            ],
            Duration::from_millis(800),
        );
        let e = r.expect_err("慢命令必须被超时打断，否则 60 s 预算只是个装饰");
        assert_eq!(e.code, "extract_failed", "超时沿用既有码，不扩码表：{}", e.message);
        assert!(e.message.contains("未在 0 秒内返回") || e.message.contains("秒内返回"),
            "错误要说清是超时：{}", e.message);
    }

    #[test]
    fn run_bounded_passes_a_fast_child_through() {
        let cmd = std::env::var_os("COMSPEC").map(PathBuf::from).unwrap();
        let st = run_bounded(&cmd, &["/c".to_owned(), "exit 0".to_owned()], Duration::from_secs(5))
            .expect("快命令不该被判超时");
        assert!(st.success(), "退出状态要原样交回调用方（它还不信呢）");
    }

    #[test]
    fn probe_returns_a_bool_without_panicking() {
        // 与 M6 的 `probe_returns_a_bool_without_panicking` 同形：只断「不 panic、返回 bool」。
        assert!(matches!(soffice_available(), true | false));
        // winget 那一条要能红：探到了就必须是 winget.exe（把候选名写成别的文件名这条就红），
        // 光断 `matches!(x.is_some(), true|false)` 是恒真、零守护（本仓「简报里的测试必须可证伪」那条）。
        assert!(
            winget_path()
                .map(|p| p.file_name().unwrap() == "winget.exe")
                .unwrap_or(true),
            "winget 探测要么没有、要么必须指向 winget.exe"
        );
    }

    #[test]
    fn winget_args_hardcode_the_package_id() {
        let a = winget_args();
        assert!(a.windows(2).any(|w| w[0] == "--id" && w[1] == WINGET_PACKAGE_ID),
            "包 ID 必须硬编码进参数：{a:?}");
        assert_eq!(WINGET_PACKAGE_ID, "TheDocumentFoundation.LibreOffice",
            "换 ID 要连 spec §四 那条「不给前端任意命令执行」一起重新过：{WINGET_PACKAGE_ID}");
    }
}
