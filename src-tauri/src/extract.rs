//! 按扩展名抽文本：一个路径进，一段中文纯文本出。
//!
//! 这一层不碰数据库、不判断状态，只回答「抽得出吗」。空字符串是有效结果（扫描件 PDF
//! 没有文字层、空文件都归到这里），由调用方标成 `skipped/empty_text`，不是失败。

use std::io::Read;
use std::path::Path;

use crate::error::{AppError, AppResult};

/// 错误码是前端要分支的稳定契约，所以两个码名不要改：
/// extract_unsupported（类型不支持）、extract_failed（抽取过程报错）
fn fail(code: &'static str, msg: &str, hint: &str) -> AppError {
    AppError::new(code, msg, Some(hint))
}

fn decode_text_bytes(bytes: &[u8]) -> String {
    // 严格 UTF-8 先试一次：绝大多数现代文本文件走这条。已知取舍：个别 GBK 双字节对恰好也是
    // 合法 UTF-8（实测 `C4 A3` / `C5 B4` / `C4 BC` 分别解成 `ģ Ŵ ļ`），整份都由这种对组成的
    // 短文件会静默走快路径、解成一片拉丁扩展字符。一整句中文的 GBK 流不在窗口内（实测在
    // `from_utf8` 处就报错），所以顺序保留、这里不改判据。
    // BOM 对 `from_utf8` 是合法字符（U+FEFF），会原样留在串首，所以要剥。
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.trim_start_matches('\u{FEFF}').to_owned();
    }
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    // 上面已经确认这不是合法 UTF-8，所以把 UTF-8 从猜测结果里排除，避免无谓的二选一。
    let encoding = detector.guess(None, chardetng::Utf8Detection::Deny);
    // 这里必须用 `decode`，不是 `decode_without_bom_handling`：`decode` 内部先跑
    // `Encoding::for_bom`（`encoding_rs-0.8.42/src/lib.rs:3039`），UTF-8 / UTF-16LE / UTF-16BE
    // 的 BOM 会**盖过上面的猜测结果**并把 BOM 一并剥掉。实测过：UTF-16LE 的字节 chardetng 猜成
    // windows-1252，而 `decode` 回报 `used = UTF-16LE`、输出与原文逐字相等（事实 17）。
    // UTF-16 文件（记事本「另存为 Unicode」、PowerShell 的 `>` 重定向都默认 UTF-16LE）就是靠这一条
    // 被正确解出的；换成 `decode_without_bom_handling` 实测得到 `ÿþŒš6e¥bJT…` 且 `had_errors`
    // 还是 false —— 正是「搜不到但不报错」那个最难排查的失效形态。
    // 这条路径上不用再 trim BOM：`decode` 已经把 BOM 截掉了，和上面 `from_utf8` 那条不同。
    let (decoded, _encoding_used, _had_errors) = encoding.decode(bytes);
    decoded.into_owned()
}

fn read_text_file(path: &Path) -> AppResult<String> {
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .and_then(|mut f| f.read_to_end(&mut buf))
        .map_err(|e| AppError::io(path, &e))?;
    Ok(decode_text_bytes(&buf))
}

/// 事实 12：docx 的正文节点是 `<w:t>`、pptx 是 `<a:t>`，`local_name` 都是 `t`。
/// 所以两种格式共用一个循环：**区分范围靠读哪个部件**（`word/document.xml` vs
/// `ppt/slides/slideN.xml`），传进来的 tag 两边都是 `b"t"`，别误读成它在筛命名空间。
/// `check_end_names = false` 是因为 Office 的命名前缀只在文档内部一致，
/// 关掉能避免严格校验把整份文件判死。
fn xml_texts(xml: &str, tag: &[u8]) -> AppResult<Vec<String>> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().check_end_names = false;
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut inside = false;
    loop {
        match reader
            .read_event()
            .map_err(|e| fail("extract_failed", &format!("XML 解析失败：{e}"), "文件可能已损坏或是加密的 Office 文档"))?
        {
            Event::Eof => {
                // 半截文件：quick-xml 把剩余内容当 UpToEof 发出来之后就回 Eof，**不报错**
                // （`IllFormedError::MissingEndTag` 只有 `read_to_end` 才会给）。这里不收口，
                // 最后一段正文就静默消失 —— 正是「不报错、只是搜不到」那一类。
                if inside {
                    close_run(&mut cur, &mut out, &mut inside);
                }
                break;
            }
            Event::Start(e) => {
                // `<w:t>` 在 OOXML 里不嵌套：run 没闭合就又开一个标签，说明上一个 run 到此为止。
                // 不关掉，紧随其后的 `<w:instrText>` 域代码就会被并进正文、进索引、进摘要。
                if inside {
                    close_run(&mut cur, &mut out, &mut inside);
                }
                inside = e.local_name().as_ref() == tag;
            }
            Event::Text(t) if inside => {
                let s = t
                    .xml10_content()
                    .map_err(|e| fail("extract_failed", &format!("XML 文本解码失败：{e}"), "文件可能已损坏"))?;
                cur.push_str(&s);
            }
            Event::GeneralRef(r) if inside => {
                // 载荷是裸片段（"apos" / "#39"），补回 & 和 ; 才能交给 unescape。
                let frag = String::from_utf8_lossy(r.as_ref()).into_owned();
                match quick_xml::escape::unescape(&format!("&{frag};")) {
                    Ok(s) => cur.push_str(&s),
                    // `unescape` 只认 5 个预定义实体和 `&#…;` 数字引用（它用
                    // `resolve_predefined_entity`），其它命名实体回 Err。这里保留 `&片段;` 原文
                    // 而不是丢弃：少一个字符是静默的，留原文至多让那一个词命中差一点。
                    Err(_) => {
                        cur.push('&');
                        cur.push_str(&frag);
                        cur.push(';');
                    }
                }
            }
            Event::End(_) if inside => {
                // 不匹配的闭标签同样收口，且不能要求它等于 tag：`check_end_names = false` 时
                // quick-xml 原样带回它自己的名字（`<w:t>abc</w:p>` 的 End 是 "p"），
                // 真去比名字，这类输入就永远收不了口。
                close_run(&mut cur, &mut out, &mut inside);
            }
            _ => {}
        }
    }
    Ok(out)
}

/// 攒到的正文只在 run 结束时进 `out`。三个收口时机（End、新的 Start、EOF）都走这一条，
/// 少一个就是一种静默失效。`<w:t>` 不嵌套，所以不需要深度计数。
fn close_run(cur: &mut String, out: &mut Vec<String>, inside: &mut bool) {
    if !cur.trim().is_empty() {
        out.push(std::mem::take(cur));
    }
    cur.clear();
    *inside = false;
}

/// `slides = false` 走 docx（word/document.xml + `<w:t>`），true 走 pptx（ppt/slides/slideN.xml + `<a:t>`）。
pub fn office_text(path: &Path, slides: bool) -> AppResult<String> {
    let file = std::fs::File::open(path).map_err(|e| AppError::io(path, &e))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| fail("extract_failed", &format!("解包失败：{e}"), "文件可能已损坏或受密码保护"))?;

    let parts: Vec<String> = if slides {
        let mut names: Vec<String> = zip
            .file_names()
            .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
            .map(|n| n.to_owned())
            .collect();
        // slide10 必须排在 slide2 之后：按文件名里的数字排，字符串序会把它排到前面。
        names.sort_by_key(|n| {
            let stem = n.trim_end_matches(".xml").trim_start_matches("ppt/slides/slide");
            stem.parse::<u32>().unwrap_or(u32::MAX)
        });
        names
    } else {
        vec!["word/document.xml".to_owned()]
    };

    let mut chunks: Vec<String> = Vec::new();
    for name in &parts {
        let mut xml = String::new();
        let read = zip
            .by_name(name)
            .map(|mut entry| entry.read_to_string(&mut xml));
        match read {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(AppError::io(path, &e)),
            Err(e) => {
                // 加密包或缺部件都在这里现形：记 failed 而不是整轮失败。
                return Err(fail(
                    "extract_failed",
                    &format!("读取 {name} 失败：{e}"),
                    "文件可能已损坏、受密码保护，或不是标准的 Office 文件",
                ));
            }
        }
        chunks.extend(xml_texts(&xml, b"t")?);
    }
    Ok(chunks.join("\n"))
}

/// xlsx/xls 走 calamine。`open_workbook_auto` 后必须 `use calamine::Reader as _`，
/// 且 `worksheet_range` 直接返回 `Result<Range, Error>`（不是双层 Result）。
pub fn sheet_text(path: &Path) -> AppResult<String> {
    use calamine::Reader as _;

    let mut book = calamine::open_workbook_auto(path)
        .map_err(|e| fail("extract_failed", &format!("打开表格失败：{e}"), "文件可能已损坏、受密码保护，或是伪装成表格的其它格式"))?;
    let mut text = String::new();
    for name in book.sheet_names() {
        let Ok(range) = book.worksheet_range(&name) else {
            continue; // 单个 sheet 读不出来不该让整份文件失败
        };
        for row in range.rows() {
            for cell in row {
                let s = cell.to_string();
                if !s.is_empty() {
                    text.push_str(&s);
                    text.push(' ');
                }
            }
        }
    }
    Ok(text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    Text,
    Word,
    Slides,
    Workbook,
    Pdf,
}

/// 纯文本分支的扩展名清单，`kind_of` 与下面的 `SUPPORTED` 都从它出发。
/// .doc/.ppt 属 M7（要 LibreOffice headless）、图片属 M6（OCR），本版本不登记、不建行。
const TEXT_EXTS: &[&str] = &[
    "txt", "md", "markdown", "csv", "json", "xml", "yml", "yaml", "ini", "conf", "sql", "java",
    "py", "js", "ts", "bat", "sh",
];

/// UI 上「支持的类型」就念这张表。它和 `kind_of` 是同一套信息的两种写法（一份给分派、
/// 一份给展示），改扩展名时两处一起改；一致性由 `supported_exts_list_matches_the_dispatch_table`
/// 钉住——清单里出现 `kind_of` 分派不到的扩展名会直接红。
const SUPPORTED: &[&str] = &[
    "txt", "md", "markdown", "csv", "json", "xml", "yml", "yaml", "ini", "conf", "sql", "java",
    "py", "js", "ts", "bat", "sh", "docx", "pptx", "xlsx", "xls", "pdf",
];

pub fn supported_exts() -> &'static [&'static str] {
    SUPPORTED
}

pub fn kind_of(ext: &str) -> Option<DocKind> {
    let e = ext.to_ascii_lowercase();
    if TEXT_EXTS.contains(&e.as_str()) {
        Some(DocKind::Text)
    } else {
        match e.as_str() {
            "docx" => Some(DocKind::Word),
            "pptx" => Some(DocKind::Slides),
            "xlsx" | "xls" => Some(DocKind::Workbook),
            "pdf" => Some(DocKind::Pdf),
            _ => None,
        }
    }
}

/// pdf-extract 内部依赖 lopdf，畸形结构有 panic 的前科；一轮索引不能因为一个坏文件整体失败。
/// 注意：本机实测的假 PDF 头与截断文件都走的是 Err 分支（事实 18），
/// 这里的 catch_unwind 防的是 panic 分支，那条分支在本机无法构造 —— 属已知缺口，不要当成已证。
fn pdf_text(path: &Path) -> AppResult<String> {
    let p = path.to_path_buf();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pdf_extract::extract_text(&p)));
    match r {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(e)) => Err(fail(
            "extract_failed",
            &format!("PDF 抽取失败：{e}"),
            "确认文件未损坏；扫描版 PDF 没有文字层需等 M6 的 OCR",
        )),
        Err(_) => Err(fail(
            "extract_failed",
            "PDF 解析器在畸形文件上 panic，已兜住并跳过该文件",
            "该文件本轮不索引；可换 PDF 工具另存一份再登记",
        )),
    }
}

/// 入口：只按扩展名分派，不做大小校验（上限归 Task 6 的扫描器），不做状态标记（归 Task 7）。
pub fn extract_text(path: &Path) -> AppResult<String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    match kind_of(&ext) {
        Some(DocKind::Text) => read_text_file(path),
        Some(DocKind::Word) => office_text(path, false),
        Some(DocKind::Slides) => office_text(path, true),
        Some(DocKind::Workbook) => sheet_text(path),
        Some(DocKind::Pdf) => pdf_text(path),
        None => Err(fail(
            "extract_unsupported",
            &format!("不支持的文件类型 .{ext}"),
            "M3 只索引文本/Office/PDF；图片属 M6 OCR，.doc/.ppt 属 M7",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // 两行都是测试专用，别提到模块顶层：`cargo clippy --lib` 不带 test cfg，顶层引入会被判 unused。
    use std::io::Write;
    use std::path::PathBuf;

    /// docx/pptx 本质是 zip + xml，测试里自己拼一个，省掉二进制 fixture。
    fn make_office(dir: &Path, name: &str, part: &str, xml: &str) -> PathBuf {
        let p = dir.join(name);
        let mut w = zip::write::ZipWriter::new(std::fs::File::create(&p).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("[Content_Types].xml", opts).unwrap();
        w.write_all(br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#)
            .unwrap();
        w.start_file(part, opts).unwrap();
        w.write_all(xml.as_bytes()).unwrap();
        w.finish().unwrap();
        p
    }

    /// 事实 10/11：quick-xml 0.41 把实体拆成独立的 GeneralRef 事件，且载荷不含 & 和 ;。
    /// 直接 xml10_content() 会把正文截断，read_text() 会把实体原样留在正文。
    /// 唯一正确的写法是 escape::unescape(&format!("&{frag};"))。
    #[test]
    fn docx_text_resolves_entities_instead_of_truncating() {
        let dir = tempfile::tempdir().unwrap();
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>合同&apos;验收</w:t></w:r><w:r><w:t xml:space="preserve"> 标准 &amp; 说明</w:t></w:r></w:p></w:body></w:document>"#;
        let p = make_office(dir.path(), "t.docx", "word/document.xml", xml);
        let text = office_text(&p, false).unwrap();
        assert!(text.contains("合同'验收"), "实体必须还原成字符：{text:?}");
        assert!(text.contains("标准 & 说明"), "{text:?}");
        assert!(!text.contains("&apos;") && !text.contains("apos"), "正文里不许残留实体片段：{text:?}");
    }

    /// `<w:instrText>` 是域代码（HYPERLINK 之类），不是正文；只认 `<w:t>` 就自动跳过。
    /// 名字里的两件事都得有断言撑着：Task 8 的摘要可读性依赖「每个 run 独立成段、段间用 `\n`
    /// 拼接」这个形状，而畸形输入上的「跳过域代码」只在状态机肯收口才成立。
    #[test]
    fn docx_skips_field_codes_and_keeps_paragraph_breaks() {
        let dir = tempfile::tempdir().unwrap();
        let xml = r#"<w:document xmlns:w="http://x"><w:body><w:p><w:r><w:t>正文一</w:t><w:br/><w:t>正文二</w:t></w:r></w:p><w:p><w:r><w:instrText>HYPERLINK</w:instrText><w:t>正文三</w:t></w:r></w:p></w:body></w:document>"#;
        let p = make_office(dir.path(), "f.docx", "word/document.xml", xml);
        let text = office_text(&p, false).unwrap();
        assert_eq!(text, "正文一\n正文二\n正文三", "run 边界就是换行，域代码那一串不许混进来");

        // 畸形一：`<w:t>` 没闭合就开新标签。没有 Start 收口，HYPERLINK 会被并进正文。
        let open_run = r#"<w:document xmlns:w="http://x"><w:body><w:p><w:r><w:t>半截正文<w:instrText>HYPERLINK</w:instrText></w:t></w:r></w:p></w:body></w:document>"#;
        let text = office_text(&make_office(dir.path(), "open.docx", "word/document.xml", open_run), false).unwrap();
        assert!(text.contains("半截正文"), "上一个 run 该在开新标签时收口，不能丢字：{text:?}");
        assert!(!text.contains("HYPERLINK"), "域代码不该进索引：{text:?}");

        // 畸形二：文件在半句正文上到底。quick-xml 不报错，只回 Eof；
        // 没有 EOF 收口，这半句就静默消失。
        let truncated = r#"<w:document xmlns:w="http://x"><w:body><w:p><w:r><w:t>尾巴正文"#;
        let text = office_text(&make_office(dir.path(), "trunc.docx", "word/document.xml", truncated), false).unwrap();
        assert!(text.contains("尾巴正文"), "截断处已攒到的正文必须收口，不能静默丢：{text:?}");
    }

    /// pptx 的正文在 ppt/slides/slideN.xml 里，逐个 slide 按序号读，文本节点是 <a:t>。
    #[test]
    fn pptx_reads_every_slide_in_numeric_order() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.pptx");
        let mut w = zip::write::ZipWriter::new(std::fs::File::create(&p).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("[Content_Types].xml", opts).unwrap();
        w.write_all(br#"<?xml version="1.0"?/>"#).unwrap();
        for (n, word) in [(2u32, "第二页"), (10, "第十页"), (1, "第一页")] {
            // clippy::needless_borrows_for_generic_args：`start_file` 的形参是 `S: ToString`，
            // `String` 本身就满足，写成 `&format!(…)` 会在 `cargo clippy --lib --all-targets`
            // 那道闸上被判红，这里按 clippy 的建议去掉借用。
            w.start_file(format!("ppt/slides/slide{n}.xml"), opts).unwrap();
            let xml = format!(
                r#"<p:sld xmlns:a="http://x" xmlns:p="http://y"><p:cSld><p:sp><p:txBody><a:p><a:r><a:t>{word}验收</a:t></a:r></a:p></p:txBody></p:sp></p:cSld></p:sld>"#
            );
            w.write_all(xml.as_bytes()).unwrap();
        }
        w.finish().unwrap();
        let text = office_text(&p, true).unwrap();
        let i1 = text.find("第一页").unwrap();
        let i2 = text.find("第二页").unwrap();
        let i10 = text.find("第十页").unwrap();
        assert!(i1 < i2 && i2 < i10, "slide10 不该排在 slide2 前：{text:?}");
    }

    #[test]
    fn xlsx_concatenates_every_sheet_and_cell() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.xlsx");
        {
            let mut wb = rust_xlsxwriter::Workbook::new();
            wb.add_worksheet().write(0, 0, "合同验收标准").unwrap();
            wb.add_worksheet().write(1, 2, "维保期").unwrap();
            wb.save(&p).unwrap();
        }
        let text = sheet_text(&p).unwrap();
        assert!(text.contains("合同验收标准"), "{text:?}");
        assert!(text.contains("维保期"), "第二个 sheet 也要读：{text:?}");
    }

    /// spec 明写：Windows 中文环境 GBK 必然出现。假设 UTF-8 不会报错，只会得到一片
    /// 乱码索引 —— 所以这条测试断言的是「解回原句」，不是「没报错」。
    #[test]
    fn gbk_bytes_are_detected_not_assumed_utf8() {
        let sentence = "响应时间不超过800毫秒，验收指标见合同。";
        let gbk = encoding_rs::GBK.encode(sentence).0;
        assert!(!std::str::from_utf8(&gbk).is_ok(), "前置条件：这段字节确实不是合法 UTF-8");
        assert_eq!(decode_text_bytes(&gbk), sentence);
    }

    #[test]
    fn utf8_and_ascii_pass_through_byte_for_byte() {
        let cn = "合同验收说明 v1.2";
        assert_eq!(decode_text_bytes(cn.as_bytes()), cn);
        assert_eq!(decode_text_bytes(b"plain ascii only"), "plain ascii only");
    }

    /// Windows 记事本「另存为 Unicode」和 PowerShell 的 `>` 重定向默认就产出 UTF-16LE，
    /// 这些文件在用户的目录里真实存在——拒收等于「永远搜不到」。
    /// 解法不是自己分派，而是别绕过 `decode` 的 BOM 嗅探（事实 17）：断言写成「解回原文」。
    /// 谁哪天把它换成 `decode_without_bom_handling`，那函数只回 2 元组、先撞编译错误；
    /// 把解构一起改掉的版本会撞这条断言，因为 windows-1252 解出的 mojibake 不等于原文。
    #[test]
    fn utf16_with_bom_is_decoded_not_guessed() {
        let cn = "验收报告";
        let mut le: Vec<u8> = vec![0xFF, 0xFE];
        for u in cn.encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        let mut be: Vec<u8> = vec![0xFE, 0xFF];
        for u in cn.encode_utf16() {
            be.extend_from_slice(&u.to_be_bytes());
        }
        for (label, bytes) in [("utf16le", &le), ("utf16be", &be)] {
            let got = decode_text_bytes(bytes);
            assert_eq!(got, cn, "{label} 要解回原文，不能是 mojibake，也不能留着 BOM 那个方块");
            assert!(!got.contains('\u{FEFF}'), "{label} 的 BOM 要剥掉");
        }
    }

    /// UTF-8 BOM 会被 from_utf8 原样留下 \u{FEFF}，它进索引串首不影响命中，但
    /// 出现在 UI 摘要里是个看不见的方块，所以这里要求剥掉。样本故意叠两层 BOM（字节级
    /// EF BB BF + 串首 U+FEFF），钉的是 `trim_start_matches` 剥**全部**前导 BOM 而不是一个；
    /// 现实文件只有一个，多剥这层不留风险，也不与探测路径的 `for_bom`（只剥一个）冲突——
    /// 带 UTF-8 BOM 的文件必然走 `from_utf8` 快路径，走不到探测分支。
    #[test]
    fn utf8_bom_is_stripped() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("﻿验收清单".as_bytes());
        let text = decode_text_bytes(&bytes);
        assert!(text.starts_with("验收清单"), "实际前缀：{:?}", &text[..text.len().min(8)]);
        assert!(!text.contains('\u{FEFF}'));
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// 真中文 PDF（Edge 打印）必须能抽出可搜的中文。这条同时也是 pdf-extract 的中文可用性证据。
    #[test]
    fn real_pdf_yields_searchable_chinese_text() {
        let text = extract_text(&fixture("sample-cn.pdf")).unwrap();
        assert!(text.chars().count() > 40, "抽出来的正文太短：{}", text.chars().count());
        assert!(text.contains("验收") && text.contains("维保"), "正文应含关键词：{text:?}");
    }

    /// 事实 18：pdf-extract 对畸形 PDF 返回 Err；catch_unwind 兜的是 lopdf 潜在 panic。
    /// 这里断言的是「返回 Err 且调用方不 panic」，不预设它走哪条分支。
    #[test]
    fn malformed_pdfs_fail_cleanly_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let garbage = dir.path().join("g.pdf");
        std::fs::write(&garbage, b"%PDF-1.4 not a real pdf body\n%%EOF\n").unwrap();
        let real = std::fs::read(fixture("sample-cn.pdf")).unwrap();
        let truncated = dir.path().join("t.pdf");
        std::fs::write(&truncated, &real[..real.len() / 3]).unwrap();

        for p in [garbage.as_path(), truncated.as_path()] {
            let r = extract_text(p);
            assert!(
                r.is_err(),
                "畸形 PDF 必须落成失败结果而不是崩掉：{:?}",
                r.map(|t| t.chars().count())
            );
        }
    }

    #[test]
    fn dispatch_is_case_insensitive_and_rejects_unknown_types() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("A.TXT"), "验收".as_bytes()).unwrap();
        assert!(extract_text(&dir.path().join("A.TXT")).unwrap().contains("验收"), "扩展名要忽略大小写");

        let e = extract_text(&dir.path().join("lib.dll")).unwrap_err();
        assert_eq!(e.code, "extract_unsupported");

        let png = dir.path().join("截图.PNG");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n").unwrap();
        let e = extract_text(&png).unwrap_err();
        assert_eq!(e.code, "extract_unsupported", "图片属 M6 OCR，本版本明确不支持");
    }

    /// 分派表和 supported_exts() 必须同源，而且**两个方向都要查**：
    /// `SUPPORTED` 里出现 `kind_of` 分派不到的扩展名 = UI 承诺了搜不到的类型；
    /// `TEXT_EXTS` 里出现 `SUPPORTED` 没有的扩展名 = 真能抽进索引、UI 却不列，用户根本想不到去搜。
    /// 只查前一个方向的话，后者可以一直绿着漂移，而这正是本测试存在的理由。
    #[test]
    fn supported_exts_list_matches_the_dispatch_table() {
        for ext in supported_exts() {
            assert!(kind_of(ext).is_some(), "{ext} 在清单里却分派不到抽取器");
            assert_eq!(ext.to_lowercase(), *ext, "清单里的扩展名统一小写");
        }
        // 反方向：分派表必须清单的子集。漏了这条，往 TEXT_EXTS 加一个 "log" 就是一次静默漂移。
        for e in TEXT_EXTS {
            assert!(supported_exts().contains(e), "{e} 能分派却不在 UI 清单里");
        }
        for kind_ext in ["txt", "md", "csv", "docx", "pptx", "xlsx", "xls", "pdf"] {
            assert!(supported_exts().contains(&kind_ext), "{kind_ext} 缺清单");
        }
    }

    /// 五个分派臂都要真的走一遍。这条存在的理由是一个具体失效：docx 如果被接成 `slides = true`，
    /// `office_text` 找不到 `ppt/slides/*` 部件、返回 `Ok("")`，于是 12 条测试全绿而库里的正文是空的 ——
    /// 属于「不报错、只是搜不到」那一类，只有把每种扩展名真的喂进 `extract_text` 才守得住。
    /// `xls` 与 `xlsx` 共用 `DocKind::Workbook` 同一个 match 臂，路由由 `xlsx` 这一条覆盖
    /// （本机造不出真 `.xls`，`rust_xlsxwriter` 只写 xlsx）。
    #[test]
    fn every_dispatch_arm_routes_to_its_own_extractor() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "分派正文text".as_bytes()).unwrap();
        let docx = make_office(
            dir.path(),
            "a.docx",
            "word/document.xml",
            r#"<w:document xmlns:w="http://x"><w:body><w:p><w:r><w:t>分派正文word</w:t></w:r></w:p></w:body></w:document>"#,
        );
        let pptx = make_office(
            dir.path(),
            "a.pptx",
            "ppt/slides/slide1.xml",
            r#"<p:sld xmlns:a="http://x" xmlns:p="http://y"><p:cSld><p:sp><p:txBody><a:p><a:r><a:t>分派正文slides</a:t></a:r></a:p></p:txBody></p:sp></p:cSld></p:sld>"#,
        );
        let xlsx = dir.path().join("a.xlsx");
        {
            let mut wb = rust_xlsxwriter::Workbook::new();
            wb.add_worksheet().write(0, 0, "分派正文workbook").unwrap();
            wb.save(&xlsx).unwrap();
        }

        // 四个标记互不相同，所以「接错抽取器」必然表现为拿到空串或 Err，而不是换了一种正文还看不出。
        for (path, want) in [
            (dir.path().join("a.md"), "分派正文text"),
            (docx, "分派正文word"),
            (pptx, "分派正文slides"),
            (xlsx, "分派正文workbook"),
            (fixture("sample-cn.pdf"), "验收"),
        ] {
            let text = extract_text(&path)
                .unwrap_or_else(|e| panic!("{} 应能抽出正文，实际报错 {}", path.display(), e.code));
            assert!(text.contains(want), "{} 分派到了错误的抽取器：{text:?}", path.display());
        }
    }
}
