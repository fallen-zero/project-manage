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
| 24 | `after_open` 里 `PRAGMA foreign_keys=ON` 是生效的，所以 **插 `index_docs` 必须先有对应 `projects` 行**，否则 `FOREIGN KEY constraint failed`；Task 1 的两条建表测试因此各带一行 `INSERT INTO projects`（Task 7/8/9 的测试用 `seed_project()` 满足同一约束）。另一半：FTS5 虚表没有 FK，**不受 `ON DELETE CASCADE` 连带**——软删/硬删项目只会带走 `index_docs` 行，虚表留下孤儿行，只能由写入侧按 rowid 显式清（Task 7 的 `clear_project`/`delete_doc`），检索侧靠 `JOIN index_docs` 天然过滤 | 探针 `a18`（Task 1 落地时实测） |

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

测试基线：**动手前 `cargo test --lib` = 41 passed**（本轮已实测：`grep -c "#\[test\]"` 各模块 4+12+7+9+9 = 41，与实跑一致；临时探针已全部删除）。每个任务的 Expected 数字都从这条链往上加，最后到 **84**（+4 +5 +4 +4 +4 +5 +6 +6 +5 = 43 条新测试；Task 10/11/12 不新增 Rust 测试）。

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
- Test: `src-tauri/src/extract.rs` 的 `mod tests`（本任务四条全部喂字节串，**不碰文件系统**，所以不用 `tempfile`；落盘路径由 Task 5 的 `extract_one` 分派测试覆盖）

**Interfaces:**
- Consumes: `chardetng::EncodingDetector::new(Iso2022JpDetection::Deny)` + `guess(None, Utf8Detection::Deny)`、`encoding_rs`、`crate::error::{AppError, AppResult}`（`AppError::io(path, &e)` 已存在，`src-tauri/src/error.rs:21`，错误码 `fs_failed`）
- Produces:
  - `pub enum ExtractError` 的落点：本任务先只用 `AppError`，错误码固定两个 —— `extract_unsupported`、`extract_failed`（编码问题不新增码：UTF-16/GBK 只要解得出来就入库，见下面 `decode_text_bytes`）
  - `fn decode_text_bytes(bytes: &[u8]) -> String`（**不是 `AppResult`**：探测解不出时用替换字符照样入库，这条函数没有失败分支；包成 `Result` 会被 `cargo clippy -- -D warnings` 的 `unnecessary_wraps` 拦下）
  - `fn read_text_file(path: &Path) -> AppResult<String>`

  `fail` 与 `read_text_file` 在本任务**没有 caller**（前者第一次被 Task 4 的 Office 分支调用，后者被 Task 5 的 `extract_one` 调用），而 `mod extract;` 是私有模块，`pub` 也救不了 `dead_code`，`cargo clippy --lib -- -D warnings` 会当场红。两处各带一条 `#[allow(dead_code)]` 并在注释里写明是哪个任务接上，**Task 4/5 落地时必须删掉对应那行**（与 Task 2 的三条 `pub fn` 同形，那三条的删除责任已经记在 Task 7/8）。若 rustc 把不可达链条往下传、连 `decode_text_bytes` 也报 `never used`，就同样加一条 allow 并在报告里点名 —— 它的删除责任跟 `read_text_file` 一起走 Task 5，别顺手豁免到永久。

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

#[allow(dead_code)] // caller 是 Task 5 的 extract_one 分派，落地时删掉本行（decode_text_bytes 若一起被报，同批删）
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
  2. `office_text` 与 `sheet_text` 自己要到 **Task 5** 的 `extract_one` 分派才有 caller，`mod extract` 是私有模块、`pub` 救不了 `dead_code`，所以这两条各带一点名 Task 5 的 `#[allow(dead_code)]`，**Task 5 落地时删掉**。`xml_texts` 只被 `office_text` 调用，按上面第 1 条实测出的规律不会单独被报；真被报了才加，并在报告里点名。

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

    /// <w:instrText> 是域代码（HYPERLINK 之类），不是正文；只认 <w:t> 就自动跳过。
    #[test]
    fn docx_skips_field_codes_and_keeps_paragraph_breaks() {
        let dir = tempfile::tempdir().unwrap();
        let xml = r#"<w:document xmlns:w="http://x"><w:body><w:p><w:r><w:t>正文一</w:t><w:br/><w:t>正文二</w:t></w:r></w:p><w:p><w:r><w:instrText>HYPERLINK</w:instrText><w:t>正文三</w:t></w:r></w:p></w:body></w:document>"#;
        let p = make_office(dir.path(), "f.docx", "word/document.xml", xml);
        let text = office_text(&p, false).unwrap();
        assert!(text.contains('一') && text.contains('二') && text.contains('三'), "{text:?}");
        assert!(!text.contains("HYPERLINK"), "域代码不该进索引：{text:?}");
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
/// `check_end_names = false` 是因为
/// Office 的命名前缀只在文档内部一致，关掉能避免严格校验把整份文件判死。
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
            Event::Eof => break,
            Event::Start(e) if e.local_name().as_ref() == tag => inside = true,
            Event::Text(t) if inside => {
                let s = t
                    .xml10_content()
                    .map_err(|e| fail("extract_failed", &format!("XML 文本解码失败：{e}"), "文件可能已损坏"))?;
                cur.push_str(&s);
            }
            Event::GeneralRef(r) if inside => {
                // 载荷是裸片段（"apos" / "#39"），补回 & 和 ; 才能交给 unescape。
                let frag = String::from_utf8_lossy(r.as_ref()).into_owned();
                if let Ok(s) = quick_xml::escape::unescape(&format!("&{frag};")) {
                    cur.push_str(&s);
                }
            }
            Event::End(e) if inside && e.local_name().as_ref() == tag => {
                if !cur.trim().is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                inside = false;
            }
            _ => {}
        }
    }
    Ok(out)
}

/// `slides = false` 走 docx（word/document.xml + `<w:t>`），true 走 pptx（ppt/slides/slideN.xml + `<a:t>`）。
#[allow(dead_code)] // caller 是 Task 5 的 extract_one 分派，落地时删掉本行
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
#[allow(dead_code)] // caller 是 Task 5 的 extract_one 分派，落地时删掉本行
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

- [ ] **Step 1: 写失败测试（4 条）**

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

    /// 分派表和 supported_exts() 必须同源：UI 上写的「支持的类型」要是真的那一套。
    #[test]
    fn supported_exts_list_matches_the_dispatch_table() {
        for ext in supported_exts() {
            assert!(kind_of(ext).is_some(), "{ext} 在清单里却分派不到抽取器");
            assert_eq!(ext.to_lowercase(), *ext, "清单里的扩展名统一小写");
        }
        for kind_ext in ["txt", "md", "csv", "docx", "pptx", "xlsx", "xls", "pdf"] {
            assert!(supported_exts().contains(&kind_ext), "{kind_ext} 缺清单");
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
Expected: `12 passed`；全量 → `62 passed; 0 failed`

- [ ] **Step 5: 提交（含 fixture）**

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
- Consumes: `walkdir::WalkDir`（**没有 `WalkBuilder`**，事实 20）、`crate::extract::{kind_of, supported_exts}`、`rusqlite::Connection`（读 `settings`）
- Produces:
  - `pub struct ScanOptions { pub exclude_dirs: Vec<String>, pub max_file_bytes: u64, pub max_files_per_project: i64 }`
  - `impl ScanOptions { pub fn load(conn: &Connection) -> AppResult<ScanOptions> }`
  - `pub struct ScannedFile { pub path: String, pub file_name: String, pub ext: String, pub size: u64, pub mtime: i64 }`
  - `pub struct ScanOutcome { pub files: Vec<ScannedFile>, pub over_size: Vec<ScannedFile>, pub walk_errors: Vec<String>, pub capped: bool }`
  - `pub fn scan_root(root: &Path, opts: &ScanOptions) -> ScanOutcome`

- [ ] **Step 1: 写失败测试（5 条）**

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
        assert!(!paths.iter().any(|p| p == "dep.js" || p == "index.js" || p == "bundle.js"),
            "排除目录里的文件不该出现：{paths:?}");
    }

    /// 未支持类型完全不建行（几十 GB 里图片/二进制占大头，全建行会把表撑爆）。
    /// 「为什么这个文件搜不到」由 /index 页面上的支持清单回答，见 Task 11。
    #[test]
    fn unsupported_extensions_produce_no_rows() {
        let dir = tempfile::tempdir().unwrap();
        tree(dir.path());
        let out = scan_root(dir.path(), &opts(&[]));
        assert!(out.files.iter().all(|f| f.ext != "png" && f.ext != "zip"), "{:?}",
            out.files.iter().map(|f| &f.ext).collect::<Vec<_>>());
    }

    #[test]
    fn oversize_supported_files_are_separated_not_dropped() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "大文件.txt", &vec![b'a'; 100]);
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

    /// 根目录不存在（移动盘没挂载）是常态：回 walk_errors，不 panic、不返回 Err。
    #[test]
    fn missing_root_is_reported_instead_of_panicking() {
        let out = scan_root(Path::new("Z:/一定不存在/的根目录"), &opts(&[]));
        assert!(out.files.is_empty());
        assert!(!out.walk_errors.is_empty(), "不可达根目录要留下痕迹：{:?}", out.walk_errors);
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

use crate::extract::kind_of;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub exclude_dirs: Vec<String>,
    pub max_file_bytes: u64,
    pub max_files_per_project: i64,
}

impl ScanOptions {
    /// settings 缺失时退回 spec 的默认值；空字符串不当成「排除全部」也不当成「不排除」。
    pub fn load(conn: &Connection) -> crate::error::AppResult<ScanOptions> {
        let get = |key: &str, default: &str| -> String {
            conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
                .unwrap_or_else(|_| default.to_owned())
        };
        Ok(ScanOptions {
            exclude_dirs: get("index_exclude_dirs", "node_modules,dist,build,target,__pycache__,.git")
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect(),
            max_file_bytes: get("index_max_file_bytes", "20971520").parse().unwrap_or(20_971_520),
            max_files_per_project: get("index_max_files_per_project", "50000").parse().unwrap_or(50_000),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: String,
    pub file_name: String,
    pub ext: String,
    pub size: u64,
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
                if entry.file_type().is_dir()
                    && opts.exclude_dirs.iter().any(|d| d.eq_ignore_ascii_case(&name))
                {
                    it.skip_current_dir();
                    continue;
                }
                if !entry.file_type().is_file() {
                    continue;
                }
                let Some(scanned) = to_scanned(&entry) else {
                    continue;
                };
                if kind_of(&scanned.ext).is_none() {
                    continue; // 未支持类型不建行，见测试里的说明
                }
                if scanned.size > opts.max_file_bytes {
                    out.over_size.push(scanned);
                    continue;
                }
                if (out.files.len() as i64) + (out.over_size.len() as i64) >= opts.max_files_per_project {
                    out.capped = true;
                    break; // 触顶即停：剩下的文件不进清单，也没有行，由作业摘要点名项目
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
Expected: `5 passed`；全量 → `67 passed; 0 failed`

`missing_root_is_reported_instead_of_panicking` 若拿不到 `walk_errors`（walkdir 对不存在的根只吐一条 IOErr，确实会进错误流），就检查是不是 `Z:/` 被解析成了别的形态；**不要**改成断言「空清单即通过」——那会放过「根目录不可达却静默」这个真实故障。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/index_scan.rs src-tauri/src/lib.rs
git commit -m "feat: M3 目录扫描：排除规则剪枝、大小与项目上限、错误不中断"
```

---

## Task 7: `index_store.rs`（写侧）—— 两点写、幂等重写、状态统计

**Files:**
- Create: `src-tauri/src/index_store.rs`
- Modify: `src-tauri/src/lib.rs`（`mod index_store;`）
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: Task 1 的两张表、Task 2 的 `tokenize::index_text`、Task 6 的 `ScannedFile`、`uuid::Uuid`
- Produces:
  - `pub enum DocOutcome { Ok(String), Empty, Skipped(&'static str), Failed(String) }`（`Ok` 带正文，其它三种都不写 FTS 行）
  - `pub fn write_doc(conn: &Connection, project_id: &str, file: &ScannedFile, outcome: DocOutcome) -> AppResult<i64>` → 回 `doc_rowid`
  - `pub fn current_rowid(conn, project_id, path, size, mtime) -> AppResult<Option<i64>>`（增量跳过用）
  - `pub fn delete_doc(conn, project_id, path) -> AppResult<()>`
  - `pub fn clear_project(conn, project_id) -> AppResult<u64>`（重建前清场）
  - `pub struct StatusCount { pub status: String, pub count: i64 }`
  - `pub fn status_counts(conn, project_id: &str) -> AppResult<Vec<StatusCount>>`
  - `pub struct DocRow { pub id: String, pub path: String, pub ext: String, pub size: i64, pub status: String, pub skip_reason: Option<String>, pub error_msg: Option<String>, pub indexed_at: Option<String> }`
  - `pub fn list_docs(conn, project_id: &str, status: Option<&str>, limit: i64, offset: i64) -> AppResult<Vec<DocRow>>`

- [ ] **Step 1: 写失败测试（6 条）**

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
    }

    /// 事实 21：同路径重写不换行、不留重复 FTS 行（rowid 稳定 + 点删再插）。
    #[test]
    fn rewriting_the_same_path_replaces_instead_of_duplicating() {
        let c = conn();
        seed_project(&c, "p1");
        let f = file("C:/x/合同验收.docx");
        let r1 = write_doc(&c, "p1", &f, DocOutcome::Ok("第一版正文 验收".into())).unwrap();
        let f2 = ScannedFile { size: 2048, mtime: 1_700_000_999, ..f.clone() };
        let r2 = write_doc(&c, "p1", &f2, DocOutcome::Ok("第二版正文 报价".into())).unwrap();
        assert_eq!(r1, r2, "同一路径重复写入必须复用同一行");
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get::<_, i64>(0)).unwrap(), 1,
            "FTS 侧不该累积历史版本");
        let old: i64 = c
            .query_row("SELECT count(*) FROM index_docs_fts WHERE index_docs_fts MATCH ?1", params!["\"验收\""], |r| r.get(0))
            .unwrap();
        assert_eq!(old, 0, "旧正文不该还能搜到");
    }

    /// skipped/failed 只留状态行，不能往虚表塞空串：空串行会让 count 统计与 FTS 行数对不上。
    #[test]
    fn non_ok_outcomes_record_status_but_write_no_fts_rows() {
        let c = conn();
        seed_project(&c, "p1");
        write_doc(&c, "p1", &file("C:/a.docx"), DocOutcome::Skipped("too_large")).unwrap();
        write_doc(&c, "p1", &file("C:/b.docx"), DocOutcome::Failed("解包失败".into())).unwrap();
        write_doc(&c, "p1", &file("C:/c.docx"), DocOutcome::Empty).unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs", [], |r| r.get::<_, i64>(0)).unwrap(), 3);
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        let reason: Option<String> = c
            .query_row("SELECT skip_reason FROM index_docs WHERE path = 'C:/a.docx'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(reason.as_deref(), Some("too_large"), "超限原因要能被 UI 直接回答");
        let err: Option<String> = c
            .query_row("SELECT error_msg FROM index_docs WHERE path = 'C:/b.docx'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(err.as_deref(), Some("解包失败"));
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

    #[test]
    fn delete_and_clear_remove_both_sides() {
        let c = conn();
        seed_project(&c, "p1");
        write_doc(&c, "p1", &file("C:/1.docx"), DocOutcome::Ok("验收".into())).unwrap();
        write_doc(&c, "p1", &file("C:/2.docx"), DocOutcome::Ok("报价".into())).unwrap();
        delete_doc(&c, "p1", "C:/1.docx").unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get::<_, i64>(0)).unwrap(), 1,
            "删主表不删虚表会留下孤儿行，命中列表里会出现已消失的文件");
        clear_project(&c, "p1").unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(c.query_row("SELECT count(*) FROM index_docs_fts", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }

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
        let page = list_docs(&c, "p1", None, 2, 4).unwrap();
        assert_eq!(page.len(), 2, "分页要有效：{:?}", page.iter().map(|d| &d.path).collect::<Vec<_>>());
        assert!(page.iter().all(|d| !d.path.is_empty()));
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib index_store`
Expected: `unresolved module`

- [ ] **Step 3: 写实现**

```rust
//! 索引表的读写。两条不变量：
//! 1. 主表与 FTS 是两处存储，任何增删都要在同一个事务里做两笔（事实 3），否则命中列表里会
//!    出现已经不存在的文件。
//! 2. 只有 `DocOutcome::Ok` 会往虚表写东西。skipped/failed 只留状态行 —— 「为什么搜不到」
//!    的答案在行上，不在正文里，也就不该出现在搜索结果里。

use rusqlite::{params, Connection, Transaction};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::index_scan::ScannedFile;
use crate::tokenize::index_text;

/// `Ok(String)` 携带抽取到的正文，交给这里决定要不要落 FTS。
pub enum DocOutcome {
    Ok(String),
    Empty,
    Skipped(&'static str),
    Failed(String),
}

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

pub fn delete_doc(conn: &Connection, project_id: &str, path: &str) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    if let Some(rowid) = tx
        .query_row(
            "SELECT doc_rowid FROM index_docs WHERE project_id = ?1 AND path = ?2",
            params![project_id, path],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        tx.execute("DELETE FROM index_docs_fts WHERE rowid = ?1", params![rowid])?;
        tx.execute("DELETE FROM index_docs WHERE doc_rowid = ?1", params![rowid])?;
    }
    tx.commit()?;
    Ok(())
}

/// 全量重建前清场。逐行删而不是 `DELETE FROM index_docs_fts`：
/// 后者是 FTS5 的整表重建语句，代价高且会把别的项目一起清掉。
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

pub fn status_counts(conn: &Connection, project_id: &str) -> AppResult<Vec<StatusCount>> {
    let mut stmt = conn.prepare(
        "SELECT index_status, count(*) FROM index_docs WHERE project_id = ?1 GROUP BY index_status",
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

需要的引入：`use rusqlite::OptionalExtension;`（给 `.optional()`）、`use uuid::Uuid;`。`Transaction` 用不到就删掉那行引入，别留 unused import。

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib index_store`
Expected: `6 passed`；全量 → `73 passed; 0 failed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/index_store.rs src-tauri/src/lib.rs
git commit -m "feat: M3 索引写入层：两点写、同路径复用 rowid、状态与分页查询"
```

---

## Task 8: `index_store.rs`（检索侧）—— 两段 FTS 查询 + 「密文不进索引」的验收

**Files:**
- Modify: `src-tauri/src/index_store.rs`（新增 `doc_hits`）
- Test: 同文件 `mod tests` 追加 6 条

**Interfaces:**
- Consumes: Task 2 的 `query_expression` / `clean_snippet`、Task 1 的两张表、`crate::ledger` 与 `crate::vault`（只为验收测试造密文行）
- Produces:
  - `pub struct DocHit { pub doc_id: String, pub project_id: String, pub project_name: String, pub path: String, pub snippet: String, pub matched_by: &'static str, pub score: f64 }`（`#[serde(rename_all = "camelCase")]`）
  - `pub fn doc_hits(conn: &Connection, query: &str, limit: i64) -> AppResult<Vec<DocHit>>`

- [ ] **Step 1: 写失败测试（6 条）**

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
    fn chinese_word_hits_body_and_name_with_a_readable_snippet() {
        let c = seeded_with_docs();
        let got = hits(&c, "验收");
        assert_eq!(got.len(), 2, "{:?}", got.iter().map(|h| &h.path).collect::<Vec<_>>());
        assert!(got.iter().all(|h| h.matched_by == "exact"), "精确段就该命中：{:?}", got.iter().map(|h| h.matched_by).collect::<Vec<_>>());
        let hit = &got[0];
        assert_eq!(hit.project_name, "政务云迁移", "结果要带项目名，供 M4 分组");
        assert!(hit.snippet.contains('[') && hit.snippet.contains(']'), "摘要要标出命中词：{}", hit.snippet);
        assert!(!hit.snippet.contains(" 甲 方"), "摘要不该是散开的字：{}", hit.snippet);
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

        // 1) 用敏感字段的明文检索，一条都不该命中
        assert!(hits(&c, secret).is_empty(), "敏感字段明文进不了 FTS");
        // 2) 结构性检查：整张虚表里连这个字符串的片段都搜不到（不依赖分词是否切开）
        let needle = "%Zhang@2026%";
        for col in ["body_tokens", "name_tokens"] {
            let n: i64 = c
                .query_row(
                    &format!("SELECT count(*) FROM index_docs_fts WHERE {col} LIKE ?1"),
                    params![needle],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 0, "{col} 里不该存在敏感值：{n}");
        }
        // 3) 反向对照：同一行的明文列 note（交付时用）走的是库内字段检索，
        //    而 FTS 这边只有磁盘文件正文，两边互不污染
        assert_eq!(hits(&c, "付款条件").len(), 1);
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
    /// 已按 `clean_snippet` 收回 CJK 空格的摘要，命中词用 [ ] 包住
    pub snippet: String,
    /// exact | prefix：放宽过的命中要能被界面标出来，否则用户会以为是 bug
    pub matched_by: &'static str,
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

/// 两段式（中文分词检索的核心取舍）：
/// 1. 精确段：`"维保"` 要求词形完全一致，结果最准。
/// 2. 前缀段：0 命中时才放宽成 `"维保"*`，把被切成「维保期」这种长词的情况捞回来。
/// 只做第 1 段会表现为「功能没坏但搜不到」；直接只做第 2 段则精度差。
/// 两段用同一个 `query_expression`，即同一套预处理，见 tokenize.rs 的头注释。
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

`AppError` 需要引入：`use crate::error::AppError;`（本任务前 `index_store.rs` 只引了 `AppResult`，改成 `use crate::error::{AppError, AppResult};`）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib index_store`
Expected: `12 passed`（Task 7 的 6 + 本任务 6）；全量 → `79 passed; 0 failed`

若 `fts5_syntax...` 里 `hits(&c, "*")` 报错而不是空，说明 `query_expression("*", false)` 回了 `Some`——事实 8 要求纯标点被过滤后回 `None`；回到 Task 2 修过滤器，**不要**在这里加特判。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/index_store.rs
git commit -m "feat: M3 两段全文检索并钉住密文不进索引的验收"
```

---

## Task 9: `index_job.rs` —— 一次 pass 的纯函数 + 后台线程包装

**Files:**
- Create: `src-tauri/src/index_job.rs`
- Modify: `src-tauri/src/lib.rs`（`mod index_job;`）
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: `index_scan::{ScanOptions, scan_root}`、`extract::extract_text`、`index_store::{write_doc, current_rowid, clear_project, DocOutcome}`、`db::open`、`tauri::Emitter`、`std::sync::atomic::{AtomicBool, Ordering}`
- Produces:
  - `pub struct ScanTarget { pub project_id: String, pub project_name: String, pub root_path: String }`
  - `pub fn targets(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<ScanTarget>>`
  - `pub struct Progress { pub state: &'static str, pub project_id: String, pub project_name: String, pub total: i64, pub done: i64, pub ok: i64, pub skipped: i64, pub failed: i64, pub current: String }`（camelCase Serialize）
  - `pub struct ProjectResult { pub project_id: String, pub project_name: String, pub scanned_total: i64, pub ok: i64, pub skipped: i64, pub failed: i64, pub unchanged: i64, pub walk_errors: i64, pub capped: bool, pub root_missing: bool }`
  - `pub struct RunSummary { pub state: &'static str, pub results: Vec<ProjectResult>, pub started_at: String, pub finished_at: String }`
  - `pub fn run_pass(conn: &Connection, opts: &ScanOptions, project_id: Option<&str>, rebuild: bool, cancel: &AtomicBool, on_progress: &mut dyn FnMut(Progress)) -> AppResult<RunSummary>`
  - `pub struct IndexShared { pub cancel: AtomicBool, pub running: AtomicBool, pub summary: Mutex<Option<RunSummary>> }`
  - `pub fn start(app: AppHandle, data_dir: PathBuf, shared: Arc<IndexShared>, project_id: Option<String>, rebuild: bool) -> AppResult<()>`
  - `pub const PROGRESS_EVERY: i64 = 20;` / `pub const PROGRESS_EVENT: &str = "index://progress";`

**分层理由：** `run_pass` 不依赖 Tauri，收 `&Connection` + 回调，所以「扫 + 抽 + 写 + 取消 + 增量跳过」全部能在内存库 + tempdir 上测。`start` 只是「开一条连接、把回调换成 `app.emit`、维护 running/cancel」的薄壳，这一层的正确性由 Task 12 的真机验证负责，本任务不给它写假测试。

- [ ] **Step 1: 写失败测试（5 条）**

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
        std::fs::write(dir.join("图片.png"), b"\x89PNG").unwrap();
        std::fs::write(dir.join("合同/node_modules/x.txt"), "不该被索引".as_bytes()).unwrap();
    }

    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    /// 一轮 pass 要把四种结局各归其位：ok / skipped(too_large) / 未支持不建行 / 排除目录不进。
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
        assert_eq!(one.skipped, 1, "超限文件应记 skipped：{one:?}");
        assert_eq!(one.failed, 0);
        assert_eq!(one.scanned_total, 3, "未支持与排除目录不该计数：{one:?}");
        assert!(!events.is_empty(), "至少要有一次进度回调");
        assert_eq!(events.last().unwrap().done, 3);

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

    /// 重建：先把该项目的行与 FTS 清掉再写，所以第二趟不该出现 unchanged，旧内容也不会残留。
    #[test]
    fn rebuild_rewrites_everything_and_leaves_no_stale_tokens() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        let p = dir.path().join("合同.txt");
        std::fs::write(&p, "第一版 验收".as_bytes()).unwrap();
        let conn = db::open_in_memory().unwrap();
        seeded_project(&conn, "p1", dir.path());
        run_pass(&conn, &opts(), None, false, &no_cancel(), &mut |_| {}).unwrap();

        std::fs::write(&p, "第二版 报价".as_bytes()).unwrap();
        let summary = run_pass(&conn, &opts(), None, true, &no_cancel(), &mut |_| {}).unwrap();
        assert_eq!(summary.results[0].ok, 1, "重建必须重读：{:?}", summary.results[0]);
        assert_eq!(crate::index_store::doc_hits(&conn, "验收", 20).unwrap().len(), 0, "旧正文要跟着清掉");
        assert_eq!(crate::index_store::doc_hits(&conn, "报价", 20).unwrap().len(), 1);
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
//! - `start` 只是把回调换成 Tauri 事件，并维护 running/cancel 标志。它不在本任务的测试范围里，
//!   正确性由 Task 12 的真机验证承担 —— 别在这里写只能证明 mock 的测试。

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
    /// running | done | cancelled | error
    pub state: &'static str,
    pub project_id: String,
    pub project_name: String,
    pub total: i64,
    pub done: i64,
    pub ok: i64,
    pub skipped: i64,
    pub failed: i64,
    /// 当前正在处理的文件路径，用于界面显示「在做什么」。只读展示，不落日志。
    pub current: String,
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
    }
}

pub struct IndexShared {
    pub cancel: AtomicBool,
    pub running: AtomicBool,
    pub summary: Mutex<Option<RunSummary>>,
}

impl IndexShared {
    pub fn new() -> Self {
        Self { cancel: AtomicBool::new(false), running: AtomicBool::new(false), summary: Mutex::new(None) }
    }
}

/// 薄壳：自己开一条连接（不能借用 AppState 里那条 —— 一轮索引要几分钟，
/// 拿着 Mutex<Connection> 会把界面上所有 IPC 全卡住），把进度回调换成 emit。
/// WAL + busy_timeout 已在 db::after_open 里配好，两个连接一读一写是允许的。
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
    std::thread::spawn(move || {
        let finish = |summary: Option<RunSummary>, shared: &Arc<IndexShared>| {
            if let Ok(mut g) = shared.summary.lock() {
                *g = summary;
            }
            shared.running.store(false, Ordering::SeqCst);
        };
        let conn = match db::open(&data_dir) {
            Ok(c) => c,
            Err(_) => {
                finish(None, &shared);
                return;
            }
        };
        let opts = ScanOptions::load(&conn).ok();
        let mut emit = |p: Progress| {
            let _ = app.emit(PROGRESS_EVENT, p);
        };
        let summary = match &opts {
            Some(o) => run_pass(&conn, o, project_id.as_deref(), rebuild, &shared.cancel, &mut emit).ok(),
            None => None,
        };
        if let Some(mut s) = summary {
            s.state = if shared.cancel.load(Ordering::SeqCst) { "cancelled" } else { "done" };
            finish(Some(s), &shared);
        } else {
            finish(None, &shared);
        }
    });
    Ok(())
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib index_job`
Expected: `5 passed`；全量 → `84 passed; 0 failed`

`cancel_stops_the_pass_early` 若一次跑完（`done == 60`），原因是第一次回调就置位、而检查点在下一轮开头 —— 断言 `done < 60` 应当成立。若始终不成立，先确认 `PROGRESS_EVERY` 与回调时机，不要靠加 `sleep` 让它「看起来对」。

- [ ] **Step 5: clippy + 提交**

```bash
cd src-tauri && cargo clippy --lib --all-targets -- -D warnings
git add src-tauri/src/index_job.rs src-tauri/src/lib.rs
git commit -m "feat: M3 索引作业：可测的 run_pass 与后台线程 emit 包装"
```

---

## Task 10: IPC 命令与 `AppState` 接线

**Files:**
- Modify: `src-tauri/src/lib.rs`（`AppState` 加 `index: Arc<IndexShared>`、5 个 `#[tauri::command]`、`generate_handler!` 注册、`fts5_available` 抽成函数）
- Test: 无新增 Rust 测试（见「为什么不写测试」）

**Interfaces:**
- Consumes: `index_job::{IndexShared, start, RunSummary, Progress}`、`index_store::{doc_hits, list_docs, status_counts, DocHit, DocRow, StatusCount}`、`index_scan::ScanOptions`、`extract::supported_exts`
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
- 每条文件行展示 `skipReason`（`too_large` / `empty_text` / `over_project_cap`）与 `errorMsg` 的中文映射，例如 `too_large` → `超出单文件上限`。映射写成 `src/pages/index-status.tsx` 内的一个 `const REASON_LABEL: Record<string, string>`，不要为它建 store。
- 上一轮摘要：`lastRun.results` 里逐项目显示 `scannedTotal / ok / skipped / failed / unchanged`，并显式标出 `capped`（`已达单项目文件上限，本轮只索引前 N 个`）与 `rootMissing`、`walkErrors`。
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

- `cargo test --lib`：`84 passed; 0 failed`，按模块拆一行（`db` / `project` / `ledger` / `vault` / `search` / `tokenize` / `extract` / `index_scan` / `index_store` / `index_job` 各几条，数字从实际输出抄，不要推算）。
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

