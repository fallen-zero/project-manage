# 交接文档 —— 项目资料管理（写给一个完全没有上下文的新对话）

**快照时间**：2026-10-12。**当前分支 `master`，HEAD 现取**：`git rev-parse --short master`（本文最后一次提交即这份交接本身，别把 SHA 写死在这里、必漂）。`feat/m6-ocr` 停在其终审合入点（`master` 已含它全部提交，`git merge-base --is-ancestor feat/m6-ocr master` = 真）。全历史零 merge commit（`git rev-list --merges --count --all` 现取）。

## 0. 先读这一段

这是一个 Tauri 桌面工具的持续开发项目，按里程碑推进。**M0–M6 都已完成并合入 `master`**，下一个里程碑是 **M7（旧格式 .doc/.ppt 走 LibreOffice headless 转换）**。

三份必读文档按重要性：

- `docs/技术方案.md` — 项目定位、架构、需求结论与里程碑表（**产品与技术的唯一依据**；与代码不一致时以它为准并同步修订）；
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
- 依赖里程碑新加的：M5.5 引 `notify 8.2.0`（文件系统监听），M6 引 `windows 0.62.2`（`Media_Ocr` 等 7 个 feature）

**执行方法**：`superpowers:subagent-driven-development`（SDD）—— 每个任务派一个全新实现者子代理 + 一个只读评审（spec 合规 + 代码质量双 verdict）+ 修复轮（≤5）；全部任务完成后做一次整分支终审；终审后走「合 master」（**合并需需求方当次批准**，常驻推送授权只覆盖功能分支）。

**每完成一个小节点就提交并推 `origin` 功能分支**（需求方 2026-10 起的常驻指示）；「遇到需要配合的一步一步告诉我改如何操作」——需要人做的事都收在 §4「需需求方做的」里。

## 2. 已经完成了什么

**M0→M6 全部 ✅ 合入 `master`**。MVP 是 M0→M4，M5/M5.5/M6 在其上加了「启动对账 + 自动追平 + notify 监听 + 图片 OCR」。

**M6（本轮收口的里程碑）= Windows 原生图片 OCR**：`png/jpg/jpeg/webp/bmp` 里的中文/英文经本机 Windows OCR 引擎进入全文索引，可被 `search_all` 命中与 `doc_preview` 打开。关键落点：

- 新纯能力模块 `src-tauri/src/ocr.rs`：`ocr_available()`（进程内 `OnceLock` 记忆化探测，Err 保守按 false）、`image_text()`（WinRT 识别 + `\\?\` 前缀剥离）、`clean_cjk_spaces()`（仅当空格两侧都 CJK 才删的纯函数；OCR 会在中文字符间塞空格，不清洗就永远搜不到）。
- `extract.rs` 加 `DocKind::Image`、`kind_of` 认 5 张扩展名、`SUPPORTED` 到 **27 项**（17 纯文本/代码 + `docx/pptx/xlsx/xls/pdf` + 5 图片）；`TEXT_EXTS` 不动（图片不是纯文本）。
- **D1 同步锁外**：OCR 挂在 `extract_text` 的**抽取阶段**、和 PDF 同侧——`run_pass` 的抽取表达式在写锁外，秒级 OCR 不压写锁。
- **D4 无包降级门**：`ocr_available=false` 时图片在 `index_scan` 扫描门与「未支持类型」同一条 `continue`——不建行、不进 `over_size`、不占配额。门**只有一个**落点。
- `index_job.rs` worker 线程体每轮 `ScanOptions::load` 之后立刻 `opts.ocr_available = crate::ocr::ocr_available();` 填真实探测值；`ScanOptions::load` 里默认 `true` 只是兜底、**两条消费路径（`lib.rs:476` `index_overview` / `:578` `doc_preview`）从不喂给 `scan_root`、set-but-never-read 惰性**。
- `IndexOverview` 只加**一个**线字段 `ocr_available`（serde camelCase 出 `ocrAvailable`），`/index` 支持清单区加一句 `{overview?.ocrAvailable ? 已启用 : 未启用（图片暂不索引）}` 三元提示。

**门禁终态**（在 `master` 的 `0aa0d30` 树上合入前刚跑过，最后一次代码提交后复跑读数）：`cargo test -j 1 --lib` **167 passed / 0 failed / exit=0**；两条 clippy `--lib -- -D warnings` 与 `--lib --all-targets -- -D warnings` **exit=0/0 warning**；`node --test "tests/**/*.test.ts"`（在**仓库根**跑） **tests 8 / pass 8 / fail 0**；`npm run build` **exit=0/`error TS`=0**。结构门：`generate_handler!` **41** 条、`TARGET_VERSION` **4**、`allow(dead_code)` 只剩 `db.rs:30`（本批 M6 已把 `ocr.rs` 里 T1/T2 留的两处豁免全收）、`ocr::ocr_available()` 生产两处调用点。

**M6 整分支终审**：Verdict `With fixes`、**0 Critical / 2 Important / 6 Minor**。五条跨模块不变量全 HELD（线格式零改、零新 IPC/迁移、D4 单一落点、`load` 默认 true 不泄漏在 `scan_root` 唯一生产调用点层面复核成立、`pdf_text`+`panic_to_err`+worker 体外三道 panic 边界未折叠）；两条具名库内部行为风险到 vendored 源码按行号坐实（`windows-future` `join()` 是纯 manual-reset event 阻塞、不需要消息泵/STA；`TryCreate…` 空指针 → `windows-core` `from_abi` 落 `Err`、不 panic）。两条 Important 与 Minor-1 由控制方按「文本/测试级不重开派发轮」当场落地（`ocr.rs` 错误 hint 去掉不存在的「设置里关闭」指引、headless e2e 补「垃圾字节 .png 落 failed」断言、T2 夹具测失败信息只回长度）。spec §五.2 / §十.3 按机制 retro-edit（`.get()` → `join()`、`join` 无超时 ⇒ 「本张返回前 cancel 不生效」）。其余 5 条 Minor 有名登记在 `docs/开发进度.md`「## M6 整分支终审」节。

**M6 真机口径转换（需求方 2026-10-12 明确批准）**：`tauri dev` GUI 起不来——**本机 16G 机的提交内存吃不下** `project_files_manage_lib` 最终 codegen 要 mmap 的 `windows` 大 rlib，四轮调优（`jobs=1` / `RUST_MIN_STACK=32M,64M` / `debuginfo=0` / 组合）都在同一处报 `os error 1455`「页面文件太小」；`cargo test --lib`（只出 rlib）不受影响 ⇒ 转 **headless 端到端测**：`index_job::tests::ocr_image_flows_end_to_end_through_index_and_search` 用能编过的 `--lib` profile 直驱真代码链（真 `image_text` + `run_pass` + FTS + `doc_preview`），0.4–0.5 s、`--nocapture` 无 SKIP 证本机 `ocr_available()` 为真、走了真 OCR。

## 3. 当前卡在哪

**代码面没有卡点**。M6 全合入 master、门禁全绿、终审收口完成。

## 4. 下一步计划

按需求方指示二选一，都不难启动：

- **A（默认方向）：开 M7（旧格式 `.doc`/`.ppt`）**——走完整流程：`superpowers:brainstorming` 收口决策（LibreOffice headless 怎么落、是否可选插件、转换产物落哪、并发与内存约束）→ spec → `superpowers:writing-plans` → `superpowers:subagent-driven-development` 执行 → 整分支终审 → 合 master（**需当次批准**）。M7 顺带值得单独修的是 M6 遗留 **Minor-4**：`ocr_available()` 用「任一识别器语言」，而 `image_text` 只用 `TryCreateFromUserProfileLanguages`——在「profile 语言无包但其它语言有包」的机器上探测 true 但每图空指针→整目录刷屏 `failed`，spec §五.2 已给「取 `AvailableRecognizerLanguages` 首个」的现成一行加固。
- **B（若你腾得出提交内存）：补 M6 三项 deferred GUI 读数**——`.superpowers/sdd/2026-10-12-m6-ocr/m6-cdp.mjs` 已备好，走 `project_create`+`project_set_root`+`index_start`+`doc_hits`+`doc_preview`（**这些是纯字符串 IPC，不需要点原生对话框**）。启动命令：`cd <repo> && WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" npm run tauri -- dev -c '{"identifier":"dev.zero.pfm.m7test"}'`（M7 时用 m7test），起来后 `SB=<Windows 绝对路径> node .superpowers/sdd/2026-10-12-m6-ocr/m6-cdp.mjs`。要验的是：WebView2 里 `/index` 那句「已启用本机 OCR」的实际渲染、~50 图首轮墙钟、以及需要另一台无包机器（本机不可构造）。

**需要需求方做的**（不阻塞代码推进，但每次提醒一次）：

1. **远端 GitHub 默认分支 symref 仍指 `feat/m4-unified-search`**（本机无 `gh`）——只能在 GitHub **Settings → Branches** 手改成 `master`。这一格从 M4 起一直挂着。
2. 若走方向 B：**腾出提交内存**——关几个大户（`vmmemWSL` 常占 ~1G、`Everything`/`ChatGPT`/`Weixin` 各数百 MB）或在 Windows 高级系统设置把「分页文件」临时调大（比如 30–50 GB），然后我一键跑。

## 5. 踩过的坑，绝对不要再踩

按「新人最容易踩」排。每条都是独立可读的（旧版编号的坑在 git 里可查）。

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

14. **计划文本自身错了走「改文本闭环」而不是加评审轮**：裁 → 修订计划本体 → `task-brief` 重生成该任务简报 → 账本与计划一起提交 → 带明确 FIX_BASE 派发 fix round 复审；**注释级零行为回归不重开派发+复审轮**（控制方当场改准，改完重跑受影响的结构门）。

## 6. 关键指针

| 主题 | 位置 |
|---|---|
| 项目定位 / 架构 / 里程碑表 | `docs/技术方案.md` |
| 里程碑实态 + 每格验收证据 + 变更日志 | `docs/开发进度.md` |
| M6 规格（决策 D1–D7、判据、真机 §八、风险、回写清单） | `docs/superpowers/specs/2026-10-12-m6-ocr-design.md` |
| M6 实施计划（6 任务 31 步） | `docs/superpowers/plans/2026-10-12-m6-ocr.md` |
| M6 SDD 账本（每格 Ruling、两条 T4 concern 的代码级裁定、终审处理） | `.superpowers/sdd/2026-10-12-m6-ocr/progress.md` |
| M6 终审评审包（11 提交、165 KB） | `.superpowers/sdd/2026-10-12-m6-ocr/review-a8d053b..740124f.diff` |
| M6 CDP 真机脚本（需求方腾出内存后可直接复用） | `.superpowers/sdd/2026-10-12-m6-ocr/m6-cdp.mjs` |
| M6 端到端真机测（headless） | `src-tauri/src/index_job.rs` 的 `ocr_image_flows_end_to_end_through_index_and_search` |
| OCR 能力层 | `src-tauri/src/ocr.rs` |
| 远端仓库 | `https://github.com/fallen-zero/project-manage.git`（默认分支 symref 待手改，见 §4） |
| 旧版累加 HANDOFF（100+ 条编号坑与账本交叉引用） | `git show 0aa0d30:docs/HANDOFF.md` |
