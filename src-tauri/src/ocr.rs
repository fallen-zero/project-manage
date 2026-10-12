//! Windows 原生图片 OCR（M6）。能力层三件事：探测可用性、识别、清洗 CJK 词间空格。
//! 只读图片、绝不写回；识别文本交回调用方（extract_text）走既有分词入库。
//! 识别本体（`image_text`）在 Task 2 落地，本文件当前只铺探测与清洗。
use std::sync::OnceLock;

/// 本机有没有可用的 OCR 识别器（任一语言）。进程内探一次记忆化——`AvailableRecognizerLanguages`
/// 取值有成本，且降级判据必须全局一致（别在同一轮里忽有忽无）。探测抛错按「不可用」保守处理。
///
/// Task 1 只铺能力层，调用点在 Task 2（extract.rs 的 image_text）与 Task 4/5（索引轮降级判定），
/// 所以现在本 crate 内还没有使用者；同 `db::open_in_memory` 的处理，显式放行而不是全局关掉。
#[allow(dead_code)]
pub fn ocr_available() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    // Err 走 bool 的默认值 false：探测失败即当作没有 OCR。
    *OK.get_or_init(|| windows_probe().unwrap_or_default())
}

#[allow(dead_code)]
fn windows_probe() -> windows::core::Result<bool> {
    use windows::Media::Ocr::OcrEngine;
    let langs = OcrEngine::AvailableRecognizerLanguages()?;
    // Size 是 u32；>0 即本机至少能建一个识别器（不断言必须中文：有英文包时英文图也能 OCR，D3）。
    Ok(langs.Size()? > 0)
}

/// Windows OCR 会在 CJK 字符间插空格（「合 同 验 收」）。仅当**空格两侧都是 CJK** 才删该空格，
/// 保留英文词之间的空格（D7，借 dowse 设计）。
#[allow(dead_code)]
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
}
