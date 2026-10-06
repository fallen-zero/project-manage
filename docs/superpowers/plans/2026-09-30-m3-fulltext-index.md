# M3 全文索引管线与 FTS5 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让软件能扫出已登记项目根目录里的文档、按类型抽出中文文本、写进 SQLite FTS5 虚表，并让「搜一个中文词能捞出文件路径 + 摘要 + 为什么某个文件没进索引」成为可验证的事实。

**Architecture:** 全部索引逻辑在 Rust 侧。库层加 v4 迁移建 `index_docs`（每个文件一行，带 `index_status`）与 rowid 对齐的 `index_docs_fts`；`tokenize.rs` 用同一个 jieba 实例包住入库/查询两侧的分词；`extract.rs` 按扩展名分派（纯文本 / docx / pptx / xlsx / pdf），畸形输入走 `catch_unwind` 兜住；`index_scan.rs` 用 walkdir 按排除规则与上限产出待索引清单；`index_store.rs` 负责两点写（表 + FTS）与两段检索；`index_job.rs` 把「扫 + 抽 + 写」做成可在内存库测试的纯函数，外面只包一层后台线程 + 进度事件。前端只做 `/index` 状态页与 IPC 封装，不碰文件系统。

**Tech Stack:** Rust 2021 / rusqlite 0.40.2 (bundled, FTS5) / jieba-rs 0.11.0 / walkdir 2.5.0 / chardetng 1.0.0 + encoding_rs 0.8.42 / quick-xml 0.41.0 / zip 8.6.0 / calamine 0.36.1 / pdf-extract 0.12.1 / tauri 2.12.0（`Emitter`）/ React 19 + TanStack Router（hash history）+ Tailwind 4 + shadcn/ui。

**Spec:** `docs/技术方案.md`（第一节需求结论、第四节数据模型、第六节 6.1 索引、第八节里程碑 M3）。本计划逐条对应它；出现分歧时以 spec 为准并回写 spec。

---

## Global Constraints

每个任务的实现都必须满足下面这些，它们是 spec 里已经钉死的口径，不是本计划的新决定：

- **只登记路径、只读文件。绝不创建、移动、复制、重命名、改写任何被索引的原始文件**；索引器对磁盘的唯一动作是「读」。
- **密文列不参与任何检索**：FTS 的内容只来自磁盘文件的抽取文本，库里没有任何一条路径把 `*_cipher` / `*_nonce` 喂给索引或检索 SQL。
- **入库与查询必须用同一个分词器、同一套预处理**（同版本词典、同样的 trim/过滤）。这条是硬约束，所以两侧都只能在 `tokenize.rs` 里拿词典。
- **不假设 UTF-8**：文本读取必须过 chardetng + encoding_rs；GBK 在 Windows 中文环境是必踩项。
- **上限**：单文件 20 MB（`20971520` 字节）、单项目 50000 个文件；超限记 `skipped` + 原因，不静默丢弃。
- **排除目录**（可配）：`node_modules`、`dist`、`build`、`target`、`__pycache__`、`.git`；比较时忽略 ASCII 大小写（Windows 上真实存在 `Node_Modules`）。
- **`index_status` 必须现在就设计好**（spec 明示后补要重建索引）：`pending | ok | skipped | failed | missing` + `skip_reason` + `error_msg`。
- **WAL 读回断言不许削弱**（M0 的既有不变量）：本计划新增的连接只能走 `db::open` / `db::open_in_memory`，不得另开一条绕过 `after_open` 的连接路径。
- **一个坏文件不能打挂整轮索引**；抽取用 `catch_unwind` 兜 panic，扫描用 `walkdir` 的错误流兜不可读目录。
- 输出语言一律中文：注释、错误 message/hint、UI 文案、commit message。错误码（`extract_failed` 这类）与 SQL 字面量保持机器可解析的英文。
- 样式：Tailwind 工具类优先；确需独立样式文件用 `.scss`（本轮预期不需要新样式文件）。
- **真实数据目录 `%APPDATA%\dev.zero.pfm\` 是只读红线**；任何真机验证必须用 `tauri dev -c` 覆盖 `identifier` 到一次性目录。
- 验证脚本只回断言结果（布尔/条数/错误文案前缀），**不把解出的明文或大段正文打印出来**。

---

## 已实测的 API 事实（本计划的代码就按这些写，不要凭记忆改）

下面每一条都是本轮在**同一台机器、同一版本依赖**上跑出来的，不是文档推理。落地时若改动依赖版本，要去包里重核对应行为。

| # | 事实 | 证据 |
|---|---|---|
| 1 | FTS5 虚表在本项目 bundled 构建里可用；`snippet()` / `bm25()` / `MATCH` / `DELETE WHERE rowid` 全部工作正常 | 探针 `a4`、`b3`：两列表 `fts5(name_tokens, body_tokens, tokenize='unicode61')` 上 `snippet(…, 1, '[', ']', '⋯', 12)` 返回字符串、`bm25` 返回负浮点 |
| 2 | **rowid 对齐可行且必要**：`index_docs` 用 `doc_rowid INTEGER PRIMARY KEY AUTOINCREMENT` 承载关系，FTS 用同一个 rowid 写入/删除；`id TEXT UNIQUE` 继续做业务主键 | 探针 `b3`：`INSERT INTO index_docs_fts (rowid, …)` + `JOIN … ON d.doc_rowid = f.rowid` 命中；按 `doc_id` 列删是**全表扫**，故不用 `doc_id` 方案 |
| 3 | 删 `index_docs` 行**不会**连带删 FTS 行（虚表无 FK），必须同一事务两点删 | 探针 `b3`：`DELETE FROM index_docs` 后 `SELECT count(*) FROM index_docs_fts` = 1 |
| 4 | jieba：`Jieba::new()` 可作为 `OnceLock` 的初始化闭包；`cut()` 与 `cut_for_search()` **都返回 `Vec<Token<'a>>`**，取 `.word`；0.11.0 里没有 `Seg` 这个类型（`jieba-rs-0.11.0/src/lib.rs:1018` 与 `:328`） | 探针 `a2`、`a14` + 读包源码 |
| 5 | **`cut_for_search` 是 `cut` 的超集但不万能**：`付款条件` → 同时产出 `付款/条件/付款条件`；而 `维保期` 不被再切，仍是单个词 | 探针 `a16` 打印 `这 是 另 一份 报价 价单 报价单 … 付款 条件 付款条件 …` |
| 6 | **两段查询是必需的**：`"维保"` 精确 0 命中、`"维保"*` 前缀 1 命中；前缀的 `*` 必须写在**引号外面**。**夹具限制（Task 2/8 实测踩过）**：造这条场景时文件名不能写 `维保说明.docx` —— 入库侧 `cut_for_search(text, true)` 的 HMM 会把「维保说明」切成 `维保 说明`，`name_tokens` 里就有了独立的 `维保`，精确段直接命中，「精确 0 命中」的前提当场失效。两处统一写 `维保期说明.docx`（切出 `维保期 说明`） | 探针 `a15`、`a16` + Task 2 实测 |
| 7 | 引号包裹后 FTS5 语法无法从查询串注入：`NEAR(合同 报价)` → `"NEAR" AND "合同" AND "报价"`，`合同*` → `"合同"`，`-合同` → `"合同"`，`"合同"` → `"合同"`；内部双引号用 `""` 转义 | 探针 `a11` |
| 8 | 纯标点/纯符号词必须从**查询侧**丢弃。unicode61 默认的 token 字符集是 `categories = "L* N* Co"`（字母、数字、**私有使用区**）；标点 `P*` 与符号 `S*`（™ € ± ★）都是**分隔符**、永不进索引，所以丢弃它们没有信息损失 —— 留着只会让整条查询变 0 命中。**入库侧保留标点**，snippet 才可读。两侧口径差只有一处：私有使用区 `Co` 是词字符而 `char::is_alphanumeric()` 对它回 false，于是全由 Co 组成的查询词会被丢弃（表现为无结果，不是错结果） | 探针 `a13`：`q="验收，"` → `"验收"` 命中 1；`q="，"` → 无表达式。类别表读 bundled 源码：默认 `zCat` 在 `libsqlite3-sys-0.38.2/sqlite3/sqlite3.c:265946`，`fts5UnicodeIsAlnum` 即查 `aCategory[]`（`:266009`），`sqlite3Fts5UnicodeCatParse` 把 `S*`→`aArray[23..26]`、`Co`→`aArray[31]`（`:267324` 起） |
| 9 | `snippet` 出来的词之间带我们插入的空格；清洗规则「空格相邻任一侧是 CJK 就删」得到可读摘要，且 Latin-Latin 之间的空格不被吃掉 | 探针 `a12`、`a13`：`甲方 要求 ， 响应 时间 不 超过 800 毫秒 。 [验收] 指标 见 合同⋯` → `甲方要求，响应时间不超过800毫秒。[验收]指标见合同⋯` |
| 10 | quick-xml 0.41 **把实体单独拆成 `Event::GeneralRef`**，`xml10_content()` 不会解实体；`GeneralRef` 的载荷是**不含 `&` 和 `;` 的裸片段**（`apos`、`#39`、`amp`） | 探针 `a6`、`a7` |
| 11 | 正确的还原写法是 `escape::unescape(&format!("&{frag};"))`；`reader.read_text(e.name())` 这条路**会把实体原样留在正文**（`合同&apos;…`），不可用 | 探针 `a7`：plan_a=`合同'验收'标准&维保`，plan_c=`合同&apos;验收&#39;标准&amp;维保` |
| 12 | docx 只取 `<w:t>`、pptx 只取 `<a:t>` 就能自动跳过 `<w:instrText>`（域代码）等噪声 | 探针 `a8`：nodes=`["合同'验收", " 标准 & 说明", "维保期", "十二个月"]` |
| 13 | zip 8.6：`ZipArchive::new(File)`、`by_name(&mut self, &str)`、`file_names() -> impl Iterator<Item=&str>`；写侧用 `zip::write::SimpleFileOptions::default()`（`FileOptions::default()` 推不出扩展类型参数） | 探针 `a3`、`a8` |
| 14 | calamine：`open_workbook_auto` 后要 `use calamine::Reader as _;`，`worksheet_range(name)?` 直接拿到 Range（**不是**双层 Result），`range.rows()` 逐格 `cell.to_string()` | 探针 `a5` |
| 15 | chardetng **1.0 改了 API**：`EncodingDetector::new(Iso2022JpDetection::Deny)`、`guess(None, Utf8Detection::Deny)`；旧的 `new()` / `guess(_, bool)` 编不过 | 探针 `a9`（改前 E0061/E0308） |
| 16 | 「先 `std::str::from_utf8` 严格试一次，失败才探测」对 GBK/UTF-8/纯 ASCII 三类输入都得到正确结果，round-trip 相等 | 探针 `a9` |
| 17 | UTF-16 **不需要自己判 BOM**：`encoding_rs::Encoding::decode` 内部先跑 `Encoding::for_bom`，认 UTF-8/UTF-16LE/UTF-16BE 三种 BOM，命中就**盖过**传进来的 encoding 并剥掉 BOM。实测：UTF-16LE 字节 chardetng 猜成 `windows-1252`，`decode` 回报 `used=UTF-16LE`、输出与原文逐字相等（BE 同理）；走 `decode_without_bom_handling` 得到 `ÿþŒš6e¥bJT…` 且 `had_errors=false` —— 正是「不报错、只是永远搜不到」。GBK 不受影响（猜 GBK、用 GBK、原文正确） | 探针 `a17`（本轮临时跑后即删；永久守护是 Task 3 的 `utf16_with_bom_is_decoded_not_guessed`）+ 读源码 `encoding_rs-0.8.42/src/lib.rs:3039` |
| 18 | pdf-extract 对「假 PDF 头」和「截断的真 PDF」都**返回 Err 而非 panic**；`catch_unwind` 是防 lopdf 在别的畸形输入上 panic 的兜底，本机无法构造出触发它的样本（见已知缺口） | 探针 `a10` |
| 19 | `tests/fixtures/sample-cn.pdf`（Edge 打印真中文 PDF，150225 B）可被 pdf-extract 抽出 86 字，含「验收」「维保」 | 探针 `a1` |
| 20 | walkdir 2.5 的入口是 **`WalkDir`**（没有 `WalkBuilder`）；`IntoIter::skip_current_dir()` 在命中排除目录后调用即可整棵剪掉；Windows 上返回的路径**带反斜杠**、`file_name()` 可用于比较；`metadata().modified()` 转 unix 秒可用 | 探针 `b1`（E0433 后改用 `WalkDir`） |
| 21 | `INSERT … ON CONFLICT (project_id, path) DO UPDATE` 不改 `id`：**同一行被重新索引时 `index_docs.id` 与 `doc_rowid` 保持稳定**，所以 FTS 可以先按 rowid 点删再插 | 探针 `b2`：第二次用不同 id 写同路径，行仍是 `d1` |
| 22 | `settings` 读回缺失键时 `query_row(...).ok()` 得到 `None`，逗号分隔值按 `trim` + 去空处理可用；值里允许出现空格（`目标 目录`） | 探针 `b2` |
| 23 | `tauri::Emitter::emit<S: Serialize + Clone>(&self, event, payload)` 在 2.12.0 存在（`tauri-2.12.0/src/lib.rs:961`），`AppHandle` 可 move 进线程 | 读 tauri 源码 |
| 24 | `after_open` 里 `PRAGMA foreign_keys=ON` 是生效的，所以 **插 `index_docs` 必须先有对应 `projects` 行**，否则 `FOREIGN KEY constraint failed`；Task 1 的两条建表测试因此各带一行 `INSERT INTO projects`（Task 7/8/9 的测试用 `seed_project()` 满足同一约束）。另一半：FTS5 虚表没有 FK，**不受 `ON DELETE CASCADE` 连带**——软删/硬删项目只会带走 `index_docs` 行，虚表留下孤儿行，只能由写入侧按 rowid 显式清（Task 7 的 `clear_project`），检索侧靠 `JOIN index_docs` 天然过滤 | 探针 `a18`（Task 1 落地时实测） |
| 25 | **`read_event` 对畸形 Office XML 不报错，只是不发收口信号**：`IllFormedError::MissingEndTag` 的文档明写「This error is returned from `Reader::read_to_end`」，逐事件读不会拿到，缺闭标签的输入一路走到 `Event::Eof`。且 `check_end_names = false` 时 End 事件**不被改写**、带的是那个不匹配闭标签自己的名字（`<w:t>abc</w:p>` 的 End 是 `p`），所以「End 名字等于 tag 才收口」在这类输入上永远收不了口。`escape::unescape` 只解析 5 个预定义实体与 `&#…;` 数字引用，其它命名实体回 `UnrecognizedEntity` | 读 `quick-xml-0.41.0`：`errors.rs:117-121`、`reader/mod.rs:111-113`（同处注明 Default 为 `true`，我们是显式关掉的）、`escape.rs:222` 与 `:47-49`。Task 4 评审据此重写状态机：三个收口时机（End / 新 Start / EOF）统一走 `close_run` |

**这些是 spec 第 127 行的更正**：spec 写「`content_rowid` 对齐 `index_docs.id`」，但 `id` 是 TEXT uuid，SQLite 的 rowid 必须是整数。实际采用**事实 2** 的形状：`index_docs` 加一列 `doc_rowid INTEGER PRIMARY KEY AUTOINCREMENT` 作为对齐锚，`id TEXT UNIQUE` 保留给业务与 IPC。Task 12 会把 spec 这句改过来。

---

## 文件结构

新增（Rust）：

- `src-tauri/src/tokenize.rs` — 唯一的词典入口。对外三个纯函数：`index_text`、`query_expression`、`clean_snippet`。不含 SQL、不含 IO。
- `src-tauri/src/extract.rs` — 「一个路径 → 一段文本」。按扩展名分派；错误只描述为什么抽不出，不碰数据库。
- `src-tauri/src/index_scan.rs` — 「一个根目录 → 待索引清单 + 超限清单 + 遍历错误 + 是否触顶」。纯文件系统，不含 SQL。
- `src-tauri/src/index_store.rs` — 索引表的读写：两点写、幂等重写、状态统计、分页列表、两段检索。
- `src-tauri/src/index_job.rs` — 把 scan/extract/store 串成一次 pass 的纯函数（可在内存库测）+ 后台线程包装与进度事件。

新增（前端）：

- `src/types/index.ts` — `IndexOverview`、`IndexDocRow`、`DocHit`、`IndexProgress` 类型。
- `src/stores/index-job.ts` — 进度事件订阅 + zustand store。
- `src/pages/index-status.tsx` — `/index` 状态页（跑索引、进度、按状态筛选的文件清单、试搜框）。

修改：

- `src-tauri/src/db.rs` — `TARGET_VERSION` 3 → 4，新增 `V4` DDL。
- `src-tauri/src/lib.rs` — 5 个 `mod`、`AppState` 加索引作业状态、5 个新 IPC 命令。
- `src/lib/api.ts` — 新命令封装。
- `src/router.tsx`、`src/components/app-shell.tsx` — 新路由与导航项。
- `docs/技术方案.md`、`docs/开发进度.md` — 收口。

测试基线：**动手前 `cargo test --lib` = 41 passed**（本轮已实测：`grep -c "#\[test\]"` 各模块 4+12+7+9+9 = 41，与实跑一致；临时探针已全部删除）。每个任务的 Expected 数字都从这条链往上加，最后到 **94**（+4 +5 +4 +4 +5 +10 +7 +7 +7 = 53 条新测试；Task 10/11/12 不新增 Rust 测试。Task 8 的 6→7 是复审补的 `query_guards_and_limit_pass_straight_through`；Task 9 的 5→7 是评审修复轮补的 `capped_and_scanned_total_survive_the_quota` 与 `running_flag_is_released_even_when_the_job_panics`）。

---

## Task 1: schema v4 —— `index_docs` + rowid 对齐的 `index_docs_fts` + settings 种子

**Files:**
- Modify: `src-tauri/src/db.rs:85`（`TARGET_VERSION`）、`db.rs:103-109`（match 分支）、`db.rs` 末尾（新增 `V4` 常量）
- Test: `src-tauri/src/db.rs` 的 `#[cfg(test)] mod tests`
- 一并提交：`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`（M3 依赖 `jieba-rs`/`walkdir`/`chardetng`/`encoding_rs`/`quick-xml`/`pdf-extract`/`zip`/`calamine` + dev 依赖 `rust_xlsxwriter` 已 `cargo add` 完并 `cargo build --lib` 通过，本任务前它们在工作树里未提交）

**Interfaces:**
- Consumes: `db::open(data_dir)`、`db::open_in_memory()`、既有 `settings(key, value)` 表
- Produces:
  - 表 `index_docs(doc_rowid INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, path TEXT NOT NULL, ext TEXT NOT NULL, size INTEGER NOT NULL DEFAULT 0, mtime INTEGER NOT NULL DEFAULT 0, index_status TEXT NOT NULL DEFAULT 'pending', skip_reason TEXT NULL, error_msg TEXT NULL, indexed_at TEXT NULL, created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now')), UNIQUE(project_id, path))`（这行与 Step 3 的 DDL 同源，冲突以 Step 3 为准；`project_id` 的外键见事实 24）
  - 虚表 `index_docs_fts(name_tokens, body_tokens, tokenize='unicode61')`，其 `rowid` == `index_docs.doc_rowid`
  - `settings` 三行种子：`index_exclude_dirs`、`index_max_file_bytes`、`index_max_files_per_project`
  - `schema_meta` 最大版本 = 4

- [ ] **Step 1: 写失败测试（3 条）**

追加到 `src-tauri/src/db.rs` 的 `mod tests` 里。注意 `open_in_memory()` 会跑完整迁移链，所以这三条测的就是生产 DDL 本身。

```rust
    /// v4 建表必须真的成功：FTS5 虚表在 bundled 构建里不是「假设可用」，这里直接建一次。
    #[test]
    fn v4_creates_index_tables_and_fts_is_writable() {
        let conn = open_in_memory().unwrap();
        // index_docs.project_id 带外键，父行必须先存在（事实 24）。
        conn.execute(
            "INSERT INTO projects (id, name) VALUES ('p-1', '索引测试项目')",
            [],
        )
        .unwrap();
        // 表存在
        let names: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master
                  WHERE name IN ('index_docs', 'index_docs_fts') ORDER BY name",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(names, vec!["index_docs", "index_docs_fts"]);

        // rowid 对齐：先插主表拿 rowid，再用同一个 rowid 插 FTS，JOIN 必须回得来
        conn.execute(
            "INSERT INTO index_docs (id, project_id, path, ext, index_status)
             VALUES ('u-1', 'p-1', 'C:/x/合同验收.docx', 'docx', 'ok')",
            [],
        )
        .unwrap();
        let rowid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO index_docs_fts (rowid, name_tokens, body_tokens) VALUES (?1, ?2, ?3)",
            params![rowid, "合同 验收", "甲方 要求 验收 指标"],
        )
        .unwrap();
        let hit: String = conn
            .prepare(
                "SELECT d.id FROM index_docs_fts f
                   JOIN index_docs d ON d.doc_rowid = f.rowid
                  WHERE index_docs_fts MATCH ?1",
            )
            .unwrap()
            .query_row(params!["\"验收\""], |r| r.get(0))
            .unwrap();
        assert_eq!(hit, "u-1");
    }

    /// status 是 CHECK 约束，写错值必须在库层就挡住，而不是靠每个调用方自觉；
    /// `missing` 也要现在就进枚举——CHECK 改不动，M5 的对账再补就得重建整张表。
    #[test]
    fn index_status_check_accepts_the_five_documented_states() {
        let conn = open_in_memory().unwrap();
        // index_docs.project_id 有 REFERENCES projects(id) 外键，而 after_open 里
        // PRAGMA foreign_keys=ON，所以必须先落一行父项目，插入才走得通（见事实 24）。
        conn.execute(
            "INSERT INTO projects (id, name) VALUES ('p-1', '索引测试项目')",
            [],
        )
        .unwrap();
        for (i, status) in ["pending", "ok", "skipped", "failed", "missing"].iter().enumerate() {
            conn.execute(
                "INSERT INTO index_docs (id, project_id, path, ext, index_status)
                 VALUES (?1, ?2, ?3, 'txt', ?4)",
                params![format!("u-{i}"), "p-1", format!("C:/x/{i}.txt"), status],
            )
            .unwrap_or_else(|e| panic!("{status} 应该在枚举内：{e}"));
        }
        let e = conn
            .execute(
                "INSERT INTO index_docs (id, project_id, path, ext, index_status)
                 VALUES ('u-x', 'p-1', 'C:/x/xx.txt', 'txt', 'done')",
                [],
            )
            .unwrap_err();
        assert!(
            e.to_string().contains("CHECK"),
            "未登记的状态必须被 CHECK 约束拒绝，实际：{e}"
        );
    }

    /// 上限与排除规则要能被 UI 改，所以 v4 必须落种子值；否则 Task 6 的 load() 拿到空字符串，
    /// 会把「没有排除目录」当成默认行为，把 node_modules 全灌进索引。
    #[test]
    fn v4_seeds_index_settings_with_the_documented_limits() {
        let conn = open_in_memory().unwrap();
        let read = |key: &str| -> String {
            conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(
            read("index_exclude_dirs"),
            "node_modules,dist,build,target,__pycache__,.git"
        );
        assert_eq!(read("index_max_file_bytes"), "20971520", "spec 定的单文件 20 MB");
        assert_eq!(read("index_max_files_per_project"), "50000");
        assert_eq!(
            conn.query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            4
        );
    }
```

`params!` 只在测试里用得到，所以**必须写在 `#[cfg(test)] mod tests` 内部**（`use super::*;` 之后加一行 `use rusqlite::params;`），不要改 `db.rs` 顶部的 `use rusqlite::Connection;`。这条是实测过的：把 `params` 提到顶层后 `cargo clippy --lib -- -D warnings` 报 `error: unused import: params --> src\db.rs:3:16`（Task 2 Step 5 就是这道闸，会被卡住）——`--lib` 不带 test cfg，测试模块不参与编译，顶层引入因此是未使用的。

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib -- db::tests::v4 db::tests::index_status`
Expected: FAIL 3 条，报 `no such table: index_docs`；种子值那条报 `QueryReturnedNoRows`（键不存在）。注意多个过滤词要放在 `--` 后面，`cargo test --lib a b` 会被 cargo 当成未知参数。

- [ ] **Step 3: 写迁移**

`db.rs` 里把版本改到 4 并加分支：

```rust
const TARGET_VERSION: i64 = 4;
```

```rust
    for target in (current + 1)..=TARGET_VERSION {
        conn.execute_batch(match target {
            1 => V1,
            2 => V2,
            3 => V3,
            4 => V4,
            _ => "",
        })?;
```

在 `V3` 之后追加：

```rust
/// v4：全文索引。三处设计与 spec 有意不同，都是为了实测结果让路：
/// 1. `doc_rowid` 是对齐锚。FTS5 的 rowid 只能是整数，而业务主键是 TEXT uuid，所以主表额外
///    带一个 AUTOINCREMENT 整数列给虚表当 rowid；按 `doc_id` 列删虚表是全表扫，按 rowid 是点删。
/// 2. 虚表不带 `content=`，是独立存储。正文不另存一份原文：磁盘上本来就有一份。
/// 3. `missing` 现在就进 CHECK。CHECK 约束无法 ALTER，M5 的启动对账要它就得重建整张表。
const V4: &str = "
CREATE TABLE index_docs (
    doc_rowid    INTEGER PRIMARY KEY AUTOINCREMENT,
    id           TEXT    NOT NULL UNIQUE,
    project_id   TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path         TEXT    NOT NULL,
    ext          TEXT    NOT NULL,
    size         INTEGER NOT NULL DEFAULT 0,
    mtime        INTEGER NOT NULL DEFAULT 0,
    index_status TEXT    NOT NULL DEFAULT 'pending'
                 CHECK (index_status IN ('pending','ok','skipped','failed','missing')),
    skip_reason  TEXT,
    error_msg    TEXT,
    indexed_at   TEXT,
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (project_id, path)
);

CREATE INDEX ix_index_docs_project ON index_docs(project_id);
CREATE INDEX ix_index_docs_status  ON index_docs(index_status);

CREATE VIRTUAL TABLE index_docs_fts USING fts5(
    name_tokens,
    body_tokens,
    tokenize = 'unicode61'
);

INSERT OR IGNORE INTO settings (key, value) VALUES
    ('index_exclude_dirs', 'node_modules,dist,build,target,__pycache__,.git'),
    ('index_max_file_bytes', '20971520'),
    ('index_max_files_per_project', '50000');
";
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib`
Expected: `44 passed; 0 failed`（基线 41 + 本任务 3）

- [ ] **Step 5: 既有库的前向迁移不能报错**

Run: `cd src-tauri && cargo test --lib db::tests`
Expected: 7 条全绿（`db::tests` 原有 4 条含 M0 的 WAL 三条，加本任务 3 条）。`migrate` 是 `for target in (current+1)..=TARGET_VERSION`，老库从 3 升 4 只跑 `V4`。

- [ ] **Step 6: 写迁移原子性的失败测试（1 条）**

上面那句「老库升 4 只跑 V4」有个前提没被守住：`execute_batch` 与写 `schema_meta` 版本行是**两条独立的自动提交语句**，而 `V4` 的两条 `CREATE` 没有 `IF NOT EXISTS`。断电或报错卡在两者中间，库里就留下「表建了一半、版本号没落」的半成品，下次启动重跑同一个版本会撞 `table index_docs already exists` → `migrate` 报错 → `db::open` 报错。而这个项目对唯一持久库的 stance 是**拒绝启动而不是静默降级**（M0 的 WAL 读回就是这个立场），所以这条路径等于把用户唯一的库变砖。修法是「一个版本 = 一个事务」：SQLite 的 DDL 可回滚，半成品会整个消失而不是留下来毒死下一次启动。

追加到 `mod tests`：

```rust
    /// 一个版本 = 一个事务：失败的 batch 既不留半成品表，也不留版本号行。
    /// 守的是「断电后下次启动还起得来」：这个库是唯一副本，没有回退路径。
    #[test]
    fn a_failed_version_leaves_no_partial_objects_and_no_version_row() {
        let conn = open_in_memory().unwrap();
        apply_version(&conn, 99, "CREATE TABLE atomic_probe(x); CREATE TABLE atomic_probe(x);")
            .expect_err("同名 CREATE 的第二条必须让整个 batch 失败");
        let objs: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'atomic_probe'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(objs, 0, "事务回滚后不该留下第一条已建好的表");
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM schema_meta WHERE version = 99", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "失败的版本不该写下版本号");
    }
```

不断言错误文案（`AppError` 的 Display 里是否带底层 SQLite 消息不在本任务的契约内），只断言两个结构后果：对象不存在、版本号没落。

- [ ] **Step 7: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib -- db::tests::a_failed_version`
Expected: 编译失败 `cannot find function apply_version`（这条函数还不存在）。

- [ ] **Step 8: 把版本循环改成事务**

`migrate` 里那段 `for target in …` 换成调用新函数，新函数放在 `migrate` 之后：

```rust
    for target in (current + 1)..=TARGET_VERSION {
        let ddl = match target {
            1 => V1,
            2 => V2,
            3 => V3,
            4 => V4,
            _ => "",
        };
        apply_version(conn, target, ddl)?;
    }
```

```rust
/// 一个版本 = 一个事务：DDL 与版本行同生同死。
///
/// 不能拆成两条自动提交语句：`execute_batch` 成功不代表版本已登记，中间出任何事
/// （报错、断电、进程被杀）都会留下「对象建了一半、`schema_meta` 还停在旧版本」的库，
/// 重放同一个版本时 `CREATE TABLE`（V4 没写 `IF NOT EXISTS`）直接失败，而本项目对唯一
/// 持久库是拒绝启动的，于是应用永久起不来。`Transaction` 的 drop 默认就是回滚
/// （rusqlite 0.40.2 `src/transaction.rs:25` 的 `DropBehavior::Rollback` 注释写明
/// "This is the default"），所以这里不需要手写 rollback 分支；用 `unchecked_transaction`
/// 是因为 `migrate` 只拿到 `&Connection`，`transaction()` 要 `&mut self`。
fn apply_version(conn: &Connection, target: i64, ddl: &str) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(ddl)?;
    tx.execute(
        "INSERT INTO schema_meta (version, applied_at) VALUES (?1, datetime('now'))",
        [target],
    )?;
    // `tx.commit()` 返回 `rusqlite::Result<()>`，与本函数的 `AppResult<()>` 不同型，
    // 直接当尾表达式会 E0308；用 `?` 走 error.rs 里那个 From 转换，与上面两处一致。
    Ok(tx.commit()?)
}
```

- [ ] **Step 9: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib && cargo clippy --lib --all-targets -- -D warnings`
Expected: `45 passed; 0 failed`（基线 41 + v4 三条 + 本步 1 条），clippy 零告警。

- [ ] **Step 10: 提交**

本任务落两个提交（评审修复轮与建表分开，别把两条不同内容的改动挤进同一条 message）：

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/db.rs
git commit -m "feat: M3 库层 v4 建 index_docs 与 rowid 对齐的 FTS5 虚表"
```

```bash
git add src-tauri/src/db.rs
git commit -m "fix: 库层迁移改为一版本一事务，失败不再留下半成品库"
```

---

## Task 2: `tokenize.rs` —— 一个词典入口 + 两段查询表达式 + snippet 清洗

**Files:**
- Create: `src-tauri/src/tokenize.rs`
- Modify: `src-tauri/src/lib.rs`（加 `mod tokenize;`）
- Test: `src-tauri/src/tokenize.rs` 的 `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `jieba_rs` 0.11（`cut` 与 `cut_for_search` 都返回 `Vec<Token>`，取 `.word`；见事实 4）、`crate::db::open_in_memory`
- Produces:
  - `pub fn index_text(text: &str) -> String`
  - `pub fn query_expression(query: &str, prefix: bool) -> Option<String>`
  - `pub fn clean_snippet(raw: &str) -> String`
  - `pub(crate) fn is_cjk(c: char) -> bool`
  - 三条 `pub fn` 各带一条 `#[allow(dead_code)]`：生产调用点要到 Task 7/8 才出现，本模块挂上时没有 caller，而 Step 5 的闸是 `cargo clippy --lib -- -D warnings`（`--lib` 不编译测试模块，测试里的调用救不了它）。这与 `db.rs:30` 既有写法同形。**Task 7/8 落地时必须删掉对应那行 `allow`**，否则就是永久豁免。

- [ ] **Step 1: 写失败测试（5 条）**

测试要建一张和 Task 1 形状一致的临时虚表，所以先放一个本地 helper（不进生产代码）：

```rust
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
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib tokenize`
Expected: 编译失败 `cannot find function index_text` / `query_expression` / `clean_snippet`（`E0425`）。测试与实现同在一个新文件里，`mod tokenize;` 还没往 `lib.rs` 挂，所以报的是「函数找不到」而不是「模块找不到」。

- [ ] **Step 3: 写实现**

`src-tauri/src/lib.rs` 的 mod 列表按字母序插 `mod tokenize;`（在 `mod search;` 之后、`mod vault;` 之前）。

```rust
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
/// 这类复合词再切成 付款/条件，查询侧按 `cut` 切出来的子词才命得中。代价是索引膨胀，而膨胀率
/// 随输入波动、**不是容量常数**：实测 `cut_for_search` 相对 `cut` 的词条数比从 1.0
/// （`合同与报价，条款从优`）到 1.57（`这份报价单含税总价为十二个月`）。Task 7 估库体积取偏上的值。
///
/// 标点保留在正文里：unicode61 不把标点当索引字符，所以它不会造成假命中，
/// 而 `snippet()` 是从原始列文本重建摘要的，留着标点才可读。
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
pub fn query_expression(query: &str, prefix: bool) -> Option<String> {
    let terms: Vec<String> = jieba()
        .cut(query, true)
        .into_iter()
        .map(|x| x.word.trim().to_owned())
        // 纯标点词永不进索引，留在查询里只会把整条 AND 变成 0 命中（实测「验收，」即如此）。
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
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib tokenize`
Expected: `5 passed`；再跑全量 `cargo test --lib` → `50 passed; 0 failed`

第三条断言按规则就是全删：`报价|单` 两侧都 CJK，`单|2026` 左邻 CJK，`2026|年` 右邻 CJK —— 规则只看「任一侧是不是 CJK」，不看另一侧是数字还是拉丁字母，所以得到 `报价单2026年`。要验「Latin-Latin 空格保留」请看上一条断言（`the [quick] brown fox 与 中文 混排`）。不要为了迁就某个期望值去给数字加例外分支。

- [ ] **Step 5: clippy + 提交**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings
git add src-tauri/src/tokenize.rs src-tauri/src/lib.rs
git commit -m "feat: M3 分词模块用单一 jieba 词典包住入库与查询两侧"
```

---

## Task 3: `extract.rs`（一）—— 纯文本编码探测，GBK 必踩项

**Files:**
- Create: `src-tauri/src/extract.rs`（本任务只写文本分支 + 骨架）
- Modify: `src-tauri/src/lib.rs`（`mod extract;`）
- Test: `src-tauri/src/extract.rs` 的 `mod tests`（本任务四条全部喂字节串，**不碰文件系统**，所以不用 `tempfile`；落盘路径由 Task 5 的 `extract_text` 分派测试覆盖）

**Interfaces:**
- Consumes: `chardetng::EncodingDetector::new(Iso2022JpDetection::Deny)` + `guess(None, Utf8Detection::Deny)`、`encoding_rs`、`crate::error::{AppError, AppResult}`（`AppError::io(path, &e)` 已存在，`src-tauri/src/error.rs:21`，错误码 `fs_failed`）
- Produces:
  - `pub enum ExtractError` 的落点：本任务先只用 `AppError`，错误码固定两个 —— `extract_unsupported`、`extract_failed`（编码问题不新增码：UTF-16/GBK 只要解得出来就入库，见下面 `decode_text_bytes`）
  - `fn decode_text_bytes(bytes: &[u8]) -> String`（**不是 `AppResult`**：探测解不出时用替换字符照样入库，这条函数没有失败分支；包成 `Result` 会被 `cargo clippy -- -D warnings` 的 `unnecessary_wraps` 拦下）
  - `fn read_text_file(path: &Path) -> AppResult<String>`

  `fail` 与 `read_text_file` 在本任务**没有 caller**（前者第一次被 Task 4 的 Office 分支调用，后者被 Task 5 的 `extract_text` 调用），而 `mod extract;` 是私有模块，`pub` 也救不了 `dead_code`，`cargo clippy --lib -- -D warnings` 会当场红。两处各带一条 `#[allow(dead_code)]` 并在注释里写明是哪个任务接上，**Task 4/5 落地时必须删掉对应那行**（与 Task 2 的三条 `pub fn` 同形，那三条的删除责任已经记在 Task 7/8）。若 rustc 把不可达链条往下传、连 `decode_text_bytes` 也报 `never used`，就同样加一条 allow 并在报告里点名 —— 它的删除责任跟 `read_text_file` 一起走 Task 5，别顺手豁免到永久。

- [ ] **Step 1: 写失败测试（4 条）**

```rust
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
```

注意最后一条里我把 BOM 也写进了字面串（`"﻿验收清单"` 首字符是 U+FEFF）——如果编辑器里那个字符不可见导致对不上，就改写成 `"\u{FEFF}验收清单"`。**不要用「剥掉后等于 `"验收清单"`」之外的模糊断言。**

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib extract`
Expected: `unresolved module` / `cannot find function decode_text_bytes`

- [ ] **Step 3: 写实现**

```rust
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

#[allow(dead_code)] // caller 是 Task 5 的 extract_text 分派，落地时删掉本行（decode_text_bytes 若一起被报，同批删）
fn read_text_file(path: &Path) -> AppResult<String> {
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .and_then(|mut f| f.read_to_end(&mut buf))
        .map_err(|e| AppError::io(path, &e))?;
    Ok(decode_text_bytes(&buf))
}
```

`chardetng::EncodingDetector::new` 的入参是 `Iso2022JpDetection`（1.0 的新签名，见事实 15）；`feed(bytes, true)` 之后才能 `guess`，且 `feed` 在 `guess` 之后不可再调（本函数一次读完）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib extract`
Expected: `4 passed`；全量 `cargo test --lib` → `54 passed; 0 failed`

- [ ] **Step 5: clippy + 提交**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings && cargo clippy --lib --all-targets -- -D warnings
```
Expected: 两道都零告警。`--lib` 那道专门用来暴露上面两条 `#[allow(dead_code)]` 该不该再加第三条；`--all-targets` 那道会把测试模块里没用到的东西（比如临时辅助函数）报出来。两道闸在本任务都要跑，是因为 Task 5/6 的闸就是 `--lib --all-targets`，现在留下的告警会在那两个任务里变成别人头上的红灯。

```bash
git add src-tauri/src/extract.rs src-tauri/src/lib.rs
git commit -m "feat: M3 纯文本抽取走 chardetng 探测，UTF-16 靠 decode 的 BOM 嗅探解出"
```

---

## Task 4: `extract.rs`（二）—— Office：docx/pptx 共用一个事件循环 + xlsx

**Files:**
- Modify: `src-tauri/src/extract.rs`（新增 Office 分支）
- Test: 同文件 `mod tests`（测试内用 `zip::write` / `rust_xlsxwriter` 现造文件，不依赖外部 fixture）

**Interfaces:**
- Consumes: `zip::ZipArchive::new(File)`、`by_name(&mut self, name)`、`file_names()`、`quick_xml::Reader::from_str(&xml)` + `config_mut().check_end_names = false`、`calamine::open_workbook_auto` + `use calamine::Reader as _`
- Produces:
  - `pub fn office_text(path: &Path, slides: bool) -> AppResult<String>`（`slides=true` 走 pptx 的 `p/slides/slideN.xml` + `<a:t>`，`false` 走 docx 的 `word/document.xml` + `<w:t>`）
  - `pub fn sheet_text(path: &Path) -> AppResult<String>`
  - 私有的 `fn xml_texts(xml: &str, tag: &[u8]) -> AppResult<Vec<String>>`

  两条收尾动作，都是 Task 3 已经踩过的形状（Task 3 的实现者实测出：带 `#[allow(dead_code)]` 的函数会被 rustc 当**额外的可达根**，所以它下游的私有函数不会被连带报）：
  1. **删掉 `fail` 上面那条 `#[allow(dead_code)]`**（`extract.rs` 里注释写着「第一个 caller 在 Task 4 的 Office 分支」那一行）。本任务的 `xml_texts` / `office_text` / `sheet_text` 就是它的 caller，豁免留在原地就变成永久豁免。
  2. `office_text` 与 `sheet_text` 自己要到 **Task 5** 的 `extract_text` 分派才有 caller，`mod extract` 是私有模块、`pub` 救不了 `dead_code`，所以这两条各带一点名 Task 5 的 `#[allow(dead_code)]`，**Task 5 落地时删掉**。`xml_texts` 只被 `office_text` 调用，按上面第 1 条实测出的规律不会单独被报；真被报了才加，并在报告里点名。

- [ ] **Step 1: 写失败测试（4 条）**

测试模块顶部除了 `use super::*;` 还需要**两行**，本任务才编译得过（Task 3 之后 `extract.rs` 顶层只有 `use std::io::Read;` 和 `use std::path::Path;`，`use super::*` 带不进这两个）：`use std::io::Write;`（`ZipWriter::write_all` 是 `Write` 的方法，缺它会报 E0599 method not found）和 `use std::path::PathBuf;`（`make_office` 的返回类型，缺它是 E0412/E0425）。这两行是测试专用，别提到模块顶层 —— 和 Task 1 的 `params`、Task 2 的 `rusqlite::params` 同一个理由：`cargo clippy --lib` 不带 test cfg，顶层引入会被判 unused。

先在 tests 里放一个「造真 zip 部件」的 helper（`a3`/`a5` 探针验证过的写法，照抄）；pptx 与 xlsx 两条测试各按自己的形状直接落文件，不复用它（pptx 要写三个 slide 部件，xlsx 得用 `rust_xlsxwriter`）：

```rust
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
```

四条测试：

```rust
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
            // 这里不能写 `&format!(...)`：`start_file` 的形参是 `S: ToString`，
            // 借用在 `cargo clippy --lib --all-targets -- -D warnings` 上被判
            // needless_borrows_for_generic_args（Task 4 实测：error: the borrowed expression
            // implements the required traits），直接把 String 传进去。
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib extract::tests::docx`
Expected: `cannot find function office_text` / `sheet_text`

- [ ] **Step 3: 写实现**

```rust
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
#[allow(dead_code)] // caller 是 Task 5 的 extract_text 分派，落地时删掉本行
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
#[allow(dead_code)] // caller 是 Task 5 的 extract_text 分派，落地时删掉本行
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
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib extract`
Expected: `8 passed`（Task 3 的 4 条 + 本任务 4 条）；全量 → `58 passed; 0 failed`

- [ ] **Step 5: clippy + 提交**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings && cargo clippy --lib --all-targets -- -D warnings
```
Expected: 两道都零告警。第一道专查上面两条新豁免够不够（`fail` 的那条删掉后不能反而变红）；第二道专查测试模块里的 helper 与断言。

```bash
git add src-tauri/src/extract.rs
git commit -m "feat: M3 Office 抽取：docx/pptx 共用事件循环并还原实体，xlsx 走 calamine"
```

---

## Task 5: `extract.rs`（三）—— PDF + 扩展名分派入口

**Files:**
- Modify: `src-tauri/src/extract.rs`（加 `DocKind`、`pdf_text`、`extract_text`）
- Test: 同文件 `mod tests`；fixture 用已生成的 `src-tauri/tests/fixtures/sample-cn.pdf`（150225 B，Edge 打印，事实 19）与其源文件 `pdf-source.html`

**Interfaces:**
- Consumes: `pdf_extract::extract_text(path)`、`std::panic::catch_unwind(AssertUnwindSafe(..))`、Task 3/4 的 `read_text_file` / `office_text` / `sheet_text`
- Produces:
  - `pub enum DocKind { Text, Word, Slides, Workbook, Pdf }`
  - `pub fn kind_of(ext: &str) -> Option<DocKind>`（大小写不敏感）
  - `pub fn supported_exts() -> &'static [&'static str]`
  - `pub fn extract_text(path: &Path) -> AppResult<String>`（`Ok(空串)` = 无正文，`Err` = 抽不出）

**派发前必须做的死代码账（本任务是把这条链收口的那个任务）：**

- **删掉三条豁免**：`read_text_file`、`office_text`、`sheet_text` 上各带一条 `#[allow(dead_code)] // ... Task 5 ...`，本任务的 `extract_text` 就是它们的第一处调用点，三条都必须删干净（注释里点名过这个责任）。
- **新增两条豁免**：`cargo clippy --lib` 不带 `--all-targets`，不编译 `#[cfg(test)]`，所以本任务新写的 `extract_text` 与 `supported_exts` 在**只有测试调用**它们的构建里就是死的。各加一条 `#[allow(dead_code)]`，`extract_text` 注明「caller 在 Task 9 的 `index_job`」、`supported_exts` 注明「caller 在 Task 10 的 IPC」，落地时删掉本行。
  `kind_of`、`pdf_text`、`DocKind`、`TEXT_EXTS`、`SUPPORTED` 不用加：rustc 把带 `allow` 的函数当**额外的可达根**（Task 3 实测确认），前两者由 `extract_text` 抵达，`SUPPORTED` 由 `supported_exts` 抵达。若闸上报出别的函数名，同批加豁免并在报告里点名。

**本机已实测的 pdf-extract 行为（事实 18 的一手证据，控制方 2026-09-30 用一次性探针跑过，探针未提交）：**

- `sample-cn.pdf` → `Ok`，**86 个字符**，同时含「验收」与「维保」。`> 40` 的门槛留了一倍余量，别把它改成贴着 86 的数。
- 假头 `%PDF-1.4 not a real pdf body` → `Err`（`failed parsing cross reference table: invalid start value`）。
- 截断到 1/3 → 同一个 `Err`。空文件 → `Err`（`couldn't parse input: invalid file header`）。
- 四种输入**都没 panic**，所以 `catch_unwind` 的 `Err(_)` 分支在本机仍不可构造 —— 实现里那句「已知缺口，不要当成已证」的注释要保持这个口径，别写成「已验证能兜住 panic」。
- 签名已核：`pub fn extract_text<P: AsRef<Path>>(path: P) -> Result<String, OutputError>`（`pdf-extract-0.12.1/src/lib.rs:2219`），`OutputError` 有 `Display`(:43) 与 `Error`(:54)，所以 `format!("PDF 抽取失败：{e}")` 直接可用，不需要 `to_string()`。

- [ ] **Step 1: 写失败测试（5 条）**

```rust
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib extract::tests::real_pdf`
Expected: `cannot find function extract_text` / `kind_of`

- [ ] **Step 3: 写实现**

```rust
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

#[allow(dead_code)] // caller 在 Task 10 的 IPC（UI 要念这张表），落地时删掉本行
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
/// 这里的 catch_unwind 防的是 panic 分支，那条分支在本机无法构造样本 —— 属已知缺口，不要当成已证。
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
#[allow(dead_code)] // caller 在 Task 9 的 index_job，落地时删掉本行
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
```

`TEXT_EXTS` 与 `SUPPORTED` 是两张表，Step 1 的 `supported_exts_list_matches_the_dispatch_table` 就是钉住它们不漂移的闸门。

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib extract`
Expected: `13 passed`；全量 → `63 passed; 0 failed`

计数由控制方当场数过，不是推算：本任务动手前 `grep -c "#\[test\]" src/extract.rs` = 8、全 crate 求和 = 58；落地后（含修订轮新增的第 5 条）应为 **13 / 63**，即本任务净新增 5 条。**不要为了让数字对上而随意增删 `#[test]`** —— 只有本任务写的这 5 条，多一条少一条都要回去核对。（修订原因见 Step 1 里 `every_dispatch_arm_routes_to_its_own_extractor` 的文档注释：五个分派臂此前在本计划里没有任何一条端到端走过 `extract_text`，Task 9 的测试只喂 `.txt`。）

- [ ] **Step 5: 跑两道 clippy 闸**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings && cargo clippy --lib --all-targets -- -D warnings
```

Expected: 两道都 exit 0。这一道闸专门用来验证「删三条豁免 + 加两条豁免」的账算对了：`--lib` 会当场暴露漏加豁免的 `extract_text`/`supported_exts`，也会暴露漏删的旧豁免（旧豁免不会红，但会让 Task 9/10 的调用点落地后变成永久豁免，所以必须删）。

- [ ] **Step 6: 提交（含 fixture）**

```bash
git add src-tauri/src/extract.rs src-tauri/tests/fixtures/pdf-source.html src-tauri/tests/fixtures/sample-cn.pdf
git commit -m "feat: M3 PDF 抽取与扩展名分派入口，附真中文 PDF fixture"
```

**这条 `git add` 之前不能省一步核对**：本机 `core.autocrlf=true` 且仓库里没有 `.gitattributes`，把一个 150 KB 的 PDF 交给 git 是有被换行转换改坏的风险的（改坏的 PDF 表现是「 fixture 在别人机器上抽不出正文」，而不是提交时报错）。控制方已实测：该文件前 8000 字节里有 NUL，git 的二进制探测据此判它不是文本，`autocrlf` 不会动它。落地后用 `git ls-files --eol src-tauri/tests/fixtures/sample-cn.pdf` 复核一次，期望看到 `i/-text`（二进制）；若是 `i/lf` 或 `i/crlf` 说明判成了文本，**停下来报告**，不要自己加 `.gitattributes`（那属于仓库级配置，需另行裁定）。

`git add` 只列这三个文件，**不要用 `git add -A`**：`src-tauri/tests/` 目前是未跟踪目录，整目录加会把无关文件一起带进来。

```bash
git add src-tauri/src/extract.rs src-tauri/tests/fixtures/pdf-source.html src-tauri/tests/fixtures/sample-cn.pdf
git commit -m "feat: M3 PDF 抽取与扩展名分派入口，附真中文 PDF fixture"
```

---

## Task 6: `index_scan.rs` —— 排除规则、上限、只读遍历

**Files:**
- Create: `src-tauri/src/index_scan.rs`
- Modify: `src-tauri/src/lib.rs`（`mod index_scan;`）
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: `walkdir::WalkDir`（**没有 `WalkBuilder`**，事实 20）、`crate::extract::kind_of`、`rusqlite::Connection`（读 `settings`，区分「无行」与「查询失败」要用 `rusqlite::OptionalExtension`）、`crate::error::AppResult`（`From<rusqlite::Error>` 已经把库错误映射成码 `db_failed`，见 `error.rs:42-50`，不要另造错误码）。只 `use kind_of`：`supported_exts` 不进本任务的代码，写成 `use crate::extract::{kind_of, supported_exts};` 会撞 `unused_imports`（它的 caller 在 Task 10）。
- Produces:
  - `pub struct ScanOptions { pub exclude_dirs: Vec<String>, pub max_file_bytes: u64, pub max_files_per_project: i64 }`
  - `impl ScanOptions { pub fn load(conn: &Connection) -> AppResult<ScanOptions> }`
  - `pub struct ScannedFile { pub path: String, pub file_name: String, pub ext: String, pub size: u64, pub mtime: i64 }`
  - `pub struct ScanOutcome { pub files: Vec<ScannedFile>, pub over_size: Vec<ScannedFile>, pub walk_errors: Vec<String>, pub capped: bool }`
  - `pub fn scan_root(root: &Path, opts: &ScanOptions) -> ScanOutcome`

**派发前必须做的死代码账**（控制方已用一次性探针实测，探针文件与 `lib.rs` 的 `mod _probe;` 均已删净；实现者照下面的结论落，不要重新发明）：

探针形态：`src-tauri/src/lib.rs` 里用 `mod index_scan;`（**私有模块**，沿用现有 8 条 `mod` 的写法，不要为了绕 lint 改成 `pub mod`），把 Step 3 的实现原文放进去、不带 `mod tests`，跑 `cargo check --lib`。

- **事实 A** —— 一条豁免都不加时报 6 条：`ScanOptions`/`ScannedFile`/`ScanOutcome` 三个 `struct is never constructed`，加 `ScanOptions::load`、`scan_root` 两条 `never used`，再加 `to_scanned`。
- **事实 B** —— 只给 `scan_root` 加 `#[allow(dead_code)]`，`to_scanned` 与三个 struct 的告警**全部消失**。豁免项自己就是可达根，下游不逐个加（Task 5 记过同一条规律，本模块再验一次）。
- **事实 C** —— 同一次探针新报 `fields `path`, `file_name`, and `mtime` are never read`（挂在 `ScannedFile`，探针行号 37）。本模块只读 `ext` 与 `size`，那三条要等 Task 7 的 `write_doc`（`file.path` / `index_text(&file.file_name)` / `file.mtime` 都在它的 INSERT 里）。
- **事实 D** —— `ScanOutcome` 的四个字段**都不报**，包括只赋值不收的 `capped`。计划先前那条「assign-only 字段算不算未读」的待实测项到此有答案：**不算**，别给它加豁免。
- **结论**：本模块要落的豁免共 3 个位置（`scan_root`、`ScanOptions::load` 各一条，caller 在 Task 9；`ScannedFile` 的 `path`/`file_name`/`mtime` 三条字段级，Task 7 落地时删）。用字段级而不是 struct 级，是因为 `ext`/`size` 现在真的被读，struct 级豁免会把它们一起罩住，Task 7 若漏读就没人报警。
- **探针顺带证伪了两条预检判断**：`unnecessary_wraps` **不会**打在计划原来那版只回 `Ok` 的 `load` 上，`while_let_on_iterator` **不会**打在 `while let Some(entry) = it.next()` 上。本模块只有 dead_code 这一类闸要过。
- **`load` 仍要改写，理由不是 lint**（控制方裁定）：原文 `unwrap_or_else(|_| default.to_owned())` 把「库读失败」和「设置行不存在」压成同一条路径，坏库/锁库时静默按 20 MB 与 50000 跑完整轮，用户看不到任何信号 —— 与本仓库对 `journal_mode` 的处理是同一条红线：能区分就必须区分。改法见 Step 3，钉住它的是 `settings_query_failure_propagates_instead_of_defaulting`。控制方实测：新版三条测试全绿、两道 clippy 闸 exit 0；把 `get` 改回吞错误的写法，该测试立刻红在 `called `Result::unwrap_err()` on an `Ok` value: ScanOptions { ..., max_file_bytes: 20971520, ... }`。
- **不要顺手把「数字解析失败」也改成 Err**：M3 没有任何写这三行的入口（Task 11 只读展示），做成 Err 要连 Task 10/11 的设置写入口与校验一起动，超出本任务范围。保留 `unwrap_or` 默认值，并在注释里写明这是有意为之。
- **Step 1 原文有两处本机跑不通，已由 Task 6 落地后回修计划本体**（实现者 `20add29` 报的 concern，控制方裁定：接受，且这是计划文本的错而不是实现的错）：① `paths.iter().any(|p| p == "dep.js")` —— `paths: Vec<&str>` 的 `iter()` 给出 `&&str`，与字面量 `"dep.js"` 比较撞 3 条 E0277、整个测试模块不编译，正字是 `|p| *p == "dep.js"`；② `&vec![b'a'; 100]` 在 `cargo clippy --lib --all-targets -- -D warnings` 上被 `useless_vec` 拦下，正字是 `&[b'a'; 100]`（数组直接 coerce 成 slice，不需要 vec）。两处都是改一个 token、断言强度零变化。

- [ ] **Step 1: 写失败测试（10 条）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const MAX: u64 = 20 * 1024 * 1024;

    fn opts(exclude: &[&str]) -> ScanOptions {
        ScanOptions {
            exclude_dirs: exclude.iter().map(|s| s.to_string()).collect(),
            max_file_bytes: MAX,
            max_files_per_project: 50000,
        }
    }

    fn touch(dir: &Path, rel: &str, bytes: &[u8]) {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, bytes).unwrap();
    }

    fn tree(dir: &Path) {
        touch(dir, "合同/验收说明.docx", b"x");
        touch(dir, "合同/node_modules/dep.js", b"y");          // 排除目录内部
        touch(dir, "交付/Node_Modules/index.js", b"z");        // 大小写不同，也要排除
        touch(dir, "dist/bundle.js", b"w");                     // 排除目录本身
        touch(dir, "readme.txt", b"hello");
        touch(dir, "图片/logo.png", b"\x89PNG\r\n\x1a\n");      // 支持清单外，不建行
        touch(dir, "报价.xlsx.zip", b"PK");                     // 压缩包，不建行
    }

    /// 排除目录要在「进入之前」剪掉：事实 20 的 skip_current_dir。
    #[test]
    fn excluded_dirs_are_pruned_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        tree(dir.path());
        let out = scan_root(dir.path(), &opts(&["node_modules", "dist"]));
        let paths: Vec<&str> = out.files.iter().map(|f| f.file_name.as_str()).collect();
        assert!(paths.contains(&"readme.txt") && paths.contains(&"验收说明.docx"), "{paths:?}");
        assert!(!paths.iter().any(|p| *p == "dep.js" || *p == "index.js" || *p == "bundle.js"),
            "排除目录里的文件不该出现：{paths:?}");
    }

    /// 未支持类型完全不建行（几十 GB 里图片/二进制占大头，全建行会把表撑爆）。
    /// 「为什么这个文件搜不到」由 /index 页面上的支持清单回答，见 Task 11。
    #[test]
    fn unsupported_extensions_produce_no_rows() {
        let dir = tempfile::tempdir().unwrap();
        tree(dir.path());
        let out = scan_root(dir.path(), &opts(&[]));
        assert!(!out.files.is_empty(), "整棵树什么都没扫出来时，下面那条 all() 是空转的真");
        assert!(out.files.iter().all(|f| f.ext != "png" && f.ext != "zip"), "{:?}",
            out.files.iter().map(|f| &f.ext).collect::<Vec<_>>());
    }

    #[test]
    fn oversize_supported_files_are_separated_not_dropped() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "大文件.txt", &[b'a'; 100]);
        touch(dir.path(), "小文件.txt", b"ok");
        let mut o = opts(&[]);
        o.max_file_bytes = 50;
        let out = scan_root(dir.path(), &o);
        assert_eq!(out.files.iter().map(|f| f.file_name.as_str()).collect::<Vec<_>>(), vec!["小文件.txt"]);
        assert_eq!(out.over_size.iter().map(|f| f.file_name.as_str()).collect::<Vec<_>>(), vec!["大文件.txt"],
            "超限文件要单独回传，好让它落成 skipped + too_large 而不是无声消失");
    }

    #[test]
    fn per_project_cap_stops_the_walk_and_flags_capped() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..25 {
            touch(dir.path(), &format!("批次/文件{i}.txt"), b"x");
        }
        let mut o = opts(&[]);
        o.max_files_per_project = 10;
        let out = scan_root(dir.path(), &o);
        assert_eq!(out.files.len(), 10, "触顶后不该继续收：{}", out.files.len());
        assert!(out.capped, "触顶必须显式标记，否则用户以为项目就这么点文件");
    }

    /// 配额要盖住超限那一桶：一个「全是 30 万个超限大文件」的项目，按原顺序走，上限根本不生效
    /// —— 走完整棵树、在内存里攒几十万个 ScannedFile，Task 7 还要逐条落 too_large 行。
    /// 这条断的是两桶之和的总量，所以 `files`/`over_size` 各自几条不重要，重要的是不许超过 cap。
    #[test]
    fn oversize_files_consume_the_per_project_quota() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..12 {
            touch(dir.path(), &format!("图纸/图{i}.txt"), &[b'a'; 10]);
        }
        let mut o = opts(&[]);
        o.max_file_bytes = 5; // 12 个文件全部超限
        o.max_files_per_project = 10;
        let out = scan_root(dir.path(), &o);
        assert!(out.files.is_empty(), "全部超限不该有可抽取行：{}", out.files.len());
        assert_eq!(out.over_size.len(), 10, "超限行也要撞配额就停：收了 {}", out.over_size.len());
        assert!(out.capped, "只有超限文件的项目触顶同样要标记");
    }

    /// 根目录自己的名字命中排除清单时不许剪掉整棵树：少掉 `entry.depth() > 0` 会得到
    /// 「0 个文件、0 条 walk_errors、capped=false」的静默空索引，用户完全看不出原因。
    #[test]
    fn a_root_named_like_an_excluded_dir_is_still_scanned() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("build"); // 用户把项目根指到了一个名叫 build 的目录
        touch(&root, "交付/验收说明.docx", b"x");
        let out = scan_root(&root, &opts(&["node_modules", "dist", "build"]));
        assert_eq!(
            out.files.iter().map(|f| f.file_name.as_str()).collect::<Vec<_>>(),
            vec!["验收说明.docx"],
            "根目录名不该在 depth 0 触发剪枝：walk_errors={:?}",
            out.walk_errors
        );
        assert!(!out.capped);
    }

    /// 根目录不存在（移动盘没挂载）是常态：回 walk_errors，不 panic、不返回 Err。
    #[test]
    fn missing_root_is_reported_instead_of_panicking() {
        let out = scan_root(Path::new("Z:/一定不存在/的根目录"), &opts(&[]));
        assert!(out.files.is_empty());
        assert!(!out.walk_errors.is_empty(), "不可达根目录要留下痕迹：{:?}", out.walk_errors);
    }

    /// 上限要真的能从 settings 改出来，这条同时钉住三件事：键名没写错、逗号的 trim 与空段处理、
    /// 数字解析。种子值和 spec 默认值字节相同，所以「读回种子值等于默认值」那种写法守不住键名笔误
    /// —— 键名写错了照样退默认值、照样绿，所以这里先把值改成与默认值不同再断言。
    #[test]
    fn load_reads_the_settings_rows_not_the_hardcoded_defaults() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("UPDATE settings SET value = '12345' WHERE key = 'index_max_file_bytes'", [])
            .unwrap();
        conn.execute("UPDATE settings SET value = '7' WHERE key = 'index_max_files_per_project'", [])
            .unwrap();
        conn.execute("UPDATE settings SET value = 'x, y,,cache' WHERE key = 'index_exclude_dirs'", [])
            .unwrap();
        let o = ScanOptions::load(&conn).unwrap();
        assert_eq!(o.max_file_bytes, 12345);
        assert_eq!(o.max_files_per_project, 7);
        assert_eq!(o.exclude_dirs, vec!["x".to_owned(), "y".to_owned(), "cache".to_owned()]);
    }

    /// 行缺失（老库没跑过 v4 的迁移、或用户手动删过）才退 spec 默认值。
    #[test]
    fn missing_setting_row_falls_back_to_spec_default() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("DELETE FROM settings WHERE key = 'index_max_file_bytes'", [])
            .unwrap();
        let o = ScanOptions::load(&conn).unwrap();
        assert_eq!(o.max_file_bytes, 20_971_520, "只有「无行」这一种情况该退默认值");
    }

    /// 与上一条配对：查询本身失败（库损坏、被锁、表不在）绝不能退默认值，必须把 Err 交回上层。
    /// 少这条，谁把 load 改回 unwrap_or_else(|_| default) 也不会有任何测试变红 —— 而表现是
    /// 「上限悄悄按 20 MB 跑完一整轮」，正是本仓库在 journal_mode 上拒绝过的静默降级。
    #[test]
    fn settings_query_failure_propagates_instead_of_defaulting() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("DROP TABLE settings", []).unwrap();
        let e = ScanOptions::load(&conn).unwrap_err();
        assert_eq!(e.code, "db_failed", "读库失败要沿用 error.rs 既有的库错误码：{}", e.message);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib index_scan`
Expected: `unresolved module`

- [ ] **Step 3: 写实现**

```rust
//! 扫盘：根目录 → 待索引清单。这一层只读文件系统的元数据，绝不打开文件内容，
//! 更不写任何东西 —— 「只登记路径、不动源文件」的红线在这一层最容易被踩穿。

use std::path::Path;

use rusqlite::Connection;

use crate::error::AppResult;
use crate::extract::kind_of;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub exclude_dirs: Vec<String>,
    pub max_file_bytes: u64,
    pub max_files_per_project: i64,
}

impl ScanOptions {
    /// 「设置行不存在」退回 spec 默认值；「查询失败」是另一回事，必须原样抛出去。
    /// 两者都用 `unwrap_or_else` 吞掉的写法见过一次，坏库时会静默按 20 MB 上限跑完整轮。
    #[allow(dead_code)] // caller 在 Task 9 的 index_job，落地时删掉本行
    pub fn load(conn: &Connection) -> AppResult<ScanOptions> {
        use rusqlite::OptionalExtension;
        let get = |key: &str, default: &str| -> AppResult<String> {
            let row: Option<String> = conn
                .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
                .optional()?;
            Ok(row.unwrap_or_else(|| default.to_owned()))
        };
        // 数字解析失败仍退默认值：M3 没有写这三行的入口（Task 11 只读展示），
        // 要把它做成 Err 得连设置写入口与校验一起动，不在本任务范围。
        let list = get("index_exclude_dirs", "node_modules,dist,build,target,__pycache__,.git")?;
        let bytes = get("index_max_file_bytes", "20971520")?;
        let cap = get("index_max_files_per_project", "50000")?;
        Ok(ScanOptions {
            exclude_dirs: list
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect(),
            max_file_bytes: bytes.parse().unwrap_or(20_971_520),
            max_files_per_project: cap.parse().unwrap_or(50_000),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ScannedFile {
    // 这三条要等 Task 7 的 write_doc 才读（file.path / index_text(&file.file_name) / file.mtime）。
    // 用字段级而不是 struct 级豁免：ext 与 size 本模块真的在读，struct 级会把它们一起罩住。
    #[allow(dead_code)] // reader 在 Task 7 的 write_doc，落地时删掉本行
    pub path: String,
    #[allow(dead_code)] // reader 在 Task 7 的 write_doc，落地时删掉本行
    pub file_name: String,
    pub ext: String,
    pub size: u64,
    #[allow(dead_code)] // reader 在 Task 7 的 write_doc，落地时删掉本行
    pub mtime: i64,
}

#[derive(Debug, Default)]
pub struct ScanOutcome {
    pub files: Vec<ScannedFile>,
    pub over_size: Vec<ScannedFile>,
    pub walk_errors: Vec<String>,
    pub capped: bool,
}

fn to_scanned(entry: &walkdir::DirEntry) -> Option<ScannedFile> {
    let meta = entry.metadata().ok()?;
    Some(ScannedFile {
        path: entry.path().display().to_string(),
        file_name: entry.file_name().to_string_lossy().into_owned(),
        ext: entry
            .path()
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default(),
        size: meta.len(),
        mtime: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    })
}

#[allow(dead_code)] // caller 在 Task 9 的 index_job，落地时删掉本行
pub fn scan_root(root: &Path, opts: &ScanOptions) -> ScanOutcome {
    let mut out = ScanOutcome::default();
    let mut it = walkdir::WalkDir::new(root)
        .follow_links(false) // 链接/软链可能把目录图成环，且指向项目外的内容不该算进本项目
        .max_depth(16)
        .into_iter();

    while let Some(entry) = it.next() {
        match entry {
            Err(e) => {
                // 权限/重解析点失败在 Windows 上是日常，收集起来继续走，一轮不能因此中断。
                out.walk_errors.push(e.to_string());
                continue;
            }
            Ok(entry) => {
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.depth() > 0
                    && entry.file_type().is_dir()
                    && opts.exclude_dirs.iter().any(|d| d.eq_ignore_ascii_case(&name))
                {
                    it.skip_current_dir();
                    continue;
                }
                // `entry.depth() > 0` 不是可有可无的：少了它，用户把项目根指到一个真名叫
                // `build`/`dist`/`target` 的目录时，根会在 depth 0 被剪掉，得到「0 个文件、
                // 0 条错误、没有触顶标记」的静默空索引 —— 正是 capped 这条要求要防的那类。
                if !entry.file_type().is_file() {
                    continue;
                }
                let Some(scanned) = to_scanned(&entry) else {
                    continue;
                };
                if kind_of(&scanned.ext).is_none() {
                    continue; // 未支持类型不建行，也不占配额，见测试里的说明
                }
                // 配额必须打在「分大小两桶之前」：超限文件同样会各落一行 skipped，同样撑表。
                // 放在 oversize 分支之后，上限对「全是超限大文件的项目」根本不生效 ——
                // 走完整棵树、在内存里攒几十万个 ScannedFile，Task 7 还要逐条落 too_large 行。
                if (out.files.len() as i64) + (out.over_size.len() as i64) >= opts.max_files_per_project {
                    out.capped = true;
                    break; // 触顶即停：剩下的文件不进清单，也没有行，由作业摘要点名项目
                }
                if scanned.size > opts.max_file_bytes {
                    out.over_size.push(scanned);
                    continue;
                }
                out.files.push(scanned);
            }
        }
    }
    out
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib index_scan`
Expected: `10 passed`；全量 → `73 passed; 0 failed`

`missing_root_is_reported_instead_of_panicking` 若拿不到 `walk_errors`（walkdir 对不存在的根只吐一条 IOErr，确实会进错误流），就检查是不是 `Z:/` 被解析成了别的形态；**不要**改成断言「空清单即通过」——那会放过「根目录不可达却静默」这个真实故障。

三条 `load` 测试都不该用 `unwrap_or` 之类的容错把它们变松：`settings_query_failure_propagates_instead_of_defaulting` 是本任务唯一守住「读库失败 ≠ 缺设置」这条区分的证据，报告里要给它一条变异取证（把 `get` 改回 `unwrap_or_else(|_| default.to_owned())`，确认它红、并抄下 panic 文案）。

- [ ] **Step 5: 跑两道 clippy 闸**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings && cargo clippy --lib --all-targets -- -D warnings
```

Expected: 两道都 exit 0。`--lib` 那道专门验本模块的死代码账：漏加 `scan_root`/`load` 任一条豁免就当场红；给 `ScanOutcome`（含 `capped`）或给 struct `ScannedFile` 整体加豁免**不会**红，但那是把 `ext`/`size` 一起罩住的过度豁免，按上面的账删到只剩字段级三条。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/index_scan.rs src-tauri/src/lib.rs
git commit -m "feat: M3 目录扫描：排除规则剪枝、大小与项目上限、错误不中断"
```

只 `git add` 这两个文件，**不要 `git add -A`**（工作区里可能有别的未跟踪产物）。

---

## Task 7: `index_store.rs`（写侧）—— 两点写、幂等重写、状态统计

**Files:**
- Create: `src-tauri/src/index_store.rs`
- Modify: `src-tauri/src/lib.rs`（`mod index_store;`）
- Modify: `src-tauri/src/index_scan.rs`（删掉 `ScannedFile` 上那三条字段级 `#[allow(dead_code)]`，以及它们上方那两行描述豁免的注释 —— 豁免本轮就删了，注释留着会让下个读者去找三条已经不存在的 `#[allow]`；其余不动）
- Modify: `src-tauri/src/tokenize.rs`（删掉 `index_text` 上那条 `#[allow(dead_code)]`，别的不动）
- Test: `index_store.rs` 同文件 `mod tests`

**Interfaces:**
- Consumes: Task 1 的两张表、Task 2 的 `tokenize::index_text`、Task 6 的 `ScannedFile`、`uuid::Uuid`
- **派发前必须做的死代码账（控制方实测，一次性探针跑完即删）**：`cargo clippy --lib -- -D warnings` 不带 `--all-targets`，不编译 `#[cfg(test)]`，本任务的六个 `pub fn` 一个生产 caller 都没有（`write_doc`/`current_rowid`/`delete_doc`/`clear_project` 到 Task 9 的 `index_job`，`status_counts`/`list_docs` 到 Task 10 的 IPC），探针当场报 **9 条** dead_code：六个函数（含下面裁定移除的那个）+ `enum DocOutcome`（四个变体合成一条 `variants Ok, Empty, Skipped, and Failed are never constructed`）+ `struct StatusCount` + `struct DocRow`。给六个函数各挂一条 `allow` 之后，`StatusCount` 与 `DocRow` 的两条**自己消失**（Task 3/5/6 记过的规律再次成立：allow 项是额外可达根，返回型被顺带罩住）；`DocOutcome` 那条不会消失，因为 lib 侧只有 match 没有构造 —— 必须在**枚举本体**挂一条（探针实测：一条 enum 级 allow 清掉那四个变体的报告，不需要逐变体挂）。本任务实际要落 **6 条豁免**（留下的人：`write_doc`/`current_rowid`/`clear_project`/`status_counts`/`list_docs` + `DocOutcome` 枚举），每条注明接上的任务号。`grep -c "allow(dead_code)" src/index_store.rs` 期望 6。
- **顺带收掉的豁免**：Task 6 在 `ScannedFile` 的 `path`/`file_name`/`mtime` 三条字段上各留了一条 `#[allow(dead_code)] // reader 在 Task 7 的 write_doc`。本任务的 `write_doc` 一旦真的读它们（`file.path`、`index_text(&file.file_name)`、`file.mtime`），把那三行删掉 —— 漏删不会红，只会变成永久豁免，所以 Step 的 clippy 闸之外要再跑一次 `grep -n "allow(dead_code)" src/index_scan.rs` 确认只剩 `scan_root` 与 `load` 两条（它们的 caller 在 Task 9）。
- Produces:
  - `pub enum DocOutcome { Ok(String), Empty, Skipped(&'static str), Failed(String) }`（`Ok` 带正文，其它三种都不写 FTS 行）
  - `pub fn write_doc(conn: &Connection, project_id: &str, file: &ScannedFile, outcome: DocOutcome) -> AppResult<i64>` → 回 `doc_rowid`
  - `pub fn current_rowid(conn, project_id, path, size, mtime) -> AppResult<Option<i64>>`（增量跳过用）
  - `pub fn clear_project(conn, project_id) -> AppResult<u64>`（重建前清场）
  - `pub struct StatusCount { pub status: String, pub count: i64 }`
  - `pub fn status_counts(conn, project_id: &str) -> AppResult<Vec<StatusCount>>`
  - `pub struct DocRow { pub id: String, pub path: String, pub ext: String, pub size: i64, pub status: String, pub skip_reason: Option<String>, pub error_msg: Option<String>, pub indexed_at: Option<String> }`
  - `pub fn list_docs(conn, project_id: &str, status: Option<&str>, limit: i64, offset: i64) -> AppResult<Vec<DocRow>>`

- **顺带收掉的豁免（第二条）**：`tokenize.rs:27` 的 `index_text` 上挂着一条 `#[allow(dead_code)] // 生产调用点在 Task 7（写入侧）/ Task 8（检索侧）`。本任务 `write_doc` 里的 `index_text(&file.file_name)` 与 `index_text(&body)` **就是它的第一个非测试调用点**，这条豁免必须在本任务删掉（可达性依据同 `extract.rs`：`kind_of`/`pdf_text` 没挂豁免，靠带豁免的 `extract_text` 抵达，今天两道闸是绿的）。提交前跑 `grep -c "allow(dead_code)" src/tokenize.rs` 期望从 **3 降到 2**（剩下 `query_expression`、`clean_snippet` 归 Task 8）。
- **裁定：本任务不产出 `delete_doc`**（控制方派发前核出的计划缺陷，理由是「豁免必须有落点」）。原 Interfaces 列了它，但整份计划里**没有任何生产调用点**：Task 9 的 `run_pass` 只用 `clear_project`/`current_rowid`/`write_doc`，Task 10 的 IPC 只 Consumes `doc_hits`/`list_docs`/`status_counts`；而「库里有一行、磁盘上没有了」的 `missing` 判定按 Task 9 Interfaces 的既有裁定归 M5 的启动对账。留着一个全 M3 无人调用的 `pub fn`，它的 `#[allow(dead_code)]` 就没有删掉的那天 —— 与本仓库对 `over_project_cap` 的裁定同形（宁可删掉宣称，不留假的可观测性）。代价：M5 落地对账时重新补这十行和一条测试。**「删主表不删虚表会留孤儿行」这条不变量不会因为少这个函数而失去守护** —— `clear_project` 走的是同一处两点删，测试照钉，且改成跨项目对照后它还多钉了一件事：清场不能顺手把别的项目清掉。
- **按 Task 7 评审补的三处（评审 Important 1 + Minor 2/3/5，控制方裁定纳入本任务）**：
  ① `indexed_at` 原来**零断言**（`grep -n indexed_at` 只命中实现行）—— `CASE WHEN ?7 = 'ok'` 那支一旦和 `status` 的绑定漂移，7 条测试全绿而 Task 10 的清单里索引时间整列为空，正是本项目要消灭的「不报错、只是看不见」。改法不加第 8 条测试（不动聚合链）：在第 1 条里断 ok 行 `indexed_at.is_some()`，把第 3 条里 `C:/d.docx` 的三元组查询带上 `indexed_at` 并断 `None`。
  复审把这条判为 ADDRESSED，同时指出**残余的一半**：`indexed_at = excluded.indexed_at`（DO UPDATE 那支）
  当时仍无断言 —— 删掉它 7 条全绿，而 Task 9 真会走到「上次 skipped/failed、这次抽出正文」的重写路径，
  结果是**一行可搜索却永远没有索引时间**的行，正是这条 finding 要防的形态。故第 3 条末尾追加一次
  `C:/d.docx` 的 ok 重写并断 `indexed_at.is_some()`（仍不加第 8 条测试）。
  ② `status_counts` 补 `ORDER BY index_status`：返回型直接喂 Task 10 的 IPC，Group By 的默认顺序是实现细节，Task 11 的状态列表会跟着查询计划漂。测试用 `contains` 断言，不依赖顺序，加了不会红。
  ③ 模块 doc 补第 3 条不变量：本模块的写函数**自己开事务**，调用方不得再包一层（`unchecked_transaction()` 跳过重入检查，外层 `commit()` 会被内层提前触发）。Task 9 逐文件调 `write_doc`，这条写在接口上比等它运行时炸。
- **顺带收掉的注释**：`index_scan.rs:49-50` 那两行「这三条要等 Task 7 的 `write_doc` 才读……用字段级而不是 struct 级豁免」描述的正是本轮删掉的三条豁免，改成过去式不如直接删（本仓库默认不写注释，这条 WHY 已不成立）。Files 清单已同步。

- [ ] **Step 1: 写失败测试（7 条）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, index_scan::ScannedFile};
    use rusqlite::{params, Connection};

    fn conn() -> Connection {
        db::open_in_memory().unwrap()
    }

    fn seed_project(conn: &Connection, id: &str) {
        conn.execute("INSERT INTO projects (id, name, status) VALUES (?1, ?2, '立项')", params![id, "政务云迁移"])
            .unwrap();
    }

    fn file(path: &str) -> ScannedFile {
        ScannedFile {
            path: path.to_owned(),
            file_name: path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned(),
            ext: "docx".to_owned(),
            size: 1024,
            mtime: 1_700_000_000,
        }
    }

    /// 事实 3：主表和 FTS 是两处存储，删一边不会带动另一边。写入必须在一个事务里两点落。
    #[test]
    fn write_doc_lands_on_both_sides_and_is_searchable() {
        let c = conn();
        seed_project(&c, "p1");
        let rowid = write_doc(&c, "p1", &file("C:/x/合同验收.docx"), DocOutcome::Ok("甲方要求验收指标".into())).unwrap();
        let id: String = c
            .query_row("SELECT id FROM index_docs WHERE doc_rowid = ?1", params![rowid], |r| r.get(0))
            .unwrap();
        assert!(!id.is_empty());
        let hit: i64 = c
            .query_row(
                "SELECT count(*) FROM index_docs_fts f JOIN index_docs d ON d.doc_rowid = f.rowid
                  WHERE index_docs_fts MATCH ?1 AND d.id = ?2",
                params!["\"验收\"", id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, 1);
        let indexed_at: Option<String> = c
            .query_row("SELECT indexed_at FROM index_docs WHERE doc_rowid = ?1", params![rowid], |r| r.get(0))
            .unwrap();
        assert!(indexed_at.is_some(), "ok 行必须带索引时间，Task 10 的清单要能回答「什么时候建的索引」");
    }

    /// 事实 21：同路径重写不换行、不留重复 FTS 行（rowid 与对外 id 都稳定 + 点删再插）。
    ///
    /// 「旧正文不该还能搜到」这条断言用的词**不能出现在文件名里**：`name_tokens` 每次重写都
    /// 由同一个 `file_name` 重新生成，用「验收」这种文件名里就有的词去断「搜不到」，
    /// 永远为 1，断言当场变成装饰（控制方实测：原写法在正确实现下 `left: 1 / right: 0` 直接红）。
    #[test]
    fn rewriting_the_same_path_replaces_instead_of_duplicating() {
        let c = conn();
        seed_project(&c, "p1");
        let f = file("C:/x/合同验收.docx");
        let r1 = write_doc(&c, "p1", &f, DocOutcome::Ok("第一版正文 付款".into())).unwrap();
        let before: i64 = c
            .query_row("SELECT count(*) FROM index_docs_fts WHERE index_docs_fts MATCH ?1", params!["\"付款\""], |r| r.get(0))
            .unwrap();
        assert_eq!(before, 1, "旧正文的词先要真的能搜到，后面那条「搜不到」才不是空转");
        let id_before: String = c
            .query_row("SELECT id FROM index_docs WHERE doc_rowid = ?1", params![r1], |r| r.get(0))
            .unwrap();
        let f2 = ScannedFile { size: 2048, mtime: 1_700_000_999, ..f.clone() };
        let r2 = write_doc(&c, "p1", &f2, DocOutcome::Ok("第二版正文 报价".into())).unwrap();
        assert_eq!(r1, r2, "同一路径重复写入必须复用同一行");
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get::<_, i64>(0)).unwrap(), 1,
            "FTS 侧不该累积历史版本");
        let old: i64 = c
            .query_row("SELECT count(*) FROM index_docs_fts WHERE index_docs_fts MATCH ?1", params!["\"付款\""], |r| r.get(0))
            .unwrap();
        assert_eq!(old, 0, "旧正文不该还能搜到");
        let now: i64 = c
            .query_row("SELECT count(*) FROM index_docs_fts WHERE index_docs_fts MATCH ?1", params!["\"报价\""], |r| r.get(0))
            .unwrap();
        assert_eq!(now, 1, "新正文要能搜到");
        let id_after: String = c
            .query_row("SELECT id FROM index_docs WHERE doc_rowid = ?1", params![r2], |r| r.get(0))
            .unwrap();
        assert_eq!(id_before, id_after, "重写不能换掉对外的 id：Task 8 的 doc_hits 与 Task 10 的 IPC 都拿它当结果标识");
    }

    /// skipped/failed 只留状态行，不能往虚表塞空串：空串行会让 count 统计与 FTS 行数对不上。
    ///
    /// 末尾那条 `DocOutcome::Ok("   ")` 是 Task 5 评审 defer 过来的：扫描型 PDF 抽出来是空串，
    /// Task 9 走的是 `Ok(body)` 这一支，`empty_text` 在这里才是承重项 —— 少了 `.trim()`，
    /// 一具空正文会带着 ok 状态进库，UI 上显示「已索引」而永远搜不到。
    #[test]
    fn non_ok_outcomes_record_status_but_write_no_fts_rows() {
        let c = conn();
        seed_project(&c, "p1");
        write_doc(&c, "p1", &file("C:/a.docx"), DocOutcome::Skipped("too_large")).unwrap();
        write_doc(&c, "p1", &file("C:/b.docx"), DocOutcome::Failed("解包失败".into())).unwrap();
        write_doc(&c, "p1", &file("C:/c.docx"), DocOutcome::Empty).unwrap();
        write_doc(&c, "p1", &file("C:/d.docx"), DocOutcome::Ok("   ".into())).unwrap();
        let blank: (String, Option<String>, Option<String>) = c
            .query_row(
                "SELECT index_status, skip_reason, indexed_at FROM index_docs WHERE path = 'C:/d.docx'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            blank,
            ("skipped".to_string(), Some("empty_text".to_string()), None),
            "空白正文算 empty_text、不算 ok，且不得有索引时间"
        );
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs", [], |r| r.get::<_, i64>(0)).unwrap(), 4);
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        let reason: Option<String> = c
            .query_row("SELECT skip_reason FROM index_docs WHERE path = 'C:/a.docx'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(reason.as_deref(), Some("too_large"), "超限原因要能被 UI 直接回答");
        let err: Option<String> = c
            .query_row("SELECT error_msg FROM index_docs WHERE path = 'C:/b.docx'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(err.as_deref(), Some("解包失败"));
        // skipped 行重写成 ok：`indexed_at = excluded.indexed_at` 这一支必须真的把时间带进来。
        write_doc(&c, "p1", &file("C:/d.docx"), DocOutcome::Ok("甲方要求验收指标".into())).unwrap();
        let upgraded: Option<String> = c
            .query_row("SELECT indexed_at FROM index_docs WHERE path = 'C:/d.docx'", [], |r| r.get(0))
            .unwrap();
        assert!(upgraded.is_some(), "skipped 行重写成 ok 之后必须补上索引时间");
    }

    #[test]
    fn status_counts_group_per_project() {
        let c = conn();
        seed_project(&c, "p1");
        seed_project(&c, "p2");
        write_doc(&c, "p1", &file("C:/1.docx"), DocOutcome::Ok("验收".into())).unwrap();
        write_doc(&c, "p1", &file("C:/2.docx"), DocOutcome::Ok("报价".into())).unwrap();
        write_doc(&c, "p1", &file("C:/3.docx"), DocOutcome::Skipped("too_large")).unwrap();
        write_doc(&c, "p2", &file("D:/1.docx"), DocOutcome::Failed("x".into())).unwrap();
        let counts = status_counts(&c, "p1").unwrap();
        let got: Vec<(String, i64)> = counts.into_iter().map(|s| (s.status, s.count)).collect();
        assert!(got.contains(&("ok".to_string(), 2)), "{got:?}");
        assert!(got.contains(&("skipped".to_string(), 1)), "{got:?}");
        assert!(!got.iter().any(|(s, _)| s == "failed"), "另一个项目的行不该混进来：{got:?}");
    }

    /// 事实 3 的另一半：删的时候两边都要删。`delete_doc` 已按上面的裁定移出本任务，
    /// 这处两点删由 `clear_project` 顶上来钉住。
    #[test]
    fn clear_project_removes_rows_on_both_sides() {
        let c = conn();
        seed_project(&c, "p1");
        seed_project(&c, "p2");
        write_doc(&c, "p1", &file("C:/1.docx"), DocOutcome::Ok("验收".into())).unwrap();
        write_doc(&c, "p1", &file("C:/2.docx"), DocOutcome::Ok("报价".into())).unwrap();
        write_doc(&c, "p2", &file("D:/1.docx"), DocOutcome::Ok("付款".into())).unwrap();
        assert_eq!(clear_project(&c, "p1").unwrap(), 2, "清场回的是本项目被删掉的行数");
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get::<_, i64>(0)).unwrap(), 1,
            "删主表不删虚表会留下孤儿行，命中列表里会出现已消失的文件");
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs WHERE project_id = 'p1'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs WHERE project_id = 'p2'", [], |r| r.get::<_, i64>(0)).unwrap(), 1,
            "清场不能顺手把别的项目一起清掉");
    }

    /// 只数行数证不出 OFFSET 生效：`LIMIT 2 OFFSET 4` 在 5 行数据上只回 1 行，
    /// 而「把 OFFSET 整个删掉」的错误实现照样回 2 行。所以这里断的是**具体是哪两行**。
    /// （控制方实测：原写的 `list_docs(.., 2, 4)` + `assert_eq!(page.len(), 2)` 在正确实现下
    /// 红在 `left: 1 / right: 2`。）
    #[test]
    fn list_docs_filters_by_status_and_pages() {
        let c = conn();
        seed_project(&c, "p1");
        for i in 0..5 {
            let outcome = if i % 2 == 0 {
                DocOutcome::Skipped("too_large")
            } else {
                DocOutcome::Ok(format!("正文{i} 验收"))
            };
            write_doc(&c, "p1", &file(&format!("C:/{i}.docx")), outcome).unwrap();
        }
        let skipped = list_docs(&c, "p1", Some("skipped"), 10, 0).unwrap();
        assert_eq!(skipped.len(), 3, "{:?}", skipped.iter().map(|d| &d.path).collect::<Vec<_>>());
        let page = list_docs(&c, "p1", None, 2, 2).unwrap();
        let got: Vec<String> = page.iter().map(|d| d.path.clone()).collect();
        assert_eq!(got, vec!["C:/2.docx".to_string(), "C:/3.docx".to_string()],
            "分页要按 path 排序真的跳过前两行");
    }

    /// 增量跳过闸门。这个函数没有 caller 之前（Task 9）最容易写漏的是 `index_status = 'ok'`
    /// 那一条：漏了它，上次 failed/skipped 的行会被当成「已经索引过」，用户看到的是一次失败
    /// 之后永久搜不到 —— 正是本项目要消灭的那类静默失效。
    #[test]
    fn current_rowid_gates_the_incremental_skip() {
        let c = conn();
        seed_project(&c, "p1");
        seed_project(&c, "p2");
        let f = file("C:/x/合同验收.docx");
        let rowid = write_doc(&c, "p1", &f, DocOutcome::Ok("正文 付款".into())).unwrap();
        assert_eq!(current_rowid(&c, "p1", &f.path, f.size as i64, f.mtime).unwrap(), Some(rowid),
            "同路径、size 与 mtime 都没变、上次是 ok：这一轮不该再读文件");
        assert_eq!(current_rowid(&c, "p1", &f.path, f.size as i64 + 1, f.mtime).unwrap(), None, "大小变了要重抽");
        assert_eq!(current_rowid(&c, "p1", &f.path, f.size as i64, f.mtime + 1).unwrap(), None, "修改时间变了要重抽");
        assert_eq!(current_rowid(&c, "p2", &f.path, f.size as i64, f.mtime).unwrap(), None,
            "另一个项目的同路径行不能冒充本项目已索引");
        let g = file("C:/x/坏文件.docx");
        write_doc(&c, "p1", &g, DocOutcome::Failed("解包失败".into())).unwrap();
        assert_eq!(current_rowid(&c, "p1", &g.path, g.size as i64, g.mtime).unwrap(), None,
            "上次失败的行必须重试");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib index_store`
Expected: `unresolved module`

- [ ] **Step 3: 写实现**

```rust
//! 索引表的读写。三条不变量：
//! 1. 主表与 FTS 是两处存储，任何增删都要在同一个事务里做两笔（事实 3），否则命中列表里会
//!    出现已经不存在的文件。
//! 2. 只有 `DocOutcome::Ok` 会往虚表写东西。skipped/failed 只留状态行 —— 「为什么搜不到」
//!    的答案在行上，不在正文里，也就不该出现在搜索结果里。
//! 3. 本模块每个写函数**自己开事务**（`unchecked_transaction()` 直接下 `BEGIN DEFERRED`，
//!    rusqlite 0.40.2 里没有重入检查，所以调用方再包一层会让内层 `BEGIN` 当场失败：
//!    `cannot start a transaction within a transaction` → `db_failed`）。Task 9 的 `run_pass`
//!    逐文件调 `write_doc`，不要在外面套事务，也不要指望「一轮一个事务」的提速。

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

use crate::error::AppResult;
use crate::index_scan::ScannedFile;
use crate::tokenize::index_text;

/// `Ok(String)` 携带抽取到的正文，交给这里决定要不要落 FTS。
#[allow(dead_code)] // 构造点在 Task 9 的 index_job；本任务只有测试构造它，落地时删掉本行
pub enum DocOutcome {
    Ok(String),
    Empty,
    Skipped(&'static str),
    Failed(String),
}

#[allow(dead_code)] // caller 在 Task 9 的 index_job，落地时删掉本行
pub fn write_doc(
    conn: &Connection,
    project_id: &str,
    file: &ScannedFile,
    outcome: DocOutcome,
) -> AppResult<i64> {
    let (status, skip_reason, error_msg, body) = match outcome {
        DocOutcome::Ok(body) => {
            if body.trim().is_empty() {
                ("skipped", Some("empty_text"), None, None)
            } else {
                ("ok", None, None, Some(body))
            }
        }
        DocOutcome::Empty => ("skipped", Some("empty_text"), None, None),
        DocOutcome::Skipped(reason) => ("skipped", Some(reason), None, None),
        DocOutcome::Failed(msg) => ("failed", None, Some(msg), None),
    };

    let tx = conn.unchecked_transaction()?;
    // ON CONFLICT 不动 id / doc_rowid（事实 21）：同一行被重新索引时 rowid 稳定，
    // 于是 FTS 侧可以先按 rowid 点删再插，不需要先查一次。
    tx.execute(
        "INSERT INTO index_docs (id, project_id, path, ext, size, mtime, index_status, skip_reason, error_msg, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, CASE WHEN ?7 = 'ok' THEN datetime('now') ELSE NULL END)
         ON CONFLICT (project_id, path) DO UPDATE SET
             ext          = excluded.ext,
             size         = excluded.size,
             mtime        = excluded.mtime,
             index_status = excluded.index_status,
             skip_reason  = excluded.skip_reason,
             error_msg    = excluded.error_msg,
             indexed_at   = excluded.indexed_at,
             updated_at   = datetime('now')",
        params![
            Uuid::new_v4().to_string(),
            project_id,
            file.path,
            file.ext,
            file.size as i64,
            file.mtime,
            status,
            skip_reason,
            error_msg
        ],
    )?;
    let rowid: i64 = tx.query_row(
        "SELECT doc_rowid FROM index_docs WHERE project_id = ?1 AND path = ?2",
        params![project_id, file.path],
        |r| r.get(0),
    )?;

    // 这一笔是**硬性前提**而不是「免得累积历史版本」的优化：FTS5 对重复 rowid 直接回约束错误。
    // 控制方实测把这句抽掉，第二次写同路径当场 Err（rusqlite 文案 `constraint failed` → AppError `db_failed`）。
    tx.execute("DELETE FROM index_docs_fts WHERE rowid = ?1", params![rowid])?;
    if let Some(body) = body {
        tx.execute(
            "INSERT INTO index_docs_fts (rowid, name_tokens, body_tokens) VALUES (?1, ?2, ?3)",
            params![rowid, index_text(&file.file_name), index_text(&body)],
        )?;
    }
    tx.commit()?;
    Ok(rowid)
}

/// 增量跳过：同路径且 size + mtime 都没变、上次是 ok，就不再读文件。
/// 返回 None 表示需要（重新）抽取。
#[allow(dead_code)] // caller 在 Task 9 的增量跳过分支，落地时删掉本行
pub fn current_rowid(
    conn: &Connection,
    project_id: &str,
    path: &str,
    size: i64,
    mtime: i64,
) -> AppResult<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT doc_rowid FROM index_docs
              WHERE project_id = ?1 AND path = ?2 AND size = ?3 AND mtime = ?4 AND index_status = 'ok'",
            params![project_id, path, size, mtime],
            |r| r.get(0),
        )
        .optional()?)
}

#[allow(dead_code)] // caller 在 Task 9 的重建前清场，落地时删掉本行
pub fn clear_project(conn: &Connection, project_id: &str) -> AppResult<u64> {
    let tx = conn.unchecked_transaction()?;
    let rowids: Vec<i64> = tx
        .prepare("SELECT doc_rowid FROM index_docs WHERE project_id = ?1")?
        .query_map(params![project_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for rowid in &rowids {
        tx.execute("DELETE FROM index_docs_fts WHERE rowid = ?1", params![rowid])?;
    }
    let n = tx.execute("DELETE FROM index_docs WHERE project_id = ?1", params![project_id])? as u64;
    tx.commit()?;
    Ok(n)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

#[allow(dead_code)] // caller 在 Task 10 的状态统计 IPC，落地时删掉本行
pub fn status_counts(conn: &Connection, project_id: &str) -> AppResult<Vec<StatusCount>> {
    let mut stmt = conn.prepare(
        "SELECT index_status, count(*) FROM index_docs WHERE project_id = ?1 GROUP BY index_status
                  ORDER BY index_status",
    )?;
    let rows = stmt.query_map(params![project_id], |r| {
        Ok(StatusCount { status: r.get(0)?, count: r.get(1)? })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocRow {
    pub id: String,
    pub path: String,
    pub ext: String,
    pub size: i64,
    pub status: String,
    pub skip_reason: Option<String>,
    pub error_msg: Option<String>,
    pub indexed_at: Option<String>,
}

#[allow(dead_code)] // caller 在 Task 10 的文件清单 IPC，落地时删掉本行
pub fn list_docs(
    conn: &Connection,
    project_id: &str,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<Vec<DocRow>> {
    // status 走参数而不是拼进 SQL：这一层暴露给 IPC，任何字符串拼接都是注入面。
    let sql = "SELECT id, path, ext, size, index_status, skip_reason, error_msg, indexed_at
                 FROM index_docs
                WHERE project_id = ?1 AND (index_status = ?2 OR ?2 IS NULL)
                ORDER BY path
                LIMIT ?3 OFFSET ?4";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![project_id, status, limit, offset], |r| {
        Ok(DocRow {
            id: r.get(0)?,
            path: r.get(1)?,
            ext: r.get(2)?,
            size: r.get(3)?,
            status: r.get(4)?,
            skip_reason: r.get(5)?,
            error_msg: r.get(6)?,
            indexed_at: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
```

引入已经写在上面的代码里，三条要点：
- `use rusqlite::OptionalExtension;` 是 `.optional()` 的 trait，缺它 `current_rowid` 编译不过；`use uuid::Uuid;` 给 `Uuid::new_v4()`。
- 原计划写的 `Transaction` **用不到**（`unchecked_transaction()` 的返回型靠类型推导即可），留着会在 `cargo clippy --lib -- -D warnings` 上被判 unused import；控制方探针已按此改法实测两道闸 exit 0。
- 原计划写的 `use crate::error::{AppError, AppResult};` 里 **`AppError` 没有任何引用点**（`?` 走的是 `From<rusqlite::Error> for AppError` 这个 impl，不是名字），同样会被判 unused import，已在上面改成只引 `AppResult`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib index_store`
Expected: `7 passed`；全量 → `80 passed; 0 failed`（基线 73 + 本任务 7，控制方探针实测）

- [ ] **Step 5: 跑两道 clippy 闸 + 豁免对账**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings && cargo clippy --lib --all-targets -- -D warnings
grep -n "allow(dead_code)" src/index_scan.rs
grep -c "allow(dead_code)" src/index_store.rs
```

Expected：两道闸 exit 0；`index_scan.rs` 只剩 `scan_root` 与 `load` 两条（Task 6 移交的那三条字段豁免必须已删）；`index_store.rs` 恰好 **6** 条。漏删不会红，只会变成永久豁免，所以这两条 grep 是提交前的硬检查，不是可选项。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/index_store.rs src-tauri/src/lib.rs src-tauri/src/index_scan.rs src-tauri/src/tokenize.rs
git commit -m "feat: M3 索引写入层：两点写、同路径复用 rowid、状态与分页查询"
```

**四个文件一起提交**：`index_scan.rs` 与 `tokenize.rs` 这次各删几行豁免，漏 add 就等于把改动丢在工作树里。`git status` 复核只应有这四个文件（禁止 `git add -A`）。

---

## Task 8: `index_store.rs`（检索侧）—— 两段 FTS 查询 + 「密文不进索引」的验收

**Files:**
- Modify: `src-tauri/src/index_store.rs`（新增 `doc_hits`）
- Test: 同文件 `mod tests` 追加 7 条

**Interfaces:**
- Consumes: Task 2 的 `query_expression` / `clean_snippet`、Task 1 的两张表、`crate::ledger` 与 `crate::vault`（只为验收测试造密文行）
- **顺带收掉的豁免**：Task 2 留在 `tokenize.rs` 的 `query_expression` 与 `clean_snippet` 两条 `#[allow(dead_code)]`（同一条 `index_text` 的豁免已在 Task 7 收掉，Task 7 落地后该文件剩 2 条）。本任务的 `doc_hits` 是这两个函数的第一个非测试调用点，两条必须删干净：提交前 `grep -c "allow(dead_code)" src/tokenize.rs` 期望 **0**。
- **本任务自己的死代码账（控制方一次性探针实测，跑完已还原，工作树干净）**：不带豁免时 `cargo clippy --lib -- -D warnings` 报 **4 条** —— `doc_hits` never used、`run_select` never used、`SELECT_SQL` constant never used、`DocHit` never constructed。**只给 `doc_hits` 挂一条豁免，4 条全消失**（第 4 次证实「allow 项即额外可达根」：`doc_hits` 一旦可达，它调用的 `run_select`、那个 const、以及返回型 `DocHit` 一起被罩住，`SELECT_SQL` 这条计划原本没算到）。同时 `tokenize.rs` 那两条豁免删掉之后**不会**报 `query_expression`/`clean_snippet`，因为它们的调用点在 `doc_hits`/`run_select` 里，可达性顺着新豁免走 —— 探针还顺带暴露 `is_cjk` 也是同一条链（只在没挂豁免的中间态报出来，挂上即消失）。所以**本任务落 1 条豁免**：`grep -c "allow(dead_code)" src/index_store.rs` 从 6 变 **7**，`src/tokenize.rs` 变 **0**。
- Produces:
  - `pub struct DocHit { pub doc_id: String, pub project_id: String, pub project_name: String, pub path: String, pub snippet: String, pub matched_by: &'static str, pub score: f64 }`（`#[serde(rename_all = "camelCase")]`）
  - `pub fn doc_hits(conn: &Connection, query: &str, limit: i64) -> AppResult<Vec<DocHit>>`

- [ ] **Step 1: 写失败测试（7 条）**

```rust
    /// 摘要 + 得分 + 命中来源，是 M4 首屏结果页要直接渲染的东西。
    fn hits(conn: &Connection, q: &str) -> Vec<DocHit> {
        doc_hits(conn, q, 50).unwrap()
    }

    fn seeded_with_docs() -> Connection {
        let c = conn();
        seed_project(&c, "p1");
        write_doc(&c, "p1", &file("C:/x/合同验收说明.docx"), DocOutcome::Ok("甲方要求验收指标见合同附件".into())).unwrap();
        write_doc(&c, "p1", &file("C:/x/维保期说明.docx"), DocOutcome::Ok("维保期为十二个月".into())).unwrap();
        write_doc(&c, "p1", &file("C:/x/报价单.xlsx"), DocOutcome::Ok("里面只有付款条件与验收流程".into())).unwrap();
        c
    }

    #[test]
    fn chinese_word_hits_with_a_readable_body_snippet() {
        let c = seeded_with_docs();
        let got = hits(&c, "验收");
        assert_eq!(got.len(), 2, "{:?}", got.iter().map(|h| &h.path).collect::<Vec<_>>());
        assert!(got.iter().all(|h| h.matched_by == "exact"), "精确段就该命中：{:?}", got.iter().map(|h| h.matched_by).collect::<Vec<_>>());
        let hit = &got[0];
        assert_eq!(hit.project_name, "政务云迁移", "结果要带项目名，供 M4 分组");
        assert!(hit.snippet.contains('[') && hit.snippet.contains(']'), "摘要要标出命中词：{}", hit.snippet);
        // 原来这条写的是 `!hit.snippet.contains(" 甲 方")`，控制方探针实测它是假断言：
        // 入库串是 jieba 词用空格拼的，「甲方」本身是一个词，原始摘要为
        // "甲方 要求 [验收] 指标 见 合同 附件"，里面根本不存在 " 甲 方" 这个子串，
        // 把 run_select 里的 clean_snippet 调用删掉，整套测试照样全绿。
        // 夹具正文全 CJK，所以「摘要里一个空格都不该剩」既断得住，又能被同一处变异打红。
        assert!(!hit.snippet.contains(' '), "摘要里不该残留分词空格：{}", hit.snippet);
        // 摘要只取自正文列（snippet 的第 2 个实参固定为 1）：只靠文件名命中的结果，摘要不带 [ ]。
        // 这条是 M4 的界面契约，探针实测 hits("报价单")[0].snippet =
        // "里面只有付款条件付款条件与验收流程"（正文没「报价单」三个字，故无标记）。
        let only_name = hits(&c, "报价单");
        assert_eq!(only_name.len(), 1);
        assert!(!only_name[0].snippet.contains('['), "正文没这个词、只靠文件名命中时摘要不带标记：{}", only_name[0].snippet);
    }

    /// 事实 5：复合词的子词查询靠 cut_for_search 才能命中。
    #[test]
    fn compound_and_subword_queries_hit() {
        let c = seeded_with_docs();
        assert_eq!(hits(&c, "付款条件").len(), 1);
        assert_eq!(hits(&c, "付款 条件").len(), 1, "子词没进索引的话这条会空");
        assert_eq!(hits(&c, "合同附件").len(), 1);
    }

    /// 事实 6：「维保」在库里是「维保期」的一部分，精确段够不着，必须落到前缀段。
    #[test]
    fn prefix_stage_reports_itself_so_the_ui_can_explain_the_looser_match() {
        let c = seeded_with_docs();
        let got = hits(&c, "维保");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].matched_by, "prefix", "放宽过的命中要能说清：{:?}", got[0]);
        assert!(got[0].path.contains("维保期说明"), "夹具名见事实 6 的夹具限制");
    }

    #[test]
    fn fts5_syntax_in_the_query_cannot_widen_the_result_set() {
        let c = seeded_with_docs();
        // 语法词被引号包成普通词：这三条都应该是 0 或窄结果，绝不等于「全表命中」。
        assert!(hits(&c, "验收 OR 报价").is_empty(), "OR 只是普通字符，整条要求两词都在");
        assert!(hits(&c, "*").is_empty(), "单星号不该等于全表命中");
        assert!(hits(&c, "NEAR(验收 报价)").is_empty());
        assert_eq!(hits(&c, "验收").len(), 2, "对照组：同一库上正常查询是有结果的");
    }

    #[test]
    fn soft_deleted_project_docs_drop_out_of_hits() {
        let c = seeded_with_docs();
        assert!(!hits(&c, "验收").is_empty());
        c.execute("UPDATE projects SET deleted_at = datetime('now') WHERE id = 'p1'", []).unwrap();
        assert!(hits(&c, "验收").is_empty(), "项目软删除后其文档不该再出现在结果里");
    }

    /// M3 的验收点。spec 明写「密文列不参与任何检索」，且要求结构性排除而不是查询后过滤：
    /// 台账的密文列从来没有进入 write_doc 的入参路径（正文只来自磁盘文件的抽取结果），
    /// 所以这条测的是「整条管线跑完后，敏感字段的明文在索引里彻底不存在」。
    #[test]
    fn cipher_column_plaintext_never_reaches_the_fts_index() {
        use crate::{ledger, vault};

        let c = seeded_with_docs();
        let code = vault::generate_recovery_code().unwrap();
        let mk = vault::initialize(&c, "主密码-至少八位", &code).unwrap();
        let secret = "机密串-Zhang@2026";
        ledger::create_credential(
            &c,
            &mk,
            "p1",
            &ledger::CredentialInput {
                env_id: None,
                title: "运维后台".to_owned(),
                username: Some(secret.to_owned()),
                password: Some(secret.to_owned()),
                url: Some("https://ops.example.gov.cn".to_owned()),
                note: Some("交付时用".to_owned()),
            },
        )
        .unwrap();

        // 1) 用敏感字段的明文检索，一条都不该命中。先钉住这条腿不是空转：
        //    查询串必须真的能成形，否则 doc_hits 跳过 SQL 直接回空、断言恒绿。
        //    探针实测 query_expression(secret,false) = Some("\"机密\" AND \"串\" AND \"Zhang\" AND \"2026\"")。
        assert!(query_expression(secret, false).is_some(), "第 1 段的查询串要能成形，否则空结果是假绿");
        assert!(hits(&c, secret).is_empty(), "敏感字段明文进不了 FTS");
        // 2) 结构性检查：虚表两列里连片段都不该有。needle 只用**一段连续**字符 ——
        //    原来写的 "%Zhang@2026%" 经探针实测：在「明文真的进了索引」的库里照样回 0
        //    （入库串是 jieba 词用空格拼的，Zhang 与 2026 之间被隔开了），
        //    也就是这条当时无论有没有泄露都恒绿。换 %Zhang% / %2026% 后各回 1，才是能变红的写法。
        for col in ["body_tokens", "name_tokens"] {
            for needle in ["%Zhang%", "%2026%"] {
                let n: i64 = c
                    .query_row(
                        &format!("SELECT count(*) FROM index_docs_fts WHERE {col} LIKE ?1"),
                        params![needle],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(n, 0, "{col} 里不该存在敏感值 {needle}：{n}");
            }
        }
        // 3) 自我证明（正面照控）：把同一串明文走**正常写入路径**放进索引，上面两条必须能变红。
        //    没有这一段，前两条只是「看起来像测试」。探针实测：hits(&c2, secret) 回 1 条、
        //    body_tokens LIKE '%2026%' 计数 1。
        let c2 = seeded_with_docs();
        write_doc(&c2, "p1", &file("C:/x/演示.docx"), DocOutcome::Ok(secret.into())).unwrap();
        assert_eq!(hits(&c2, secret).len(), 1, "正面照控：明文一旦进了索引，第 1 段断言真的会红");
        let n: i64 = c2
            .query_row("SELECT count(*) FROM index_docs_fts WHERE body_tokens LIKE '%2026%'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "正面照控：明文一旦进了索引，第 2 段的 LIKE 真的抓得到");
        // 4) 反向对照：台账的明文列 note（交付时用）走的是库内字段检索，
        //    而 FTS 这边只有磁盘文件正文，两边互不污染
        assert_eq!(hits(&c, "付款条件").len(), 1);
    }

    /// 两条 guard 分支和 limit 的去向。`search.rs` 给自己的同名 guard 留了测试
    /// （断 `invalid_input`），这里补齐，否则「空白查询不扫库」「超长要拒绝」全靠肉眼。
    /// limit 那三条钉的是**现状**不是意图：SQLite 里负数 LIMIT = 不限行、0 = 无行，
    /// `doc_hits` 不做二次校验，clamp 归调用方边界（Task 10 的 IPC 写 `limit.unwrap_or(50).clamp(1, 200)`）。
    #[test]
    fn query_guards_and_limit_pass_straight_through() {
        let c = seeded_with_docs();
        assert!(doc_hits(&c, "   ", 50).unwrap().is_empty(), "空白查询不扫库");
        let long = "验".repeat(129);
        let e = doc_hits(&c, &long, 50).unwrap_err();
        assert_eq!(e.code, "invalid_input", "超 128 字要拒绝，不是静默按前 128 字搜");
        assert_eq!(doc_hits(&c, "验收", 1).unwrap().len(), 1, "limit 要真的限制条数");
        assert_eq!(doc_hits(&c, "验收", -1).unwrap().len(), 2, "负数 limit 原样交给 SQL（SQLite：不限行）");
        assert!(doc_hits(&c, "验收", 0).unwrap().is_empty(), "0 就是 0 行，不许悄悄变成默认值");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib index_store::tests::chinese_word`
Expected: `cannot find function doc_hits`；`cipher_column...` 同样失败

- [ ] **Step 3: 写实现**

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocHit {
    pub doc_id: String,
    pub project_id: String,
    pub project_name: String,
    pub path: String,
    /// 已按 `clean_snippet` 收回 CJK 空格的摘要，命中词用 [ ] 包住。
    /// 两条 M4 要知道的契约（控制方探针实测）：摘要取自**正文列**（`snippet()` 的第 2 个实参固定为 1），
    /// 只靠文件名命中的结果摘要不带 [ ]；且摘要串是预分词后的正文，`cut_for_search` 的复合词会
    /// 连着出现两次（实测「里面只有付款条件付款条件与验收流程」）—— 不影响召回，只影响观感。
    /// 要拿原文做摘要得在建表时给虚表加一列 UNINDEXED 正文，不许在检索侧拼。
    pub snippet: String,
    /// exact | prefix：放宽过的命中要能被界面标出来，否则用户会以为是 bug
    pub matched_by: &'static str,
    /// bm25 原值，**越小越好**：bundled `sqlite3.c:245090` 交回的是 `-1.0 * score`，
    /// 所以 `ORDER BY bm25(...)` 升序就是最相关在前。这个数只用于相对排序，别直接显示给用户。
    pub score: f64,
}

const SELECT_SQL: &str = "SELECT d.id, d.project_id, p.name, d.path,
       snippet(index_docs_fts, 1, '[', ']', '⋯', 12), bm25(index_docs_fts)
  FROM index_docs_fts f
  JOIN index_docs d   ON d.doc_rowid = f.rowid
  JOIN projects p    ON p.id = d.project_id
 WHERE index_docs_fts MATCH ?1
   AND d.index_status = 'ok'
   AND p.deleted_at IS NULL
 ORDER BY bm25(index_docs_fts)
 LIMIT ?2";

fn run_select(conn: &Connection, expr: &str, limit: i64, matched_by: &'static str) -> AppResult<Vec<DocHit>> {
    let mut stmt = conn.prepare(SELECT_SQL)?;
    let rows = stmt.query_map(params![expr, limit], |r| {
        Ok(DocHit {
            doc_id: r.get(0)?,
            project_id: r.get(1)?,
            project_name: r.get(2)?,
            path: r.get(3)?,
            snippet: crate::tokenize::clean_snippet(&r.get::<_, String>(4)?),
            score: r.get(5)?,
            matched_by,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// 两段式（中文分词检索的核心取舍）：第一段「精确」要求词形完全一致，结果最准；
/// 第二段「前缀」只在 0 命中时放宽成 `"维保"*`，把被切成「维保期」这种长词的情况捞回来。
/// 只做第一段会表现为「功能没坏但搜不到」，直接只做第二段则精度差。
/// 两段用同一个 `query_expression`，即同一套预处理，见 tokenize.rs 的头注释。
///
/// `limit` 原样进 SQL，这里不校验也不补默认值：SQLite 里负数 LIMIT = 不限行、0 = 无行，
/// clamp 属于调用方的系统边界（Task 10 的 IPC：`limit.unwrap_or(50).clamp(1, 200)`）。
/// 刻意不做第二道校验 —— 本项目只在边界校验一次，两道 clamp 会漂成两个数。
#[allow(dead_code)] // caller 在 Task 10 的检索 IPC，落地时删掉本行
pub fn doc_hits(conn: &Connection, query: &str, limit: i64) -> AppResult<Vec<DocHit>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    if q.chars().count() > 128 {
        return Err(AppError::new(
            "invalid_input",
            "正文检索关键词过长",
            Some("请用短词搜正文；要找某个具体文件，去 /index 页按项目翻清单"),
        ));
    }
    if let Some(expr) = query_expression(q, false) {
        let hits = run_select(conn, &expr, limit, "exact")?;
        if !hits.is_empty() {
            return Ok(hits);
        }
        if let Some(loose) = query_expression(q, true) {
            return run_select(conn, &loose, limit, "prefix");
        }
    }
    Ok(Vec::new())
}
```

引入要动**两行**（控制方探针实测，少任一条都当场红）：
- `use crate::error::AppResult;` → `use crate::error::{AppError, AppResult};`（`AppError::new` 在本任务的超长查询分支第一次被用到）。
- `use crate::tokenize::index_text;` → `use crate::tokenize::{index_text, query_expression};`。上面 Step 3 的代码里 `query_expression(q, false)` 是**非限定调用**，不补这条就是 `E0425 cannot find function`，整个模块不编译（计划原版只提了 `AppError`，漏了这条）。`clean_snippet` 在代码里写的是全限定 `crate::tokenize::clean_snippet`，不需要进 `use`。

另外上面那段 `///` 说明原来写成 `/// 1. …` / `/// 2. …` 的编号列表，后面紧跟两条不缩进的正文行 ——
`cargo clippy --lib -- -D warnings` 会以 `doc list item without indentation` **报错**（不是提示，是 -D 之后的 error），
探针实测两条。改成不用编号的散文写法即可，语义一字未减。

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib index_store`
Expected: `14 passed`（Task 7 的 7 + 本任务 7）；全量 → `87 passed; 0 failed`

若 `fts5_syntax...` 里 `hits(&c, "*")` 报错而不是空，说明 `query_expression("*", false)` 回了 `Some`——事实 8 要求纯标点被过滤后回 `None`；回到 Task 2 修过滤器，**不要**在这里加特判。（控制方探针实测：这条不报错、回空，`snippet(...)`/`bm25(...)`/`"维保"*` 前缀段/`p.deleted_at IS NULL` 四个 FTS5 行为都和计划预期一致。）

**报告里要交的变异证据（至少三条，每条只该红一条）**：
- 去掉 `AND p.deleted_at IS NULL` → `soft_deleted_project_docs_drop_out_of_hits` 必红（其余不敏感）。
- 去掉两段式里的前缀段（`if let Some(loose) = ...` 那一支）→ `prefix_stage_reports_itself...` 必红。
- 把 `run_select` 里的 `crate::tokenize::clean_snippet(&r.get::<_, String>(4)?)` 换成裸的 `r.get::<_, String>(4)?` → `chinese_word_hits_with_a_readable_body_snippet` 必红。**这条是本轮修订的理由本身**：修订前这个变异一处都不红，摘要散开的失效看不见。
- （可选加测）把 `LIMIT ?2` 改成写死数字 → `query_guards_and_limit_pass_straight_through` 必红。

前两条是本任务「看不见就全绿」的失效面（漏了前缀段表现为「维保」搜不到而测试不会自己喊），第三条是复审补上的那条，所以不许只交「14 passed」就当验证过。

- [ ] **Step 5: 跑两道 clippy 闸 + 豁免对账**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings && cargo clippy --lib --all-targets -- -D warnings
grep -c "allow(dead_code)" src/tokenize.rs
grep -c "allow(dead_code)" src/index_store.rs
```

Expected：两道闸 exit 0；`tokenize.rs` **0**（本任务收掉最后两条，漏删不会红，只会变永久豁免）；`index_store.rs` **7**（Task 7 的 6 条 + `doc_hits` 这条新的）。控制方探针实测这组数字。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/index_store.rs src-tauri/src/tokenize.rs
git commit -m "feat: M3 两段全文检索并钉住密文不进索引的验收"
```

**两个文件一起提交**：本任务删掉 `tokenize.rs` 的两条豁免，漏 add 就等于把改动丢在工作树里（计划原版只 add 了 `index_store.rs`，控制方按 Files 与 Interfaces 改正）。`git status` 复核只应有这两个文件，禁止 `git add -A`。

---

## Task 9: `index_job.rs` —— 一次 pass 的纯函数 + 后台线程包装

**Files:**
- Create: `src-tauri/src/index_job.rs`
- Modify: `src-tauri/src/lib.rs`（`mod index_job;`）
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: `index_scan::{ScanOptions, scan_root}`、`extract::extract_text`、`index_store::{write_doc, current_rowid, clear_project, DocOutcome}`、`db::open`、`tauri::Emitter`、`std::sync::atomic::{AtomicBool, Ordering}`
  - `extract_text` 的 `Err` 实际有**三个**码名，不是两个：`extract_unsupported`、`extract_failed`，加上 `AppError::io` 带来的 `fs_failed`（读文件本身失败，典型是扫描到抽取之间文件被删/被占用）。下面的 `match` 用「`extract_unsupported` 单列、其余一律 `Failed`」的写法，所以 `fs_failed` 会落进 `failed` 而不是 `missing` —— 这是有意的：`missing` 描述的是对账结论（库里有一行、磁盘上没有了），归 M5 的启动对账写，抽取这一轮不判它。别在 Task 9 里顺手把它改成 `missing`。
- **`capped` 的语义（Task 6 修复轮改过扫描侧，这里按新语义接）**：`scan_root` 的单项目配额是在分「可抽取 / 超限」两桶**之前**判的，所以 `ScanOutcome.capped = true` 说的是「`files` + `over_size` 合计撞到 `max_files_per_project`，至少还有一个受支持的文件被丢掉」，而不是「可抽取清单被截断」。接法：`ProjectResult.scanned_total` 取两桶之和（它就是本轮会产生多少行），`capped` 原样透传；`break` 之后剩下的文件既没有行也没有计数，`ScanOutcome` 里**没有**被丢弃的条数字段，因此摘要只能说「截断了」而不能说「还差 M 个」—— 这是有意的，别为了凑那个数字去给 `ScanOutcome` 加字段。超限那一桶同样会被截，Task 11 的文案要按这个写（见 Task 11 Step 4）。
- **顺带收掉的豁免**（`run_pass` 是这三处 `pub` 项的第一个非测试 caller）：`extract.rs` 里 `extract_text` 上面那条 `#[allow(dead_code)] // caller 在 Task 9`、`index_scan.rs` 里 `scan_root` 与 `ScanOptions::load` 上面各一条同形豁免，本任务落地时一并删掉。漏删不会红，只会变成永久豁免，所以 Step 里要跑 `grep -n "allow(dead_code)" src/extract.rs src/index_scan.rs` —— 期望结果：`extract.rs` 只剩 `supported_exts` 那一条（caller 在 Task 10），`index_scan.rs` 应该**一条都不剩**。
  `index_store.rs` 这边由 `run_pass` 收掉四条：`write_doc`、`current_rowid`、`clear_project`、以及挂在 `enum DocOutcome` 本体上那条（`use crate::index_store::{clear_project, current_rowid, write_doc, DocOutcome};` 就是它们的非测试调用点）。**删掉枚举级豁免后必须让 `run_pass` 真的构造 `Empty`**（见 Step 3 的 `Ok(body) if body.trim().is_empty()` 那一臂）：探针实测不带那一臂时闸 1 报 `variant \`Empty\` is never constructed`（`index_store.rs:22`），因为 lib 侧只有 match 没有构造 —— Task 7 当年就是为这件事在枚举本体挂的豁免（见 Task 7 Interfaces 的死代码账）。这条臂不是为豁免加的，它同时补掉一个真 bug：`acc.ok += 1` 遇上空白正文时，`write_doc` 落库是 `skipped/empty_text`、内存计数器却记成 ok，两个口径在 Task 11 的界面上会当场对不上。
  **Task 9 提交前的期望是 `grep -c "allow(dead_code)" src/index_store.rs` 回 3**（`status_counts`、`list_docs` 归 Task 10，`doc_hits` 是 Task 8 落的、它的 caller 同样在 Task 10）。计划原版写的「回 2」是**在 Task 8 落 `doc_hits` 之前算的**，属陈旧数字，控制方探针实测更正为 3。`delete_doc` 不存在（Task 7 已按裁定移除），别去找它。
- **本任务自己要落的新豁免（控制方一次性探针实测，跑完已还原）**：`index_job.rs` 里 `start` 与 `IndexShared::new` **各需一条**，共 **2** 条。
  - 只写实现、一条豁免都不挂时，`cargo clippy --lib -- -D warnings` 报 **39 条** `error: … is never used / never constructed`。原因不是「`start` 没人调」这么简单：`start` 是 `run_pass` 在 lib 侧唯一的非测试调用者，而 `cargo clippy --lib` 不带 `--all-targets`、不编译 `#[cfg(test)]`，所以 `start` 一死，`run_pass` → `targets`/`progress_for`/`truncate`/`now`/`PROGRESS_EVENT`/`PROGRESS_EVERY`/`ScanTarget`/`Progress`/`ProjectResult`/`done_total`/`RunSummary` 整串跟着死，并且顺着调用把 `extract.rs` 全模块 12 条（含 `extract_text`/`kind_of`/`DocKind`/`TEXT_EXTS`）、`index_scan.rs` 3 条（`ScanOptions`/`load`/`ScannedFile`）一起拖出来 —— 「allow 项是额外可达根」（本仓库第 6 次证实）反过来用就是：删掉旧根必须先立新根。
  - **只给 `start` 挂一条**，39 条塌成 **2 条**：剩 `associated function \`new\` is never used`（`IndexShared::new`，构造点在 Task 10 的 `AppState::manage`）与上面那条 `DocOutcome::Empty`。所以 `new` 要自己挂一条，注明 caller 在 Task 10。**别顺手给 `IndexShared` 结构体也挂**：它的四个字段（含修复轮加的 `last_error`）全被 `start` 那侧用过，探针实测不报。
- **`//!` 模块 doc 用 `- ` bullet 列表是安全的**（控制方探针实测：原版那四行按 Step 3 逐字编译，两道闸 exit 0）。Task 8 撞到的 `doc list item without indentation` 是「编号列表后面紧跟不缩进的正文行」，与本处的列表结尾不同 —— 别把这段改成散文「以防万一」。
- Produces:
  - `pub struct ScanTarget { pub project_id: String, pub project_name: String, pub root_path: String }`
  - `pub fn targets(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<ScanTarget>>`
  - `pub struct Progress { pub state: &'static str, pub project_id: String, pub project_name: String, pub total: i64, pub done: i64, pub ok: i64, pub skipped: i64, pub failed: i64, pub current: String, pub error: Option<String> }`（camelCase Serialize）。`state` 的实际取值集是 `running | done | cancelled | error`；`error` 只在 `state == "error"` 时有值，`current` 的契约始终是「当前文件路径」，别把错误文案塞进去。
  - `pub struct ProjectResult { pub project_id: String, pub project_name: String, pub scanned_total: i64, pub ok: i64, pub skipped: i64, pub failed: i64, pub unchanged: i64, pub walk_errors: i64, pub capped: bool, pub root_missing: bool }`
  - `pub struct RunSummary { pub state: &'static str, pub results: Vec<ProjectResult>, pub started_at: String, pub finished_at: String }`
  - `pub fn run_pass(conn: &Connection, opts: &ScanOptions, project_id: Option<&str>, rebuild: bool, cancel: &AtomicBool, on_progress: &mut dyn FnMut(Progress)) -> AppResult<RunSummary>`
  - `pub struct IndexShared { pub cancel: AtomicBool, pub running: AtomicBool, pub summary: Mutex<Option<RunSummary>>, pub last_error: Mutex<Option<String>> }`
  - `pub fn start(app: AppHandle, data_dir: PathBuf, shared: Arc<IndexShared>, project_id: Option<String>, rebuild: bool) -> AppResult<()>`
  - `pub const PROGRESS_EVERY: i64 = 20;` / `pub const PROGRESS_EVENT: &str = "index://progress";`
  - 模块私有（Task 10 不消费，只在 `start` 与测试里用）：`struct RunningGuard(Arc<IndexShared>)` + `RunningGuard::enter`、`fn terminal(state, results, error) -> Progress`、`fn report_failure(emit, shared, msg)`。

**分层执行时要知道的两个接口约束（Task 9 评审修复轮加的）：**
- **`running` 只由 `RunningGuard` 的 Drop 复位**，`start` 里不许再出现第二处 `running.store(false)`（线程体正常走完、提前 `return`、panic unwind 三种出口共用同一个复位点）。唯一的例外是 `Builder::spawn` 返回 `Err` —— 那时闭包还没跑过、守卫根本没构造，只能就地复位后再返回错误。这条不是洁癖：本机物理 16 GB，`spawn` 因 `os error 1455` 失败是**日常**（本计划的探针就遇到过），少了这个兜底就得到「界面上再也起不动第二轮、只能重启应用」且没有任何日志。
- **一轮彻底失败必须留下原因**：`db::open` / `ScanOptions::load` / `run_pass` 三个出口原来都塌成同一个 `finish(None)`，把 `AppError` 当场丢掉，Task 10 的 IPC 只能看到 `summary == None`，分不清「从没跑过」「库打不开」「settings 坏了」「中途报错」。现在三处都走 `report_failure`：写 `last_error` + emit 一次 `state == "error"` 的终止事件。存的是 `format!("{e}")`（`AppError` 的 `Display` 自带 `[code] message（hint）`）而不是 `AppError` 本体 —— 它只 derive 了 `Debug`、没有 `Clone`，而这里要跨线程留一份。代价：机器码混在串里，Task 10 若要按 code 分支就得改存三元组，届时再说。
- **成功/取消也要发终止事件**：`run_pass` 只在项目边界发进度，只订阅事件的前端拿不到「这一轮结束了」的信号，只能反过来轮询 summary。`terminal(...)` 就是那一条轮级事件，`project_id` 是空串 —— 它不属于任何一个项目，Task 11 别拿它当 key。

**分层理由：** `run_pass` 不依赖 Tauri，收 `&Connection` + 回调，所以「扫 + 抽 + 写 + 取消 + 增量跳过」全部能在内存库 + tempdir 上测。`start` 只是「开一条连接、把回调换成 `app.emit`、维护 running/cancel」的薄壳，这一层的正确性由 Task 12 的真机验证负责，本任务不给它写假测试。

- [ ] **Step 1: 写失败测试（7 条）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, index_scan::ScanOptions};
    use rusqlite::{params, Connection};
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    const MAX: u64 = 20 * 1024 * 1024;

    fn opts() -> ScanOptions {
        ScanOptions { exclude_dirs: vec!["node_modules".to_owned()], max_file_bytes: MAX, max_files_per_project: 50000 }
    }

    fn seeded_project(conn: &Connection, id: &str, root: &Path) {
        conn.execute("INSERT INTO projects (id, name, status) VALUES (?1, ?2, '立项')", params![id, "政务云迁移"])
            .unwrap();
        conn.execute(
            "INSERT INTO project_dirs (id, project_id, kind, path) VALUES (?1, ?2, 'root', ?3)",
            params![format!("{id}-d"), id, root.display().to_string()],
        )
        .unwrap();
    }

    /// 造一棵内容确定的树：两个可索引文本、一个超限、一个不支持类型、一个排除目录。
    fn tree(dir: &Path) {
        std::fs::create_dir_all(dir.join("合同/node_modules")).unwrap();
        std::fs::write(dir.join("合同/验收说明.txt"), "甲方要求验收指标".as_bytes()).unwrap();
        std::fs::write(dir.join("维保.txt"), "维保期为十二个月".as_bytes()).unwrap();
        std::fs::write(dir.join("大图.txt"), vec![b'a'; 300]).unwrap(); // 配合下面的小上限
        std::fs::write(dir.join("空扫描.txt"), "   \n\t".as_bytes()).unwrap(); // 抽取成功但正文空白
        std::fs::write(dir.join("图片.png"), b"\x89PNG").unwrap();
        std::fs::write(dir.join("合同/node_modules/x.txt"), "不该被索引".as_bytes()).unwrap();
    }

    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    /// 库里的行数。重建用例要拿它证明「磁盘上没了的旧行跟着掉」，
    /// 光看 `doc_hits` 不够：命中原问句可能只是因为那行根本没被写过。
    /// `project_id` 走参数而不是写死 `'p1'` —— `missing_root_...` 那条用例里同时有 p1 和 p2，
    /// 助手写死 id 会让它读出 0、然后以错误的理由变红。
    fn doc_rows(conn: &Connection, project_id: &str) -> i64 {
        conn.query_row("SELECT count(*) FROM index_docs WHERE project_id = ?1", [project_id], |r| r.get(0)).unwrap()
    }

    /// 一轮 pass 要把结局各归其位：ok / skipped(too_large) / skipped(empty_text，空白抽取) / 未支持不建行 / 排除目录不进。
    /// 同时验证「pass 结束后可检索」这条端到端性质（内存库 + 真文件）。
    #[test]
    fn run_pass_indexes_a_generated_tree_and_separates_outcomes() {
        let dir = tempfile::tempdir().unwrap();
        tree(dir.path());
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        let mut o = opts();
        o.max_file_bytes = 100;

        let mut events = Vec::new();
        let summary = run_pass(&conn, &o, None, false, &no_cancel(), &mut |p| events.push(p.clone())).unwrap();
        assert_eq!(summary.state, "done");
        let one = &summary.results[0];
        assert_eq!(one.ok, 2, "两个正常文本文件应入库：{one:?}");
        assert_eq!(one.skipped, 2, "超限与空白抽取各一行都该记 skipped：{one:?}");
        assert_eq!(one.failed, 0);
        assert_eq!(one.scanned_total, 4, "未支持与排除目录不该计数：{one:?}");
        assert!(!events.is_empty(), "至少要有一次进度回调");
        assert_eq!(events.last().unwrap().done, 4);
        // 计数器与库里的行必须同一个口径：空白抽取在 index_docs 上是 skipped/empty_text，不是 ok。
        // 少这条断言，`acc.ok += 1` 与 write_doc 落库的 status 各自漂移也不会有人喊 ——
        // 而 Task 11 的界面要同时读这两个数（摘要读计数器、状态统计读 status_counts）。
        let blank: (String, Option<String>) = conn
            .query_row(
                "SELECT index_status, skip_reason FROM index_docs WHERE path LIKE '%空扫描.txt'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(blank, ("skipped".to_string(), Some("empty_text".to_string())), "空白抽取的行口径要和计数器一致：{blank:?}");

        let hits = crate::index_store::doc_hits(&conn, "验收", 20).unwrap();
        assert_eq!(hits.len(), 1, "pass 之后必须搜得到：{hits:?}");
        assert!(crate::index_store::doc_hits(&conn, "不该被索引", 20).unwrap().is_empty(),
            "排除目录里的内容混进了索引");
    }

    /// 增量：第二趟一个文件都不该再读（unchanged 计数），并且状态行不会被写坏。
    #[test]
    fn second_pass_skips_unchanged_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::write(dir.path().join("验收.txt"), "甲方要求验收指标".as_bytes()).unwrap();
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());

        let first = run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(first.results[0].ok, 1);
        assert_eq!(first.results[0].unchanged, 0);

        let second = run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(second.results[0].unchanged, 1, "size+mtime 没变的 ok 行不该重读：{:?}", second.results[0]);
        assert_eq!(second.results[0].ok, 0, "没重读就不该再计 ok");
        assert_eq!(crate::index_store::doc_hits(&conn, "验收", 20).unwrap().len(), 1, "跳过后仍可搜");
    }

    /// 取消：第一个进度回调就把标志立起来，pass 必须停在中间。
    #[test]
    fn cancel_stops_the_pass_early() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        for i in 0..60 {
            std::fs::write(dir.path().join(format!("文件{i}.txt")), format!("正文{i} 验收").as_bytes()).unwrap();
        }
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        let cancel = AtomicBool::new(false);

        let summary = run_pass(&conn, &opts(), None, false, &cancel, &mut |_p| {
            cancel.store(true, Ordering::Relaxed);
        })
        .unwrap();
        assert_eq!(summary.state, "cancelled");
        let done = summary.results[0].ok + summary.results[0].skipped;
        assert!(done < 60, "取消后不该把 60 个文件全跑完：{done}");
        assert!(done > 0, "至少要真的开始过，否则这条测试什么都没证明");
    }

    /// 重建：先把该项目的行与 FTS 清掉再写。三件事都要钉住 ——
    /// ① 内容变了的文件必须重读；② 磁盘上已经没了的旧行不能留；③ 一个字节都没动的文件
    /// 在重建趟也必须被重读（`unchanged == 0`）。
    /// ②才是 `clear_project` 唯一承重的场景：`write_doc` 的 `ON CONFLICT DO UPDATE` 会自己
    /// 换掉同路径旧正文，所以「同一文件改了内容」根本测不到它，只有「这一轮不再出现的旧行」测得到。
    /// ③不是给 `!rebuild` 那个短路守卫当检测的 —— 探针实测：把 `} else if !rebuild` 改成
    /// `} else if true`，7 条**全绿**。原因是 rebuild 趟开头 `clear_project` 已把该项目的行全删了，
    /// `current_rowid` 必然回 None，所以那个短路在 rebuild 趟里是不可观测的死逻辑；它只在
    /// 「以后谁把 `clear_project` 挪走」时才显形，而那一改动同时会红 ②（见 Step 4 变异清单）。
    /// `不变.txt` 的真正价值：它是两轮之间唯一「路径、内容、mtime 全不变」的在场文件，
    /// 给 `doc_rows == 3` 与 `稳定 == 1` 两条正面照控提供落点 —— 没有它，本用例只能断「归零」。
    #[test]
    fn rebuild_rewrites_everything_and_leaves_no_stale_tokens() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        let p = dir.path().join("合同.txt");
        std::fs::write(&p, "第一版 验收".as_bytes()).unwrap();
        let gone = dir.path().join("归档说明.txt");
        std::fs::write(&gone, "这份要归档 验收".as_bytes()).unwrap();
        let same = dir.path().join("不变.txt");
        std::fs::write(&same, "这一份两轮都不动 稳定".as_bytes()).unwrap();
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(doc_rows(&conn, "p1"), 3, "第一趟三行都该在库里");
        // 正面照控：验收 只出现在 合同/归档说明 两个文件的**正文**里（两个文件名都没有它），
        // 所以 == 2 才真的证明正文进了索引。只查「归档」会被 name_tokens 满足，证不到正文那条腿。
        assert_eq!(crate::index_store::doc_hits(&conn, "验收", 20).unwrap().len(), 2,
            "正面照控：两份正文都在库里，否则后面的 0 命中什么都没证明");

        std::fs::write(&p, "第二版 报价".as_bytes()).unwrap();
        std::fs::remove_file(&gone).unwrap();
        let summary = run_pass(&conn, &opts(), None, true, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(summary.results[0].ok, 2, "重建必须把在场的全重读：{:?}", summary.results[0]);
        assert_eq!(summary.results[0].unchanged, 0, "重建趟不该有文件被跳过：{:?}", summary.results[0]);
        assert_eq!(crate::index_store::doc_hits(&conn, "验收", 20).unwrap().len(), 0, "旧正文要跟着清掉");
        assert_eq!(crate::index_store::doc_hits(&conn, "归档", 20).unwrap().len(), 0, "磁盘上没了的旧行不能留");
        assert_eq!(crate::index_store::doc_hits(&conn, "报价", 20).unwrap().len(), 1);
        assert_eq!(crate::index_store::doc_hits(&conn, "稳定", 20).unwrap().len(), 1, "没动的文件要被重写回来");
        assert_eq!(doc_rows(&conn, "p1"), 2, "库里的行数也要跟着掉");
    }

    /// 撞配额这一轮的口径：`capped` 必须原样透到 `ProjectResult`，`scanned_total` 必须是
    /// 「两桶之和」—— 它就是本轮会产生多少行，Task 11 的摘要与状态统计要拿它对账。
    /// `ScanOutcome` 里**没有**「被丢弃了几条」的字段，所以这里只能断「收下的那 3 条」，
    /// 别说「还差 M 个」；也别为了凑那个数去给 `ScanOutcome` 加字段。
    /// 没有这条用例，`acc.capped = outcome.capped` 与 `acc.walk_errors` 两行是零守护的赋值。
    #[test]
    fn capped_and_scanned_total_survive_the_quota() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..4 {
            std::fs::write(dir.path().join(format!("验收{i}.txt")), format!("第{i}份 报价").as_bytes()).unwrap();
        }
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        let mut o = opts();
        o.max_files_per_project = 3;

        let summary = run_pass(&conn, &o, None, false, &no_cancel(), &mut |_| {}).unwrap();
        let r = &summary.results[0];
        assert!(r.capped, "触顶必须透上来，否则 Task 11 说不出「为什么少了几个」：{r:?}");
        assert_eq!(r.scanned_total, 3, "两桶之和就是本轮的行数：{r:?}");
        assert_eq!(r.ok, 3);
        assert_eq!(r.walk_errors, 0, "触顶不是遍历错误，不该混进 walk_errors");
        assert_eq!(doc_rows(&conn, "p1"), 3, "被丢弃的文件既不建行也不计数");
    }

    /// `running` 只由 `RunningGuard` 的 Drop 复位。`start` 的线程体在本任务不可测（要 AppHandle），
    /// 但「一次 panic 把 running 永久留在 true、界面上再也起不动第二轮、只能重启应用」是这个模块
    /// 最贵的一类失效，必须有一处能变红的断言钉住守卫本身。
    #[test]
    fn running_flag_is_released_even_when_the_job_panics() {
        let shared = Arc::new(IndexShared::new());
        shared.running.store(true, Ordering::SeqCst);
        let s = Arc::clone(&shared);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guard = RunningGuard::enter(&s);
            panic!("作业里炸了，这条用例的前提就是它要炸");
        }))
        .is_err();
        assert!(panicked, "内部没有 panic，这条用例什么都没测");
        assert!(!shared.running.load(Ordering::SeqCst), "panic 也必须释放 running");
    }

    /// 根目录没挂载是日常（移动盘/网络盘）。它只能让该项目自己标 root_missing，
    /// 不能中断整轮，也不能把别的项目的结果冲掉。
    #[test]
    fn missing_root_is_flagged_without_killing_the_run() {
        let good = tempfile::tempdir().unwrap();
        std::fs::write(good.path().join("验收.txt"), "甲方要求验收".as_bytes()).unwrap();
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", good.path());
        conn.execute("INSERT INTO projects (id, name, status) VALUES ('p2', '换盘项目', '立项')", []).unwrap();
        conn.execute("INSERT INTO project_dirs (id, project_id, kind, path) VALUES ('p2d', 'p2', 'root', 'Z:/没挂载/根目录')", [])
            .unwrap();

        let summary = run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(summary.state, "done");
        let p1 = summary.results.iter().find(|r| r.project_id == "p1").unwrap();
        let p2 = summary.results.iter().find(|r| r.project_id == "p2").unwrap();
        assert_eq!(p1.ok, 1, "可达项目照常索引：{p1:?}");
        assert!(p2.root_missing, "不可达项目必须被点名：{p2:?}");

        // 只指定 p2 时也必须正常返回（而不是 Err），UI 才能给出「根目录不可达」的提示
        let only_p2 = run_pass(&conn, &opts(), Some("p2"), false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(only_p2.results.len(), 1);
        assert!(only_p2.results[0].root_missing);

        // `rebuild && !root_missing` 的另一半：盘没挂载**不等于**文件被删了，重建不该顺手清掉
        // 这个项目已有的行。少了 `!root_missing` 这个条件，拔掉的移动盘会在下一次重建时
        // 把它的索引全冲成空表，而摘要照样报 done。
        // 用「先建后删的临时目录」造这个场景：p2 上面那条 SQL 里的 Z:/ 从来就没有行，
        // 拿它断「行数没变」是恒真的，什么也没证明。
        let removable = tempfile::tempdir().unwrap();
        std::fs::write(removable.path().join("验收.txt"), "甲方要求验收".as_bytes()).unwrap();
        let conn2 = db::open_in_memory().unwrap();
        seeded_project(&conn2, "p9", removable.path());
        run_pass(&conn2, &opts(), Some("p9"), false, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(doc_rows(&conn2, "p9"), 1, "前提：这个项目的行真的在库里");
        drop(removable); // 目录一旦没了，本轮既扫不到它、也不该清掉它的历史行
        let after = run_pass(&conn2, &opts(), Some("p9"), true, &no_cancel(), &mut |_| {}).unwrap();
        assert!(after.results[0].root_missing, "{:?}", after.results[0]);
        assert_eq!(after.results[0].scanned_total, 0);
        assert_eq!(doc_rows(&conn2, "p9"), 1, "根不可达时 rebuild 不该动它已有的行");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib index_job`
Expected: `unresolved module or crate` / `cannot find function run_pass`

- [ ] **Step 3: 写实现**

```rust
//! 索引作业。分成两层是刻意的：
//! - `run_pass` 纯函数，收 &Connection + 进度回调，内存库就能测完（取消、增量、重建、不可达根目录）。
//! - `start` 把回调换成 Tauri 事件，并负责 running/cancel 两个标志的生命周期（`RunningGuard`
//!   的 Drop 复位、`spawn` 起不来时返回错误而不是 panic）。它的线程体不在本任务的测试范围里
//!   （要 `AppHandle`），正确性由 Task 12 的真机验证承担 —— 别在这里写只能证明 mock 的测试；
//!   但 `RunningGuard` 的复位语义是可测的，`running_flag_is_released_even_when_the_job_panics` 钉它。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db;
use crate::error::{AppError, AppResult};
use crate::extract::extract_text;
use crate::index_scan::{scan_root, ScanOptions};
use crate::index_store::{clear_project, current_rowid, write_doc, DocOutcome};

pub const PROGRESS_EVENT: &str = "index://progress";
/// 每 20 个文件回一次进度：5 万文件全量跑，逐文件 emit 会把事件队列压垮，
/// 而界面上「10 秒动一次」已经够用。项目边界与结尾一定额外发一次。
pub const PROGRESS_EVERY: i64 = 20;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanTarget {
    pub project_id: String,
    pub project_name: String,
    pub root_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// running | done | cancelled | error。四个都有构造点：`running` 由 `progress_for` 发，
    /// `done`/`cancelled` 与 `error` 由 `terminal` 发（修复轮之前 `done`/`error` 一个都没有，
    /// 只订阅事件的前端因此等不到「这一轮结束了」的信号）。
    pub state: &'static str,
    pub project_id: String,
    pub project_name: String,
    pub total: i64,
    pub done: i64,
    pub ok: i64,
    pub skipped: i64,
    pub failed: i64,
    /// 当前正在处理的文件路径，用于界面显示「在做什么」。只读展示，不落日志。
    /// 终止事件（`terminal`）里是空串：那一刻不属于任何一个文件。
    pub current: String,
    /// 只有 `state == "error"` 时有值，是给界面直接念的一句话（含 `[code]` 前缀）。
    /// 单独立一个字段而不是塞进 `current`：`current` 的契约是文件路径，混用会让 Task 11
    /// 的「当前文件」栏显示成错误文案。
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResult {
    pub project_id: String,
    pub project_name: String,
    pub scanned_total: i64,
    pub ok: i64,
    pub skipped: i64,
    pub failed: i64,
    pub unchanged: i64,
    pub walk_errors: i64,
    pub capped: bool,
    pub root_missing: bool,
}

impl ProjectResult {
    /// 已处理数由四个计数器算出，不另存一个 `done` 字段 —— 少一处能写错的地方。
    fn done_total(&self) -> i64 {
        self.ok + self.skipped + self.failed + self.unchanged
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub state: &'static str,
    pub results: Vec<ProjectResult>,
    pub started_at: String,
    pub finished_at: String,
}

/// 只取未删除项目的 root 目录：一个项目一个根，是「一个根一个扫描任务」的前提（spec 数据模型）。
pub fn targets(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<ScanTarget>> {
    let sql = "SELECT d.project_id, p.name, d.path
                 FROM project_dirs d
                 JOIN projects p ON p.id = d.project_id
                WHERE d.kind = 'root' AND p.deleted_at IS NULL
                  AND (?1 IS NULL OR d.project_id = ?1)
                ORDER BY p.name";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![project_id], |r| {
        Ok(ScanTarget {
            project_id: r.get(0)?,
            project_name: r.get(1)?,
            root_path: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// 时间戳走 SQLite 的 `datetime('now')`：`chrono` 虽在 spec 第七节的清单里，但 Cargo.toml
/// 目前没引（M0-M2 没用到），为一个字符串新增依赖不划算，且这样和 V1-V4 里 `created_at` 同源。
fn now(conn: &Connection) -> String {
    conn.query_row("SELECT datetime('now')", [], |r| r.get(0)).unwrap_or_default()
}

pub fn run_pass(
    conn: &Connection,
    opts: &ScanOptions,
    project_id: Option<&str>,
    rebuild: bool,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> AppResult<RunSummary> {
    let started_at = now(conn);
    let mut results = Vec::new();

    for target in targets(conn, project_id)? {
        if cancel.load(Ordering::Relaxed) {
            return Ok(RunSummary { state: "cancelled", results, started_at, finished_at: now(conn) });
        }
        let mut acc = ProjectResult {
            project_id: target.project_id.clone(),
            project_name: target.project_name.clone(),
            ..Default::default()
        };
        let root_missing = !std::path::Path::new(&target.root_path).is_dir();
        acc.root_missing = root_missing;
        let outcome = if root_missing {
            // 根目录不在（盘没挂载）时连 walk 都不发起：walkdir 会逐条吐错误，白跑一趟。
            Default::default()
        } else {
            scan_root(std::path::Path::new(&target.root_path), opts)
        };
        if rebuild && !root_missing {
            clear_project(conn, &target.project_id)?;
        }
        acc.walk_errors = outcome.walk_errors.len() as i64;
        acc.capped = outcome.capped;

        // 超限文件也要建行：spec 要求「超限记 skipped + 原因」，这样 UI 才能回答「为什么它不在结果里」
        let mut queue: Vec<(crate::index_scan::ScannedFile, bool)> = outcome
            .files
            .iter()
            .cloned()
            .map(|f| (f, false))
            .chain(outcome.over_size.iter().cloned().map(|f| (f, true)))
            .collect();
        queue.sort_by(|a, b| a.0.path.cmp(&b.0.path));
        acc.scanned_total = queue.len() as i64;

        for (file, oversize) in queue {
            if cancel.load(Ordering::Relaxed) {
                on_progress(progress_for(&target, &acc, "cancelled", &file.path));
                // 半成品也要交出去：少这一行，取消时返回的 `results` 是**空表**，
                // 摘要里连被取消的那个项目都不在，Task 11 的界面只能显示「什么都没做」。
                // 探针实测原版没有这行时 `cancel_stops_the_pass_early` 当场 panic：
                // `index out of bounds: the len is 0 but the index is 0`。
                results.push(acc);
                return Ok(RunSummary { state: "cancelled", results, started_at, finished_at: now(conn) });
            }
            let handled = if oversize {
                write_doc(conn, &target.project_id, &file, DocOutcome::Skipped("too_large"))?;
                acc.skipped += 1;
                true
            } else if !rebuild
                && current_rowid(conn, &target.project_id, &file.path, file.size as i64, file.mtime)?
                    .is_some()
            {
                acc.unchanged += 1; // size + mtime 没变且上次 ok：连文件都不打开
                false
            } else {
                match extract_text(std::path::Path::new(&file.path)) {
                    Ok(body) if body.trim().is_empty() => {
                        // 扫描件没文字层是日常（spec 明写「抽出为空属于正常，不是失败」）。
                        // 库里那行由 write_doc 落成 skipped/empty_text，计数器就必须也加到 skipped 上，
                        // 否则 Task 11 的摘要说「N 个成功」而状态统计里它是 skipped，两个口径当场对不上。
                        // 顺带把 DocOutcome::Empty 变成有生产构造点（删枚举级豁免后它不然会报
                        // `variant Empty is never constructed`，见 Interfaces 的死代码账）。
                        write_doc(conn, &target.project_id, &file, DocOutcome::Empty)?;
                        acc.skipped += 1;
                    }
                    Ok(body) => {
                        write_doc(conn, &target.project_id, &file, DocOutcome::Ok(body))?;
                        acc.ok += 1;
                    }
                    Err(e) if e.code == "extract_unsupported" => {
                        // 走到这里说明清单与分派表漂移了（Task 5 的一致性测试没兜住）。
                        write_doc(conn, &target.project_id, &file, DocOutcome::Skipped("type_unsupported"))?;
                        acc.skipped += 1;
                    }
                    Err(e) => {
                        // 一个坏文件只废它自己：panic 已在 extract 层兜住，这里连 Err 也只落一行 failed。
                        write_doc(
                            conn,
                            &target.project_id,
                            &file,
                            DocOutcome::Failed(truncate(&e.message, 300)),
                        )?;
                        acc.failed += 1;
                    }
                }
                true
            };
            // 每 20 个有效处理回一次；unchanged 不占进度条的事件密度。
            if handled && (acc.ok + acc.skipped + acc.failed) % PROGRESS_EVERY == 0 {
                on_progress(progress_for(&target, &acc, "running", &file.path));
            }
        }
        on_progress(progress_for(&target, &acc, "running", ""));
        results.push(acc);
    }

    let state = if cancel.load(Ordering::Relaxed) { "cancelled" } else { "done" };
    Ok(RunSummary { state, results, started_at, finished_at: now(conn) })
}

fn truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// 进度事件由「当前扫描目标 + 累计计数」拼成。做成自由函数而不是循环外的闭包：
/// 闭包要同时按可变借用拿 `on_progress`、按借用拿循环内的 `target`，`run_pass` 里过不了借用检查。
fn progress_for(
    target: &ScanTarget,
    acc: &ProjectResult,
    state: &'static str,
    current: &str,
) -> Progress {
    Progress {
        state,
        project_id: target.project_id.clone(),
        project_name: target.project_name.clone(),
        total: acc.scanned_total,
        done: acc.done_total(),
        ok: acc.ok,
        skipped: acc.skipped,
        failed: acc.failed,
        current: current.to_owned(),
        error: None,
    }
}

/// 轮级终止事件。`run_pass` 只在项目边界发「这个项目」的进度，一轮结束/失败没有任何事件，
/// 只订阅事件的前端就只能反过来轮询 summary。`done`/`cancelled`/`error` 三个状态全靠这里发。
/// `project_id`/`project_name` 在终止事件里是空串 —— 它不属于某一个项目，Task 11 别拿它当 key。
fn terminal(state: &'static str, results: &[ProjectResult], error: Option<String>) -> Progress {
    let sum = |f: fn(&ProjectResult) -> i64| results.iter().map(f).sum();
    Progress {
        state,
        project_id: String::new(),
        project_name: String::new(),
        total: sum(|r| r.scanned_total),
        done: sum(|r| r.done_total()),
        ok: sum(|r| r.ok),
        skipped: sum(|r| r.skipped),
        failed: sum(|r| r.failed),
        current: String::new(),
        error,
    }
}

/// `running` 唯一的复位点。线程体正常走完、提前 `return`、还是 panic unwind，Drop 都会执行。
/// 少了它，一次 panic 就把 `running` 永久留在 true —— 界面上再也起不动第二轮，只能重启应用，
/// 而且没有任何日志说明为什么。`start` 里不许再出现第二处 `running.store(false)`
/// （唯一例外是 `spawn` 返回 Err 时守卫根本没构造出来，只能就地复位）。
struct RunningGuard(Arc<IndexShared>);

impl RunningGuard {
    fn enter(shared: &Arc<IndexShared>) -> Self {
        Self(Arc::clone(shared))
    }
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::SeqCst);
    }
}

pub struct IndexShared {
    pub cancel: AtomicBool,
    pub running: AtomicBool,
    pub summary: Mutex<Option<RunSummary>>,
    /// 上一轮为什么没跑成。三个失败出口（库打不开 / settings 读不出 / pass 中途报错）都必须
    /// 往这里写一句话，Task 10 的界面才能回答「点了索引按钮怎么什么都没发生」。
    /// 存 `format!("{e}")` 而不是 `AppError` 本体：它只 derive 了 `Debug`、没有 `Clone`，
    /// 而这里要跨线程留一份。代价是机器码混在串里，Task 10 若要按 code 分支再改存三元组。
    pub last_error: Mutex<Option<String>>,
}

impl IndexShared {
    #[allow(dead_code)] // 构造点在 Task 10 的 AppState::manage，落地时删掉本行
    pub fn new() -> Self {
        Self {
            cancel: AtomicBool::new(false),
            running: AtomicBool::new(false),
            summary: Mutex::new(None),
            last_error: Mutex::new(None),
        }
    }
}

/// 一轮彻底失败的出口：原因同时写进 `last_error`（给 Task 10 的 IPC 读）和一次 `error` 终止事件
/// （给只订阅事件的前端），两处用同一个串，界面上才不会「按钮说失败了、事件说没有」。
/// 失败轮的 `summary` 置 `None`：一轮没跑完就不该留下半截摘要，Task 11 显示的是「这一轮的结果」。
fn report_failure(emit: &mut impl FnMut(Progress), shared: &IndexShared, msg: String) {
    let event = terminal("error", &[], Some(msg.clone()));
    if let Ok(mut g) = shared.summary.lock() {
        *g = None;
    }
    if let Ok(mut g) = shared.last_error.lock() {
        *g = Some(msg);
    }
    emit(event);
}

/// 薄壳：自己开一条连接（不能借用 AppState 里那条 —— 一轮索引要几分钟，
/// 拿着 Mutex<Connection> 会把界面上所有 IPC 全卡住），把进度回调换成 emit。
/// WAL + busy_timeout 已在 db::after_open 里配好，两个连接一读一写是允许的。
#[allow(dead_code)] // caller 在 Task 10 的 index_start IPC，落地时删掉本行
pub fn start(
    app: AppHandle,
    data_dir: PathBuf,
    shared: Arc<IndexShared>,
    project_id: Option<String>,
    rebuild: bool,
) -> AppResult<()> {
    if shared.running.swap(true, Ordering::SeqCst) {
        return Err(AppError::new(
            "index_running",
            "已有一轮索引在跑",
            Some("先等它结束，或在索引页点「取消」"),
        ));
    }
    shared.cancel.store(false, Ordering::SeqCst);
    // 用 Builder 而不是 thread::spawn：后者在线程创建失败时**直接 panic**，而本机物理 16 GB、
    // commit charge 耗尽（os error 1455）是这轮开发里反复出现过的实况。起不来就返回错误，
    // 让 Task 10 的 IPC 能念出原因，而不是把 panic 丢进 Tauri 的命令线程里。
    let worker = Arc::clone(&shared);
    if let Err(e) = std::thread::Builder::new().name("index-job".to_owned()).spawn(move || {
        let _guard = RunningGuard::enter(&worker);
        let mut emit = |p: Progress| {
            let _ = app.emit(PROGRESS_EVENT, p);
        };
        let conn = match db::open(&data_dir) {
            Ok(c) => c,
            Err(err) => return report_failure(&mut emit, &worker, format!("{err}")),
        };
        let opts = match ScanOptions::load(&conn) {
            Ok(o) => o,
            Err(err) => return report_failure(&mut emit, &worker, format!("{err}")),
        };
        match run_pass(&conn, &opts, project_id.as_deref(), rebuild, &worker.cancel, &mut emit) {
            // state 由 run_pass 自己定（它末尾已经按 cancel 判过一次），这里不再重算第二遍。
            // 注意是 `Ok(s)` 而不是 `Ok(mut s)`：少了「外面再算一遍 state」之后没人改它，
            // 留着 `mut` 会被 `-D warnings` 打成 `variable does not need to be mutable`（控制方实测）。
            Ok(s) => {
                let event = terminal(s.state, &s.results, None);
                if let Ok(mut g) = worker.summary.lock() {
                    *g = Some(s);
                }
                if let Ok(mut g) = worker.last_error.lock() {
                    *g = None;
                }
                emit(event);
            }
            Err(err) => report_failure(&mut emit, &worker, format!("{err}")),
        }
    }) {
        // 线程根本没起来：闭包没跑过、守卫也没构造，`running` 只能在这里自己复位。
        shared.running.store(false, Ordering::SeqCst);
        return Err(AppError::new(
            "index_spawn_failed",
            &format!("索引线程启动失败：{e}"),
            Some("本机内存/提交空间不足时会出现，先关掉几个占内存大的程序再试"),
        ));
    }
    Ok(())
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib index_job`
Expected: `7 passed`；全量 → `94 passed; 0 failed`

`cancel_stops_the_pass_early` 若一次跑完（`done == 60`），原因是第一次回调就置位、而检查点在下一轮开头 —— 断言 `done < 60` 应当成立。若始终不成立，先确认 `PROGRESS_EVERY` 与回调时机，不要靠加 `sleep` 让它「看起来对」。（控制方探针实测：原版文本缺 `results.push(acc)` 时这条不是「跑完」而是**当场 panic** `index out of bounds: the len is 0 but the index is 0`；补上那行后全绿，`done` 落在 20 —— 那次实测时本模块是 5 条测试，修订后 7 条，这条断言本身没变。）

**报告里要交的变异证据（每条只该红一条）**：

以下 1–5 条控制方已在修订版夹具上逐条实测，红的那一条与文案都照抄：

1. 删掉循环内取消分支的 `results.push(acc);` → 只有 `cancel_stops_the_pass_early` 红（panic 文案见上）。
2. 删掉 `Ok(body) if body.trim().is_empty()` 那一臂（让空白正文重新走 `Ok`）→ 只有 `run_pass_indexes_a_generated_tree_and_separates_outcomes` 红，且**先红在 `ok` 计数**（测试里 `ok` 的断言排在 `skipped` 前，实测文案 `left: 3 / right: 2`；`skipped` 同时从 2 变 1，只是轮不到报）。这条变异的**第二半**：同一处改动下 `cargo clippy --lib -- -D warnings` 以 101 退出，报 `variant \`Empty\` is never constructed`（`index_store.rs:22`）。这就是为什么 `Empty` 臂必须落在生产分支里 —— 删了枚举级豁免后，没有生产构造点它就不干净。
3. 删掉 `if rebuild && !root_missing { clear_project(conn, &target.project_id)?; }` 整块 → 实测 `6 passed; 1 failed`，只红 `rebuild_rewrites_everything_and_leaves_no_stale_tokens`，文案 `旧正文要跟着清掉 / left: 1 / right: 0`。**这条是修复轮补出来的**：原版用例只准备一个文件、两轮之间路径不变，而 `write_doc` 是 `ON CONFLICT (project_id,path) DO UPDATE` + 按 rowid 先删 FTS 再插，旧正文本来就被子替换 —— 所以原版这处变异 **5 条全绿**（实现者实测），`clear_project` 在 `run_pass` 里零承重。夹具后来加的「第一趟在库里、第二趟磁盘上没了」那个文件才是它唯一能被测到的场景，同时补了正面照控和 `doc_rows` 的行数断言。
4. 删掉 `acc.capped = outcome.capped;` 那行 → 实测 `6 passed; 1 failed`，只红 `capped_and_scanned_total_survive_the_quota`（`assert!(r.capped, …)`）。
5. 把 `RunningGuard` 的 `Drop` 里那句 `running.store(false, …)` 注释掉 → 实测 `6 passed; 1 failed`，只红 `running_flag_is_released_even_when_the_job_panics`。**不要**改成靠 `impl Drop` 之外别的地方复位来「更方便」：这条用例存在的唯一理由就是「一次 panic 把 `running` 永久留在 true，界面上再也起不动第二轮」。

已登记的**不可检测项**（控制方实测过，别浪费时间去「修好它」）：

6. 把 `} else if !rebuild` 改成 `} else if true` → 实测 **7 条全绿**。原因写在 rebuild 用例的注释里：rebuild 趟开头 `clear_project` 已删光该项目的行，`current_rowid` 必然回 None，所以这个短路在 rebuild 趟里不可观测；而增量趟里 `!rebuild` 恒真，去掉它同样没有可观测差别。它和第 3 条覆盖的是同一段风险，第 3 条已经能红，所以这里不额外造夹具。

可选加测（控制方未跑，实现者若跑请交红绿两次的输出）：

7. 把增量分支 `!rebuild && current_rowid(...).is_some()` 改成恒 false → `second_pass_skips_unchanged_files` 必红。

**已被探针否掉的担心，别自己去「修」**：`//!` 模块 doc 里那个 `- ` bullet 列表**不触发** `doc list item without indentation`（原版逐字编译，两道闸 exit 0）。Task 8 那次中招的是「编号列表后面紧跟不缩进的正文行」，形态不同。

- [ ] **Step 5: 两道 clippy 闸 + 豁免对账 + 提交**

```bash
cd src-tauri && cargo clippy --lib -- -D warnings && cargo clippy --lib --all-targets -- -D warnings
grep -c "allow(dead_code)" src/index_store.rs   # 期望 3
grep -c "allow(dead_code)" src/index_scan.rs    # 期望 0
grep -c "allow(dead_code)" src/extract.rs       # 期望 1（supported_exts，caller 在 Task 10）
grep -c "allow(dead_code)" src/index_job.rs     # 期望 2（start、IndexShared::new）
```

四道数字全对上再提交。**`git add` 必须带上那三个被收掉豁免的文件** —— 原版只列了 `index_job.rs` 与 `lib.rs`，那样会把 7 行豁免删除留在工作树里不入库，提交后的树上 `grep -c` 与计划对不上（Task 8 犯过同一条，已修过一次）：

```bash
git add src-tauri/src/index_job.rs src-tauri/src/lib.rs \
        src-tauri/src/index_store.rs src-tauri/src/index_scan.rs src-tauri/src/extract.rs
git commit -m "feat: M3 索引作业：可测的 run_pass 与后台线程 emit 包装"
```

---

## Task 10: IPC 命令与 `AppState` 接线

**Files:**
- Modify: `src-tauri/src/lib.rs`（`AppState` 加 `index: Arc<IndexShared>`、5 个 `#[tauri::command]`、`generate_handler!` 注册、`fts5_available` 抽成函数）
- Test: 无新增 Rust 测试（见「为什么不写测试」）

**Interfaces:**
- Consumes: `index_job::{IndexShared, start, RunSummary, Progress}`、`index_store::{doc_hits, list_docs, status_counts, DocHit, DocRow, StatusCount}`、`index_scan::ScanOptions`、`extract::supported_exts`
- **顺带收掉的豁免（M3 的最后一批）**：`index_store.rs` 里 Task 9 之后剩的 `status_counts`/`list_docs` 两条，和 `extract.rs` 里 Task 5 留的 `supported_exts` 一条 —— 上面那行 Consumes 与 `supported_exts: extract::supported_exts().to_vec()` 就是它们的非测试调用点。提交前 `grep -n "allow(dead_code)" src/index_store.rs src/extract.rs` 期望**只剩 `extract.rs` 里与本项目无关的既有写法**（逐条对照 Task 9 的报告，别凭印象）。收干净之后整个 `src/` 里应当只剩 `db.rs` 那一条 WAL 用的豁免 —— 这条留给 Task 12 的收口检查（`grep -rn "allow(dead_code)" src/*.rs` 期望只有 `db.rs` 一行）。
- Produces（IPC 契约，前端 Task 11 按这些名字与字段写类型）:
  - `index_start(projectId?: string, rebuild: boolean) -> void`
  - `index_cancel() -> void`
  - `index_overview() -> IndexOverview`
  - `index_docs(projectId: string, status?: string, limit: number, offset: number) -> DocRow[]`
  - `search_docs(query: string, limit?: number) -> DocHit[]`

- [ ] **Step 1: 接线**

`AppState` 增加字段并在 `setup` 里初始化：

```rust
struct AppState {
    conn: Mutex<rusqlite::Connection>,
    data_dir: PathBuf,
    mk: Mutex<Option<MasterKey>>,
    /// 索引作业跨线程共享：running/cancel 是原子量，summary 是上一轮的结论。
    index: Arc<index_job::IndexShared>,
}
```

`setup` 里 `app.manage(AppState { conn: Mutex::new(conn), data_dir: data_dir.clone(), mk: Mutex::new(None), index: Arc::new(index_job::IndexShared::new()) })`。

把 `db_status` 里的 FTS5 探测抽成可复用的：

```rust
fn fts5_available(conn: &rusqlite::Connection) -> bool {
    conn.query_row(
        "SELECT 1 FROM pragma_compile_options WHERE compile_options LIKE '%FTS5%'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .is_ok()
}
```

`db_status` 内改为 `let fts5_available = fts5_available(&conn);`（局部变量与函数同名会遮住函数，改名 `has_fts`）。

- [ ] **Step 2: 五个命令**

```rust
#[tauri::command]
fn index_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: Option<String>,
    rebuild: bool,
) -> AppResult<()> {
    index_job::start(app, state.data_dir.clone(), state.index.clone(), project_id, rebuild)
}

#[tauri::command]
fn index_cancel(state: State<'_, AppState>) -> AppResult<()> {
    state.index.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectState {
    project_id: String,
    project_name: String,
    root_path: String,
    root_exists: bool,
    ok: i64,
    skipped: i64,
    failed: i64,
    pending: i64,
    missing: i64,
    total: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IndexOverview {
    running: bool,
    fts5_available: bool,
    last_run: Option<index_job::RunSummary>,
    projects: Vec<ProjectState>,
    exclude_dirs: Vec<String>,
    max_file_bytes: i64,
    max_files_per_project: i64,
    supported_exts: Vec<&'static str>,
}

#[tauri::command]
fn index_overview(state: State<'_, AppState>) -> AppResult<IndexOverview> {
    let conn = db(&state)?;
    let opts = index_scan::ScanOptions::load(&conn)?;
    let targets = index_job::targets(&conn, None)?;
    let mut projects = Vec::new();
    for t in targets {
        let counts = index_store::status_counts(&conn, &t.project_id)?;
        let pick = |s: &str| counts.iter().find(|c| c.status == s).map(|c| c.count).unwrap_or(0);
        projects.push(ProjectState {
            total: counts.iter().map(|c| c.count).sum(),
            project_id: t.project_id.clone(),
            project_name: t.project_name.clone(),
            root_exists: std::path::Path::new(&t.root_path).is_dir(),
            root_path: t.root_path,
            ok: pick("ok"),
            skipped: pick("skipped"),
            failed: pick("failed"),
            pending: pick("pending"),
            missing: pick("missing"),
        });
    }
    Ok(IndexOverview {
        running: state.index.running.load(std::sync::atomic::Ordering::SeqCst),
        fts5_available: fts5_available(&conn),
        // 上一轮结论只是展示用，拿不到锁就回 None，不因为 UI 刷新阻塞作业线程。
        last_run: state.index.summary.lock().ok().and_then(|g| g.clone()),
        projects,
        exclude_dirs: opts.exclude_dirs,
        max_file_bytes: opts.max_file_bytes as i64,
        max_files_per_project: opts.max_files_per_project,
        supported_exts: extract::supported_exts().to_vec(),
    })
}

#[tauri::command]
fn index_docs(
    state: State<'_, AppState>,
    project_id: String,
    status: Option<String>,
    limit: i64,
    offset: i64,
) -> AppResult<Vec<index_store::DocRow>> {
    let conn = db(&state)?;
    index_store::list_docs(
        &conn,
        &project_id,
        status.as_deref().filter(|s| !s.is_empty()),
        limit.clamp(1, 500),
        offset.max(0),
    )
}

#[tauri::command]
fn search_docs(
    state: State<'_, AppState>,
    query: String,
    limit: Option<i64>,
) -> AppResult<Vec<index_store::DocHit>> {
    let conn = db(&state)?;
    index_store::doc_hits(&conn, &query, limit.unwrap_or(50).clamp(1, 200))
}
```

`generate_handler![]` 末尾追加 `index_start, index_cancel, index_overview, index_docs, search_docs`。

- [ ] **Step 3: 为什么不写 Rust 测试**

这五个命令的函数体只做「取连接 → 调已测函数 → 组装展示结构」，业务分支全在 Task 7-9 已覆盖的纯函数里。要真正确认 IPC 往返成立，只有把应用跑起来点一遍 —— 那是 Task 12 的活。这里给它们加 mock 测试只会证明「mock 被调了」，是负价值的测试。**代价要认**：`index_start` 的线程壳、`app.emit` 的连通性、capabilities 是否需要新权限，全部到 Task 12 才见分晓。

- [ ] **Step 4: 编译门禁**

```bash
cd src-tauri && cargo clippy --lib --all-targets -- -D warnings && cargo build --lib
```

Expected: 零告警、`Finished`。同时确认前端事件名可用：`src-tauri/capabilities/default.json` 里已有 `core:default`（M2 验过），Rust→JS 的 `emit` 走它，无需新增权限条目。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: M3 索引相关 IPC 命令与作业状态接线"
```

---

## Task 11: 前端 `/index` 状态页

**Files:**
- Create: `src/types/index.ts`、`src/stores/index-job.ts`、`src/pages/index-status.tsx`
- Modify: `src/lib/api.ts`、`src/router.tsx`、`src/components/app-shell.tsx`
- Test: 无前端单测框架（`package.json` 只有 `dev/build/preview/tauri`），门禁是 `npm run build`（含 `tsc`）+ Task 12 真机断言

**Interfaces:**
- Consumes: Task 10 的 5 个命令与字段名
- Produces: 路由 `/index`；`useIndexJobStore()`（`progress`、`running`）；`api` 里的 `indexStart/indexCancel/indexOverview/indexDocs/searchDocs`

- [ ] **Step 1: 类型**

`src/types/index.ts`，字段名与 Task 10 的 `camelCase` 序列化一一对应：

```ts
export interface ProjectState {
  projectId: string;
  projectName: string;
  rootPath: string;
  rootExists: boolean;
  ok: number;
  skipped: number;
  failed: number;
  pending: number;
  missing: number;
  total: number;
}

export interface ProjectResult {
  projectId: string;
  projectName: string;
  scannedTotal: number;
  ok: number;
  skipped: number;
  failed: number;
  unchanged: number;
  walkErrors: number;
  capped: boolean;
  rootMissing: boolean;
}

export interface RunSummary {
  state: "done" | "cancelled" | "error";
  results: ProjectResult[];
  startedAt: string;
  finishedAt: string;
}

export interface IndexProgress {
  state: "running" | "done" | "cancelled" | "error";
  projectId: string;
  projectName: string;
  total: number;
  done: number;
  ok: number;
  skipped: number;
  failed: number;
  current: string;
}

export interface IndexOverview {
  running: boolean;
  fts5Available: boolean;
  lastRun: RunSummary | null;
  projects: ProjectState[];
  excludeDirs: string[];
  maxFileBytes: number;
  maxFilesPerProject: number;
  supportedExts: string[];
}

export interface DocRow {
  id: string;
  path: string;
  ext: string;
  size: number;
  status: string;
  skipReason: string | null;
  errorMsg: string | null;
  indexedAt: string | null;
}

export interface DocHit {
  docId: string;
  projectId: string;
  projectName: string;
  path: string;
  snippet: string;
  matchedBy: "exact" | "prefix";
  score: number;
}
```

- [ ] **Step 2: API 封装**

`src/lib/api.ts` 末尾追加（沿用文件里既有的 `invoke` 直调风格，不引入新抽象）：

```ts
import type {
  DocHit,
  DocRow,
  IndexOverview,
  IndexProgress,
} from "@/types/index";

export const indexStart = (projectId?: string, rebuild = false) =>
  invoke<void>("index_start", { projectId: projectId ?? null, rebuild });
export const indexCancel = () => invoke<void>("index_cancel");
export const indexOverview = () => invoke<IndexOverview>("index_overview");
export const indexDocs = (projectId: string, status: string | null, limit = 200, offset = 0) =>
  invoke<DocRow[]>("index_docs", { projectId, status, limit, offset });
export const searchDocs = (query: string, limit?: number) =>
  invoke<DocHit[]>("search_docs", { query, limit: limit ?? null });
export const onIndexProgress = (handler: (p: IndexProgress) => void) =>
  listen<IndexProgress>("index://progress", (e) => handler(e.payload));
```

`listen` 要从 `@tauri-apps/api/event` 引入（文件顶部现在是 `import { invoke } from "@tauri-apps/api/core";`，加一行 `import { listen } from "@tauri-apps/api/event";`）。

- [ ] **Step 3: store**

`src/stores/index-job.ts`：

```ts
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { indexCancel, indexOverview, indexStart, onIndexProgress } from "@/lib/api";
import type { IndexOverview, IndexProgress } from "@/types/index";

interface IndexJobState {
  overview: IndexOverview | null;
  progress: IndexProgress | null;
  error: string | null;
  load: () => Promise<void>;
  start: (projectId?: string, rebuild?: boolean) => Promise<void>;
  cancel: () => Promise<void>;
  subscribe: () => Promise<() => void>;
}

export const useIndexJobStore = create<IndexJobState>((set) => ({
  overview: null,
  progress: null,
  error: null,
  load: async () => {
    try {
      set({ overview: await indexOverview(), error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },
  start: async (projectId, rebuild) => {
    try {
      await indexStart(projectId, rebuild);
      set({ error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },
  cancel: async () => {
    await indexCancel();
  },
  subscribe: async () => {
    const unlisten = await onIndexProgress((p) => set({ progress: p }));
    // 一轮结束后再拉一次总览，把 lastRun 与各项目计数换成库里的真值
    return () => unlisten();
  },
}));

// 供页面在进度走到终态时刷新总览
export const isTerminal = (p: IndexProgress | null) =>
  p === null ? false : p.state === "done" || p.state === "cancelled" || p.state === "error";
```

- [ ] **Step 4: 页面**

`src/pages/index-status.tsx`。要点（都要写进去，它们就是 spec 的「可观测性」要求落地）：

- 顶部一行按钮：`建立索引`（增量）、`全量重建`（`rebuild: true`）、`取消`（仅 `running` 时可点）。
- 进度条：`done / total`（total 来自当前项目的 `scannedTotal`，扫完才有，所以开始阶段显示「正在扫描…」而不是假百分比）。
- **支持类型清单**：把 `overview.supportedExts` 原样列出，并配一句「清单之外的类型（图片、.doc/.ppt、压缩包）不会建行：图片等归 M6 OCR，.doc/.ppt 归 M7」。这是「为什么这个文件搜不到」的第一层答案。
- **超限与排除**：显示 `maxFileBytes`（换算成 MB）、`maxFilesPerProject`、`excludeDirs`。
- 项目表：每个项目一行 —— 名称、根目录（不可达时红标 `rootExists=false`，文案「根目录当前不可达，通常是盘没挂载」）、`ok / skipped / failed / pending / missing` 五个数字。选中一行后下方列出该项目的文件行（`indexDocs`），状态筛选下拉：全部 / ok / skipped / failed。
- 每条文件行展示 `skipReason`（`too_large` / `empty_text`）与 `errorMsg` 的中文映射，例如 `too_large` → `超出单文件上限`。映射写成 `src/pages/index-status.tsx` 内的一个 `const REASON_LABEL: Record<string, string>`，不要为它建 store。**不要给 `over_project_cap` 留映射**：触顶后被丢弃的文件根本不建行（`scan_root` 在 `break` 之后什么都不返回，见 Task 6 的 `capped`），M3 没有任何写入方；留着等于告诉用户「这里本该有一行」，是假的可观测性。上限这件事由下一行的项目级 `capped` 标注回答。
- 上一轮摘要：`lastRun.results` 里逐项目显示 `scannedTotal / ok / skipped / failed / unchanged`，并显式标出 `capped`（文案用 `已达单项目文件上限，本轮只处理前 N 个（含超限跳过的行），其余文件本轮没有行`）与 `rootMissing`、`walkErrors`。**别把 `scannedTotal` 念成「索引了 N 个」**：Task 6 的配额是在分「可抽取 / 超限」两桶**之前**判的（见 Task 9 Interfaces 的 capped 语义），这 N 个里含 `too_large` 的 skipped 行，而真正能抽正文的文件可能反而被挤掉了。
- **试搜区**：一个输入框 + 结果列表，直接吃 `searchDocs`。每条显示 `path`、`snippet`、`matchedBy`；`prefix` 那条要在旁边标「宽松匹配」，让用户知道这不是精确命中。这一区同时是 M4 的输入素材，本任务只求「能验」。
- 敏感字段脱敏的既有约定不影响本页（本页没有任何密文列）。
- 样式：全部用 Tailwind 工具类 + `@/components/ui` 里的 `Button`/`Card`/`Input`/`Select`/`Badge`；不新建样式文件。

生命周期：`useEffect` 里先 `load()`，再 `subscribe()`；收到终态事件时再 `load()` 一次；卸载时调用 unlisten。

- [ ] **Step 5: 路由与导航**

`src/router.tsx`：

```tsx
import { IndexStatusPage } from "@/pages/index-status";

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/index",
  component: IndexStatusPage,
});

const routeTree = rootRoute.addChildren([searchRoute, projectsRoute, projectDetailRoute, indexRoute]);
```

`src/components/app-shell.tsx` 的 `nav` 数组加一项（label 用「索引」，`to: "/index"`），其余照现有条目写。

- [ ] **Step 6: 编译门禁**

```bash
npm run build
```

Expected: `tsc` 无错误、vite 构建成功，模块数比当前（2113）增加。**注意**：`tsc` 会检查 `invoke` 的字段名与 Task 10 的 `camelCase` 是否一致，所以这一步同时是「IPC 契约两端对齐」的第一道闸。

- [ ] **Step 7: 提交**

```bash
git add src/types/index.ts src/stores/index-job.ts src/pages/index-status.tsx src/lib/api.ts src/router.tsx src/components/app-shell.tsx
git commit -m "feat: M3 索引状态页：进度、按状态翻文件清单、支持类型与上限说明"
```

---

## Task 12: 真机端到端验证 + 文档收口

**Files:**
- Modify: `docs/技术方案.md`（数据模型第 125-127 行按实测更正；M3 状态改「完成」）
- Modify: `docs/开发进度.md`（新增 `## M3 验收证据` + 变更日志一条）

**Interfaces:**
- Consumes: 前面所有任务的产出
- Produces: 真机证据 + 文档一致性

- [ ] **Step 1: 一次性沙盒，绝不碰真实数据目录**

```bash
mkdir -p /e/temp/pfm-m3/root/合同
cp src-tauri/tests/fixtures/sample-cn.pdf /e/temp/pfm-m3/root/合同/验收说明.pdf
```

另用 Rust 测试同款方式造 3 个文件放在同一目录：`GBK 说明.txt`（GBK 字节，含「响应时间不超过800毫秒」）、`报价单.xlsx`（含「付款条件」）、`维保.docx`（含 `合同&apos;验收` 与 `维保期`）。**不要**在 `E:\zero\demo\2026` 下同级其它项目目录里造文件或读它们的文件。

- [ ] **Step 2: 用覆盖 identifier 的方式起应用**

```bash
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" \
  npm run tauri dev -c '{"identifier":"dev.zero.pfm.m3test"}'
```

数据落在 `%APPDATA%\dev.zero.pfm.m3test\`，仓库文件与真实库都不受影响（这条手法来自 M2 验收）。

- [ ] **Step 3: CDP 驱动，断言只回布尔/条数**

连 `http://127.0.0.1:9222/json/list` 找到页面 `webSocketDebuggerUrl`，用 `Runtime.evaluate`（`awaitPromise: true, returnByValue: true`）在页面里发 `invoke`。三个必测点，脚本只打印断言结果，**不打印正文片段**：

1. `index_overview()` → `projects.length >= 1`、`fts5Available === true`、`supportedExts.includes("pdf")`。
2. 登记根目录（UI 上走项目详情 → 目录映射 → 手填 `E:\\temp\\pfm-m3\\root\\合同`）后 `index_start(null, true)`；等 `index://progress` 的 `state === "done"`，再 `index_overview()` 断言 `projects[0].ok >= 4`、`failed === 0`、`lastRun.state === "done"`。
3. `search_docs(...)` 的四条中文断言（这是 M3 的收口证据，逐条给数字）：
   - `"验收"` → 至少 2 条，且其中一条 `path` 以 `.pdf` 结尾（证明真中文 PDF 进了索引）。
   - `"维保"` → 至少 1 条且 `matchedBy === "prefix"`（证明两段放宽真的生效）。
   - `"付款条件"` → 至少 1 条（证明 xlsx + 复合词切分生效）。
   - `"响应时间"` → 至少 1 条，且 `snippet` 不含「 甲 方 」式散开（证明 GBK 与 `clean_snippet` 生效）。

   另外验一条负向：在台账里存一个密码为 `机密串-Zhang@2026`，`search_docs("机密串-Zhang@2026")` → `length === 0`。

- [ ] **Step 4: 上限与排除的真机抽查（可选但推荐）**

在沙盒根目录塞一个 > 20 MB 的 `.txt` 和一个 `node_modules/x.txt`，重跑 `index_start(null, true)`，断言 `/index` 页上该项目出现 1 条 `skipped`（`too_large`），且 `search_docs("不该被索引")` 仍为空。做完把它们删掉（只删自己造的文件）。

- [ ] **Step 5: 真实库只读复核**

```bash
ls -l "$APPDATA/dev.zero.pfm/" 2>/dev/null
```

Expected: 目录仍在，内容未被本轮改动（只有应用自身在真实进程里做的前向迁移可以动它；本次用的是 m3test，不应有任何变化）。跑完把 `dev.zero.pfm.m3test` 目录删干净（先确认没有孤儿 `project-files-manage.exe` 占着它：`Get-CimInstance Win32_Process -Filter "name='project-files-manage.exe'" | Select ProcessId,CommandLine`）。

- [ ] **Step 6: 更正 spec**

`docs/技术方案.md` 第 125-127 行改成实测口径：

```
index_docs          每个文件一行：doc_rowid(自增，给 FTS 当 rowid) / id(uuid) / path / ext /
                    size / mtime / index_status = pending|ok|skipped|failed|missing +
                    skip_reason + error_msg
index_docs_fts      FTS5 虚表：name_tokens + body_tokens，tokenize=unicode61；
                    词由 jieba cut_for_search 预切，rowid 与 index_docs.doc_rowid 对齐
```

并在第六节 6.1 补四条实测结论：查询走「精确段 → 前缀段」两段；未支持类型不建行（清单在 UI 上明示）；UTF-16 不用自己判 BOM，`encoding_rs` 的 `decode` 会用它盖过 chardetng 的猜测结果（chardetng 自己没有 UTF-16 模型）；图片/旧格式本版本明确不索引。里程碑表 M3 状态改「完成」。

- [ ] **Step 7: 写 M3 验收证据**

`docs/开发进度.md` 新增 `## M3 验收证据`，按 M2 那节的格式写全：

- `cargo test --lib`：`94 passed; 0 failed`，按模块拆一行（`db` / `project` / `ledger` / `vault` / `search` / `tokenize` / `extract` / `index_scan` / `index_store` / `index_job` 各几条，数字从实际输出抄，不要推算）。
- `cargo clippy --lib --all-targets -- -D warnings` 结果。
- `npm run build` 的模块数与耗时。
- Step 3 的逐条真机断言结果（只写「几条命中 / matchedBy 是什么 / 布尔值」，不抄正文）。
- 分词决策的证据：`维保` 需要前缀段、`付款条件` 需要 `cut_for_search`，各自在哪个测试里钉住。
- 快速复核：docx 实体在 quick-xml 0.41 里是独立事件，`xml10_content()` 不解实体，错写会得到 `合同&apos;…`（探针结论，已写进本计划的事实 10/11）。
- 已知缺口（照实写，别写成已证）：
  1. `pdf_text` 的 `catch_unwind` panic 分支本机无法构造样本（两个畸形 PDF 都走 Err），该分支可被重构静默删除而不触发测试 —— 记为无测试守护的 deferred minor。
  2. `index_start` 的线程壳与 `app.emit` 连通性没有 Rust 单测，只有 Task 12 的真机证据。
  3. 触顶（`capped`）后的剩余文件没有行，只在一轮摘要里点名项目。
  4. 索引大小/耗时仍是**真实数据目录上的一次实测**，几十 GB 全量的表现未测（spec 遗留项 1 的部分完成）。
  5. 加密 Office 文件（`by_name` 失败）落 `failed`，但没造出真实加密样本。
  6. `snippet` 对超长正文的截断点、以及「高亮位置映射回原文」仍归 M4。

- [ ] **Step 8: 提交**

```bash
git add docs/技术方案.md docs/开发进度.md
git commit -m "docs: 记录 M3 验收证据并按实测更正索引表设计"
```

---

## 已知缺口（整份计划的账，Task 12 会抄进进度文档）

- **性能数字仍是估算**：20 MB / 5 万上限、`PROGRESS_EVERY = 20` 都是拍出来的初始值，只在沙盒小样本上验过行为，没验过吞吐。真项目第一次全量跑之后可能要回头调批次与上限默认值。
- **未测分支**：`catch_unwind` 的 panic 分支、FTS 的 `missing` 状态（M5 才产生它）、**无 BOM 的 UTF-16**（chardetng 认不出来，仍会被猜成单字节编码解出 mojibake —— 记事本/PowerShell 写 UTF-16 都带 BOM，所以实际少见，但它是「不报错、只是搜不到」那个失效形态的残留口子）、UTF-32（头四字节 `FF FE 00 00` 会被 `for_bom` 匹配成 UTF-16LE，解出来是错的，encoding_rs 无 UTF-32）、更多编码（GB2312 与 GBK 同源，Big5 未测）。
- **召回的固有代价**：前缀段会放宽结果（`matchedBy = "prefix"` 就是为此存在的标注）。如果实际用起来噪音大，M4 可以按 `matchedBy` 分组而不是混排。
- **FTS 虚表的孤儿行**：`index_docs.project_id` 的 `ON DELETE CASCADE` 只带走主表行，虚表没有 FK、不受连带（事实 24），所以走 M1 的硬删项目路径会留下永不回收的 `index_docs_fts` 行。检索侧靠 `JOIN index_docs` 天然过滤，功能上看不见它们，代价只是索引体积。裁定：本轮不加触发器（那是过度设计），也不在半途改 M1 的删除路径；由 M5 的启动对账统一回收。Task 10 的接线者若顺手，可在 `index_start(rebuild)` 之外不做处理——重建本来就走 `clear_project`，会连带清掉虚表。
- **不做的东西**（避免下一个会话又去补）：不加 `content=` 外部内容表（正文不双存）、不引入 Tantivy、不做 notify（M5）、不做 OCR（M6）、不做 .doc/.ppt（M7）、不给前端加单测框架。

## 执行方式

计划写完。两种执行路径二选一：**Subagent-Driven**（每个任务派一个新子代理实现 + 两段评审，我在任务间做验收）或 **Inline**（本会话按批执行、到检查点停下给你看）。

