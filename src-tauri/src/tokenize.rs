//! 分词与查询串构造。入库和查询都只能从这里拿词典，这是整套全文检索的地基约束：
//! 一旦两侧词表不一致，库里是「验收 指标」、查询侧切成「验 收指 标」，搜索表现为
//! 「界面在工作、就是搜不到」，且不会报任何错。
//!
//! 版本相关行为（同一词典、同一预处理）由 cargo.lock 钉住；换 jieba-rs 版本 = 重建索引，
//! 不是可选项，M3 不做词典版本号兼容。

use std::sync::OnceLock;

use jieba_rs::Jieba;

/// 查询串长度上限（按 char 计）。两条检索通路的边界守卫都引这一个数：
/// 之前 `search::field_hits` 与 `index_store::doc_hits` 的守卫各写了一遍字面量 128，改一处就会让两边口径分家。
/// 家放在这里而不是 `search.rs`：`index_store` 本来就 `use crate::tokenize::...`，
/// 反过来引 `search.rs` 会新造一条 `index_store → search` 的反向依赖，只为搬一个数。
pub const MAX_QUERY_CHARS: usize = 128;

fn jieba() -> &'static Jieba {
    static J: OnceLock<Jieba> = OnceLock::new();
    // Jieba::new() 载入内置词典，是百毫秒级的一次性开销，故用 OnceLock 复用。
    J.get_or_init(Jieba::new)
}

/// 入库侧唯一入口。用 `cut_for_search` 而不是 `cut`：它是 `cut` 的超集，会把「付款条件」
/// 这类复合词再切成 付款/条件，查询侧按 `cut` 切出来的子词才命得中（这条召回由
/// `compound_words_are_indexed_as_whole_and_parts` 钉住）。代价是索引膨胀，而膨胀率随输入波动，
/// **不是容量常数**：实测 `cut_for_search` 相对 `cut` 的词条数比从 1.0（`合同与报价，条款从优`）
/// 到 1.57（`这份报价单含税总价为十二个月`）。Task 7 估库体积请取偏上的值，别拿单个比值做预算。
///
/// 标点保留在正文里：unicode61 默认的 token 字符集是 `categories = "L* N* Co"`
/// （libsqlite3-sys-0.38.2 bundled `sqlite3.c:265946`），标点 P* 不在其中、是分隔符，
/// 所以它不会造成假命中；而 `snippet()` 是从原始列文本重建摘要的，留着标点才可读。
pub fn index_text(text: &str) -> String {
    jieba()
        .cut_for_search(text, true)
        .into_iter()
        .map(|t| t.word.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 查询侧唯一的切词入口：`cut` + 去空白 + 丢纯标点词，返回 `cut` 的原序。
/// 两个消费方：`query_expression` 拼 FTS5 语法，`doc_preview`（M4 Task 5 落地）拿它决定原文里高亮哪些词。
/// 必须是同一个入口——预览若另起一次切词，两处词表会随词典版本各自漂移，
/// 表现为「列表里摘要标出了这个词、点开原文却没有标记」。
pub fn query_terms(query: &str) -> Vec<String> {
    jieba()
        .cut(query, true)
        .into_iter()
        .map(|x| x.word.trim().to_owned())
        // 纯标点词永不进索引，留在查询里只会把整条 AND 变成 0 命中（实测「验收，」即如此）。
        // 这里用 is_alphanumeric 而不是「非空白」，是因为它和索引侧几乎同口径：unicode61 默认只把
        // `L* N* Co` 当词字符，标点 P* 与符号 S*（™ € ± ★）都是分隔符、永不进索引，
        // 所以丢弃「整词无字母数字」没有信息损失。唯一比索引侧窄的是私有使用区 Co——它是词字符
        // 而 is_alphanumeric 回 false，于是全由 Co 组成的查询词会被丢掉、返回无结果而不是错结果。
        // 本项目索引的是中/英/数字文本，故判据保持不变，只在此登记这一处口径差。
        .filter(|t| !t.is_empty() && t.chars().any(|c| c.is_alphanumeric()))
        .collect()
}

/// 查询侧唯一出口。每个词都包成 `"词"`，FTS5 的操作符（`NEAR`/`*`/`-`/`|`/引号）
/// 就只能是普通字符，查询串无法注入语法；串内的 `"` 按 FTS5 规则翻倍成 `""`。
///
/// 前缀查询的关键约束：`*` 必须写在引号外面。`"维保"*` 能命中索引里的「维保期」，
/// `"维保*"` 里的 `*` 只是词内一个字面字符，永远命不中。
pub fn query_expression(query: &str, prefix: bool) -> Option<String> {
    let terms: Vec<String> = query_terms(query)
        .into_iter()
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
/// 两步：① 折掉 `cut_for_search` 造成的复合词重复；② 空格相邻任一侧是 CJK 就删掉。
/// 第二步的规则一字不变（Latin 之间的空格保留、数字与中文之间也删、`⋯` 两侧不带空格故不受影响）。
pub fn clean_snippet(raw: &str) -> String {
    // 按单个空格切再拼回，是无损往返（`split(' ')` 保留空段），所以下面第二步看到的串与入参同形。
    let folded = fold_duplicated_compound(&raw.split(' ').collect::<Vec<_>>());
    let joined = folded.join(" ");
    let chars: Vec<char> = joined.chars().collect();
    let mut out = String::with_capacity(joined.len());
    for (i, c) in chars.iter().enumerate() {
        if *c == ' ' {
            let near_cjk = (i > 0 && is_cjk(chars[i - 1])) || chars.get(i + 1).is_some_and(|n| is_cjk(*n));
            if near_cjk {
                continue;
            }
        }
        out.push(*c);
    }
    out
}

/// 折叠对象只有一种：紧跟在复合词 W 左侧的那一串词，每一个都是 W 的**真子串**，
/// 且它们在 W 里首次出现的区间并起来**恰好铺满 W**。
/// 「铺满」这一步就是它不篡改正文的原因：单枚真子串必然比 W 短、铺不满，所以
/// 「用户真的把某个词写了两遍」（`数据 数据`、`甲方 甲方要求`）永远落在判据之外；
/// 而 `cut_for_search` 吐出的子词组（实测 `付款 条件 付款条件`、`报价 价单 报价单`）恰好铺满。
/// 带 `[` / `]` 高亮标记的词一律不参与：标记是 `技术方案.md` 6.1 要求暴露给前端的信息，
/// 为了观感吃掉它，代价比重复词大。
/// 签名里只给内层 `&str` 命名生命周期：返回值只从词里抄 `&str`，不带外层切片的借用。
fn fold_duplicated_compound<'a>(tokens: &[&'a str]) -> Vec<&'a str> {
    let mut drop = vec![false; tokens.len()];
    for idx in 0..tokens.len() {
        let run = foldable_run(tokens, idx);
        for k in 1..=run {
            drop[idx - k] = true;
        }
    }
    tokens
        .iter()
        .enumerate()
        .filter(|(i, _)| !drop[*i])
        .map(|(_, t)| *t)
        .collect()
}

/// 返回 `tokens[w_idx]` 左侧应丢掉的词数（0 = 不折）。左侧任何一处不满足条件就整组放弃。
fn foldable_run(tokens: &[&str], w_idx: usize) -> usize {
    let whole: Vec<char> = tokens[w_idx].chars().collect();
    if whole.len() < 2 || tokens[w_idx].contains('[') || tokens[w_idx].contains(']') {
        return 0;
    }
    let mut covered = vec![false; whole.len()];
    let mut run = 0usize;
    let mut i = w_idx;
    while i > 0 {
        i -= 1;
        let part = tokens[i];
        let part_chars: Vec<char> = part.chars().collect();
        // 空段（连续空格）算「不是真子串」→ 整组放弃，保守。
        if part_chars.is_empty()
            || part_chars.len() >= whole.len()
            || part.contains('[')
            || part.contains(']')
            || !tokens[w_idx].contains(part)
        {
            return 0;
        }
        // 用首次出现的区间：稳定，且不会为凑铺满去找最宽松的落点。
        let start_chars = tokens[w_idx][..tokens[w_idx].find(part).unwrap_or(0)]
            .chars()
            .count();
        for slot in &mut covered[start_chars..start_chars + part_chars.len()] {
            *slot = true;
        }
        run += 1;
        if covered.iter().all(|c| *c) {
            return run;
        }
    }
    0
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
        // 而查询侧（`query_expression` 里丢弃非字母数字词的那个 filter）永远不会因此变红，所以只能在这一侧钉住。
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

    /// 折叠规则：只有「前面若干个词恰好铺满后面那个复合词」才折，铺不满就不折。
    /// 正例两枚都来自实测的 `cut_for_search` 输出：`付款 条件 付款条件` 是**无重叠铺满**，
    /// `报价 价单 报价单` 是**有重叠铺满**（报价=0..2、价单=1..3），后者证明规则不要求子词互不相交。
    #[test]
    fn clean_snippet_folds_the_indexed_compound_back_into_one_word() {
        assert_eq!(
            clean_snippet("里面 只有 付款 条件 付款条件 与 验收 流程"),
            "里面只有付款条件与验收流程"
        );
        assert_eq!(clean_snippet("报价 价单 报价单 一份"), "报价单一份", "重叠子词同样被铺满判据收下");
        // 用户真写了两遍的情形：两个词等长，谁都不是对方的真子串，一个字都不许动。
        assert_eq!(clean_snippet("数据 数据 库"), "数据数据库", "相邻同词是原文真重复，折了就是篡改正文");
        assert_eq!(clean_snippet("甲方 甲方要求"), "甲方甲方要求", "单枚前缀铺不满复合词，不许折");
    }

    /// `[` / `]` 是 `技术方案.md` 6.1 要求暴露给前端的高亮标记，折叠不许把它吃掉；
    /// 摘要里那枚省略号 `⋯` 会粘在首个词上（`⋯付款` 不是 `付款条件` 的子串），
    /// 于是被截断的窗口首格不折叠——这是**已知取舍**，宁可不折也不折错。
    #[test]
    fn clean_snippet_keeps_the_highlight_markers_and_leaves_truncated_runs_alone() {
        assert_eq!(clean_snippet("[付款] 条件 付款条件"), "[付款]条件付款条件", "带标记就不折，标记是界面契约");
        assert_eq!(clean_snippet("付款 [条件] 付款条件"), "付款[条件]付款条件");
        assert_eq!(
            clean_snippet("⋯付款 条件 付款条件 与 验收"),
            "⋯付款条件付款条件与验收",
            "省略号粘住首词时放弃折叠（观感问题不该吃掉了召回证据）"
        );
    }

    /// 查询侧唯一切词入口的三条行为：丢空白词、丢纯标点词、保持 `cut` 的原序。
    /// 期望向量不是想当然：本轮在仓库外用同一版 jieba-rs 实测过
    /// `cut("合同 报价")` 确实吐出一枚独立的 `" "` 词（被 `trim` + `is_empty` 丢掉），
    /// `cut("验收，")` = `["验收", "，"]`。
    #[test]
    fn query_terms_drops_blanks_and_punctuation_only_words() {
        assert_eq!(query_terms("验收"), vec!["验收"]);
        assert_eq!(
            query_terms("验收，"),
            vec!["验收"],
            "纯标点词进不了索引，留在查询里只会把整条 AND 变成 0 命中"
        );
        assert_eq!(query_terms("合同 报价"), vec!["合同", "报价"], "丢掉空白词后，词序就是 cut 的返回序");
        assert!(query_terms("，。！ ").is_empty(), "全标点时一个词都不产出");
        assert!(query_terms("").is_empty());
        assert!(query_terms("   ").is_empty());
    }

    /// 「入库与查询同一个分词器」这条红线的**结构**守卫：查询侧只允许有一处 `cut`。
    /// 为什么不写成行为断言：把 `query_expression` 里那份切词复制一份出来，两边结果照样相等，
    /// 任何行为断言都绿 —— 那条断言不可证伪。数源码里的出现次数才守得住「别再加第二套词表」。
    /// `index_text` 用的是 `.cut_for_search(`，与本 needle 不重叠（needle 要求左括号紧跟 `cut`）。
    /// needle 必须用 `concat!` 拼：写成字面量会数到它自己。
    /// 因此本文件里不许出现第二处带点的 `cut` 字面量——注释与测试里也算。要在文档里提这个调用，写成 `cut(...)` 或用反引号把点断开，否则这条守卫会在没有任何代码回归的情况下红。
    #[test]
    fn query_side_has_exactly_one_cut() {
        let src = include_str!("tokenize.rs");
        let n = src.match_indices(concat!(".cut", "(")).count();
        assert_eq!(n, 1, "查询侧的切词只能有 `query_terms` 里那一处，实测 {n} 次");
    }
}
