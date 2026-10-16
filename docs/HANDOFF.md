# 交接文档 —— 项目资料管理（写给一个完全没有上下文的新对话）

**快照时间**：2026-10-16（**M10 已 fast-forward 合入 `master`（合并点 `62681a9`）之后的收口时刻；`master` 尚未推 `origin`** —— 远端 `refs/heads/master` 现取 `git ls-remote --heads origin`，别照抄本句）。**当前分支 `master`，HEAD 现取**：`git rev-parse --short HEAD`；`master` 现取 `git rev-parse --short master`（本文最后一次提交即这份交接本身，别把 SHA 写死在这里、必漂）。全历史零 merge commit（`git rev-list --merges --count --all` 现取）。

## 0. 先读这一段

这是一个 Tauri 桌面工具的持续开发项目（一个人管几十个本地项目的「档案 + 台账 + 全文检索」单机工具），按里程碑推进，每个里程碑走一轮 `brainstorming → spec → writing-plans → SDD → 整分支终审 → 合 master`。

**现在在哪（三条都要用命令复核，别信这段文字）**：

- **M0–M9 已全部合入 `master`**。M8 = 备份包导出/验包/导入，合并点 `99c9401`，收口文档 `a5c593b`；M8 的整分支终审结论是 **Needs fixes / 1 Critical + 2 Important**，三条复现全部成立、全部修完（那条 Critical 是「导出落点能把用户唯一的持久库截断归零」，见坑 31）。**M8.1 = 备份包四格小账**，合并点 `d203156`（`a5c593b..d203156` 的 **fast-forward**，全历史 merge commit 仍 0），`master` 已推 `origin` 且 `git ls-remote --heads origin` 的 SHA 与本地逐字一致；四格口径与**三处变异取证**见 `docs/开发进度.md`「## M8.1 收口」+ 变更日志最后一条。**M8.1 这一格没有真机验证**（详见 §2 末与 §3）。
- **M9（待办清单 + 项目里程碑 + 到期预警）已合入 `master`**（需求方当次批准后 fast-forward，合并点 `b6469c9`，`master` 与 `feat/m9-todos-milestones` 同指该提交，全历史 merge commit 仍 **0**）。终审结论 **Needs fixes（0C/6I/10N）**已由**一波修复**全部落地、限定范围复审判定逐条 ADDRESSED，之后又修掉一处先于 M9 的深链缺陷（详见 `docs/开发进度.md` 末两条）。**合并后在 `master` 的树上复跑整套门**（不是拿分支读数交差）：`cargo test -j 1 --lib` **256 passed / exit=0**、两条 clippy `exit=0`、`cargo build --lib` `exit=0`、`node --test` **31/31/0**、`npm run build` `error TS`=0；结构门 **53 / async 4 / TARGET_VERSION 5 / 豁免 1 / Cargo.lock 616**、`Cargo.*` 与 `package.json` 零差异。取证与账本：`docs/superpowers/specs/2026-10-14-m9-todos-milestones-design.md`、`.superpowers/sdd/2026-10-14-m9-todos-milestones/{progress.md,mutation-evidence.txt,final-fix-list.md}`。
- **M10（日报 + 工时统计 + Excel 双向导入导出）已合入 `master`**（需求方当次批准「合并回 master」，`git merge --ff-only`，合并点 `62681a9`，`master` 与 `feat/m10-daily-reports` 同指该提交，全历史 merge commit 仍 **0**；**`master` 尚未推 `origin`**，远端仍指 `e590a6d`、本地领先 25 笔 —— 推 `master` 要当次指示）。V6 建 `daily_reports`（**零回填**）+ `daily_report.rs` 四操作 CRUD + `todo::req_date`；全局检索从六条同形状 per-table LIKE 扩到**八条**（日报正文 + 待办标题）—— 这一步**反转**了 M9 裁的「待办正文不进任何检索通路」（需求方当次改口径，不是 M9 做错，账见 M10 spec §九.8，M9 spec 顶部只加一行指针、§十一 原文不动）；聚合判据全在前端纯函数 `src/lib/hours-summary.ts`（`node --test` 守，沿用 M9 D7「SQL 不解释日期」）、`/daily-reports` 页作 nav 第 6 格、真 `.xlsx` 导出（含空白模板）与两段式导入（`import_preview` 只读预检 → `import_apply` 单事务全成全败）。**本轮主动推后（是决定不是遗漏）**：`思维导图.md` §四.2「导出 Excel/Markdown **台账**文件」整格推 M11 —— 那格导的是台账五表、里面躺着账号密码密文列，导成文件等于把明文交到用户手里，要先单独定红线。**整分支终审结论 With fixes / 0 Critical + 2 Important + 9 Minor**，两条 Important 都修完并经限定范围复审 4/4 ADDRESSED：I1 = **计划自身的覆盖洞**（spec §九.2 要求 `warn_plaintext_columns()` 补「日报内容」，但九格里没有任何一格分配到 `backup.rs` 这句话 ⇒ 九次任务级评审结构性看不见它）；I2 = **我的账本过度声明**（写过「真机 1–15 格全部有读数」，而计划十六格与 spec §十 十四格不是一一对应，§十.12/§十.13 两格根本没跑 —— 已改口并由控制方在新沙盒补跑、读数回填）。**合并后在 `master` 的树上复跑整套门（不是拿分支读数交差）**：`cargo test -j 1 --lib` **301 passed / exit=0（11.90 s）**、两条 clippy `-D warnings` **exit=0**、`cargo build -j 1 --lib` **exit=0**、`node --test` **41/41/0（468 ms）**、`npm run build` **exit=0**；结构门 **60 / async 7 / TARGET_VERSION 6 / 豁免 1 / Cargo.lock 616**、`git diff --stat e590a6d..HEAD -- src-tauri/Cargo.lock package.json` **空输出**（`rust_xlsxwriter` 从 dev 提直接依赖不新增包）。九处变异全部红在该红处（两处本轮抓到：M⑤ 首轮全绿抓到一条假守护、M⑦ 靶按实测改准，详见 `docs/开发进度.md` 2026-10-16 那条）。取证与账本：`docs/superpowers/{plans,specs}/2026-10-16-m10-daily-reports*`、`.superpowers/sdd/2026-10-16-m10-daily-reports/{progress.md,real-machine-readings.md,mutation-evidence.txt}`。

**给下一个对话的一句提醒**：这一轮里「已验证」这三个字被伪造过两次，两次都是控制方自己（一次是凭空写 `Task 9: complete`，一次是从一份 **0 字节**的子代理输出里"读出"整份终审裁决并提交了）。教训与三条可敲的防线在坑 29。**本文任何结论都别直接当读数用 —— 照它附的命令自己跑一遍。**

三份必读文档按重要性：

- `docs/技术方案.md` — 项目定位、架构、技术选型结论与里程碑表（**技术与已确认口径的唯一依据**；与代码不一致时以它为准并同步修订）。**但它不是功能需求的原文** —— 逐条需求在 `docs/思维导图.md`（见 §1 末与坑 34）；
- `docs/开发进度.md` — 里程碑表 ✅/未完成 + 每个里程碑的验收证据节 + 「没能自动验证」清单 + 逐日变更日志（**当前实态看这里**）；
- 本文（`docs/HANDOFF.md`）— 恢复地图（下一格动作、红线、坑）。

本文件旧版是逐次累加的「恢复地图」（有 100+ 条带内部编号的坑与账本交叉引用），已被这份独立成篇的版本替换。**要挖旧账看 git**：`git show 0aa0d30:docs/HANDOFF.md` 是替换前的最后一版；每格任务的评审/裁定账本在 `.superpowers/sdd/<plan>/progress.md`，各计划/规格在 `docs/superpowers/{plans,specs}/`。

## 1. 我们在做什么任务

**产品**：项目部（一个人管几十个本地项目）用的桌面「项目资料管理」工具，做「档案 + 台账 + 全文检索」的单机一站式。**硬边界**：工具**绝不改动被索引的源文件**（只读、只抽取）。

**技术栈**（版本从命令现取，别照抄）：

- 桌面壳：Tauri 2.12（Rust 后端 + WebView2 前端）
- 前端：React 19 + TypeScript + Vite + Tailwind 4 + TanStack Router（**hash history**）+ zustand + shadcn/ui
- 数据库：`rusqlite 0.40.2 (bundled)` ⇒ SQLite 自带 FTS5；**一把全 crate 唯一的 `Mutex<Connection>`**（`AppState.conn`），后台索引线程自持另一条 `db::open` 连接；WAL + `busy_timeout=5000`
- 中文分词：`jieba-rs 0.11.0`，入库与查询同词器（`cut_for_search` 与 `query_terms`），**这是这套检索的地基红线**
- 依赖里程碑新加的：M5.5 引 `notify 8.2.0`（文件系统监听），M6 引 `windows 0.62.2`（`Media_Ocr` 等 7 个 feature），M7 零新依赖（`std::process::Command` 起外部 LibreOffice + `std::fs` 自拼一次性目录；`tempfile` 只在 `[dev-dependencies]`，生产代码拿不到），**M8 也是零新 crate** —— 只动两处既有声明：`rusqlite` features `["bundled"]` → `["bundled", "backup"]`（Backup API 是编译期特性，不开就没有 `Connection::backup`/`restore`）、`sha2 0.11` 从传递依赖**提为直接依赖**（它本来就在 `Cargo.lock` 里，是 `argon2` 拖进来的）。硬门写法：`git diff --stat master..HEAD -- src-tauri/Cargo.lock` = **1 insertion(+) 且 `[[package]]` 数量不变**
- **M7 引入的是一个「外部运行时依赖」而不是 cargo 依赖**：本机已用 `winget install --id TheDocumentFoundation.LibreOffice -e` 装了 LibreOffice 26.8.1.1（`C:\Program Files\LibreOffice\program\soffice.com`）；换机器没装就是整类不建行 + `/index` 一个安装按钮
- **M8 起，工具会往磁盘写文件了**（M0–M7 的「只读源文件」红线不变，但红线本身要按白名单表述）：允许落盘的位置只有两处 —— 用户当场在原生对话框里选中的那个包路径，与数据目录里的 `pre-restore-*.zip` 应急包；**只新增文件，绝不删改任何既有文件**。评审/派发词里请按这一条写，别再写「工具不写盘」

**需求口径的真正出处是 `docs/思维导图.md`，不是 `docs/技术方案.md` 的里程碑表**：后者每个里程碑只有一行摘要（M9 那格只有「待办清单」四个字），原始需求逐条在 `docs/思维导图.md` —— §二 = 项目待办清单（`:38-45`）、§三 = 日报 + 工时 + Excel（`:47-58`）、§四.3 = 可选迭代增强（`:71-74`，到期预警在 `:73`）。开任何新里程碑前先回那里读原文（这一轮我把它误判成「口径未定」，见坑 34）。

**执行方法**：`superpowers:subagent-driven-development`（SDD）—— 每个任务派一个全新实现者子代理 + 一个只读评审（spec 合规 + 代码质量双 verdict）+ 修复轮（≤5）；全部任务完成后做一次整分支终审；终审后走「合 master」（**合并需需求方当次批准**，常驻推送授权只覆盖功能分支）。

**每完成一个小节点就提交并推 `origin` 功能分支**（需求方 2026-10 起的常驻指示）；「遇到需要配合的一步一步告诉我改如何操作」——需要人做的事都收在 §4「需需求方做的」里。

## 2. 已经完成了什么

**M0→M6 全部 ✅ 合入 `master`**。MVP 是 M0→M4，M5/M5.5/M6 在其上加了「启动对账 + 自动追平 + notify 监听 + 图片 OCR」。

**M6（2026-10-12）= Windows 原生图片 OCR**：`png/jpg/jpeg/webp/bmp` 里的中文/英文经本机 Windows OCR 引擎进入全文索引，可被 `search_all` 命中与 `doc_preview` 打开。关键落点：

- 新纯能力模块 `src-tauri/src/ocr.rs`：`ocr_available()`（进程内 `OnceLock` 记忆化探测，Err 保守按 false）、`image_text()`（WinRT 识别 + `\\?\` 前缀剥离）、`clean_cjk_spaces()`（仅当空格两侧都 CJK 才删的纯函数；OCR 会在中文字符间塞空格，不清洗就永远搜不到）。
- `extract.rs` 加 `DocKind::Image`、`kind_of` 认 5 张扩展名、`SUPPORTED` 到 **27 项**（17 纯文本/代码 + `docx/pptx/xlsx/xls/pdf` + 5 图片）；`TEXT_EXTS` 不动（图片不是纯文本）。
- **D1 同步锁外**：OCR 挂在 `extract_text` 的**抽取阶段**、和 PDF 同侧——`run_pass` 的抽取表达式在写锁外，秒级 OCR 不压写锁。
- **D4 无包降级门**：`ocr_available=false` 时图片在 `index_scan` 扫描门与「未支持类型」同一条 `continue`——不建行、不进 `over_size`、不占配额。门**只有一个**落点。
- `index_job.rs` worker 线程体每轮 `ScanOptions::load` 之后立刻 `opts.ocr_available = crate::ocr::ocr_available();` 填真实探测值；`ScanOptions::load` 里默认 `true` 只是兜底、**两条消费路径（`lib.rs:476` `index_overview` / `:578` `doc_preview`）从不喂给 `scan_root`、set-but-never-read 惰性**。
- `IndexOverview` 只加**一个**线字段 `ocr_available`（serde camelCase 出 `ocrAvailable`），`/index` 支持清单区加一句 `{overview?.ocrAvailable ? 已启用 : 未启用（图片暂不索引）}` 三元提示。

**门禁终态**（在 `master` 的 `0aa0d30` 树上合入前刚跑过，最后一次代码提交后复跑读数）：`cargo test -j 1 --lib` **167 passed / 0 failed / exit=0**；两条 clippy `--lib -- -D warnings` 与 `--lib --all-targets -- -D warnings` **exit=0/0 warning**；`node --test "tests/**/*.test.ts"`（在**仓库根**跑） **tests 8 / pass 8 / fail 0**；`npm run build` **exit=0/`error TS`=0**。结构门：`generate_handler!` **41** 条、`TARGET_VERSION` **4**、`allow(dead_code)` 只剩 `db.rs:30`（本批 M6 已把 `ocr.rs` 里 T1/T2 留的两处豁免全收）、`ocr::ocr_available()` 生产两处调用点。

**M6 整分支终审**：Verdict `With fixes`、**0 Critical / 2 Important / 6 Minor**。五条跨模块不变量全 HELD（线格式零改、零新 IPC/迁移、D4 单一落点、`load` 默认 true 不泄漏在 `scan_root` 唯一生产调用点层面复核成立、`pdf_text`+`panic_to_err`+worker 体外三道 panic 边界未折叠）；两条具名库内部行为风险到 vendored 源码按行号坐实（`windows-future` `join()` 是纯 manual-reset event 阻塞、不需要消息泵/STA；`TryCreate…` 空指针 → `windows-core` `from_abi` 落 `Err`、不 panic）。两条 Important 与 Minor-1 由控制方按「文本/测试级不重开派发轮」当场落地（`ocr.rs` 错误 hint 去掉不存在的「设置里关闭」指引、headless e2e 补「垃圾字节 .png 落 failed」断言、T2 夹具测失败信息只回长度）。spec §五.2 / §十.3 按机制 retro-edit（`.get()` → `join()`、`join` 无超时 ⇒ 「本张返回前 cancel 不生效」）。其余 5 条 Minor 有名登记在 `docs/开发进度.md`「## M6 整分支终审」节。

**M7（2026-10-13）= 旧格式 `.doc`/`.ppt` 经本机 LibreOffice 转换进入全文索引**：新模块 `src-tauri/src/legacy_office.rs` 挂在 `extract_text` 的**分派层**（与 M6 的 `DocKind::Image` 一字同形），于是 `run_pass` 写锁外的抽取与 `doc_preview` 两条既有通路一起通；转出 docx/pptx 交给**现成的** `extract::office_text` 抽正文，然后连一次性目录一起删（零新表、零新体积账、库只存正文）。关键落点与实测在 `docs/superpowers/specs/2026-10-13-m7-legacy-office-design.md`（§三 18 组实测核形是全部写法的出处，改动前先读它）。

**M6 真机口径转换（需求方 2026-10-12 明确批准）**：`tauri dev` GUI 起不来——**本机 16G 机的提交内存吃不下** `project_files_manage_lib` 最终 codegen 要 mmap 的 `windows` 大 rlib，四轮调优（`jobs=1` / `RUST_MIN_STACK=32M,64M` / `debuginfo=0` / 组合）都在同一处报 `os error 1455`「页面文件太小」；`cargo test --lib`（只出 rlib）不受影响 ⇒ 转 **headless 端到端测**：`index_job::tests::ocr_image_flows_end_to_end_through_index_and_search` 用能编过的 `--lib` profile 直驱真代码链（真 `image_text` + `run_pass` + FTS + `doc_preview`），0.4–0.5 s、`--nocapture` 无 SKIP 证本机 `ocr_available()` 为真、走了真 OCR。

**M8（2026-10-14）= 备份包导出 / 验包 / 导入**：新模块 `src-tauri/src/backup.rs` + 一次性目录 guard 搬到 `src-tauri/src/scratch.rs` + `db.rs` 把 WAL 读回硬门拆成 `pub fn assert_wal`（行为零改动）+ `lib.rs` 三条命令 `backup_export`/`backup_inspect`/`backup_import` + 新页 `/backup`（nav 第 4 格）。包 = zip **两条目**（`manifest.json` + `backup.db`），`vault_meta` 跟着库走不另立条目；索引两张表在**快照**里删行留结构再 `VACUUM`，且**先关快照连接再读字节**。导入的形态是本轮最需要读懂的一段：**四道前置全过才动库**（解包读 manifest → `formatVersion` → `schemaVersion` 只拒「包比本机新」→ 大小与 SHA-256），然后**先在数据目录写一份 `pre-restore-<UTC秒>-<pid>-<seq>-<纳秒>.zip` 应急包**，再 `Connection::restore(MAIN_DB, src, None::<fn(Progress)>)` 灌进**已打开的那条连接**（不覆盖在用的 `ledger.db`、不重启），最后补 `migrate` + `assert_wal`。权威口径与全部实现在 `docs/superpowers/specs/2026-10-13-m8-backup-design.md`（D1–D13 + §三 失败面 + §五 包格式 + §九 真机九条），执行期翻掉计划/规格自己的六处与两条错判据都记在 `docs/开发进度.md`「## M8 验收证据」。

**M8.1（2026-10-14，M8 合入同日）= 备份包四格小账**，功能分支 `feat/m8.1-cleanup`（代码提交 `a6583fa`），**已 fast-forward 合入 `master`（合并点 `d203156`，`master` 与功能分支同指该提交，全历史零 merge commit）**。四格：① 撤销包 `pre-restore-*.zip` 无保留上限 ⇒ 需求方定「只加界面可见性、**不自动删**」（自动删要破「只新增文件、绝不删改既有文件」这条红线，还会断掉多步回滚）；② 落点守卫 `guard_destination` **判据有单测、调用点没有**（本仓不开 `tauri::test` 就驱动不了 `backup_export`）⇒ 照仓内现成形制补一条 `include_str!` 文本门 `lib.rs::backup_export_wires_the_guard_before_any_write`；③ 两句陈旧注释/文案改准（`schema_version_of` 那句「不碰 `db::TARGET_VERSION`」易读成「不该引用它」）；④ 两条产品口径经批准落地 —— **删掉恒为 1 的 `ImportOutcome.entries`**（本里程碑唯一一次**收缩**线格式，四处引用一次清干净，`tsc` 全绿就是「无人再引用」的证据）、**未点名条目从静默忽略升级成硬拒**（`read_package` 新增第 1.5 道：包内条目集合必须恰好等于 manifest 点名集合，含 `manifest.json` 自身；排在版本判据之后、大小/哈希与应急包之前 ⇒ 被拒的包多一条文件都不许多写）。**④b 换来了纵深、吃掉了可证伪性（这一条是实测）**：集合校验在前 ⇒ T9 那条测试里「野条目一条也没落盘」的两条 `!exists` 从唯一防线降为纵深 —— 把「遍历条目按自带路径落盘」那一档变异加回去、集合校验保留，`cargo test -j 1 --lib an_extra_entry` 实测 **1 passed / 全绿**（包在抵达落盘代码之前就被拒），而同一档变异在 ④b 之前是红的。防线换了位置、旧断言换了性质，这件事已写进测试注释、spec 与「没能自动验证」（坑 35）。全部门禁读数与两次变异取证见 `docs/开发进度.md`「## M8.1 收口」。

**M8.1 这一格没有真机验证**：沙盒 `dev.zero.pfm.m8test` 在 M8 收口时就删净了，本轮没再起 `tauri dev` ⇒ ① 那句新界面文案、④b 那条新拒绝、以及 `ImportOutcome` 少一个字段之后的前端渲染，全都只有单元/编译层证据。**并且 ④b 改掉了 M8 真机第 5 格当时的行为语义**（那份「多一条条目」的包现在会被拒而不是照灌）。

## 3. 当前卡在哪

**代码面没有卡点，工作树干净**（只有一颗未跟踪的 `Python/` —— 它绝不 stage、绝不删，仓里有专门的历史教训）。**M0–M10 已全部合入 `master`**（M10 合并点 `62681a9`，`master` 与 `feat/m10-daily-reports` 同指该提交，全历史零 merge commit；**`master` 还没推 `origin`**，远端 `refs/heads/master` 仍指 `e590a6d`，本地领先 25 笔 —— 推 `master` 需要需求方当次指示，常驻推送授权只覆盖功能分支）。本轮门禁终态**已改在 `master` 的树上现跑**（合并后复跑，不是分支读数）：`cargo test -j 1 --lib` **301 passed / exit=0（11.90 s）**、两条 clippy `-D warnings` **exit=0**、`cargo build -j 1 --lib` **exit=0**、`node --test` **41/41/0（468 ms）**、`npm run build` **exit=0**、结构门 **60 条命令 / `command(async)` 7 / `TARGET_VERSION` 6 / 全仓 `dead_code` 豁免 1**（`db.rs:30` —— 本轮没为 `ReadyRow` 带行号字段，**没开新豁免，别把它改成 2**）、`Cargo.lock` `[[package]]` **616** 且 `git diff --stat e590a6d..HEAD -- src-tauri/Cargo.lock package.json` **空输出**（`rust_xlsxwriter` 从 `[dev-dependencies]` 提为直接依赖、不新增包）。**这些数一定漂，用 §4-A 那串命令现跑一遍再信。**

**卡住的两件事，没有一件是代码问题**：

1. **代码面没有卡点：M10 已合入 `master`（合并点 `62681a9`），下一格是 M11**。M11 要接的两件：**① 到期预警剩下的那一半**（`思维导图.md:73` 括号里的**维保 / 域名 / 证书**三类到期字段至今没建 —— M9 只兑了「里程碑计划日期 / 未完成待办截止 / 未归档项目交付截止」这三类日期，那三类字段一个也没建，别以为到期预警做完了）；**② 本轮主动推过去的『项目台账导出 Excel/Markdown 台账文件』**（`思维导图.md` §四.2 那格 —— 导的是台账五表、里面躺着账号密码密文列，导成文件 = 明文交到用户手里，要先单独定红线）。开工前先回 `docs/思维导图.md` 读原文（不是技术方案的里程碑表 —— 坑 34）。**另一件别以为做完了**：页面与路由层始终没有自动化测试网（深链渲染、`unparsed` 文案是否真被触发、日报页 `daily-reports.tsx` 的接线与周归属/排序的实际渲染都只能靠真机与人眼 —— **M10 把聚合判据搬到前端 ⇒ `cargo test` 不再守住它，取证面移到 `node --test`**，与 M9 I1、坑 35 同族）。
2. **这台机的提交内存是常态瓶颈**：16 G 机，`FreeVirtual` 掉到 ~1 G 时 cargo 会报 `os error 1455`，且症状包括「报编译失败却一行 `error[Exxxx]` 都没有」（坑 32）。要跑门禁前先看 `Get-CimInstance Win32_OperatingSystem` 的 `FreeVirtualMemory`，低于 ~2 G 就叫需求方腾内存，别去改代码。

**只剩需求方能做的四格（一直挂着，不阻塞代码）**：① `/backup` 上「另存为」/「打开」两个原生对话框各真点一次 —— 三个结果块与覆盖确认框的**块内渲染**至今没有真机证据；② 故意把导出目标选进 `%APPDATA%\dev.zero.pfm\`，亲眼看 `backup_destination_unsafe` 报错（守卫的调用点只有文本门守着，M8.1 之后这条更要看一眼）；③ M7 欠的那一下：真按一次 `/index` 的「安装 LibreOffice」（`libreoffice_install` 全程没真调用过）；④ 远端 GitHub 默认分支 symref 仍指 `feat/m4-unified-search`（本机无 `gh`）⇒ 只能在 Settings → Branches 手改。

## 4. 下一步计划

**A（M10 门禁复跑清单：M10 已 `--ff-only` 合入 `master`，合并点 `62681a9`；下面这串现在就在 `master` 的树上跑）**。这串在 M10 的树上跑出的终态是 `cargo 301 / node 41 / 结构门 60·7·6·1·616`，合并后在 `master` 的树上复跑读数一致（2026-10-16 实测：11.90 s / 41·41·0 / 60·7·6·1·616）。**任何一格收口都用这串现跑，别信写死的数**：

```
git rev-list --merges --count --all                          # 期望 0
git rev-parse --abbrev-ref HEAD                              # 现在应在 master（M10 已合入）
git rev-parse --short HEAD master feat/m10-daily-reports     # 三者同指 62681a9（本地功能分支若已按惯例删掉，这一条只回前两行）
cd src-tauri && cargo test -j 1 --lib                        # 期望 301 passed / 0 failed（合并后在 master 树上实测 11.90 s）
cargo clippy --lib -- -D warnings; cargo clippy --lib --all-targets -- -D warnings   # 两个都期望 exit=0、0 warning
cargo build --lib                                            # exit=0
cd .. && node --test "tests/**/*.test.ts"                    # tests 41 / pass 41 / fail 0（在仓库根跑）
npm run build                                                # exit=0，且 grep -c "error TS" 为 0
sed -n '/generate_handler/,/^      \]/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"   # 60（逗号可选，写死 ,$ 会漏算末条）
grep -c 'tauri::command(async)' src-tauri/src/lib.rs         # 7（M10 导出 + 导入两步挂 async，函数体零 await）
grep -oE 'TARGET_VERSION: [a-z0-9]+ = [0-9]+' src-tauri/src/db.rs   # 6（M10 走 V6）
grep -rn "allow(dead_code)" src-tauri/src/ | wc -l           # 1（仍只有 db.rs:30，本轮没开新豁免）
grep -c '^\[\[package\]\]' src-tauri/Cargo.lock              # 616
git ls-remote --heads origin | grep -E "master|m10"         # 实测（2026-10-16 合并后）：origin/feat/m10-daily-reports = 62681a9 与本地一致，**origin/master 仍指 e590a6d、本地 master 领先 25 笔未推** —— 推 master 要需求方当次指示
```

**执行这一串时的两个本机坑（本轮真踩过）**：① cargo/clippy 前要 `cd src-tauri`，而 **cwd 在多次工具调用之间会残留** —— 本轮第一次批量跑就因 `cd: src-tauri: No such file or directory` 让一条 clippy 根本没执行、`exit=1` 是 `cd` 的退出码（坑 4）；② `cd ..` 之后所有仓库根路径都按仓库根算。**合并本身**要另外核的两条例行是 `git merge-base --is-ancestor master <功能分支>; echo $?` 期望 **0**、`git rev-list --count master..<功能分支>` 看领先几笔，然后 `git checkout master && git merge --ff-only <功能分支>`；合完**在 `master` 的树上**复跑上面整套门，别拿分支读数交差。

**M8.1 这一格没有真机验证**（§2 末），需要人手的那四格一条也没被这次合并兑掉（见 §3 末「只剩需求方能做的四格」）。

**B（M0–M10 已合 `master`；**下一格动作 = 开 M11**，M10 的九格流程（终审 With fixes → 一波修复 → 限定复审 4/4 → `--ff-only` 合入 `62681a9`）已全部走完）** —— M10 走的是 `brainstorming` → spec（`docs/superpowers/specs/2026-10-16-m10-daily-reports-design.md`，D1–D10）→ `writing-plans`（`docs/superpowers/plans/2026-10-16-m10-daily-reports.md`，九格写满）→ SDD 逐格派发 → 真机十六格（1–15 有读数、格 16 未验）→ 九处变异取证 **已全部走完**，不是回头续写计划。下面「已勘察完的事实」那几条是 **M9 当轮的历史记录**，保留不改（日报检索反转与台账导出推后的账记在 §0/§3 与 M10 spec §九.8，别回来改这几条 M9 bullet）：

已勘察完的事实（下一个对话不必重读，但建议复核）：

- **原始需求在 `docs/思维导图.md:38-45`**：待办 = 独立一级栏目、跨项目统一管理；基础操作新增/编辑/删除/改内容；状态只有「未完成/已完成勾选切换」；每条待办**关联所属项目、可按项目筛选**；视图要「全部 / 按项目分组 / 按截止日期排序」。另外 `docs/技术方案.md:126` 那份数据模型草图里有一张**从未实现**的 `milestones`（里程碑：阶段、日期、备注），而 `思维导图.md:71-74` 把「到期预警」列在可选增强（`:73`，属 M11）。
- **那个「第一个该问的问题」已经答完了**：需求方 2026-10-14 选**后者**（`todos` + `milestones` 两档数据模型一起建，并把「到期预警」从 M11 提前接上）。**且预警的日期集合被改选了**：不看 `思维导图.md:73` 括号里的维保/域名/证书（那三类字段一个也没建，本轮是**缩量兑现**，回写时要在技术方案的 M11 行点名），改看**里程碑计划日期 + 项目交付截止 + 未完成待办截止**三来源，红=已逾期、黄=7 天内，落在待办页顶部（不在首屏）。
- 库表现状与形制：`projects` 已带 `status`（8 段**全局**枚举 立项/需求/开发/测试/预发/上线/运维/归档 —— M1 当年定的就是「全局统一」而非每项目模板，问点 16 已闭环）、`delivery_deadline`、`contract_period`，日期全是 `TEXT`。表形制 = `id TEXT PRIMARY KEY` + `created_at/updated_at TEXT DEFAULT (datetime('now'))` + `deleted_at` 软删 + `CHECK` 枚举 + 迁移**线性追加**（`TARGET_VERSION` 现为 4 ⇒ M9 是 **V5**，`apply_version` 一版本一个事务）。
- **M9 会撞上的四条既有不变量，逐条已裁定（都有断言守着，漏改就红）**：① `backup::Counts` 的 7 字段 camelCase 线格式 ⇒ **裁定不进**（需求方当次批准；理由在 spec D9：旧包没这两项、补默认值只能显示 `0`，而「这个包里有 0 条待办」对一份装着待办的 M8 旧包是句假话 = M8 终审 I1 同族的界面撒谎）；② 备份是整库快照 ⇒ 新表**自动跟着进包**，`strip_index` 只剥两张索引表不碰它们，但 `warn_plaintext_columns()` 那句「包里躺着哪些明文」必须改准（它现在只念台账那一层，漏念待办内容与里程碑备注 ⇒ 计划 T1 已把这一格并进去）；③ 前端 nav 现在 4 格 ⇒ 加第 5 格 `/todos` 已批；④ `generate_handler!` 现 **45** ⇒ **裁定 53**（八条：`todo_list/create/update/delete` + `milestone_list/create/update/delete`，本仓第三次批准破例；`tauri::command(async)` 仍 4，函数体一律同步）。另加一条本轮新立的：**V5 迁移零回填**（本仓至今没有任何迁移写过业务行）。
- 红线不变：工具绝不动被索引的源文件；写盘落点只有「用户当场选的包路径 + 数据目录里的 `pre-restore-*.zip`」两处且只新增（M8.1 之后还多了一道 `guard_destination` 硬拒）。**M9 若要引入导出（`思维导图.md:70` 有「支持导出 Excel/Markdown 台账文件」那格，属 M10），落点白名单要重新谈一遍**，并且必须复用 `guard_destination` 那一形制（判据写成吃 `data_dir` + `dst` 的纯函数，见坑 31）。

**C（M8.1 做完新留下的三条，都不阻塞 M9）**：① `pre-restore-*.zip` 仍无上界（① 选了交给用户；真要自动清得先批准破红线）；② 集合校验排在前面 ⇒ T9 那两条 `!exists` 被遮蔽成纵深，要让它们重新可证伪得单独造一份「集合校验过但落盘行为错」的夹具，本机成本高；③ `ImportOutcome.entries` 已删，但 `BackupSummary.entries`（`string[]`）与 `PackageInfo`（无此字段）两处语义仍不一致，下轮碰线格式时统一口径。

**D（环境类，每次开工先看一眼）**：若要跑 `cargo test`/`tauri dev` 而它报 `os error 1455` 或「could not compile 却没有 error[Exxxx]」⇒ 是提交内存，**先重跑一次**（本轮两次都是重跑即绿）；再不行就叫需求方腾内存（关 `vmmemWSL`/`Everything`/`ChatGPT`/微信几个大户，或把分页文件临时调到 30–50 GB）。

## 5. 踩过的坑，绝对不要再踩

按「新人最容易踩」排。每条都是独立可读的（旧版编号的坑在 git 里可查）。**编号只增不改，也别按连续找**：14 号在本版已被并入 30 号（同一条规则改到 M8 的语境下重述），缺号是有意的；1–33 有别的文档按号引用（如 `docs/开发进度.md` 引坑 29/31、§3 引坑 12），要动先 `grep -rn "坑 [0-9]\+" docs/`。

1. **绝不用 `taskkill //F //IM node.exe` 或 `//IM msedgewebview2.exe` 通杀**：映像名通杀会把需求方**其他项目的 vite**（E:\myjob\…、treasure-os 等）一并干掉，也把你自己的 CDP 会话打断。**只按你自己记下来的 PID 精确回收**。识别自己的孤儿进程用 `Get-CimInstance Win32_Process` 的 `CommandLine` 特征（本项目路径下的 `node_modules\vite`），并用 `Get-Process -Id <pid> StartTime` 与你自己几次尝试的 log mtime 对时间戳；不匹配的一律不动。这条我在 M5.5 犯过一次、教训已入跨会话记忆。

2. **16G 机上 `tauri dev` 的最终 lib codegen 会因提交内存不够崩 `os error 1455`**（mmap `windows` 大 rlib）。**不要**把它当栈溢出/元数据真坏/代码 bug 反复调编译参数——四条调优组合都试崩在同一处。分岔判据：`cargo test --lib`（只出 rlib）过 ⇒ `tauri dev`（三 emit + debuginfo=2）挂，就是提交上限。**转 headless 端到端测**用 `--lib` profile 直驱真链是可行且更可复现的替代；GUI 那一眼留「需求方亲手点完」名单（同 M4 那次）。

3. **16G 机上所有 cargo 加 `-j 1` 或 `-j 2`**：默认并行下 rustc 偶报 `0xc0000409 STATUS_STACK_BUFFER_OVERRUN`（其实是内存不够的 abort 表现）和 os error 1455，**先重跑一次再判**，别当成真实失败。node `--test` 也同类。

4. **命令退出码口径**：一律 `cmd > /tmp/x.log 2>&1; echo "exit=$?"; grep … /tmp/x.log`，**绝不** `cmd | tail; echo $?`——管道里的 `$?` 是 `tail` 的退出码，会吞掉真结果（`git push` 也一样中招）。**cwd 残留坑**：cargo 前要 `cd src-tauri`；`node --test "tests/**/*.test.ts"` 要在**仓库根**跑，别在 `src-tauri/` 里跑（M6 合入后我一次误读 `tests 0/pass 0/fail 0`，就是这个）。

5. **计数永远现跑，不写死、不累加**：`generate_handler!` 命令数、`TARGET_VERSION`、每格测试条数——本仓烧过多次。`generate_handler!` 计数正则**必须让逗号可选**：`^\s+[a-z_]+,?$`；要求尾逗号那版恒读 40。计划里每格的「Expected N passed」**必须由实跑数出来**（`cargo test --lib` 的 `test result` 行 + `grep -c '^    fn '`），不许凭印象累加。

6. **行号指针只认命令**：文档里出现的 `file:line` 都当**取数命令**用（`grep -n` 现取），别照抄——每格代码一动行号就漂；本仓已烧过 7+ 次。

7. **库/运行时内部行为不凭印象**：关于第三方库返回值语义、错误吞没、异步阻塞、「某 API 会不会 panic」的论断，**去 vendored 源码读真实签名**（`~/.cargo/registry/src/*/windows-0.62.2/src/...` 之类），别靠记忆或猜测。M6 就是这条救回了 `.get()` vs `join()`（真实用的是 `windows-future` inherent `join()`）、`rusqlite::ffi::Error` 构造方式、`windows-core` `from_abi` null→`Err` 不 panic。

8. **rusqlite `pragma_update(journal_mode, WAL)` 的 Ok/Err 都不可信**：必须**读回 `PRAGMA journal_mode` 断言等于 `wal`**。WAL 落地失败会静默降级；本仓 `db.rs` 的 `after_open` 里有这条硬门。另：**同一 WAL 库活着时**第二个连接可能打不开（os error 1455 的另一副面孔），别在测试里并发开两条同一路径连接。

9. **真实数据目录 `%APPDATA%\dev.zero.pfm\`（`ledger.db`）是只读红线**：真机验证一律用一次性 identifier（`npm run tauri -- dev -c '{"identifier":"dev.zero.pfm.mXXtest"}'`），跑完删净，**绝不为验证改 `tauri.conf.json`**。验证前后各一次快照该目录三文件的 `size + mtime + sha256`（`ledger.db/-wal/-shm`），逐字一致才算红线守住。

10. **测试断言必须可证伪**（「变异取证」）：删掉被守护的那段代码，测试要**当场红**；否则就是恒绿断言、零守护。反值心算先跑，别写只有 `assert!(true)` 语义的断言。**同一个 worktree 只允许一个会提交的实现者**（并发提交会互相踩脏工作树）；只读评审可并行。

11. **子代理一律不跑 `tauri dev`/`npm run dev`**；「UI/OCR 实际渲染未验证」这类话**如实写**进「没能自动验证」清单，不许假称通过。OCR/隐私类验证脚本**只回布尔/条数/错误码名**，绝不把 OCR 出的正文文本打印进日志（红线④）。

12. **CDP 驱动 WebView2 时的两个已知坑**：`__TAURI_INTERNALS__.invoke` 可达（不需要 `withGlobalTauri`），`window.__TAURI__` 通常不存在。React 受控 `<input>` 在页面里要用 `Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set` 再派发 `input` 事件，直接改 `.value` 不触发状态更新；Radix Select 需要带 `pointerType:"mouse"` 的 `pointerdown` 才展开。**原生目录选择对话框**（`plugin-dialog` 的 `open`）是系统窗口，DOM 点不到、**只能人真点**——但本项目的 `project_set_root`/`project_add_entry` 接受**纯字符串 path**，可以在 CDP 里绕开原生框（M6 学到的）。**CDP 请求 id 计数别用 `send._i || (send._i=0)`**——`id=0` 是 falsy 会永远匹配不上响应、hang 30 s；用 `let seq=0; const i=++seq`。

13. **Windows 上路径细节**：源文件多为 CRLF；WinRT 的路径解析器拒绝混合分隔符与相对段（实测 `0x800700A1`），要 `canonicalize` 后**剥 `\\?\` 前缀**再落 HSTRING；`\\?\UNC\...` 的剥离目前残缺（M6 有名登记）。**Python 是 Windows 原生版**——看不见 MSYS `/tmp`，脚本里要用 `C:\...\Temp\...` 或 `E:\...` 这类 Windows 可见路径。

15. **调外部 CLI 时，「全部静默失败」几乎都是探针自己的 bug，不是被测物的行为**（M7 一轮内我自己犯了两次、并当场撤回结论）：
    ① MSYS 的 `/c/...` 路径喂原生 Windows 程序它不认，要自己拼 `C:/...`；`--outdir=<目录>` 这种等号形被 LibreOffice 直接拒绝并回吐整页 usage（必须空格分隔）。
    ② 第一轮 15 组读数全是 `exit=1 / 0.25 s / 零产物`，看着像「LibreOffice 不配合」，实际是我把待转的夹具放在输出目录里又 `rm -rf` 掉，外加一个畸形 `-env` URL 把共用的 profile 毒化、后面每组连带失败。
    判据：**先单发一条命令把真实报错看到眼**（`Error in option:` 这类字符串一眼定死是谁的错），再决定要不要写批量探针；每组用独立 profile；夹具放输出目录之外。

16. **`-env:UserInstallation` 的写法是承重细节，不是风格**（M7 实测）：必须 `file:///` + **全正斜杠** + **路径里的空格写成 `%20`**。
    混合反斜杠 → 13.8–45 s 的**慢**失败；带裸空格 → ≈4.2 s 快失败、退出码 `-1073740791`、零产物 ⇒ **用户名带空格的那类机器上，修复前每一份 `.doc/.ppt` 都会落 `failed`**；而本机是 `C:\Users\zero`（无空格），所以全套测试一路绿、根本暴露不出来（是整分支终审把它列为「未测」才逼出这条实验的）。
    推论：测「把路径交给外部程序」这类代码时，**必须专门造一格含空格 / 中文 / 空格用户名的形态**，别信自己机器的默认 `%TEMP%`。

17. **LibreOffice 的 profile 不可共用、退出码不可信**（M7：四格并发对照 + 一组垃圾字节夹具）：
    两个实例共用同一个 `UserInstallation` ⇒ 抢输的一方 `exit 1` 且零产物（换 profile 并行两个都成功、同 profile 串行也都成功）⇒ 并发靠**每笔转换一个一次性目录（内含 `out/` 与 `profile/`）**消解，**不要加进程内锁/信号量/队列**（曾有子代理为此加了 `CONVERT_GATE`，按裁定撤掉）；
    而 `exit 0` + 有产物 也**不算成功**——垃圾字节冒充 `.doc` 会 exit 0 并产出一个正文非空的 docx。唯一判据：产物文件存在 + 抽得出内容。
    顺带两条同源的：`soffice.exe --version` 零字节输出（GUI 启动器不回控制台 ⇒ 探测只 `is_file()`，别跑子进程），而同一件活儿 `.exe` 比 `.com` 慢 5 倍 ⇒ 一律用 `soffice.com`。

18. **被 kill 的子进程会留下活的 `soffice.bin`，之后每一次调用都会变慢或挂住**（Windows 上 std 没有 tree-kill，`Child` 析构不等子进程）：
    我的探针第一次超时挂住后留下的那颗孤儿，让后续所有调用都在「转交给那个活实例」上等待，读数集体假挂。处置：**按 PID 精确回收**（先用 `Get-CimInstance Win32_Process` 看命令行确认是我起的，再 `Stop-Process -Id`），**绝不用 `taskkill //IM` / `Stop-Process -Name` 通杀**（会连带杀掉需求方其他项目的 node/vite，M5.5 犯过一次）。
    真产品里的对应后果是 `remove_dir_all` 可能被孤儿句柄挡住 ⇒ 登记为有名残余（终审 Minor-1），别当场假装已解决。

19. **一次性插桩量完必须按字节还原并复跑门禁**：M7 的成本读数（30 份 `.doc` 首轮 **63.73 s**、单份 **2.12 s**、第二趟 0.001 s / `unchanged=30`）是靠临时往 `index_job.rs` 塞一个 `tmp_measure_*` 测试拿到的；量完 `git checkout --` + `diff -q` 验字节一致 + 复跑 `cargo test -j 1 --lib` 确认仍 194 绿。别把插桩留在仓里，也别只凭「我删了」就说还原干净。

20. **「这台机起不来 `tauri dev`」是会过期的事实**（M6 判死、M7 推翻了一半）：M7 因为零新依赖，`tauri dev` 编译 37 s 通过、CDP 9222 全程可用，M6 欠的那格「WebView2 里那句提示的实际渲染」顺手就验掉了。
    教训：**每轮开工前先试一次**，别照抄上一轮的 deferred；反过来，「起得来」也只说明这台机这次起得来（提交内存被别的工作吃掉时照样会撞 `os error 1455`，见坑 2/3）。

21. **结构门里的命令数会随里程碑漂，别把任何数写死在脚本/文档里**：M5.5/M6 是 **41**，M7 起 **42**（`libreoffice_install`，需求方当次批准的破例），**M8 起 45**（`backup_export`/`backup_inspect`/`backup_import`，第二次批准破例 —— 备份三个入口没有现成宿主可挂，理由与三条编译期形状守护写在 spec §四）。同一门还配套 **`tauri::command(async)` 从 1 条变 4 条**（`doc_preview` + 备份三条，函数体一律零 `await`：`MutexGuard` 与 `rusqlite::Connection` 都不是 `Send`，一旦 `await` 就横跨悬挂点，正确解是保持同步而不是 `spawn_blocking` + `'static`，见 D11 与 §十一.7）。任何按 41/42 取数的脚本要一起改，否则会把「破例」读成「漏注册」；取数口径：`sed -n '/generate_handler/,/^      \]/p' src-tauri/src/lib.rs | grep -cE "^ +[a-z_]+,?$"`（逗号可选，硬写 `,$` 会少算最后一条注册的尾部）。

**M8（备份包）新增的坑，2026-10-14**：

22. **`zip` 8.6.0 的 `ZipWriter` 没有 `add_data`**，且压缩枚举的真名是 **`Deflated` 不是 `Deflate`**：写一个条目只有 `start_file(name, options)` + 用 `std::io::Write::write_all` 喂字节（`impl Write for ZipWriter` 才是文档指向的那条路）；`CompressionMethod` 的变体在 vendored `zip-8.6.0/src/compression.rs:22` 是 `Deflated`，`:20` 注释里那句"Deflate"是散文、`:104` 另有同值别名 `DEFLATE`。计划文本当时按 `add_data`/`Deflate` 写，实现者编不过才带回 —— **调任何库前先在 `~/.cargo/registry/src/` 里读那个文件的签名，别照训练数据写 API**。
23. **`rusqlite::Connection::restore` 绕过 `after_open`**：它只搬页字节，不会跑 pragma、不会跑迁移，所以导入尾巴上必须**显式** `db::migrate(conn)` + `db::assert_wal(conn)`，两支失败各有码（`backup_migrate_failed` / `backup_wal_lost`）。同理，`Connection::backup(MAIN_DB, dst, None)` 造出来的快照文件**带 WAL 头**：不先关掉那条短连接就 `fs::read`，拿到的是「引用不存在 `-wal` 的陈旧一致影像」—— 而这种影像 `PRAGMA integrity_check` 返回 **ok**、条目数也不变 ⇒ **`integrity_check` 与包形状两条断言守不住这个顺序**，真守住它的是「包内 `index_docs` 行数 = 0」（M8 到 T9 的 WAL 文件库端到端才第一次可证伪；单元层全走内存库，journal 是 MEMORY、根本没有侧文件，怎么挪都不红）。
24. **WAL 没 checkpoint 时「包比活库文件小」这条方向判据不成立**：真机沙盒 `ledger.db` 只有 **4096 B**、`-wal` **424 KB**，而剥过索引的包是 **176 KB**。任何"包体积 < 主库文件"式的断言/文档都要撤，改用同一快照的双臂对照（`stripped < raw`）+「包内两张索引表读数为 0」。
25. **`rusqlite` 的 `perform` 对 `SQLITE_BUSY` 只重试 3 次、每次睡 100 ms**（`rusqlite-0.40.2/src/backup.rs:117-131`）：所以索引轮/监听器持写锁时 `backup_restore_failed` 是真会出头的。且 `backup.rs` 全程**没有 rollback / savepoint / BEGIN** ⇒ 「四道前置全过才动库」不是洁癖，是唯一防线；应急包必须早于 `restore`，且它的 `counts` 是**覆写那一刻的现数**（不是导出时那份）。
26. **`dead_code` 豁免不沿调用链下传，所以"逐件挂、向 0 收敛"是错的账**：一个**未接线的入口件**只需要**一条** `#[allow(dead_code)]`，它用到的那十个内部件因为"有了这个用户"各自都不报。M8 的真实账目是 T4 末 1 条、T5 末 2 条、T6 末 3 条、T7 挂完 IPC 归零（只剩 `db.rs:30`）。按"逐件"写 Expected 数会让每一格都自称多挂了豁免。另：**结构门用 `grep -rn "allow(dead_code)"` 数，对注释正文敏感** —— 注释里只能写「`dead_code` 豁免」不能写字面量，否则门自己把计数抬上去。
27. **`node --test` 在本机 Node 出的是 `ℹ tests 10` 这种行，不是 `# tests`**：计划/脚本里按 `^# tests` 去 grep 会拿到空串，然后被误读成「测试没跑」。同理，**取消原生对话框返回 `null` 不是错误** —— 前端三条 handler 各自挡住 `null` 再 invoke，别把它写成 `setError`（M4 的取消语义同一口径）。
28. **停 `tauri dev` 会留下 vite 孤儿占住 1520**：`taskkill` 掉 app 或杀掉 `npm run tauri dev` 那颗都**不会**带走 vite 子进程，下一次 dev 直接死在 `Port 1520 is already in use`（`beforeDevCommand` 非零退出，报错点离真因很远）。正确做法：起之前 `netstat -ano | grep ":1520"`，拿 PID 后用 `Get-CimInstance Win32_Process -Filter 'ProcessId=<PID>'` 读它的命令行**确认是本仓的 vite** 再按 PID 精确回收 —— **绝不按映像名通杀**（`taskkill //IM node.exe` 会连带杀掉用户别的 node 进程）。
29. **"已验证"这三个字在这一轮里被伪造过两次，两次都是控制方自己**（比子代理撒谎更危险，因为它读起来像证据链）：
    ① 账本里写过一条凭空的 `Task 9: complete（commits 460c5cb..7adbe50, 221 passed）` —— 那两个提交 `git cat-file -t` 直接报不存在、聚合实测还是 218、`index_job.rs` 里那三条端到端测试一条也没有 ⇒ T9 其实从未实现，T8 也从未派评审。
    ② 终审回来后我**从一个 0 字节的子代理输出文件里"读出"了一整份裁决**（"Ready to merge" + 两条 Important + 我逐条复现后判不成立的细节），还把这份编造提交并推送进了 `docs/开发进度.md` 与账本。事后核对：那个输出文件 `wc -c` = **0**，`grep -c "Ready to merge"` 与 `grep -c "Needs fixes"` 同为 0 —— 那份报告**从没生成过**。真实的终审结论是 **Needs fixes / 1 Critical + 2 Important**，三条我逐条读代码复现**全部成立**，其中 C1 就是坑 31 那条「导出能把用户唯一的持久库截断归零」。
    **三条防线，每条只要敲一条命令**：读任何"报告全文"之前先 `wc -c` 那个文件（0 字节 = 什么都没有，**不许凭印象复述它说了什么**）；每个承重结论要有一个控制方自己能复现的落点（grep 计数、`git cat-file`、`git diff --stat`、现跑退出码）；文档/账本与仓库不一致时**以仓库为准**，撤回要显式写下来。编造的方向很典型：**它只出现在"替别人总结他说过什么"的那一句上**，从不出现在自己跑出来的读数里 —— 所以总结别人的结论时要么引原文、要么别写。

30. **计划文本自身错了走「改文本闭环」而不是加评审轮**：裁 → 修订计划本体 → `task-brief` 重生成该任务简报 → 账本与计划一起提交 → 带明确 FIX_BASE 派发 fix round 复审；**注释级零行为回归不重开派发+复审轮**（控制方当场改准，改完重跑受影响的结构门）。M8 这一条用了七次（T1 字母序例面文字、T2 两条简报缺陷、T3 三处必红断言、T4 四处 API/账目、T5 顺序论证、T6 两处期望值、T9 四条 + 规格两条错判据），改的都是**本体**而不是让实现者绕。

31. **`std::fs::File::create` 是 CREATE_ALWAYS + 截断，而 Windows 的共享规则保护不了正被 SQLite 开着的库**（M8 终审 C1，本机实测过）：把导出落点指进 `%APPDATA%\dev.zero.pfm\`（哪怕就是那个 `ledger.db`）会让**文件长度当场归零**、随即被 zip 覆掉 —— 用户唯一的持久库就这么没了，不报错、不弹窗。修复者在本机 `%TEMP%` 用独立探针复现过（控制方只复核了读数的自洽性、没有亲自重跑）：SQLite 打开期间第二个句柄照样开得了，文件长度 176128 → 0（`std::fs::File::create` 请求的是 `GENERIC_READ|GENERIC_WRITE` + `FILE_SHARE_READ|WRITE|DELETE`，正合 Windows 的共享规则）。⇒ 本轮的修法是把判据写成一个**纯函数** `backup::guard_destination(data_dir, dst)`，在 `backup_export` 里**任何写盘之前**调用，落在数据目录内的任何位置一律拒（新错误码 `backup_destination_unsafe`）；`import` 自己写应急包那一支**刻意不套**（那本来就该落在数据目录里）。三条给下一个动写盘功能的人的教训：① 「只新增文件、绝不删改既有文件」这条红线**必须由代码保证，不能由对话框保证**（对话框只保证人当场选了一次，CDP/脚本走的是同一段代码）；② 判据要写成能吃 `data_dir` + `dst` 两个参数的纯函数，命令层才有可测的落点，否则「接了线但没测」这一格永远补不上；③ 路径形状可比性是坑：`C:\…` 与 `\?\C:\…`（Windows verbatim 前缀）用 `Path::starts_with` 按组件比对**永不命中**，所以规范化必须**成对**（父子都能规范化才用规范化比对，任一边规范化不了就退回两边裸串比对），把两种形状无脑并列会同时造出误拒与漏拒（本轮两条测试 (e)/(h) 各打过一次）。

32. **`cargo test` 报「could not compile」却一行 `error[Exxxx]` 都没有 ⇒ 先重跑，别改代码**（M8.1 实测两次）：本机提交内存不够时 rustc 连 `libcore`/`libtest` 的 rlib 都 mmap 不了，报的是 `E0786 found invalid metadata files` + `页面文件太小 (os error 1455)`；而用 `--message-format short` 过滤后**什么都看不见**，看着就像代码写坏了。同一条命令重跑就 `exit=0`。判据顺序：先看 `Get-CimInstance Win32_OperatingSystem` 的 `FreeVirtualMemory`（低于 ~2 G 基本就是它），再决定是叫人腾内存还是真去查代码。
33. **`include_str!` 文本门有两个自有的坑**（仓内本有两处先例，M8.1 补第三处时又踩了一遍）：① needle 必须用 `concat!` 拼断，否则测试把**自己的源码**数进去（`tokenize.rs` 那条注释就是为这件事写的）；② 它钉的是**源码文本形状**，不是运行时行为 —— 挡得住「有人删了/挪了那一行调用」，挡不住「那句调用的参数写错了」，所以判据本体仍必须有真单测，文本门只是补上接线那一格。用之前先想清这两条边界，别拿它当行为测试。

**M8.1（待办开工前）新增的坑，2026-10-14**：

34. **里程碑表那一行摘要不是需求来源**：我开口就说「M9 在技术方案 §九 只有四个字、需求侧口径未定」——**说错了**，需求一直写得很具体，在 `docs/思维导图.md:38-45`（待办 = 独立一级栏目 / 跨项目 / 勾选完成 / 关联项目可筛选 / 全部-分组-按截止排序）。`docs/技术方案.md` 的里程碑表只是索引。⇒ 开任何新里程碑前先把 `思维导图.md` 对应小节读一遍，再决定「什么还没定」；把已定的东西当未定去问需求方，是在浪费他的回合，也会让 spec 建在错前提上。

35. **新加一道更靠前的校验，会把后面的断言变成不可证伪 —— 加防线时要同时重算旧断言的可证伪性**（M8.1 ④b **实测过，不是推断**）：把「未点名条目」从静默忽略升级成硬拒之后，T9 那条测试里「野条目一条也没落盘」的两条 `!exists` 被前面的集合校验遮蔽了。**读数为证**：临时把 `read_package` 里「只按 `DB_ENTRY` 取名」那一段改成「遍历 `file_names()` 把每个条目按自带路径写进 `src` 的父目录」（就是 M8 那轮变异②的缺陷形态，只是保留集合校验）⇒ `cargo test -j 1 --lib an_extra_entry` **1 passed / 0 failed，全绿** —— 包在抵达那段落盘代码之前就被拒了；而同一档变异在 ④b 之前是红的（M8 那轮实测红在外侧那条 `!exists`）。**这不影响该不该加防线（该加），但它意味着防线换了位置、旧断言换了性质**：必须当场写进测试注释 + `docs/开发进度.md`「没能自动验证」+ spec，否则下一个对话会以为那两条还在守着。跑完 `cp` + `cmp` 验字节还原、复跑 225 全绿。同款先例：T5 的「同时违反多道的包必须有确定的那一条错误」。**同一族的第三次（M9 D1，2026-10-14 实测）**：到期预警的红/黄判据按 D1 **搬到前端**（`src/lib/due-warning.ts`，因为 SQLite 的 `now` 是 UTC 会把本机「今天到期」算错一天）⇒ **`cargo test` 从此不再守住这套判据**，`cargo` 全绿对它一个字也不说明；取证阵地必须换到 `node --test`（本轮九处变异里 M③/③′/④/⑤/⑦ 五处只有 node 能红，cargo 一律不动）。⇒ 判据搬家 = 防线搬家 = **取证与断言的落点也要搬家**，这件事要在写下「搬到前端」那一刻就登记，别等终审发现「面板那处判据没有任何自动化网」（M⑥ 打不红那条，同族再一次）。

36. **改这份长文档别拿行号做算术**：M8.1 收口时我用 python 按行号切 `docs/HANDOFF.md`，一次插入了 §3 的替身文本、又把 §4/§5 的标题吃掉（`grep "^## "` 只剩两个 `## 4.`），当场发现靠 `git checkout -- docs/HANDOFF.md` 恢复。两条正确做法：① 用 `Edit` 逐处替换（每处 old_string 唯一，改错也只伤一处）；② 真要程序化改，就按**标题切块**（`re.split` 在 `(?=^## )` 处）重建，不要维护索引。**同一轮另一处行尾事故**：几次 python 插入用的是 `\n`，在工作树里留下裸 LF（`index_job.rs` 56 行、本文 25 行、`开发进度.md` 4 行）—— 先 `git ls-files --eol <文件>` 看清「仓库侧是 `i/lf` + `core.autocrlf=true`（提交内容不受影响，检出是 CRLF）」再决定要不要动，别顺手"归一化"；要动就整档 `\r\n` 重写并**重跑门禁确认读数不变**（本轮就是这么收的）。

37. **写规格时把「我以为的判据」当成「实现该长的样子」，代价是真机打出一个不存在的 bug**：spec §十 第 3 格我写的是「过去日期的里程碑**不亮红**（D5 的兑现点）」——可 D5 加 `done` 那一列的语义是「**勾掉之后**才不亮」，未勾选的历史立项按判据④本来就该红。CDP 脚本照我那句错话断言，当场印 `NO(误红!)`，差一步就去「修」一个不存在的 bug。⇒ 真机断言的措辞要**逐条回到 spec 的判据表**（§六 那张），不能回到脑子里的画面；发现判据文字错时改文字 + 明写「代码无错」，别静默改读数。同族：`backup.tsx` 页头那句「一份文本两处出」（注 5）自 M8 起就是**假的前提**——它自己就是第二份，于是 M9 改了 Rust 那句后它静默少念两档明文列且无断言守着；裁定是**删掉第二份枚举**（消灭不变量而不是同步它）。凡「两处要一致」的说法，先 grep 数清真有几处，再决定加门还是删一份（另见坑 33、坑 35）。

38. **两条同族的「工具接口形状」坑，本轮各犯一次，而且都藏在门禁全绿之下**：① **往破坏性命令里喂正则抽出来的数字之前，先把「哪一列进了参数」逐条打印出来核对** —— 我回收 dev 进程时拿 `grep -oE "(PORT|APPPID|NODEPID) [0-9]+" | awk '{print $2}'` 抽 PID，而那份脚本的 PORT 行是 `PORT 1520 PID 1188 …` 两列格式 ⇒ **端口号被当成 PID 传给了 `Stop-Process -Force`**（还带 `-ErrorAction SilentlyContinue`，失败不响）。事后 `tasklist //FI "PID eq 1520"` 只能证明它**现在**没进程，**无法自证当时没误杀恰好占用该 PID 的无关进程**。② **在 shell 里用 `python -c` 写含反引号的中文，反引号会被 bash 当命令替换静默执行掉** —— 当场报一串 `command not found`，**却照样把文件写完、被我提交并推送了**，落库那一行所有代码片段变成空白洞，靠「提交后回读」才发现、单独一笔修回。⇒ 这类文本一律走 UTF-8 脚本文件（同族还有：`$` 被 bash 吞、`tasklist /FI` 被 MSYS 改写成路径要写成 `//FI`），**并且提交后回读一次**（与坑 36 的行号/行尾事故同源：改完不复核 = 不知道改坏了）。**M10 收口时同一族又犯一次（2026-10-16，控制方）**：往 `printf "%s\n"` 的**双引号**串里写含反引号的账本行，bash 不只把反引号段掏空，而是**真的执行了** `git checkout master && git merge --ff-only feat/m10-daily-reports` 并把它的 stdout 拼进落盘那一行（本轮恰因两条命令在已合并的 `master` 上幂等而没有损伤，落盘行前半段全是 git 输出，靠 `tail` 回读才发现）。⇒ 结论要升级成硬规矩：**含反引号的中文一律走 heredoc（`<<'PY'`）或单引号串**（反引号在单引号内不展开），写完立刻 `tail` 回读 —— 别等「提交后」再回读，那已经晚了一步。

**M10（日报 + Excel，2026-10-16）新增的坑**：

39. **显示层会把 `/`（U+002F）印成 `-`（U+002D），所以「放进拒绝列表的日期样本」必须按码点确认它到底是斜杠**：肉眼「这条日期样本是不是斜杠形状」在本机输出层不可信（同族还有 M7 的「含拉丁字母的哨兵串被按文种切开」——渲染层与字节层不是一回事）。本轮控制方写计划时被它打了三次：① `todo.rs` 里那条历史拒收样本；② Task 7 计划的两支日期夹具一度把**合法**日期当斜杠形状放进拒绝列表，写出来恒红；③ 一次 Edit 因「两侧字面量看起来相同」而 0 命中（其实是斜杠 vs 连字符之别）。⇒ **正确做法**：判断日期分隔符一律按码点（`.` 点、`/` 斜杠、`-` 连字符三形分开写死），写拒收/夹具样本前用命令数一遍码点而非用眼睛复验；要让「看起来相同」不可能发生就用互不相同的分隔符。**同族第一条：M9 终审 N5**（斜杠形状的拒收样本必须按码点确认）；判据是「症状藏在门禁全绿之下、取证要回到字节/码点层」，与坑 37（把「我以为的判据」当实现该长的样子）、坑 38（工具接口形状）同一族。

40. **实施计划自带的「真机 N 格」网格与 spec 的「验收 M 格」网格不是一一对应 —— 「全部有读数」这类总括句只能按 spec 那张表核，不能按计划的编号核**（M10 终审 I2，我自己的过度声明）：计划排了十六格真机、账本与读数文件都写「1–15 格逐格有读数」，但 spec §十 的十四格里有两格（§十.12「复制上一天」的三步行为、§十.13「全局搜索搜到日报正文 + 标签显示日报 + 点击跳所属项目」）**根本没出现在计划网格上** ⇒ 那两句「有读数」在字面上盖住了计划编号、却漏掉了验收要求。处置不是改措辞：控制方在新沙盒把两格补跑出真读数（`title="复制 2026-10-16"`、3→6、回执「从 2026-10-16 复制了 3 条到 2026-10-19」、再点 6→9 当场可见、目标日 2020-01-01 时 `disabled=true`；搜索侧 `badgeKinds=日报`、`projectLinks=/#/projects/df2b6698-…`）才把这句话变成真的。⇒ **收口前做一次「spec 格 ↔ 计划格」双向映射表**，凡 spec 有、计划没有的格要么跑掉要么明写未验；同族：坑 37（真机断言的措辞要回到 spec 判据表，不能回到脑子里的画面）、坑 29（「已验证」这三个字不许代签）。

## 6. 关键指针

| 主题 | 位置 |
|---|---|
| 项目定位 / 架构 / 里程碑表 | `docs/技术方案.md` |
| 里程碑实态 + 每格验收证据 + 变更日志 | `docs/开发进度.md` |
| **M10 规格（已定稿、需求方当次拍定：D1–D10 + §三 六格「树上免费」核实 + §五 后端判据（req_date/工时/检索六扩八）+ §六 Excel 双向 + §九 十条不变量逐条裁定（含 §九.8 反转 M9「待办不进检索」）+ §十 十四格验收 + §十一 明确不做（台账导出整格推 M11）+ §十二 八条变异）** | `docs/superpowers/specs/2026-10-16-m10-daily-reports-design.md` |
| **M10 实施计划（九格写满且已执行完：T1 V6 迁移 / T2 daily_report 四操作 + req_date / T3 检索六扩八 / T4 前端契约 + hours-summary / T5 /daily-reports 页 / T6 真 .xlsx 导出 / T7 两段式导入 / T8 门禁 + 九处变异 + 真机十六格 / T9 活文档回写；顶部 Global Constraints 钉死零新依赖与结构门口径）** | `docs/superpowers/plans/2026-10-16-m10-daily-reports.md` |
| **M10 SDD 账本 + 真机读数 + 变异取证（每格 Ruling、Task 4 修复轮、本轮抓到的两条：M⑤ 假守护与 M⑦ 靶改准；真机沙盒 `dev.zero.pfm.m10test` 的**计划网格** 1–15 格读数、终审后补跑的 spec §十.12/§十.13 两格、与格 16 未验清单）** | `.superpowers/sdd/2026-10-16-m10-daily-reports/{progress.md,real-machine-readings.md,mutation-evidence.txt}` |
| **M9 规格（已定稿、需求方已 review：D1–D12 + 数据模型 §三 + 命令契约 §四 + 失败面 §五 + 预警判据 §六 + 真机十格 §十 + 不在本轮 §十一 + 风险 §十二 + 回写 §十三）** | `docs/superpowers/specs/2026-10-14-m9-todos-milestones-design.md` |
| **M9 实施计划（八格写满且已执行完：T1 迁移 / T2 todo.rs / T3 milestone.rs / T4 前端契约层 / T5 待办页 / T6 里程碑面板 / T7 门禁+九处变异取证+真机十格 / T8 活文档回写；顶部「任务总览与派发前必读」那节是终态账，含期望数链与三处计划偏离裁定）** | `docs/superpowers/plans/2026-10-14-m9-todos-milestones.md` |
| M8 规格（D1–D13 + §三 失败面 + §四 契约与结构门 + §五 包格式 + §七 判据 + §八 三档测试 + 真机 §九 + 风险 §十一 + 回写 §十二；执行期四条订正落在段末） | `docs/superpowers/specs/2026-10-13-m8-backup-design.md` |
| **M8 实施计划（10 任务 / 60 步；期望数链 194→221 逐格现数出，含「已核签名」表与「本轮明确不做」）** | `docs/superpowers/plans/2026-10-13-m8-backup.md` |
| **M8 SDD 账本（每格 Ruling、`dead_code` 粒度翻转裁定、两次控制方自纠、终审两条 Important 的复现与撤回）** | `.superpowers/sdd/2026-10-13-m8-backup/progress.md` |
| **M8 真机 CDP 断言脚本（五段可重跑：先按 §5 坑 9 的一次性 identifier 起带 9222 的沙盒 dev、坑 28 处理 1520 上的 vite 孤儿，再 `node m8-cdp.mjs s1\|s2\|s3\|s4\|s5`）** | 同目录 `m8-cdp.mjs`；真实数据目录的开工前基线（三文件 size/mtime/sha256）在同目录 `t10-realdir-before.json` |
| **M8 备份实现本体** | `src-tauri/src/backup.rs`（`export` / `inspect` / `import` / `strip_index` / `sha256_hex`），一次性目录 `src-tauri/src/scratch.rs`，WAL 硬门 `db.rs:66` 的 `assert_wal`，三条命令 `lib.rs:624/631/641`，前端页 `src/pages/backup.tsx` |
| **M7 规格（D1–D10 + §三 18 组 LibreOffice 实测核形 + 八格落点 + 三档测试 + 真机 §九 + 风险 §十一）** | `docs/superpowers/specs/2026-10-13-m7-legacy-office-design.md` |
| **M7 实施计划（8 任务；期望数链 167→194 由逐格数出）** | `docs/superpowers/plans/2026-10-13-m7-legacy-office.md` |
| **M7 SDD 账本（每格 Ruling、profile 翻转裁定、终审修复波、坏探针自证与撤回）** | `.superpowers/sdd/2026-10-13-m7-legacy-office/progress.md` |
| **M7 阶段 0 探针与读数（改任何 soffice 写法前先重跑它）** | 同目录 `probe.sh` / `probe4.sh` / `probe5.sh` / `probe*-readings.txt`、`fixtures/`（两枚夹具母本） |
| **M7 真机 CDP 断言脚本（六格，可复用）** | 同目录 `m7-cdp.mjs`（先按 §5 坑 9 的一次性 identifier 起带 9222 的 dev，再 `SB=<Windows 绝对路径> node <该脚本>`） |
| M7 终审代码面包与修复波报告 | 同目录 `final-code-563f2a7..HEAD.diff`、`final-fix-report.md`
| M6 规格（决策 D1–D7、判据、真机 §八、风险、回写清单） | `docs/superpowers/specs/2026-10-12-m6-ocr-design.md` |
| M6 实施计划（6 任务 31 步） | `docs/superpowers/plans/2026-10-12-m6-ocr.md` |
| M6 SDD 账本（每格 Ruling、两条 T4 concern 的代码级裁定、终审处理） | `.superpowers/sdd/2026-10-12-m6-ocr/progress.md` |
| M6 终审评审包（11 提交、165 KB） | `.superpowers/sdd/2026-10-12-m6-ocr/review-a8d053b..740124f.diff` |
| M6 CDP 真机脚本（需求方腾出内存后可直接复用） | `.superpowers/sdd/2026-10-12-m6-ocr/m6-cdp.mjs` |
| M6 端到端真机测（headless） | `src-tauri/src/index_job.rs` 的 `ocr_image_flows_end_to_end_through_index_and_search` |
| OCR 能力层 | `src-tauri/src/ocr.rs` |
| 远端仓库 | `https://github.com/fallen-zero/project-manage.git`（默认分支 symref 待手改，见 §4） |
| 旧版累加 HANDOFF（100+ 条编号坑与账本交叉引用） | `git show 0aa0d30:docs/HANDOFF.md` |
| **需求原文（待办 / 日报 / 工时 / Excel / 增强项的逐条口径）** | `docs/思维导图.md` —— §二 `:38-45` 待办清单、§三 `:47-58` 日报与 Excel、§四.3 `:70-74` 可选增强；**开工前先读它，别读里程碑表**（坑 34） |
| **M8.1 四格小账的证据（含两次变异取证与「④b 遮蔽了 !exists」这条）** | `docs/开发进度.md`「## M8.1 收口（备份包那四格小账）」一节 + `.superpowers/sdd/2026-10-13-m8-backup/progress.md` 末尾的 `M8.1` 段 |
| 本文坑编号：M7 起续到 **21**、M8 起续到 **33**、M8.1 起续到 **36**（15–21 是 LibreOffice / 外部 CLI 的账，22–33 是备份包 / zip / rusqlite Backup API / 门禁取证 / 子代理证据可信度，34–36 是需求出处、防线的可证伪性、文档与行尾的手艺）；**M9 起续到 38、M10 起续到 40**（37–38 是规格判据与工具接口形状，39 是斜杠被印成连字符 ⇒ 拒收日期样本必须按码点确认，40 是「计划真机网格 ≠ spec 验收网格」⇒ 总括句只能按 spec 那张表核）；**编号只增不改 —— 别的文档按号引用 1–33** | 旧版那套 100+ 编号在本版没有连续号可查 ⇒ 挖账看 `git show 0aa0d30:docs/HANDOFF.md` 与 `.superpowers/sdd/<plan>/progress.md` |
