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
4. 预览里的三个动作：在资源管理器中选中（`revealItemInDir`）/ 复制完整路径 / 跳项目详情页（**刻意不含「用外部程序打开文件」**——`openPath` 会把文件交给外部应用，而那个应用随时可以保存回写，这条工具的立身边界是「只读源文件」；取舍写进 `doc-preview-dialog.tsx` 的注释。原稿这一条写的是「打开文件 / 打开所在目录 / 复制路径」，2026-10-09 整分支终审核出规格与代码不一致，按代码改）

**边界**：M3 划给 M4 的 12 项账，本轮实际拉进来的只有一件，而且不是清账、是避免制造账——见第五节末尾。其余 11 项列在第十节，逐项写明归属与为什么。

## 二、决策与理由

| # | 决定 | 被否掉的替代与为什么 |
|---|---|---|
| D1 | 合并与聚合放 **Rust 侧**（新增 `search_all` 命令，内部同时调 `search::field_hits` 与 `index_store::doc_hits`） | 前端并发两条自己拼：改动小，但「同项目相邻 + 每段取前 N」的口径只活在 ts 里，进不了 `cargo test` 的门禁网，而两边各自的 50 条上限叠起来后总数不可控。改造 `search_local` 返回全部：`/index` 试验台也在用 `search_docs`，改完两边语义分家，且 `search.rs` 要 import `index_store`，模块边界变浑。 |
| D2 | 三段骨架，「项目」段的语义钉死为**项目档案本身的命中**（`source == "project"`），不是聚合轴；台账段与正文段各自段内按 `project_id` 聚簇，**不做跨段的项目合并** | 跨段合并会让同一项目的台账行与正文行比邻，而 `bm25` 与 LIKE 命中根本没有可比分数——§3.4 明令「不做假的归一化」。七段平铺（六段台账 + 正文）：改动最小，但命中跨 8 个项目时同一个东西的两类结果隔三屏远，「在哪个项目里」要用户自己拼。 |
| D3 | 簇间排序**不用分数**：按该簇命中条数降序、并列按项目名升序；簇内保持来源自身的序（台账按现有 SQL 序、正文按 `bm25` 升序） | 按「簇内最强分数」排需要跨域可比分数，即 D2 否掉的那件事。 |
| D4 | 预览入参**只有 `doc_id`**，`path` 由后端从 `index_docs` 取，且要求 `index_status = 'ok'` 才读盘 | 按 `path` 取 + 后端前缀校验：好处是未入库的文本也能点开看，代价是整条通路的安全全押在一条 `substr` 比较上，而 Windows 的盘符大小写 / 结尾分隔符 / 8.3 短名 / `\\?\` 前缀 / junction 全是绕过面，会把 M3 的 m5（路径从未归一化）从 Minor 升级成本轮必修 + 一次 schema 改动。按行 id 取，路径权威就收在库内，前端根本拿不到路径参数。 |
| D5 | 列表摘要继续用 `snippet()`，观感问题（`cut_for_search` 复合词重复两次）在**展示清洗层**解决，规则写成 `A B AB` 三连折叠 | 折「相邻同词」：那正是原文真重复（「数据 数据」）的形态，折了等于篡改用户正文。加 `UNINDEXED` 正文列取原文摘要：要进 V5 迁移，而旧行的正文列只能重抽才能填上 ⇒ 所有已建索引一次全量重建（真实库是几十 GB 重扫），为一个观感问题付这个代价不成比例。接受现状不处理：用户会当成 bug。 |
| D6（控制方代定，可推翻） | `search_all` 一次往返跑完 6 段 LIKE + 1 条 FTS，**不**提前拆「正文先回、台账后回」的分段渲染 | 拆段多一条命令、多一处时序，而 spec §十-1 的「性能数字未做运行时验证」这笔账要在真机沙盒里量一次才有资格决定怎么拆。量法见第九节。 |
| D7（控制方代定，可推翻） | `matchedBy` 不逐条标 `exact`，只给 `prefix` 的行一个小徽章，并在该段**第一次**出现放宽命中时补一行说明 | 逐条标 `exact` 是给绝大多数结果加噪音；完全不标则违反 §6.1「噪音由标注暴露给前端」。 |

## 三、契约

Rust 侧新增（`search.rs`）与前端 `src/types/search.ts` 逐字段对应，serde 一律 `camelCase`：

```rust
#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct SearchBundle {
    pub query: String,
    pub projects: Vec<FieldHit>,      // source == "project" 的命中，最多 15 条
    /// 被 `MAX_PROJECT_HITS` 截掉的条数。「项目」段是平铺列表，没有簇可挂计数。
    pub projects_hidden: usize,
    pub ledger: ClusterSection<Cluster<FieldHit>>,
    pub docs: ClusterSection<Cluster<DocHit>>,
    pub relaxed: bool,                // 正文段里存在 matchedBy == "prefix" ⇒ 段级说明只出现一次
    /// COUNT(DISTINCT d.project_id)，谓词必须与 `index_store.rs` 的 SELECT_SQL 同口径：
    /// `d.index_status = 'ok' AND p.deleted_at IS NULL`。少半个谓词就会出现「文案说正文有索引、
    /// 正文段却是空的」这种自相矛盾的空态。
    pub indexed_projects: usize,
}

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct ClusterSection<C> { pub clusters: Vec<C>, pub hidden_clusters: usize }

/// 两段的簇形状除 `items` 的元素类型外逐字段相同，因此只写一个泛型结构，不写两个同构结构。
/// 泛型不影响线格式：serde 按 `T` 的具体实例化生成序列化，`rename_all` 照样生效。
#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct Cluster<T> { pub project_id: String, pub project_name: String,
                        pub items: Vec<T>, pub hidden: usize }

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

- 取数：`field_hits` 沿用既有 `PER_GROUP_LIMIT = 50`（`search.rs:16`，不动）；`doc_hits` 取 200，与 `search_docs` 现有 clamp 上界一致（`lib.rs` 的 `fn search_docs` 命令体内那个 `clamp(1, 200)`），不新造数字。**10-09 终审订正**：本行原写「`lib.rs:542` 的 `clamp(1, 200)`」，那个行号今天已是空行（clamp 在它下面几行）⇒ 一律换成符号指针，理由见 `docs/HANDOFF.md` 坑 86 同族的行号指针漂移（本仓已把内部行号指针统一改为符号指针）。
- 展示：`MAX_CLUSTERS_PER_SECTION = 30`、`MAX_ITEMS_PER_CLUSTER = 15`、`MAX_PROJECT_HITS = 15`（**2026-10-09 按真机 5 万行实测从 20/10/10 上调**；读数、对照探针与裁定口径记录在计划 Task 9 Step 3，本轮实测的原始 JSON 落 `.superpowers/sdd/2026-10-07-m4-unified-search/t9-d6.json`）。三处截断各有承载，漏一个就等于把结果悄悄扔掉：簇内被截掉的条数进该簇 `hidden`；整簇被段级上限扔掉的簇数进 `ClusterSection.hidden_clusters`；「项目」段是平铺列表没有簇可挂，截掉的条数进 `projects_hidden`。三个数都只显示、**不做展开** —— 被截掉的行压根没进 bundle，要真展开得再发一次带分页的 IPC，本轮不做（第十节）。**上调不改变这条边界，也不把 `hidden` 的语义放宽**：实测里最坏一簇 `hidden = 190`，15 条仍然触顶 ⇒ 真正解得了它的是簇内分页/展开那次 deferred IPC，不是继续调大数字。
- 查询守卫：`MAX_QUERY_CHARS = 128`。今天这个数在 `search.rs:87` 与 `index_store.rs:262` 各写了一遍字面量 128，本轮提到一处共享常量。落点选 `tokenize.rs`（查询侧模块，`index_store` 本来就引它），而不是 `search.rs` —— 后者要新造一条 `index_store → search` 的反向依赖，只为搬一个数。

## 五、预览通路

1. 按 `doc_id` 取 `index_docs` 行（`SELECT d.path, d.index_status, p.name ... JOIN projects p ON p.id = d.project_id`，带上 `p.deleted_at IS NULL`），行不存在或项目已软删 ⇒ `preview_unavailable`；`index_status != 'ok'` ⇒ `preview_unavailable`（`skipped`/`failed` 的行本来就没有正文可抽，不能假装能读）。软删的那半是防御：预览的入参只有 id，靠这一条把首屏之外的旧 id 也关在索引作用域里。
2. `index_docs` 里没有任何列指向登记动作，所以**不试图反查根目录归属**：能被读的文件恰好等于「曾被索引为 `ok` 的行」，这个集合就是索引作用域，D4 已经把它关住了。
3. 抽取调 `extract::extract_text(path)`（同一条扩展名分派表、同一套 `chardetng` + `encoding_rs`）。这一步**必须包 panic 边界**：见下面的小节。
4. 命中位置在**原文**上算。词只从一个地方拿：`tokenize.rs` 新增 `pub fn query_terms(query: &str) -> Vec<String>`，把现在写在 `query_expression`（`:42`）里的 `jieba().cut(q, true)` + 同一个 `is_alphanumeric` 过滤抽出来，`query_expression` 改调它再拼 FTS 语法。这样「检索命中的词」与「预览高亮的词」天然是同一套切词，符合 spec 的入库/查询同词器红线；若预览另起一次 `cut`，两处口径会随词典版本漂。匹配是大小写不敏感的子串匹配，规则写成一句可实现的：两侧各按 `char` 逐个取 `to_lowercase()` 的**第一个**字符再比，因而偏移永不因折叠而变长（`İ` 这类会展开成两字符的映射按首字符比，接受它匹配不到）。中文没有大小写，走同一条代码路径。
5. 窗口化：`PREVIEW_WINDOW_CHARS = 4000` 是**半宽**（按 char 计，不要求落在词或行的边界上），所以以命中为中心的窗是 `[start-4000, end+4000]`、**没被并过的**单窗最长 `2 * 4000 + 命中自身长度` 个 char；并窗的判据是**新窗左沿** `start-4000` 落进已有窗口（含刚好相接）就并进去（M4 Task 5 复审 Important-1：本句原先写的是「后续命中落进已有窗口」，按字面拿命中自身的 `start` 去比上一扇右沿，会在两处命中相距 4000–8000 char 时既不并窗、新窗左沿又越过上一扇右沿 → 同一段正文被拼两遍，并在其间插入一个代表「此处有省略」的分隔串，而那段一字未省——与同节「`ranges` 按构造永不跨过分隔串」这条不变量互相矛盾，所以以不变量为准）。超出 `MAX_PREVIEW_WINDOWS = 3` 个**窗**就不再取更多命中；上限封顶的是窗数不是窗长，链式合并会让一扇窗变长，长文里可以接近整篇，这一点由 `truncated` 说实话。`text` 是各窗口按原文顺序拼接、窗口之间插入固定分隔串 `PREVIEW_GAP = "\n⋯\n"`（**3 个字符**，换行与 `⋯` 各占 1 个码元，不参与命中匹配）；`ranges` 相对拼出来的 `text` 计算，按构造永不跨过分隔串。`truncated` 的判据见第三节。
6. 纯文件名命中（`snippet` 不带 `[ ]` 的那类，见 `index_store.rs:210-213`）在原文里可能一处都匹配不上：`ranges` 为空是合法结果，界面显示原文窗口 + 「正文里没有这个词，命中的是文件名」。

**panic 边界这件事必须说全，因为它是一张跨模块的网**：M3 的 `panic_to_err` 在 `index_job.rs:146`，是私有函数，只守索引 pass 那一条路。预览是在 IPC 命令线程上跑抽取的，那条路上今天**一道边界都没有**——一份畸形 docx 被用户在首屏点开就当场终止应用进程（`[profile.release]` 的 `panic = "abort"` 已在终审 C1 里去掉，所以 panic 会展开、抓得住，但得有人抓）。本轮把 `panic_to_err` 提到 `extract.rs` 做成 `pub(crate)`，索引与预览两条路共用同一个实现，`index_job.rs` 改成引用它。这条是一个跨模块不变量：**任何按 path 调用抽取的地方都必须包边界**，实现时要在 `extract_text` 的 doc 注释上写明，否则下一个加读盘通路的人又要重新发现一遍。

另外，因为 D4 把 `path` 从入参里拿掉了，M3 的 m5（路径未归一化）在本轮**不再是安全前提**，它退回「同一目录的另一种写法会落成两行」的观感/去重问题，仍归 M5。

**线程口径：光出锁不够，`doc_preview` 必须换线程跑（Task 6 首轮复审 Important-1 的裁定）**。上面第 1–5 条描述的是一条会做无上限磁盘 IO + pdf/docx 抽取的通路，而 Tauri 的命令默认 `execution_context = Blocking`：Windows/WebView2 上 `ipc_handler` 是在 `add_WebMessageReceived` 的 COM 回调里**同步调用**的，所以 sync 形态的命令体占的就是消息泵那一个线程 —— 期间没有任何别的 `invoke` 进得来，「锁外抽盘」买到的东西被 sync 形态原地退掉。因此本节的契约是：`doc_preview` 命令带 `#[tauri::command(async)]`（命令体落进 `async_runtime::spawn` 的 future，跑在 tokio **多线程 runtime 的 worker** 上 —— 2026-10-09 整分支终审在 `tauri-macros-2.7.0/src/command/wrapper.rs` 亲验：`respond_async_serialized(async move { let result = $path(...) })`，`$path` 确实在 `async move` 块内；但 `command_wrapper` 的 Async 分支**不看** `asyncness`，那个 `kind = "sync_threadpool"` 只是 tracing span 的一个字段、全文件没有一次 `spawn_blocking` ⇒ 早期稿子把「标签」当成了「机制」，现按实测改写），`search_all` 本轮保持 sync（它只做 SQL 且取数有 200 行上限，是否也要换线程由第九节的实测说话，与其它上限的裁定同一条口径：由数字说话不由推测说话）。两条推论都必须在实现里成文：① 出锁从此**真的承重** —— 全仓那把 `Mutex<Connection>` 过去只有 IPC 线程取（后台索引线程自己开连接），有了第二条线程之后，「锁内查库、锁外抽盘」才是防住并发命令排队在慢 IO 上的那道闸；② `async` 让同类命令**可能乱序返回**（sync 形态下由消息泵天然串行，这在以前不可能发生），所以第六节 `src/stores/local-search.ts` 那条自增 `seq` 守卫（以及第八节点名的 `search-order.ts::isNewest`）从防御性写法升为承重设计，前端不许把它当可选优化省掉。

## 六、模块与文件切分

- `src-tauri/src/search.rs` —— 新增 `unified_bundle()` 与三个 DTO（`SearchBundle` / `ClusterSection<C>` / `Cluster<T>`；`DocPreview` 归 `doc_preview.rs`，`DocHit`/`FieldHit` 复用既有）；分组、聚簇、截断都在这里。不新建 `unified.rs`：架构图 §三 里 `search` 那一格写的就是「jieba 分词 → FTS5 查询 → 结果分组」，分组本来就是它的职责。
- `src-tauri/src/doc_preview.rs` —— 新文件，只放预览这一个能力。它自己**不含任何 `File::open`**，只调 `extract::extract_text`，所以「索引与检索对磁盘的唯一动作是读」这条红线仍可只 grep `extract.rs`（今天只有 `:44` 与 `:130` 两处）。
- `src-tauri/src/extract.rs` —— 收 `panic_to_err`（`pub(crate)`）。
- `src-tauri/src/index_job.rs` —— 删私有 `panic_to_err`，改引 `extract::panic_to_err`；`run_pass` 的调用点形状不变。
- `src-tauri/src/tokenize.rs` —— `clean_snippet`（`:79`）增加一条 **`A B AB` 三连折叠**：连续三个 token 满足「第三个恰等于前两个按原序拼接」时丢掉前两个子词，保留复合词；这三个 token 任一携带 `[` 或 `]` 高亮标记时整组不折（折叠会吃掉标记，而标记是 §6.1 要求暴露给前端的东西）。新增 `pub fn query_terms()`，`query_expression`（`:42`）改为调它（第五节第 4 条）；`MAX_QUERY_CHARS` 的家（第四节）。
- `src-tauri/src/lib.rs` —— 两条命令 + `generate_handler!`。
- `src/lib/api.ts` —— `searchAll`、`docPreview` 两个封装。
- `src/types/search.ts` —— `SearchBundle` / `ClusterSection<T>` / `Cluster<T>` / `DocPreview`（`ranges` 写成 `[number, number][]`）；`SOURCE_ORDER` 的六段常量保留（`field_hits` 仍在用），段标题改在 `search-bundle.tsx` 里定义。
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
| `preview_file_missing` | `extract_text` 回的是 `AppError::io` 那条路（`error.rs:21`，码为 **`fs_failed`**），即文件改名/删除/不可读 | `doc_preview` **不原样透出 `fs_failed`**：按 `err.code == "fs_failed"` 翻译成这条码，message「文件已不可达」+ hint 指向 `/index` 重建（真正的失效标记归 M5 reconcile）。这一层翻译是契约的一部分，前端只认 `preview_*` 两个码，不必知道 `extract` 内部的 io 码名。 |
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
- `query_terms` 的过滤行为：断 `query_terms("验收，")` 不含纯标点词、词序与 `cut` 返回序一致。**「两处同源」不写行为测试**——把 `query_expression` 里的 `cut` 复制一份进 `query_terms`，任何行为断言都照样绿，这条断言不可证伪；同源改由 grep 门禁守：`tokenize.rs` 内 `.cut(` 恰好出现 1 次（`index_text` 用的是 `.cut_for_search(`，与该模式不重叠）。谁把 `query_expression` 改回自带一份 `cut`，门禁就红。
- `clean_snippet` 的三连折叠，三条一起存在才算守住：① 正向 `付款 条件 付款条件` → `付款条件`；② 反向 `数据 数据库` 不满足「第三个 = 前两个拼接」，一个字都不许动；③ 括号守卫 `[付款] 条件 付款条件` 不折（原样返回）。反向那条是防篡改的：真重复的正文（`甲方 甲方`）不属于任何一条规则，必须原样输出。
- `doc_preview` 的三条失败态各一条，断 `code` 而不是断 message（防文案漂）。
- 预览的 panic 边界：把 `doc_preview` 内部拆成接收抽取器参数的形式（生产传 `extract::extract_text`，测试传 `|_| panic!()`），断回 `extract_failed` 而不是进程终止。这是 M3 的 I2 教训——中间层没有测试等于没守。参数化协作者不是测试专用注入缝。
- `clean_snippet` 折叠重复词：正向「付款条件付款条件」→「付款条件」；反向「数据 数据库」这类**不是紧邻同词**的输入不许被折。

前端断言用 **`node --test`**（仓库自带 Node 就有 test runner，`.ts` 由类型剥离直跑，控制方实测 `2 pass / exit 0`；`tsconfig.json` 已同时开着 `allowImportingTsExtensions` 与 `noEmit`，带 `.ts` 后缀的 import 不会被 tsc 判错），**且只测抽成纯函数的逻辑**：`src/lib/search-order.ts` 运行时无任何 value import（`import type` 会被擦除），承担 seq 守卫（慢的旧请求不许盖新 bundle）、三个截断计数各自产出「还有 N 条 / 还有 N 个项目未显示」、`relaxed` 只产出一条段级说明、某段为空则不产出该段。**JSX 本身不在这张网里** —— 渲染正确性由 `npm run build` 的 tsc 段加第九节真机点验负责，这一格在第十节点名。测试文件落仓库根 `tests/`（**在 `src/` 之外**）：`tsconfig.json` 的 `include` 只有 `["src"]`，而 devDependencies 没有 `@types/node`，一个 import `"node:test"` 的文件放进 `src/` 会让 `npm run build` 因解析不到该模块类型而红——测试文件本身不该是构建的依赖。运行命令固定为 `node --test "tests/**/*.test.ts"` 并断言确切的 `pass` 条数：实测「glob 没匹配到任何文件」时 `node --test` 回 **exit 0 / tests 0**，只看退出码等于没有门禁。

> 本节初稿写的是「前端 vitest」。核对仓库后发现 M0→M3 从来没有测试运行器（`package.json` 无 test 脚本、`package-lock.json` 里 vitest 出现 0 次、`src/` 下无 `*.test.*`），为一个里程碑的装配判断新装 vitest + jsdom + @testing-library 不划算。2026-10-07 由需求方选定改走 `node --test` + 纯逻辑抽取，不新增依赖。

`/index` 试验台不新增测试口径，但 `clean_snippet` 变了要确认它现有断言不因此漂。

## 九、真机验收与性能实测

沿用 M3 全部红线：真实数据目录 `%APPDATA%\dev.zero.pfm\` 只读、任何真机验证用一次性 identifier `dev.zero.pfm.m4test` 且跑完删净；子代理一律不跑 `tauri dev` / `npm run dev`（例外由控制方授予）；验证脚本只回断言结果（布尔/条数/`matchedBy`/文案前缀/尺寸），**绝不打印抽出的正文或任何解密内容**；密文列不参与任何检索。

静态看不见、只能真机证的四格：

1. 对话框里三个动作真的唤起资源管理器 / 原生剪贴板 / 路由跳转（`opener` 的 `{path:["**"]}` scope 与 clipboard 焦点问题都是 M1 踩过的）。**2026-10-09 需求方亲手点完的读数**：目录选择框弹出且能选中、`revealItemInDir` 真的弹出资源管理器并高亮在该文件上、`window.confirm` 取消拦得住且确认能真删掉；原稿这一条里的「唤起系统程序」随 §一.4 一并撤掉（那个动作刻意不做）。
2. `±4000` 字符窗口与高亮在真实中文正文上的观感（`snippet` 的重复词折叠是否真的生效）。**含 Task 5 复审登记的一条有名缺口**：并窗判据取「新窗左沿」后合并带翻倍（见第五节第 5 条），`MAX_PREVIEW_WINDOWS` 只封顶窗数不封顶窗长，命中每隔不到两个半宽出现一次时长文的 `text` 会接近整篇 —— 载荷上界没了这件事是裁度过、可接受的，但「对话框还读不读得动」只有真机看得见，所以真机侧灌一份命中间隔约 8000 char 的长文，只回 `text` 码元长度、`ranges.len()`、`truncated` 三个数字，明显卡顿才升级为终审 finding。
3. **性能实测（D6 的前置）**：往沙盒灌 5 万行量级的 `index_docs`（复用 M3 那批自造 `.txt` 的手法），量 `search_all` 从键入停止到回包的往返耗时若干次取分布；同时量一次「台账段 LIKE 全扫」与「正文段 FTS」各自的占比。数字写进 `docs/开发进度.md` 的验收证据。若 LIKE 那半段明显拖住首屏，再回来决定 D6 要不要拆段——**拆不拆由这次实测说话，不由推测说话**。
4. **`async` 到底有没有兑现（第五节那条线程口径的唯一运行时证据）**：单测里证明不了「不占消息泵」这件事，134 条 Rust 测试一条都不覆盖它。做法是在沙盒 tempdir 造一份慢到可辨的文件（几百 MB 纯文本或多页 PDF，只读、跑完删净），在页面里同一时刻并发发 `invoke("doc_preview", {慢文件})` 与一条便宜的 `invoke("db_status")`，只回两个毫秒数和「`db_status` 是否先返回」。`db_status` 先回 = 属性生效；它被推到 `doc_preview` 之后 = 属性没起作用，升级为终审的独立 finding；**任一条始终不返回同样是结论，不许当作「没跑出来」跳过** —— async 形态下命令体里的 panic 既不终止进程也不提示界面，而是让那一次 `invoke` 永不落地，这一格正好是「抽取必须包在 panic 边界里」那条红线的唯一真机反证。同一口径顺手量 `search_all`：5 万行那次往返期间并发 `db_status`，若 `search_all` 已 > 800 ms 且把 `db_status` 顶到后面，就在 `docs/开发进度.md` 写明「首屏检索也在冻消息泵，`search_all` 应一并换 `async`」，交给终审落地 —— 本轮不趁真机顺手改 Rust，免得绕过任务评审这道闸。

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
2. `clean_snippet` 的三连折叠有误折风险，所以规则收窄到「第三个 token 恰等于前两个按原序拼接」，且这三个 token 任一携带高亮标记就整组不折。正向、反向（不满足拼接关系）、括号守卫三条测试必须同时存在，再加一条「真重复正文 `甲方 甲方` 原样输出」兜住篡改风险。
3. 预览按行 id 取，意味着「已注销根目录但行还在」的那批文件仍可能被预览到——M3 的轮末 `sweep_unrooted_projects` 会在本轮或下一轮末尾把它们清掉，清掉后 `doc_id` 就查不出行了。窗口期内可预览是已知且可接受的。
4. 5 万行量级的 LIKE 全表扫是首屏新引入的每键开销，debounce 250 ms 挡不住一次全量扫描；第九节的实测就是为这一条准备的，实测不过关就要改方案。**2026-10-09 实测结论**：稳态 675–737 ms 未越过写死的 800 ms 线 ⇒ 不改方案；但同一轮量到「`search_all` 往返期间 `db_status` 被顶到后面（612 vs 620 ms）」⇒ 这条冻结是真的，只是没到触发线，M5 的优先项是**分段渲染 / 合并那 6 条 LIKE**，不是把 `search_all` 换成 `async`（换了只是把冻结从消息泵搬到 tokio worker 池，并顺带把下面第 6 条的 worker 占用问题引进来，而 `Mutex<Connection>` 那个串行点一点没变）。
5. **预览按 `index_docs.path` 重抽 ⇒ 绕过了全仓唯一那道尺寸门**（2026-10-09 整分支终审 I-4，M4 新开的格子，M3 期这条通路不存在）。`max_file_bytes` 只在 `index_scan` 的扫描循环里判（超限落 `skipped/too_large`，根本不打开文件），而 `doc_preview` 拿登记过的 path 直接 `extract_text`，`extract_text` 的 doc 注释自己写着「不做大小校验（上限归扫描器）」。后果：① 合法 20 MB 文件一次预览会被 `render_preview_with` 放大成约 4 份全量驻留（`raw` String + `hit_ranges` 里的 `lower_first` String + `chars`/`hay` 两份 `Vec<char>`，char 4 字节）⇒ 约 120–140 MB 峰值；② 文件在索引后膨胀 ⇒ 抽取不再受任何上限约束，而分配失败是 **abort**，`panic_to_err` 抓不到、也不在它边界内。归 M5：最小修法是从 `row_for_preview` 带出登记的 `d.size` 或读 settings 的 `index_max_file_bytes` 再决定抽不抽（动的是 Task 5 已定稿的模块形状，故不在本轮）。
6. **预览没有并发/取消闸门**（同上终审 I-5，由第 5 条的实测机制升级而来，不是 UX 抱怨）。真机单次慢预览 257,983 / 266,093 ms，而对话框只有「关闭」——关掉只卸载前端，后端那次抽取照跑完；命令体直接跑在 tokio **多线程** runtime 的 worker 上（本机 12 逻辑核 ⇒ 12 个 worker，全仓无 `spawn_blocking`），所以连点 N 份慢文档 = N 个 worker 被占死，之后**所有** async 命令与插件 async 任务一起排队。归 M5 的最小可用缓解是一个 `AtomicBool` 单飞闸门（约 8 行，占用中回 `preview_unavailable`）。
