//! 分词与查询串构造。入库和查询都只能从这里拿词典，这是整套全文检索的地基约束：
//! 一旦两侧词表不一致，库里是「验收 指标」、查询侧切成「验 收指 标」，搜索表现为
//! 「界面在工作、就是搜不到」，且不会报任何错。
//!
//! 版本相关行为（同一词典、同一预处理）由 cargo.lock 钉住；换 jieba-rs 版本 = 重建索引，
//! 不是可选项，M3 不做词典版本号兼容。

use std::sync::OnceLock;

use jieba_rs::Jieba;

fn jieba() -> &'static Jieba {
    static J: OnceLock<Jieba> = OnceLock::new();
    // Jieba::new() 载入内置词典，是百毫秒级的一次性开销，故用 OnceLock 复用。
    J.get_or_init(Jieba::new)
}

/// 入库侧唯一入口。用 `cut_for_search` 而不是 `cut`：它是 `cut` 的超集，会把「付款条件」
/// 这类复合词再切成 付款/条件，查询侧按 `cut` 切出来的子词才命得中。
/// 换来的是中文复合词的召回，代价是索引膨胀。膨胀率随文本波动，**不是容量常数**：
/// brief 那个样本实测是 21 → 23 词条（约 1.11 倍），那只是一次测量的一个串，别拿它做预算；
/// task-2-report §2 的分词实测表里 `十二个月` 5 个字就切出 `十二 二个 十二个 月` 4 个 token，
/// 说明同一函数在别的输入上贵得多。Task 7 估库体积请以那张表为准、并往更保守的方向取值。
///
/// 标点保留在正文里：unicode61 不把标点当索引字符，所以它不会造成假命中，
/// 而 `snippet()` 是从原始列文本重建摘要的，留着标点才可读。
#[allow(dead_code)] // 生产调用点在 Task 7（写入侧）/ Task 8（检索侧）落地后才出现
pub fn index_text(text: &str) -> String {
    jieba()
        .cut_for_search(text, true)
        .into_iter()
        .map(|t| t.word.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 查询侧唯一入口。每个词都包成 `"词"`，FTS5 的操作符（`NEAR`/`*`/`-`/`|`/引号）
/// 就只能是普通字符，查询串无法注入语法；串内的 `"` 按 FTS5 规则翻倍成 `""`。
///
/// 前缀查询的关键约束：`*` 必须写在引号外面。`"维保"*` 能命中索引里的「维保期」，
/// `"维保*"` 里的 `*` 只是词内一个字面字符，永远命不中。
#[allow(dead_code)] // 同 index_text：Task 8 的 doc_hits 才把这段表达式喂给 MATCH
pub fn query_expression(query: &str, prefix: bool) -> Option<String> {
    let terms: Vec<String> = jieba()
        .cut(query, true)
        .into_iter()
        .map(|x| x.word.trim().to_owned())
        // 纯标点词永不进索引，留在查询里只会把整条 AND 变成 0 命中（实测「验收，」即如此）。
        // 这里用 is_alphanumeric 而不是「非空白」，是有意的不对称，别当 bug「修掉」：
        // unicode61 把符号类别（Sm/Sc/Sk/So，如 ™ € ± ★）也当词字符并索引，
        // 而 char::is_alphanumeric() 对它们回 false，于是整串只有符号的查询会返回 None（连 SQL 都不发），
        // 「25°」这类词会退化成只查「25」。这是计划裁定的行为、影响面窄（符号单独成词，或符号紧贴数字），
        // 故保持判据不变，只在此登记两侧口径为何不一致。
        .filter(|t| !t.is_empty() && t.chars().any(|c| c.is_alphanumeric()))
        .map(|t| {
            let quoted = format!("\"{}\"", t.replace('"', "\"\""));
            if prefix {
                format!("{quoted}*")
            } else {
                quoted
            }
        })
        .collect();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

pub(crate) fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F   // CJK 符号与标点
        | 0x3400..=0x4DBF // 扩展 A
        | 0x4E00..=0x9FFF // 基本汉字
        | 0xF900..=0xFAFF // 兼容汉字
        | 0xFF00..=0xFFEF // 半宽全宽形式
    )
}

/// `snippet()` 回的是我们插入空格连接起来的词条串，直接展示会读成「甲 方 要 求」。
/// 规则只有一条：空格相邻任一侧是 CJK 就删掉。Latin 之间的空格原样保留，
/// 数字与中文之间也删（「800 毫秒」→「800毫秒」）。省略号 `⋯` 两侧不带空格，故不受影响。
#[allow(dead_code)] // 同 index_text：Task 8 用它清洗 snippet() 出来的摘要
pub fn clean_snippet(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len());
    for (i, c) in chars.iter().enumerate() {
        if *c == ' ' {
            let near_cjk = (i > 0 && is_cjk(chars[i - 1]))
                || chars.get(i + 1).is_some_and(|n| is_cjk(*n));
            if near_cjk {
                continue;
            }
        }
        out.push(*c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    /// 建一张与生产同形状的虚表（只两列 + unicode61），词条由本模块预先切好。
    fn fts() -> Connection {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE tmp_fts USING fts5(name_tokens, body_tokens, tokenize = 'unicode61');",
        )
        .unwrap();
        conn
    }

    fn put(conn: &Connection, rowid: i64, name: &str, body: &str) {
        conn.execute(
            "INSERT INTO tmp_fts (rowid, name_tokens, body_tokens) VALUES (?1, ?2, ?3)",
            params![rowid, index_text(name), index_text(body)],
        )
        .unwrap();
    }

    fn rows(conn: &Connection, q: &str, prefix: bool) -> Vec<i64> {
        let Some(expr) = query_expression(q, prefix) else {
            return Vec::new();
        };
        let mut stmt = conn
            .prepare("SELECT rowid FROM tmp_fts WHERE tmp_fts MATCH ?1 ORDER BY rowid")
            .unwrap();
        stmt.query_map(params![expr], |r| r.get::<_, i64>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// 入库与查询必须出自同一个词典：这是整套检索的地基约束，用一条命中把两边绑在一起。
    #[test]
    fn index_and_query_share_one_dictionary() {
        let conn = fts();
        put(&conn, 1, "合同验收说明.docx", "甲方要求验收指标");
        assert_eq!(rows(&conn, "验收", false), vec![1]);
        assert_eq!(rows(&conn, "验收指标", false), vec![1], "查询侧切出来的词必须能和库里的对上");
        assert!(rows(&conn, "根本没写过", false).is_empty());
        // 入库侧必须保留标点：`body_tokens` 就是 snippet() 拿来渲染的那一列，
        // 谁在这里把标点过滤掉，摘要会变成「甲方要求验收指标」这种没气口的串，
        // 而查询侧（:47 丢弃纯标点词）永远不会因此变红，所以只能在这一侧钉住。
        assert!(index_text("合同，报价。").contains('，'), "入库侧不许过滤标点");
        assert_eq!(index_text(""), "", "空串不该产出一个空格");
    }

    /// 事实 5：cut_for_search 会把「付款条件」再切成 付款/条件，所以分开的词也命得中。
    #[test]
    fn compound_words_are_indexed_as_whole_and_parts() {
        let conn = fts();
        put(&conn, 1, "报价单.xlsx", "里面只有付款条件与验收流程");
        assert_eq!(rows(&conn, "付款条件", false), vec![1]);
        assert_eq!(rows(&conn, "付款 条件", false), vec![1], "子词没进索引的话这条会空");
    }

    /// 事实 6：jieba 把「维保期」当一个词，精确查「维保」命不中，必须靠前缀那段。
    #[test]
    fn prefix_stage_rescues_queries_shorter_than_the_indexed_word() {
        let conn = fts();
        put(&conn, 1, "维保期说明.docx", "维保期为十二个月");
        assert!(rows(&conn, "维保", false).is_empty(), "先证明精确段确实够不着");
        assert_eq!(rows(&conn, "维保", true), vec![1], "前缀段把它救回来");
    }

    /// 事实 7：查询串里的 FTS5 语法词与操作符不能改变查询含义；事实 8：纯标点词被丢弃。
    #[test]
    fn query_syntax_cannot_leak_and_punctuation_is_dropped() {
        let conn = fts();
        put(&conn, 1, "合同.docx", "合同与报价，条款从优");
        put(&conn, 2, "报价.docx", "报价单一份");
        assert_eq!(rows(&conn, "合同 OR 报价", false), Vec::<i64>::new(), "OR 只是普通字，整条要求两个词都在");
        assert_eq!(rows(&conn, "NEAR(合同 报价)", false), Vec::<i64>::new());
        assert_eq!(rows(&conn, "合同*", false), vec![1], "* 被引号吃掉，退化成精确词");
        assert_eq!(rows(&conn, "合同，", false), vec![1], "标点词丢弃后仍按「合同」查");
        assert_eq!(rows(&conn, "，。！ ", false), Vec::<i64>::new(), "全是标点时不发 SQL");
        assert!(query_expression("维保", true).unwrap().starts_with('"'), "前缀号只能出现在引号外");
        assert_eq!(query_expression("维保", true).unwrap(), "\"维保\"*");
    }

    /// 事实 9：只删相邻 CJK 的空格，Latin 之间的空格保留。
    #[test]
    fn clean_snippet_only_collapses_cjk_boundaries() {
        assert_eq!(
            clean_snippet("甲方 要求 ， 响应 时间 不 超过 800 毫秒 。 [验收] 指标 见 合同⋯"),
            "甲方要求，响应时间不超过800毫秒。[验收]指标见合同⋯"
        );
        assert_eq!(
            clean_snippet("the [quick] brown fox 与 中文 混排"),
            "the [quick] brown fox与中文混排"
        );
        assert_eq!(clean_snippet("报价 单 2026 年"), "报价单2026年");
        assert_eq!(clean_snippet(""), "");
    }
}
