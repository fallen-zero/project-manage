//! 旧版 Office 二进制格式（.doc/.ppt，Office 97-2003）的正文抽取（M7）。
//! 纯 Rust 没有可靠方案（`calamine`/`dowse` 都只覆盖 docx/xlsx/pptx/xls），所以走**本机 LibreOffice
//! 无头转换**：转成 docx/pptx 后交给 `extract::office_text` 那条既有抽取链，产物用完即删（D2）。
//! 本模块只读源文件、只往自己的临时目录写；探不到 LibreOffice 时由 `index_scan` 的降级门整类不建行（D6）。
//!
//! 下面每一条关于 soffice 行为的论断都指向 spec §三 的实测读数，改动前先重跑
//! `.superpowers/sdd/2026-10-13-m7-legacy-office/probe5.sh`，别把脚本 bug 读成 LibreOffice 的行为。
//!
//! # 关于本文件里的 `dead_code` 豁免（T1 记账）
//! 本任务只铺纯核，生产调用点全在本批后续任务：转换件（`CONVERT_TIMEOUT`、`win_slash`、`profile_url`、
//! `convert_args`、`product_path`、`Scratch`、`run_bounded`）由 T2 的 `legacy_text` 用、
//! T3 经 `extract_text` 调到；`soffice_candidates`/`soffice_path`/`soffice_available`/`LIBREOFFICE_SUBDIR`
//! 由 T2 与 T4 的降级门用；`WINGET_PACKAGE_ID`/`winget_path`/`winget_args`/`InstallOutcome` 由 T5 的
//! `start_install` 用。处理方式与 `db::open_in_memory`（`db.rs:30`）和 M6 的 ocr.rs 首提交同款：
//! **逐项显式放行，绝不模块级/全局关掉**（模块级豁免会连 T2 以后新写的没接线代码一起吞掉）。
//! 后续任务接线落地时**必须顺手删掉对应豁免**；T5 的收口结构门（在全仓源码里 grep 本属性的字面量）
//! 要求终态只剩 `db.rs:30` 一处，本文件届时要一条不剩。
//!
//! # T2 追加的一格（`legacy_text`，注释写明由 T3 删）
//! T2 实测过一件事（`cargo check -j 1 --lib`，非 test cfg）：把 T1 标注 T2 的那 11 格逐项放行摘掉后，
//! 纯核项**一条 `dead_code` 都不报**——放行只压住它自己那一格，调用边照样把下游传成活代码，所以
//! `legacy_text` 一存在，`win_slash`/`convert_args`/`Scratch` 那批就跟着活了；真正还报 never used 的
//! 只剩 `legacy_text` 自己。按上面「谁接线谁删」的口径这 11 格本可当场清掉，但控制方在派发时裁定
//! 「T2 不碰其余豁免，归 T3/T4/T5/T6 各自收」，故此处只把账记下、不擅自删。
//!
//! # 并发形状：目录隔离，不是排队（spec §三.16 / §五.6 / §十.10 的裁定）
//! **一笔转换一个一次性目录**，里面 `<scratch>/out` 与 `<scratch>/profile` 两格，成功与失败都随
//! `Scratch` 析构整棵删掉。早先「profile 按进程复用 + 一把进程内 mutex 把调用排成串行」已被实测推翻：
//! 两个实例共用同一个 `UserInstallation` 时抢输的一方 `exit 1` 且零产物（probe6 四格，
//! `.superpowers/sdd/2026-10-13-m7-legacy-office/probe6-readings.txt`），而且起不来时 LibreOffice 会弹
//! 一句把锅甩给 `program\bootstrap.ini`「已损坏」的模态框（文件实测没坏），子进程不退出 ⇒ 那一笔烧满
//! 60 s 超时。`cargo test` 默认多线程、生产上索引 worker 与预览通路也真会并发，所以复用 profile 换来的
//! 是随机假失败。**并发靠目录隔离消解，本模块绝不再加进程内的锁/信号量/队列。**
//! 代价照实记：每份多 ≈0.78 s 的 profile 初始化（spec §三.3），30 份的首轮约多 25 s；
//! 换来的是 `%TEMP%\pfm-m7\` 不再累积 `profile-<pid>`（spec §十一.6 的旧账作废）。
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
#[allow(dead_code)] // T2/T4 接线（LIBREOFFICE_SUBDIR 进生产）后删
const LIBREOFFICE_SUBDIR: &str = "LibreOffice";

/// 三个候选位：`ProgramFiles` → `ProgramFiles(x86)` → PATH 各段。**探的是 `soffice.com`**：
/// 实测同一活儿 `.exe` 6.65 s、`.com` 1.27 s，且 `.exe --version` 一个字都不往控制台写（spec §三.1）。
/// 做成吃 `Option<&OsStr>` 的纯函数，是为了让「顺序」与「`.com` 这个文件名」能被单测打红。
#[allow(dead_code)] // T2/T4 接线（soffice_candidates 进生产）后删
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
    // OsStr 没有 Display，所以先转字符串再做纯拼接（绝不能退回 `with_extension`，理由见下）。
    let stem = src.file_stem().unwrap_or_else(|| OsStr::new("doc")).to_string_lossy();
    // 用 format! 而不是 `with_extension`：后者以「最后一个点」为界替换扩展名，于是源名里带点
    // （中文项目里极常见：`合同2024.03验收.doc` 的 stem 是 `合同2024.03验收`）会被它当成扩展名剥掉，
    // 算出 `合同2024.docx` —— 而 LibreOffice 的真产物是 `合同2024.03验收.docx`，T2 会「产物不存在」假落 failed。
    out_dir.join(format!("{stem}.{target}"))
}

/// 一次转换的一次性目录（每笔一个），里面分 `<scratch>/out` 与 `<scratch>/profile` 两格；
/// Drop 时整棵删掉——成功与失败路径都清（spec §五.6）。**profile 必须长在同一个一次性目录里**
/// （spec §三.16：共用 `UserInstallation` 的并行实例会抢输、败者零产物还弹模态框卡到超时）。
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

/// 把 `.doc` / `.ppt` 转成 docx/pptx 再抽出正文。**在抽取阶段（写锁外）被调用**（D1）——
/// 一份 ≈1.2 s 的进程冷启动只拖长这一轮，不进任何单笔写锁。
/// 产物只当管道：落到 `Scratch`（每文件一个目录，避开同名覆盖）→ 交给既有 `office_text` 抽正文
/// → 目录随 `Scratch` 析构删掉，库里只留正文（D2，零新表零新体积账）。
#[allow(dead_code)] // T3 接线（extract_text 两臂）落地后删本行豁免
pub fn legacy_text(path: &Path, slides: bool) -> AppResult<String> {
    let target = if slides { "pptx" } else { "docx" };
    let soffice = soffice_path().ok_or_else(|| AppError::new(
        "extract_failed",
        &format!("未找到 LibreOffice，无法读取旧格式（{}）", path.display()),
        Some("在「全文索引」页的支持类型卡片里点「安装 LibreOffice」；装完不用重启，下一轮自动开始索引"),
    ))?;
    // 一笔转换一个一次性目录，里面分 `out` 与 `profile` 两格：**profile 必须每笔独立**
    // （spec §三.16 实测：两个实例共用同一个 UserInstallation 时，抢输的一方 exit 1 且零产物，
    // 还会弹模态框把这笔卡到超时；而索引 worker 与预览通路会真并发，`cargo test` 默认多线程）。
    // 代价是每份多 ≈0.78 s 的 profile 初始化（§三.3）；换来并发互不干扰 + 随 Scratch 一起删净。
    let scratch = Scratch::new(target)?;
    let out = scratch.dir().join("out");
    std::fs::create_dir_all(&out).map_err(|e| AppError::io(&out, &e))?;
    let args = convert_args(
        target,
        &win_slash(&out),
        &win_slash(path),
        &profile_url(&scratch.dir().join("profile")),
    );
    let status = run_bounded(&soffice, &args, CONVERT_TIMEOUT)?;
    // **不信退出码**（spec §三.6：垃圾字节冒充 .doc 也是 exit 0）。唯一的成功判据是产物真的落盘。
    let produced = product_path(&out, path, target);
    if !produced.is_file() {
        return Err(AppError::new(
            "extract_failed",
            &format!("LibreOffice 退出码 {} 但没产出 .{target}（{}）", status.code().unwrap_or(-1), path.display()),
            Some("文件可能损坏、受密码保护，或不是真正的 97-2003 二进制格式"),
        ));
    }
    // 既有抽取链一行不改：docx 走 word/document.xml + <w:t>，pptx 走 ppt/slides/slideN.xml + <a:t>。
    crate::extract::office_text(&produced, slides)
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

/// T5 起由 `start_install`（`libreoffice_install` IPC 命令）构造；**届时删除这条 `dead_code` 豁免**
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

    /// 源名里带点（日期/版本号）时产物名必须**整段 stem** 保留。
    /// 这条是 T1 评审 Important-1 补的：`with_extension` 会把 `.03验收` 当扩展剥掉，
    /// 算出 `合同2024.docx` 而真产物是 `合同2024.03验收.docx` ⇒ T2 假落 failed。
    /// 变异取证：把实现换回 `out_dir.join(stem).with_extension(target)` ⇒ 本条当场红。
    #[test]
    fn product_path_keeps_every_dot_inside_the_source_stem() {
        let p = product_path(Path::new("C:/out"), Path::new("C:/src/合同2024.03验收.doc"), "docx");
        assert_eq!(p, PathBuf::from("C:/out/合同2024.03验收.docx"), "带点号的 stem 一个字符都不许丢：{p:?}");
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
        let t0 = std::time::Instant::now();
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
        assert!(e.message.contains("秒内返回"), "错误要说清是超时：{}", e.message);
        // 光断 Err 测不到「真的把子进程杀了」：ping -n 20 自然跑完要 ≈19 s，
        // 所以 elapsed 必须贴着预算（800 ms）而不是跟着 ping 走 —— 少这一句，删掉 kill 也测不出差别。
        let el = t0.elapsed();
        assert!(el < Duration::from_secs(5), "超时要真的 kill，而不是等子进程自己跑完（实得 {el:?}）");
    }

    #[test]
    fn run_bounded_passes_a_fast_child_through() {
        let cmd = std::env::var_os("COMSPEC").map(PathBuf::from).expect("Windows 上必有 cmd.exe");
        let st = run_bounded(&cmd, &["/c".to_owned(), "exit 0".to_owned()], Duration::from_secs(5))
            .expect("快命令不该被判超时");
        assert!(st.success(), "退出状态要原样交回调用方（它还不信呢）");
    }

    #[test]
    fn probe_returns_a_bool_without_panicking() {
        // 这一条测的是「调用不 panic」——panic 本身就会让测试失败，与断言无关。
        // T1 评审（Important-2，plan-mandated）指出原先的 `assert!(matches!(.., true | false))` 恒真、
        // 零守护，故按本仓「测试必须可证伪」的口径改成显式 `let _`，把意图写在注释里而不是假装在断言。
        let _ = soffice_available();
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

    /// 条件跑：本机装了 LibreOffice 就真转换两枚夹具；没装则显式 SKIP（不假装通过，M6 T2 同形制）。
    /// 哨兵串**必须是纯中文**——实测含拉丁字母的串会被 LibreOffice 按文种切成多个 <w:t>，整串搜不到
    /// （spec §三.13）；失败信息只回长度，绝不回显正文（红线④）。
    #[test]
    fn legacy_text_reads_known_chinese_from_doc_and_ppt() {
        if !soffice_available() {
            eprintln!("SKIP: 本机无 LibreOffice，旧格式转换本体未在真机跑（降级路径由扫描门测守护）");
            return;
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for (file, slides) in [("sample-cn.doc", false), ("sample-cn.ppt", true)] {
            let t = legacy_text(&dir.join(file), slides).unwrap_or_else(|e| panic!("{file} 应能转换：[{}] {}", e.code, e.message));
            assert!(t.contains("合同验收哨兵串"), "{file} 抽出的正文应含已知词（长度 {}）", t.chars().count());
            assert!(t.contains("付款条件与响应时间"), "{file} 第二段也该在（长度 {}）", t.chars().count());
        }
    }

    /// 红线①的自动化版：转换是只读操作——夹具的 size+mtime 逐字不变，且**同目录不许多出任何文件**。
    #[test]
    fn legacy_text_leaves_the_source_directory_untouched() {
        if !soffice_available() {
            eprintln!("SKIP: 本机无 LibreOffice");
            return;
        }
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-cn.doc");
        let before = std::fs::metadata(&fixture).unwrap();
        let siblings = std::fs::read_dir(fixture.parent().unwrap()).unwrap().count();
        let _ = legacy_text(&fixture, false).unwrap();
        let after = std::fs::metadata(&fixture).unwrap();
        assert_eq!((after.len(), after.modified().ok()), (before.len(), before.modified().ok()),
            "转换绝不能写被索引的源文件");
        assert_eq!(std::fs::read_dir(fixture.parent().unwrap()).unwrap().count(), siblings,
            "源目录不能多出任何文件（实测无 .~lock 残留，spec §三.5）");
    }

    /// 转换失败要落 Err 而不是 panic：不存在的输入路径。
    /// （注意**反方向**已被实测证过：垃圾字节冒充 .doc 会 exit 0 并产出正文非空的 docx，
    /// 所以这里不能写成「垃圾文件必错」，那条留到 T7 的端到端里按 `ok`/`skipped` 的现状照实断言。）
    #[test]
    fn legacy_text_reports_a_clean_error_for_a_missing_file() {
        let r = legacy_text(Path::new("C:/definitely/not/here.doc"), false);
        match soffice_path() {
            None => {
                // 没装 LibreOffice 的机器：这一格必须仍然给出可行动的错误，而不是 panic 或空串。
                let e = r.expect_err("没装 LibreOffice 时也要 Err");
                assert_eq!(e.code, "extract_failed");
                assert!(e.hint.is_some(), "要告诉人怎么修");
            }
            Some(_) => {
                let e = r.expect_err("文件不存在该报错");
                assert_eq!(e.code, "extract_failed", "实得：{} {}", e.code, e.message);
            }
        }
    }
}
