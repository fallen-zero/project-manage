# M7 旧格式 .doc/.ppt（LibreOffice headless）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `.doc` / `.ppt`（Office 97-2003 二进制格式）里的中文/英文正文经本机 LibreOffice 无头转换进入全文索引，可被 `search_all` 正文段命中、可被 `doc_preview` 打开高亮；本机没有 LibreOffice 时整类不建行并给出可一键安装的引导。

**Architecture:** 新增纯能力模块 `src-tauri/src/legacy_office.rs`（探测 `soffice_path`/`soffice_available`、参数拼装、一次性临时目录、带超时的子进程执行、`legacy_text`）；转换挂在既有 `extract_text` 分派的**抽取阶段**（`run_pass` 内、写锁外，与 M6 的 `DocKind::Image => ocr::image_text` 一字同形），转出的 docx/pptx 交给**现成的** `extract::office_text(path, slides)` 抽正文后立刻删除临时目录，写库仍走 `write_doc`→`index_text`（jieba）不改分词链。探不到 LibreOffice 时旧格式在 `index_scan` 的扫描门退回「不建行、不占配额」。`IndexOverview` 加**一个** `soffice_available` 字段 + **一条新命令** `libreoffice_install`（本批唯一破结构门处，需求方当次批准 41→42）。

**Tech Stack:** Rust 2021（**零新 crate**：`std::process::Command` + `std::fs` + `std::env`；`tempfile` 只在 `[dev-dependencies]`，生产代码用不了）；外部运行时依赖 LibreOffice 26.8.1.1（`soffice.com`，winget 装的）；rusqlite 0.40.2 bundled / jieba-rs / walkdir 全部沿用。前端 React + TS：`IndexOverview` 一个布尔 + 一行两态提示 + 一个按钮（`@tauri-apps/plugin-opener` 的 `openUrl` 作退路，权限零改动）。

**Spec:** `docs/superpowers/specs/2026-10-13-m7-legacy-office-design.md`（决策 D1–D12、**§三 实测核形 15 组读数**、判据 §五、八格落点 §六、三档测试 §八、真机断言 §九、风险 §十一、回写清单 §十二；分歧以 spec 为准）

## Global Constraints

- **基线**（控制方 2026-10-13 在 `feat/m7-legacy-office` 起点 `265e707` 现取）：`cd src-tauri && cargo test -j 1 --lib` = **167 passed / exit=0**；两条 clippy（`--lib -- -D warnings`、`--lib --all-targets -- -D warnings`）exit 0 且 0 warning；`node --test "tests/**/*.test.ts"`（**在仓库根跑**，在 `src-tauri/` 里跑会读到假的 `tests 0/pass 0`）= **tests 8 / pass 8 / fail 0**；`npm run build` exit 0 且 `error TS` 计数 0；结构门 `generate_handler!` = **41**、`TARGET_VERSION` = **4**、`allow(dead_code)` 只剩 `db.rs:30`。
- **每个 Task 的 Expected 条数必须实跑数出来**（`cargo test -j 1 --lib` 的 `test result` 行 + 逐条数本任务列出的测试函数名），**不许凭印象累加**（本仓坑 5）。下面每步写的 `167 → 177` 一类数字是控制方按本计划逐条数出的**待验证期望**，实跑不一致就停下报控制方。
- 命令显式锚目录（cwd 残留会让 cargo exit=101、让 `node --test` 读到 0 条）；退出码一律 `cmd > /tmp/x.log 2>&1; echo "exit=$?"; grep … /tmp/x.log`，**绝不** `cmd | tail; echo $?`（管道里 `$?` 是 `tail` 的）；16 G 机上 cargo 一律 `-j 1` 或 `-j 2`，偶报 `0xc0000409` / `os error 1455` **先重跑一次再判**（坑 3）。
- **文档里的行号只当取数命令用**（`grep -n` 现取），本计划给的 `file:line` 是 2026-10-13 的读数，动过就漂。
- **本批有意破一条门：`generate_handler!` 41 → 42**（新增 `libreoffice_install`，需求方 2026-10-13 当次批准）。其余照守：**零新依赖**、**零新迁移**（`TARGET_VERSION` 仍 4，不写任何 `settings` 新键）、**不扩码表**（旧格式失败一律 `extract_failed`，预览三成因仍共用 `preview_unavailable`）、**不扩 `index_status` / `skip_reason` 值域**、**线格式只加一个字段**（`IndexOverview.soffice_available` → `sofficeAvailable`；`SearchBundle` 七键、`DocPreview` 六键两条既有断言**字节不动且必须继续绿**）。
- 红线（每派发词带全）：① **绝不动被索引的源文件**——只读、只往自己的临时目录写；spec §三.5 已实测「转换不会在源目录留下 `.~lock` 或任何新文件」，谁改这条写法要重新实测。② 真实数据目录 `%APPDATA%\dev.zero.pfm\` 只读；真机一律一次性 identifier `dev.zero.pfm.m7test`、跑完删净，绝不为验证改 `src-tauri/tauri.conf.json`。③ **子代理一律不跑 `tauri dev`/`npm run dev`**；「UI 实际渲染未验证」如实写进「没能自动验证」。④ **错误信息与测试失败信息绝不回显抽出的正文**，只回路径/退出码/条数/长度。⑤ 输出中文（错误码、SQL、JSON key、命令行参数除外）。⑥ **同一 worktree 只允许一个会提交的实现者**，只读评审可并行。
- **入库/查询同词器红线**：`legacy_text` 抽出的文本经 `write_doc`→`index_text`（jieba `cut_for_search`）入库，与查询侧 `tokenize::query_terms` 同源；M7 一行不改分词链。
- **关于 LibreOffice 的写法一律按 spec §三 的实测，不许凭记忆改回去**：可执行文件是 **`soffice.com`**（不是 `.exe`，同一活儿 6.65 s vs 1.27 s，且 `.exe --version` 零输出）；`--outdir` **必须空格分隔**（等号形被直接拒绝）；`-env:UserInstallation` 必须是**全正斜杠的 `file:///` URL**（混合反斜杠是 14–45 s 的**慢**失败）；**退出码不可信**（垃圾字节冒充 `.doc` 会 exit 0 并产出正文非空的 docx），成功判据只能是「产物存在 + 抽得出内容」；`--convert-to docx`/`pptx` **不写 filter 名**（实测自动选对目标 filter）；转换产物名 = 源 stem + 目标扩展名，**两份同名文件进同一 outdir 会互相覆盖** ⇒ 每个源文件一个独立目录；**夹具与断言用纯中文哨兵串**（含拉丁字母的串会被按文种切成多个 `<w:t>`，整串永远搜不到）。
- **LibreOffice 已在本机装好**（26.8.1.1，`C:\Program Files\LibreOffice\program\soffice.com`）。本批测试会**真起子进程**（每份 ≈1.2 s），所以：跑门禁前确认没有 `tauri dev` 占着 `target/`；不要为「让 CI 更快」把条件测改成无条件跳过。

---

## 文件结构

| 文件 | 动作 | 职责 |
|---|---|---|
| `src-tauri/src/legacy_office.rs` | **新建** | 旧格式转换能力层：候选探测、argv 拼装、Windows 路径形态、一次性目录 guard、带超时的子进程执行、`legacy_text`、winget 安装引导的纯部分。**不碰 SQLite、不碰扫描、不碰 IPC**。 |
| `src-tauri/src/extract.rs` | 改 | `DocKind` 加 `LegacyWord`/`LegacySlides`；`kind_of` 认两个扩展名；`SUPPORTED` 27→29；`extract_text` 加两臂；删掉两处「归 M7」旧文案。 |
| `src-tauri/src/index_scan.rs` | 改 | `ScanOptions` 加 `soffice_available: bool`；扫描门那一处 `match` 加降级臂；`load` 默认值 + 钉测试。 |
| `src-tauri/src/index_job.rs` | 改 | worker 每轮 `ScanOptions::load` 之后填真实探测值；两处测试 `opts()` 构造补字段。 |
| `src-tauri/src/lib.rs` | 改 | `mod legacy_office;`；`IndexOverview` 加一个字段并填活探测值；**新命令 `libreoffice_install` + 注册第 42 条**。 |
| `src/types/index.ts` + `src/lib/api.ts` + `src/pages/index-status.tsx` | 改 | 类型加 `sofficeAvailable`；`api` 加 `installLibreOffice()`；`/index` 那句旧文案改准 + 一行两态提示 + 「安装 LibreOffice」按钮（`openUrl` 退路）。 |
| `src-tauri/tests/fixtures/sample-cn.doc` / `sample-cn.ppt` | 新建 | 真二进制 OLE 夹具（魔数 `D0CF11E0A1B11AE1`，9,216 B / 606,208 B），正文含纯中文哨兵「合同验收哨兵串」「付款条件与响应时间」。 |

依赖序：**T1（legacy_office 纯核）→ T2（legacy_text + 夹具）→ T3（extract 接线）→ T4（扫描门 + worker）→ T5（新命令）→ T6（概览字段 + 前端）→ T7（端到端）→ T8（真机 + 文档，控制方）**。T5/T6 可在 T4 之后并行评审（T6 读 T5 的返回形状）。

---

## Task 1: `legacy_office.rs` 的纯核（探测 / argv 拼装 / 临时目录 / 超时执行）

**Files:**
- Create: `src-tauri/src/legacy_office.rs`
- Modify: `src-tauri/src/lib.rs`（`mod` 区加 `mod legacy_office;`）
- Test: `src-tauri/src/legacy_office.rs` 的 `mod tests`

**Interfaces:**
- Consumes: `crate::error::{AppError, AppResult}`（`AppError::new(code:&'static str, message:&str, hint:Option<&str>)`）。
- Produces（T2/T3/T4/T5 全依赖，签名照抄不许改）:
  - `pub const CONVERT_TIMEOUT: std::time::Duration`（60 s）
  - `pub fn soffice_candidates(program_files: Option<&std::ffi::OsStr>, program_files_x86: Option<&std::ffi::OsStr>, path_var: Option<&std::ffi::OsStr>) -> Vec<std::path::PathBuf>`
  - `pub fn soffice_path() -> Option<std::path::PathBuf>` / `pub fn soffice_available() -> bool`
  - `pub fn win_slash(p: &std::path::Path) -> String`
  - `pub fn profile_url(profile_dir: &std::path::Path) -> String`
  - `pub fn convert_args(target: &str, out_dir: &str, src: &str, profile_url: &str) -> Vec<String>`
  - `pub fn product_path(out_dir: &std::path::Path, src: &std::path::Path, target: &str) -> std::path::PathBuf`
  - `pub struct Scratch { … }`，`Scratch::new(tag:&str) -> AppResult<Scratch>`、`Scratch::dir(&self) -> &std::path::Path`，`Drop` 删目录
  - `pub fn run_bounded(program:&std::path::Path, args:&[String], budget:std::time::Duration) -> AppResult<std::process::ExitStatus>`
  - `pub fn winget_path() -> Option<std::path::PathBuf>`、`pub fn winget_args() -> Vec<String>`、`pub struct InstallOutcome { pub started_via_winget: bool }`（`#[derive(serde::Serialize)]` + `#[serde(rename_all="camelCase")]`）、`pub fn start_install() -> AppResult<InstallOutcome>`（**T5 用**，本任务只把 `winget_path`/`winget_args`/`InstallOutcome` 建好，`start_install` 留到 T5）

- [ ] **Step 1: 建模块骨架 + 纯核实现（探测与拼装部分）**

`lib.rs` 的 `mod` 区加 `mod legacy_office;`。新建 `src-tauri/src/legacy_office.rs`：

```rust
//! 旧版 Office 二进制格式（.doc/.ppt，Office 97-2003）的正文抽取（M7）。
//! 纯 Rust 没有可靠方案（`calamine`/`dowse` 都只覆盖 docx/xlsx/pptx/xls），所以走**本机 LibreOffice
//! 无头转换**：转成 docx/pptx 后交给 `extract::office_text` 那条既有抽取链，产物用完即删（D2）。
//! 本模块只读源文件、只往自己的临时目录写；探不到 LibreOffice 时由 `index_scan` 的降级门整类不建行（D6）。
//!
//! 下面每一条关于 soffice 行为的论断都指向 spec §三 的实测读数，改动前先重跑
//! `.superpowers/sdd/2026-10-13-m7-legacy-office/probe5.sh`，别把脚本 bug 读成 LibreOffice 的行为。
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};

/// 单份文件的转换预算。实测稳态 1.03–1.19 s/份（spec §三.2），60 s 是「真卡死」与「慢盘」的分界。
/// 之所以必须有这一道：`run_pass` 只在**文件之间**查 cancel，子进程不返回就停不下来。
pub const CONVERT_TIMEOUT: Duration = Duration::from_secs(60);

/// 安装用的包 ID：**硬编码在本机 Rust 侧，绝不取自任何入参**（spec §四 那条「不给前端任意命令执行」）。
pub const WINGET_PACKAGE_ID: &str = "TheDocumentFoundation.LibreOffice";
const LIBREOFFICE_SUBDIR: &str = "LibreOffice";

/// 三个候选位：`ProgramFiles` → `ProgramFiles(x86)` → PATH 各段。**探的是 `soffice.com`**：
/// 实测同一活儿 `.exe` 6.65 s、`.com` 1.27 s，且 `.exe --version` 一个字都不往控制台写（spec §三.1）。
/// 做成吃 `Option<&OsStr>` 的纯函数，是为了让「顺序」与「`.com` 这个文件名」能被单测打红。
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
pub fn soffice_path() -> Option<PathBuf> {
    soffice_candidates(
        std::env::var_os("ProgramFiles").as_deref(),
        std::env::var_os("ProgramFiles(x86)").as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
    .into_iter()
    .find(|p| p.is_file())
}

pub fn soffice_available() -> bool {
    soffice_path().is_some()
}

/// Windows 路径 → **全正斜杠**形式，并剥掉 `\\?\` 那个扩展长度前缀（M6 在 WinRT 上吃过同一课，坑 13）。
/// 已知残缺与 M6 一致：`\\?\UNC\server\share` 剥完变成 `UNC/server/share`，那条路径会转换失败并优雅落
/// `failed`（不 panic）。见测试 `win_slash_on_unc_is_known_broken_like_m6`，别把它悄悄"修"成没测过的样子。
pub fn win_slash(p: &Path) -> String {
    let s = p.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.replace('\\', "/")
}

/// `-env:UserInstallation` 要的 URL。**混合反斜杠的写法是慢失败**（实测 13.8–45 s 才报错，spec §三.4），
/// 所以这里只走 `win_slash`，拼成 `file:///C:/...`。
pub fn profile_url(profile_dir: &Path) -> String {
    format!("file:///{}", win_slash(profile_dir))
}

/// 一次转换的 argv。**顺序与形态都是实测钉死的**：
/// `--outdir` 要**空格分隔**（等号形被直接拒、并回吐整页 usage）；`--convert-to` **不写 filter 名**
/// （实测自动选 `MS Word 2007 XML` / `Impress Office Open XML`）；`-env:` 用等号（那一个是 URL 语法）。
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
pub fn product_path(out_dir: &Path, src: &Path, target: &str) -> PathBuf {
    let stem = src.file_stem().unwrap_or(OsStr::new("doc"));
    out_dir.join(stem).with_extension(target)
}

/// 独立用户配置目录（每进程一个，**故意不删**：实测每次新建 profile 多花 ≈0.78 s/份，spec §三.3；
/// 代价记在 spec §十一.6——`%TEMP%\pfm-m7\` 下会留约 359 KB）。
pub fn profile_dir() -> PathBuf {
    let dir = std::env::temp_dir()
        .join("pfm-m7")
        .join(format!("profile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// 一次性输出目录（每文件一个），Drop 时整棵删掉——成功与失败路径都清（spec §五.6）。
/// 不用 `tempfile`：它在 `[dev-dependencies]`，生产代码拿不到。
pub struct Scratch {
    dir: PathBuf,
}

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
pub fn winget_path() -> Option<PathBuf> {
    std::env::var_os("PATH")
        .as_deref()
        .and_then(|pv| std::env::split_paths(&pv).find(|seg| seg.join("winget.exe").is_file()))
        .map(|seg| seg.join("winget.exe"))
}

/// 安装命令的参数。**包 ID 从这里硬编码**，`libreoffice_install` 那条 IPC 命令不吃任何前端入参。
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    /// true = 已在自己的控制台窗口里起起 `winget install`（UAC 与进度都在那个窗口，本命令不等它结束）；
    /// false = 本机探不到 winget，**由前端 `openUrl` 打开官网下载页**。
    pub started_via_winget: bool,
}
```

- [ ] **Step 2: 写十二条会红的纯核测试**

在 `legacy_office.rs` 末尾加：

```rust
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
```

- [ ] **Step 3: 跑测试确认全绿**

Run: `cd src-tauri && cargo test -j 1 --lib legacy_office:: > /tmp/t1.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t1.log | tail -1`
Expected: exit=0，12 条 legacy_office 测试全 PASS（上面实列 12 个 `#[test]`：`soffice_candidates_order…`、`…_never_offer_the_gui_launcher`、`convert_args_uses_space_form…`、`product_path_reuses…`、`win_slash_flips…`、`win_slash_on_unc…`、`profile_url_is_forward…`、`scratch_dir_disappears…`、`run_bounded_kills…`、`run_bounded_passes…`、`probe_returns_a_bool…`、`winget_args_hardcode…`）。

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/t1a.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t1a.log | tail -1`
Expected: **179 passed**（基线 167 + 本任务 12）。数不出来就停下报控制方，别改测试凑数。

- [ ] **Step 4: 变异取证（三条都要真红）**

① 把 `soffice_candidates` 里的 `"soffice.com"` 改成 `"soffice.exe"` ⇒ `soffice_candidates_never_offer_the_gui_launcher` 红。② 把 `convert_args` 里那两项合成 `"--outdir=<dir>"` 一项 ⇒ `convert_args_uses_space_form_outdir_and_no_filter_name` 红。③ 把 `Scratch` 的 `Drop` 体清空 ⇒ `scratch_dir_disappears_when_it_is_dropped` 红。④ 把 `run_bounded` 的超时 `return Err` 改成 `continue` ⇒ `run_bounded_kills_a_child_that_overruns_the_budget` 红（且会慢到 ping 结束，正是要防的形态）。逐条还原。

- [ ] **Step 5: 两条 clippy + 结构门**

Run: `cd src-tauri && cargo clippy -j 1 --lib -- -D warnings > /tmp/t1c.log 2>&1; echo "clippy_lib=$?"; grep -c "warning" /tmp/t1c.log; cargo clippy -j 1 --lib --all-targets -- -D warnings > /tmp/t1d.log 2>&1; echo "clippy_all=$?"; grep -c "warning" /tmp/t1d.log`
Expected: 两条 exit=0、warning 计数 0。
注意：`InstallOutcome` 此刻**还没有生产构造点**，`--all-targets` 那条会报 `dead_code`。**处理方式**：给 `InstallOutcome` 加 `#[allow(dead_code)]` 并在注释里点名「T5 起由 `libreoffice_install` 构造，届时删除豁免」——同时记进账本，T5 收口时必须把这条豁免删掉（M6 就是这么收的：终态 `allow(dead_code)` 只剩 `db.rs:30`）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/legacy_office.rs src-tauri/src/lib.rs
git commit -m "feat(m7): legacy_office 纯核（探 soffice.com / argv 实测形态 / 一次性目录 guard / 超时执行）"
```

---

## Task 2: `legacy_text` 本体 + 两枚二进制夹具（真转换，条件跑）

**Files:**
- Modify: `src-tauri/src/legacy_office.rs`（加 `legacy_text`）
- Create: `src-tauri/tests/fixtures/sample-cn.doc`（9,216 B）、`src-tauri/tests/fixtures/sample-cn.ppt`（606,208 B）
- Test: `legacy_office.rs` 的 `mod tests`

**Interfaces:**
- Consumes: T1 的 `soffice_path`/`convert_args`/`product_path`/`run_bounded`/`profile_dir`/`profile_url`/`win_slash`/`Scratch`/`CONVERT_TIMEOUT`；`crate::extract::office_text(path: &Path, slides: bool) -> AppResult<String>`（既有）。
- Produces: `pub fn legacy_text(path: &Path, slides: bool) -> AppResult<String>`（T3 的 `extract_text` 两臂调它）。

- [ ] **Step 1: 把控制方已造好的夹具复制进仓（造法可复现，见 spec §三.14）**

```bash
mkdir -p src-tauri/tests/fixtures
cp .superpowers/sdd/2026-10-13-m7-legacy-office/fixtures/sample-cn.doc src-tauri/tests/fixtures/sample-cn.doc
cp .superpowers/sdd/2026-10-13-m7-legacy-office/fixtures/sample-cn.ppt src-tauri/tests/fixtures/sample-cn.ppt
python -c "import sys;[print(f, open(f,'rb').read(8).hex().upper()) for f in sys.argv[1:]]" src-tauri/tests/fixtures/sample-cn.doc src-tauri/tests/fixtures/sample-cn.ppt
```
Expected: 两行都是 `D0CF11E0A1B11AE1`（真 OLE 复合文档魔数，不是 zip 也不是改名的文本）。若目录里找不到这两个夹具，停下报控制方（用 spec §三.14 的 `probe5.sh` 重新生成，**不要**自己手写 XML 现造）。

- [ ] **Step 2: 写三条会红的条件测试**

```rust
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
```

Run: `cd src-tauri && cargo test -j 1 --lib legacy_office:: > /tmp/t2r.log 2>&1; echo "exit=$?"; tail -20 /tmp/t2r.log`
Expected: 编译失败 `cannot find function legacy_text in this scope` —— 这是合法 RED（新函数只到编译层，变异取证在 Step 4 用真转换补）。

- [ ] **Step 3: 实现 `legacy_text`**

```rust
/// 把 `.doc` / `.ppt` 转成 docx/pptx 再抽出正文。**在抽取阶段（写锁外）被调用**（D1）——
/// 一份 ≈1.2 s 的进程冷启动只拖长这一轮，不进任何单笔写锁。
/// 产物只当管道：落到 `Scratch`（每文件一个目录，避开同名覆盖）→ 交给既有 `office_text` 抽正文
/// → 目录随 `Scratch` 析构删掉，库里只留正文（D2，零新表零新体积账）。
pub fn legacy_text(path: &Path, slides: bool) -> AppResult<String> {
    let target = if slides { "pptx" } else { "docx" };
    let soffice = soffice_path().ok_or_else(|| AppError::new(
        "extract_failed",
        &format!("未找到 LibreOffice，无法读取旧格式（{}）", path.display()),
        Some("在「全文索引」页的支持类型卡片里点「安装 LibreOffice」；装完不用重启，下一轮自动开始索引"),
    ))?;
    let scratch = Scratch::new(target)?;
    let args = convert_args(
        target,
        &win_slash(scratch.dir()),
        &win_slash(path),
        &profile_url(&profile_dir()),
    );
    let status = run_bounded(&soffice, &args, CONVERT_TIMEOUT)?;
    // **不信退出码**（spec §三.6：垃圾字节冒充 .doc 也是 exit 0）。唯一的成功判据是产物真的落盘。
    let produced = product_path(scratch.dir(), path, target);
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
```

- [ ] **Step 4: 跑绿 + 全量 + 变异取证**

Run: `cd src-tauri && cargo test -j 1 --lib legacy_office:: > /tmp/t2.log 2>&1; echo "exit=$?"; grep -E "^test legacy_office|test result" /tmp/t2.log`
Expected: 三条新测试 PASS（本机装有 LibreOffice ⇒ 不 SKIP）。

变异取证三条：① 把 `if !produced.is_file()` 整块删掉 ⇒ `legacy_text_reports_a_clean_error_for_a_missing_file` 在有 LibreOffice 的分支红（缺文件时产物也不存在，会一路走到 `office_text` 拿到另一个码）。② 把 `convert_args(...)` 里传的 `&profile_url(&profile_dir())` 换成直接塞 `profile_dir().display().to_string()`（即带反斜杠、无 `file:///`）⇒ `legacy_text_reads_known_chinese_from_doc_and_ppt` 红，且**慢**（spec §三.4 实测混合分隔符是 14–45 s 的慢失败，这条变异会让测试时长明显跳起来——那个跳起来本身就是证据）。③ 把 `Slides` 判据写反（`let target = if slides { "docx" } else { "pptx" };`）⇒ 同一条件测试的 `.ppt` 那一圈红（pptx 里找 `word/document.xml` 必失败）。逐条还原。
> **不在变异清单里的（如实说）**：把源路径的 `win_slash(path)` 换成 `path.to_string_lossy()`（带反斜杠、可能带 `\\?\`）**预期会红但本计划不列为取证项**——因为「LibreOffice 收不收反斜杠源路径」我**没测过**（探针全程只喂正斜杠形式）。实现者若想知道，跑一次单独看结果并写进账本，别把它当已证的守护。

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/t2a.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t2a.log | tail -1`
Expected: **182 passed**（179 + 3）。

- [ ] **Step 5: clippy + 提交**

Run: `cd src-tauri && cargo clippy -j 1 --lib --all-targets -- -D warnings > /tmp/t2c.log 2>&1; echo "clippy=$?"; grep -c "warning" /tmp/t2c.log`
Expected: exit=0、0 warning。

```bash
git add src-tauri/src/legacy_office.rs src-tauri/tests/fixtures/sample-cn.doc src-tauri/tests/fixtures/sample-cn.ppt
git commit -m "feat(m7): legacy_text 现转 docx/pptx 交既有 office_text 抽正文；附两枚二进制 OLE 夹具"
```

---

## Task 3: `extract.rs` 接线（两个 DocKind + kind_of + SUPPORTED 27→29 + 删两处旧文案）

**Files:**
- Modify: `src-tauri/src/extract.rs`（`enum DocKind`、`kind_of`、`SUPPORTED`、`extract_text`、`None` 臂 hint、`TEXT_EXTS` 上方注释）
- Test: `extract.rs` 的 `mod tests`

**Interfaces:**
- Consumes: `legacy_office::legacy_text(&Path, bool) -> AppResult<String>`（T2）。
- Produces: `DocKind::LegacyWord` / `DocKind::LegacySlides`（T4 的降级门按这两个 variant 匹配）；`extract_text` 能吃 `.doc`/`.ppt`；`supported_exts()` 长度 29（T6 的 UI 自动带出）。

- [ ] **Step 1: 写会红的分派测试**

```rust
    /// M7：`.doc`/`.ppt` 必须**同时**满足两件事——`kind_of` 分派到两个新 variant（抽取侧）、
    /// 且出现在 `supported_exts()`（UI 承诺侧）。与 M6 的 `png_kind_of…` 同形。
    #[test]
    fn legacy_office_exts_dispatch_and_are_listed() {
        assert!(matches!(kind_of("doc"), Some(DocKind::LegacyWord)), ".doc 应分派到 LegacyWord");
        assert!(matches!(kind_of("ppt"), Some(DocKind::LegacySlides)), ".ppt 应分派到 LegacySlides");
        assert!(matches!(kind_of("DOC"), Some(DocKind::LegacyWord)), "大小写不敏感");
        for e in ["doc", "ppt"] {
            assert!(supported_exts().contains(&e), "{e} 应在 UI 清单");
        }
    }
```

- [ ] **Step 2: 跑红**

Run: `cd src-tauri && cargo test -j 1 --lib extract:: > /tmp/t3r.log 2>&1; echo "exit=$?"; grep -E "error|cannot find" /tmp/t3r.log | head -5`
Expected: 编译错误（`LegacyWord`  variant 不存在）。

- [ ] **Step 3: 实现四处**

1. `enum DocKind`（`extract.rs:199`）加两个 variant：
```rust
    /// M7：旧版 Office 二进制格式（Word 97-2003 / PowerPoint 97-2003），
    /// 经本机 LibreOffice 现转 docx/pptx 后走 `office_text`（`legacy_office::legacy_text`）。
    LegacyWord,
    LegacySlides,
```
2. `kind_of`（`:230`）的 `match` 里，在图片那行之后加：
```rust
            "doc" => Some(DocKind::LegacyWord),
            "ppt" => Some(DocKind::LegacySlides),
```
3. `SUPPORTED`（`:220`）加两项，末尾变成 `…, "webp", "bmp", "doc", "ppt"` ⇒ 共 **29** 项。**`TEXT_EXTS` 不动**（旧格式不是纯文本，与 M6 对图片的处理同形）。
4. `extract_text`（`:295`）加两臂，并把 `None` 臂的 hint 改准：
```rust
        Some(DocKind::LegacyWord) => crate::legacy_office::legacy_text(path, false),
        Some(DocKind::LegacySlides) => crate::legacy_office::legacy_text(path, true),
        None => Err(fail(
            "extract_unsupported",
            &format!("不支持的文件类型 .{ext}"),
            "文本/Office（含 .doc/.ppt）/PDF/图片都已接入；压缩包等类型仍不索引",
        )),
```
5. 删掉 `TEXT_EXTS` 上方注释里那句已过时的「**`.doc`/`.ppt` 属 M7（要 LibreOffice headless），本版本不登记、不建行**」（`extract.rs:211`），改写成：
```rust
/// 旧版 .doc/.ppt 自 M7 起走 `DocKind::LegacyWord`/`LegacySlides` 的转换分支（在 `SUPPORTED` 里，
/// 但**不是**纯文本，别塞进这张表）。
```

- [ ] **Step 4: 三处同源 + 全量 + clippy**

Run: `cd src-tauri && cargo test -j 1 --lib extract:: > /tmp/t3.log 2>&1; echo "exit=$?"; grep -E "supported_exts_list|legacy_office_exts" /tmp/t3.log`
Expected: `supported_exts_list_matches_the_dispatch_table`（既有，双向查）与新那条都 PASS。

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/t3a.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t3a.log | tail -1`
Expected: **183 passed**（182 + 1）。
**注意可能的连带红**（控制方 2026-10-13 已 grep 预判为「无」）：全仓既有测试里没有任何 `.doc`/`.ppt` 夹具、也没有任何断言清单条数为 27 的用例，所以本任务预期**不需要**改任何既有测试期望——若实跑出现别的用例红（例如有人新增了含 `.doc` 的「未支持不建行」夹具），按 M6 T3 的先例**只改测试期望/夹具、不碰逻辑**，并把改动写进账本。

Run: `cd src-tauri && cargo clippy -j 1 --lib --all-targets -- -D warnings > /tmp/t3c.log 2>&1; echo "clippy=$?"; grep -c "warning" /tmp/t3c.log`
Expected: exit=0、0 warning。

- [ ] **Step 5: 变异取证**

只往 `SUPPORTED` 加 `"doc"` 而不加 `kind_of` ⇒ `supported_exts_list_matches_the_dispatch_table` 红（清单有、分派无）。只加 `kind_of` 不加 `SUPPORTED` ⇒ 同一条反向红（`能分派却不在 UI 清单里`）。两处都加但漏 `extract_text` 的臂 ⇒ 编译失败（`match` 非穷尽），同样打回。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/extract.rs
git commit -m "feat(m7): extract 分派 .doc/.ppt → LegacyWord/LegacySlides；SUPPORTED 27→29 并删两处「归 M7」旧文案"
```

---

## Task 4: 探不到 LibreOffice 时「不建行」（扫描门单一落点）+ worker 每轮填真实值

**Files:**
- Modify: `src-tauri/src/index_scan.rs`（`ScanOptions` 加字段、`load` 默认、门那一处 `match`、测试 `opts()`）
- Modify: `src-tauri/src/index_job.rs`（worker 填探测值、测试 `opts()`）
- Test: `index_scan.rs` 的 `mod tests`

**Interfaces:**
- Consumes: `legacy_office::soffice_available()`（T1）、`DocKind::LegacyWord`/`LegacySlides`（T3）。
- Produces: `ScanOptions { .., pub soffice_available: bool }`（`index_job` 的 worker 与 `index_overview` 都用它）。

- [ ] **Step 1: 写三条会红的测试**

```rust
    /// D6：探不到 LibreOffice 时，`.doc`/`.ppt` 与今天的未支持类型**完全同路**——
    /// 不建行、不进 `over_size`、不占单项目配额。降级门只有这一处落点。
    #[test]
    fn legacy_docs_are_not_scanned_when_soffice_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("旧报告.doc"), b"x").unwrap();
        std::fs::write(dir.path().join("旧片子.ppt"), b"x").unwrap();
        std::fs::write(dir.path().join("新说明.txt"), b"hello").unwrap();
        let mut o = opts(&[]);
        o.soffice_available = false;
        let out = scan_root(dir.path(), &o);
        assert!(out.files.iter().all(|f| f.ext != "doc" && f.ext != "ppt"),
            "无 LibreOffice 时旧格式不该进清单：{:?}", out.files.iter().map(|f| &f.ext).collect::<Vec<_>>());
        assert!(out.over_size.iter().all(|f| f.ext != "doc"), "也不该进超限桶（不占配额）");
        assert!(out.files.iter().any(|f| f.ext == "txt"), "纯文本不受影响");
    }

    /// 对照臂：可用时放行建行（扫描层只放行，真转换在抽取阶段，别在这里跑子进程）。
    #[test]
    fn legacy_docs_are_scanned_when_soffice_is_available() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("旧报告.doc"), b"x").unwrap();
        let out = scan_root(dir.path(), &opts(&[])); // 测试用 opts() 默认 soffice_available=true
        assert!(out.files.iter().any(|f| f.ext == "doc"), "可用时 .doc 应放行：{:?}", out.files.iter().map(|f| &f.ext).collect::<Vec<_>>());
    }

    /// 兑 M6 终审 Minor-6 的同形账：`load` 的两个能力默认值**必须有测试钉住**。
    /// 默认 true 是惰性兜底（生产由 worker 覆盖成活探测值），两条消费路径从不喂给 scan_root；
    /// 没有这一条，谁把默认改成 false 也不会有任何测试变红。
    #[test]
    fn load_defaults_both_capability_flags_to_true() {
        let conn = crate::db::open_in_memory().unwrap();
        let o = ScanOptions::load(&conn).unwrap();
        assert!(o.ocr_available, "load 的 OCR 默认值（生产会被 worker 覆盖）");
        assert!(o.soffice_available, "load 的 LibreOffice 默认值（同上）");
    }
```

Run: `cd src-tauri && cargo test -j 1 --lib index_scan:: > /tmp/t4r.log 2>&1; echo "exit=$?"; grep -E "error\[|no field|test result" /tmp/t4r.log | head -5`
Expected: 编译错误 `struct ScanOptions has no field named soffice_available` —— 合法 RED。

- [ ] **Step 2: 实现三处**

`index_scan.rs`：
1. `ScanOptions` 加字段（紧跟 `ocr_available`）：
```rust
    /// 本机有没有 LibreOffice 可用（M7 D6）。`load` 只给保守默认 true，真实探测值由
    /// `index_job` 的 worker 每轮现探后填进来（D5：**不记忆化**，换来「一键装完不用重启」）；
    /// false 时 `.doc`/`.ppt` 在扫描门视同未支持类型。
    pub soffice_available: bool,
```
2. `load` 的构造里加 `soffice_available: true,`，并在 `ocr_available: true` 那行注释旁补一句「默认值的两条一起由 `load_defaults_both_capability_flags_to_true` 钉」。
3. 降级门那一处 `match`（`index_scan.rs:126`）扩成：
```rust
                match kind_of(&scanned.ext) {
                    None => continue, // 未支持类型不建行，也不占配额，见测试里的说明
                    // D4：无 OCR 时图片同未支持 —— 不建行不占配额（降级门在这一处而非 run_pass）。
                    Some(crate::extract::DocKind::Image) if !opts.ocr_available => continue,
                    // M7 D6：无 LibreOffice 时旧格式同未支持。**全仓只有这一处**该判据（spec §五.3），
                    // 出现第二处就会造出「UI 说支持、扫描又挡一遍」的双口径。
                    Some(crate::extract::DocKind::LegacyWord) | Some(crate::extract::DocKind::LegacySlides)
                        if !opts.soffice_available => continue,
                    _ => {}
                }
```

`index_job.rs`：
4. worker 线程体里（`index_job.rs:472` 那次 `ScanOptions::load` 成功之后、紧跟 M6 那句 `opts.ocr_available = …`）加：
```rust
                    opts.soffice_available = crate::legacy_office::soffice_available();
```
5. 两处测试 `opts()` 构造补字段：`index_job.rs:527` 的 `ScanOptions { …, ocr_available: true, soffice_available: true }`、`index_scan.rs:159` 的 `opts()` 同理（默认 true，让「可用」成为测试基线，反向由上面第一条显式关掉）。

- [ ] **Step 3: 跑绿 + 全量 + clippy**

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/t4.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t4.log | tail -1`
Expected: **186 passed**（183 + 3），且既有 `unsupported_extensions_produce_no_rows`、`per_project_cap…`、`over_size` 相关用例**不回归**。

Run: `cd src-tauri && cargo clippy -j 1 --lib -- -D warnings > /tmp/t4c.log 2>&1; echo "c1=$?"; cargo clippy -j 1 --lib --all-targets -- -D warnings > /tmp/t4d.log 2>&1; echo "c2=$?"; grep -c warning /tmp/t4c.log /tmp/t4d.log`
Expected: 两条 exit=0、0 warning。

- [ ] **Step 4: 变异取证 + 单一落点门**

① 删掉那条 `Some(LegacyWord)|Some(LegacySlides) if !opts.soffice_available => continue` ⇒ `legacy_docs_are_not_scanned_when_soffice_is_unavailable` 红。② 把 `index_job.rs` 里那句 worker 覆盖删掉 ⇒ 现有 186 条**不会红**（本机装了 LibreOffice ⇒ 探测值与默认值都是 true，形态与 M6 的 set-but-never-read 完全同形）；所以这一格只能靠**结构门 grep** 守，跑：
Run: `cd src-tauri && grep -rn "soffice_available" src/ | grep -v "^src/legacy_office.rs" `
Expected 命中恰好三处：`index_scan.rs` 的字段声明 + 那条降级臂（含 `load` 的默认值行与注释各一次，按实际输出如实登记）、`index_job.rs` worker 那一句。若 `index_job.rs` 那一处不见了 ⇒ 接线被删，报出来。
③ 全仓再搜第二落点：`grep -rn "soffice_available" src/ | grep -v index_scan.rs | grep -v index_job.rs | grep -v legacy_office.rs` 必须 **exit=1**（`lib.rs` 的 `index_overview` 读的是 `soffice_available()` 活探测**函数**、不是这个字段，T6 落地时按此核对；若 T6 之后这一条命中了 `lib.rs` 里的 `opts.soffice_available`，说明 UI 读到了惰性默认，判为缺陷）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/index_scan.rs src-tauri/src/index_job.rs
git commit -m "feat(m7): 探不到 LibreOffice 时旧格式在扫描门退回不建行（D6 单一落点）+ worker 每轮现探（D5）"
```

---

## Task 5: 新命令 `libreoffice_install`（第 42 条 IPC，零参数 + 硬编码包 ID）

**Files:**
- Modify: `src-tauri/src/legacy_office.rs`（加 `start_install`，**删掉 T1 给 `InstallOutcome` 的 `allow(dead_code)` 豁免**）
- Modify: `src-tauri/src/lib.rs`（命令函数 + `generate_handler!` 注册第 42 条）
- Test: `legacy_office.rs` 与 `lib.rs` 的 `mod tests`

**Interfaces:**
- Consumes: T1 的 `winget_path()`/`winget_args()`/`InstallOutcome`。
- Produces: IPC `libreoffice_install` → `{ startedViaWinget: boolean }`（T6 的前端按这个布尔决定要不要 `openUrl`）。

- [ ] **Step 1: 写会红的两条守护测试**

`legacy_office.rs` 的 `mod tests` 加：
```rust
    /// 「不给前端任意命令执行」这条红线的**形状**守护：安装命令的参数表里只能出现硬编码字符串，
    /// 且包 ID 是常量（spec §四）。把 WINGET_PACKAGE_ID 改成吃入参的话，这条与下一条一起红。
    #[test]
    fn start_install_takes_no_arguments() {
        // 编译期形状断言：签名必须是 `fn() -> AppResult<InstallOutcome>`。
        // 给 start_install 加任何一个入参，这一行就编不过 —— 这就是它的守护，不需要跑起来。
        const _: fn() -> AppResult<InstallOutcome> = start_install;
    }

    #[test]
    fn install_never_launches_a_soffice_child_directly() {
        // 安装走的是 winget，不是 soffice：argv 表里不许混进转换参数，也不许拼前端来的字符串。
        let a = winget_args();
        assert_eq!(a.first().unwrap(), "install");
        assert!(a.iter().all(|x| !x.starts_with("--convert-to") && !x.contains("file:///")),
            "安装参数与转换参数必须是两套：{a:?}");
    }
```

`lib.rs` 的 `mod tests`（M5.5 起已存在，含 `bootstrap_data_opens_a_wal_db_at_the_given_dir`）加：
```rust
    /// 第 42 条命令必须是**零参数**的（spec §四：这条命令能启动外部安装器，
    /// 任何入参都是任意命令执行的入口）。签名一变，这一行编不过。
    #[test]
    fn libreoffice_install_command_takes_no_arguments() {
        const _: fn() -> crate::error::AppResult<crate::legacy_office::InstallOutcome> = libreoffice_install;
    }
```

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/t5r.log 2>&1; echo "exit=$?"; grep -E "^error|cannot find" /tmp/t5r.log | head -6`
Expected: 编译失败（`start_install`、`libreoffice_install` 未定义）—— 合法 RED。

- [ ] **Step 2: 实现 `start_install`**

`legacy_office.rs` 加（同时**删除 `InstallOutcome` 上的 `#[allow(dead_code)]` 与那条「T5 删豁免」注释**）：
```rust
/// 一键安装：探到 winget 就在**自己的控制台窗口**里起安装，本函数不等它结束（fire-and-forget）。
/// 为什么不等：等就得让这条 sync 命令睡住消息泵几分钟（M5 finding 1 的机制），而 UAC 与下载进度
/// 本来就在 winget 自己的窗口里，用户看得见。探不到 winget（LTSC/组策略机）返回 false，
/// 由前端 `openUrl` 官网页 —— 退路不占第二条 IPC，也不把插件的 Rust API 拉进命令层。
///
/// **零参数 + 包 ID 硬编码**是这条命令的红线形状，由 `start_install_takes_no_arguments` 与
/// `winget_args_hardcode_the_package_id` 两条钉住。`CREATE_NEW_CONSOLE` = 0x00000010。
pub fn start_install() -> AppResult<InstallOutcome> {
    let Some(winget) = winget_path() else {
        return Ok(InstallOutcome { started_via_winget: false });
    };
    use std::os::windows::process::CommandExt as _;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    Command::new(&winget)
        .args(winget_args())
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map_err(|e| AppError::new(
            "fs_failed",
            &format!("启动 winget 安装失败：{e}"),
            Some("也可以用浏览器打开 LibreOffice 官网下载页手动安装（文档基金会，免费）"),
        ))?;
    Ok(InstallOutcome { started_via_winget: true })
}
```

- [ ] **Step 3: 加命令并注册第 42 条**

`lib.rs`：
```rust
/// 一键安装 LibreOffice（M7 D8）。**零参数**：包 ID 与全部 argv 都硬编码在 `legacy_office`。
/// 返回 `startedViaWinget=false` 表示本机没有 winget，前端要退到「打开官网下载页」。
/// 这是全仓第 42 条命令，也是唯一一条会启动外部安装器的命令 —— spec §四 单列了理由与两条守护测试。
#[tauri::command]
fn libreoffice_install() -> AppResult<legacy_office::InstallOutcome> {
    legacy_office::start_install()
}
```
注册表（`lib.rs:645` 的 `generate_handler![`）里，把最后一项 `doc_preview` 补上逗号后追加一行：
```rust
            doc_preview,
            libreoffice_install
        ])
```

- [ ] **Step 4: 全量 + 结构门**

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/t5.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t5.log | tail -1`
Expected: **189 passed**（186 + 3：`start_install_takes_no_arguments`、`install_never_launches…`、`libreoffice_install_command_takes_no_arguments`）。

Run（本批唯一破例那条门，正则**必须让逗号可选**，要求尾逗号那版恒读 40）：
`cd src-tauri && grep -A60 "generate_handler!" src/lib.rs | grep -cE "^ +[a-z_]+,?$"`
Expected: **42**（M0–M6 是 41）。

Run: `cd src-tauri && grep -rn "allow(dead_code)" src/`
Expected: 只剩 `src/db.rs:30` 一处 —— T1 那条豁免必须已删。若 `legacy_office.rs` 还在，判为本任务未完成。

Run: `cd src-tauri && cargo clippy -j 1 --lib -- -D warnings > /tmp/t5c.log 2>&1; echo "c1=$?"; cargo clippy -j 1 --lib --all-targets -- -D warnings > /tmp/t5d.log 2>&1; echo "c2=$?"; grep -c warning /tmp/t5c.log /tmp/t5d.log`
Expected: 两条 exit=0、0 warning。

- [ ] **Step 5: 变异取证**

① 给 `start_install` 加一个 `id: String` 入参并把 `winget_args()` 换成 `vec!["install".into(), "--id".into(), id]` ⇒ `start_install_takes_no_arguments` 编译期红。② 把 `libreoffice_install` 改成吃 `state: State<AppState>` ⇒ `libreoffice_install_command_takes_no_arguments` 红。③ 把 `WINGET_PACKAGE_ID` 改成别的串 ⇒ `winget_args_hardcode_the_package_id`（T1 那条）红。逐条还原。
（注：`const _: fn() -> … = f;` 这类形状断言的变异**必然表现为编译失败**，属本仓认可的「新函数只到编译层」取证形态，账本里写清是编译层红而非断言红。）

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/legacy_office.rs src-tauri/src/lib.rs
git commit -m "feat(m7): 新命令 libreoffice_install（第 42 条 IPC，零参数 + 硬编码包 ID，winget 起窗即返回）"
```

---

## Task 6: `IndexOverview.sofficeAvailable` + `/index` 两态提示与安装按钮

**Files:**
- Modify: `src-tauri/src/lib.rs`（`IndexOverview` 加字段、`index_overview` 填活探测值）
- Modify: `src/types/index.ts`、`src/lib/api.ts`、`src/pages/index-status.tsx`
- Test: 无新增 Rust 测试；靠 `npm run build`（含 `tsc`）+ 一条变异取证

**Interfaces:**
- Consumes: `legacy_office::soffice_available()`、`libreoffice_install`（T5）。
- Produces: 前端 `IndexOverview.sofficeAvailable: boolean` 与 `installLibreOffice(): Promise<{startedViaWinget:boolean}>`。

- [ ] **Step 1: Rust 侧加字段并填活探测值**

`lib.rs` 的 `struct IndexOverview`（`#[serde(rename_all = "camelCase")]`，`:448`）在 `ocr_available` 之后加：
```rust
    /// 本机有没有 LibreOffice 可用（`legacy_office::soffice_available()` 的**活探测**值）。
    /// 与 `ocr_available` 同一措辞理由：不透出 `ScanOptions::load` 的惰性默认 —— 那两条消费路径
    /// 从不喂给 `scan_root`，set-but-never-read；UI 必须看到真实探测，否则「装完不用重启」这句承诺会假。
    soffice_available: bool,
```
`index_overview` 的构造处（`:527` 那行 `ocr_available` 之后）填：
```rust
        soffice_available: crate::legacy_office::soffice_available(),
```

- [ ] **Step 2: 前端类型与 api 包装**

`src/types/index.ts` 的 `IndexOverview`（`ocrAvailable` 之后）加：
```ts
  /** 本机是否有 LibreOffice 可用（Rust 侧 `legacy_office::soffice_available()` 活探测）。
   *  false 时 `.doc`/`.ppt` 在扫描门整类不建行，界面上给一键安装入口。 */
  sofficeAvailable: boolean;
```
`src/lib/api.ts`：在 `import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";` 那行加上 `openUrl`，并新增：
```ts
export interface LibreOfficeInstallOutcome {
  startedViaWinget: boolean;
}

/** 一键安装 LibreOffice（无参数：包 ID 硬编码在 Rust 侧）。true = 已弹出 winget 窗口，进度与 UAC 都在
 *  那个窗口里，本调用不等安装结束；false = 本机没有 winget，调用方要退到打开官网下载页。 */
export const installLibreOffice = () =>
  invoke<LibreOfficeInstallOutcome>("libreoffice_install");

/** winget 不在时的退路：用系统默认浏览器打开 LibreOffice 下载页（opener:default 已含 https://* scope）。 */
export const openLibreOfficeDownloadPage = () =>
  openUrl("https://www.libreoffice.org/download/download-libreoffice/");
```

- [ ] **Step 3: `/index` 那句旧文案改准 + 两态提示 + 按钮**

`src/pages/index-status.tsx` 第 388 行那段（`清单之外的类型（.doc/.ppt、压缩包）不会建行：.doc/.ppt 归 M7。图片的支持情况见下条 OCR 状态。`）改成：
```tsx
          <p className="text-muted-foreground">
            清单之外的类型（压缩包等）不会建行。旧版 Office 与图片的支持情况见下面两条状态。
          </p>
```
紧跟 OCR 那一行（`ocrAvailable` 那个三元）之后加：
```tsx
          {overview?.sofficeAvailable ? (
            <p className="text-xs text-muted-foreground">旧版 Office（.doc/.ppt）已启用本机 LibreOffice 转换。</p>
          ) : (
            <div className="flex flex-wrap items-center gap-2">
              <p className="text-xs text-muted-foreground">
                未检测到 LibreOffice：.doc/.ppt 暂不索引。装一次即可，装完不用重启，下一轮自动开始建行。
              </p>
              <Button size="sm" variant="outline" disabled={installBusy} onClick={() => {
                setInstallBusy(true);
                // 这一条必须自己带 finally：命令是 fire-and-forget，返回 true 只代表「窗口已弹出」，
                // 装完没装完界面不知道，所以按钮复位只跟着这一次 IPC，不假装反映安装进度。
                void installLibreOffice()
                  .then((r) => {
                    if (!r.startedViaWinget) {
                      void openLibreOfficeDownloadPage().catch(() =>
                        setInstallError({ code: "unknown", message: "已探到本机没有 winget，也没能打开下载页，请手动访问 libreoffice.org", hint: null }),
                      );
                    }
                  })
                  .catch((e) => setInstallError(toAppError(e)))
                  .finally(() => setInstallBusy(false));
              }}>
                {installBusy ? "正在启动…" : "安装 LibreOffice"}
              </Button>
            </div>
          )}
          {installError && (
            <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              <div className="font-medium">启动安装失败</div>
              <div className="mt-0.5 break-all" title={`${installError.message}${installError.hint ? `（${installError.hint}）` : ""}`}>
                {installError.message}
                {installError.hint ? `（${installError.hint}）` : ""}
              </div>
            </div>
          )}
```
组件内 state 区（与 `docsError` 同一处，`:72` 附近）加：
```tsx
  const [installBusy, setInstallBusy] = useState(false);
  const [installError, setInstallError] = useState<AppErrorShape | null>(null);
```
`import` 那行补上 `installLibreOffice, openLibreOfficeDownloadPage`（从 `@/lib/api`）。

> **刻意不复用 store 的 `error`**：那条是索引作业的拒绝/失败口径，`load()` 的成功路径会把它擦成 null（页面里那两大段注释正是为这个写的）；安装按钮的提示必须活过一次总览往返，所以用组件本地 state。

- [ ] **Step 4: 门禁（含前端）+ 接线结构门**

Run: `npm run build > /tmp/t6b.log 2>&1; echo "build=$?"; grep -c "error TS" /tmp/t6b.log`（**仓库根**）
Expected: exit=0、`error TS` 计数 0。

Run: `node --test "tests/**/*.test.ts" > /tmp/t6n.log 2>&1; echo "exit=$?"; grep -E "^# (tests|pass|fail)" /tmp/t6n.log`（**仓库根**）
Expected: tests 8 / pass 8 / fail 0（本批零新前端测试 ⇒ 这条是无回归检查，不充当「前端已验证」的证据，照实写）。

Run: `cd src-tauri && grep -rn "soffice_available" src/lib.rs`
Expected: 两条命中 —— 字段声明（`struct IndexOverview`）与 `index_overview` 的 `crate::legacy_office::soffice_available()` 调用。**不许**出现 `opts.soffice_available`（那说明 UI 读到了惰性默认）。

- [ ] **Step 5: 变异取证（字段接线）**

`npm run build` 之后再手动删掉 `src/types/index.ts` 里那行 `sofficeAvailable: boolean;` ⇒ `npm run build` 必须**当场红**（`index-status.tsx` 读 `overview?.sofficeAvailable` 报 TS 属性不存在）。还原并复跑确认 exit=0。这条证明线格式字段真的接到了界面，不是只加了个键。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/lib.rs src/types/index.ts src/lib/api.ts src/pages/index-status.tsx
git commit -m "feat(m7): IndexOverview 透出 sofficeAvailable + /index 两态提示与一键安装按钮（openUrl 退路）"
```

---

## Task 7: 端到端（headless，真 LibreOffice 驱动整条链）

**Files:**
- Modify: `src-tauri/src/index_job.rs`（测试模块加两条端到端）
- Test: `index_job.rs` 的 `mod tests`

**Interfaces:**
- Consumes: 全部前序任务（`extract_text` 分派 → `legacy_text` → `office_text` → `write_doc` → FTS → `doc_hits` → `doc_preview`）。
- Produces: spec §九.1 的硬证据（M7 主承诺）。

- [ ] **Step 1: 写端到端测试（照 M6 的 `ocr_image_flows_end_to_end…` 形状）**

```rust
    /// M7 端到端（headless，需求方选的路线，同 M6 蓝本）：真 LibreOffice 驱动整条链 ——
    /// 一份含已知中文的 .doc 复制进 tempdir → `run_pass` → `extract_text` → `legacy_office::legacy_text`
    /// → `office_text`(docx) → `write_doc`(jieba) 入库 → `doc_hits` 搜得到 → `render_preview` 出正文
    /// → 第二趟 `unchanged` 证「不重转」。覆盖 spec §九 断言 1 与 §八 的 1/3 格，并当场证红线①。
    /// 没装 LibreOffice 显式 SKIP，不假装通过。
    #[test]
    fn legacy_doc_flows_end_to_end_through_index_and_search() {
        if !crate::legacy_office::soffice_available() {
            eprintln!("SKIP: 本机无 LibreOffice，旧格式端到端不假装通过（降级由扫描门测守护）");
            return;
        }
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-cn.doc");
        let src_before = std::fs::metadata(&fixture).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("旧合同.doc");
        std::fs::copy(&fixture, &doc).unwrap();
        let doc_before = std::fs::metadata(&doc).unwrap();
        // txt 刻意不含「合同验收」，别让正文段命中被夹具之外的东西稀释。
        std::fs::write(dir.path().join("台账.txt"), "项目 台账 摘要".as_bytes()).unwrap();

        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());

        // §九.1：一轮后 .doc 行 ok。opts() 默认 soffice_available=true（可用那一支）。
        let summary = run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(summary.state, "done");
        let status: String = conn
            .query_row("SELECT index_status FROM index_docs WHERE path LIKE '%旧合同.doc'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "ok", "真转换应入库 ok（不是 failed/skipped）");

        // 正文段可搜：无空格原词命中（转换抽出的正文经既有 jieba 入库，分词链未动）。
        let hits = crate::index_store::doc_hits(&conn, "合同验收", 20).unwrap();
        assert!(hits.iter().any(|h| h.path.ends_with("旧合同.doc")),
            "旧格式正文应进 FTS 并被原词命中：{:?}", hits.iter().map(|h| &h.path).collect::<Vec<_>>());

        // 预览可开：走同一个 extract_text，于是现转一次。断长度，不回显正文（红线④）。
        let doc_id: String = conn
            .query_row("SELECT id FROM index_docs WHERE path LIKE '%旧合同.doc'", [], |r| r.get(0))
            .unwrap();
        let row = crate::doc_preview::row_for_preview(&conn, &doc_id).unwrap();
        let pv = crate::doc_preview::render_preview(&doc_id, &row, "合同验收", crate::index_scan::DEFAULT_MAX_FILE_BYTES)
            .unwrap();
        assert!(pv.text.contains("合同验收"), "旧格式预览应能现转并含已知词（长度 {}）", pv.text.chars().count());

        // 增量不重转：size+mtime 未变 ⇒ 第二趟计 unchanged，一个字节都不打开（D1 的成本承诺兑现处）。
        let second = run_pass(&conn, &opts(), Some("p1"), false, &no_cancel(), &mut |_| {}).unwrap();
        let r2 = &second.results[0];
        assert_eq!(r2.ok, 0, "没重读就不该再计 ok：{r2:?}");
        assert!(r2.unchanged >= 1, ".doc 应计入 unchanged（不重起 LibreOffice）：{r2:?}");

        // 红线①：源夹具与 temp 副本逐字未动。
        let src_after = std::fs::metadata(&fixture).unwrap();
        assert_eq!((src_after.len(), src_after.modified().ok()), (src_before.len(), src_before.modified().ok()),
            "源夹具一字未动");
        assert_eq!(std::fs::metadata(&doc).unwrap().len(), doc_before.len(), "被索引副本一字未动");
    }

    /// `.ppt` 走的是同一套代码，只有目标扩展名与 `office_text` 的 `slides` 分支不同。
    /// 单独测一条是因为那条分支的读取入口是 `ppt/slides/slideN.xml`（spec §三.12），
    /// 与 docx 的 `word/document.xml` 是两个部件 —— 少这一条，ppt 通路就只有注释在守。
    #[test]
    fn legacy_ppt_flows_end_to_end_through_index_and_search() {
        if !crate::legacy_office::soffice_available() {
            eprintln!("SKIP: 本机无 LibreOffice");
            return;
        }
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-cn.ppt");
        let dir = tempfile::tempdir().unwrap();
        let ppt = dir.path().join("旧片子.ppt");
        std::fs::copy(&fixture, &ppt).unwrap();
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        let summary = run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(summary.state, "done");
        let status: String = conn
            .query_row("SELECT index_status FROM index_docs WHERE path LIKE '%旧片子.ppt'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "ok", "含字的 .ppt 应经真转换入库 ok");
        let hits = crate::index_store::doc_hits(&conn, "付款条件", 20).unwrap();
        assert!(hits.iter().any(|h| h.path.ends_with("旧片子.ppt")),
            "ppt 的第二段中文应可被搜到：{:?}", hits.iter().map(|h| &h.path).collect::<Vec<_>>());
    }
```

- [ ] **Step 2: 跑绿并记录耗时基数**

Run: `cd src-tauri && cargo test -j 1 --lib index_job::tests::legacy_ > /tmp/t7.log 2>&1; echo "exit=$?"; grep -E "test result|legacy_|finished" /tmp/t7.log`
Expected: 两条 PASS、`finished in N s`。把 N 如实报给控制方（本批**真起子进程**的测试，M6 的 e2e 是 0.4–0.5 s，本批预期 2–5 s/条）——这是 spec §十一.8 那条基数。

Run: `cd src-tauri && cargo test -j 1 --lib > /tmp/t7a.log 2>&1; echo "exit=$?"; grep "test result" /tmp/t7a.log | tail -1`
Expected: **191 passed**（189 + 2）。

- [ ] **Step 3: 变异取证（承重，不许只跑绿）**

① 把 `extract.rs` 的 `Some(DocKind::LegacyWord) => …legacy_text(path, false)` 那行删掉 ⇒ 编译失败（match 非穷尽），算红。② 把 `legacy_text` 里 `crate::extract::office_text(&produced, slides)` 的 `slides` 硬写成 `false` ⇒ `legacy_ppt_flows_end_to_end…` 红（pptx 里没有 `word/document.xml`，会报「读取失败」）。③ 把 `index_job.rs` worker 那句 `opts.soffice_available = …` 换成 `opts.soffice_available = false;` ⇒ 两条端到端都红（扫描门把旧格式挡掉、连行都没有）。逐条还原并复跑 191。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/index_job.rs
git commit -m "test(m7): 旧格式端到端 headless（真 LibreOffice 走 run_pass→FTS→doc_preview→第二趟不重转），167→191"
```

---

## Task 8: 真机断言 + 文档收口（控制方执行，子代理禁跑 dev）

**Files:** Modify `docs/技术方案.md`、`docs/开发进度.md`、`docs/HANDOFF.md`（回写清单见 spec §十二）

- [ ] **Step 1: 真机断言（沙盒 `dev.zero.pfm.m7test`）**

启动：`cd <repo> && WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" npm run tauri -- dev -c '{"identifier":"dev.zero.pfm.m7test"}'`（**先做两个必然失败的编译前探针**：`-c <不存在的文件>` 与 `-c '{"__probe_unknown_key__":1}'`，确认 `-c` 的 JSON 完整到达，再动手——M3 起的手法）。
按 spec §九 逐条取读数：
1. 主承诺已由 T7 的 headless 端到端出证；真机这一格补 **`index_overview` 的 `sofficeAvailable=true`** 往返。
2. `libreoffice_install` 的返回体形状（纯字符串 IPC，CDP 可直接 `invoke`）；`startedViaWinget=true` 会真弹 winget 窗口 ⇒ **那一下由需求方亲手看**，不自动化。
3. 30 份 `.doc` 首轮墙钟 + 单份均值 + 第二趟 `unchanged=30`（D1 成本账的真机读数，与 spec §三.2 的 1.2 s 基数对照）。
4. 降级那一支：管理员权限把 `C:\Program Files\LibreOffice\program\soffice.com` **临时改名** ⇒ 起一轮 ⇒ 旧格式一行都不建 ⇒ `/index` 念「未检测到 LibreOffice」并出现按钮 ⇒ **改回原名**并复跑确认建行恢复。改前改后各存一次该目录文件清单。做不动（被占用/无权限）就如实记 deferred。
5. 红线：`%APPDATA%\dev.zero.pfm\` 三文件 size+mtime+SHA-256 前后逐字对照（基线：`ledger.db 4096`、`-wal 399672`、`-shm 32768`，sha 前缀 `da63a52b`/`183a209b`/`cb642100`）；沙盒与 `%TEMP%\pfm-m7*` 删净；`tauri.conf.json` 一字未动；回收 dev 只按自己记的 PID，绝不 `taskkill //IM`。
6. GUI 渲染（那句提示 + 按钮）：本机 16 G 提交内存大概率仍起不来（HANDOFF 坑 2）⇒ 起不来就照实写 deferred，**不许假称通过**。

- [ ] **Step 2: 门禁终态 + 全部结构门现取**

`cargo test -j 1 --lib`（终态实跑数）、两 clippy 0 warning、`node --test`（仓库根）8/8、`npm run build` exit=0 且 `error TS`=0；结构门：`generate_handler!` **42**（本批唯一变动，正则有可选逗号）、`TARGET_VERSION` **4**、`SET index_status = 'missing'` 仍 1 行、`allow(dead_code)` 只剩 `db.rs:30`、`SUPPORTED` 条数 **29**、`soffice_available` 接线三处（字段 + 降级臂 + worker/overview 两处生产调用）+ `legacy_office::soffice_available()` 生产两处调用点 + **`grep -rn "归 M7" src-tauri/src src` 必须 exit=1**（那句「.doc/.ppt 归 M7」是活假话，M7 落地后必须清干净；注意别把 `\.doc\b`/`\.ppt\b` 也塞进这条门——夹具名与测试里的路径字符串本来就该出现 `.doc`）。

- [ ] **Step 3: 回写文档**

`docs/技术方案.md`：§6.1 line 166 那句改准（`.doc/.ppt` 自 M7 经本机 LibreOffice 纳入、未装时整类不建行、`/index` 一行状态 + 一键安装）、27→29 项、line 215 里程碑表 M7 行、line 54 库行为表补实测的 LibreOffice 论断、§九 加默认值一条。
`docs/开发进度.md`：里程碑表 M7 行 + 新增「M7 验收证据」整节（门禁读数、§九 逐条断言、**没能自动验证清单逐条点名**、缺口归属）+ 变更日志一条。
`docs/HANDOFF.md`：§0/§4 下一格转 M8；§5 新增本轮踩到的坑（MSYS 路径与 `--outdir=` 等号形、畸形 `-env` 是慢失败、`soffice.exe --version` 零输出、含拉丁字母的哨兵串会被 run 切分、调试外部 CLI 要先单发一条看真实报错）；§6 指针表加 M7 的 spec/计划/账本/探针四行。

- [ ] **Step 4: 提交 + 推功能分支**

```bash
git add docs/技术方案.md docs/开发进度.md docs/HANDOFF.md
git commit -m "docs(m7): 真机读数 + 文档回写收口（技术方案 27→29 项与 M7 行、开发进度证据节、HANDOFF 下一格转 M8）"
git push origin feat/m7-legacy-office
```

**合 `master` 仍需需求方当次批准**（惯例：`git rev-list --merges --count --all` = 0 且 master 是严格祖先 ⇒ fast-forward，不造合并提交）。

---

## 计划自检（writing-plans，控制方 2026-10-13 已跑）

1. **Spec 覆盖**：D1→T2/T7（转换在 `extract_text` 分派、写锁外，e2e 证不压锁）；D2→T2（`Scratch` Drop 即删、零新表）；D3→T3（只 `doc`/`ppt`）；D4→T4（无开关）；D5→T4（worker 每轮现探、不记忆化）；D6→T4（单一落点 + 第二落点 grep 门）；D7→T6（一个线字段）；D8→T5 + T6（命令 + 按钮 + `openUrl` 退路）；D9→T2/T4（`failed`/`skipped(empty_text)`/不建行三态，复用既有 arm）；D10→T1（argv/路径/超时四件）；判据 §五 八条→T1 纯核测试 + T3 三处同源 + T4 单一落点 grep + T7 承重变异；§八 三档→T1（恒绿 12 条）/T2（条件 3 条）/T7（端到端 2 条）；§九 真机→T8；§十二 回写→T8 Step 3；§十一 风险 3（垃圾文件被当正文）在 T2 Step 2 的注释里点名不许写成「垃圾必错」，风险 4（run 切分）落在夹具用纯中文哨兵 + T2 断言形状。
2. **占位符**：无 TBD/TODO。唯一「留到下一任务」的是 `start_install`（T1 只建 `InstallOutcome` 与 `winget_*`，`start_install` 在 T5 落地），并用**显式的 `#[allow(dead_code)]` + 「T5 删除豁免」注释**处理，且 T5 Step 4 有一条结构门强制它被删掉——与 M6 处理 T1/T2 豁免的同款收口。
3. **类型一致**：`soffice_candidates(Option<&OsStr>,Option<&OsStr>,Option<&OsStr>)->Vec<PathBuf>`、`soffice_path()->Option<PathBuf>`、`soffice_available()->bool`、`win_slash(&Path)->String`、`profile_url(&Path)->String`、`convert_args(&str,&str,&str,&str)->Vec<String>`、`product_path(&Path,&Path,&str)->PathBuf`、`Scratch::new(&str)->AppResult<Scratch>`、`run_bounded(&Path,&[String],Duration)->AppResult<ExitStatus>`、`legacy_text(&Path,bool)->AppResult<String>`、`winget_args()->Vec<String>`、`InstallOutcome{started_via_winget:bool}`（出 `startedViaWinget`）、`ScanOptions.soffice_available: bool`、`IndexOverview.soffice_available`（出 `sofficeAvailable`）跨任务一致；`extract_text` 两臂传的 `slides` 布尔与 `office_text(path, slides)` 的形参名同源。
4. **测试期望数（控制方按本计划逐条数出，非累加印象）**：T1 +12 ⇒ 179、T2 +3 ⇒ 182、T3 +1 ⇒ 183、T4 +3 ⇒ 186、T5 +3 ⇒ 189、T6 +0 ⇒ 189、T7 +2 ⇒ **191**；`node --test` 全程 8/8（零新前端测试）。派发每格时实现者须实跑回报，与本表不符即停。
