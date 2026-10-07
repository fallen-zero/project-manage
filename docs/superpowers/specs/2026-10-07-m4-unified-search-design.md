# M4 首屏统一检索 设计

- 状态：已与需求方逐段确认（2026-10-07，四个岔口由需求方选定，另有两个由控制方代定并标注可推翻）
- 上游 spec：`docs/技术方案.md` 第一节需求 1/2/3、§3.3 摘要与高亮、§3.4 跨域结果合并、§6.1 两段检索、第八节里程碑 M4
- 本文是 M4 的实现依据；与 `docs/技术方案.md` 分歧时以那份为准并回写它。

## 一、本轮交付与边界

M3 已经交出两条互不相认的检索通路：`search_local` → `FieldHit[]`（台账明文列 LIKE，六段各 50 条上限），`search_docs` → `DocHit[]`（FTS5 + jieba 预分词，带 `matchedBy` 与 `score`）。首屏 `SearchPage` 只接了前者，正文那条只在 `/index` 的试验台里能看。

本轮把它们合成一个用户能用的东西，交付面四件：

1. `search_all` —— 一次 IPC 往返回一个带分组结构的 bundle
2. 三段渲染（项目档案 / 台账条目 / 文件正文），台账与正文两段**段内按项目聚簇**
3. `doc_preview` —— 点开正文命中时按行 id 从磁盘重抽原文、在原文上做大小写不敏感子串高亮
4. 预览里的三个动作：打开文件 / 打开所在目录 / 复制路径（全部复用 M1 已有能力）

**边界**：M3 划给 M4 的 12 项账，本轮实际拉进来的只有一件，而且不是清账、是避免制造账——见第五节末尾。其余 11 项列在第十节，逐项写明归属与为什么。

## 二、决策与理由

| # | 决定 | 被否掉的替代与为什么 |
|---|---|---|
| D1 | 合并与聚合放 **Rust 侧**（新增 `search_all` 命令，内部同时调 `search::field_hits` 与 `index_store::doc_hits`） | 前端并发两条自己拼：改动小，但「同项目相邻 + 每段取前 N」的口径只活在 ts 里，进不了 `cargo test` 的门禁网，而两边各自的 50 条上限叠起来后总数不可控。改造 `search_local` 返回全部：`/index` 试验台也在用 `search_docs`，改完两边语义分家，且 `search.rs` 要 import `index_store`，模块边界变浑。 |
| D2 | 三段骨架，「项目」段的语义钉死为**项目档案本身的命中**（`source == "project"`），不是聚合轴；台账段与正文段各自段内按 `project_id` 聚簇，**不做跨段的项目合并** | 跨段合并会让同一项目的台账行与正文行比邻，而 `bm25` 与 LIKE 命中根本没有可比分数——§3.4 明令「不做假的归一化」。七段平铺（六段台账 + 正文）：改动最小，但命中跨 8 个项目时同一个东西的两类结果隔三屏远，「在哪个项目里」要用户自己拼。 |
| D3 | 簇间排序**不用分数**：按该簇命中条数降序、并列按项目名升序；簇内保持来源自身的序（台账按现有 SQL 序、正文按 `bm25` 升序） | 按「簇内最强分数」排需要跨域可比分数，即 D2 否掉的那件事。 |
| D4 | 预览入参**只有 `doc_id`**，`path` 由后端从 `index_docs` 取，且要求 `index_status = 'ok'` 才读盘 | 按 `path` 取 + 后端前缀校验：好处是未入库的文本也能点开看，代价是整条通路的安全全押在一条 `substr` 比较上，而 Windows 的盘符大小写 / 结尾分隔符 / 8.3 短名 / `\\?\` 前缀 / junction 全是绕过面，会把 M3 的 m5（路径从未归一化）从 Minor 升级成本轮必修 + 一次 schema 改动。按行 id 取，路径权威就收在库内，前端根本拿不到路径参数。 |
| D5 | 列表摘要继续用 `snippet()`，观感问题（`cut_for_search` 复合词重复两次）在**展示清洗层**解决 | 加 `UNINDEXED` 正文列取原文摘要：要进 V5 迁移，而旧行的正文列只能重抽才能填上 ⇒ 所有已建索引一次全量重建（真实库是几十 GB 重扫），为一个观感问题付这个代价不成比例。接受现状不处理：用户会当成 bug。 |
| D6（控制方代定，可推翻） | `search_all` 一次往返跑完 6 段 LIKE + 1 条 FTS，**不**提前拆「正文先回、台账后回」的分段渲染 | 拆段多一条命令、多一处时序，而 spec §十-1 的「性能数字未做运行时验证」这笔账要在真机沙盒里量一次才有资格决定怎么拆。量法见第九节。 |
| D7（控制方代定，可推翻） | `matchedBy` 不逐条标 `exact`，只给 `prefix` 的行一个小徽章，并在该段**第一次**出现放宽命中时补一行说明 | 逐条标 `exact` 是给绝大多数结果加噪音；完全不标则违反 §6.1「噪音由标注暴露给前端」。 |

## 三、契约

Rust 侧新增（`search.rs`）与前端 `src/types/search.ts` 逐字段对应，serde 一律 `camelCase`：

```rust
#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct SearchBundle {
    pub query: String,
    pub projects: Vec<FieldHit>,      // source == "project" 的命中，最多 10 条
    /// 被 `MAX_PROJECT_HITS` 截掉的条数。「项目」段是平铺列表，没有簇可挂计数。
    pub projects_hidden: usize,
    pub ledger: ClusterSection<LedgerCluster>,
    pub docs: ClusterSection<DocCluster>,
    pub relaxed: bool,                // 正文段里存在 matchedBy == "prefix" ⇒ 段级说明只出现一次
    /// COUNT(DISTINCT d.project_id)，谓词必须与 `index_store.rs` 的 SELECT_SQL 同口径：
    /// `d.index_status = 'ok' AND p.deleted_at IS NULL`。少半个谓词就会出现「文案说正文有索引、
    /// 正文段却是空的」这种自相矛盾的空态。
    pub indexed_projects: usize,
}

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct ClusterSection<C> { pub clusters: Vec<C>, pub hidden_clusters: usize }

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct LedgerCluster { pub project_id: String, pub project_name: String,
                           pub items: Vec<FieldHit>, pub hidden: usize }

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct DocCluster    { pub project_id: String, pub project_name: String,
                           pub items: Vec<DocHit>,  pub hidden: usize }

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct DocPreview {
    pub doc_id: String, pub path: String, pub project_name: String,
    /// 窗口拼接后的原文，不是整篇。窗口之间用 `PREVIEW_GAP` 分隔串隔开，分隔串**留在** `text` 里。
    pub text: String,
    /// **UTF-16 码元偏移**（既不是字符偏移也不是字节偏移），后端算好。
    /// serde 把 `(usize, usize)` 写成 JSON 数组，前端类型是 `[number, number][]`。
    pub ranges: Vec<(usize, usize)>,
    /// `text` 不等于整篇原文时为 `true`：任一窗口把原文切断了，或有命中没能进窗口。
    pub truncated: bool,
}
```

偏移为什么取 UTF-16 码元：前端要按 range 切片上色，`String.prototype.slice` 用的正是码元下标，取这个单位前端零换算。取 char 偏移则会在「命中前出现过 emoji 等 BMP 外字符」的真实文本上错一格——这种错位静态看不见，只能靠一条带 BMP 外字符的测试钉住（第八节点名）。后端换算规则：窗口内定位置换按 char 走，产出 range 时按 `char::len_utf16()` 累加一次即可。

命令两条，都加进 `lib.rs` 的 `generate_handler!`：

```rust
fn search_all(state: State<AppState>, query: String) -> AppResult<SearchBundle>;
fn doc_preview(state: State<AppState>, doc_id: String, query: String) -> AppResult<DocPreview>;
```

`search_local` 与 `search_docs` **保留不动**：`/index` 的试验台要继续能吃 `search_docs`，它验的是索引侧召回，与首屏的呈现口径不是同一件事。这一点要写进试验台那一段的页面说明，免得首屏上线后试验台变成「第二套真值来源」。

`indexed_projects` 的动机是 M3 的 m8（文案对「根不存在」和「0 个可索引文件」都说同一句话）：首屏空态在「一个项目都还没索引」时必须说「正文还没建索引，去 `/index` 启动」，而不是「没有命中」。

## 四、排序与截断口径

数值全部写成具名常量，**不许在两处各写一个字面量**。落点：取数与展示上限在 `search.rs` 顶部，预览侧（`PREVIEW_WINDOW_CHARS` / `MAX_PREVIEW_WINDOWS` / `PREVIEW_GAP`）在 `doc_preview.rs` 顶部，查询长度守卫见下面第三条。

- 取数：`field_hits` 沿用既有 `PER_GROUP_LIMIT = 50`（`search.rs:16`，不动）；`doc_hits` 取 200，与 `search_docs` 现有 clamp 上界一致（`lib.rs:542` 的 `clamp(1, 200)`），不新造数字。
- 展示：`MAX_CLUSTERS_PER_SECTION = 20`、`MAX_ITEMS_PER_CLUSTER = 10`、`MAX_PROJECT_HITS = 10`。三处截断各有承载，漏一个就等于把结果悄悄扔掉：簇内被截掉的条数进该簇 `hidden`；整簇被段级上限扔掉的簇数进 `ClusterSection.hidden_clusters`；「项目」段是平铺列表没有簇可挂，截掉的条数进 `projects_hidden`。三个数都只显示、**不做展开** —— 被截掉的行压根没进 bundle，要真展开得再发一次带分页的 IPC，本轮不做（第十节）。
- 查询守卫：`MAX_QUERY_CHARS = 128`。今天这个数在 `search.rs:87` 与 `index_store.rs:262` 各写了一遍字面量 128，本轮提到一处共享常量。落点选 `tokenize.rs`（查询侧模块，`index_store` 本来就引它），而不是 `search.rs` —— 后者要新造一条 `index_store → search` 的反向依赖，只为搬一个数。

## 五、预览通路

1. 按 `doc_id` 取 `index_docs` 行（`SELECT d.path, d.index_status, p.name ... JOIN projects p ON p.id = d.project_id`，带上 `p.deleted_at IS NULL`），行不存在或项目已软删 ⇒ `preview_unavailable`；`index_status != 'ok'` ⇒ `preview_unavailable`（`skipped`/`failed` 的行本来就没有正文可抽，不能假装能读）。软删的那半是防御：预览的入参只有 id，靠这一条把首屏之外的旧 id 也关在索引作用域里。
2. `index_docs` 里没有任何列指向登记动作，所以**不试图反查根目录归属**：能被读的文件恰好等于「曾被索引为 `ok` 的行」，这个集合就是索引作用域，D4 已经把它关住了。
3. 抽取调 `extract::extract_text(path)`（同一条扩展名分派表、同一套 `chardetng` + `encoding_rs`）。这一步**必须包 panic 边界**：见下面的小节。
4. 命中位置在**原文**上算。词只从一个地方拿：`tokenize.rs` 新增 `pub fn query_terms(query: &str) -> Vec<String>`，把现在写在 `query_expression`（`:42`）里的 `jieba().cut(q, true)` + 同一个 `is_alphanumeric` 过滤抽出来，`query_expression` 改调它再拼 FTS 语法。这样「检索命中的词」与「预览高亮的词」天然是同一套切词，符合 spec 的入库/查询同词器红线；若预览另起一次 `cut`，两处口径会随词典版本漂。匹配是大小写不敏感的子串匹配，规则写成一句可实现的：两侧各按 `char` 逐个取 `to_lowercase()` 的**第一个**字符再比，因而偏移永不因折叠而变长（`İ` 这类会展开成两字符的映射按首字符比，接受它匹配不到）。中文没有大小写，走同一条代码路径。
5. 窗口化：以第一个命中为中心取 `±PREVIEW_WINDOW_CHARS = 4000` 字符（按 char 计，不要求落在词或行的边界上）；后续命中若落在已有窗口内就并入，超出 `MAX_PREVIEW_WINDOWS = 3` 个窗口就不再取。`text` 是各窗口按原文顺序拼接、窗口之间插入固定分隔串 `PREVIEW_GAP = "\n⋯\n"`（5 个字符全是码元 1，不参与命中匹配）；`ranges` 相对拼出来的 `text` 计算，按构造永不跨过分隔串。`truncated` 的判据见第三节。
6. 纯文件名命中（`snippet` 不带 `[ ]` 的那类，见 `index_store.rs:210-213`）在原文里可能一处都匹配不上：`ranges` 为空是合法结果，界面显示原文窗口 + 「正文里没有这个词，命中的是文件名」。

**panic 边界这件事必须说全，因为它是一张跨模块的网**：M3 的 `panic_to_err` 在 `index_job.rs:146`，是私有函数，只守索引 pass 那一条路。预览是在 IPC 命令线程上跑抽取的，那条路上今天**一道边界都没有**——一份畸形 docx 被用户在首屏点开就当场终止应用进程（`[profile.release]` 的 `panic = "abort"` 已在终审 C1 里去掉，所以 panic 会展开、抓得住，但得有人抓）。本轮把 `panic_to_err` 提到 `extract.rs` 做成 `pub(crate)`，索引与预览两条路共用同一个实现，`index_job.rs` 改成引用它。这条是一个跨模块不变量：**任何按 path 调用抽取的地方都必须包边界**，实现时要在 `extract_text` 的 doc 注释上写明，否则下一个加读盘通路的人又要重新发现一遍。

另外，因为 D4 把 `path` 从入参里拿掉了，M3 的 m5（路径未归一化）在本轮**不再是安全前提**，它退回「同一目录的另一种写法会落成两行」的观感/去重问题，仍归 M5。

## 六、模块与文件切分

- `src-tauri/src/search.rs` —— 新增 `unified_bundle()` 与四个 DTO（`SearchBundle` / `ClusterSection<C>` / `LedgerCluster` / `DocCluster`；`DocPreview` 归 `doc_preview.rs`）；分组、聚簇、截断都在这里。不新建 `unified.rs`：架构图 §三 里 `search` 那一格写的就是「jieba 分词 → FTS5 查询 → 结果分组」，分组本来就是它的职责。
- `src-tauri/src/doc_preview.rs` —— 新文件，只放预览这一个能力。它自己**不含任何 `File::open`**，只调 `extract::extract_text`，所以「索引与检索对磁盘的唯一动作是读」这条红线仍可只 grep `extract.rs`（今天只有 `:44` 与 `:130` 两处）。
- `src-tauri/src/extract.rs` —— 收 `panic_to_err`（`pub(crate)`）。
- `src-tauri/src/index_job.rs` —— 删私有 `panic_to_err`，改引 `extract::panic_to_err`；`run_pass` 的调用点形状不变。
- `src-tauri/src/tokenize.rs` —— `clean_snippet`（`:79`）增加一条「折叠紧邻重复词」；新增 `pub fn query_terms()`，`query_expression`（`:42`）改为调它（第五节第 4 条）；`MAX_QUERY_CHARS` 的家（第四节）。
- `src-tauri/src/lib.rs` —— 两条命令 + `generate_handler!`。
- `src/lib/api.ts` —— `searchAll`、`docPreview` 两个封装。
- `src/types/search.ts` —— `SearchBundle` / `ClusterSection<T>` / `LedgerCluster` / `DocCluster` / `DocPreview`（`ranges` 写成 `[number, number][]`）；`SOURCE_ORDER` 的六段常量保留（`field_hits` 仍在用），段标题改在 `search-bundle.tsx` 里定义。
- `src/stores/local-search.ts` —— `hits: FieldHit[]` 换成 `bundle: SearchBundle | null`，**保留现有那个自增 `seq` 守卫**（连打两个字时旧结果不许盖新结果）。
- `src/pages/search.tsx` —— 只剩输入框、debounce（`DEBOUNCE_MS = 250` 不变）、store 接线、三种空态。
- `src/components/search-bundle.tsx`（新）—— 三段 + 簇 + 「还有 N 条」。台账段改成按项目聚簇后，`source` 失去了原来「段标题」的位置，但不许因此消失：每条命中行左端渲染 `SOURCE_LABELS[h.source]` 作小徽章（`ui/badge.tsx` 已有），这样 `SOURCE_LABELS` 仍有唯一消费方，用户也仍看得出这条是环境地址还是备注。
- `src/components/doc-preview-dialog.tsx`（新）—— 用已有 `ui/dialog.tsx`；`ui/` 下没有 sheet/tabs，不为这个需求装。
- 样式一律 Tailwind 工具类；确实要独立样式文件时用 `.scss`，不新增裸 `.css`。

## 七、错误处理

错误码沿用 `AppError { code, message, hint }` 与前端 `toAppError`：

| code | 触发 | message 口径 |
|---|---|---|
| `invalid_input` | 查询超长（> `MAX_QUERY_CHARS`） | 与今天两条通路一致的文案，只把 128 这个数引自共享常量 |
| `db_failed` | 任何 rusqlite 错误（走 `error.rs:42` 的 `From`） | 「数据库操作失败：{原生错误}」，hint 指向备份恢复 |
| `preview_unavailable` | 行不存在，或 `index_status != 'ok'` | 「这条索引记录不能预览」+ hint 说明可能是已注销根目录/重建过 |
| `preview_file_missing` | `extract_text` 回 `io` 类错误（文件改名/删除） | 「文件已不可达」+ hint 指向 `/index` 重建（真正的失效标记归 M5 reconcile） |
| `extract_failed` | 抽取失败，含 `panic_to_err` 兜住的 panic | 「这一份抽取不出来，列表里的摘要仍可用」 |

原则：**预览失败只让对话框变红，绝不让整段结果消失**。搜索本身失败才走现有的页面顶部 error 行。

## 八、测试策略

延续 M3 的纪律：**每条测试都要能被它所守护的那个表达式打红**，写测试前先在被守护表达式上做一次反值心算，避免恒绿断言。

Rust 单测（内存库 `db::open_in_memory()` + 夹具只在 `tempfile::tempdir()`）：

- 分组归属：`source == "project"` 只进 `projects`，其余五个 source 只进 `ledger`。变异：把两个 `push` 的段写反 ⇒ 红。
- 段内聚簇与簇序：**必须带阳性对照**——两个项目命中数相同、项目名决定先后；再加一组命中数不同的。否则「并列按项目名升序」这条是恒真的。
- 截断与三个计数，**分三个夹具**（正文段一次只取 200 条，一个夹具凑不出「21 簇 × 每簇 12 条」= 252 行）：① 簇数截断 = 21 个项目各 1 条正文命中，断 `docs.clusters.len() == 20` 且 `hidden_clusters == 1`；② 簇内截断 = 1 个项目 12 条命中，断 `items.len() == 10` 且 `hidden == 2`；③ 平铺截断 = 15 条 `source == "project"` 命中，断 `projects.len() == 10` 且 `projects_hidden == 5`。变异：把任一个计数写成 `0` ⇒ 红。这三个数是「不静默丢弃」的唯一证据，缺一个断言就等于那一级的截断没人守。
- 空查询与超长查询都不加新守卫：`search_all` 不加第三道空判、也不加第四道长度校验，`field_hits`（`search.rs:87`）与 `doc_hits`（`index_store.rs:262`）的既有分支各自负责，本轮只把两处 `128` 字面量换成共享常量（同调于 `index_store.rs:256` 注释写的「只在边界校验一次」）。两条测试因此只断行为：空查询回 `Ok` 且三段皆空、`relaxed == false`；129 字回 `invalid_input`。变异：在聚合层补一道 `if q.is_empty() { Err(..) }` ⇒ 前者红。
- `doc_preview` 的 ranges：大小写不敏感命中、CJK 无大小写命中、命中落在窗口外、多命中并窗、超过 3 窗 `truncated = true`、纯文件名命中时 `ranges` 为空。偏移单位单独钉两条：**不是字节偏移**（中文构造，字节偏移会翻 3 倍错位）、**不是码点偏移**（命中前放一个 BMP 外字符，UTF-16 码元比 char 多 1；这条是前端 `text.slice` 能对齐的唯一证据）。
- `query_terms` 与 `query_expression` 同源：断 `query_terms("验收，")` 不含纯标点词，且 `query_expression` 拼出的词序与之一致。变异：让 `query_expression` 保留自己那份 `cut` 而不引 `query_terms` ⇒ 两条切词从此可以各改各的，这条测试是唯一的报警。
- `doc_preview` 的三条失败态各一条，断 `code` 而不是断 message（防文案漂）。
- 预览的 panic 边界：把 `doc_preview` 内部拆成接收抽取器参数的形式（生产传 `extract::extract_text`，测试传 `|_| panic!()`），断回 `extract_failed` 而不是进程终止。这是 M3 的 I2 教训——中间层没有测试等于没守。参数化协作者不是测试专用注入缝。
- `clean_snippet` 折叠重复词：正向「付款条件付款条件」→「付款条件」；反向「数据 数据库」这类**不是紧邻同词**的输入不许被折。

前端 vitest：store 的 `seq` 时序（慢的旧请求不许盖新的 bundle）、三段各自空态、三个截断计数各自渲染成「还有 N 条 / 还有 N 个项目未显示」（只显不展）、`relaxed` 只在正文段出现一次说明。

`/index` 试验台不新增测试口径，但 `clean_snippet` 变了要确认它现有断言不因此漂。

## 九、真机验收与性能实测

沿用 M3 全部红线：真实数据目录 `%APPDATA%\dev.zero.pfm\` 只读、任何真机验证用一次性 identifier `dev.zero.pfm.m4test` 且跑完删净；子代理一律不跑 `tauri dev` / `npm run dev`（例外由控制方授予）；验证脚本只回断言结果（布尔/条数/`matchedBy`/文案前缀/尺寸），**绝不打印抽出的正文或任何解密内容**；密文列不参与任何检索。

静态看不见、只能真机证的三格：

1. 对话框里三个动作真的唤起系统程序 / 资源管理器 / 原生剪贴板（`opener` 的 `{path:["**"]}` scope 与 clipboard 焦点问题都是 M1 踩过的）。
2. `±4000` 字符窗口与高亮在真实中文正文上的观感（`snippet` 的重复词折叠是否真的生效）。
3. **性能实测（D6 的前置）**：往沙盒灌 5 万行量级的 `index_docs`（复用 M3 那批自造 `.txt` 的手法），量 `search_all` 从键入停止到回包的往返耗时若干次取分布；同时量一次「台账段 LIKE 全扫」与「正文段 FTS」各自的占比。数字写进 `docs/开发进度.md` 的验收证据。若 LIKE 那半段明显拖住首屏，再回来决定 D6 要不要拆段——**拆不拆由这次实测说话，不由推测说话**。

## 十、不在本轮

M3 划给 M4 的账，逐项写明为什么不进来：

| 项 | 内容 | 归属 | 为什么不在本轮 |
|---|---|---|---|
| m1 | `max_depth(16)` 之外的文件静默消失、无信号 | M5 | 症状是漏召回不是错召回，与首屏无耦合 |
| m2 另一半 | `ON DELETE CASCADE` + FTS5 不参与级联 | M5（加硬删之前必须做） | 今天全仓无 `DELETE FROM projects`，不可达 |
| m3 | `pending` 状态无生产写入点 | M5 分批续建 | 预留语义 |
| m4 | 增量判据同秒同尺寸会跳过 | M5 | 真实命中率极低 |
| m5 | 路径未归一化 / `UNIQUE` 是 BINARY 比较 | M5 | D4 之后不再是安全前提，退回观感与去重 |
| m6 | 两个 start handler 的重复 5 行；逐项目启动无 UI 入口 | M5 | 与首屏无关 |
| m7 | `void cancel()` 未接 catch | M5 | 后端 `index_cancel` 只 `store(true)`，实际不会失败 |
| m8 | `/index` 的 `scanning` 文案对根缺失/空项目同一句 | M5 | 本轮只在**首屏**空态用 `indexed_projects` 解决同类问题，`/index` 页那处不动 |
| (c) | `toAppError` 把原生 `TypeError` 渲染成 `{}` | M5（全仓债务，6 个 store 共享） | M4 不加重它 |
| 终审 m2/残留 11 | `set_root_dir` 的 `DELETE`+`INSERT` 未包事务，可与轮末 sweep 撞 | M5（与 m5 同一簇） | 后果是整项目重抽，可自愈，不是损坏 |
| sweep 可观测 | 回收条数不进摘要，界面看不见 | M5 | 加字段要涟漪 `types`/store/页面/IPC 契约 |
| 复审 m-3~m-6 | 边界计数口径措辞、`emit` 自身 panic 是否重复终态、外层 `report_failure` 无再上兜底、裸字符串比较判改指 | M5 / 待真机 | 见 `docs/开发进度.md` 已知缺口 7–10 |

本轮自己新增、且明确推后的只有一件：`search_all` 的 LIKE 段若实测拖慢首屏，拆成分段渲染（D6）。

另外记一件本轮**故意不做**的：三个截断计数只报数、不给「展开看剩余」的按钮。要做就得给 `search_all` 加分页入参，而分页与「段内按项目聚簇」相互纠缠（第 21 簇的边界会随 offset 漂），本轮的三个数已足够回答「结果为什么不全」。

## 十一、风险

1. `bm25` 与 LIKE 的召回口径不同，三段并列会让用户以为「排在前面的段更相关」——段序固定为「项目档案 / 台账条目 / 文件正文」并在页面上写一句「三类结果各自排序，不代表相关度高低」。
2. `clean_snippet` 折叠紧邻重复词有误折叠风险，阳性/反向两条测试必须同时存在，且只折**完全相同**的相邻词。
3. 预览按行 id 取，意味着「已注销根目录但行还在」的那批文件仍可能被预览到——M3 的轮末 `sweep_unrooted_projects` 会在本轮或下一轮末尾把它们清掉，清掉后 `doc_id` 就查不出行了。窗口期内可预览是已知且可接受的。
4. 5 万行量级的 LIKE 全表扫是首屏新引入的每键开销，debounce 250 ms 挡不住一次全量扫描；第九节的实测就是为这一条准备的，实测不过关就要改方案。
