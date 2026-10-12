# M6 设计 —— Windows 原生图片 OCR（png/jpg/jpeg/webp/bmp）

依据 `docs/技术方案.md`（M6 = 图片 OCR，§九 line 214、§6.1 line 166「未支持类型不建行」、范围线 line 230、降级风险 line 240）与借鉴库 `ltspace/dowse` 的 OCR 设计（`clean_cjk_spaces()` 只读源码参照、不作依赖）。执行方式仍是 `superpowers:subagent-driven-development`。

- **行号纪律**：本文所有 `文件:行号` 是 2026-10-12 在 `master`（M5.5 已合入，`a8d053b`）的当下读数，**只作对照，取数只认命令**（本仓同形错已记七次，HANDOFF 坑 65/67/73/86/92 + §5 L 组）。落点一律用符号定位：`grep -n "pub enum DocKind\|pub fn kind_of\|pub fn extract_text\|const SUPPORTED\|TEXT_EXTS" src-tauri/src/extract.rs`。
- **一个已消除的头号风险**：技术方案 §240 写「Windows OCR 需系统语言包支持中文，缺失时降级待 M6 验证」。控制方 2026-10-12 在**本机**探测为**真**：`C:\Windows\OCR\` 下有 `zh-cn`，WinRT `OcrEngine::AvailableRecognizerLanguages` 返回 `zh-Hans-CN`。⇒ M6 的 happy-path 在这台机现在就可验（见 §八 与 §九测试策略）；但**别的盒子可能没有**，故降级路径仍必须实现且不可假称已验。

## 一、本轮交付与边界

**交付**：让 `png/jpg/jpeg/webp/bmp` 里的中文/英文文字进全文索引，可被 `search_all` 的正文段命中、可被预览通路展示。一句话——**给图片一种「正文」**。

**五格，只有 `ocr.rs` 是新的，其余是接线**：
1. 新模块 `src-tauri/src/ocr.rs`：能力探测 `ocr_available()`、识别 `image_text()`、清洗 `clean_cjk_spaces()`。
2. `extract.rs`：加 `DocKind::Image` + 分派，`SUPPORTED`/`kind_of`/分派表三处同源。
3. `index_job`/`index_scan`：OCR 不可用时图片**不建行**（等同今天的未支持类型）。
4. `index_overview`：透出**一个** `ocr_available: bool`，`/index` 据此显示降级提示。
5. `Cargo.toml`：新增 `windows`（`Media::Ocr` + `Graphics::Imaging` + `Storage` 相关 features）。

**边界（明确不做，写死）**：
- **`.doc`/`.ppt` 旧格式 = M7**（LibreOffice headless），本批一格不碰。
- OCR 只出**纯文本**：不做版面/表格结构、不做倾斜矫正/旋转、不做多栏重排、不做图片去重。扫描件 PDF 的「抽出为空」语义（M3 既有）与图片 OCR 空文字走同一条 `skipped(empty_text)`。
- **默认全开、无开关**：延续 M5.5「不做 settings 写入口 / 不做可关开关」。要关只能改分派那一行。
- 绝不动被索引的图片文件（红线 1）：只读、`SoftwareBitmap` 解码，不写回、不生成缩略图。
- 单张 ≤20 MB 复用现成 `ScanOptions::max_file_bytes`（`index_scan.rs` 的扫描门），不新增尺寸常量。

## 二、决策与理由

| 编号 | 决策 | 被否掉的备选与理由 |
|---|---|---|
| D1 | OCR **在既有索引轮内、抽取阶段同步做**（`extract_text` 的 `DocKind::Image` arm），与 PDF 同侧、在写锁外 | ① 独立 OCR worker 队列（dowse 形）：要多一套常驻队列 + 新开「OCR worker 写库 vs `index_job` 的 `running` 单飞闸门」这条不变量——**这正是 M5.5 spec §九 刻意推走、没做的「监听线程写索引的新不变量」的同一类**，复杂度明显高一档且刚在 M5 finding 里吃过写锁排队的亏。② 同步但放 `write_doc` 事务内：会把秒级 OCR 塞进写锁持有期，直接恶化 M5 的 `db_failed` 那条。选「抽取阶段同步」是因为 `run_pass` 本就是后台线程、且**抽取在锁外**（`match panic_to_err(\|\| extract_text(..))` 先抽、其 `Ok` arm 才 `write_doc`），OCR 慢只拖长这一轮、不拖长任何单笔锁。需求方 2026-10-12 选定。 |
| D2 | **默认对全部图片开 OCR**，靠「增量（size+mtime 不变不重抽）+ 取消按钮 + 熔断」三件既有兜底控成本 | ① 每项目 OCR 配额（像 `max_files_per_project`）：要多一个上限常量 + 一个新的 `skip_reason` 值，而 M5 D4 明令别乱扩 `skip_reason` 值域（会让界面在同一行多挂红标）。② 默认关、手动开：要新增 IPC/复选框，破「零新命令 / 无设置页」两条既有惯例，范围一下胖。需求方 2026-10-12 选定「默认全开」。代价照实记：照片多的目录**首轮**可能几十分钟（只有首轮，之后增量跳过未变图片）。 |
| D3 | **启动探测一次 OCR 可用性**，不可用则图片整类退回「不建行」+ `/index` 一句提示，**不逐图刷 `failed`** | 需求方 2026-10-12 选定（三选一里的探测式）。被否：① 逐图失败落 `failed`——没语言包时一个相册目录会生成几百条 `failed` 行，把「系统性缺失」伪装成「这些文件各自坏了」，且和 D6「为什么搜不到」的答案污染。② 完全不特殊处理。探测点放 `AppState` 建库时算一次（`ocr_available()`），避免每张图都构造一次 `OcrEngine`。 |
| D4 | **`ocr_available()==false` 时图片等同今天的未支持类型：不建行、不占单项目配额** | 与技术方案 §6.1 line 166「未支持的类型不建行（也不占单项目配额）」一脉相承——今天 png 就是走 `kind_of` 返回 `None` 不建行。M6 只是「有语言包时才把 png 变支持」。判据：不可用 ≠ 抽取失败，是「本机不支持这一类」，所以既不建行也不算 `failed`。 |
| D5 | 抽出为空（装饰图、或识别无文字）走 **`DocOutcome::Ok` 里 `body.trim().is_empty() ⇒ skipped(empty_text)`**；单张识别真报错走 **`failed` + `error_msg`** | 复用 M3 既有两态，不新增状态。扫描件 PDF「抽出为空属正常」（技术方案 line 241）与图片空文字同口径。真失败（解码坏、`SoftwareBitmap` 打不开的损坏图）才 `failed`。 |
| D6 | `index_overview` 加**一个** `ocr_available: bool` 线格式字段 | 本里程碑**没有** M5.5 那条「零新线格式字段」红线（那是逐里程碑的），但**只加这一个、不多铺**：降级提示是 §八 真机要验的、也是 D3 的兑现点，必须到界面。`/index` 页据此在支持清单旁显「图片 OCR：本机未启用（缺中文语言包）」或「已启用（zh-Hans-CN）」。前端 `src/types/index.ts` 的 `IndexOverview` 同步加键（camelCase）。 |
| D7 | `clean_cjk_spaces` 只在**空格两侧都是 CJK 时删该空格**，保留英文词间空格（借 dowse 设计，非依赖） | Windows OCR 会在中文字符间插空格（「合 同 验 收」），不清洗则 jieba 分词后搜「合同验收」搜不到。无脑 `replace(" ","")` 会毁掉中英混排里英文词的分隔 ⇒ 判据精确到「两侧皆 CJK」。纯函数、可单测、无 Windows 依赖。 |

## 三、状态口径与契约（不扩 `index_status` 词汇表；扩 SUPPORTED）

| `index_status` | 图片在 M6 的写入方 | 变化 |
|---|---|---|
| `ok` | OCR 出非空文字 → `write_doc(DocOutcome::Ok(text))` | 图片首次有正文 |
| `skipped` | `empty_text`（OCR 空）；`too_large`（>20MB，扫描门已挡） | 复用，不扩值域 |
| `failed` | `image_text` 返回 Err（解码/识别异常） | 复用 |
| `pending`/`missing` | 不涉及 | 不动（`missing` 仍是 `mark_missing` 单生产写入点） |

**OCR 不可用时**：图片根本不进 `kind_of` 的有效分派（D4），不建行——所以**没有**「ocr_unavailable 的 skip_reason」这种东西（D3 否掉了刷屏，D4 定为不建行），skip_reason 值域一字不扩。

**契约不变量（本批守住 + 新增一条）**：
- 零新 IPC 命令（`generate_handler!` 仍 41；OCR 不需要用户单独触发，它就是索引轮抽取的一步）。
- 新线格式：`IndexOverview` **仅加一个** `ocr_available: bool`；`SearchBundle` 七键、`DocPreview` 六键两条断言**照旧绿不许动**（图片 OCR 出的文字进的是既有 FTS 正文列，不改 bundle/preview 形状）。
- **入库/查询同词器红线（红线 3）不破**：OCR 出的清洗后文本走 `write_doc` → `index_text`（jieba `cut_for_search`）入库，与查询侧 `tokenize::query_terms` 同源。OCR 只是「多了一种拿到正文的途径」，分词链一行不改。
- 迁移：`TARGET_VERSION` 是否 +1 见 §五落点 5——图片不新增列，理论上**零新迁移**；但 `SUPPORTED` 变了不改 schema。裁定：无新迁移。

## 四、判据

- **三处同源不破**（有现成测试 `supported_exts_list_matches_the_dispatch_table` 双向查）：`SUPPORTED` 加了 5 个图片扩展名 ⇔ `kind_of` 能分派到 `DocKind::Image` ⇔ `extract_text` 的 match 有 Image arm 且它下面真的写正文。漏任一处 = UI 承诺了搜不到的类型，或真能抽进索引却不列。
- **`ocr_available()` 探测的口径**：`OcrEngine::AvailableRecognizerLanguages` 非空即算可用（能建出任一识别器）。**不断言必须有中文**——有英文包时英文图也能 OCR，降级判据是「一个识别器都没有」，不是「没有中文」。`/index` 提示文案按实际可用语言念。探测在 setup 建库时做一次、存 `AppState`，`run_pass`/`extract` 读这个布尔，**绝不每张图 `TryCreateFrom...` 探一遍**（构造 OcrEngine 有成本，且探测语义应全局一致，避免同一轮内忽有忽无）。
- **降级不建行要能被验**：真机断言（§八）里，构造「`ocr_available()=false`」这台机做不到（它有中文包），故降级那一支走**代码级判据 + 单测注入**（见 §九），不假装真机验过降级路径。
- **成本可控性**（D2 的代价，要写清不粉饰）：图片 OCR 是**秒级/张**，一个含 N 张图的目录首轮 ≈ N×(秒)。既有三件兜底必须都在：① 增量 `current_rowid`（size+mtime 未变且 ok ⇒ 不重 OCR、连文件都不开）② 取消按钮（`run_pass` 每若干文件查 `cancel`，M3 既有）③ 熔断/root 不可达整项目跳过（M5 既有，护不可达盘）。**不新增限速**（D2 否了配额）；若真机量出某规模首轮久到不可接受，那是 §十风险 2 记的「回头加最小间隔或配额」——本批先取数不预设。
- **抽取在锁外**（承 D1）：`image_text` 只在 `extract_text` 里被调，而 `extract_text` 在 `run_pass` 抽取阶段（写库 `write_doc` 之前）执行 ⇒ OCR 的秒级耗时**不进** SQLite 写锁。这条要能被评审静态核（`image_text` 调用点不在 `write_doc`/事务内）。

## 五、落点（五格）

1. `Cargo.toml`：`windows = { version = "<plan 期 cargo 实取>", features = ["Media_Ocr","Graphics_Imaging","Graphics_Imaging_Codecs","Storage","Storage_Streams","Foundation","Foundation_Collections","Globalization","Win32_System_WinRT"] }`（feature 名以 `windows` 实包为准，计划阶段核对）。技术方案 line 186 预置 `0.62.2`；若版本流动以实锁为准。
2. `src-tauri/src/ocr.rs`（新）：`ocr_available()->bool`、`image_text(&Path)->AppResult<String>`、`clean_cjk_spaces(&str)->String`。`image_text` 内 `SoftwareBitmap::CreateFromFileAsync` → `OcrEngine::TryCreateFromUserProfileLanguages()`（或从 `AvailableRecognizerLanguages` 首个）→ `RecognizeAsync` → 逐 `lines` join `\n` → `clean_cjk_spaces`。WinRT 异步用 `windows` 的 `.get()` 阻塞等（OCR 在后台线程跑，阻塞该线程无妨，它本就在 `run_pass` 线程）。
3. `extract.rs`：`enum DocKind` 加 `Image`；`kind_of` 把 `png|jpg|jpeg|webp|bmp` → `Some(DocKind::Image)`；`SUPPORTED`（`index_store.rs:217` 附近）加这 5 项；`extract_text` 加 `Some(DocKind::Image) => crate::ocr::image_text(path)`。**但**：`extract_text` 拿不到 `ocr_available`（它是纯函数入口）——见落点 4 的注入。
4. **可用性注入点**（关键接线，避免 `extract_text` 反查全局）：`run_pass` 在**每轮开头**读一次 `ocr_available`（从 `AppState`/`IndexShared` 传入，或 `run_pass` 入参多一个 `ocr_available: bool`），在分派前判：`if kind==Image && !ocr_available { 不建行、continue（视同未支持）}`。`extract_text` 保持纯（只在 ocr_available 时才被喂 Image 路径）。⇒ 降级不建行落在 `run_pass` 这一层，与 `kind_of` 的静态支持解耦。
5. `index_overview`（`lib.rs`）：`IndexOverview` 加 `ocr_available: bool`（camelCase 出），值来自 setup 建库时探测一次存 `AppState.ocr_available: bool`；`src/types/index.ts` 的 `IndexOverview` 加键；`index-status.tsx` 支持清单区据它显一句。零新命令、无新列 ⇒ 无迁移。

## 六、错误处理

- `image_text` 内部任一 WinRT 调用 Err（损坏图、解码失败、`RecognizeAsync` 报错）→ 返回 `Err`，`run_pass` 落这一行 `failed` + `error_msg`（复用 PDF 那套 `panic_to_err`/truncate 300 的形态）。
- **不 panic**：WinRT 调用若有 panic 前科，走既有 `panic_to_err` 边界（`extract.rs` 那三处 panic 边界之一）；OCR 的 `catch_unwind` 归 `index_job` 抽取调用点那层，本批不新建边界。
- `ocr_available()` 探测本身若抛（极端：`AvailableRecognizerLanguages` 取不到）→ 按「不可用」处理（保守降级，宁可当没 OCR，也不逐图崩）。
- 空结果不是错：见 D5。

## 七、测试策略

- **`clean_cjk_spaces` 纯函数单测**（无 Windows 依赖，恒可跑）：CJK 间空格删（「合 同 验 收」→「合同验收」）、英文词间空格保留（"contract验收" 不动英文侧）、中英边界、连续多空格、首尾空格、无空格幂等。这条是 §六红线 3 的地基配套，必须能红。
- **`ocr_available()` 单测**：只断「调用不 panic、返回 bool」，**不硬断 true**（别的盒子可能 false）。
- **`image_text` 单测（条件跑）**：加 `src-tauri/tests/fixtures/sample-cn-ocr.png`（含一串已知中文，照 `sample-cn.pdf` 先例）。测试体**先 `if !ocr_available() { return; }` 门控**：有识别器时 `image_text` 该 fixture 断含那个已知词、且**无 CJK 间残留空格**（证清洗到位）；无语言包的盒子如实跳过并记「降级路径未在此机验」。⇒ 这台机（有 zh-Hans-CN）会真跑，拿到真 OCR 读数。
- **不该写**（本仓裁定）：为 `ocr_available=false` 造注入点做「不建行」行为断言——`run_pass` 要真实 `App`/文件系统，`ocr_available` 又依赖系统包，本机造不出「没语言包」；把「降级不建行」写成有名缺口交 §八代码级判据 + 真机（在无包盒子）验，不硬造恒绿断言。
- **门禁**：`cargo test --lib` 基线 **157** → 加 ocr 模块测试（`clean_cjk_spaces` ~5 + `ocr_available` 1 + `image_text` 条件 1）⇒ 实际条数以实跑为准（本仓坑 9：别累加，跑出来数）；`node --test` 仍 8（前端只加一处降级文案 + 一个字段读取，JSX 不在测试网）。两条 clippy、`npm run build`、结构门（`generate_handler!` 仍 41、`SET index_status='missing'` 仍 1 行、`app_data_dir` 仍 1、`allow(dead_code)` 只剩 `db.rs:30`）照旧，另加一条新门：`grep -n "ocr_available" src-tauri/src/lib.rs` 命中数 ≥1 且 `IndexOverview` 里那个字段存在（防「加了模块没接线」）。

## 八、真机验收（沙盒 `dev.zero.pfm.m6test`，一次性 identifier，跑完删净；真实目录逐字对照；子代理不跑 dev、控制方执行；CDP 走 `__TAURI_INTERNALS__.invoke`）

1. **happy-path（本机有中文包，可验）**：往一个登记了根目录的项目放一张含已知中文的 `.png` → 起一轮 → `search_all(那个词)` 正文段命中它、`index_docs` 该行 `ok`、`doc_preview` 能出那段清洗后的文字。**这条是 M6 主承诺，本机必取到读数**（已确认这台机有 `zh-Hans-CN`）。
2. **清洗兑现**：OCR 出的正文经 `search_all` 用**无空格原词**（「合同验收」）命中 ⇒ 证 `clean_cjk_spaces` 在真链路上生效（若没清洗，索引里是「合 同 验 收」分词后的散字，原词搜不到）。
3. **增量不重 OCR**：同图不改 → 再跑一轮 → 该行仍 `ok`、`indexed_at` 不刷新（size+mtime 未变、`current_rowid` 命中 ⇒ 连图都不开）。
4. **空图 / 损坏图**：放一张纯色无字 png → 该行 `skipped(empty_text)`；放一个坏扩展名当 `.png`（非图字节）→ `failed` + error_msg。两态各一条。
5. **降级提示（受限）**：`index_overview.ocr_available` = true（本机）→ `/index` 支持清单区显示「图片 OCR 已启用」。这台机验不了 `false` 那一支（有语言包），如实记「降级 UI 未在本机验，属有名缺口」。
6. **红线**：全程真实数据目录 `%APPDATA%\dev.zero.pfm\` 三文件 size+mtime+SHA-256 逐字未变；沙盒与 `%TEMP%` 夹具删净。
7. **成本读数**：放 ~50 张小图跑一轮，记整轮 wall-time（首图多的成本参照，喂给 §十风险 2）。**不测几十 GB 真实照片集**（那是 M5.5 就欠着的真实规模账，M6 不新造）。

**没能自动验证（逐条点名）**：`ocr_available=false` 的「图片不建行」整条降级路径（本机有包，造不出「没包」）；无中文只有英文包的行为；真实大规模相册首轮的墙钟与内存；`RecognizeAsync` 在极端大图/超多图文本机的资源边界。

## 九、不在本轮

| 项 | 归属 | 理由 |
|---|---|---|
| `.doc`/`.ppt` 旧格式 | M7（LibreOffice headless） | 技术方案明列，与本批正交 |
| OCR 版面/表格/多栏/矫正/旋转 | 不做 | 只出纯文本；上结构是另一套工程，YAGNI |
| 图片 OCR 配额 / 可关开关 | 不做（除非 §十风险 2 实测触发） | D2/D6：默认全开、无 settings 写入口 |
| 独立 OCR worker 队列 | 不做 | D1 选了同步锁外 |
| 扫描件 PDF 与图片共用 OCR | 不做（PDF 走 pdf-extract 那条，抽空即 skipped） | 本批只处理**图片文件**；PDF 内嵌图 OCR 是另一档（性能/复杂度），留后 |
| 英文以外的其它语言优化 | 不做 | 只保证「有任一识别器就 OCR」，中文靠清洗，其它语言不专门调 |
| 缩略图/图片预览展示 | 不做 | 红线只读、不生成派生文件 |

## 十、风险

1. **别的盒子没装中文 OCR**：本机有（已探），但分发/他人机器可能只有英文或全无 ⇒ 全量降级为「图片不建行」，功能对中文用户静默不可用。**缓解**：`/index` 的 `ocr_available` 提示必须显眼；未来若要硬保证中文 OCR，考虑捆一个纯 Rust 引擎（tesseract 需外部 exe，重），那是新里程碑不在此。
2. **图片多目录首轮很久**：D2 选了默认全开无配额，靠增量+取消。若真机 §八.7 或真实使用测出久到不可接受，回头加每项目 OCR 上限（会牵 D3 的 skip_reason 值域，届时要连 UI 文案）。本批先取数。
3. **WinRT 异步在 `run_pass` 线程阻塞**：`image_text` 用 `.get()` 同步等 OCR，占着该 worker 线程。OCR 单张秒级可接受；但 `RecognizeAsync` 若在某图卡死（系统 OCR 挂），这轮会卡到 `cancel` 生效。本机不可廉价构造「OCR 挂死」，记有名缺口。
4. **`windows` crate 版本/features 流动**：feature 名（`Media_Ocr` 等）随 `windows` 次版本变，计划阶段必须 `cargo` 实锁 + 编译验，别照技术方案那个 `0.62.2` 凭印象写死。
5. **OCR 文本进 FTS 的分词质量**：清洗后长无空格中文串靠 jieba；OCR 错字（识别把「己」认成「已」）不可控，属精度上限不是缺陷，别为此在入库侧做模糊（红线：分词链不动）。
6. **`IndexOverview` 新字段的下游**：加 `ocr_available` 要同步 `src/types/index.ts` + store + 页面，漏一处 = TS 或运行时 undefined。一条断言/一处 tsc 守住。

## 十一、要回写与要更正的文档

- **回写活文档** `docs/技术方案.md`：§6.1 line 166「图片…本版本明确不索引（OCR 归 M6）」——M6 落地后这句要改准为「图片经 Windows OCR 纳入（需本机语言包），旧格式 .doc/.ppt 仍 M7」；line 214 里程碑表 M6 行状态、line 230 范围线兑现情况、line 240 §240 降级风险按本机探测读数更新（「本机已验 zh-Hans-CN 可用；无包盒子的降级已实现为不建行 + UI 提示」）。
- **M6 收口时要动**：`docs/开发进度.md`（里程碑表 M6 行 + 新增「M6 验收证据」整节 + 没能自动验证清单 + 缺口归属）、`docs/HANDOFF.md` 刷新（含本轮新坑）。
- **不动**：`.superpowers/sdd/2026-10-10-m5-reconcile/`、`.../2026-10-12-m55-cleanup/`（被已提交文档按路径引用的证据）。M6 另建自己的 SDD 工作区目录。
