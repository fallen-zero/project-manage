//! Windows 原生图片 OCR（M6）。能力层三件事：探测可用性、识别、清洗 CJK 词间空格。
//! 只读图片、绝不写回；识别文本交回调用方（extract_text）走既有分词入库。
use std::sync::OnceLock;

/// 本机有没有可用的 OCR 识别器（任一语言）。进程内探一次记忆化——`AvailableRecognizerLanguages`
/// 取值有成本，且降级判据必须全局一致（别在同一轮里忽有忽无）。探测抛错按「不可用」保守处理。
///
/// 生产调用点：`index_job` worker 每轮开头把它填进 `ScanOptions.ocr_available`（M6 T4，D4 降级门）。
pub fn ocr_available() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    // Err 走 bool 的默认值 false：探测失败即当作没有 OCR。
    *OK.get_or_init(|| windows_probe().unwrap_or_default())
}

// 唯一调用者是 ocr_available（T4 已接上生产调用点，allow 已收口删除）。
fn windows_probe() -> windows::core::Result<bool> {
    use windows::Media::Ocr::OcrEngine;
    let langs = OcrEngine::AvailableRecognizerLanguages()?;
    // Size 是 u32；>0 即本机至少能建一个识别器（不断言必须中文：有英文包时英文图也能 OCR，D3）。
    Ok(langs.Size()? > 0)
}

/// Windows OCR 会在 CJK 字符间插空格（「合 同 验 收」）。仅当**空格两侧都是 CJK** 才删该空格，
/// 保留英文词之间的空格（D7，借 dowse 设计）。
pub fn clean_cjk_spaces(s: &str) -> String {
    let is_cjk = |c: char| matches!(c as u32,
        0x3000..=0x303F | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF);
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for i in 0..chars.len() {
        let c = chars[i];
        if c == ' ' && i > 0 && i + 1 < chars.len()
            && is_cjk(chars[i - 1]) && is_cjk(chars[i + 1]) {
            continue; // 两侧皆 CJK 的空格删掉
        }
        out.push(c);
    }
    out
}

/// 对一张图片跑 Windows 原生 OCR，返回清洗（`clean_cjk_spaces`）后的识别文本。
/// 在**抽取阶段（写锁外）**被调用；只读文件，绝不写回/改动。
///
/// WinRT 异步桥接用 windows-future 0.3.2 的 inherent 方法 `join()`（join.rs:16-21，阻塞等到完成
/// 取结果；非简报初稿写的 `.get()`）——本函数只在 run_pass 的后台 worker 线程上跑，阻塞它无妨。
/// 调用链各步签名逐个读 vendored 源码核实（windows-0.62.2，行号见 task-2-report）：
/// `StorageFile::GetFileFromPathAsync` → `OpenAsync(Read)` → `BitmapDecoder::CreateAsync`
/// → `GetSoftwareBitmapConvertedAsync(Bgra8, Ignore)`（WinRT OCR 只吃 Bgra8/Gray8，统一转 Bgra8）
/// → `OcrEngine::TryCreateFromUserProfileLanguages`（无识别器时返回空指针，windows-core 的
/// from_abi 将其落成 Err 而非 panic，type.rs:43-51）→ `RecognizeAsync` → `Lines/GetAt/Text` 逐行拼接。
/// 任一步失败 → `extract_failed`（D5：单张真失败由调用方落 failed，不吞错）。
/// 生产调用点：M6 T3 起由 `extract::extract_text` 的 `DocKind::Image` 臂调用。
pub fn image_text(path: &std::path::Path) -> crate::error::AppResult<String> {
    use crate::error::AppError;
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};

    // 统一把 WinRT 错误映射为 extract_failed；message 带路径与原始错误，hint 指降级方向。
    let fail = |e: windows::core::Error| {
        AppError::new(
            "extract_failed",
            &format!("Windows OCR 识别失败（{}）：{e}", path.display()),
            Some("图片可能已损坏、格式不支持，或本机 OCR 识别器不可用（可在设置里关闭图片识别）"),
        )
    };

    // WinRT 的路径解析器拒绝混合分隔符/相对段（实测 0x800700A1「无效字符」），先规范成全反斜杠绝对路径。
    // fs::canonicalize 返回带 `\\?\` 前缀的形式，WinRT 同样不认，落 HSTRING 前剥掉该前缀。
    let abs = std::fs::canonicalize(path).map_err(|e| AppError::io(path, &e))?;
    let abs = abs.to_string_lossy();
    let abs: &str = &abs;
    let hpath = windows::core::HSTRING::from(abs.strip_prefix(r"\\?\").unwrap_or(abs));
    let file = StorageFile::GetFileFromPathAsync(&hpath).map_err(fail)?.join().map_err(fail)?;
    let stream = file
        .OpenAsync(FileAccessMode::Read)
        .map_err(fail)?
        .join()
        .map_err(fail)?;
    let decoder = BitmapDecoder::CreateAsync(&stream).map_err(fail)?.join().map_err(fail)?;
    let bitmap = decoder
        .GetSoftwareBitmapConvertedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Ignore)
        .map_err(fail)?
        .join()
        .map_err(fail)?;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(fail)?;
    let result = engine.RecognizeAsync(&bitmap).map_err(fail)?.join().map_err(fail)?;

    let lines = result.Lines().map_err(fail)?;
    let n = lines.Size().map_err(fail)?;
    let mut raw = String::new();
    for i in 0..n {
        if i > 0 {
            raw.push('\n');
        }
        // HSTRING 的 Deref 目标是 [u16]（windows-strings 0.5.1 hstring.rs:65），须走 to_string_lossy 转 String。
        raw.push_str(&lines.GetAt(i).map_err(fail)?.Text().map_err(fail)?.to_string_lossy());
    }
    Ok(clean_cjk_spaces(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_spaces_between_cjk_only() {
        assert_eq!(clean_cjk_spaces("合 同 验 收"), "合同验收");
    }
    #[test]
    fn keeps_spaces_around_latin() {
        // "contract" 两侧是拉丁，其后的空格必须留；中英边界只删 CJK-CJK 之间那个。
        assert_eq!(clean_cjk_spaces("contract 验收 单"), "contract 验收单");
    }
    #[test]
    fn idempotent_and_edge_spaces_kept_when_single_side_noncjk() {
        assert_eq!(clean_cjk_spaces("合同"), "合同");
        assert_eq!(clean_cjk_spaces(""), "");
        assert_eq!(clean_cjk_spaces(" 双 空格 "), " 双空格 "); // 首尾非 CJK 邻的空格不动
    }
    #[test]
    fn probe_returns_a_bool_without_panicking() {
        let _ = ocr_available(); // 记忆化、有/无语言包都必须不 panic
        assert!(matches!(ocr_available(), true | false));
    }

    /// 条件跑：本机装了 OCR 语言包（开发机 zh-Hans-CN 已确认）就真识别一枚已知中文夹具；
    /// 没装则显式 SKIP 打印——不假装通过（§八.5 降级路径由 ocr_available 的探测承担）。
    #[test]
    fn image_text_reads_known_chinese_when_ocr_available() {
        if !ocr_available() {
            eprintln!("SKIP: 本机无 OCR 语言包，降级路径（§八.5）非未测");
            return;
        }
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-cn-ocr.png");
        let t = image_text(&p).expect("fixture 应能识别");
        // 断言只回脱敏读数（长度 + 空格可见化），不打印整段正文（红线4）。
        assert!(t.replace(' ', "").contains("合同验收OCR"),
            "清洗+识别应含已知词，实得长度 {}，空格可见化：{}",
            t.chars().count(), t.replace(' ', "\u{2423}"));
        // 清洗在 image_text 里承重（本机实测 raw 含 CJK 字间空格）：未清洗则输出是「合 同 验 收…」，
        // 这条不带 replace 的断言会变红。变异取证见 task-2-report。
        assert!(t.contains("合同验收"), "image_text 输出应经 clean_cjk_spaces，实得空格可见化：{}",
            t.replace(' ', "\u{2423}"));
    }

    /// 不依赖 OCR 语言包的稳健错误路径：内容是垃圾字节的「.png」必须优雅落 Err（不 panic）。
    /// 无 OCR 的盒子走 TryCreateFromUserProfileLanguages 失败，有 OCR 的走解码失败——两条都算 Err。
    #[test]
    fn image_text_rejects_non_image_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("fake.png");
        std::fs::write(&p, b"this is definitely not a PNG payload").unwrap();
        let r = image_text(&p);
        assert!(r.is_err(), "非图片字节应落 Err，不得 panic");
    }
}
