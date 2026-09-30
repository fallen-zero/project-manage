//! 按扩展名抽文本：一个路径进，一段中文纯文本出。
//!
//! 这一层不碰数据库、不判断状态，只回答「抽得出吗」。空字符串是有效结果（扫描件 PDF
//! 没有文字层、空文件都归到这里），由调用方标成 `skipped/empty_text`，不是失败。

use std::io::Read;
use std::path::Path;

use crate::error::{AppError, AppResult};

/// 错误码是前端要分支的稳定契约，所以两个码名不要改：
/// extract_unsupported（类型不支持）、extract_failed（抽取过程报错）
#[allow(dead_code)] // 第一个 caller 在 Task 4 的 Office 分支，落地时删掉本行
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

#[allow(dead_code)] // caller 是 Task 5 的 extract_one 分派，落地时删掉本行（decode_text_bytes 若一起被报，同批删）
fn read_text_file(path: &Path) -> AppResult<String> {
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .and_then(|mut f| f.read_to_end(&mut buf))
        .map_err(|e| AppError::io(path, &e))?;
    Ok(decode_text_bytes(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
