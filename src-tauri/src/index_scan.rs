//! 扫盘：根目录 → 待索引清单。这一层只读文件系统的元数据，绝不打开文件内容，
//! 更不写任何东西 —— 「只登记路径、不动源文件」的红线在这一层最容易被踩穿。

use std::path::Path;

use rusqlite::Connection;

use crate::error::AppResult;
use crate::extract::kind_of;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub exclude_dirs: Vec<String>,
    pub max_file_bytes: u64,
    pub max_files_per_project: i64,
}

impl ScanOptions {
    /// 「设置行不存在」退回 spec 默认值；「查询失败」是另一回事，必须原样抛出去。
    /// 两者都用 `unwrap_or_else` 吞掉的写法见过一次，坏库时会静默按 20 MB 上限跑完整轮。
    #[allow(dead_code)] // caller 在 Task 9 的 index_job，落地时删掉本行
    pub fn load(conn: &Connection) -> AppResult<ScanOptions> {
        use rusqlite::OptionalExtension;
        let get = |key: &str, default: &str| -> AppResult<String> {
            let row: Option<String> = conn
                .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
                .optional()?;
            Ok(row.unwrap_or_else(|| default.to_owned()))
        };
        // 数字解析失败仍退默认值：M3 没有写这三行的入口（Task 11 只读展示），
        // 要把它做成 Err 得连设置写入口与校验一起动，不在本任务范围。
        let list = get("index_exclude_dirs", "node_modules,dist,build,target,__pycache__,.git")?;
        let bytes = get("index_max_file_bytes", "20971520")?;
        let cap = get("index_max_files_per_project", "50000")?;
        Ok(ScanOptions {
            exclude_dirs: list
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect(),
            max_file_bytes: bytes.parse().unwrap_or(20_971_520),
            max_files_per_project: cap.parse().unwrap_or(50_000),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ScannedFile {
    // 这三条要等 Task 7 的 write_doc 才读（file.path / index_text(&file.file_name) / file.mtime）。
    // 用字段级而不是 struct 级豁免：ext 与 size 本模块真的在读，struct 级会把它们一起罩住。
    #[allow(dead_code)] // reader 在 Task 7 的 write_doc，落地时删掉本行
    pub path: String,
    #[allow(dead_code)] // reader 在 Task 7 的 write_doc，落地时删掉本行
    pub file_name: String,
    pub ext: String,
    pub size: u64,
    #[allow(dead_code)] // reader 在 Task 7 的 write_doc，落地时删掉本行
    pub mtime: i64,
}

#[derive(Debug, Default)]
pub struct ScanOutcome {
    pub files: Vec<ScannedFile>,
    pub over_size: Vec<ScannedFile>,
    pub walk_errors: Vec<String>,
    pub capped: bool,
}

fn to_scanned(entry: &walkdir::DirEntry) -> Option<ScannedFile> {
    let meta = entry.metadata().ok()?;
    Some(ScannedFile {
        path: entry.path().display().to_string(),
        file_name: entry.file_name().to_string_lossy().into_owned(),
        ext: entry
            .path()
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default(),
        size: meta.len(),
        mtime: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    })
}

#[allow(dead_code)] // caller 在 Task 9 的 index_job，落地时删掉本行
pub fn scan_root(root: &Path, opts: &ScanOptions) -> ScanOutcome {
    let mut out = ScanOutcome::default();
    let mut it = walkdir::WalkDir::new(root)
        .follow_links(false) // 链接/软链可能把目录图成环，且指向项目外的内容不该算进本项目
        .max_depth(16)
        .into_iter();

    while let Some(entry) = it.next() {
        match entry {
            Err(e) => {
                // 权限/重解析点失败在 Windows 上是日常，收集起来继续走，一轮不能因此中断。
                out.walk_errors.push(e.to_string());
                continue;
            }
            Ok(entry) => {
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type().is_dir()
                    && opts.exclude_dirs.iter().any(|d| d.eq_ignore_ascii_case(&name))
                {
                    it.skip_current_dir();
                    continue;
                }
                if !entry.file_type().is_file() {
                    continue;
                }
                let Some(scanned) = to_scanned(&entry) else {
                    continue;
                };
                if kind_of(&scanned.ext).is_none() {
                    continue; // 未支持类型不建行，见测试里的说明
                }
                if scanned.size > opts.max_file_bytes {
                    out.over_size.push(scanned);
                    continue;
                }
                if (out.files.len() as i64) + (out.over_size.len() as i64) >= opts.max_files_per_project {
                    out.capped = true;
                    break; // 触顶即停：剩下的文件不进清单，也没有行，由作业摘要点名项目
                }
                out.files.push(scanned);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const MAX: u64 = 20 * 1024 * 1024;

    fn opts(exclude: &[&str]) -> ScanOptions {
        ScanOptions {
            exclude_dirs: exclude.iter().map(|s| s.to_string()).collect(),
            max_file_bytes: MAX,
            max_files_per_project: 50000,
        }
    }

    fn touch(dir: &Path, rel: &str, bytes: &[u8]) {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, bytes).unwrap();
    }

    fn tree(dir: &Path) {
        touch(dir, "合同/验收说明.docx", b"x");
        touch(dir, "合同/node_modules/dep.js", b"y");          // 排除目录内部
        touch(dir, "交付/Node_Modules/index.js", b"z");        // 大小写不同，也要排除
        touch(dir, "dist/bundle.js", b"w");                     // 排除目录本身
        touch(dir, "readme.txt", b"hello");
        touch(dir, "图片/logo.png", b"\x89PNG\r\n\x1a\n");      // 支持清单外，不建行
        touch(dir, "报价.xlsx.zip", b"PK");                     // 压缩包，不建行
    }

    /// 排除目录要在「进入之前」剪掉：事实 20 的 skip_current_dir。
    #[test]
    fn excluded_dirs_are_pruned_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        tree(dir.path());
        let out = scan_root(dir.path(), &opts(&["node_modules", "dist"]));
        let paths: Vec<&str> = out.files.iter().map(|f| f.file_name.as_str()).collect();
        assert!(paths.contains(&"readme.txt") && paths.contains(&"验收说明.docx"), "{paths:?}");
        assert!(!paths.iter().any(|p| *p == "dep.js" || *p == "index.js" || *p == "bundle.js"),
            "排除目录里的文件不该出现：{paths:?}");
    }

    /// 未支持类型完全不建行（几十 GB 里图片/二进制占大头，全建行会把表撑爆）。
    /// 「为什么这个文件搜不到」由 /index 页面上的支持清单回答，见 Task 11。
    #[test]
    fn unsupported_extensions_produce_no_rows() {
        let dir = tempfile::tempdir().unwrap();
        tree(dir.path());
        let out = scan_root(dir.path(), &opts(&[]));
        assert!(out.files.iter().all(|f| f.ext != "png" && f.ext != "zip"), "{:?}",
            out.files.iter().map(|f| &f.ext).collect::<Vec<_>>());
    }

    #[test]
    fn oversize_supported_files_are_separated_not_dropped() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "大文件.txt", &[b'a'; 100]);
        touch(dir.path(), "小文件.txt", b"ok");
        let mut o = opts(&[]);
        o.max_file_bytes = 50;
        let out = scan_root(dir.path(), &o);
        assert_eq!(out.files.iter().map(|f| f.file_name.as_str()).collect::<Vec<_>>(), vec!["小文件.txt"]);
        assert_eq!(out.over_size.iter().map(|f| f.file_name.as_str()).collect::<Vec<_>>(), vec!["大文件.txt"],
            "超限文件要单独回传，好让它落成 skipped + too_large 而不是无声消失");
    }

    #[test]
    fn per_project_cap_stops_the_walk_and_flags_capped() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..25 {
            touch(dir.path(), &format!("批次/文件{i}.txt"), b"x");
        }
        let mut o = opts(&[]);
        o.max_files_per_project = 10;
        let out = scan_root(dir.path(), &o);
        assert_eq!(out.files.len(), 10, "触顶后不该继续收：{}", out.files.len());
        assert!(out.capped, "触顶必须显式标记，否则用户以为项目就这么点文件");
    }

    /// 根目录不存在（移动盘没挂载）是常态：回 walk_errors，不 panic、不返回 Err。
    #[test]
    fn missing_root_is_reported_instead_of_panicking() {
        let out = scan_root(Path::new("Z:/一定不存在/的根目录"), &opts(&[]));
        assert!(out.files.is_empty());
        assert!(!out.walk_errors.is_empty(), "不可达根目录要留下痕迹：{:?}", out.walk_errors);
    }

    /// 上限要真的能从 settings 改出来，这条同时钉住三件事：键名没写错、逗号的 trim 与空段处理、
    /// 数字解析。种子值和 spec 默认值字节相同，所以「读回种子值等于默认值」那种写法守不住键名笔误
    /// —— 键名写错了照样退默认值、照样绿，所以这里先把值改成与默认值不同再断言。
    #[test]
    fn load_reads_the_settings_rows_not_the_hardcoded_defaults() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("UPDATE settings SET value = '12345' WHERE key = 'index_max_file_bytes'", [])
            .unwrap();
        conn.execute("UPDATE settings SET value = '7' WHERE key = 'index_max_files_per_project'", [])
            .unwrap();
        conn.execute("UPDATE settings SET value = 'x, y,,cache' WHERE key = 'index_exclude_dirs'", [])
            .unwrap();
        let o = ScanOptions::load(&conn).unwrap();
        assert_eq!(o.max_file_bytes, 12345);
        assert_eq!(o.max_files_per_project, 7);
        assert_eq!(o.exclude_dirs, vec!["x".to_owned(), "y".to_owned(), "cache".to_owned()]);
    }

    /// 行缺失（老库没跑过 v4 的迁移、或用户手动删过）才退 spec 默认值。
    #[test]
    fn missing_setting_row_falls_back_to_spec_default() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("DELETE FROM settings WHERE key = 'index_max_file_bytes'", [])
            .unwrap();
        let o = ScanOptions::load(&conn).unwrap();
        assert_eq!(o.max_file_bytes, 20_971_520, "只有「无行」这一种情况该退默认值");
    }

    /// 与上一条配对：查询本身失败（库损坏、被锁、表不在）绝不能退默认值，必须把 Err 交回上层。
    /// 少这条，谁把 load 改回 unwrap_or_else(|_| default) 也不会有任何测试变红 —— 而表现是
    /// 「上限悄悄按 20 MB 跑完一整轮」，正是本仓库在 journal_mode 上拒绝过的静默降级。
    #[test]
    fn settings_query_failure_propagates_instead_of_defaulting() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("DROP TABLE settings", []).unwrap();
        let e = ScanOptions::load(&conn).unwrap_err();
        assert_eq!(e.code, "db_failed", "读库失败要沿用 error.rs 既有的库错误码：{}", e.message);
    }
}
