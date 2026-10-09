# 交接文档 —— M4 首屏统一检索

**快照时间**：2026-10-09 约 13:30（本地）—— 需求方 10-09 指示「暂停开发、写交接」；本文件第 **8** 次刷新
**分支**：`feat/m4-unified-search`，代码 HEAD `ef49c1a`（D6 三个上限按实测上调），其前依次 `cd04f33`（把真机与 D6 读数落回计划与 spec 的 docs 提交）、`8ebdb92`（T9 派发前预检）、`62af638`（本文件上一版）、`0757545`（T8 fix round 1）。`git rev-list --count master..HEAD` 在 `cd04f33` 时是 **36**，本次文档提交落地后 **37**；`497c0a1..HEAD`（两天累计）= **34 → 35**；提交后 `git status --porcelain` 干净。**取数只认命令**（坑 73）。
**门禁基线**（**控制方在 HEAD `ef49c1a` 的树上现跑**，不是转述报告）：`cd src-tauri && cargo test --lib` → **134 passed / 0 failed / exit 0**（9.52s）；`cargo clippy --lib -- -D warnings` 与 `cargo clippy --lib --all-targets -- -D warnings` → **两条 exit=0、0 warning**；`node --test "tests/**/*.test.ts"` → **`ℹ tests 8 / ℹ pass 8 / ℹ fail 0` / exit 0**；`npm run build` → **exit 0**；`grep -rn "searchLocal" src/` → **exit=1**（T8 的删除门）；旧上限字面量 `grep -rn "20/10/10\|20 簇\|10 条\|10 平铺" src/ src-tauri/src/` → **exit=1**（D6 改完没留假话）
**一句话状态**：M4 九个任务里 **Task 1–8 已完整收口**（账本里 `Task N: complete` 共 **8** 节）。**Task 9 做了一半**：真机六条断言 + 两格实测 + D6 的 5 万行实测**全部拿到读数**，D6 的代码后果（三个上限 20/10/10 → 30/15/15）已由实现者落成 **`ef49c1a`**、控制方把读数落回计划与 spec（`cd04f33`）。**但 T9 没收口**，差两件事，且它们是顺序的：① **`ef49c1a` 还没过任务评审**（评审包都没生成、双 verdict 没拿）；② **Step 4 的文档收口没动**（`docs/开发进度.md` 的 M4 行 `:13` 还写「未开始」）。**没有被技术问题卡住，也没有轮次配额烧完** —— 这是需求方 10-09 按下的暂停键。今天需要你亲手点的那三下（原生目录选择框 / `revealItemInDir` / `window.confirm`）**已经点完并拿到读数**，恢复后不再需要先等你。

---

## 1. 我们在做什么任务

做的是本地桌面工具「项目资料管理」（Tauri 2 + React + rusqlite bundled FTS5 + jieba-rs），按里程碑推进：M0 脚手架与 WAL 校验、M1 项目档案与目录映射、M2 信息台账与列加密、M3 全文索引管线都已 ✅ 完成。**M4 = 首屏搜索框统一结果**，做完 M4 就是 MVP（M0→M4）。

M4 本轮交付的形状（权威定义在 spec，不要从本文件重述去实现）：

- **两条 IPC 命令**：`search_all` 返回一个 `SearchBundle`（三段：项目档案 / 信息台账 / 文档正文），`doc_preview(doc_id, query)` 返回窗口拼接的原文 + UTF-16 码元高亮区间。
- **段内按项目聚簇，不跨段合并**：`bm25` 与 `LIKE` 的分数不可比，spec §3.4 明令禁止假归一化。
- **三级截断 30 簇 / 15 条 / 15 平铺，各有计数、只显示不展开**（原设计是 20/10/10，**2026-10-09 按真机 5 万行实测上调**，读数与口径见 §4.5 与 spec §四；`DOCS_FETCH_LIMIT` 仍是 200）。
- **预览**：词只从 `tokenize::query_terms` 拿（入库/查询同词器是这套检索的地基红线），`ranges` 用码元下标让前端 `text.slice` 零换算。

执行方式是 `superpowers:subagent-driven-development`：每个任务一个全新实现者子代理 + 一个任务评审（spec 合规 + 代码质量双 verdict）+ 修复轮（≤5 轮），全部任务完成后做一次整分支终审。**连续执行，不在任务之间停下来问「要不要继续」**；只有四类情况才停：不可逆/破坏性操作、安全敏感动作、超出本 worktree 的副作用（merge/push/publish）、计划坏到每条前进路径都是猜。

绑定红线（每条派发词里都要带上，违反即返工）：

1. **工具绝不动被索引的原始文件** —— 只登记路径、只读文件。夹具只能在 `tempfile::tempdir()` 与 `db::open_in_memory()` 里造。
2. **真实数据目录 `%APPDATA%\dev.zero.pfm\`（库文件 `ledger.db`）是只读红线**；真机验证一律用一次性 identifier `dev.zero.pfm.m4test`，跑完删净，绝不为验证改 `src-tauri/tauri.conf.json`。
3. **子代理一律不许跑 `tauri dev` / `npm run dev`**；「UI 实际渲染未验证」必须如实写进报告。
4. **密文列不参与任何检索** —— 新增 SQL 里不许出现 `*_cipher` / `*_nonce`。
5. **验证脚本只回断言结果**（布尔 / 条数 / `matchedBy` / 文案前缀 / 尺寸），绝不打印抽出的正文或任何解密内容。
6. **Git：无远端，只本地提交，绝不 push、绝不 amend、不 `--no-verify`、不 `git add -A`**，按文件 name 逐个 stage。输出语言一律中文（错误码、SQL 字面量、JSON key 除外）。

---

## 2. 已经完成了什么

### 2.1 里程碑与文档（10-07 及更早）

- **M0–M3 全部 ✅**，证据与真机结论在 `docs/开发进度.md`（里程碑表 `5-20`、环境结论 `22-45`、M3 验收证据 `70`）。
- **M4 spec 定稿**（`807e3c1` + 写计划前修订 `34a48e1`）：`docs/superpowers/specs/2026-10-07-m4-unified-search-design.md`。决策 D1–D7 每条都带被否掉的备选与理由；§十把 M3 划过来的账逐条写明为什么不进本轮；§十一 4 条风险（最硬一条：5 万行量级 `LIKE` 全表扫是首屏新引入的每键开销）。
- **M4 实施计划定稿**（`a44cdac`，**现 2492 行**；定稿之后计划本体又被改过 **16 次**（10-09 占四次：`4b8c851` T8 预检四处、`b2670db` T8 评审四处、`8ebdb92` T9 预检三处、`cd04f33` T9 读数回写并勾掉 Step 1/2/3），数法：`git log --oneline -- docs/superpowers/plans/2026-10-07-m4-unified-search.md | wc -l` 减掉定稿那次 —— 别背这个数，改一次就漂）：`docs/superpowers/plans/2026-10-07-m4-unified-search.md`，9 个任务，每个都是「失败测试 → 跑红 → 最小实现 → 跑绿 → 变异取证 → 门禁 → 按名 stage 提交」的可照抄粒度。**规则**：凡已切过简报的任务，其计划正文被改到就必须重切简报 + Grep 验证（坑 58），否则实现者照抄被否决的旧写法。

### 2.2 这两天（10-08 实现 T1–T7、10-09 实现 T8 并做完 T9 的真机 + D6）：`497c0a1..HEAD` 共 35 次提交（含本文件这次；`master..HEAD` = 37；两个数都是本文件刷新时当场 `git rev-list --count` 量的，不是累加出来的）；**Task 1–8 全部收口，Task 9 欠两步**（见 §3）

| 任务 | 提交链 | 该任务终态 | 评审结论 |
|---|---|---|---|
| T1 `query_terms` 唯一切词入口 + `MAX_QUERY_CHARS` | `497c0a1` | 106 passed / tokenize 7 | Spec ✅ / Approved，0C 0I 7Minor：3 条折进 T3 的「注释保鲜」Step，其余 4 条当场裁定**不改**（各有理由，见账本） |
| T2 `panic_to_err` 提到 `extract.rs` 做 `pub(crate)` | `f72a5c8` | 106（extract +1、index_job −1） | Spec ✅ / Approved，0C **1I**（plan-mandated）2M。`Cargo.toml:46` 的悬空指针判为**计划本体缺陷**，改在 T5 的 Files+Step 6 里收口 |
| T3 `clean_snippet` 的 `A B AB` 三连折叠 | `e18864a` → 计划修订 `29d390f` → 整改 `2670768` | 108（tokenize 9） | 首轮 Spec ✅ / **Needs fixes**（0C 3I 6M，其中 2 条 Important 是简报本身写错：签名 `E0106`/`E0716`、那条变异**三重蕴含**下永远不红）→ fix round → 复审 7 条全 ADDRESSED |
| T4 `search::unified_bundle` 三段聚合 + 段内聚簇 + 三个截断计数；`index_store::indexed_project_count` | `899b58c` → 计划修订 `9dca06c` → 整改 `91476b9` → 计划修订 `cdc5163` | **120**（search 20、index_store 15） | 首轮 Spec ✅ / **Needs fixes**（0C **3I** 5M）→ fix round 1 → **定向复审：5 条 finding 全 ADDRESSED，无新 C/I**，剩 7 条文本级/登记类 open |
| T5 `doc_preview.rs` 重抽原文 + 窗口拼接 + UTF-16 码元区间 | 计划修订 `c5ed0b1` → 实现 `614f1a2` → 计划修订 `ea4187d` → 整改 `d544175` | **134**（doc_preview 14） | 派发前预检切掉两处计划原文自相矛盾（坑 59）；首轮 Spec ✅ / **Needs fixes**（0C **2I** 5M，三条具名风险全 SETTLED）→ fix round 1 → **定向复审：5 条全 ADDRESSED、无新 C/I**，具名风险 SETTLED（复审自己复现了变异读数并给出「多一扇窗 ⇒ 其后每个命中起点位移 ≥3 码元」的一般化论证） |
| T6 两条 IPC 命令接线（`search_all` + `doc_preview`） | 实现 `6bc6354` → 计划/spec 修订 `3677865` → **fix round 1 `077c275`** | **134**（本任务 0 条新单测），handler 39 → **41**，`async` 属性唯一落点 `lib.rs:558` | 首轮 **Spec ✅ / Approved**（0C **1I** 4M，三条具名风险全 SETTLED）。I-1 = 「sync 形态把『锁外抽盘』买到的东西抵消掉了」，控制方到 `tauri-macros`/`wry`/`tauri-runtime-wry`/`tauri` 四处源码逐行复核后**采信并选修法 (a)**：`doc_preview` 加 `#[tauri::command(async)]`，`search_all` 留给 T9 实测。fix round 1 只动三件（属性 + `lib.rs:481-482` 两句注释 + 删一个空行），**只提交 `lib.rs` 与 `doc_preview.rs` 两个文件** → **定向复审：Approved / Spec ✅，风险 A（一行属性兑现没兑现）SETTLED、风险 B（还有没有别处在说同一句假话）NOT SETTLED = 恰好一处** → 那 1 条 Important + 3 条 Minor 全部**裁为落终审**（复审建议如此，理由见 §3 缺口表），不开 round 2 |
| T7 前端类型 + `api.ts` 两条封装 + `search-order.ts` 纯逻辑 + 仓库第一个 `node --test` | 派发前预检 `3a2cfc8` → 实现 `23b8b53` → 计划修订 `c2b3d35` → **fix round 1 `034b0ef`（已完成）** | Rust 侧仍 **134**（本任务零 Rust 改动，四条 Rust 门禁在此是无回归检查）；`node --test` **`pass 8 / fail 0 / exit=0`**（整改夹具并进第 3 条，**没有**新开第 9 条 ⇒ `pass 8` 那 7 处口径全部不动）；首轮实现恰好 5 文件、fix round 恰好 **2 文件（`api.ts` + `tests/search-order.test.ts`）**，`src/lib/search-order.ts` 与 `src/types/search.ts` 在整改轮**零改动**（`git diff --exit-code` 三条对账 = 0），`package-lock.json` 未动 | 首轮 **Spec ✅ / Request changes**（0C **3I** 4M）。四条具名风险：R-1 线格式对账 **SETTLED**（评审逐字段读了 `search.rs:36-88` 四处 `#[serde(rename_all="camelCase")]`、`index_store.rs:222-240`、`doc_preview.rs:39-49`，含 `lib.rs:265 query` / `:561 doc_id` 的入参名）；R-2 `isNewest` 等价变异推论 **SETTLED**（`local-search.ts:17/27/44` 保证 `latest ≥ mine`）；R-3 注释成谎 **NOT SETTLED** ⇒ I-2；R-4 value-import 门禁主目标达成、残留为次要形态。**I-1 = 计划本体给夹具时就漏了段序守护**，控制方用两次实验钉死：交换 `bundleToSections` 的 ledger/docs 两个 `if` 块 ⇒ `pass 8` 全绿；补三段同现夹具 ⇒ 同一处交换 `pass 7 / fail 1` 且失败点指名第 3 条。走 `sdd-plan-revision-fix-loop` 改计划本体（三处）+ 重生成简报（282→300）而不是先派发再返工。**fix round 1 定向复审：Approved / Spec ✅，I-1/I-2/M-1 三条全 ADDRESSED、无新 C/I**；具名风险 NR-1 由复审**自己重做**变异实验并 **SETTLED**，还给了一个比控制方更强的判据（见 §5.G 第 71 条的追加），并把报告 §8.8.4 的一处自我描述**向上纠正**为假门禁 |
| T8 首屏渲染：store 换 bundle + 三段结果 + 原文预览对话框 | 派发前预检 `4b8c851` → 实现 `40f4ca3` → 计划修订 `b2670db` → **fix round 1 `0757545`（已完成）** | **零 Rust 改动**（`cargo test --lib` 仍 **134**，此格是无回归检查）、`npm run build` exit 0（产物 `index-Bw2yV3Qo.js`）、node 侧仍 **`pass 8 / fail 0`**、T8 自己的删除门 `grep -rn "searchLocal" src/` **exit=1**；首轮恰好 6 文件、fix round 恰好 **3 文件**（`doc-preview-dialog.tsx` / `local-search.ts` / `search.tsx`），`search-order.ts`/`types`/`tests`/`src-tauri` 全程零改动（复审用 `sed -n '20,65p' brief \| diff - src/stores/local-search.ts` = **exit=0** 验逐字性） | 首轮 **Spec ✅ / Request changes**（0C **1I** 8M），具名风险（段级与簇级两套截断说明会不会同屏重复）**SETTLED**：三个上限各指一个不同的轴（平铺段头 / 整簇被丢 / 簇内被截），`cluster_by_project` 先算 `hidden_clusters` 再 `take` 再 `truncate` ⇒ 集合不相交，量词也不同（「个项目」vs「条」）。**I-1 又是计划本体的错**（简报逐字给了 `void api.revealFolder(...)` / `void api.copyText(...)`，把两个**设计承认会失败**的动作的 rejection 吞成零反馈 —— `api.ts` 那句注释自己点名剪贴板失焦会抛、`doc_preview.rs` 专造了 `preview_file_missing`）。裁定：照 `project-detail.tsx::guard` 的既有形态补本地 `guard` + **独立的 `notice` 态**（不复用 `error`：`error` 一真正文整段不渲染，而复制失败必须留着正文让用户再试）。同轮落 M-1（切 `docId` 先 `setData(null)`）、M-2（no-hit 文案改读 `bundle.query`，与判据同源）、M-6（`LocalSearchState.query` 写了没人读，删）。**定向复审：四件全 ADDRESSED、Approved / Spec ✅、0 新 C/I、1 条新 Minor（M-8 `notice` 跨文档残留）** ⇒ 与另 4 条文本级残留一并**落终审，不开 round 2** |
| T9 真机验收 + D6 实测 + 文档收口（**做了一半，未收口**） | 预检 `8ebdb92` → D6 代码 `ef49c1a`（实现者）→ 读数落回文档 `cd04f33`；**评审包未生成、双 verdict 未拿；Step 4 未做** | 真机部分**零代码改动**（D6 那一改除外）；`cargo test --lib` 仍 **134**、node 仍 **`pass 8`**、两条 clippy exit=0；沙盒三处已删净、真实目录 `stat` 三行**与基线逐字一致**（只读红线守住）；`ef49c1a` 只动 `src-tauri/src/search.rs`（14 ins / 14 dels） | **尚未评审**。已确定的是读数：断言 1–5 全绿（线格式 7 键 / snippet 无分词空格残留 / `text.slice(ranges[0]) === "验收"` / `preview_unavailable` / `preview_file_missing`）；断言 6 的**前提被当场改写**（`search_all` 是 sync、消息泵串行 ⇒ 它守的不是乱序）；第 7 格 114 万码元 + `truncated=false` 但 **longtask 全程为空** ⇒ 判据字面的「且」不成立，议题按证据重写；第 8 格 **`async` 兑现成立**（`db_status` 3 ms 先回 vs 慢预览 266,093 ms）；D6 ⇒ 稳态 675–737 ms 未越 800 ⇒ `DOCS_FETCH_LIMIT` **维持 200**，触顶簇 100%（稀疏对照 `capped=0` 证明不是夹具假象）⇒ **三个上限 20/10/10 → 30/15/15** |

今天最硬的一条证据：T4 的**变异 6**（`partition` 谓词 `== "project"` → `!= "note"`）在补第 11 条测试**之前**跑出来是**全量 119 条照绿** —— 即等价变异，客观证明「台账段」这个三段的三分之一**零行为守护**；补完 `ledger_section_clusters_its_own_hits_instead_of_leaking_into_projects` 之后，同一个变异的**唯一红点**是 `src-tauri/src/search.rs:638` 那句「`「生产门户」不是项目名，平铺段必须空`」，其余 119 条照绿。

三条 Important 的裁定（权威文本在账本 `## Task 4 首轮评审裁定`）：

- **I-1（台账段零守护）**：授权补第 11 条测试，并**全链同步条数** —— 119→120、search 19→20、下游 133→134（T5/T6/T7/T9 正文与「完成判据」一起改）。
- **I-2（三个截断计数只相对本次取到的样本，会低报）**：**不加 `docsCapped` 线格式位**。spec §三 把 bundle 的 7 个字段定死为线格式契约，加一位是 spec 级改动，不由实现者或控制方在本轮擅自扩；D7 禁的是「静默丢弃」，而当前每个截断都有承载位。落地为两处诚实性：`DOCS_FETCH_LIMIT` 头注释 + 三个计数字段的注释写明会低报；T8 的文案只能说「本次结果里另有 N 条未展开」，不能说「库里还有 N 条」。**这条留了一个待用户点头的问题**（见 §3）。
- **I-3（注释里的假声称）**：`cluster_by_project` 头注释改为**逐键写守护现状**（第一级条数键由 5 条测试守着；第二级名字键在「删除」形态只有 2/10 概率红，在「反转方向」形态确定性红；**第三级 id 键当前没有任何测试能打红 = 有名缺口，交 M5**），并把变异 2 的形态从「删行」改成「反转比较方向」，把「去掉第三级」列为**预期不红**的取证条目第 7 条。

复审还独立纠了控制方一处取证口径：它不采信 `bundle_wire_format_is_camel_case`（presence-only 的 `contains_key` 锁不住键集合），改从结构体本体数出 7 个 `pub` 字段。这条我采纳并折进 T6（见 §4.3）。

**当前逐模块实数**（T5 整改落地后 `cargo test --lib` 全量输出；不是筛选命令的聚合行）：
tokenize 9 / extract 15 / index_job 12 / index_store 15 / search 20 / **doc_preview 14** / db 8 / index_scan 10 / ledger 12 / project 10 / vault 9 = **134**。

T5 两条 Important 的裁定（权威文本在账本 `## Task 5 首轮评审裁定` 与 `## Task 5 fix round 1 复审`）：

- **I-1（并窗判据拿错了操作数 = 真行为缺陷，且继承自简报与 spec 的字面表述）**：判据改成比较**新窗左沿** `lo = start.saturating_sub(半宽)`，即 `Some(prev) if lo <= prev.1`；spec §五.5 与计划本体都已就地更正并注明来源。代价：合并带翻倍 → 链式合并让一扇窗可接近整篇，`text` 载荷失去长度上界（`MAX_PREVIEW_WINDOWS` 封的是窗数）；这条裁为**接受**并在 T9 真机侧看观感（坑 61、§4.5 第三条）。
- **I-2（`near_hits…` 名不符实的假守护）**：夹具按半宽重算成 8002 间距（第二处命中的窗左沿恰等于第一扇右沿），断言文本、测试名、`body` 签名、14/134 条数链全部不动，另加两条防退化的断言；同一夹具兼任第二格取证（把 `lo` 换回 `start` 也必须红）。效果：T4 那轮的「等价变异全绿 = 零守护」规矩在这里把缺口 B 撤了案。

T6 一条 Important 的裁定（权威文本在账本 `## Task 6 首轮评审裁定（round 1/5，fix round 派发前）`）：

- **I-1（sync 形态把「锁外抽盘」买到的东西原地退掉）**：Tauri 命令默认 `execution_context = Blocking`，而 Windows/WebView2 上 `ipc_handler` 是在 `add_WebMessageReceived` 的 COM 回调里**同步调用**的 —— sync 命令的命令体占的就是消息泵那一个线程，期间没有任何别的 `invoke` 进得来。控制方没有直接采信评审，而是逐条到本机 vendored 源码复核（`tauri-macros-2.7.0/src/command/wrapper.rs:245/:257-258/:355/:382-389`（`$path` 在 `:384`）/`:425/:427`、`tauri-2.12.0/src/ipc/mod.rs:371/:375`、`wry-0.57.0/src/webview2/mod.rs:944-978`、`tauri-runtime-wry-2.12.0/src/lib.rs:5289-5307`），全部为真才落裁定：**选评审给的两条修法里的 (a)** —— `doc_preview` 加 `#[tauri::command(async)]`（非 `async fn` ⇒ kind 是 `sync_threadpool`，命令体被生成进 `async move` 块、再由 `async_runtime::spawn` 投递），`search_all` 本轮保持 sync，交给 T9 的 5 万行实测（判据已写死在 T9 Step 3 新增的第三段）。**这一轮复审的副作用：本文件与计划原本写的 `:357`/`:383-388` 两个行号是错的**，复审后当场重 `grep` 更正为 `:355`/`:382-389`（坑 65 的重演，见 §5.G）。**控制方自查还多核到一条评审没写的事实**：全仓只有 `lib.rs:37` 一处取 `Mutex<Connection>`、既有 39 条命令的 `db(&state)` 调用点全在 IPC 命令里，而后台索引线程自己开连接（`index_job.rs:402`）⇒ 加 `async` 之前这把锁**无人竞争**，「锁外抽盘」对界面延迟的真实收益是 0；加了 `async` 它才第一次承重。这就是不选 (b)「只降级注释」的理由：出锁与换线程是同一个契约的两半。代价三条：① 同类命令从此**可能乱序返回**（sync 下由消息泵天然串行，这事以前不可能发生），T7 的 `isNewest` / T8 的 store `seq` 守卫从防御性写法升为**承重**，派发词已带这一条；② 运行时兑现 134 条单测一条都不覆盖，唯一证据是 T9 Step 2 新增的第 8 条并发对账（`db_status` 是否先于慢预览返回）；③ 顺带把 `lib.rs:481-482` 那句 M3 写下的「此时别的 IPC 进得来」改成实话（它是 sync 命令，那句按机制根本不成立）。

**T6 fix round 1 的定向复审结论（`077c275`，权威文本在账本 `## Task 6 fix round 1 复审`）**：Approved / Spec ✅，无 Critical，**1 条 Important + 3 条 Minor，四条全部裁为落终审、不开 round 2** —— 这是复审自己的建议原话（「我建议的处置：改一句话、落终审、控制方本轮把它连同 M-1 的三处指针一起写进已提交的 HANDOFF」），本文件就是它的落点，因为**账本 gitignored、只落账本等于没落**。两条具名风险：风险 A（一行属性到底兑现没兑现）**SETTLED**；风险 B（还有没有别处在说同一句假话）**NOT SETTLED = 恰好一处**，且复审把症状**改强**了 —— 见缺口表第一行。

**T7 三条 Important + 那条假门禁的裁定（权威文本在账本 `## Task 7 首轮评审裁定（round 1/5，fix round 派发前）` 与 `## Task 7 fix round 1 复审（round 1/5 → complete）`）**：

- **I-1（段序零守护 = 计划本体给夹具时就漏了）**：**不新开第 9 条测试**（那要动 `pass 8` 的 7 处口径链），而是把「三段同现 + `relaxed: true`」夹具**并进第 3 条**，断言 `kind` 序列与三格各自的 `note`（平铺段与台账段都不许跟着说「含前缀放宽匹配」）。依据 = 控制方自己复现的交换实验；同一实验升格为 fix round 的验收判据，写进计划 Step 5 新增的第 6 条变异。代价：零生产代码改动，测试体积 +17 行。
- **I-2（`api.ts` 那句注释「今日半谎」）**：改成**时间锚**写法（「页面切到这个入口是 Task 8 的事，`searchLocal` 在迁移完成前仍被旧调用方用着」），不写路径锚/行号锚（坑 67 同族）。`searchLocal` 迁完必零调用方这一格**裁给 T8 派发前预检**，不在 T7 删 wrapper —— 删它会牵动 T9 的真机对照通路。
- **I-3（评审要求「重复口径各留一份」要收）裁定不做**：T8 的计划文本已逐条安排到位（`import { isNewest }` 与三处 `isNewest(mine, seq)`；`SOURCE_LABELS` 留作**簇内行内徽章**、段标题走 `SECTION_TITLES`，即两者本就不该合并），代价为零；但**派发 T8 时必须点名这三格归属**，否则 `isNewest`「只被测试用着的孤岛」会继续存在。
- **M-1（计划正文自己写了一条假门禁）**：Step 4 原文「`npm run build` 会抓到线格式对不上」按坑 69 改写为「只保证 TS 侧自洽；跨侧对账 = `search.rs` 既有的 `bundle_wire_format_is_camel_case`（断言键集合恰好 7）+ 评审逐字段读两个 `#[serde(rename_all = "camelCase")]` 结构体」，并把 **`DocPreview` 无 Rust 侧 wire 测试**当场 grep 后登记为有名缺口（补它 = 134→135，会重开 Task 5 已定稿的模块条数与整条链，而 T7 不是归属任务；中间由 T9 真机 `invoke("doc_preview")` 读数兜住）。
- **定向复审贡献的一条可迁移判据（比控制方那格实验更强）**：`bundleToSections` 是**静态块序**（没有输入相关的排序逻辑），所以那条整数组 `deepEqual` 在六种排列里只放过恒等式 ⇒ 三段同现**传递性覆盖**全部两段同现的子序，没有段序错乱绕得过去。复审据此把报告 §8.8.4 的自述**向上纠正**（原写「只调两段且第三段为空仍靠自证」是低估自身覆盖度），判为良性、不阻塞本轮，终审收报告时改写那一格。

**SDD 工件现状**（`.superpowers/sdd/2026-10-07-m4-unified-search/`，**gitignored ⇒ 陈旧只伤下一会话，本文件才是已提交的恢复地图**）：账本 `progress.md`（**308 行 / 25 个 `## ` 段**：预检 10 行表 + 红线 + T1–T8 八个 complete 节 + 两个暂停点 + 每任务的首轮裁定/复审节；**最后一节 `## Task 8 fix round 1 复审（round 1/5 → complete）` 在 `:298`** = 当前状态；用 `grep -n "^## " progress.md` 现取，别背行数）；简报 T1–**T8** 已生成（T5 **551 行**、T6 **167 行**、T7 **300 行**、**T8 361 行** = 首切 318 → 预检重切 344 → 首轮评审后 343 → fix round 前 **361**，一个任务里重切四次是计划本体被改四轮的正常痕迹）；报告 T1–T8（`task-8-report.md` **694 行 / 九节**，§九 = fix round 1；`task-8-rereview-1.md` 100 行是定向复审的落盘）；报告 T1–T8（`task-7-report.md` **769 行 / 八节**；`task-8-report.md` **694 行 / 九节**，§九 = fix round 1，含每条门禁原始输出与 `exit=` 行、四件的可证伪证据、还原的四件字节级证据）；评审包 **14 份** diff（T8 两份：首轮 `review-4b8c851..40f4ca3.diff` **1 commit / 20123 B**（基线是 docs 提交，天然在范围外）、fix round `review-40f4ca3..0757545.diff` **2 commits / 21595 B** —— docs 提交 `b2670db` 晚于 FIX_BASE、靠基线排除不掉，派发词显式声明它是**规格来源、不是评审对象**，复审照做、对文档零 finding）。**T5 已 complete**（`614f1a2` + `d544175`）；**T6 已 complete**（`6bc6354` + `077c275`）；**T7 已 complete**（`23b8b53` + `034b0ef`）；**T8 已 complete**（`23b8b53` 起算的两提交：`40f4ca3` + `0757545`）；**T9 = 未收口**（`task-9-brief.md` **96 行**（首切 93 → 预检改三处后 96）；实现者的 `task-9-report.md` **302 行**；控制方自己的真机报告 `t9-report.md` **80 行**，其 §五/§六 是占位，等 Step 4 一起收；证据文件 10 份 `t9-*.json` + 两张截图 `t9-shot-{results,nohit}.png` + 基线 `t9-realdir-baseline.txt`，全在同目录）。**评审包仍是 14 份 —— `ef49c1a` 的评审包还没生成，这是恢复后的第一件事。**

---

## 3. 当前卡在哪

**没有被技术问题卡住，也没有轮次配额烧完。** T1–**T8** 各自走完「实现 → 双 verdict 评审 →（需要时）计划文本闭环 → 修复轮 → 定向复审 → 账本 `Task N: complete`」，T7 与 T8 都只用掉 round 1/5 即收口。**当前停着是需求方 2026-10-09 的指示「暂停开发、写交接文档」，不是障碍。**

**T9 停在一个不好停的位置：它的真机与实测全做完了，但代码后果没收尾。** 两件事按顺序欠着：

1. **`ef49c1a`（D6 三个上限 20/10/10 → 30/15/15）还没过任务评审** —— 评审包都没生成。它是走过 fix round 的六个任务之外**第一个「控制方已派发实现者、但没评审」的提交**，账本与本报告都点名了这一点。恢复后第一件事：`review-package PLAN_FILE 8ebdb92 ef49c1a` —— **BASE 用 `8ebdb92`（派发前的 HEAD，它自己就是 T9 预检那个 docs 提交，正好在范围外）**，所以这一次范围里只有那一个代码提交，不用做「docs 是规格来源」的声明；`cd04f33`（我把读数落回计划与 spec 的那次 docs）**在 `ef49c1a` 之后**，别把它当基线。
2. **计划 Task 9 的 Step 4「清场 + 文档收口」只做了清场**：沙盒三处已删净、真实目录 `stat` 与基线逐字一致，但 `docs/开发进度.md` 的 M4 行 `:13` **仍写「未开始」**，测试条数 / 真机结论 / 没能自动验证清单 / §3 尾巴第 3 条那笔归属账，一条都还没写进去。

**已经拿到、不用再重跑的读数（下一轮别重复劳动）**：断言 1–5 全绿；断言 6 成立但前提改写；第 7 格 1,139,999 码元 + `truncated=false` + `longTasks` 全空；第 8 格 `db_status` 3 ms 先回 vs 慢预览 266,093 ms；D6 稳态 675–737 ms / 触顶 100% / 稀疏对照 `capped=0`；M-8 已用 CDP 复现。**原始 JSON 与截图都落在 `.superpowers/sdd/2026-10-07-m4-unified-search/` 下（`t9-*.json`、`t9-shot-*.png`、`t9-realdir-baseline.txt`），报告是 `t9-report.md`（控制方）与 `task-9-report.md`（实现者）。**

**T9 及之后每一轮都必须带的那条前提**（T7 的 R-2 复核过、T8 的复审又核了一遍，成立）：`doc_preview` 是 `#[tauri::command(async)]`（全仓唯一一条 async 命令），同一类命令的两次调用从此**可能乱序返回**（sync 形态下由消息泵天然串行，这事以前不可能发生），所以 `src/stores/local-search.ts` 里那三处 `isNewest(mine, seq)` 与 `clear()` 的 `seq++` 是**承重件**，不许被当成可选优化删掉或弱化 —— T8 已经把 `isNewest` 从「只被测试用着的孤岛」接成了承重设计，复审逐字核对过它没被弱化。**T9 的并发对账（`db_status` 是否先于慢预览返回）是这条承重件唯一的运行时证据**，134 条单测一条都不覆盖它。

需求方 2026-10-09 已裁定的一件事（不用再问）：**「结果被取数上限截过」不加 `docsCapped` 线格式位，按现方案收口** ⇒ T8 的文案就是现在这版（段级 `s.note` 说实话、簇级 `clusterNote` 说实话），M4 后续不许再扩线格式。

**T9 只有人能做的部分 —— 10-09 需求方已全部点完，读数记录在下方，恢复后不需要你先动手**（三条路在 CDP 里点不通，M3 已实测确认）：原生目录选择对话框、`revealItemInDir`（在资源管理器里选中文件）、`window.confirm`。操作口径三条红线：① 只用一次性 identifier `dev.zero.pfm.m4test`（`npm run tauri -- dev -c '{...}'`，`-c` 必须走 npm 的 `--`），**绝不碰真实数据目录 `%APPDATA%\dev.zero.pfm\ledger.db`**；② **绝不为验证修改 `src-tauri/tauri.conf.json`**（覆盖只走命令行 `-c`）；③ 需要 CDP 时临时设环境变量 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`，跑完删掉沙盒目录与孤儿进程（vite 1520 / `msedgewebview2.exe` 9222，坑 30）。**另外真机时会看见一个已知缺陷**：关掉预览再打开另一份文档，上一条「操作失败：…」的红字可能还挂着（M-8，已登记在 §4.3 末的终审清单，一行级修法），**那不是新发现，不用再开一轮**。

**那三下的读数（需求方 10-09 逐条回答，一次一问拿到的）**：① **原生目录选择框** —— 弹出、能选中 `Temp\pfm-m4-fix\p2`、项目「手点验证」建得出来；② **`revealItemInDir`** —— 真的弹出资源管理器并高亮在那个文件上（这条通路 M3 只能验到代码形状，本轮第一次拿到真机证据；Task 8 补的失败回执走的是另一条：控制方用 CDP 复现了失败态，回执是红字 `操作失败：path doesn't exist`，**没有静默**）；③ **`window.confirm`** —— 确认框弹出、点「取消」拦得住、点「确认」真删掉（与库里软删 + 项目数回到 4 一致）。附带一条重要更正：需求方报「换文档后红字没残留」，与复审预测的 M-8 相反 —— 真相是**她重新搜索了**，而 `search.tsx` 的 `busy` 那格与 `<SearchBundleView>` 互斥渲染 ⇒ 整棵结果树卸载、`notice` 归零；控制方随后用 CDP 走「同一结果集内换文档」这条路径，**M-8 复现成功**（详见缺口表 M-8 那一行与坑 80/83）。

已登记的有名缺口与归属（**都不阻塞，且都已写明为什么不在本轮补**）：

| 项 | 归属 | 现状 |
|---|---|---|
| **`doc_preview.rs:7-8` whole-block 假话**（T6 fix round 1 复审 I-1）：「预览跑在 IPC 命令线程上…就**当场终止应用进程**」 | **M4 终审（不开 round 2，复审明确建议落终审）** | `async` 落地后两半都失效：命令体现在在 `async_runtime::spawn` 出去的 tokio worker 上（前半假）。复审还把**症状改强**（控制方复核成立）：若 `panic_to_err` 的 `catch_unwind` 被摘掉，sync 下 panic 穿过消息泵触发 abort，而 **async 下穿过 tokio 任务边界不 abort、进程照跑、`return_result` 永不执行 ⇒ 前端那次 `await invoke("doc_preview")` 永远挂着**（`Cargo.toml:45-48` 明令禁 `panic = "abort"`；`grep -rn "panic::set_hook\|set_hook" src/` 零命中 ⇒ 界面无提示）。终审逐字替换文本见 §4.3 末条 |
| **`lib.rs:<数字>` 行号指针当下就指错**（T6 复审 M-1，控制方实时核对后比复审读数更差一档）| **M4 终审**（跨三文件，其中两个在本轮 Files 之外）| `index_store.rs:275`、`index_store.rs:671`、`search.rs:21` 三处都写 `lib.rs:542`，而 542 今天是**空行**：`:543` 是 `search_docs` 的头注释、`:545` 是 `fn search_docs`、它的 clamp 实体在 `:551`；离 542 最近的代码 `:538` 属于**另一条命令** `index_docs` 的 clamp ⇒ 不是「脆」而是已错。改法与清扫命令在 §4.3 末条 |
| **`index_overview` 内两句注释互相矛盾**（T6 复审 M-3）| **M4 终审**（成对改写，零行为）| `lib.rs:464-468` 说「`db_status` / `project_list` / … **全排在它后面**」、`:481-483` 说「别的 IPC **进不来**」。两句各对一个时段都对（取锁块 `:469-479` 期间命令排队；出锁探测块 `:483-496` 期间占的是消息泵），但没人能从相邻的注释里重建这个区分，后写的 T6 那半句还会把实现者拉回去给 `index_overview` 也补 `async` |
| 簇序第三级 `id` 键无测试可红 | M5 | 注释已说实话，取证条目写成「预期不红」。补法要么给 `projects.id` 开测试注入点，要么在测试里裸 SQL 固定 id |
| 台账侧 `MAX_CLUSTERS_PER_SECTION` 截断无独立用例（要 21 个有台账命中的项目） | M4 终审按需 | 第 11 条已证明 `ledger_rows` 真喂进 `cluster_by_project`；未证的只剩台账侧簇数计数，与正文侧共用 `search.rs:292-295` 同一段代码，风险窄 |
| `⋯` 字面量本身无独立测试（`[ ]` 由 `:520` 接住） | M4 终审 | 唯一守护者是新写进 `tokenize.rs:120` 的「来源 = `index_store::SELECT_SQL` 的 `snippet()` 实参」注释 |
| `MAX_QUERY_CHARS` 下边界（`>` 改 `>=`）无守护 | 已裁定不做 | 上侧被拒由 T4 的 129 字 `invalid_input` 用例钉住；补它要再动整条链 +1，代价大于收益 |
| `indexed_projects` 每次查询一次 `count(DISTINCT project_id)`（索引 `ix_index_docs_project` 对这个形状不帮忙） | T9 真机出成本证据 | spec §三 定死的字段，量级不达标再谈缓存 |
| 纯静默拒绝 WAL 那条分支（自 M0 起）本机不可构造 | 无测试守护，有名记录 | 见 `rusqlite` 那条坑（§5 第 1 条） |
| `lower_first` 折叠展开型映射（ß/İ）本机无夹具 | 无测试守护，有名记录 | 不可构造，不是漏写 |
| 预览 `text` 载荷**没有长度上界**（并窗链式合并可把一扇窗拉到接近整篇；`MAX_PREVIEW_WINDOWS` 封的是窗数不是窗长） | T9 真机观感 + M4 终审 | T5 复审 Minor-1。裁度是**接受**：加长度上限就得在超限处断开，而断开处必然相接或重叠，正是 Important-1 修掉的「正文吐两遍 + 假省略号」失效形态。判据写在 T9 Step 2 第 7 条（`text` > 6 万码元**且**明显卡顿才升级为终审 finding） |
| 重叠带（间距 4000–8000 char）的「重复段落」症状无**专用**夹具 | M4 终审（复审裁定不阻塞 T5） | 并窗判据本身有守护（`near_hits…` 的 8002 边界夹具 + 两格变异都红），缺的是症状的直接断言。复审给的结构性理由：条数链 14/134 与 Important-2 裁定「断言文本/测试名/`body` 签名全部不动」把它裁在 T5 之外，且加带内夹具不多抓一类变异 |
| `async` 的**运行时兑现**（预览大文件期间别的 IPC 是否真进得来）无单测覆盖 | T9 Step 2 第 8 条（唯一证据）+ 已写进 T9 Step 4 的「没能自动验证」清单 | T6 只能在编译侧证明这个签名在 async 形态下成立（`State<'_, AppState>` 进 `spawn` 的 future 要求 `Send`，形状不对编不过）。已在 T9 写成可判据的并发对账：`db_status` 先回 = 生效；被推到 `doc_preview` 之后 = 属性没起作用 ⇒ 终审独立 finding |
| `search_all` 仍是 sync：5 万行下若 >800 ms，它每次键入都在冻消息泵 | T9 实测出数字，终审落地 | 本轮不动它有明确理由：除 `doc_preview` 外其余 **40 条命令都是 sync** 是成规模的既有口径，不在一个接线任务里顺手改；判据与「不在 T9 改代码、只登记」都写死在 T9 Step 3 新增的第三段 |
| `db.rs:30` 的 `#[allow(dead_code)]`（M1 给 `open_in_memory` 开的）| **永久豁免**，或将来给它开非测试入口时一并回收 | T6 那条 grep 门只扫 `search.rs`/`doc_preview.rs` 两个文件，全仓这一处不在门内。T6 复审 Minor-3 要求把「门只扫两个文件」写进 Expected 叙事，已落；不写明就会有人把「一条门 = 豁免账收干净」读成全仓成立 |
| ~~`index_store.rs:671` 的 `lib.rs:542` 这类**行号指针**每长一次漂一次~~ → **已升级为上一行的「当下就指错」** | M4 终审统一换符号指针 | T6 首轮复审判的是「仍落在想命名那一格内、不是假话但不脆」；fix round 1 之后控制方按「别转述」的要求实时重核，读数更差一档（542 = 空行），所以这一格的归属不变、**严重度描述变了** —— 见本表第 2 行。教训照抄进 §5.G 第 67 条：**复审给的行号读数同样要当场 grep** |
| `DocPreview` 的线格式**没有 Rust 侧 wire 测试**（`search.rs` 有 `bundle_wire_format_is_camel_case` 断言键集合恰好 7；`doc_preview.rs` 里当场 grep 无 `serde_json::to_value`） | M4 终审 | T7 评审 M-1 的副产物。补它 = `134 → 135` 且动 Task 5 已定稿的模块条数与整条链，而 T7 不是它的归属任务；中间由 T9 真机 `invoke("doc_preview")` 的实际 JSON 读数兜住。**更要紧的是这条坑的母题**：计划原文一度写「`npm run build` 会抓到线格式对不上」，那是假门禁（`invoke<T>` 是断言不是校验，前后端无编译期耦合），已改写 —— 引它去解释为什么 TS 侧改字段名不会红 |
| `search-order.ts` 的 value-import 门禁（`/^import\b/` 逐行扫）**漏** `export { x } from "y"` / `export * from` 再导出与缩进的 import 行 | M4 终审按需，本轮**不加固** | T7 评审 R-4：漏网形态里确实有能让 `node --test` 当场炸而 tsc 全绿的一种（`export … from "@/lib/api"`）。不加固的理由：交付面只有一行 `import type`，改强判据后需要新夹具才能把它跑红，等于把一个已被变异证实可红的门换成一个没人证明可红的门。现状 = 已用 `@/lib/ipc`（红在加载期 `ERR_MODULE_NOT_FOUND`）与 `node:fs`（红在第 8 条断言）两格分别证明「门禁会咬」 |
| `isNewest` 的 `mine > latest` 形态零守护（`>=` 与 `==` 在可达输入上不可区分） | 已裁定**不做**，R-2 复核后维持 | 不变量 `latest ≥ mine` 由 `local-search.ts:17/27/44` 的结构保证（自增后比较）；要守一个不可能发生的形态得先造出注入点。方向性有守护：`<=` / `!==` / `return true` 三种变异都红 |
| ~~T8 落地后 `searchLocal` 必然**零调用方**（今天唯一调用方 `local-search.ts:34` 正是 T8 要迁走的那格），而 T8 的 Files 清单没有删它这一项~~ → **已在 T8 收口：wrapper 删掉**，回收门 `grep -rn "searchLocal" src/` exit=1 | ~~T8 派发前预检裁定~~ **已闭合**（`40f4ca3`） | T7 评审 I-2 → T8 预检 D-3 裁定为「删，不留第三种读法」：删 wrapper + 删 `searchAll` 之上那句变假话的注释 + 删 `api.ts` import 里的 `FieldHit`（不删就 `TS6196`）。**Rust 的 `search_local` 命令与 41 条 handler 一字未动**，T9 要两侧对照就 CDP 直接 `invoke("search_local", …)`。`src/lib/api.ts` 已按计划补进 T8 的 Files 与 `git add`（坑 56） |
| 预览的**极端单文档成本 + 没有取消入口**（T9 第 7 格；简报的升级判据字面不成立，议题已按证据重写） | **M4 终审** | 真机读数：对抗夹具（3.4 MB / 96 万字符 / 命中间隔 16 字）下 `doc_preview` 两次 **257,983 / 266,093 ms**，`text.length = 1,139,999` 码元、`ranges.length = 60,000`、**`truncated = false`**（`MAX_PREVIEW_WINDOWS` 封窗数不封窗长，链式合并把整篇并成一扇）。但**渲染侧不卡**：`longTasks` 全程为空、rAF 1 ms、20 万次同步循环 2 ms ⇒ 简报那句「> 6 万码元 **且** 明显卡顿」的「且」不成立。真正的问题是等 160–266 秒且对话框只有「关闭」——关掉只卸载前端，后端那次抽取照跑完。**终审别照着「渲染卡顿」去给合并加长度上限**（那会重造 Task 5 Important-1 修掉的相接窗失效形态，见坑 84） |
| `search_all` 仍是 sync：真机证明它执行期间 `db_status` 被顶到后面（**但写死的判据没满足，本轮没改**） | **M4 终审裁定**（读数已入计划 Task 9 Step 3） | 并发读数：`searchAllMs = 612`、`dbStatusMs = 620`、`dbReturnedFirst = **false**` ⇒ 首屏每次键入短冻消息泵约 0.6–0.8 s；而计划写死的升级条件是「> 800 ms **且** `db_status` 被推到后面」，稳态 675–737 ms 没越过 800 ⇒ **不加**。终审议题：值不值得换 `async`；换之前要记住 `doc_preview` 那格的教训 —— 加了之后同类命令可能乱序返回，前端 `seq`/`isNewest` 守卫就从「防御性」变成「正在承重」（那条已有真机证据：`db_status` 3 ms 先回 vs 慢预览 266,093 ms） |
| D6 上限已按实测上调为 30/15/15，但**这不算解决问题**：最坏一簇 `hidden = 190`，15 条照样触顶 | **M5**（簇内分页/展开那次 deferred IPC，spec §十） | 49,003 行实测：五个常用词查询返回的簇 100% 触顶（稀疏对照 `capped = 0` 证明指标不是夹具假象）。上调只是把线往前挪，**真正的解法是再发一次带 offset 的 IPC**，而 offset 会和「段内聚簇」纠缠，本轮明确不做。`truncated`/`hidden` 的语义一字未放宽（「只相对本次取到的样本，会低报」仍然成立） |
| `toAppError` 的兜底分支对 JS `Error` 会显示成「操作失败：{}」（T8 fix round 1 实现者 concern 2 → 控制方读 `ipc.ts` 复核为真，D-11；**10-09 真机给了一条降级证据**：沙盒里 `revealItemInDir` 失败时回来的 rejection 是**字符串** `path doesn't exist` ⇒ 走的是 `typeof e === "string"` 那支，本轮没触发 `{}`；风险仍在（JS 侧自己抛的 `Error` 会踩到），一行修法照改） | **M4 终审**（共享函数本体的一格，改它超出 T8 的 Files 面） | `src/lib/ipc.ts::toAppError` 的 else 分支是 `typeof e === "string" ? e : JSON.stringify(e)`，而 JS `Error` 的 `message` 是**不可枚举自有属性** ⇒ `JSON.stringify(new Error("x"))` = `"{}"`。**逐字修法（终审一次改完，所有调用方同时受益）**：那一格改成 `typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e)`。不在 T8 做的三条理由：① 修法在共享函数、不是调用方各加守卫；② `ipc.ts` 不在 T8 Files 面；③ plugin 命令（`revealItemInDir`/`writeText`）的 rejection 现实中多为**字符串**，已被 `typeof e === "string"` 那支接住，`{}` 需要 JS 侧 Error 才触发 ⇒ 罕见但不为零。**同形的前科**：`project-detail.tsx::guard` 用的是 `e instanceof Error ? e.message : String(e)`，反过来会把后端 AppError 形状的 rejection 弄成 `[object Object]`（M3 期遗留，一并给终审） |
| 预览对话框的 `notice` 会跨文档残留（T8 定向复审的 M-8，**10-09 已真机复现，不再是推断**） | **M4 终审**（一行级；T9 之后没有代码任务再改这个文件，见 §4.3 末） | 真机读数：`redAfterClose = []` 而 `redOnSecondDoc = ["操作失败：path doesn't exist"]` —— 同一结果集内换文档时上一份的失败红字挂着；反过来「重新搜索」会消失，原因是 `search.tsx` 的 `busy` 那格与 `<SearchBundleView>` 互斥渲染 ⇒ 整棵树卸载、`notice` 归零（需求方肉眼报的「没残留」与此一致，不是矛盾）。`useState` 挂在**总是挂载**的 `DocPreviewDialog` 上（宿主 `search-bundle.tsx` 无条件渲染它，关窗只卸载 `DialogContent`），而 effect 的两条清理分支只清 `data`/`error`、**不碰 `notice`**。**逐字修法**：effect 最顶部（`if (!docId)` 之前）加一行 `setNotice(null);`，一格同时收两条分支。代码形状来自简报 Step 2 逐字 ⇒ 属计划层缺口，实现者无过。机制：宿主 `search-bundle.tsx` 无条件渲染它，关窗只卸载 `DialogContent`），而 effect 的两条清理分支只清 `data`/`error`、**不碰 `notice`** ⇒ A 复制失败→关→开 B，B 带着 A 的红字；B 的加载窗口会同时显示「正在抽原文」+「操作失败：…」。**逐字修法**：effect 最顶部（`if (!docId)` 之前）加一行 `setNotice(null);`，一格同时收两条分支。代码形状来自简报 Step 2 逐字 ⇒ 属计划层缺口，实现者无过 |

| 报告 §8.8.4 那一格的**自我描述低估了自身覆盖度**（T7 fix round 1 复审向上纠正） | **M4 终审收报告时改写**（不阻塞） | 原文写「只调换两段且第三段为空仍靠自证」；复审的判定是：`bundleToSections` 是静态块序（无输入相关排序逻辑），整数组 `deepEqual` 在六种排列里只放过恒等式 ⇒ 三段同现**传递性覆盖**全部两段同现子序。方向良性（不是漏守护），所以只改文本、不加测试、不开轮 |

旧交接文档里的三个尾巴，现在的状态：

1. ~~todo 只建到 #35~~ → **已重建并跟进**：当前 todo 是 #1–#6，**#1–#4（T5/T6/T7/T8）completed，#5 T9 in_progress（就是欠着那两步）、#6 终审 pending**。恢复地图仍然是**账本**，不是 todo —— todo 只活在本会话，账本活在 git 之外但要按节名去读。
2. ~~T2–T9 简报未生成~~ → **已全部生成**（T1–T9 九份都在，T9 那份 **96 行**，是按 10-09 预检修订后重切的）。
3. **归属口径不一致（这条值得优先记住）**：`docs/开发进度.md` 的 M3 已知缺口第 6–11 条（`107-112` 行）以及 `125` 行写「归 M4」，但 spec §十 已把同一批（`m-3` 措辞、`m-4` emit 重复终态、`m-5` 外层 `report_failure` 无再上兜底、`m-6` 裸字符串比较判改指、`set_root_dir` 的 `DELETE`+`INSERT` 窄竞态、`ON DELETE CASCADE` 另一半）收回 **M5 / 待真机**，M4 计划里**没有**这些活。另外该文件 `13` 行的 M4 状态还写着「未开始」。**T9 文档收口时必须一起改**，否则这笔账会在两个文档之间反复弹。

---

## 4. 下一步计划

### 4.1 恢复入口（照抄即可）

```bash
cd /e/zero/demo/2026/project-files-manage
git branch --show-current          # 应为 feat/m4-unified-search
git log --oneline -4 | cat         # cd04f33 docs(T9 读数落计划+spec) / ef49c1a fix(D6 30/15/15) / 8ebdb92 docs(T9 预检) / 62af638 docs(本文件 T8 版)
cd src-tauri && cargo test --lib   # 应为 134 passed / 0 failed
node --test "tests/**/*.test.ts"   # 仓库根跑；应为 ℹ tests 8 / pass 8 / fail 0
npm run build                      # 应为 exit=0
tail -20 ../.superpowers/sdd/2026-10-07-m4-unified-search/progress.md   # 账本最后一节 = Task 9 真机 + D6 落地（写明「ef49c1a 尚未评审、Step 4 未做」）
```

**账本的恢复判据这次不适用**： usual 规则是「第一个没有 `Task N: complete` 的任务就是下一个要派的」，而 T9 已经有读数、有半个代码后果 —— **别重跑真机**（沙盒已删、211 MB 语料已清，重跑一次要四十多分钟）。按 §3 那两步收尾：

1. **给 `ef49c1a` 补任务评审**：`review-package PLAN_FILE 8ebdb92 ef49c1a`（BASE 就是派发前的 HEAD `8ebdb92`，范围里只有那一个代码提交，docs 天然在外）。评审要带的三个重点：① 三条截断测试的**测试名与夹具规模是否一起改了**（本仓硬规矩：名字里嵌的数字也算断言）；② `cluster_by_project` 上方那句**按测试函数名点名证据**的注释是否同步（`21st`→`31st`）与 `"p20"`→`"p30"` 这类夹具字面量；③ 实现者交上来的那条可证伪探针（把 `MAX_ITEMS_PER_CLUSTER` 改回 17 ⇒ 那条测试必须红）是否被复现 —— 这是本会话**第一次由实现者主动做变异探针**，值得独立复核而不是照抄。
2. **做 Step 4 的文档收口**：`docs/开发进度.md` 的 M4 行 `:13` 从「未开始」改成完成、写终态条数（**cargo 134 / node pass 8**）、两条命令名、三个上限的实测裁定（含「簇级 20 本轮无证据」与「15 也照样触顶，真正的解法是 deferred 分页」这两条诚实附注）、`search_all` sync 占消息泵的读数（612 vs 620 ms，判据不成立所以本轮没加 `async`）、以及 §4.5 末那条「没能自动验证」清单；同时把 §3 尾巴第 3 条那笔归属账（M3 缺口 6–11 条 + `:125` 写「归 M4」而 spec §十 已收回 M5/待真机）一并改掉。**这一步会同时勾掉计划 Task 9 的 Step 4 复选框**（现仍 `[ ]`）。

**注意 T9 的两个特例**（派发/评审时都要说明）：① T9 的真机部分**由控制方亲自执行**（红线第 3 条禁子代理跑 `tauri dev` / 浏览器 / CDP），所以有两份报告 —— `t9-report.md`（控制方，§五/§六 是占位）与 `task-9-report.md`（实现者的 D6 代码部分）；② 三个上限的改动**由计划 Task 9 Step 3 的 Files 面明确允许**（`src-tauri/src/search.rs`），不是越界。

助手脚本（都在 SDD skill 目录下）：`scripts/task-brief PLAN_FILE N`（切简报）、`scripts/review-package PLAN_FILE BASE HEAD`（评审包，BASE 必须是**派发前**记的 HEAD，绝不 `HEAD~1`）、`scripts/sdd-workspace PLAN_FILE`（工作目录）。

### 4.2 剩下的任务 + 终审（T1–T8 已 complete，下一个是 T9 真机，需要需求方点三下）

| 任务 | 动什么 | 该任务终态 |
|---|---|---|
| ~~T5~~ **已完成** | 新模块 `doc_preview.rs`：重抽原文 + 窗口拼接 + UTF-16 码元区间；`lib.rs` 的 `mod doc_preview;`；`Cargo.toml:46` 与 `extract.rs:577` 两处悬空注释；一块 T6 回收的 `#![allow(dead_code)]`。fix round 1 把并窗判据从「命中起点」改成「新窗左沿」（Important-1）并把 `near_hits…` 的夹具按半宽重算（Important-2） | **134**（doc_preview 14），提交 `614f1a2` + `d544175` |
| ~~T6~~ **已完成**（`6bc6354` + `077c275`） | 两条 IPC 命令接线（`search_all` 锁内查库；`doc_preview` **锁内查库、锁外抽盘**，刻意不写成一条持锁调用）；回收 `search.rs` 与 `doc_preview.rs` 两块豁免；两处注释/字符串级残留（`PREVIEW_WINDOW_CHARS` 的 6 行 doc 注释、`extract.rs:603` 的 `crate::panic_to_err`）→ 首轮 Files 与 `git add` 是**五个**文件。**fix round 1 的 delta 三件**：`doc_preview` 加 `#[tauri::command(async)]`、`lib.rs:481-482` 那两行注释改为说实话、`doc_preview.rs:9-10` 删掉一个多余空行，**只提交两个文件**（详见 §4.3） | **134**，handler 39 → **41**，门禁六道（首轮的 2 条 exit=1 豁免回收、1 条悬空符号，加本轮的悬空符号两形态与 `tauri::command(async)` 恰好 1 行） |
| ~~T7~~ **已完成**（`23b8b53` + `034b0ef`） | 前端类型 `src/types/search.ts`（4 个线格式类型）+ `api.ts` 的 `searchAll`/`docPreview` + 纯逻辑 `src/lib/search-order.ts`（刻意只 `import type`）+ 仓库根 `tests/search-order.test.ts` + `package.json` 的 `test` 脚本。**fix round 1 的 delta 三件已全落**：① 第 3 条用例补「三段同现 + `relaxed: true`」夹具（钉段序 + 三格各自的 `note`），并按新加的第 6 条变异取证（交换 ledger/docs 两个 `if` 块 ⇒ `pass 7 / fail 1`）；② `api.ts` 那句注释换成时间锚；③ 报告 §2.2 那句「tsc 会抓到线格式」按假门禁改写，并把 `DocPreview` 无 wire 测试写进「没能自动验证」清单 | Rust 仍 **134**、`node --test` **`ℹ tests 8 / pass 8 / fail 0` / exit 0**、`npm run build` exit 0（`error TS` 计数 0）、`grep -rn "\"search_all\"\|\"doc_preview\"" src/lib/api.ts` **exit=0**（这条把 §4.3 第 6 点两侧对账闭环）。整改轮**恰好 2 文件**，`search-order.ts` / `types/search.ts` / `package.json` / `package-lock.json` / `tsconfig.json` / `src-tauri` 全部 `git diff --exit-code 23b8b53 034b0ef` = **0**（实现体零漂移 ⇒ 「测试能红只靠夹具」这条红线成立） |
| ~~T8~~ **已完成**（`40f4ca3` + `0757545`） | Files 面（计划 `### Task 8`，起点 `:2021`）**六个文件**（fix round 的那三个文件是它的子集）：`src/stores/local-search.ts`（`hits` → `bundle`，三处内联 `mine === seq` 改走 `isNewest(mine, seq)`，并删掉写了没人读的 `query` 字段）、`src/lib/api.ts`（删 `searchLocal` wrapper + 那句变假话的注释 + import 里的 `FieldHit`）、`src/pages/search.tsx`（收成薄壳：输入框 + debounce + 三种空态 + `<SearchBundleView>`）、**新建** `src/components/search-bundle.tsx`、**新建** `src/components/doc-preview-dialog.tsx`（含 fix round 补的 `guard` + 独立 `notice` 态）、`src/pages/index-status.tsx`（「试搜正文」Card 加一句定位说明）。**本任务零新增单测**（JSX 不在这张网里，spec §八），报告 §七 逐条点名 9 项「没能自动验证」交 T9 | 首轮恰好 6 文件 + fix round 恰好 3 文件；`npm run build` exit 0、node 侧仍 **`pass 8`**、Rust 仍 **134**（零 Rust 改动）、T8 的删除门 `grep -rn "searchLocal" src/` **exit=1**、密文列扫描 `grep -rn "_cipher\|_nonce" src/` **exit=1** |
| **T9（做了一半，欠两步）** | 真机验收（**三下只有人能点的已经点完**：原生目录选择器、`revealItemInDir`、`window.confirm`）+ D6 上限实测 + `async` 并发对账 —— 全部拿到读数；D6 的代码后果 `ef49c1a` 已落。**欠：`ef49c1a` 的任务评审（评审包还没生成）+ Step 4 的文档收口**（`docs/开发进度.md` M4 行、没能自动验证清单、归属口径那笔账） | 见 §3 那两步与 §4.1；读数与附注已写进计划 Task 9 Step 1/2/3 正文 |
| 终审 | 整分支 code review（最重的一次），一次性整改 + 一次定向复审。逐字动作清单在 §4.3（T6 三条 + T8 五条），10-09 又添了 §5 末列的几条 | 134 全链 |

计划正文在执行期间不改；若评审裁定某任务**文本本身**写错，走「改计划本体 → 重新生成该任务简报 → 账本记 `Ruling:` → 派发 fix round」的闭环（10-08 之前 T3/T4/T5/T6 各走过一次，**T7 一天内两次**：派发前预检 4 处 + 首轮评审 3 处；**T8 也是两次**：派发前预检 4 处（含一条把假联合拆掉的简化）+ 首轮评审 4 处），不要为此多开一轮评审，也不要让实现者自己发挥。**连续两轮预检都抓到同一形缺陷：简报逐字给的代码里带着「写了没人读」的声明/导入，而它自己的 `npm run build` 门会因此红**（T7 的 `FieldHit`、T8 的 `as HitSource` + `busy`）—— 预检时必须把每个新增声明问一遍「谁读它」。

### 4.3 T6：首轮六处不可省的活（都已在 `6bc6354` 落地）+ fix round 1 的三件 delta

首轮六处（留作对账，不要重做）：

1. **新增的 Step 4**：T4 定向复审留下的六条逐字替换块（4 条文本级 + 1 条旧任务号指针 + 1 条测试强度）——含把键集合断言补成 `assert_eq!(obj.len(), 7, ...)`，配一条证伪取证：临时往 `SearchBundle` 加一个 `pub docs_capped: bool` 这条**必须红**，然后**删干净**。这道闸是给「M4 不加线格式字段」那条裁定上的，不是装饰。（计划原文这条标题写的「四处」是简报自己的计数错，六条里 5 条落 `search.rs`、第 5 条落 `index_store.rs`；实现者当时读成「四处 = 1–4」并把六条全落地，是唯一安全读法 —— 坑 63。）
2. **回收两块临时豁免**：删掉 `unified_bundle` 上方的 `#[allow(dead_code)]`（按「紧贴 `pub fn unified_bundle` 的那行」定位，行号会漂），**以及 `doc_preview.rs` 文件头那块 5 行的 `#![allow(dead_code)]`**（Task 5 按预检修订开的，形态同形）。门禁 `grep -rn "allow(dead_code)" src/search.rs src/doc_preview.rs` 期望 **exit=1（0 命中）**，一次扫两个文件。**覆盖面只有这两个文件**：全仓还剩 `db.rs:30` 一处 M1 遗留的永久豁免，不归本任务、不许顺手删（见 §3 的缺口表）。
3. **同一个 `doc_preview.rs` 里还要同步一句 doc 注释**（T5 复审 Minor-1）：`PREVIEW_WINDOW_CHARS` 上方那句「单窗最长…」在并窗判据换形后成了假话（链式合并能把一扇窗拉到接近整篇），整段换成计划 Task 5 Step 1 里那份 6 行写法，**逐字照抄、不要自己重述**。
4. **`extract.rs` 多一行字符串**（T5 复审登记的越界残留）：`release_profile_does_not_abort_so_panic_guards_work` 里 `assert_ne!` 的尾句把 `crate::panic_to_err` 改成裸 `panic_to_err`（`f72a5c8` 把边界助手搬进 `extract.rs` 后这个路径根本不存在）。纯字符串，不许动那个函数体里任何其他表达式。
5. **Step 3 的门：首轮从四条变五条，本轮再变六条**。第五条扫悬空符号两种形态 `grep -rn "crate::panic_to_err\|index_job::panic_to_err" . --include=*.rs --include=*.toml` 期望 **exit=1**（T5 的门只扫 `index_job::` 前缀，所以 `crate::` 那种写法一路绿到今天 —— 坑 60）。第六条钉住本轮裁定的**范围**：`grep -n "tauri::command(async)" src/lib.rs` 期望**恰好 1 行且 exit=0**，紧贴 `fn doc_preview` —— 漏加 = 0 行 = 红，给 `search_all` 也顺手加 = 2 行 = 红，因为「只动 `doc_preview`」是裁定不是随手选择，而编译器和 clippy 都不替你看着这两个方向。
6. Files 里已点名**五个**文件（`lib.rs` / `search.rs` / `doc_preview.rs` / `index_store.rs` / `extract.rs`，后四者只动注释、那一处断言、那 5 行豁免与那 1 行字符串），`git add` 也是这五个 —— 别让实现者以为无权改它们。Step 5 的字面量对账：`grep -rn "\"search_all\"\|\"doc_preview\"" ../src/lib/api.ts` 在 **T7 落地前必然 exit=1**，T7 之后必须 exit=0 —— T7 要重跑这条并写进它的 Expected。

**fix round 1 的 delta 只有三件**（评审裁定 → 改计划本体 → 重切简报 167 行之后剩下的活；终态是 134 passed / handler 41，**提交只含 `lib.rs` 与 `doc_preview.rs` 两个文件** —— 首轮那五个文件里的另外三个本轮零改动）：

- `lib.rs` 的 `doc_preview` 命令属性 `#[tauri::command(…)]` 补上 `async`，并按修订后的 Step 1 逐字同步它上面那句注释（「光出锁还不够…」）。理由与三条源码出处、以及「为什么 `search_all` 本轮不加」都在计划 Task 6 Step 1 下面那四段里，不要自己重述。
- `lib.rs:481`（`index_overview` 的「出锁段：`is_dir()` 可以慢，但此时别的 IPC 进得来。」）整行换成计划里给的那两句。它是 M3 落地的 sync 命令写的假话，与本任务 Important-1 同源，零行为改动；`index_overview` 的函数体与那段带「Task 10 评审的 Important 1」的旧任务号注释都不许动。
- `doc_preview.rs:9-10` 两个连续空行删掉一个。这是简报 Files 那句「前后各留一个空行」逐字执行的结果（**计划自己的措辞缺陷**，已改写成按目标形态下指令），本仓没有 rustfmt 闸门，不留给 M5。

**T6 复审留给终审的三条，逐字替换文本在这里（终审只有一次 fix 派发，别再让实现自己去推导）**：

1. **`doc_preview.rs:7-8`**（I-1，本轮 `async` 落地后成为假话）。现状文本是「…预览跑在 IPC 命令线程上，\n没有边界的话，一份畸形 docx 被用户在首屏点开就当场终止应用进程。」整行 `:7-8` 换为：

   ```rust
   //! 2. **抽取必须包在 panic 边界里**（`extract::panic_to_err`）。预览自 T6 起是 `async` 形态，命令体跑在
   //!    `async_runtime::spawn` 出去的 tokio worker 上：没有这道边界时 panic 不会终止进程，而是让那一次
   //!    `invoke` 永不返回 —— `ipc/mod.rs:375-388` 的 `return_result` 在 panic 之后根本走不到，
   //!    前端那次 `await` 一直挂着，而本仓没有 `panic::set_hook`，界面上连一条提示都没有。
   ```

   （证据：`tauri-2.12.0/src/ipc/mod.rs:371/:375/:380/:477` —— `return_result` 在同一个 spawned future 内、排在 `task.await` 之后；`Cargo.toml:45-48` 明令禁 `panic = "abort"`；`grep -rn "panic::set_hook\|set_hook" src/` 零命中。）

   **同批核对过、确认不需要跟着改的两处**（避免终审时有人以为漏了）：`extract.rs:575-581` 那条 `release_profile_does_not_abort_so_panic_guards_work` 的头注释和计划 Task 2 Step 5 的「摘掉 `catch_unwind` ⇒ 测试线程 panic 也算红」取证条目，讲的分别是 **release 下 `panic = "abort"`** 和**测试线程**这两个场景，都不依赖「命令体跑在哪条线程」，`async` 改不动它们。本条只改 `doc_preview.rs:7-8` 那句，**`panic_to_err` 边界本身一字未动**。
2. **三处 `lib.rs:542` 行号指针**（M-1，`index_store.rs:275`、`index_store.rs:671`、`search.rs:21`）。统一改为**符号指针**，例如「`lib.rs` 的 `fn search_docs`（它的 clamp 在命令体内，上界 200）」；同批跑一次清扫 `grep -rn "lib\.rs:[0-9]" src/`，本仓现存 3 处命中就是上面这三处，`extract.rs:31` 那条指向的是依赖包内文件、**不属本仓指针、不许改**。
3. **`index_overview` 内那对互相矛盾的注释**（M-3，`lib.rs:464-468` 与 `:481-483`）。事实是两个时段：取锁块 `:469-479` 期间别的命令排在 `Mutex<Connection>` 后面（`:465-466` 那句对此为真）；出锁探测块 `:483-496` 期间命令体仍占着消息泵（`:482` 那句对此为真）。**成对改写**：`:464-466` 补限定「排队的时段是上面那块锁」，`:482` 精确成「排在 `:469-479` 那块锁后面的命令同样进不来」。只改其中一句都会留下半句假话，也别顺手给 `index_overview` 加 `async`（40 条 sync 是既有口径）。

**T8 这一轮另留给终审的五条**（同样是逐字动作，终审只有那一次 fix 派发，别再让实现者自己推导）：

1. **`doc-preview-dialog.tsx` 的 `notice` 跨文档残留（M-8）**：在 `DocPreviewDialog` 的 `useEffect` 最顶部（`if (!docId)` 之前）加一行 `setNotice(null);`。为什么在这一格：`useState` 挂在**总是挂载**的组件上（宿主 `search-bundle.tsx` 无条件渲染它，关窗只卸载 `DialogContent`），而 effect 的两条清理分支原本只清 `data`/`error`。现状后果：A 复制失败 → 关 → 开 B，B 带着 A 的红字；B 的加载窗口会「正在抽原文」+「操作失败：…」同屏。**代码形状来自简报 Step 2 逐字** ⇒ 计划层缺口，实现者无过。
2. **`src/lib/ipc.ts::toAppError` 的兜底分支对 JS `Error` 显示成 `{}`（D-11）**：那一格 `typeof e === "string" ? e : JSON.stringify(e)` 改成 `typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e)`（`Error.message` 不可枚举 ⇒ `JSON.stringify(new Error("x"))` 是 `"{}"`）。**同批一起看**反向形态：`src/pages/project-detail.tsx::guard` 用的是 `e instanceof Error ? e.message : String(e)`，遇到后端 AppError 那种**普通对象**会显示成 `[object Object]` —— 两个方向的兜底各只覆盖一半，终审一次收干净（共享函数改一处，所有调用方同时受益；不要在各调用点加守卫）。
3. **`SOURCE_ORDER` 自 T8 起零消费方（M-4）**：现在只有 `types/search.ts` 里那句「M4 要把文件正文结果插进来时改这里」的注释还指着它。终审裁定删还是留（它在 M3 期是既有导出，删它要确认没有别处 import）。
4. **预览的两句措辞不精确（M-5）**：`doc-preview-dialog.tsx` 里「只显示命中附近的窗口，中间以 ⋯ 分隔」在**单窗**时不成立（一扇窗没有分隔符），而「正文里没有这个词，命中的是文件名」除了文件名命中之外还有 `lower_first` 折叠这一成因。纯文案，零行为。
5. **`search.rs:26` 那句「交给 Task 8 文案」的诚实性补偿没落地（M-7）**：T8 的段级文案最终形态是 `s.note`（见 §2.2 的 T8 行），没有覆盖那条评论所指的东西。动它要碰被 3 条 Rust 测试钉着的注释 ⇒ 会重开 Task 4 的条数链，所以**明确不在 T8 做**，终审统一处置。

**T9 这一轮（真机 + D6）另留给终审的六条**：

1. **`search_all` 是否也加 `#[tauri::command(async)]`**：真机读数 —— 并发「`search_all` + `db_status`」⇒ `searchAllMs = 612`、`dbStatusMs = 620`、**`dbReturnedFirst = false`** ⇒ `db_status` 被顶到后面，**sync 的 `search_all` 确实在占消息泵**（对照：async 的 `doc_preview` 期间 `db_status` 3 ms 就回）。但写死的判据是「`search_all` > 800 ms **且** `db_status` 被推到后面」，稳态 675–737 ms 没越过 800 ⇒ **判据不成立，本轮没加**。终审议题写成「首屏每次键入都短冻消息泵约 0.6–0.8 s，值不值得换 `async`」——注意换了之后前端 `seq` 守卫就从「防御性」变成「正在承重」（这条本轮已经靠 `doc_preview` 验过一次）。
2. **预览的极端单文档成本 + 没有取消入口**（第 7 格，议题已按证据重写）：载荷 1,139,999 码元 / `ranges.length = 60,000` / `truncated = false`，而**渲染侧 `longTasks` 全程为空、rAF 1 ms ⇒ 不是卡顿**；真正不可用的是等 160–266 秒且对话框只有「关闭」——关掉只卸载前端，后端那次抽取照跑完。**别照着「渲染卡顿」这个错议题去给合并加长度上限**（那会重新制造 Task 5 Important-1 已修掉的相接窗失效形态）。
3. **首屏输入框 placeholder 仍是 M3 那句**「搜项目、网址、账号用途、备注…」，而 `search_all` 现在带回三段、正文段是本轮的主体。一行文案，落终审（`src/pages/search.tsx`，不在 T8/T9 的 Files 面）。
4. **另两条截断测试没做变异探针**：`ef49c1a` 里实现者只对 `MAX_ITEMS_PER_CLUSTER` 做了「改回 17 ⇒ 必须红」的取证（结果红，非零守护）；`MAX_CLUSTERS_PER_SECTION` 与 `MAX_PROJECT_HITS` 那两条测试没做同等探针 ⇒ 终审可补，或照本仓口径登记为「未证明可红」。
5. **`lib.rs:542` 那组行号指针的第四处出处**：`search.rs` 注释里的 `lib.rs:542` 漂了（clamp 实际在 `src-tauri/src/lib.rs:551`，**值 200 仍与注释一致 ⇒ 是行号漂不是假话**）。与 T6 复审 M-1 那三处（`index_store.rs:275`/`:671`、`search.rs:21`）**同批换成符号指针**，清扫命令仍是一条：`grep -rn "lib\.rs:[0-9]" src/`（`extract.rs:31` 那条指依赖包内文件，别改）。
6. **反向提示（免得终审误改）**：`src/pages/index-status.tsx` 里那个「20 个」是 `PROGRESS_EVERY` 的巧合，**不是**旧上限的残留；`grep` 到它别动。实现者已核过 `src/` 与 `src-tauri/src/` 的旧上限字面量 `grep -rn "20/10/10\|20 簇\|10 条\|10 平铺"` 双双 exit=1。

### 4.4 完成判据（计划里的原话，可逐条核对）

- `cargo test --lib` → **134 passed; 0 failed**（分模块：tokenize 9 / extract 15 / index_job 12 / index_store 15 / search 20 / doc_preview 14 / db 8 / index_scan 10 / ledger 12 / project 10 / vault 9）
- `node --test "tests/**/*.test.ts"` → **pass 8 / fail 0 / exit=0**；`npm test` 与 `npm run build` 两条 exit 0
- 两道 clippy（`--lib -- -D warnings`、`--lib --all-targets -- -D warnings`）都 exit 0 且输出里 0 条 warning
- 结构 grep 门禁（**注意方向**：期望 0 命中的那几条，正确结果是 `exit=1`，不是命令失败）：`grep -rn "chars().count() > 128" src/` → exit 1；`grep -rn "crate::panic_to_err\|index_job::panic_to_err" src/`（两种前缀都要扫，见坑 60）→ exit 1；`grep -rn "\.cut(" src/tokenize.rs` → 恰好 1 行；`grep -rn "File::open" src/doc_preview.rs` → exit 1；`grep -rn "\.cut(\|cut_for_search" src/doc_preview.rs` → exit 1；另有一条在 T6 Step 3 而不是完成判据里：`grep -rn "allow(dead_code)" src/search.rs src/doc_preview.rs` → exit 1（就是 §4.3 第 2 条那两块豁免的回收证据）；同一格还有钉裁定范围的一条：`grep -n "tauri::command(async)" src/lib.rs` → **恰好 1 行且 exit=0**（只给 `doc_preview`；0 行 = 没落实裁定，2 行 = 越界改了 `search_all`）
- `generate_handler!` 条数 = 41；**每任务 1–2 个提交**（走过 fix round 的 T3/T4/T5/T6/T7/T8 各多一个），每个只含该任务 Files 点名的文件；`git status --porcelain` 干净
- `%APPDATA%\dev.zero.pfm\` 逐文件与真机基线快照一致；`%TEMP%` 无本轮残留

### 4.5 T9 里已经预先写死的裁定（不用再问人）

**10-09 状态：这一节的五格全部跑完了，裁定照字面执行，没有一项需要再问人。** 逐格读数与结论都写在**计划 Task 9 Step 1/2/3 正文**（那三个复选框现在是 `[x]`）与账本 `## Task 9 真机 + D6 落地`，本节的规则保留作为出处：① 延迟稳态 675–737 ms 未越 800 ⇒ `DOCS_FETCH_LIMIT` **维持 200**；② 触顶簇 100%（稀疏对照 `capped=0`）⇒ **三个上限 20/10/10 → 30/15/15**（`ef49c1a`）；③ 载荷 114 万码元但渲染不卡 ⇒ 判据字面不成立，议题按证据重写后落终审（见 §4.3 第 2 条）；④ `async` 兑现成立（`db_status` 3 ms 先回 vs 266,093 ms）；⑤ `search_all` 期间 `db_status` 确实被顶到后面（620 vs 612 ms），但 800 ms 那半边不成立 ⇒ 本轮不加 `async`，登记终审。

沙盒灌 5 万行 `index_docs`（正文 1–2 KB 随机 CJK，全在 `%TEMP%`）后量两件事，只回数字：

- **一次 `search_all` 往返 > 800 ms** ⇒ `DOCS_FETCH_LIMIT` 200 → 100，并在 `docs/开发进度.md` 登记「降到 100，因为 X ms」。
- **触顶簇占比 > 30%** ⇒ `MAX_ITEMS_PER_CLUSTER = 10` 太小，三个数一起上调 20/10/10 → 30/15/15，并同步改 T4 的三条截断测试期望值与 spec §四。
- 两条都没触到就维持原值，只记数字。**不许**为了「看起来更快」偷偷放宽 `truncated`/`hidden` 的语义。
- **预览载荷长度**（T5 复审 Minor-1 的有名缺口，判据也已写死）：沙盒造一份 ≥4 万 char、命中间隔约 8000 char（刻意落进合并带）的长文，`doc_preview` 后只回 `text` 码元长度、`ranges.len()`、`truncated` 三个数字。**`text` > 6 万码元且页面渲染明显卡顿或滚动失效** ⇒ 升级为终审的独立 finding（议题：给合并加长度上限、上限处宁可多插一个 `⋯`）；不卡就记数字收口，不在 T9 里顺手改算术。
- **`async` 到底有没有兑现**（T6 Important-1 裁的那条，全链条唯一的运行时证据就在这一格）：沙盒造一份慢到可辨的文件（几百 MB 纯文本或多页 PDF，只读、跑完删），页面里**同一时刻**并发发 `invoke("doc_preview", {慢文件})` 与一条便宜的 `invoke("db_status")`，只回两个毫秒数 + 「`db_status` 是否先返回」。先回 = 属性生效；被推到 `doc_preview` 之后 = 属性没起作用 ⇒ 终审独立 finding。这条是 134 条单测一条都不覆盖的格子，跑不通就不许在收口文档里说「预览不再冻结界面」成立。
- **`search_all` 要不要也换 `async`**：同一口径顺手量它 —— 5 万行那次往返期间并发 `db_status`。若 `search_all` 已 > 800 ms **且** `db_status` 被顶到后面，就在 `docs/开发进度.md` 写明「首屏检索也在冻消息泵，`search_all` 应一并换 `async`」，交终审那一次性整改落地；**不在 T9 趁真机顺手改 Rust**，免得绕过任务评审这道闸。

收口文档必须含「没能自动验证」清单（逐条点名，不许省略）：JSX 渲染正确性、`revealItemInDir` 那一步要人点、`isNewest` 的 `>=` 变异假绿、`lower_first` 折叠展开型映射（ß/İ）在本机无夹具、纯静默拒绝 WAL 那条分支自 M0 起就无测试守护、5 万行延迟只是沙盒数字、第三级簇序键无守护（M5）、预览「重复段落」症状在重叠带内（4000–8000 char）无专用夹具（只由并窗不变量间接守）、`PREVIEW_GAP` 的字面值本身无守护（T5 复审裁定：它不是线上协议，为常量写断言等于重言式）、`#[tauri::command(async)]` 的运行时兑现（只有上面那条并发对账覆盖，编译绿不等于它成立）。

### 4.6 本轮明确不做（别顺手加）

结果分页/展开按钮（要做就得给 `search_all` 加 offset，而 offset 会和「段内聚簇」纠缠）、`docsCapped` 线格式位、失效核对写回 `missing`（M5）、`/index` 页 `scanning` 文案（M5）、路径归一化（M5）、图片 OCR（M6）、全仓旧任务号注释大扫除（实测 25 处，指的都是 **M3 计划**的 Task 10/11/12，语义不算错；本轮只改 `index_store.rs:671` 那一条，因为它同时含会被 grep 门数进去的裸字面量）。完整表在 spec §十。

### 4.7 派发纪律（今天的两个平台限制，别再假装做过）

- 本会话 `Agent` 工具**不暴露 model 参数** → 子代理一律继承会话模型，SDD 的模型分级做不到，账本里记为 inherited。
- **resume 不可用** → fix round 用**全新实现者 + 完整报告路径**（报告文件尾部有前几轮的记录），已记为 fallback。
- 同一 worktree 只允许**一个会提交的实现者**；它跑的时候控制方**不在这个 worktree 里提交任何东西**（连纯文档也不）——见 §5 第 55 条。

---

## 5. 踩过的坑，绝对不要再踩

### A. 第三方库/运行时的内部行为 —— 不实测就去读源码，别凭印象

1. **`pragma_update` 的 Ok/Err 不携带任何信息**：它内部就是 `execute_batch`，而 `execute_batch` 故意丢弃 pragma 结果行（`Error::ExecuteReturnedResults` 那条返回被 `if false` 硬关）。SQLite 拒绝切 WAL 的方式是**返回原模式**，不是报错。所以「切 WAL」必须读回 `PRAGMA journal_mode` 并断言 `wal`，且读回要写在**库层函数体内**而不是测试里。网络盘/被拦截路径就是这种静默拒绝。
2. **`bm25()` 交回的是 `-1.0 * score`，越小越好**：`ORDER BY bm25(...)` 升序才是「最相关在前」。当降序排就全反了。
3. **unicode61 默认 token 字符集是 `L* N* Co`**：标点 P* 是分隔符（不会造成假命中），符号 S*（™ € ± ★）也不是词字符；但私有使用区 Co **是**词字符而 `is_alphanumeric` 回 false —— 这是查询侧判据比索引侧唯一窄的一处，已在 `tokenize.rs` 注释里登记，别再「顺手统一」。
4. **中文全文检索的隐性失效**：unicode61 会把一整句连续中文切成一个巨型 token，`trigram` 要 3 字符而中文高频是 2 字词。表现是「界面在工作、就是搜不到，且不报任何错」。唯一防线是入库/查询同一个分词器同一套预处理 —— 这就是 `query_terms` 必须唯一切词入口的原因。
5. **FTS5 前缀：`*` 必须写在引号外面**。`"维保"*` 能命中索引里的「维保期」，`"维保*"` 里的 `*` 只是词内字面字符，永远不中。
6. **`snippet()` 是从原始列文本重建摘要的**，所以正文里保留标点才可读；而本项目摘要取的是**预分词后的正文列**，`cut_for_search` 的复合词会连着出现两次（实测「里面只有付款条件**付款条件**与验收流程」）。这不是 bug，D5 的折叠规则是针对它的，且只折「第三个 token 恰等于前两个按原序拼接」，任一 token 带高亮标记就整组不折。
7. **jieba 的索引膨胀比不是容量常数**：实测 `cut_for_search` 相对 `cut` 的词条数比从 **1.0 到 1.57** 波动，别拿单个比值做体积预算。
8. **rusqlite 取 `usize` 会编不过**：`from_sql_integral!(usize)` 在 `rusqlite-0.40.2/src/types/from_sql.rs:146`（不是 `src/types.rs`），而它**上面那行 `:145` 挂着 `#[cfg(feature = "fallible_uint")]`**，本项目 `Cargo.toml:25` 只开 `bundled` —— 所以 `count(*)` 只能 `get::<_, i64>()` 再转（`index_store::indexed_project_count` 就是这么写的，Task 4 实测撞过 `E0277`）。控制方写计划时只查到「有 `from_sql_integral!(usize)`」而没查它上面的 `cfg`，这条已作为「库 API 断言要读到那行 cfg」的教训登记。换版本前先复核第 1 条那两个源码位置。

### B. 测试与计划文本 —— 断言必须能被它守护的东西打红

9. **期望条数只能实跑数出来，不能累加**。本计划第一版链数就写错了（109/120/134），虚增的数会让实现者以为自己写多了、评审以为自己写少了。
10. **恒绿断言是缺陷不是守护**：「两处同源」这种事复制实现照样绿，写不出能红的行为断言 —— 改用文本门禁（`include_str!` + 数 `.cut(` 恰 1 次；grep `File::open`）。派发前对被守护表达式做**反值心算**。
11. **变异取证要每条实测**：本计划给 T1/T3/T4/T5/T7 都列了「把 X 改成 Y，必须让第 N 条红」，不许只做第一条就交。已登记的有名假绿：T7 里 `isNewest` 的 `>=` 变异不会被任何一条测试打红。
12. **测试里别写死会被本任务 grep 门禁判成违规的字面量**：T4 的测试用 `DOCS_FETCH_LIMIT` 而不是裸 `200`（同任务 Step 5 的门禁就是查裸值）。
13. **不要为生命周期问题发明助手**：`body` 夹具最终口径是 `fn body(text: String)` + 调用点 `.to_owned()`；`&'static str` / `impl Into<String> + 'static` 那两版都是把计划文本逼去造 `static_str()` 这种不该存在的东西。
14. `let mut x = x;` 影子重绑在 `-D warnings` 下有 `clippy::redundant_locals` 风险 —— 把 `mut` 写进 tuple pattern。
15. `Vec<&str> == Vec<String>` 在 Rust 里不成立，比较两侧类型要统一。
16. **夹具文件名会改变命中段**：M3 真机上文件叫 `维保.docx` 时，`search_docs("维保")` 回的是 `matchedBy="exact"` —— 因为 `SELECT_SQL` 的 `MATCH` 没限定列名，**文件名**里的词在精确段就够着了，前缀段根本不触发。写「前缀放宽」类用例时文件名必须避开查询词。
17. **「不该有测试」也要显式记**：本机不可构造的分支（纯静默拒绝 WAL、`lower_first` 的 ß/İ 展开）不是没写测试，是不可构造，必须在账本/文档里点名为 deferred minor，否则下一个人以为漏了。

### C. 前端与 `node --test`

18. **`node --test` 的 glob 空匹配 = exit 0 且 tests 0**（静默假绿）；传目录实参 exit 1；`--test-match` 组合 exit 9。所以门禁只认输出里的 **`pass N`**，`exit=0` 本身不构成证据。
19. **本机 PATH 上的 node 是 v26.3.0**，harness 报的 v24.18.0 不是仓库实际用的那个。版本相关判断以实跑 `node -v` 为准。
20. **`src/lib/search-order.ts` 里不许出现运行时值 import**（只 `import type`，会被类型剥离抹掉），否则 `node --test` 跑不动。测试放仓库根 `tests/`：`tsconfig.json` 的 `include` 只有 `["src"]`，且 devDependencies 里**没有 `@types/node`**。
21. **本轮不引新依赖**（前端测试网是用户明确选的「node --test + 纯逻辑抽取」）。别顺手装 vitest。
22. **`package.json` 的 `test` 脚本与 `tests/` 目录是 T7 的产出，不是前置条件**（T7 之前两者都不存在，别在 T1–T6 里以为「测试网已经在了」）。T7 落地后：`"test": "node --test \"tests/**/*.test.ts\""` 排在 `build` 之后，仓库根 `tests/search-order.test.ts` 有 8 条 —— 现在**可以**直接 `npm test`，T8/T9 的前端侧门禁就接在这一格上，不要再另起一套。

### D. 真机验证（CDP）

23. **React 受控输入直接改 `.value` 不触发状态更新**：要用 `Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set` 赋值再派发 `input` 事件；Radix Select 需要带 `pointerType:"mouse"` 的 `pointerdown` 才展开。
24. **合成事件与 `disabled` 的关系要分控件**：真 `disabled` 的原生 `<button>` 合成 click **进不来**（M3 两轮独立实测），只挂 `data-disabled`/`aria-disabled` 的 Radix 控件进得来。判据是 DOM 上那个真属性，不是文案。
25. **别在同一个 click 助手里既 `dispatchEvent(new MouseEvent("click"))` 又 `el.click()`** —— 那是点两下，M2 就是这么建出了重复项目。
26. **shadcn 的 `Field` 是 `<label>` 把 `<input>` 包在里面**，`label.parentElement.querySelector("input")` 会拿到相邻的另一个框。正确做法是先 `hit.querySelector("input,textarea")`，再退回 `parentElement`，并支持 `htmlFor`。
27. **事件残留会把你读到上一轮的终态**：进度事件往 `window.__m3prog` 里堆，发命令前不清空就会拿到上一轮结果（M3 因此把一次 `skipped=1` 读成自相矛盾）。`index_overview` 是两阶段的，页面不 remount 时 `overview` 是旧的 —— 要看新登记项目的红标必须先切一次路由再回来。
28. **`-c` 覆盖 identifier 要走 npm 的 `--`**（`npm run tauri -- dev -c '{...}'`），且**动手前先做两个必然失败的编译前探针**（`-c <不存在的文件>` 报 os error 2；`-c '{"__probe_unknown_key__":1}'` 报 `was unexpected`），零成本验掉「JSON 被三层引号咬碎、应用悄悄落到真实 identifier」这个风险。
29. **跑 `tauri dev` 时别同时跑 `cargo test`/`clippy`**：抢同一个 `target/` 会让 rustc 报内部编译错误、`cargo test` 以 101 退出，看着像内存问题其实是文件占用。先把自己起的 dev 进程树收干净再跑门。
30. **停 dev 会留孤儿**：vite 占 1520、`msedgewebview2.exe` 占 9222。重启前先按命令行特征找回来，否则报 `Port 1520 is already in use`。
31. **dev 端口 1420 本机不可用**（Windows 把 1397–1496 列为 TCP 排除端口），已改 1520，`vite.config.ts` 与 `tauri.conf.json` 的 `devUrl` 必须同步。
32. **真机断言失败是 finding，不是绕面对象**；数字变了照实报，绝不为凑数改测试。库文件名是 `ledger.db`，不是 pm.db。

### E. 环境 / Git / 工具

33. **16 GB 内存会让 rustc 报假失败**（`os error 1455` / 内部编译错误）：先重跑一次再判定是不是真错。
34. **管道会吞退出码**：`cmd | tail -5; echo $?` 取到的是 `tail` 的状态。要 `cmd; echo exit=$?`。
35. **Git Bash 路径是 `/e/zero/demo/2026/project-files-manage`**（`/e/demo/...` 不存在），而 HOME 解析到 `/c/Users/zero`，但活动 profile 在 `D:\Users\zero`。
36. **仓库源文件是 CRLF**：脚本处理要保留换行（Python 侧 `newline=''`），`git` 会刷 `LF will be replaced by CRLF` 警告，属正常，不是有文件坏了。`docs/` 下的 md 是 LF。
37. **从 Read 回显复制文本去做 Edit 可能匹配不上**：本会话实测 Read 把文件里的 `=>` 显示成 `->`，导致 `old_string` 0 命中。用 `sed -n 'X,Yp' file | cat -A` 核对真实字节再改。
38. **Git 纪律见第 1 节红线第 6 条**。另外 `.superpowers/sdd/` 自带 `.gitignore`（内容 `*`），账本和简报不会被 stage；`git status` 开头那句「工作树 cleanliness 未知」是因为仓库配了 content filter，不是有脏文件。

### F. 协作与子代理

39. **账本第一行必须写 `# SDD ledger — plan: <计划路径>`**。没有它，压缩上下文后的控制器会重跑已完成的任务序列 —— 这是被观测到的最贵的失败。恢复时信任账本 + `git log`，而不是自己的记忆。
40. **同一 worktree 只允许一个会提交的实现者**；只读评审可以并行。不要对热重载/未提交的树取验收证据。
41. **实现者绝不派生子代理**（包括「找个 reviewer 帮我看看」）；评审只从控制方来。子代理也不得擅自新建 Skill。
42. **派发词里不要粘历史**：简报路径 + 报告路径 + 接口清单 + 红线，就这些。曾有一个会话的派发词 42k 字符、其中 99% 是粘贴的前序任务总结。
43. **评审包给「代码面」并声明 diff 是地图**，评审提问固定包含一条「跨模块不变量有没有被破坏」。
44. **裁定评审发现时自己复现最便宜的那个实验**；数据生命周期类问题走成因修法，不要推到下一个里程碑只压症状；整改派发只做高价值 Minor。
45. 本会话一个平台事实：`Agent` 工具**不暴露 model 参数**，所以「为子代理显式选模型」这条做不到，一律继承会话模型 —— 已在账本口径里记为 inherited，不要假装做过分级。
46. **变异还原用的备份必须是「本轮编辑之后」的那份**。拿「开工前」备份 `cp` 回去会把自己本轮刚做的编辑一起回滚，而且除了 CR 计数（338→332）表面上完全看不出来。每次还原后 `diff -q` + 数 CR。
47. **`cargo test --lib <模块名>` 的聚合行不是模块真数**：筛选走全路径子串匹配，`search` 会连 `extract::tests::real_pdf_yields_searchable_chinese_text` 这类同名巧合一起算进去（首轮报出 22 passed，模块真数是 19）。数模块条数要从**全量**输出按 `^test <模块>::tests::` 数；注意 `-- --list` 的行首**没有** `test ` 前缀（格式是 `mod::tests::name: test`），拿 `grep "^test "` 去数 `--list` 会数出 0。
48. **`#[allow(dead_code)]` 这类临时豁免必须在本里程碑内被一条 grep 门禁回收**（M4 的形态：Task 6 落地后 `grep -rn "allow(dead_code)" src/search.rs` 期望 exit=1）。只写在注释里的「Task N 记得删」等于没人负责。
49. **注释与测试文本也算进 grep 门禁的命中范围**：计划里给实现者的示例注释若含 `unwrap_or(50)`、`, 200)` 这类字面量，会直接把自己的门禁弄红。指针一律写「函数名 + 行号」，不写被门扫描的字面量形态。
50. **数字链一改就要全链同步**：补一条测试 = 该模块 +1、该任务终态 +1、后续每个任务的 Expected、完成判据、交接文档表格与分模块清单全部跟着改。控制方用 `grep -n "旧数"` 收口，别指望实现者发现。
51. **随机 id 让「删除排序键」这种变异变成抛硬币**：`project::create_project` 用 UUID v4（`project.rs:110`），并列簇在去掉名字键后按随机 id 序排，实测 10 次只红 2 次。做排序键的变异取证要用**反转比较方向**，不要用删行；确实无法守护的键（如第三级 id）就写成「预期不红」的取证条目并登记有名缺口，别伪造确定性。

### G. 今天（10-08）新加的二十二条（52–73）

（标题订正：上一版写「十六条」而那一版实际列了 52–66 共 **15** 条 —— 坑 63 说的「计数词会说谎」在本文件自己身上也应验了，这次改完用 `awk '/^### G\./,/^---/' | grep -c '^[0-9]'` 核过。本轮又加 69–71 三条，标题跟着从「十七条」改成「二十条」—— **同一个计数句在同一天里第二次说谎**，所以这条标题的数今后一律用上面那条命令现量再写。本文件最后这版加 72–73 两条后改成「二十二条」，改完再量一次 = **22**，与标题一致。）

52. **presence-only 断言锁不住键集合**：`contains_key("projectId")` 只证明「该在的在」，不证明「不该在的不存在」。线格式是契约时要用 `assert_eq!(obj.len(), 7)`，并配对一条证伪取证（临时加一个字段 → 必须红 → 删干净）。M4 的教训具体形态：T4 的 `bundle_wire_format_is_camel_case` 被控制方当成「7 字段契约已守住」报了出去，复审不采信，才补出 T6 Step 4 那条长度断言 —— **「我们决定不加 `docsCapped`」这条裁定在补断言之前没有任何闸门**。
53. **等价变异全绿 = 那段代码零守护的客观证据**，不是「测试很稳」。某条变异改完**全量照绿**（T4 当时是 119 条），结论必须是「补测试」，而不是「这条变异不成立」。T4 的变异 6 就是靠这条把「三段之一的台账段从没被测过」查出来的。
54. **不可满足的变异期望是计划缺陷，不是实现缺陷**：不许为了让它红而改夹具、删守卫或注随机 id。正确动作是把那条改写成「预期不红」的取证条目 + 登记有名缺口（T3 走过一次、T4 走过一次），并在账本写 `Ruling:`。
55. **控制方不能在跑着「会提交的实现者」的 worktree 里提交任何东西**，哪怕纯文档：会和它的 `git add`/`commit` 抢 `index.lock`，而且 HEAD 中途移动会让 `review-package` 的 BASE/HEAD 对不上报告里写的范围。今天 11:33 那次交接文档提交是刻意**压到 fix round 落地之后**才做的。
56. **「注释级 / 断言级零行为回归」不配开新的一轮派发+复审**：折进下一个本来就要改那个文件的任务（今天 6 条全进 T6 Step 4）。但**必须同时把文件名列进那个任务的 Files 和 `git add`**，否则实现者按简报的边界无权改，残留在下一轮又弹出来。
57. **报告与简报的行号指针必然漂**（今天实测 ±1 到 ±3，还有一处顺序颠倒）：判定标准是语义目标（那段注释 / 那个函数），不是行号。要求 fix round 在报告新章节里按**本轮终态**重出全部指针；报告是 git-ignored 的 SDD 工件，漂了由控制方直接改文本，不开轮。
58. **改计划 markdown 时，代码围栏的配对是给 `task-brief` 用的**：该脚本的 awk 按每行 `^``` ` 翻一个 `infence` 标志，围栏总数一旦变**奇数**，后续所有 `### Task N` 都被判成「在代码块里」，于是不再切边界 —— 今天插了一段带闭合围栏的散文，Task 5 简报直接暴涨到 1333 行（一路吃到文件尾），Task 6 报 `exit 3 / no heading matching`。改完计划先 `grep -c '^```' PLAN` 验偶数，再生成简报；简报行数暴涨/为 0 就是这个信号。
59. **「模块声明归下一个任务」是计划级陷阱**：Rust 里没被 `mod` 声明的文件不参与编译，本任务的 RED 期望、`N passed`、全量条数、`cargo clippy --lib` 全都不会成立，而且失败形态是**静默的**（`cargo test --lib` 一声不响停在旧数）。补上声明后又有第二层：`cargo clippy --lib` 不带 `--tests`，`cfg(test)` 关闭时该模块每个 pub 项都没有 caller，会被逐条判 `dead_code`（`%TEMP%\dcprobe` 用 `rustc --crate-type lib --deny warnings` 实测复现）—— 所以必须同形开一块**带具名回收点**的 `#![allow(dead_code)]`，并把回收门扩成一次扫所有豁免所在文件。
60. **一条 grep 门只挡住它写出来的那种字面形态**：T5 的门扫 `index_job::panic_to_err` 全仓 0 命中，控制方就报「悬空指针收干净了」；实际 `extract.rs:603` 的断言消息写的是 `crate::panic_to_err` —— 搬迁后同样悬空，但模式不匹配，门是绿的。改名/搬家的残留要按**符号本体**扫（grep `panic_to_err` 把每一处读一遍），或把门扩成 `crate::X\|old::X` 两种前缀（T6 Step 3 现在是五条就是这个原因）。
61. **「并窗 / 合并」类判据的两种写法差在带宽，而不变量才是规格**：spec §五.5 字面写「后续命中落进已有窗口」，按字面拿命中自身 `start` 比上一扇右沿 → 间距落在 `(半宽, 2×半宽)` 时既不并窗、新窗左沿又越过上一扇右沿，同一段正文被拼两遍并在其间插一个代表「有省略」的分隔串，而那段一字未省。正确判据是比较**新窗左沿** `start - 半宽`。连带后果要知道：合并带翻倍后链式合并能让一扇窗接近整篇，`MAX_PREVIEW_WINDOWS` 封的是**窗数不是窗长**，`text` 载荷失去上界；给合并加长度上限会重新制造相接处（就是本坑的失效形态），所以只能接受并在真机侧看观感（T9 Step 2 第 7 条）。判据换形时，旧夹具的间距必须重算 —— `four_spaced_hits` 看不见这个缺陷只是因为它的间距 10002 恰好在带外。
62. **报告里「提交后又复跑了一次」如果耗时逐字相同，就是复贴不是复跑**：T5 fix round 的报告里提交前后两次 `cargo test` 报出完全一样的 `7.84s` / `0.35s`。本轮结论没受影响（另有 md5 与 `git status` 干净作字节级 dedup 证据），但「复跑」这一格要的证据形态是**两次之间必然不同的东西**（时间戳、或改动前后的对照），派发词里要提前写明，否则这条自证毫无强度。
63. **计划/简报里的「计数词」是第二个会说谎的地方，比行号更难发现**：Task 6 Step 4 标题写「四处文本残留」而下面编号到 6 条（六条里 5 条落 `search.rs`、第 5 条落 `index_store.rs`），实现者只能猜 —— 它猜成「四处 = 1–4」并把六条全落地，是唯一安全读法，但那是侥幸不是流程保证。同形前科还有坑 60（门只挡它写出来的那种形态）。修法：Files 与 Step 标题里的计数句必须和编号同批写；一旦发现不符，按 `sdd-plan-revision-fix-loop` 入口 B 改本体 + 重切简报，别留给下一轮去猜。
64. **Tauri 命令默认 `execution_context = Blocking`，Windows 上 sync 命令占的就是 WebView2 消息泵那一个线程**：所以「先出锁再做慢 IO」在 sync 形态下**不会**让别的 IPC 进得来 —— 它们连不上那个线程。要换线程必须写 `#[tauri::command(async)]`（对非 `async fn` 生成 kind `sync_threadpool`，`$path(...)` 被放进 `async move` 块、再由 `async_runtime::spawn` 投递）。三条同源事实：① 全仓 `Mutex<Connection>` 只有 `lib.rs:37` 一处取锁、38 个调用点全在 IPC 命令里，后台索引线程自己开连接（`index_job.rs:402`）⇒ **没有 async 之前那把锁无人竞争**，「锁外抽盘」对界面延迟的真实收益是 0，加了才第一次承重；② `async` 让同类命令**可能乱序返回**（sync 下由消息泵天然串行，这事以前不可能发生）⇒ 前端 `seq` 守卫从防御性写法升为承重设计；③ 运行时兑现单测证明不了，只有真机并发对账那一格。取证位置（本机 vendored 源码，逐行读过的）：`tauri-macros-2.7.0/src/command/wrapper.rs:245/:257-258/:355/:382-389`（`$path` 在 `:384`）/`:425/:427`、`tauri-2.12.0/src/ipc/mod.rs:371/:375`、`wry-0.57.0/src/webview2/mod.rs:944-978`、`tauri-runtime-wry-2.12.0/src/lib.rs:5289-5307`。
65. **写进规格的行号引用必须当场 `grep -n` 取，不能凭「我刚读过」的印象**：本轮我在计划里把 `body_async` 写成 `:354-362`，第一次纠正成 `:357`/`:383-388` —— **那个纠正本身还是错的**，fix round 1 复审后重 `grep` 才落到真值 `:355`（`fn body_async`）/ `:382-389`（生成体，`$path` 在 `:384`）。也就是说同一处引用连错两次，而两次都是「我刚读过」的产物。而「`$path` 在 `async move` 块**内**」这一格才是裁定成立的关键，引用漂了就等于把一个正好支撑结论的证据弄丢。规格里的假行号比没行号更坏：下一个读者去那一行找不到东西，然后会怀疑整段。
66. **往 markdown 表格/文档中间插行：`old_string` = 锚点行、`new_string` = 只写新行 ⇒ 把锚点整行覆盖掉**（本轮 HANDOFF §2.2 的 T5 行被 T6 行吃掉一次，靠读回才发现；同形前科是账本里以 `## 暂停点` 标题为锚插入、两次毁掉那个标题）。做法：插行时 `new_string` 必须**原样重复**锚点行，或者改完立刻 `Read` 那一屏确认。
67. **复审给出的行号读数，采信前自己到实时树上一格一格重核**：T6 复审对 `lib.rs:542` 的判定是「仍落在 `search_docs` 头注释那一格内 ⇒ 不是假话，只是脆」；控制方按派发词里「别转述，实时核对」的要求重跑，`sed -n '542p'` 打出来是**空行**（`:543` 才是头注释、`:545` 是 `fn search_docs`、clamp 在 `:551`），而离 542 最近的代码 `:538` 属于**另一条命令** `index_docs` 的 clamp ⇒ 严重度从「脆」升到「当下就指错」，归属不变（仍落终审）但描述变了。同类漂移跨三个文件（`index_store.rs:275`/`:671`、`search.rs:21` 都写 `542`），所以修法不是补一个正确数字，而是**换成符号指针 + 跑一次 `grep -rn "lib\.rs:[0-9]" src/` 清扫**（注意 `extract.rs:31` 那条指向的是依赖包内的 `lib.rs`，不属本仓指针，别一并改掉）。
68. **转述评审/复审「它还要求改 X」之前必须回到它的原文定位 X；缓存文件会消失**：本轮把一条 `extract.rs:574-576` 的「跟着改期望」写进了 HANDOFF 终审清单，随后要去核对时发现复审的那份输出落在 `%USERPROFILE%\.qoder\cache\...\agent-request-*-received.txt`，**会话中途已被清理、文件不存在了**。核查后确认那两处（`extract.rs:575-581` 的 release-abort 守卫、计划 Task 2 的摘边界取证条目）讲的分别是 release `panic = "abort"` 和测试线程，都不依赖命令体在哪条线程，**根本不需要跟着改** —— 于是把那一格改成「同批核对过、确认不需要改」的正面记载。教训：评审要求里凡是会变成下游任务的**行号/文件级动作**，落档时要么当场从原文抄出可复核的字面，要么只写自己到实时树上核过的，否则缓存一没就只剩我自己的想象。
69. **计划正文的「Expected」可以理直气壮地写一条守不住任何东西的门禁**：Task 7 Step 4 原文写「`npm run build` exit 0 —— tsc 段守类型，`ranges: [number, number][]` 与 Rust 的 `Vec<(usize, usize)>` 线格式对不上就会在这里红」，实现者照抄进报告，评审一戳就破：**Tauri 的 `invoke<T>` 是类型断言不是校验，前后端之间没有任何编译期耦合**，Rust 侧改字段名时 tsc 一个字都不会报。这条与坑 52/53 不同族：那两条是「断言太弱」，本条是「散文把不存在的守护说成存在」，而散文不会红，只有去读依赖侧结构体的人才看得见。做法：凡 Expected 里出现「X 会抓到 Y」这类句子，先问一遍「Y 坏掉时 X 到底会不会红」，答不出机制就是假门禁，就地改写并把真对账路径写出来（本轮改法是 `search.rs` 既有的 `bundle_wire_format_is_camel_case` + 评审逐字段读两个 `#[serde(rename_all)]` 结构体，并把 `DocPreview` 没这层测试登记为有名缺口）。
70. **控制方自己的派发词也会犯它写在坑 19 里的错，而且错得更隐蔽**：我在 T7 派发词里写「node v24.18.0」，那是 **harness 环境元数据**里的值，本机 PATH 上实为 **v26.3.0**（`which node` = `/d/tools/nodejs/node`）—— 而计划 `:51` 与坑 19 早就把这条写对了，是实现者实跑 `node -v` 把我纠正的。两个结论：① 平台元数据不是实测，写进派发词前同样要现查；② 把实测事实写进计划正文（本仓的「已实测事实」表）真的会在下游救回来 —— 这条是它的一次正面回放。
71. **测试名里承诺的行为也算一条断言，必须有夹具钉住；判定方法是做一次「交换/合并/借用」变异**：T7 第 3 条的名字写「有内容的段按 项目 / 台账 / 正文 顺序产出」，但 8 条夹具**没有一条让两段同时非空**（`sections[0]` 全来自单段夹具）⇒ 把 `bundleToSections` 里 ledger 与 docs 两个 `if` 块整体交换，8 条照绿（控制方实测：`pass 8 / fail 0 / exit=0`）。补上「三段同现」夹具后同一处交换变红（`pass 7 / fail 1`，失败点指名第 3 条）。可迁移的判据：测试名或注释里出现「顺序 / 各自 / 互不 / 只追加一次」这类关系词时，就用**交换输出块顺序**、**合并两个分支**、**让 A 段借用 B 段的计数**这三类变异去证伪；红不了就是零守护，按坑 53 走「补测试」而不是「这条变异不成立」。**复审把这条推得更远**：被测函数若是**静态块序**（没有输入相关的排序逻辑），一条**整数组** `deepEqual` 会传递性地覆盖所有两段同现的子序（六种排列只放过恒等式），于是一格「我以为只自证了一半」的断言其实全守住了 —— 这既可能让人**高估**也可能让人**低估**覆盖度，而唯一分辨办法还是那句：自己跑变异，别读报告。
72. **派发词里同时写「改写 X 节」和「不要删旧内容」= 把冲突丢给实现者裁决**：T7 fix round 的派发词两条都说了，实现者只能发明第三种读法 —— 它选了「假句**原位保留** + 紧贴下方一句『是假的，本轮撤回』+ 完整新叙事**另置** §8.3」。复审与控制方一致判**可接受**（线性阅读者不可能越过假句而不撞见撤回；报告是 gitignored 工件，误导面止于本目录），但那是侥幸。**规则**：改派发词时，凡对同一处文本同时下达「替换」与「保留」两种强度，必须当场写清哪一种赢；否则下游的读法不在你的裁定里，复审就没法判它对错。
73. **控制方自己的一次计划编辑，就是下游行号的漂移源**（坑 65/67 在本文件身上**第三次**应验）：我在 §6 指针里写了 `T8 = 1993`、`T9 = 2311`（那是 `10ba0ef` 时的真值，计划 2403 行），而**我自己随后又改了两次计划**：`3a2cfc8`（T7 派发前预检）净 +10 行 ⇒ T8 到 `2003`，`c2b3d35`（T7 首轮评审裁定）净 +18 行 ⇒ T8/T9 落到 `2021 / 2339`（现 2431 行）。两次合计把下游整体推下 **28 行**，而这 28 行全是我自己写的。前两次（坑 65、坑 67）的错数来自「我引用别人给的读数」，这次是**我自己量的数被我自己随后的编辑弄漂** —— 漂移源换成了控制方本人，所以更隐蔽：它躺在「指针」这一节里，读者会当成权威去 `sed -n`。**规则**：文档里凡是指向**会变的文件**的行号，一律写命令（`grep -n "^### Task" PLAN`）而不是数；确实要留读数就标「只作对照，取数只认命令」。

---

### H. 今天（10-09，Task 8 这一轮）新加的六条（74–79）

74. **「写了没人读」有两种强度，简报逐字给代码时两种都要当场查**：`busy` 那种（声明了、effect 里写、JSX 从不读）会被 `noUnusedLocals` 抓成 `TS6133`，**编译器替你看着**；`LocalSearchState.query` 那种（interface 成员 + 初值 + 两处 `set` 全在、全仓零读取方）**tsc 一个字都不报**，只有主动问「谁读它」才能抓。T7 预检的 `FieldHit`、T8 预检的 `as HitSource`、T8 定稿的 `busy`、T8 评审的 `query` 是同一条坑的四次应验，强度依次递减。**判据**：对本轮新增的每个 state 字段 / interface 成员 / import 做一次读取方点名，答不出就删。
75. **给「总是挂载」的组件加状态，必须列出「effect 每个分支清哪些格」**：M-8 就是这么漏的 —— `DocPreviewDialog` 的 `useState` 挂在宿主无条件渲染的那个组件上（关窗只卸载 `DialogContent`，不是卸载组件），而 `notice` 只有 `guard` 的成功路径清、effect 的两条分支都不碰 ⇒ A 文档的「操作失败」红字跨到 B 文档上。**「组件卸载所以状态自然消失」这个假设在受控对话框里根本不成立**。修法一行级（effect 顶部 `setNotice(null)`），但要紧的是流程：新状态的清除路径必须逐条写明，不能靠读者推断。
76. **兜底格式化对真实 rejection 形状是盲的，而一个 `catch` 里三种形状都会出现**：当场 `node -e` 验过 —— `JSON.stringify(new Error("x"))` = **`{}`**（`message` 是不可枚举自有属性），`String({code:"a",message:"b"})` = **`[object Object]`**。也就是说 `ipc.ts::toAppError` 那句 `JSON.stringify(e)` 兜不住 JS `Error`，而 `project-detail.tsx::guard` 那句 `String(e)` 兜不住后端 AppError 形状的普通对象 —— **两处各只覆盖一半，方向还相反**。Rust 侧回来的（对象）、插件抛的（裸字符串）、JS 自己抛的（`Error`）三类都会进同一个 `catch`，所以兜底分支必须显式分派三种，不能只写一个 stringify。
77. **派发词里指定「追加为报告第 N 节」之前必须先 grep 现有标题编号**：我在 fix round 派发词里写「追加为 §八」，而首轮报告已经有 `## 八、遗留 / 建议` ⇒ 撞号。实现者没擅自给别人定稿的正文重编号（对的），控制方当场把新节标题改成 `九`、节内 `8.x` 小节沿用并在节首注明。报告是 gitignored 工件 ⇒ 直接改文本、不开轮（坑 57 同口径）；但下次凡指定章节号，先 `grep -n "^## " 报告` 再写。
78. **具名风险写成「状态机 / 不变量」的形式，复审就会在 diff 之外找到东西；写成「功能对不对」它只会打勾**：本轮 NR-1 问的是「`notice` 与 `error` 会不会互相盖掉或残留」，复审的结论是 NOT SETTLED —— 互相盖掉**没有**（两格写路径不相交，它逐处列了 `setError`/`setNotice` 的落点），但**跨文档残留**有，于是带出新 Minor M-8。如果那条风险写成「预览失败处理对不对」，它会答「ADDRESSED，有 catch 有回执」然后收工。同形前科是坑 62：证据形态要求写死（「两次之间必然不同的东西」），自证才有强度。
79. **「代码是简报逐字给的」不减轻 finding，只改变处置路径**：T8 的 I-1（`void api.revealFolder(...)` 吞掉设计承认会失败的动作）与 M-8 都是简报自己的形状。评审照样按级别提，但裁定必须是「**先改计划本体，再派 fix round**」（本仓闭环），而不是「实现者临场发挥补一下」。反过来也成立：实现者按简报写出缺陷代码时，只要在报告里点名「这是简报的形状」，控制方就不能把它记成实现质量差 —— 本轮实现者两处都这么报了，判为正确处置。

### I. 今天（10-09，Task 9 真机 + D6 这一轮）新加的六条（80–85）

80. **探针 / 夹具脚本本身就是被测环境的一部分，它错了会把结论整体带走**：这一轮我自己的探针连续三次误引导 —— ① 量渲染卡顿时「点击之后每轮开头都点一次『关闭』」⇒ 刚打开的对话框被自己关掉，读数成 `marksAppearedMs = null`，我当场把它记成「渲染 160 秒不出来」；② 嵌套模板串里的正则被转义打坏 ⇒ 探针静默挂死 25 分钟；③ 一个 `[class*=outline]` 的选择器打穿对话框内容 ⇒ **违反本仓红线第 5 条**，把生成语料整段打印进上下文。**规则**：CDP/DOM 探针每步单独一次 evaluate（别写大循环）、只回布尔/计数/文案前缀、超时给足并且**失败先怀疑探针而不是被测**。三次里只有一次是真发现（M-8），而那一次的证据也是换成小步探针之后才拿到的。
81. **给 sync 命令做 CDP 计时，必须在空队列上取**：超时不会取消服务端那次执行 —— 它会留下**孤儿调用占住消息泵那一个线程**，于是后续每一次测量都排在它后面，读数逐次递增（我这轮 `search_all` 从 32 s → 240 s 就是这么来的，看着像一个性能发现，其实是探针事故）。两个处置：要么每次超时后先等队列排空（用一次便宜的 `db_status` 探，回到个位数毫秒才算空），要么把长任务改成小步分别 evaluate。**同一条也解释了为什么 async 的命令不会这样：`db_status` 3 ms 就能插进去。**
82. **会改库状态的索引动作要排在依赖旧状态的断言之后**：D6 那轮跑了 `index_start(rebuild: true)`，它把断言 5 依赖的那格状态（「文件已从磁盘删掉但库里还留着 `ok` 行」）扫成了 `missing` ⇒ 我随后给需求方设计的接力实验直接做不成（她回「没有命中」）。**取证顺序 = 先用旧状态跑完所有断言，再做会改状态的步骤**；已经改完就得另造一份等价夹具（本轮后来新建 `红字残留实验.txt` 只索引到单个项目、`rebuild: false`，就是为了不再动别的行）。
83. **观察类结果不要从数据库反推，一次一问直接问**：我试图用 `project_list` + 扫 `index_docs` 路径判断需求方那三下有没有做成，但软删 + 索引清扫已经把证据抹平（只剩「项目数回到 4」这种两读事实）；我甚至一度想用只读连接去打开那个 WAL 正被应用占着的库 —— **打不开，也不该打**（第二个连接会参与 checkpoint）。改成一次一问，第一问就拿到决定性事实（红字没跨文档），而它比我猜的结论更好：顺着它读出了 `busy` 那格与结果树互斥卸载这条机制。**判据**：凡「只有人眼能看见」的读数，问人；数据库只用来回答「状态有没有落盘」。
84. **判据里的「且」要按证据重写议题，既不按字面放过也不硬凑**：第 7 格写的是「`text` > 6 万码元 **且** 渲染明显卡顿或滚动失效 ⇒ 升级终审」。实测载荷那半边 114 万 ≫ 6 万成立、渲染那半边不成立（`longTasks` 全空、rAF 1 ms、20 万次同步循环 2 ms）⇒ 按字面就该「记数字收口」放过了。但真正不可用的是**等 160–266 秒且对话框只有『关闭』没有取消**（关掉只卸载前端，后端那次抽取照跑完）。裁定：照样登记终审，**议题写成「极端单文档成本 + 无取消」**而不是「渲染卡顿」—— 因为议题决定下一轮改什么，写成卡顿就会去给合并加长度上限，而那正是 Task 5 Important-1 修掉的相接窗失效形态的重演。
85. **改一个常量要顺着它把「名字里的数字」全查一遍**：D6 三个上限 20/10/10 → 30/15/15 牵出的不是三行赋值，而是 ① 两条测试的**函数名**本身就嵌着数字（`section_drops_the_21st_cluster…` / `cluster_keeps_10_items_and_reports_the_other_2`）② `cluster_by_project` 上方的注释**按测试函数名**点名证据（写着「21st」「21 个等条数簇」）③ 夹具里的字面量 `"p20"`（代表「名序最大的那一簇被扔掉」，上限变 30 后它不再是被扔的那个）。漏掉任一处，仓库里就留下一句假话，而 tsc、cargo、clippy 都不会报。本仓被「名字/注释与实现脱节」咬过四次（坑 57/60/63/71），这次是第一次发生在**常量**而不是代码位置上。**判据**：改常量后跑一遍 `grep -rn "<旧值>"` 加上「谁在注释里提这个名」的 grep，两处都要 exit=1 才算收干净。

---

## 6. 指针

| 东西 | 路径 | 关键位置 |
|---|---|---|
| M4 spec（决策权威） | `docs/superpowers/specs/2026-10-07-m4-unified-search-design.md` | 行号一律 `grep -n "^## "` 现取（共 202 行，本轮量得：决策表 `:20`、契约 `:32`、排序与截断 `:87`、**预览通路 `:95`（末尾新增「线程口径」整段 = T6 Important-1 的裁定）**、模块切分 `:110`、错误处理 `:126`、测试策略 `:140`、**真机与性能 `:163`（「三格」已扩成「四格」，第 4 格 = `async` 的运行时对账）**、不在本轮 `:174`、风险 `:197`）。**注意 `:110/:140` 这一类后半篇的数已被 `3677865` 那次追加往下推过两行**，与本文件坑 73 同形，取数只认命令。**例外登记**：§四 `:92` 与 §八 `:148` 在 10-09 被 retro-edit 过一次（D6 三个上限 20/10/10 → 30/15/15，连带夹具规模与期望值 21/12/15 → 31/17/20），带日期、出处与「上调不改变不做展开这条边界」的附注 —— 本仓 ordinarily 不 retro-edit 历史规格里「描述旧状态」的句子，这次例外的理由是那一格是**活的契约数字**，代码已经是新值，spec 留着旧数就是假话 |
| M4 实施计划（执行权威） | `docs/superpowers/plans/2026-10-07-m4-unified-search.md` | Global Constraints `13`；**已实测事实 `37`、条数链 `58`**；文件结构 `62`；T1–T9 起点**一律 `grep -n "^### Task"` 现取**，本文件重写时量得：T7 = `1721`、**T8 = `2021`**、T9 = `2365`。注意 T8/T9 因为 T7 的**两次计划编辑**（`3a2cfc8` 净 +10、`c2b3d35` 净 +18）而整体下移（`1993→2021`、`2311→2339`）T8 = `2021`、T9 = `2382`（T9 又被 T8 那两轮计划编辑推过两次：`2339→2365→2382`）—— **这一格是坑 73 的现场，而且已经第四次要抓**：上一版的数是被我自己的计划编辑弄漂的，不是子代理报错的，所以那一格只作对照，取数只认命令；**Task 6 Step 1 下面那四段 = `async` 裁定的权威文本**（`body_async` 的行号在 fix round 1 复审后更正为 `:355`/`:382-389`，`$path` 在 `:384`）；完成判据与「本轮不做」在文件末（共 **2492** 行；`^``` 计数 **124 = 偶**，坑 58 的门 —— T9 这几轮改动全落在既有围栏内或散文里，改完重量过）。**Task 9 的复选框现状：Step 1/2/3 = `[x]`（`:2396`/`:2406`/`:2431`），Step 4 仍 `[ ]`（`:2445`）**（现取：`grep -n "^- \[.\] \*\*Step" PLAN \| awk -F: '$1>2390'`，别背）。Step 1/2/3 的勾是把真机读数直接写进正文（含两条前提纠正与 D6 的诚实附注），Step 4 只做了「清场」那一半，文档收口那一半没动。 |
| SDD 账本（恢复地图） | `.superpowers/sdd/2026-10-07-m4-unified-search/progress.md` | 预检 10 行表 `9-20`；红线 `24`；进度一览 `33`（**已写 T1–T8 complete**）；**最后一节 `## Task 9 真机 + D6 落地…` 在 `:331`** = 当前状态：Step 1/2/3 的读数归档、`ef49c1a` **尚未评审**与 Step 4 未做这两条欠账、实现者七条 concerns 的逐条裁定（含「`index-status.tsx` 那个『20 个』是 `PROGRESS_EVERY`、别误改」这条反向提示）（共 **341** 行 / **28** 个 `## ` 段，`grep -n "^## " progress.md \| tail -5` 现取） |
| **下一个动作 = 给 `ef49c1a` 生成评审包并派任务评审，然后做 Step 4 文档收口** | `bash <SDD-skill>/scripts/review-package docs/superpowers/plans/2026-10-07-m4-unified-search.md 8ebdb92 ef49c1a` | **BASE 用 `8ebdb92`**（D6 派发前的 HEAD，它本身就是 T9 预检那个 docs 提交 ⇒ 范围内只有那一个代码提交，docs 天然在外，不用做「规格来源」声明）。**`cd04f33`（我把读数落回计划与 spec 那次）在 `ef49c1a` 之后，别拿它当基线。** 评审三重点写在 §4.1 第 1 步（测试名里的数字 / 按测试名点名证据的注释 / 实现者那条变异探针要独立复现）。第二步 = Step 4 文档收口（`docs/开发进度.md` 的 M4 行 `:13`、没能自动验证清单、§3 尾巴第 3 条那笔归属账），做完才能记 `Task 9: complete`。**T9 的真机部分不要重跑**：沙盒已删、211 MB 语料已清、读数全在 `t9-*.json` 与计划 Task 9 Step 1/2/3 正文里。**T8 的三份工件只作对账**（`task-8-brief.md` 361 行、`task-8-report.md` 694 行 / 九节、`task-8-rereview-1.md` 100 行）；T9 的两份报告是 `task-9-report.md`（实现者 302 行）与 `t9-report.md`（控制方 80 行，§五/§六 仍是占位）。 |
| T1–T8 实现者报告 | 同目录 `task-{1..8}-report.md` | T4 报告 391 行（尾部 §八 = fix round 1 与全部变异实测）；**T5 报告 §九 = fix round 1**（原始门禁输出 + `exit=` 行、两格变异读数、7 处还原的 `diff -q` 证据）；T6 报告含 Step 4 六条的逐条落点行号表与那条「`… \| tail -N; echo exit=$?` 读的是 tail 的状态」的自我否证补跑；**T7 报告 769 行 / 八节**（§一–§七 = 首轮：6 格变异读数（其中 `>=` 那格按简报要求如实记「不红」）、value-import 的**两格区分**（`@/lib/ipc` 红在加载期 `ERR_MODULE_NOT_FOUND`、`node:fs` 红在第 8 条断言）、它自己抓到的那次转写偏差；**§八 = fix round 1**：三条还原证据（`cp` 备份 sha256 `58b76330…` 与实现者记录逐字节相同 + `git diff --exit-code` = 0 + `git status --porcelain` 空）、变异 #6 的 `pass 7 / fail 1` 读数、以及它按裁定把 §2.2 那句假门禁就地改写并补「没能自动验证」条目 —— 复审后来还把这一格的自述**向上纠正**了一次，见 §3 缺口表末行）；**T8 报告 694 行 / 九节**（§九 = fix round 1：四道门禁各自的原始 `exit=` 行、四件 delta 的**可证伪证据明写成「三件零守护 + I-1 半守护」不粉饰**、还原的四件证据 md5 `de6b507f…` + `diff -q` + `git diff --exit-code` + `porcelain` 全列；定向复审另落 `task-8-rereview-1.md` 100 行，含它自己重做的变异与还原） |
| 评审包 | 同目录 `review-*.diff` | **14 份**，命名即 BASE..HEAD。T8 fix round `review-40f4ca3..0757545.diff`（**2 commits / 21595 B** —— docs 提交 `b2670db` 晚于 FIX_BASE、靠基线排除不掉，复审派发词同样显式声明它是**规格来源、不是评审对象**，复审照做、对文档零 finding）；T8 首轮 `review-4b8c851..40f4ca3.diff`（**1 commit / 20123 B**，基线恰是 docs 提交所以天然在范围外；评审者还独立验出 Step 1/Step 3 落地代码与简报 **byte-identical**）。T7 首轮 `review-3a2cfc8..23b8b53.diff`（**1 commit / 13978 B** —— 同上，范围干净）；T7 fix round `review-23b8b53..034b0ef.diff`（**2 commits / 75994 B** —— 含文档轮的体积信号：13978 → 75994）。T6 首轮 `review-5a00500..6bc6354.diff`（1 commit / 18315 B，范围内**没有**文档）；T6 fix round 1 `review-6bc6354..077c275.diff`（**2 commits / 103580 B，范围内含 docs 提交 `3677865`**） |
| 里程碑与证据史 | `docs/开发进度.md` | 里程碑表 `5-20`（**M4 行 `13` 还写「未开始」，T9 收口时改**）；环境结论 `22-45`；M3 验收证据 `70`；M3 已知缺口 `107-112`（**归属待按 spec §十 更正**） |
| 技术方案（更早的权威） | `docs/技术方案.md` | §3.4「不做假的归一化」是 D2/D3 的依据 |
| 上一版交接（本文件历史 **12** 版，本次落地后 13；数法 `git log --oneline -- docs/HANDOFF.md \| wc -l`） | `git show 3677865:docs/HANDOFF.md`（§G 15 条 / 全 66 条坑，§2.2–§3 写「T6 fix round 1 待派发」）、`git show 10ba0ef:docs/HANDOFF.md`（§G 17 / 全 68，T6 已 complete、T7 未派发）、`git show c2b3d35:docs/HANDOFF.md`（§G 20 / 全 71，写「T7 fix round 1 待派发」）、`git show e0c09eb:docs/HANDOFF.md`（§G 22 / 全 73，写「下一个动作 = Task 8 的入口 B 派发前预检」）、`git show 62af638:docs/HANDOFF.md`（§G+§H 28 / 全 79，写「下一个动作 = Task 9 真机验收 + D6 上限实测 + 文档收口」）| 本文件已含各版全部坑并扩到 **85 条**（A–F 51 + §G 22 = 52–73 + §H 6 = 74–79 + §I 6 = 80–85；现量：`awk '/^### [GHI]\./,/^---/' docs/HANDOFF.md \| grep -c '^[0-9]'` = **34** 条分节条目）。**这几版里最要命的不是数旧，是那句「下一个动作」都已过期** —— 恢复时只认本文件 §3 与账本最后一节，别 `git show` 一个旧版照着派发 |
