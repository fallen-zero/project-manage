//! 点开正文命中后的原文预览：查库拿路径 → 锁外重抽原文 → 在原文上定位 → 窗口化 → 码元区间。
//!
//! 两条红线在这一格里：
//! 1. **本模块不自己开文件读盘**，读盘只经 `extract::extract_text`（同一条扩展名分派表、
//!    同一套 chardetng + encoding_rs）。这样「索引与检索对磁盘的唯一动作是读」仍只需 grep
//!    `extract.rs` 一处（今天只有 `:44` 与 `:130`）。
//! 2. **抽取必须包在 panic 边界里**（`extract::panic_to_err`）。预览跑在 IPC 命令线程上，
//!    没有边界的话，一份畸形 docx 被用户在首屏点开就当场终止应用进程。

use std::path::{Path, PathBuf};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::extract::{panic_to_err, extract_text};
use crate::tokenize::query_terms;

/// 一扇窗的**半宽**（按 char 计），所以**没被并过**的单扇窗最长
/// `2 * PREVIEW_WINDOW_CHARS + 命中自身长度` 个 char（两端各扩一个半宽，中间还得放下命中本身
/// —— Task 5 复审指出原句少了后半截）。链式合并会让窗更长：`MAX_PREVIEW_WINDOWS` 只封顶**窗数**、
/// 不封顶窗长，命中每隔不到两个半宽出现一次时会并成一扇，长文里这扇可以接近整篇，由 `truncated` 说实话。
/// （Task 5 复审 Minor-1：并窗判据从「命中起点」换成「新窗左沿」后合并带翻倍，旧句的「单窗最长」成了假话。）
/// 不要求落在词或行边界上：预览是给人核对原文的，切在字中间比多加一层对齐逻辑更好读。
const PREVIEW_WINDOW_CHARS: usize = 4_000;
/// 一次预览最多拼几扇窗；多出来的命中直接丢弃，由 `truncated` 说实话。
const MAX_PREVIEW_WINDOWS: usize = 3;
/// 窗与窗之间的分隔串，**留在** `text` 里（3 个字符、各 1 个码元）。
/// 它不参与命中匹配，所以 `ranges` 按构造永不跨窗——跨了前端就会把上窗尾部涂到下窗头上。
const PREVIEW_GAP: &str = "\n⋯\n";

/// `index_docs` 一行里预览要用的两列。
pub struct PreviewRow {
    pub path: String,
    pub project_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocPreview {
    pub doc_id: String,
    pub path: String,
    pub project_name: String,
    /// 窗口拼接后的原文，不是整篇。
    pub text: String,
    /// **UTF-16 码元**偏移（既不是字符偏移也不是字节偏移），后端算好。
    pub ranges: Vec<(usize, usize)>,
    /// `text` 不等于整篇原文：任一窗口切断了原文，或有命中没能进窗口。
    pub truncated: bool,
}

fn unavailable() -> AppError {
    AppError::new(
        "preview_unavailable",
        "这条索引记录不能预览",
        Some("记录可能来自已注销的目录，或索引重建前的旧结果；到 /index 重跑一轮索引"),
    )
}

/// 只在持锁时调用；返回后**释放锁**再抽盘。握着 `state.conn` 去读盘会把整个应用卡在一次慢 IO 上
/// （索引作业那条路之所以自开连接，就是同一个理由）。
/// 行不存在 / 项目已软删 / `index_status != 'ok'` 三种都归 `preview_unavailable`：
/// skipped 与 failed 的行本来就没有正文，不能假装能读（spec §五.1）。
pub fn row_for_preview(conn: &Connection, doc_id: &str) -> AppResult<PreviewRow> {
    let row = conn
        .query_row(
            "SELECT d.path, p.name
               FROM index_docs d
               JOIN projects p ON p.id = d.project_id AND p.deleted_at IS NULL
              WHERE d.id = ?1 AND d.index_status = 'ok'",
            params![doc_id],
            |r| Ok(PreviewRow { path: r.get(0)?, project_name: r.get(1)? }),
        )
        .optional()?;
    row.ok_or_else(unavailable)
}

/// 抽取层的 io 码（`AppError::io` 的 `fs_failed`）在预览这里翻译成 `preview_file_missing`：
/// 文件改名/删除是预览独有的失效形态，不该让前端去认识 `extract` 内部的 io 码名（spec §六）。
/// **其余码原样透出**（`extract_unsupported`、`extract_failed`、`db_failed`）。
fn map_io_error(e: AppError) -> AppError {
    if e.code == "fs_failed" {
        AppError::new(
            "preview_file_missing",
            "文件已不可达",
            Some("文件可能已改名或被移走；真正的失效标记要到 M5 的核对轮才会写回记录"),
        )
    } else {
        e
    }
}

/// 生产入口：抽取器固定为 `extract::extract_text`。
pub fn render_preview(doc_id: &str, row: &PreviewRow, query: &str) -> AppResult<DocPreview> {
    render_preview_with(doc_id, row, query, extract_text)
}

/// 收抽取器参数不是测试专用注入缝：`extract_text` 会在畸形 docx/pdf 上 panic，
/// 而 panic 边界只能在这一层包，所以「边界在不在」必须能被一条 `|_| panic!()` 的用例打红（M3 的 I2）。
fn render_preview_with(
    doc_id: &str,
    row: &PreviewRow,
    query: &str,
    extract_with: impl Fn(&Path) -> AppResult<String>,
) -> AppResult<DocPreview> {
    let path = PathBuf::from(&row.path);
    let raw = panic_to_err(move || extract_with(&path), &row.path)
        .and_then(|r| r)
        .map_err(map_io_error)?;
    let chars: Vec<char> = raw.chars().collect();
    let hits = hit_ranges(&raw, query);
    let (windows, dropped) = take_windows(&hits, chars.len());
    // 无命中（纯文件名命中的那一类，§五.6）也要给东西看：取开头一扇全长窗。
    let shown = if windows.is_empty() {
        vec![(0usize, chars.len().min(2 * PREVIEW_WINDOW_CHARS))]
    } else {
        windows
    };
    let whole = shown.len() == 1 && shown[0] == (0, chars.len());
    let (text, ranges) = stitch(&chars, &shown, &hits);
    Ok(DocPreview {
        doc_id: doc_id.to_owned(),
        path: row.path.clone(),
        project_name: row.project_name.clone(),
        text,
        ranges,
        truncated: dropped || !whole,
    })
}

/// 逐 char 取小写**首字符**：char 数严格不变，这是「下标能对上原文」的前提。
/// ß→ss 这类展开成两字符的映射按首字符比，代价是那种词少认一次命中；
/// 换来的是偏移永不因折叠而漂——漂了前端就涂错色，比少涂一格严重（spec §五.4）。
fn lower_first(src: &str) -> String {
    src.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect()
}

/// 原文上的 **char** 区间（半开），升序且已合并重叠。
/// 词只从 `tokenize::query_terms` 拿：检索命中的词与预览高亮的词必须同一套切词，
/// 这里另起一次 `cut` 就会随词典版本漂成两处口径（spec §五.4）。
fn hit_ranges(text: &str, query: &str) -> Vec<(usize, usize)> {
    let hay: Vec<char> = lower_first(text).chars().collect();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for term in query_terms(query) {
        let needle: Vec<char> = lower_first(&term).chars().collect();
        if needle.is_empty() || needle.len() > hay.len() {
            continue;
        }
        let mut from = 0usize;
        while from + needle.len() <= hay.len() {
            let Some(off) = hay[from..].windows(needle.len()).position(|w| w == needle.as_slice()) else {
                break;
            };
            let at = from + off;
            out.push((at, at + needle.len()));
            from = at + needle.len(); // 同词的不重叠出现：跳过整段继续找
        }
    }
    out.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in out {
        match merged.last_mut() {
            // `<` 不是笔误：这里比的是**命中**，两段首尾相接（`s == prev.1`）在原文上是两格，
            // 前端要各涂一格，所以不并。真正必须分开的是「合同」「报价」这种相邻不重叠的命中
            // （`multi_word_query_highlights_both_terms` 断 `[(0,2),(2,4)]`）；写 `<=` 会把它们
            // 并成一格，与本任务自己的用例冲突。Task 5 首轮评审裁定：断言是规格，实现文本写错了。
            Some(prev) if s < prev.1 => prev.1 = prev.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

/// 以命中为中心扩半宽，**左沿相接或越过上一扇右沿**的窗并成一扇；超出 `MAX_PREVIEW_WINDOWS` 的命中丢弃并置 `truncated`。
/// 只与**最后一扇**比就够：命中升序，窗也升序。
fn take_windows(hits: &[(usize, usize)], text_len: usize) -> (Vec<(usize, usize)>, bool) {
    let mut windows: Vec<(usize, usize)> = Vec::new();
    let mut dropped = false;
    for &(start, end) in hits {
        let hi = (end + PREVIEW_WINDOW_CHARS).min(text_len);
        // 比的是新窗的**左沿** `lo`，不是命中起点 `start`：窗是 `[start - 半宽, end + 半宽]`，
        // 拿 `start` 去比，两处命中相距落在「一个半宽到两个半宽」这条带里时既不并窗、`lo` 又越过
        // 上一扇的右沿 —— `stitch` 会把中间那段正文**吐两遍**，还在它中间插一个代表「此处有省略」的
        // `PREVIEW_GAP`，而那段正文根本没被省略（Task 5 首轮评审 Important-1）。
        let lo = start.saturating_sub(PREVIEW_WINDOW_CHARS);
        // 这里刻意不用 `match windows.last_mut()` 里再写 `windows.len()` / `windows.push()`：
        // `last_mut()` 的可变借用横跨所有分支，那两个调用会撞 E0502（计划原文就是这个形状，
        // Task 5 实现者实测编不过）。借用在 `merged` 出作用域前结束，所以先算出布尔再分支。
        let merged = match windows.last_mut() {
            Some(prev) if lo <= prev.1 => {
                prev.1 = prev.1.max(hi);
                true
            }
            _ => false,
        };
        if merged {
            continue;
        }
        if windows.len() >= MAX_PREVIEW_WINDOWS {
            dropped = true;
        } else {
            windows.push((lo, hi));
        }
    }
    (windows, dropped)
}

fn utf16_len(chars: &[char]) -> usize {
    chars.iter().map(|c| c.len_utf16()).sum()
}

/// 按窗口切原文、中间插 `PREVIEW_GAP`，同时把命中从「原文 char 坐标」换算到
/// 「拼出来的 text 的码元坐标」。换算只做一次，就在写入每一扇窗的时候。
fn stitch(chars: &[char], windows: &[(usize, usize)], hits: &[(usize, usize)]) -> (String, Vec<(usize, usize)>) {
    let mut out = String::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut base = 0usize; // 已写入 out 的码元数
    for (i, &(ws, we)) in windows.iter().enumerate() {
        if i > 0 {
            out.push_str(PREVIEW_GAP);
            // 码元数现算而不是写死 3：将来换分隔串不会悄悄错。形态用 `encode_utf16().count()`，
            // 省掉每扇窗一次 `collect::<Vec<char>>()`。
            base += PREVIEW_GAP.encode_utf16().count();
        }
        out.extend(chars[ws..we].iter());
        for &(hs, he) in hits {
            if hs >= ws && he <= we {
                let start = base + utf16_len(&chars[ws..hs]);
                ranges.push((start, start + utf16_len(&chars[hs..he])));
            }
        }
        base += utf16_len(&chars[ws..we]);
    }
    (out, ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, index_scan::ScannedFile, index_store, index_store::DocOutcome};
    use rusqlite::params;

    /// 只回固定正文的「抽取器」：偏移与窗口算术的用都不该需要磁盘。
    fn body(text: String) -> impl Fn(&Path) -> AppResult<String> {
        move |_| Ok(text.clone())
    }

    fn row_for(path: &str) -> PreviewRow {
        PreviewRow { path: path.to_owned(), project_name: "政务云迁移".to_owned() }
    }

    /// 按拼出来的 `text` 取一段码元切片：前端用的就是 `String.prototype.slice`，
    /// 测试必须用同一把尺子，否则「码元契约」只是注释。
    fn utf16_slice(text: &str, r: &(usize, usize)) -> String {
        text.encode_utf16()
            .skip(r.0)
            .take(r.1 - r.0)
            .flat_map(|u| char::decode_utf16([u]).map(|c| c.unwrap_or('\u{FFFD}')))
            .collect()
    }

    fn conn() -> Connection {
        let c = db::open_in_memory().unwrap();
        c.execute("INSERT INTO projects (id, name, status) VALUES ('p1', '政务云迁移', '立项')", [])
            .unwrap();
        c
    }

    /// 造一行 `index_docs` 并回它的 `id`（`write_doc` 回的是 doc_rowid，预览按 id 查）。
    fn seed_row(conn: &Connection, path: &str, outcome: DocOutcome) -> String {
        let scanned = ScannedFile {
            path: path.to_owned(),
            file_name: path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned(),
            ext: path.rsplit('.').next().unwrap_or("").to_owned(),
            size: 1024,
            mtime: 1_700_000_000,
        };
        let rowid = index_store::write_doc(conn, "p1", &scanned, outcome).unwrap();
        conn.query_row("SELECT id FROM index_docs WHERE doc_rowid = ?1", params![rowid], |r| r.get(0))
            .unwrap()
    }

    /// 与 Task 6 命令体同一条链：锁内查库、锁外抽盘。
    fn preview(conn: &Connection, doc_id: &str, q: &str) -> AppResult<DocPreview> {
        render_preview(doc_id, &row_for_preview(conn, doc_id)?, q)
    }

    #[test]
    fn preview_highlights_every_hit_in_the_stitched_text() {
        let p = render_preview_with("d1", &row_for("C:/x/合同.docx"), "验收", body("甲方要求验收指标见合同附件".to_owned())).unwrap();
        assert_eq!(p.ranges.len(), 1);
        assert_eq!(utf16_slice(&p.text, &p.ranges[0]), "验收");
        assert_eq!(p.doc_id, "d1");
        assert!(!p.truncated, "正文短到一扇窗装得下时不该说自己是截断的");
    }

    /// 钉「不是字节偏移」：中文一处就翻 3 倍，前端 slice 会整段错位。
    #[test]
    fn ranges_are_utf16_units_not_byte_offsets() {
        let p = render_preview_with("d1", &row_for("C:/x/a.docx"), "验收", body("甲方验收".to_owned())).unwrap();
        assert_eq!(p.ranges, vec![(2, 4)], "字节偏移会是 (4, 8)");
    }

    /// 钉「不是 char 偏移」：命中前有一个 BMP 外字符，码元比 char 多 1。
    /// 这条是前端 `text.slice` 能对齐的唯一证据。
    #[test]
    fn a_bmp_outside_char_before_the_hit_costs_two_units() {
        let p = render_preview_with("d1", &row_for("C:/x/a.docx"), "验收", body("🎉甲方验收".to_owned())).unwrap();
        assert_eq!(p.ranges, vec![(4, 6)], "char 偏移会是 (3, 5)");
        assert_eq!(utf16_slice(&p.text, &p.ranges[0]), "验收");
    }

    #[test]
    fn matching_is_case_insensitive_and_keeps_the_original_spelling() {
        let p = render_preview_with(
            "d1",
            &row_for("C:/x/a.docx"),
            "payment",
            body("Payment terms and payment notes and PAYMENT due".to_owned()),
        )
        .unwrap();
        let got: Vec<String> = p.ranges.iter().map(|r| utf16_slice(&p.text, r)).collect();
        assert_eq!(got, ["Payment", "payment", "PAYMENT"], "高亮要涂原文写法，不是小写串");
    }

    /// §五.4 的「检索命中的词与预览高亮的词同一套切词」在这一条上可观测：
    /// 用户敲「验收，」，FTS 侧丢掉「，」命中正文，预览也必须照样涂出来。
    #[test]
    fn punctuation_in_the_query_is_dropped_by_the_shared_tokenizer() {
        let p = render_preview_with("d1", &row_for("C:/x/a.docx"), "验收，", body("甲方要求验收指标".to_owned())).unwrap();
        assert_eq!(p.ranges.len(), 1, "针若取 query 原文（含「，」）就会 0 命中");
    }

    #[test]
    fn multi_word_query_highlights_both_terms() {
        let p = render_preview_with("d1", &row_for("C:/x/a.docx"), "合同 报价", body("合同报价单里写了验收".to_owned())).unwrap();
        assert_eq!(p.ranges, vec![(0, 2), (2, 4)], "相邻但不重叠的两处保持分开，前端各涂一格");
    }

    #[test]
    fn near_hits_share_one_window_so_no_gap_is_emitted() {
        // 夹具是按半宽算出来的，不是随手一个短串：HALF = `PREVIEW_WINDOW_CHARS` = 4000，
        // 第一处命中 (0,2) 的窗是 `[0, 4002]`；要让第二处命中的**窗左沿**恰等于 4002（相接），
        // 它的 `start` 就得落在 `4002 + HALF = 8002`。
        // 于是并窗判据从 `<=` 翻成 `<` 时两扇窗不再相接 → `stitch` 多插一个 `PREVIEW_GAP` → 本条红；
        // 而 `<=` 下是一扇覆盖全文的窗 → `p.text` 就是原文、不含分隔串。
        // （原夹具 `"甲验收乙"` 只有一处命中，并窗分支根本不执行，Step 4 变异 3 因此在给定夹具下
        // 逻辑不可满足 —— Task 5 首轮评审 Important-2，名不符实的假守护。）
        let text = format!("验收{}验收", "字".repeat(8_000));
        let p = render_preview_with("d1", &row_for("C:/x/a.docx"), "验收", body(text.clone())).unwrap();
        assert_eq!(text.chars().count(), 8_004, "间距算错了这条就退化成单命中夹具");
        assert_eq!(p.ranges, vec![(0, 2), (8_002, 8_004)], "两处命中进同一扇窗，各自涂一格");
        assert!(!p.truncated, "相接的两处命中该并成一扇，窗数没超上限不该截断");
        assert!(!p.text.contains(PREVIEW_GAP), "两窗相接还分开拼就会多出一个假省略号");
        assert_eq!(p.text, text);
    }

    #[test]
    fn four_spaced_hits_give_three_windows_and_the_fourth_is_dropped() {
        let mut text = String::new();
        for _ in 0..4 {
            text.push_str("验收");
            text.push_str(&"字".repeat(10_000));
        }
        let p = render_preview_with("d1", &row_for("C:/x/a.docx"), "验收", body(text)).unwrap();
        assert_eq!(p.ranges.len(), MAX_PREVIEW_WINDOWS, "第 4 处超出窗数上限就不该出现");
        assert!(p.truncated);
        assert_eq!(p.text.match_indices(PREVIEW_GAP).count(), MAX_PREVIEW_WINDOWS - 1);
        for r in &p.ranges {
            assert!(!utf16_slice(&p.text, r).contains('⋯'), "range 跨过分隔串就会把上窗尾部涂进下窗");
        }
    }

    /// §五.6：纯文件名命中时原文里一处都匹配不上，`ranges` 为空是合法结果。
    #[test]
    fn a_file_name_only_match_yields_no_ranges_but_still_shows_a_head_window() {
        let text = "数".repeat(9_000);
        let p = render_preview_with("d1", &row_for("C:/x/报价单.docx"), "报价单", body(text.clone())).unwrap();
        assert!(p.ranges.is_empty());
        assert_eq!(p.text.chars().count(), 2 * PREVIEW_WINDOW_CHARS, "无命中时给开头一扇全长窗");
        assert!(p.truncated);
    }

    #[test]
    fn a_missing_row_is_preview_unavailable() {
        let c = conn();
        let e = preview(&c, "no-such-doc", "验收").unwrap_err();
        assert_eq!(e.code, "preview_unavailable");
    }

    #[test]
    fn a_skipped_row_is_preview_unavailable() {
        let c = conn();
        let id = seed_row(&c, "C:/x/大图.png", DocOutcome::Skipped("超出大小上限"));
        let e = preview(&c, &id, "验收").unwrap_err();
        assert_eq!(e.code, "preview_unavailable", "skipped 行没有正文可抽，不能假装能读");
    }

    #[test]
    fn a_soft_deleted_projects_row_is_preview_unavailable() {
        let c = conn();
        let id = seed_row(&c, "C:/x/合同.docx", DocOutcome::Ok("甲方要求验收指标".into()));
        c.execute("UPDATE projects SET deleted_at = datetime('now') WHERE id = 'p1'", []).unwrap();
        assert_eq!(preview(&c, &id, "验收").unwrap_err().code, "preview_unavailable");
    }

    /// io 码要翻译，其余码不许改写（前端只认 `preview_*` 两个码）。
    #[test]
    fn io_errors_are_translated_and_other_codes_pass_through() {
        let missing = render_preview_with("d1", &row_for("C:/definitely/not/here.txt"), "验收", extract_text).unwrap_err();
        assert_eq!(missing.code, "preview_file_missing", "fs_failed 不能原样给前端");
        let png = render_preview_with("d1", &row_for("C:/definitely/not/here.png"), "验收", extract_text).unwrap_err();
        assert_eq!(png.code, "extract_unsupported", "非 io 码保持原样");
    }

    /// M3 的 I2 教训：中间层没有测试等于没守。这一条钉住 panic 边界真的接到了预览这条路上。
    #[test]
    fn a_panicking_extractor_is_caught_not_fatal() {
        let boom: fn(&Path) -> AppResult<String> = |_| panic!("模拟解析器在畸形文件上炸");
        let e = render_preview_with("d1", &row_for("C:/x/a.docx"), "验收", boom).unwrap_err();
        assert_eq!(e.code, "extract_failed");
    }
}
