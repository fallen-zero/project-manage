# M7 设计 —— 旧格式 .doc/.ppt（LibreOffice headless 转换）

依据 `docs/技术方案.md`（M7 = 旧格式，§八 line 215、§6.1 line 166「未支持类型不建行」、需求结论 line 17「检索粒度选 D 含旧格式」、库行为表 line 54「旧格式 .doc 纯 Rust 无可靠方案 → 需外部 `soffice --headless`」）。执行方式仍是 `superpowers:subagent-driven-development`。

- **行号纪律**：本文所有 `文件:行号` 只是 2026-10-13 在 `master`（`563f2a7`）的对照读数，**取数只认命令**。落点一律用符号定位：`grep -n "pub enum DocKind\|pub fn kind_of\|pub fn extract_text\|const SUPPORTED\|fn office_text" src-tauri/src/extract.rs`、`grep -n "ocr_available\|pub fn scan_root" src-tauri/src/index_scan.rs`。
- **本文的第二/三节是实测，不是设计意图**：§三 的每条读数都能用 `.superpowers/sdd/2026-10-13-m7-legacy-office/probe5.sh`（及配套 `probe.sh`/`probe4.sh`）在本机重跑复现，LibreOffice 版本 26.8.1.1（winget 装的当前稳定版）。设计里凡是关于 LibreOffice 内部行为的论断，只有出现在 §三 的才算已证；没出现的实现阶段要现场取数，别照抄。

## 一、本轮交付与边界

**交付**：让 `.doc` / `.ppt`（Word/PowerPoint 97-2003 二进制格式）里的中文/英文正文进全文索引，可被 `search_all` 正文段命中、可被 `doc_preview` 打开并高亮。一句话——**给旧格式一种「正文」，用本机外部转换器现取**。

**落点八格（以 §六 表为准），只有第一格是新的，其余全是接线与文案**：

1. 新模块 `src-tauri/src/legacy_office.rs`：`soffice_path()`（探三个候选位）、`soffice_available()`、`legacy_text(path, slides)`（起进程 → 等超时 → 校验产物 → 交回 `extract::office_text` → 删临时目录）。
2. `extract.rs`：`DocKind` 加 `LegacyWord`/`LegacySlides`、`kind_of` 认两个扩展名、`SUPPORTED` 27→29、`extract_text` 加两臂，并删掉两处「归 M7」的旧文案。
3. `index_scan.rs`：`ScanOptions` 加一个 `soffice_available`，降级门加一条同形 `continue`。
4. `index_job.rs`：worker 每轮 `ScanOptions::load` 之后填真实探测值（M6 已开这条路）。
5. `lib.rs`：`IndexOverview` 加**一个** `soffice_available`（出 `sofficeAvailable`）+ **一条新命令** `libreoffice_install`（见 D8，本批唯一破结构门处）。
6. 前端：`src/types/index.ts` 加一个键、`/index` 那行状态两态化 + 一个「安装 LibreOffice」按钮。

**边界（明确不做，写死）**：

- **只 `.doc`/`.ppt`**（D3）。WPS 系的 `.wps/.dps/.et`、`.rtf`、`.xls`（已由 calamine 支持）一概不登记；本机同时装有 WPS Office 12.1.0.28505 与 Office 2019 这一事实**不构成**本轮走 COM 自动化的理由（需求方 2026-10-13 选定 LibreOffice 路线）。
- **不缓存转换产物**（D2）：转出的 docx/pptx 只当管道，用完即删。零新表、零新目录生命周期、零清理入口、库体积不变。
- **不做安装包捆绑**、不做设置页开关、不做独立转换 worker 队列、不做批量转换阶段（§十 逐条给理由）。
- 绝不动被索引的源文件（红线 1）：只读源、`--outdir` 指向我们自己的临时目录。**§三.5 已实测**：转换后源目录文件数不变、无 `.~lock` 残留、`size+mtime` 逐字未动 ⇒ 不需要「先复制源文件再转换」这个预案。
- 单文件上限、排除目录、配额、增量、取消、熔断**全部沿用现成的**，不新增常量。

## 二、决策与理由

| 编号 | 决策 | 被否掉的备选与理由 |
|---|---|---|
| D1 | 转换**在既有索引轮的抽取阶段同步做**（`extract_text` 的新 arm 内起子进程），与 PDF/OCR 同侧、在写锁外 | ① 批量转换阶段（一轮开头把本轮所有旧格式一次喂给 soffice）：实测三份一批 1.20 s vs 三次单份 3.11 s（§三.9），确实省，但要在 `run_pass` 里插一个新阶段、把「转换阶段整体失败」与「单文件抽取失败」拆成两条错误路径、取消判定也要在阶段之间重做一次——按 D3 的量级（几十份）换几十秒不值。② 独立转换 worker 队列（dowse 形）：M6 的 D1 已经否过一次同类（常驻队列 + 新开「worker 写库 vs `running` 单飞闸门」这条不变量），本轮没有任何新理由把它请回来。选同步锁外的正面理由：`run_pass` 本就是后台线程，`match panic_to_err(\|\| extract_text(..))` 先抽、`Ok` arm 才 `write_doc` ⇒ 秒级转换只拖长这一轮，不拖长任何单笔写锁（M5 finding 1 那条 `db_failed` 不会因为本批变严重）。需求方 2026-10-13 选定。 |
| D2 | **转完即删**，不缓存 docx/pptx；`doc_preview` 打开旧格式时**现转一次** | ① app data 缓存产物：多一个目录的生命周期、多一条体积实测要量、多一个清理入口（M5.5 才刚为 `missing` 行的清理入口纠结过一次，结论是 YAGNI）。② 旧格式干脆不给预览：要在 `doc_preview` 里加一条「这类行不给预览」的新分支，反而比复用 `extract_text` 更特殊化。代价照实记：预览一份 `.doc` = 一次进程冷启动 + 一次抽取（本机稳态 ≈1.2 s 转换 + 抽取，§三.2），由既有的预览单飞闸门（`PreviewShared`）与 20 MB 尺寸门管着，不新建闸门。需求方 2026-10-13 选定。 |
| D3 | 覆盖面**只有 `.doc` 与 `.ppt`** | 否掉加 WPS 系/`.rtf`：`.wps/.dps/.et` 是另一套格式族，LibreOffice 对金山私有后缀的兼容性本轮**未取任何数**，且一旦收录就要按类型分别实测失败率。需求方 2026-10-13 选定「几十份以内 / 只 .doc/.ppt」的量级与覆盖面。 |
| D4 | **默认全开、无开关**：探到 LibreOffice 就转，探不到就整类退回不建行 | 延续 M5.5/M6 的「不做 settings 写入口、不做可关开关」。要关只能改分派那一行。被否「设置页勾选旧格式是否索引」：要新增 IPC + 复选框 + settings 写入口与校验三件，范围一下胖（M6 的 D2 同一理由）。 |
| D5 | 探测**不记忆化**：每轮 `ScanOptions::load` 之后现探一次（`OnceLock` 不用） | 与 M6 的 `ocr_available()` 有意不同，理由在成本形态：OCR 探测要构造 WinRT 对象，LibreOffice 探测只是三次 `is_file()`。换来的收益是**装完不用重启应用**——一键安装（D8）之后下一轮就探得到，`.doc/.ppt` 直接开始建行。一轮内只探一次 ⇒ 同一轮里降级判据仍不会分叉（M6 §四 那条「别在同一轮里忽有忽无」的要求照守）。 |
| D6 | 降级门**只有一个落点**：`index_scan::scan_root` 里 `Some(LegacyWord\|LegacySlides) if !opts.soffice_available => continue`，与 `None => continue` 完全同路 | 承 M6 的 D4 与整分支终审不变量③（「降级门确为那一个 `match`、先于配额与两桶、无第二处分裂分支」）。不在 `run_pass` 里再判一次：`ScanOptions::load` 的默认值 + worker 覆盖已经保证生产路径拿到活探测值，第二处落点只会造成「UI 说支持、扫描又挡一遍」的双口径。默认值这次**当场钉一条测试**（兑 M6 终审 Minor-6 那格同形账）。 |
| D7 | `IndexOverview` 加**一个** `soffice_available: bool`（camelCase 出 `sofficeAvailable`） | 与 M6 的 D6 同一判据：降级提示要上屏才回答得了「这文件为什么不在结果里」，所以线格式**只加这一个**。`SearchBundle` 七键、`DocPreview` 六键两条断言照旧不许动（旧格式正文进的是既有 FTS 正文列）。 |
| D8 | **一键安装引导**：新命令 `libreoffice_install()`（零参数）用 `CREATE_NEW_CONSOLE` 起 `winget install --id TheDocumentFoundation.LibreOffice -e --accept-package-agreements` 并**立刻返回**（fire-and-forget）；探不到 winget 时返回 `startedViaWinget=false`，由前端调 `openUrl` 打开官网下载页 | 需求方 2026-10-13 明确要求「真一键」，并当次批准 **`generate_handler!` 41→42** 这条结构门的代价（M4/M5/M5.5/M6 四个里程碑都把它当字面不变量验过，spec §四 点名理由）。被否与替代：① 只开下载页（零新 IPC，但不是「一键安装」，已作为 winget 缺失时的退路保留）；② 引 `tauri-plugin-shell` 做带 scope 的命令白名单（审计面更漂亮，代价是多一个插件依赖 + 一段 capability 配置，为一个按钮不成比例）。`openUrl` 那条退路零改动：`capabilities/default.json` 已有 `opener:default`，而 vendored `tauri-plugin-opener-2.7.0/permissions/default.toml` + `allow-default-urls.toml` 里 `opener:default` = `allow-open-url` + `allow-reveal-item-in-dir` + scope `https://*`（本轮实读，非记忆）。 |
| D9 | 失败口径：**转换超时/产物缺失/抽取报错 ⇒ `failed` + `error_msg`；抽出空正文 ⇒ `skipped(empty_text)`；没装 LibreOffice ⇒ 不建行不占配额**。`index_status` 与 `skip_reason` 两个值域**一字不扩** | 复用 `run_pass` 既有四 arm（`Ok(empty)`/`Ok(body)`/`extract_unsupported`/其它 `Err`），一条都不新增。§三.6 实测到一条反直觉事实：**垃圾字节冒充的 `.doc` 会以 exit 0 成功转出 docx**（LibreOffice 按纯文本把它读进去），所以「成功」的判据只能是「产物存在且能抽出内容」，不是退出码、不是 stderr 为空（§三.7：正常成功的调用里 stderr 也带 `Could not find platform independent libraries <prefix>` 这行噪音）。 |
| D10 | 命令行形态钉死四件：可执行文件用 **`soffice.com`**（不是 `soffice.exe`）、`--outdir` **空格分隔**（不接受 `--outdir=` 等号形）、`-env:UserInstallation` 必须是**全正斜杠的 `file:///` URL**、**每个源文件一个独立 outdir + 一个独立 profile**（两者装在同一个一次性目录里；初选「profile 每进程复用」**已被 §三.16 翻转**） | 四条全部来自 §三.2/3/4/8/10 的实测，不是风格选择：`.exe` 同一活儿 6.65 s 对 `.com` 1.27 s；`--outdir=X` 直接被拒并回吐整页 usage；混合反斜杠的 URL 失败得又慢又静默（13.8–45 s）；不带独立 profile 会往 `%APPDATA%\Roaming\LibreOffice` 落用户配置（§三.10）并与用户自己开的实例抢锁；两份同名不同目录的源进同一 outdir 会**互相覆盖**只留一份产物（§三.8）。
被否的初选「profile 每进程一个、复用」：省的是 ≈0.78 s/份（§三.3），但 §三.16 实测**两个实例共用同一个 UserInstallation 时抢输的一方直接 exit 1 且不产出文件**，而本项目里索引 worker 与预览通路会真并发、`cargo test` 默认更是多线程跑测试——省那 0.78 s 换来的是「随机假失败 + 弹窗把进程卡到超时」，不划算。 |

## 三、实测核形（阶段 0 探针，LibreOffice 26.8.1.1，本机 2026-10-13）

复现：`bash .superpowers/sdd/2026-10-13-m7-legacy-office/probe5.sh`（读数同目录 `probe*-readings.txt`）。

1. **两个启动器的分工**：`soffice.exe --version` → exit 0 **且零字节输出**（GUI 启动器不回控制台，探测不能靠跑 `--version`）；`soffice.com --version` → `LibreOffice 26.8.1.1 bf6ef266...`。
2. **墙钟**：`.com` 单份 `doc→docx` 稳态 **1.03 / 1.10 / 1.08 s**（连测三次），首次带新建 profile 1.27 s；同一活儿 `.exe` **6.65 s**（慢 5 倍）。→ D10 用 `.com`；D1 的「秒级、锁外」由此有了基数。
3. **profile 复用比每次新建便宜**：复用 1.14/1.18 s vs 每次 `rm -rf` 后重建 **1.92 s**（差 ≈0.78 s/份）。profile 落盘 359 KB。→ **当时结论：每进程一个、复用**（已被 §三.16 翻转，复用只省常数、却引入并发假失败）。
4. **`--outdir=<目录>` 是错的**：报 `Error in option: --outdir=...` 并回吐整页 usage、exit 1；`--outdir <目录>`（空格）才生效。另：MSYS 形 `/c/...` 路径原生程序不认，要 `C:/...`。
5. **源目录零残留（红线实测）**：转换后源目录文件数仍为 1、无任何 `.*` / `*~lock*` 残留、源文件 `size+mtime` 与转换前逐字相同。⇒ 不需要「复制源文件到临时目录再转」的预案。
6. **退出码不可信（承重）**：内容为 `this is definitely not a Compound File Binary payload` 的 `bad.doc` → **exit 0 且产出 `bad.docx`**，其 `word/document.xml` 里有 1 个 `<w:t>`、首段 65 字节（垃圾被当纯文本读进来了）。不存在的输入 → exit 1、无产物、stderr `Error: source file could not be loaded`（0.58 s 快失败）。⇒ D9 的「产物存在 + 抽出非空」才是成功判据。
7. **stderr 有噪音不等于失败**：正常成功的调用里也打印 `Could not find platform independent libraries <prefix>`。
8. **产物命名与覆盖**：产物名 = 源 stem + 目标扩展名（`src.txt→src.doc`、`fixture.doc→fixture.docx`），落在 `--outdir`；**两份同名（不同目录）源进同一 outdir ⇒ 只剩一份产物**；文件名含中文与空格正常工作。
9. **批量对照**：三份一次调用 1.20 s vs 三份各起一次 3.11 s（每份额外成本 ≈0.6 s，省的是常数不是量级）。
10. **独立 profile 必需**：不带 `-env:UserInstallation` 时 `%APPDATA%\Roaming\LibreOffice` 被创建（污染用户 profile + 与用户手开的实例共享锁）。
11. **filter 名可省**：`--convert-to docx`（不带 `:MS Word 2007 XML`）产出正常 docx，stderr 自报 `using filter : MS Word 2007 XML`；`--convert-to pptx` 同理自报 `Impress Office Open XML`。⇒ 命令里不写 filter 名，少一个字符串。
12. **正文真的进得了抽取器**：`fixture.docx` 含 `word/document.xml`（UTF-8 声明），`fixture.pptx` 含 `ppt/slides/slide1.xml`——正好是 `extract::office_text` 两条分支各自的读取入口。
13. **run 按文种切（对断言有直接影响）**：含拉丁字母的 `合同验收M7哨兵串` 在 docx 里被切成三个 `<w:t>`（`合同验收` / `M7` / `哨兵串`），整串出现 0 次；改成纯中文 `合同验收哨兵串` → 整段一个 `<w:t>`、出现 1 次。→ 夹具与断言一律用**纯中文哨兵串**（M6 那条 `sample-cn-ocr.png` 用了「合同验收OCR」，形状不同，别照抄）。
14. **旧格式夹具的造法（可复现）**：`.doc` ← `soffice --convert-to doc` 吃一个 UTF-8 txt（9,216 B）；`.ppt` ← 用 Python stdlib `zipfile` 手搓一个最小 pptx（2,021 B，含 `<a:t>` 中文）再 `--convert-to ppt`（**606,208 B**）。两条都不需要 Word/WPS。
15. **安装**：`winget install --id TheDocumentFoundation.LibreOffice -e` 走通（拉 374,870,016 B 的 MSI，依赖 `Microsoft.VCRedist.2015+.x64`，末尾提示「重启电脑以完成安装」但 `soffice.com` 当场可用）。winget 本机版本 v1.29.380。
16. **共用 profile 会假失败（T2 执行期实测，`probe6-readings.txt`，四格对照）**：同一 `UserInstallation` 下两个实例**并行** ⇒ 胜者 exit 0 有产物、**败者 exit 1 且零产物**（同 profile 并行 2/2 组里各输一次；同源并行同样输）；**换两个不同 profile 并行 ⇒ 两个都 exit 0**；同 profile **串行 ⇒ 都成功**。
    另附一条现场教训：LibreOffice 在这种「profile 起不来」的场景下，Windows 上会弹**模态对话框**（文案是把人往 `programootstrap.ini 已损坏` 上带的误导话——那个文件实测 112 B、内容是出厂模板、没坏），而模态框没人点就等于子进程不退出 ⇒ 我们的 60 s 预算只能兜住它、不能免掉它。**根因消除靠的是「每笔转换自带独立 profile」，不是靠加超时或加锁。** ⇒ D10 翻转为每文件一个 profile。

## 四、状态口径与契约

| `index_status` | 旧格式在 M7 的写入方 | 变化 |
|---|---|---|
| `ok` | 转换成功 + 抽出非空正文 → `write_doc(DocOutcome::Ok(text))` | 旧格式首次有正文 |
| `skipped` | `empty_text`（转出但抽出为空）；`too_large`（扫描门已挡，沿用） | 复用，不扩值域 |
| `failed` | 超时 kill / 产物缺失 / `office_text` 报错（`extract_failed`） | 复用 |
| `pending` / `missing` | 不涉及 | 不动（`missing` 仍是 `mark_missing` 唯一生产写入点） |

**没装 LibreOffice 时**：`.doc/.ppt` 走 D6 的降级门**不建行**，所以**不存在**「`libreoffice_unavailable` 这种 skip_reason」。

**契约（本批要守的 + 有意破的）**：

- **`generate_handler!` 41 → 42**：本批**唯一**的破例，破的是 D8 那条 `libreoffice_install`。需求方 2026-10-13 当次批准。守护两条：一条测试断言该命令**签名零参数**（给它加一个 `String` 入参就该红），一条断言包 ID 那个字符串在 Rust 侧硬编码、不出自任何入参。理由写在这里而不是只写在代码注释里：这条命令等于「应用能启动一个外部安装器」，是本仓至今最大的一次能力面扩张，**不给前端任意命令执行**是它唯一的红线形态。
- 线格式：`IndexOverview` **仅加一个** `soffice_available`；`SearchBundle` 七键、`DocPreview` 六键两条断言照旧绿不许动。
- **零新依赖**：`std::process::Command` + `std::fs` 够（`tempfile` 在 `[dev-dependencies]`、生产代码用不了，临时目录自己拼 + Drop guard）。LibreOffice 是**外部运行时依赖**，不进 `Cargo.toml`，也不进 `tauri.conf.json`。
- **零新迁移**：`TARGET_VERSION` 仍 4；不写任何 `settings` 新键（D4 无开关）。
- **入库/查询同词器红线不破**：旧格式正文与 OCR 正文一样，走 `write_doc` → `index_text`（jieba `cut_for_search`），与查询侧 `tokenize::query_terms` 同源，分词链一行不改。
- **panic 边界不折叠**：`legacy_text` 只在 `extract_text` 里被调，调用点仍在 `run_pass` 的 `panic_to_err` 之内、`doc_preview` 的 `panic_to_err` 之内——本批**不新建**第四道边界，也不许把已有的三道折成一道。
- **真实数据目录只读红线**不变，真机一律一次性 identifier `dev.zero.pfm.m7test`。

## 五、判据（评审可直接拿来打勾）

1. **三处同源不破**（现成测试 `supported_exts_list_matches_the_dispatch_table` 双向查）：`SUPPORTED` 加 `doc`/`ppt` ⇔ `kind_of` 能分派 ⇔ `extract_text` 的 match 有对应 arm 且真写正文。漏一处 = UI 承诺了搜不到的类型，或真能抽进索引却不列。
2. **转换在锁外**：`legacy_text` 的调用点必须不在 `write_doc`/事务内（评审静态核 `run_pass` 那一段的表达式顺序，同 M6 不变量①的形状）。
3. **降级门只有一处**（D6）：`index_scan.rs` 里除了那个 `match`，全仓不得再出现第二处「因为 soffice 不可用而跳过旧格式」的分支；`grep -rn "soffice" src-tauri/src/` 的命中清单要在 §六 落点表里逐条对得上。
4. **探测的候选与优先级**：`%ProgramFiles%\LibreOffice\program\soffice.com` → `%ProgramFiles(x86)%\...` → PATH 各段下的 `soffice.com`，第一个 `is_file()` 命中即用。**探的是 `.com` 不是 `.exe`**（§三.1/2）。探测不跑子进程（§三.1 实测 `.exe` 零输出，跑了也白跑）。
5. **参数拼法是纯函数**：`legacy_office` 把「argv 怎么拼」做成可单测的纯函数（含 `--outdir <目录>` 空格形、`-env:UserInstallation=file:///<全正斜杠>`、`--headless --norestore --nolockcheck --nodefault` 四件），断言拼出来的参数向量形状；§三.4 与 §三.11 的教训不许靠注释传下去。
6. **临时目录生命周期**：**每笔转换一个一次性目录，里面装着这一笔的 outdir 与 profile**（`<scratch>/out`、`<scratch>/profile`），成功与失败路径都随 Drop 整棵删掉（6 行 guard）。理由：§三.8 的产物覆盖风险 + §三.16 的并发假失败都由「共用目录」引起，一次目录一起删最省事。代价照实翻转：不再 `%TEMP%` 累积 profile（原风险 6 作废），改成每份多付 ≈0.78 s 的 profile 初始化。
7. **成本可控性（不粉饰）**：D1/D3 的成本账 = 几十份 × ≈1.2 s ⇒ 首轮多几十秒，只在「首次遇到该文件」那一轮付（`current_rowid` 的 size+mtime 增量门在后，§三.2 的转换耗时不进写锁）。既有三件兜底（增量、取消、熔断/root 不可达）都在，本批**不新增限速或配额**。
8. **取消语义照实**：正在等子进程时 `cancel` 不生效，最坏等这一次（超时上限 60 s）返回；与 M6 终审 Minor-3 的口径同形，别写成「cancel 立即生效」。

## 六、落点

| # | 文件 | 改动 | 说明 |
|---|---|---|---|
| 1 | `src-tauri/src/legacy_office.rs`（新） | `soffice_path() -> Option<PathBuf>`、`soffice_available() -> bool`、`legacy_text(path, slides: bool) -> AppResult<String>`；私有 `argv_for(...)`、`run_bounded(...)`（`spawn` + `try_wait` 200 ms 轮询 + 到点 `kill`，60 s 预算）、`TempDir` Drop guard | 只读源、只写自己的临时目录（红线）；抽出正文交回调用方走既有分词入库。`legacy_text` 内部**转完调 `crate::extract::office_text(tmp, slides)`**，于是 docx/pptx 那条既有抽取链（含实体还原）一行不改。 |
| 2 | `src-tauri/src/extract.rs` | `DocKind` 加 `LegacyWord`/`LegacySlides`；`kind_of` 认 `"doc"/"ppt"`；`SUPPORTED` 27→29；`extract_text` 加两臂 → `crate::legacy_office::legacy_text(path, false/true)`；**删两处旧文案**：`TEXT_EXTS` 上方注释「.doc/.ppt 属 M7」与 `None` 臂 hint 里那句「.doc/.ppt 属 M7，暂不支持」 | `TEXT_EXTS` 不动（旧格式不是纯文本），与 M6 对图片的处理同形。 |
| 3 | `src-tauri/src/index_scan.rs` | `ScanOptions` 加 `soffice_available: bool`（`load` 默认 `true`，**并钉一条测试**）；降级门加 `Some(LegacyWord\|LegacySlides) if !opts.soffice_available => continue`，与 `None` 同路、先于配额与两桶 | D6 的单一落点。 |
| 4 | `src-tauri/src/index_job.rs` | worker 线程体每轮 `ScanOptions::load` 之后 `opts.soffice_available = crate::legacy_office::soffice_available();`（紧跟 M6 那句 `opts.ocr_available = ...`） | 不记忆化（D5），一轮一次。 |
| 5 | `src-tauri/src/lib.rs` | `IndexOverview` 加 `soffice_available`（值 = 命令调用时的活探测，不读 `ScanOptions` 的惰性默认，与 M6 同一措辞）；新命令 `libreoffice_install() -> AppResult<InstallOutcome>`（**零参数**，`{ started_via_winget: bool }` 出 `startedViaWinget`）；`generate_handler!` 注册第 42 条 | 命令体：探到 winget 就 `CREATE_NEW_CONSOLE` 起安装并立刻返回，**不等安装结束**；探不到返回 false。不起线程、不加 `async`（既有的「全仓唯一一条 `command(async)`」那道门照守）。sync 形态意味着这一次调用睡的是消息泵那个线程（M5 finding 1 的机制）——`Command::spawn` 本身是进程创建、期望毫秒级，但**本批没单独计时**；真机 §九.2 顺手量一次 `invoke` 往返，超过 100 ms 量级就把它挪进既有 worker 线程形态（不预设，取数为准）。 |
| 6 | `src/types/index.ts` + `src/lib/api.ts` | `IndexOverview` 加 `sofficeAvailable: boolean`；加一个 `installLibreOffice()` 包装 | 与 M6 的 `ocrAvailable` 同处同形。 |
| 7 | `src/pages/index-status.tsx` | 支持清单区：清单 29 项自动带出；把「清单之外的类型（.doc/.ppt、压缩包）不会建行：.doc/.ppt 归 M7」改准；新增一行两态提示 + 「安装 LibreOffice」按钮（`startedViaWinget=false` 时前端 `openUrl` 官网页） | 那句旧文案不改就是活假话（M6 收口时同样动过它）。按钮要有失败态：`invoke` 被拒或抛错时按既有 `notice` 红字口径显示，不静默（M4 的 M-8 那条红字残留教训同形，别新增第三条 `setNotice` 无守卫的写路径）。 |
| 8 | `src-tauri/tests/fixtures/` | 新增 `sample-cn.doc`（9,216 B）、`sample-cn.ppt`（606,208 B），正文含纯中文哨兵串「合同验收哨兵串」与「付款条件与响应时间」 | 造法见 §三.14，脚本留在 `.superpowers/sdd/2026-10-13-m7-legacy-office/`（该目录被其自身 `.gitignore` 排除在库外，二进制夹具进仓）。 |

数据流（一句话）：`run_pass` 抽取表达式 → `extract_text` 认出 `.doc/.ppt` → `legacy_office::legacy_text` 起 `soffice.com` 转 docx/pptx 到一次性目录 → `extract::office_text` 抽正文 → 删临时目录 → `write_doc` 走既有 jieba 入库 → FTS/搜索/预览全不动。

## 七、错误处理

- **超时**（60 s 预算，§三.2 稳态 1.2 s 作基数）：`kill` 子进程 → `Err(extract_failed)`，`error_msg` 写「LibreOffice 未在 60 秒内返回」⇒ 该文件 `failed`，一轮继续。这条路径**本机可证伪**：注入一条必然慢的命令（`ping -n 30`）就能打红，不需要一个真会卡死 LibreOffice 的坏文件。
- **产物缺失或零字节**（即便 exit 0，§三.6）：`Err(extract_failed)`，message 带 exit 码与产物名，**不带 stderr 全文**（§三.7 那行噪音会误导人）。
- **抽出为空**：走既有 `DocOutcome::Empty` → `skipped(empty_text)`，与扫描件 PDF、无字图片同口径。
- **子进程起不来**（路径探到但执行失败/被杀软拦）：`Err(extract_failed)` + hint 指「可用 /index 那一行的安装按钮，或用浏览器访问官方下载页」。
- **不 panic**：`Command` 本身不 panic；`legacy_text` 的调用点已在 `panic_to_err` 边界内，`office_text` 若 panic 由同一条边界兜。本批不新建边界、不折边界。
- **错误信息不回显正文**（红线 4 同形）：任何 `error_msg`/日志/测试失败信息只回路径、退出码、长度，绝不拼进抽出文字。

## 八、测试策略

**第一档：不依赖 LibreOffice，恒绿（CI/任何机器都要跑）**

1. `kind_of` 把 `doc`/`ppt` 分派到两个新 variant；`SUPPORTED` 一致性测试自动覆盖两项（既有那条双向查不用改，只是命中数变了）。
2. 降级门两态：`soffice_available=false` 时 `.doc/.ppt` **不建行、不进 `over_size`、不占配额**；`=true` 时进清单。照 M6 的 `images_are_not_scanned_when_ocr_unavailable` 同形写一条旧格式版。
3. `ScanOptions::load` 的默认值钉测试（兑 M6 Minor-6 同形账）。
4. argv 拼装的纯函数测试：`--outdir` 空格形、`-env:UserInstallation` 全正斜杠 `file:///`、四个开关齐、`--convert-to` 不带 filter 名（§三.4/11 的直接兑现）。
5. `soffice_path()` 的候选优先级（把候选构造函数做成吃 `&[PathBuf]` 的纯函数，喂不存在的路径 ⇒ `None`）。
6. 超时 kill：`run_bounded` 吃「怎么起进程」的闭包，注入 `ping -n 30` ⇒ 断返回 `Err` 且**子进程已被杀**（能红，不是恒绿）。
7. 新命令零参数 + 包 ID 硬编码两条（§四 契约）。

**第二档：条件跑（本机装了 LibreOffice 才跑，没装显式打 SKIP）**

8. `legacy_text(sample-cn.doc)` 出含「合同验收哨兵串」的正文（**纯中文哨兵**，§三.13）；`legacy_text(sample-cn.ppt)` 同理。失败信息只回长度（照 M6 T2 收口后的口径）。
9. 转换前后夹具的 `size + mtime` 逐字未变、且夹具所在目录**文件数不变**（红线①⑤，§三.5 的自动化版）。
10. 垃圾字节冒充的 `.doc`：断 `legacy_text` 返回 **`Ok` 且正文非空**（本机 26.8.1.1 实测：LibreOffice 把那串垃圾当纯文本读了出来，`document.xml` 里 1 个 `<w:t>`、首段 65 字节），并断错误信息里没有回显正文。这条是**行为快照**而不是理想行为——它如实记录 §三.6 那个反直觉事实，不假装这类文件会落 `failed`。若某个 LibreOffice 版本让它转成 `Err`，是**重新裁定 + 补 §十一.3 的读数**，不许改断言迁就。

**第三档：端到端 headless（M6 蓝本）**

11. 真 `.doc` 复制进临时目录 → `seeded_project` + `run_pass` → 该行 `ok` → `doc_hits("合同验收")` 命中 → `row_for_preview` + `render_preview` 出含该词的正文 → 第二趟 `unchanged≥1`（**证不重转**，D1/增量的兑现）。`.ppt` 同一条。

**门禁**：`cargo test --lib` 基线 **167**；本批新增条数**一律由每格实跑报出**，spec 与计划里都不预先写「Expected N passed」这种累加值（本仓坑 5，以及「计划里的测试期望数要当场数出实数」那条闸门）。；两条 clippy（`--lib --`、`--lib --all-targets --`）exit 0 / 0 warning；`node --test` 仍 8（前端只加一个字段读取 + 一个按钮，JSX 不在测试网，同 M6）；`npm run build` exit 0。结构门：`generate_handler!` = **42**（本批唯一变动，正则必须让逗号可选 `^\s+[a-z_]+,?$`）、`TARGET_VERSION` 仍 4、`SET index_status = 'missing'` 仍 1 行、`allow(dead_code)` 只剩 `db.rs:30`、`soffice_available` 生产接线两处（worker 填 `ScanOptions` + `index_overview` 透出）+ `libreoffice_install` 注册一处。

**不该写**（避免恒绿断言）：不为「LibreOffice 完全没装的机器整轮体验」造注入点——本批真机用 §九.4 的改名法构造 false，**代码级 false 分支**由第一档第 2/5 条守，不硬造「假装没装」的 app 级断言。

## 九、真机验收（一次性 identifier `dev.zero.pfm.m7test`，跑完删净；真实目录三文件逐字对照；子代理禁 `tauri dev`、控制方执行）

1. **主承诺（本机可验，headless）**：走 §八.11 那条端到端拿到 `ok` / 命中 / 预览 / 不重转四个读数。这是 M7 的本批硬证据，**本机必取到**（LibreOffice 26.8.1.1 已装）。
2. **一键安装的返回体**：CDP 直接 `invoke("libreoffice_install")`（纯字符串 IPC，不需要点原生对话框），断回 `startedViaWinget` 布尔存在。`true` 那一支会真的弹一个 winget 窗口——**这一步由需求方亲手看**（它可能触发 UAC，且会真的动系统），不自动化。
3. **墙钟**：往沙盒放 **30 份** `.doc` 跑一轮，记整轮 wall-time 与「单份均值」，用 §三.2 的 1.2 s 基数对照；再跑第二趟记 `unchanged=30`（证增量免转）。这是 D1 成本账的真机读数，不拿单测冒充。
4. **降级那一支（本批能构造，比 M6 强）**：把 `C:\Program Files\LibreOffice\program\soffice.com` **临时改名**（需管理员）⇒ 起一轮 ⇒ `.doc/.ppt` **一行都不建**、`/index` 念「未检测到 LibreOffice」并出现安装按钮 ⇒ **改回原名并复跑一轮确认建行恢复**。改前改后各取一次该目录文件清单留档。若改名这一手在本机做不动（权限/被占用），就如实记为 deferred，不假装验过。
5. **红线**：真实数据目录 `%APPDATA%\dev.zero.pfm\` 三文件 `size + mtime + SHA-256` 从动手前到收尾逐字不变；沙盒与 `%TEMP%` 夹具删净；`tauri.conf.json` 一字未动（覆盖只走 `npm run tauri -- dev -c`）。
6. **GUI 渲染**：`/index` 那行新提示 + 按钮的实际渲染——受 16 G 提交内存限制（M6 已判，HANDOFF 坑 2），本轮预期仍 deferred；做到「类型接线 + 删 TS 字段 tsc 当场红」为止，照实写进「没能自动验证」，不许假称通过。

**没能自动验证（逐条点名，不许省略）**：WebView2 里那句提示与按钮的实际渲染；winget 真的弹窗口并装成功那一段（需人）；未装 LibreOffice 且**没有 winget** 的机器（LTSC/组策略机）；真实几十 GB 语料里旧格式占比与整轮墙钟；`soffice.com` 在别的 LibreOffice 版本（7.x / 24.x）上的参数兼容性；LibreOffice 被杀软拦截子进程创建的行为。

## 十、不在本轮

| 项 | 归属 | 理由 |
|---|---|---|
| `.wps/.dps/.et/.rtf` | 未排期 | D3；LibreOffice 对金山私有后缀的兼容性本批**一个数都没取** |
| 缓存转换产物 / 新表 / 体积账 | 不做 | D2 |
| 批量转换阶段（一次调用多份） | 不做 | D1；§三.9 实测省的只是常数 |
| 独立转换 worker 队列 | 不做 | D1，M6 的 D1 同一理由 |
| 安装包捆绑便携版 LibreOffice | 不做 | 需求方 2026-10-13 选「探测 + 降级 + 一键安装引导」这一档；捆绑要付安装包 500MB+、首次解压、杀软关、许可证附带文本四件 |
| 设置页开关 / settings 新键 | 不做 | D4，延续「无 settings 写入口」 |
| Word/PowerPoint/WPS COM 自动化 | 不做 | 会在源目录留 `~$` 属主文件、可能弹恢复对话框，与红线 1 摩擦（需求方 2026-10-13 否掉） |
| 扫描版 PDF 内嵌图 OCR | 不做 | M6 的「不在本轮」原样延续 |
| M6 遗留 Minor-4（OCR 探测语言口径）等 5 条有名 Minor | M8 清账批 | 与本批正交；按本仓惯例「里程碑本体与历史债务分两批」 |

## 十一、风险

1. **LibreOffice 版本漂移**：本机 26.8.1.1；他人机器可能是 7.x/24.x，导出 filter 默认选择与产物结构理论上会变。判据里**不写**「必须某版本」，成功判据是「产物存在 + 抽出非空」（§三.6）；一旦版本变更让 `--convert-to docx` 不再自动选 `MS Word 2007 XML`，加固路径是写全 filter 名（§三.11 已备读数）。
2. **首轮成本被低估的可能**：§三.2 的 1.2 s/份是**小夹具**读数；真实 `.doc` 带图片/表格会更慢，且 LibreOffice 对超大 PPT 可能吃几百 MB。几十份 ⇒ 几十秒到几分钟（真机 §九.3 取数）。若久到不可接受，回头加「每项目旧格式上限」，那会牵 `skip_reason` 值域与 UI 文案（本批先取数不预设）。
3. **垃圾文件被当正文索引**（§三.6 实测）：伪装成 `.doc` 的任意字节会以 `ok` 落库、正文是那串垃圾文本，不是 `failed`。后果是搜索噪音而非数据损坏；要改成「必须像 Compound File 才转」要在扫描侧加文件头判据，本批不做、如实登记。
4. **中英混排整词可能搜不到**（§三.13 实测 run 按文种切）：`合同验收M7哨兵串` 抽出后是「合同验收\nM7\n哨兵串」，整串查不中，子词「合同验收」查得中。与既有 docx 通路同形（不是 M7 引入的新缺陷），但要在 §九 的读数里用纯中文哨兵，别拿这条测出假失败。
5. **新命令的能力面**：`libreoffice_install` 是本仓第一条「会启动外部安装器」的命令。缓解已写进 §四（零参数、包 ID 硬编码、两条守护测试）；残余风险是**任何前端代码路径都能调它**（app 自有命令不吃 capability 条目，这是 Tauri 的默认形态）——真出问题只能靠删这条命令。要在终审里点名问一遍。
6. ~~profile 目录留在 `%TEMP%`~~ —— **本条已随 §三.16 的翻转作废**：profile 现在长在每笔转换的一次性目录里，随 `Scratch` 一起删。执行期留证：翻转前 `%TEMP%\pfm-m7\` 累积过 15 个 `profile-<pid>`（各 ~363 KB），正是原设计「故意不删」的账。
7. **606 KB 二进制夹具进仓**：`.ppt` 那枚因 LibreOffice 的 PPT 导出自带母版而大（`.doc` 9 KB）。远端仓库已含业务名与测试夹具（M4 那笔公开账），再加两枚二进制不改变「已公开」的性质，但体积账要在终审提一句。
8. **子进程在单测里真跑**：第二/三档合计约 5–8 s（每份 ≈1.2 s 转换 + 起进程），比 M6 的 0.4–0.5 s e2e 更慢，`cargo test --lib` 总时长要重新看基数；16 G 机上照旧 `-j 1`（HANDOFF 坑 3）。
10. **LibreOffice 弹模态对话框会把一笔转换卡到超时**（§三.16 现场）：触发条件是它自己起不来（profile 被别的实例占着、或 `-env` URL 畸形），而 headless 模式并不保证不弹。**缓解已落在形状里**：每笔转换自带独立 profile（占不到别人的锁）+ URL 只由 `profile_url()` 从我们自己造的目录拼（不给外部输入）；残余风险是别的软件把安装目录搞坏，那时最坏就是 `failed` 一行，不是一轮卡死。真机 T8 起 dev 前先看一眼有没有窗口残留。
9. **等子进程时不响应取消**（§五.8）：常态 1.2 s、上限 60 s；`running` 单飞期间所有旧格式都在这一把锁里排队。不是缺陷，但界面文案别说「随时可停」。

## 十二、要回写与要更正的文档

- **`docs/技术方案.md`**：§6.1 line 166 那句「压缩包与 `.doc`/`.ppt` 旧版 Office 仍不索引（旧格式归 M7）」要改成「`.doc/.ppt` 自 M7 起经本机 LibreOffice 转换纳入索引（未装时整类不建行、`/index` 有一行状态与安装按钮），压缩包仍不索引」，并把「共 27 项」改 **29 项**；§八 里程碑表 M7 行状态；line 215 那句「可选插件」按实况改准（不是插件，是外部依赖 + 引导安装）；§2.2 库行为表把 line 54 那格补上**本机实测**的 LibreOffice 行为（`.com`/`.exe` 之别、退出码不可信、`--outdir` 空格形、独立 profile 必需）；§九 默认值加一条「旧格式需本机 LibreOffice，无开关、探到即用、装完不用重启」。
- **`docs/开发进度.md`**：里程碑表 M7 行 + 新增「M7 验收证据」整节（门禁实跑读数、§九 逐条断言、没能自动验证清单、缺口归属）+ 变更日志一条。
- **`docs/HANDOFF.md`**：§0/§4 的「下一格」转 M8；§5 新增本轮踩到的坑（至少四条：MSYS `/c/...` 与 `--outdir=` 等号形都会让 LibreOffice 直接失败；畸形 `-env` URL 是**慢**失败不是快失败；含拉丁字母的哨兵串会被 run 切分；`soffice.exe --version` 零输出）；§6 指针表加 M7 的 spec/计划/账本/探针四行。
- **不动**：历史 spec/计划与各里程碑验收节（不 retro-edit）；`.superpowers/sdd/` 下被文档按路径引用的旧证据目录。M7 自己的工作区目录 `2026-10-13-m7-legacy-office/` 已建（探针脚本与读数在此）。
