# M8 设计 —— 备份包导出/导入（SQLite Backup API + SHA-256 + 覆盖可撤销）

依据 `docs/技术方案.md`（line 20「备份选 A：手动导出/导入备份包」、line 80 架构图 `backup` 格、§6.2 line 173-178 备份包口径、line 218 里程碑表 M8 行、line 234「索引表与台账表同库不同表，保证备份口径一致」）与 `docs/HANDOFF.md` §4 方向 A。执行方式仍是 `superpowers:subagent-driven-development`。

- **行号纪律**：本文所有 `文件:行号` 是 2026-10-13 在 `master`（M7 已合入，合并点 `da140a4`、收口提交 `00c6fce`）的当下读数，**只作对照，取数只认命令**。落点一律用符号定位：`grep -n "pub fn open\|fn migrate\|TARGET_VERSION\|pub struct AppState" src-tauri/src/db.rs src-tauri/src/lib.rs`。
- **本轮的性质与前七个里程碑不同，要写在最前面**：M0–M7 里工具在生产路径**只写一个文件**（`ledger.db`，`db.rs:16` 建目录 + SQLite 自管 `-wal/-shm`），且红线是「绝不动被索引的源文件」。M8 第一次让工具往用户指定的任意位置写文件、并第一次让工具**整库覆写自己正在用的库**。所以本文的 §三 失败面与 §六 契约不是配套章节，是本轮的主内容。
- **一条已被本轮推翻的旧措辞**：技术方案与 HANDOFF 从 M4 起一路写「零新 IPC（41）」，而 M7 已把它变成 **42**；`docs/HANDOFF.md:47` 那句「41」现在读的是 M6-era 的历史数。本轮基线**现取**：`generate_handler!` = **42**（块在 `lib.rs:659-702`，正则 `^ +[a-z_]+,?$`），M8 后为 **45**。这条从「写死数字」改为「每轮现取」的处置见 §四。

## 一、本轮交付与边界

**交付**：把「台账 + 项目 + 密钥库元数据」装进一个可移动的单文件备份包，并能在另一台机器（或同一台机器想回滚时）把它灌回正在运行的应用；索引按既定口径**不进包**。一句话——**给这个单机工具一种「带得走 + 退得回」**。

**六块交付物，只有 `backup.rs` 是全新的**，其余是接线、一处搬移与文档（§六 的落点表按文件摊成七行，§六 末尾的依赖序是九个可提交任务 —— 三个数各管各的，别互相换算，本仓这类账烧过多次）：

1. 新模块 `src-tauri/src/backup.rs`：`export` / `inspect` / `import` 三个纯函数 + `manifest` 的序列化对。
2. `scratch.rs`（新，从 `legacy_office.rs` **搬**过来）：一次性目录 + Drop guard，`legacy_office` 与 `backup` 共用。
3. `db.rs`：`fn migrate` 提可见性（导入后要能重放迁移）；`Cargo.toml` 给 `rusqlite` 开 `backup` feature、给 `sha2` 提直接依赖。
4. `lib.rs`：三条 IPC 命令（`backup_export` / `backup_inspect` / `backup_import`），**三条全标 `#[tauri::command(async)]`**（见 D11）。
5. 前端：新路由 `/backup` + `src/pages/backup.tsx` + `api.ts` 三层薄封装。
6. 文档回写 + 真机。

**边界（明确不做，写死）**：

- **附件不进包，本轮也不是「忘了做」**：技术方案 §6.2 line 178 写「附件（截图）一并放进 zip」，但附件对应的 `note_assets` 表与附件目录**在本仓从未存在**（全仓 grep 只在技术方案 line 124 命中，`db.rs` 的 V1–V4 里没有这张表，生产路径不落任何第二个文件）。本轮**点名缩量**：`format_version` 留位，附件打包跟着 `note_assets` 那一轮一起做。这与 M5 里程碑名里的 `notify` 缩量到 M5.5 是同一种处置（不静默跳过）。
- 不做定时/自动备份（技术方案 line 20 已定「手动」），不做备份包列表页，不做增量备份（只整库）。
- 不给包体加密封装、不设包口令（D6）。
- 不导入后就自动扫盘重建索引（D7），不做备份包内容的「合并/差异导入」（只有整库覆写）。
- 不新增设置开关，不新增 `index_status`/`skip_reason` 值。

## 二、决策与理由

需求方 2026-10-13 逐格采纳（下表每格的「选定」都是当场点选的推荐项，不是控制方代定）。

| 编号 | 决策 | 被否掉的备选与理由 |
|---|---|---|
| D1 | 导入承担**迁移 + 本机回滚双用**：覆盖当前库，但覆盖前先自动存一份现状 ⇒ 覆盖可撤销 | ① 只做换机器迁移（本机有数据就拒绝导入）：把「回到上周状态」这个最常见用途整条堵死，回滚要靠人手动拷文件。② 本轮只做导出：技术方案 §6.2 与里程碑表把导出/导入写在同一格，拆开要在文档里点名改口径。 |
| D2 | 落地机制用 `rusqlite::Connection::restore`，把包内库**灌回当前已打开的连接** | ① 覆盖 `ledger.db` 文件 + 重启：Windows 上那个文件正被应用自己开着，覆盖是整条路最脏的一笔，而且要把「重启前你得有个能写被占用文件的 shell」留给需求方。② ATTACH + 事务内逐表 `INSERT SELECT`：确实能回滚，但要手写表清单（D4 刚否掉白名单，这里会换个地方长回来），而 FTS5 虚表搬不动（`index_docs_fts` 是 `CREATE VIRTUAL TABLE`，不能 `INSERT SELECT` 跨库拄）。真实签名已读 vendored `rusqlite-0.40.2/src/backup.rs:64`（`Connection::backup`）与 `:102`（`Connection::restore`，`&mut self`）。 |
| D3 | 路径由用户当场选（原生「另存为」/「打开」），命令只接一个路径字符串 | 固定落数据目录 `backups/`：全程可自动验，但备份与库在同一块盘（本机 C/D/E 还是同一块 NVMe 的分区，见跨会话记忆），盘坏了一起没——而防整盘坏正是备份存在的理由。`dialog:default` 从 M1 起就在 capabilities，权限零改动。测试仍可 headless：对话框只是前端选路径的手段，不是命令的一部分（CDP 直接 `invoke("backup_export", {path})` 就能驱动真代码，M4/M5 的 `verify-tauri-ipc-via-seeded-sandbox-db` 手法同形）。 |
| D4 | 「索引不进包」的兑现 = **全量快照 → 在快照连接里 `DELETE` 掉 `index_docs` 与 `index_docs_fts` 的行 → `VACUUM`**，表结构照留 | ① 白名单逐表拷进空库：包最小，但「以后加了新表忘了列进白名单」= 静默丢数据，正是本仓最防的那类失效；判据上「排除两张派生表」比「记住其余全部」容易说对，而且删错会被「恢复后台账条数与导出前一致」当场抓红。② 连索引一起打包：违反技术方案 line 176 那条既定决策，且包体积随索引涨到几十 MB。③ 保留结构、只删行（不 `DROP TABLE`）：`DROP` 会让导入端的 schema 与 `TARGET_VERSION` 对不上，而 V4 的 DDL 没写 `IF NOT EXISTS`（`db.rs:120` 注释明写这个坑：重放同一版本会直接失败），删行则完全绕开这条。 |
| D5 | 三条新命令：`backup_export` / `backup_inspect` / `backup_import` ⇒ `generate_handler!` **42 → 45** | 两条（import 自带校验）省一条命令，但确认框就只能念「确定要用这个文件覆盖当前数据吗」，包里是什么要灌完才知道；schema 比本机新的包会走到 restore 那步才现形。第三条买到的东西具体：覆盖前看得见（时间、行数、schema 版本），且「包比本机新」这一类拒在写库之前。代价照 M7 的形制摊开：终审那道门基准数从 42 改 45，逐条复跑。 |
| D6 | 包体**不**加密封装、不设包口令；改为在 `/backup` 界面与 manifest 里**明说哪些列是明文** | ① 用主密码派生子密钥再封一层：零新记忆点，但新机器是空库（`vault_meta` 在包里而非本机），要先解包出 `vault_meta` 再解锁，导入变成多步交互，链条长一大截。② 单独设包口令：与主密码不同 ⇒ 换机器要拿两样东西，直接违反技术方案 line 150 那句「拿到备份包 + 主密码即可解密」，且忘口令 = 备份作废。诚实的代价写进 §九 风险 1：库里只封四列 `*_cipher`，`projects.name/code/manager/customer/contact_*/contract_*`、`ledger_servers.ip/bastion`、`ledger_credentials.url`、`ledger_links.url/title`、各表 `note` 全是明文列 —— 一个没加密的备份包 ≈ 一份内网入口清单。要防就在存放层防（BitLocker / 加密网盘），不在应用里再造第二套密钥体系。 |
| D7 | 导入后**不**自动起索引轮，只显一句提示 + 复用现成的「建立索引」按钮 | ① 自动起增量轮：省一次点击，代价是导完立刻后台扫几分钟（几十个根目录全量重读），笔记本拔线走人得记得去按取消。② 自动且 `rebuild=true`：`rebuild` 的全部作用是那一格 `if rebuild && !root_missing`（`index_job.rs:225`）里的 `index_store::clear_project`（`index_store.rs:120`），而导入后的索引表本来就是空的 ⇒ 为不存在的数据多付一次全清，零收益。**这条选项成立的前提已现读**：M5.5 的 ticker 只在 watcher 置脏时起轮（`watch.rs:126 due_for_pass`），所以「导入清空索引表」不会被它自动补回来 —— 不自动是真做得到，不是嘴上说说。 |
| D8 | 入口**新开一页 `/backup`**（nav 从 3 格变 4 格） | ① 挂 `/index` 底部卡片：零新路由最省，但备份里恰恰**不含**索引，挂那页是语义错位；且那页已承载支持清单、上限、运行态、`last_error`、OCR 与 LibreOffice 两行提示 + 安装按钮（`index-status.tsx:386-449`），再加两块就不认不出了。② 顶栏弹 Dialog（照 `VaultControl` 形制）：导入结果的长回执（包内摘要、SHA-256 不匹配是哪个文件、应急包完整路径）塞对话框里不好看。仓库现状：`app-shell.tsx:8-12` 只有三个 nav，**没有设置页也没有关于页**，所以这一格必须选。 |
| D9 | 一次性目录 guard 从 `legacy_office::Scratch`（`legacy_office.rs:134-165`）**搬到** `src-tauri/src/scratch.rs`，`new` 多收一个前缀参数（`pfm-m7` / `pfm-m8`），两个模块共用 | 在 `backup.rs` 再抄一份同样的 pid+seq+epoch-nanos+Drop-remove 逻辑是 verbatim duplication（评审 rubric 明令的红线）。搬移代价很小：`Scratch` 全仓只有 `legacy_office.rs` 内三处引用（`:217` 生产、`:374` 测试、注释两处），纯搬不改行为，M7 既有断言逐条原样跟着走。这是 brainstorming 允许的「顺手改善正在动的代码」，范围严格限于这一处。 |
| D10 | SHA-256 用 `sha2` 提为**直接依赖**，`rusqlite` 只加 `backup` **feature** | 仓库里没有现成的 SHA-256（`aes-gcm`/`argon2` 都不带 sha2；`zip` 8.6.0 只给 CRC32，验不了技术方案 line 175 那句 SHA-256）。关键成本核过：`sha2` **已在 `Cargo.lock`**（0.10.9 由 `lopdf`/`tauri-codegen`/`wry` 拉，0.11.0 由 `lzma-rust2` 拉），两个版本的 `.crate` 与解包源码都在本包缓存里 ⇒ 提为直接依赖**不新增下载、不新增编译单元**，`Cargo.lock` 的期望差异只有本包 `dependencies` 那一行。自己实现哈希是绝对红线（不写 crypto）。替代方案里 `windows` crate 的 CNG/BCrypt 能做但 Windows-only 且要多开 feature，为省一行依赖不值。 |
| D11 | **三条命令全标 `#[tauri::command(async)]`**（本仓 `command(async)` 从 1 条变 4 条），且函数体**全同步、一个 `await` 都不许有** | 本仓的 sync 命令睡的就是消息泵那一个线程（M4 那次 266,093 ms 慢预览量到的机制，`doc_preview.rs` 头注释记着后果：不回执、前端 `await` 永挂），所以任何秒级命令都必须挪出去——`export` 要跑 `VACUUM`、`inspect` 要对整份 db 字节算哈希、`import` 要灌整库，三者都是几十毫秒到几秒。**不逐条按「这条大概够快」分档**：那条判据靠猜，猜错的表现是界面冻一下没人报错，正是本仓最难事后归因的那类。**async 只当挪线程的开关用**，函数体保持同步：`Connection` 非 `Send`/`Sync`、`MutexGuard` 非 `Send`，一旦有 `await` 点跨住锁，future 就不 `Send`、编译直接拒 —— 这是「零 await」的机制护栏，不是一条风格偏好。**否掉的备选**：`tauri::async_runtime::spawn_blocking` 需要把连接变成 `Arc<Mutex<..>>` 塞进 `'static`，等于把 AppState 的所有权模型掀了重来（§十一 风险 7 钉死不许走）。 |
| D12 | 导出用**临时自开的第二条连接**（`db::open(&data_dir)`），导入用 `AppState.conn` 那把锁 | 导出若持 `AppState.conn` 的 std Mutex 做 VACUUM，期间界面上每个读写命令都在锁上排队（不报错，纯等），而 `VACUUM` 恰好是最长的一步。仓库本就有第二条连接的生产先例：worker 每轮 `db::open`（`index_job.rs:468`）、watcher 一条常驻（`watch.rs:70`）。**但导入用的应急包那一次导出反其道走 `AppState.conn` 的锁**：它必须抓「即将被覆写的那份库」的此刻状态，且它整段都在同一条写锁内，排队是免费的。 |
| D13 | `backup_import` 在同一把 `state.conn` 写锁**之内**依次做：查 `index.running` → 解包 → 写应急包 → `restore` → `migrate` | 把「查闸门 + 写应急包 + 覆盖」收进同一把锁，让「应急包失败必定早于覆写」在**结构上**成立而不是靠顺序写对。闸门口径：`index.running` 为真 ⇒ 回 `backup_busy`，提示「先等这一轮结束或点取消」。 |

## 三、失败面（本轮主内容）

`restore` 按页拷贝、无回滚，rusqlite 对 `Busy` 只重试 3 次 ×100 ms 就报错（vendored `backup.rs:117-131` 实读）。所以**导入侧不吹「全有或全无」**，只保证「失败点尽量前移 + 失败后有确定的撤销路」：

**导入的四道前置**（全过了才动库）：① zip 能解、`manifest.json` 能读；② 每个条目 SHA-256 与 manifest 相符；③ `schema_version <= TARGET_VERSION`（包来自更新版应用 ⇒ 拒，绝不拿旧代码跑新 schema）；④ 应急包已成功写进数据目录。之后 `restore` 真失败，库里就是半新半旧，**唯一救法是拿刚落在数据目录的那份 `pre-restore-*.zip` 再导一次** —— 这就是 D1 那条「覆盖可撤销」的兑现物，也是应急包不做成贴心功能、做成承重件的原因。

**四道前置各自都能本机证伪**（逐条给得出读数）：① 往包里塞非 zip 字节 / 删掉 `manifest.json`；② 翻 `backup.db` 一个字节；③ 手造 manifest 写 `schema_version = 9`；④ 把应急包落点做成不可写（只读夹具目录）。四条的期望是同一个形状：**调用前后活库逐字相同**（`projects` 行数 + 某一行的完整内容 + `PRAGMA integrity_check`），且返回码等于那一条专属错误码 —— 只断「报错了」不够，因为「报错但已经写了一半」也想通过它。另配一条反向对照：同一夹具去掉前置④的不可写条件后导入必须成功，证明这四道不是恒红摆设。

| 失败点 | 后果 | 库的状态 | 处置 |
|---|---|---|---|
| zip 解不开 / manifest 缺或坏 | 报错 | **未动** | `backup_package_invalid` |
| 条目哈希与 manifest 不符 | 报错，指出是哪个文件 | **未动** | `backup_hash_mismatch`（`message` 带条目名，不带正文） |
| `schema_version > TARGET_VERSION` | 报错 | **未动** | `backup_schema_too_new`，提示先升级应用 |
| `index.running` 为真 | 报错 | **未动** | `backup_busy`，提示等一轮或取消 |
| 应急包写失败（盘满/权限） | 报错 | **未动** | `AppError::io`，复用现成码 |
| `restore` 中途失败（Busy 超 3 次 / IO） | 报错 | **可能半新半旧** | `backup_restore_failed`，hint 直接给出那份 `pre-restore-*.zip` 的完整路径 |
| `restore` 后 `migrate` 失败 | 报错 | 库已是包内容，版本落后 | `backup_migrate_failed`，hint 同给应急包路径 |
| `restore` 后现读 `journal_mode` 不是 wal | 报错 | 库已是包内容，且**持久化模式已退化** | `backup_wal_lost`，hint 给应急包路径并点名「这份库不再受断电保护」 |
| 导出侧任一步失败 | 报错 | **绝不碰 `ledger.db`**（快照在临时目录，guard 删干净） | 目标位置最多留一个半截 zip，不碰任何既有文件 |

**密钥侧**：导入成功后 `AppState.mk` 无条件置 `None`，界面回未解锁态。理由：包里的 `vault_meta` 可能出自换主密码之前，用旧 MK 解不开新库 —— 让它当场报「需要解锁」比悄悄错好。这条不需要新命令，`backup_import` 返回后由命令层清。

**红线一句话**：备份/导入只读库、只写「用户当场选中的那个包路径」+「自己数据目录里的应急包」，**永不碰被索引的源文件**（D3 的「导出不动盘」）。

## 四、契约与结构门

- **`generate_handler!` 42 → 45**（本批第二次破「零新 IPC」，需求方 2026-10-13 知情批准）。守护沿用 M7 那条形制并加一条：三条命令的**入参只有一个 `path: String`**（不接受第二个参数，尤其不接受命令名/包 ID 那类可执行内容），且 `WINGET_PACKAGE_ID` 那种「硬编码在 Rust 侧」的原则本轮没有新增项 —— 备份命令不做任何「起进程」的事。
- **零新迁移**：`TARGET_VERSION` 保持 **4**（备份包不新增列，manifest 是文件不是表）。
- **不扩码表**：`index_status` 五值、`skip_reason` 值域一字不动；新错误码是 `AppError.code` 的字符串（自由取值，M4 起就不是枚举）。
- **新线格式**：三个新返回结构（`BackupSummary` / `PackageInfo` / `ImportOutcome`）是**新命令自带的新线格式**，不动既有断言 —— `SearchBundle` 七键（`search.rs:796/799`，后者是 `assert_eq!(obj.len(), 7)`）、`DocPreview` 六键（`doc_preview.rs:386`）两条断言字节不动且必须继续绿。**`IndexOverview` 本轮不加字段**（备份页自己拉自己的数据，不搭索引概览的车）。
- **两处「全仓唯一」的旧措辞会失真，必须回写**：① `#[tauri::command(async)]` 从 1 条变 **4** 条（唯一现存那条是 `doc_preview`，`lib.rs:577`）；② 「生产路径唯一落盘文件是 `ledger.db`」变多（包路径与 `pre-restore-*.zip`）。技术方案 line 175 那句「各文件 SHA-256」本轮兑现，但它同时含**附件**那半格 —— 按 §一 缩量口径写明只做了一半。
- **本轮用到的外部 API 全部实读过签名**（红线 A，计划阶段不许凭印象写）：
  - `rusqlite-0.40.2/src/backup.rs:64` `Connection::backup<N: Name, P: AsRef<Path>>(&self, name, dst_path, Option<fn(Progress)>) -> Result<()>`；`:102` `Connection::restore<N, P, F: Fn(Progress>>(&mut self, name, src_path, Option<F>)`。**两者 progress 参数类型不对称**（backup 收 `Option<fn(Progress)>` 函数指针，restore 收泛型 `Option<F>`）⇒ `None` 直传会歧义，各自要显式标注；`restore` 要 `&mut self`，所以 `db(&state)` 那条返回 `MutexGuard` 的现成 helper 得配 `mut` 绑定（`let mut conn = db(&state)?;`）。模块门在 `rusqlite-0.40.2/src/lib.rs:118-119` `#[cfg(feature = "backup")] pub mod backup;` ⇒ 不开 feature 连 `MAIN_DB` 之外的这条路整个不存在。
  - `restore` 的 Busy 语义 `backup.rs:117-131`：`while r == More || r == Busy`，Busy 计数到 3 就 `break` 并回 `SQLITE_BUSY`（每次之间 sleep 100 ms）⇒ **最多重试 3 次**，这是 §三 那条「restore 中途失败可能是半新半旧」的机制出处，不是推测。
  - `zip-8.6.0` 写侧**没有 `add_data`**（`grep "pub fn add_data" src/write.rs` 零命中）：只有 `start_file<S: ToString, T: FileOptionExtension>`（`write.rs:1501`）+ `impl<W: Write + Seek> Write for ZipWriter<W>`（`write.rs:761`，所以写字节走 `std::io::Write::write_all`）+ `finish(self) -> ZipResult<W>`（`write.rs:1795`）。`SimpleFileOptions` 从 `zip::write` 再导出（`write.rs:27`），根只导出 `ZipWriter`（`lib.rs:23`）。
  - `sha2-0.11.0/src/lib.rs:36` `Sha256` 由 `digest::buffer_fixed!` 生成，`Digest` 从 `:19` 再导出（`use sha2::{Digest, Sha256}`）；`digest-0.11.3` 与 `sha2-0.11.0` 的 `rust-version` 均为 **1.85**，与仓库 `Cargo.toml:6 rust-version = "1.85"` 齐平 ⇒ 版本地板不冲突。hex 用 `bytes.iter().map(|b| format!("{b:02x}")).collect()`，**不**为 hex 再加一个 crate。
- **依赖面**：`rusqlite` features `["bundled"]` → `["bundled", "backup"]`（**不是新 crate**）；`sha2` 提为直接依赖（已在锁内，见 D10）。门：**期望 `Cargo.lock` 只多本包 `dependencies` 里那一行引用，不新增 `[[package]]`**；实现者要贴 `git diff Cargo.lock` 的原文读数。若 `sha2` 因此被统一到单版本（少一个 `[[package]]`），那是改进、如实记下，不许当成"反正锁文件变了"放过。

## 五、包格式（`format_version = 1`）

```
pfm-backup-2026-10-13T2130.zip          ← 用户选的目标路径，界面给默认名
├── manifest.json
└── backup.db                             ← 故意不叫 ledger.db：避免被双击或拷回数据目录当库文件用
```

`manifest.json`（serde，`rename_all = "camelCase"`，与仓库出参口径一致）：

```json
{
  "formatVersion": 1,
  "appVersion": "0.1.0",
  "schemaVersion": 4,
  "createdAt": "2026-10-13T21:30:12Z",
  "indexExcluded": true,
  "counts": { "projects": 12, "ledgerEnvs": 30, "ledgerCredentials": 48,
              "ledgerServers": 15, "ledgerLinks": 20, "ledgerNotes": 40, "settings": 3 },
  "files": [ { "name": "backup.db", "size": 40960, "sha256": "<hex>" } ],
  "plaintextWarning": "包内 projects/ledger_*/settings 的非 *_cipher 列为明文，含 ip/bastion/url/note"
}
```

- `counts` 在导出时从**快照连接**现数（不是从活库数），这样 inspect 念出来的「含 12 个项目」与包内容必然同源。数某张表失败**不吞**：`export` 直接返回 `Err` 中止本次导出（宁可报「导出失败」，也不产出一份数字不全的 manifest —— 「看得见包内有什么」是 D5 花一条命令换来的，不能是空的）。注意这里没有 `index_status`/`skip_reason` 什么事：那两个词是索引轮的字段，备份命令的失败面只有 `AppError`（§七 最后一条）。
- 哈希对象 = 写入 zip 的**条目原始字节**（压缩前）；验算时对解压出的字节算。
- 防 zip-slip：只按 `files[].name` 里那一个固定条目名取，包内其他条目一律忽略；解出后落临时目录自己指定的文件名，绝不用包内名字拼路径。
- `formatVersion` 只保证「同版本读写 + 拒绝更高 `schemaVersion` 的包」，不做跨版本兼容矩阵（YAGNI；真要有第二个格式版本时再谈）。

## 六、落点（按文件七行）

| # | 文件 | 动作 | 要点 |
|---|---|---|---|
| 1 | `src-tauri/Cargo.toml` | 改 | `rusqlite = { version = "0.40.2", features = ["bundled", "backup"] }`；`sha2 = "0.11"`（版本以实锁为准，先试已在锁内的 0.11.0）。核 `git diff Cargo.lock` 期望差异。 |
| 2 | `src-tauri/src/scratch.rs` | **新建（搬移）** | 把 `legacy_office.rs:134-165` 的 `Scratch` 原样搬来，`new(tag)` → `new(prefix, tag)`；`legacy_office.rs` 改 `use crate::scratch::Scratch;` 并传 `"pfm-m7"`。零行为改动，M7 的 `:374` 那条测试跟着搬。 |
| 3 | `src-tauri/src/db.rs` | 改 | `fn migrate` 提为 `pub(crate) fn migrate`（导入后要能重放）；把 `db.rs:47-62` 那段「现读 `PRAGMA journal_mode` 断等于 wal」拆成 `pub fn assert_wal(conn) -> AppResult<()>`，`after_open` 改调它（**行为零改动**，M0 既有那条断言必须继续绿，另加一条「拆出来的 `assert_wal` 与拆前在同一夹具上同红同绿」的钉值）。**不动** `open`/`after_open` 的其余逻辑与 `TARGET_VERSION`。 |
| 4 | `src-tauri/src/backup.rs` | **新建** | `export(&Connection, dst: &Path) -> AppResult<BackupSummary>`（内部：`conn.backup(MAIN_DB, tmp/db)` → 打开该快照 → DELETE 两张索引表 → `VACUUM` → 现数 counts → 关闭快照连接 → 读字节算 SHA-256 → `ZipWriter` 落 dst → 删临时）；`inspect(src: &Path) -> AppResult<PackageInfo>`（纯只读，不碰任何连接）；`import(&mut Connection, src: &Path) -> AppResult<ImportOutcome>`（四道前置 → `restore(MAIN_DB, 临时 db)` → `db::migrate`）。hex 用 `bytes.iter().map(|b| format!("{b:02x}"))`，**不**再为 hex 加依赖。 |
| 5 | `src-tauri/src/lib.rs` | 改 | `mod backup; mod scratch;`；三条 `#[tauri::command(async)]`，**锁的取法按 D12 分两种**：`export` 只用 `state.data_dir` 现开一条临时连接（不碰 `state.conn`），`inspect` 不碰任何连接，`import` 取 `let mut conn = db(&state)?` 并在**同一把锁内**按 D13 顺序做完（查 `state.index.running` → 应急包 → `restore` → `migrate`）；导入成功后 `*state.mk.lock() = None`。 |
| 6 | `src/types/backup.ts` + `src/lib/api.ts` + `src/pages/backup.tsx` + `src/router.tsx` + `src/components/app-shell.tsx` | 改/新建 | nav 加 `{ to: "/backup", label: "备份" }`（`app-shell.tsx:8-12`）；页面三块：导出（`save` 对话框）、选包验包（`open` 对话框 + 摘要表）、导入（确认框 + 结果回执含应急包完整路径 + 索引重建提示 + 「建立索引」按钮）。busy 形制照 `index-status.tsx:74-81` 的 `installBusy`/`installError` 本地 state，错误回执照 `:440-449`。 |
| 7 | `docs/技术方案.md` / `docs/开发进度.md` / `docs/HANDOFF.md` | 改 | 见 §十二。 |

依赖序：T1（scratch 搬移 + Cargo 两个开关，纯搬零行为）→ T2（manifest 纯核：结构体 + 序列化 + hex + counts 查询）→ T3（export）→ T4（inspect）→ T5（import）→ T6（三条 IPC + 结构门）→ T7（前端 `/backup`）→ T8（headless 端到端往返）→ T9（真机 + 文档）。**同 worktree 串行提交**（每格一个会提交的实现者，只读评审可并行）。

## 七、判据

- **「索引不进包」要能被正面验到**：往返测试断言恢复后的 `index_docs` 行数为 0，且 `settings` 与台账行数与导出前逐字相同 —— 只断「台账在」不算，因为把索引一起打包也能让台账在。
- **「应急包早于覆写」要能被结构验到**：T5 里按四道前置各造一条失败，逐条断「库未变」；再加一条「应急包路径出现在 restore 失败的 `hint` 里」（失败可读性也是判据）。
- **快照连接必须先关再读字节**：`backup()` 写完 dst、`VACUUM` 跑完后必须 drop 掉那条连接，才能把文件读进 zip。原因现读自本仓：`db.rs:479` 注释「最后一个连接关闭时 SQLite 会 checkpoint 并删除 -wal 侧文件」—— Backup API 按页拷贝含 db 头，WAL 位是随头页带进快照的，不关连接就打包会拿到「带未 checkpoint WAL 引用」的 db（技术方案 line 175 明令避免的正是这个）。T3 要有一条断言：包里的 `backup.db` 单独 `Connection::open` 后 `PRAGMA integrity_check` 返回 ok，且临时目录里没有 `-wal` 残留进包。
- **覆写之后 WAL 这条硬门必须再读回一次**：M0 那条「不信 `pragma_update`，现读 `PRAGMA journal_mode` 断等于 wal」的硬门只活在 `db::open`/`after_open`（`db.rs:44-62`）里，而 `restore` 是往**已打开**的连接里灌页、根本不经过 `after_open` —— 于是本轮第一次出现「库内容整库换过、但 WAL 断言从没在这份内容上跑过」的状态。处置：把 `db.rs:47-62` 那段读回断言拆成 `pub fn assert_wal(conn) -> AppResult<()>`（`after_open` 改调它，行为零改动），`backup_import` 在 `restore` 之后、返回之前**现读一次**；读回不是 wal 就报 `backup_wal_lost`（此时库已被覆写，所以这条的 hint 必须直接给出那份 `pre-restore-*.zip` 的完整路径）。**为什么不能省**：网络盘/被杀软拦的路径正是这条门存在的原因（见 skill `rusqlite-durable-db-open-check`），而备份回来的库如果掉回 delete 模式，应用照常跑得挺好，直到下次断电才现形 —— 那是最坏的一种「当时看不出来」。
- **async 边界不破锁语义**：三条命令体内不得有任何 `await`；`Connection` 非 `Sync` 且 `MutexGuard` 非 `Send`，`backup.rs` 全程同步是「future 仍 `Send`」的前提。这条要能被评审静态核（`backup.rs` 里不出现 `.await`、`tokio`、`spawn_blocking`）。
- **降级/异常不许刷屏**：备份是显式动作，任何失败都不写库状态行、不进 `last_error`（那是索引轮的字段）。

## 八、测试策略

- **恒绿档（不碰外部条件）**：manifest 序列化↔反序列化对称、camelCase 键集合断言（新线格式自钉）、hex 编码（含空字节/大小写）、`counts` 查询在内存库上逐表对数、`files[].name` 里出现 `../` 或绝对路径时被忽略（zip-slip）、`schema_version` 过新的拒绝。
- **文件级往返档**：内存库种数据 → `export` 到 tempdir 的 zip → `inspect` → 另开内存库 `import` ⇒ 断言台账/项目/`settings`/`vault_meta` 逐行相同、`index_docs` 与 `index_docs_fts` 为空、包内只有两条目、`backup.db` 打开即 `integrity_check` ok、**导入后 `journal_mode` 现读为 wal**（内存库这条读回在 `db.rs:63-65` 是被放行的另一支，所以这一格必须用**文件库**跑才有意义 —— 内存库上它是恒真的废断言，别拿它交差）。
- **headless 端到端档**：真文件库（`tempfile::tempdir()` 里的 `db::open`）+ 真 `run_pass` 建索引 → 导出 → 改活库（删一条项目）→ 应急包路径断言在数据目录内 → 导入 → 断被删的项目回来了、索引表空、真实夹具源文件 size+mtime 未动。`index.running` 那条闸门可本机证伪（构造一个 running=true 的 `IndexShared` 直接喂命令层表达式）。
- **`Scratch` 搬移**：以「搬移前后 `cargo test --lib legacy_office::` 的条数与结果完全相同」为验证（**两个数都现跑取，别照抄本文任何一处**）；**不**为搬移新增功能测试，但要新增一条 `new` 的前缀参数测试（`pfm-m7` 与 `pfm-m8` 各生成在预期父目录下）。
- **不该写**（本仓裁定）：为「`VACUUM` 真把包缩下来」写绝对体积阈值（依赖 SQLite 页填充细节，夹具一小就假绿）。也**不**拿活库的 `ledger.db` 文件大小作对照 —— WAL 下最近改动都在 `-wal` 侧文件里，主库文件可以小到没有比较意义。改成**双臂对照**：同一份夹具、同样 `conn.backup` 到两个临时文件，一臂剥索引 + `VACUUM`、一臂原样 ⇒ 断「剥过的 < 原样的」且**两臂之差大于一个下限页数**（下限由夹具写入的索引正文字节数推出来，不许事后放宽）。若某轮夹具分不出差，如实报「夹具规模不足」重造夹具，不许把断言改成 `<=` 蒙过去。
- **门禁**：`cargo test --lib` 基线 **194** → 每格 Expected **实跑数出来**（`grep -c '^    fn '` + `test result` 行），不许累加（坑 9）；`node --test` 8 → **10**（T8 给 `backupFileName` 加两条：Windows 非法字符与补零后的字典序==时间序，都是本机真会踩的形状；JSX 与薄封装仍不进测试网。原写「仍 8」是因为 spec 阶段没预料到「默认包名」里有一格可证伪逻辑 —— 这条修订由计划阶段带回，见计划 T8 注记。新页面不进测试网）；两条 clippy `-D warnings` 0；`npm run build` exit 0 / `error TS` 0；结构门：`generate_handler!` **45**、`TARGET_VERSION` **4**、`allow(dead_code)` 只剩 `db.rs:30`（搬移 `Scratch` 时若顺手带上豁免，T6 接线完必须删掉 —— 「谁接线谁删」）。

## 九、真机验收（沙盒 `dev.zero.pfm.m8test`，一次性 identifier，跑完删净；真实目录只读；子代理不跑 dev，控制方执行；CDP 走 `__TAURI_INTERNALS__.invoke`）

1. **导出 happy path**：沙盒库种几个项目 + 台账 + 跑一轮索引（让索引表非空，才能证「索引不进包」）→ `invoke("backup_export", {path: %TEMP%\pfm-m8-test\out.zip})` → 断 zip 存在、两条目、`manifest.json` 里 `counts` 与沙盒库现数一致、`backup.db` 大小 < 沙盒 `ledger.db`。
2. **索引确实不进包**：把包里 `backup.db` 解出只读打开 → `SELECT count(*) FROM index_docs` = **0**，而沙盒活库非 0。
3. **覆写可撤销（主承诺）**：导出后再改沙盒库（删一条台账）→ `invoke("backup_import", {path: 那个包})` → 台账条数回到导出前、`index_docs` 变 0、数据目录出现 `pre-restore-*.zip`，并且**那份应急包自己也能被 `inspect` 读通、它的 `counts` 等于覆写前那一刻活库的现数**（不必真二次导回，那一步与断言 3 前半是同一条代码路径）。同一格还要读到：页脚 `journal_mode=wal` 在导入后仍成立（`db_status` 现取）、界面回到未解锁态（`mk` 已被清，`vault` 那格显示要解锁）。
4. **包比本机新被拒**：手造 manifest `schemaVersion = 99` → `inspect` 返回 `backup_schema_too_new`，且**库未变**（前后逐字相同）。
5. **哈希不符被拒 + 指名文件**：翻包里 `backup.db` 一字节 → `inspect`/`import` 报 `backup_hash_mismatch` 且消息含 `backup.db`，库未变。
6. **闸门**：起一轮索引（或置 `running`）后立即 `import` → `backup_busy`，库未变。
7. **明文字段那句提示可见**：`/backup` 页 DOM 里那句「包内含 ip/bastion/url/note 明文」渲染出来（M7 已证 `tauri dev` 在这台机起得来，编译 37 s，别再拿 `os error 1455` 当永久限制）。
8. **红线**：真实数据目录 `%APPDATA%\dev.zero.pfm\` 三文件 size+mtime+SHA-256 逐字未动（本轮**只读**它，所有读写都在 `dev.zero.pfm.m8test`）；沙盒与 `%TEMP%` 夹具删净。
9. **需要人亲手做的两格**（照实记，不假装自动验过）：① 原生「另存为」/「打开」对话框那两下真点一次（CDP 只证命令本身通，不证对话框选路径通）；② 覆盖前那句确认提示在你机器上的实际文案与按钮过一眼。

**没能自动验证（逐条点名）**：`VACUUM` 后包的绝对体积（只断方向）；`restore` 中途真失败的库状态（要构造 SQLite 在页拷到一半断电，本机不可廉价造，只能靠 `pre-restore` 那条撤销路）；网盘/U 盘这类慢或只读的导出目标；跨机器（真的拿到另一台机导入）—— 本机同 identifier 只能证机制，不能证「换机器」那一半。

## 十、不在本轮

| 项 | 归属 | 理由 |
|---|---|---|
| 附件（`note_assets` + 截图目录）进包 | 跟着 `note_assets` 落地那一轮（§一 已点名缩量） | 表与目录在本仓从未存在，本轮「一并打包」无处可打 |
| 备份包加密/口令 | 不做 | D6；要防在存放层防 |
| 定时自动备份、包列表、增量/差异导入 | 不做 | 技术方案 line 20 定「手动整库」；单人工具 YAGNI |
| 导入后自动重建索引 | 不做 | D7，成本不该由一次点击替你决定 |
| 恢复码轮换/导出提醒 | 不做 | `vault.rs` 的恢复码只在 init/轮换时给一次、不落盘（`:9-10` 拒绝 DPAPI），本轮包里带的是 `vault_meta` 的两份包裹，语义不变 |
| 备份包格式的多版本兼容矩阵 | 不做 | `format_version=1` 只做「同版本读 + 拒更高 schema」 |

## 十一、风险

1. **备份包是一份明文内网入口清单**（D6 的代价，不粉饰）：`ip`/`bastion`/`url`/`note`/项目名/本地路径都在明文列里。缓解：manifest 带 `plaintextWarning`、`/backup` 页念同一句、技术方案 §6.2 回写这句。真要有硬保证，那是包体加密那一格，本轮明确不做。
2. **`restore` 无回滚**：四道前置后的失败面（§三）。缓解：应急包在覆写之前已成功落盘；残余风险「半新半旧」记为有名缺口，靠那份包撤销。
3. **`sha2` 提为直接依赖**：`Cargo.lock` 期望只多一行引用；若实测冒出第二个 `[[package]]` 或新的下载，回来报控制方，不许"反正只是个小 crate"。
4. **`Backup API` 把 WAL 头随页带进快照**：不先关快照连接就打包 ⇒ 得到一个引用不存在 `-wal` 的 db。缓解：§七 那条「先 drop 再读字节」判据 + 一条 `integrity_check` 断言。
5. **三条命令都收任意路径字符串**：这是 D3 换来的能力（也是 opener 插件从 M1 起就在 `path: "**"` 上做的同类取舍）。守住的两条：只写用户当场选中的那一个路径 + 数据目录里的应急包，**只新增文件、绝不删改既有文件**；`inspect` 不解压落盘到用户可见位置（只落临时目录）。
6. **VACUUM 时长**：几百 MB 库（索引没清掉前的活库）上 VACUUM 可能几十秒，期间导出命令占着它自己那条临时连接。缓解：快照里索引已删 ⇒ VACUUM 的对象只含台账，天然小；仍慢就写进 §十 的「回头加进度事件」。
7. **`command(async)` 从 1 条变 4 条**：如果 `Connection` 非 `Send` 引发编译问题，正确解是「函数体保持同步、零 `await`」（D11 的形制，现仓 `doc_preview` 已是活先例），**不许**改成 `spawn_blocking` + `Arc<Mutex<...>>` 那种把库塞进 `'static` 的形状（会把 AppState 的所有权模型掀了重来）。
8. **`restore` 之后 WAL 位还在不在，本轮是「读回来」而不是「推理出来」**：Backup API 按页拷贝、页 1 就是 db 头（`sqlite3` 把 journal mode 记在头 18/19 字节），但目标连接已处于 WAL 时 SQLite 的处置我没有读到源码级证据 ⇒ §七 那条「导入后现读 `journal_mode`」的门就是它的替代物。**如果本机读出不是 wal，那是真发现、不是测试噪声**：回来报控制方，加固路是 restore 后显式重设 `journal_mode=WAL` 并再读回（同 `db.rs` M0 那条门的口径）。

## 十二、要回写与要更正的文档

- **`docs/技术方案.md`**：§6.2 追加/更正 ——「附件一并打包」改准为「本轮附件不落盘，`note_assets` 未实现 ⇒ 包内只有 `backup.db` + `manifest.json`，附件跟着那一轮」；line 80 架构图那格补「导入前四道前置 + 应急包」；line 218 里程碑表 M8 行状态；line 234「同库不同表保证备份口径一致」按 D4 补一句「同库 ⇒ 导出时按表剥索引」。
- **`docs/开发进度.md`**：里程碑表 M8 行 + 新增「M8 验收证据」整节 + 没能自动验证清单 + 缺口归属。
- **`docs/HANDOFF.md`**：结构门基线改为**每轮现取**并写下本轮读数（42 → 45）；§4 下一格转 M9；新坑按现数追加。
- **不动**：`.superpowers/sdd/2026-10-10-m5-reconcile/`、`.../2026-10-12-m55-cleanup/`、`.../2026-10-12-m6-ocr/`、`.../2026-10-13-m7-legacy-office/`（被已提交文档按路径引用的证据）。M8 另建自己的 SDD 工作区目录。
