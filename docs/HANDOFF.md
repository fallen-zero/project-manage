# 交接文档 —— M4 首屏统一检索

**快照时间**：2026-10-08 约 11:50（本地）
**分支**：`feat/m4-unified-search`，代码 HEAD `91476b9`（其后两次都是文档提交，最后一次是 `cdc5163`），`git status --porcelain` 干净
**门禁基线**：`cd src-tauri && cargo test --lib` → **120 passed / 0 failed / exit 0**（11:46 由控制方在 HEAD 上实跑，不是推算）
**一句话状态**：M4 九个任务里 **Task 1–4 已完整收口**（实现 → 双 verdict 评审 → 修复轮 → 定向复审 → 账本 `Task N: complete`），Task 5 的简报已按最新计划文本重切好、**尚未派发实现者**。没有被技术问题卡住；暂停是需求方要求的。

---

## 1. 我们在做什么任务

做的是本地桌面工具「项目资料管理」（Tauri 2 + React + rusqlite bundled FTS5 + jieba-rs），按里程碑推进：M0 脚手架与 WAL 校验、M1 项目档案与目录映射、M2 信息台账与列加密、M3 全文索引管线都已 ✅ 完成。**M4 = 首屏搜索框统一结果**，做完 M4 就是 MVP（M0→M4）。

M4 本轮交付的形状（权威定义在 spec，不要从本文件重述去实现）：

- **两条 IPC 命令**：`search_all` 返回一个 `SearchBundle`（三段：项目档案 / 信息台账 / 文档正文），`doc_preview(doc_id, query)` 返回窗口拼接的原文 + UTF-16 码元高亮区间。
- **段内按项目聚簇，不跨段合并**：`bm25` 与 `LIKE` 的分数不可比，spec §3.4 明令禁止假归一化。
- **三级截断 20 簇 / 10 条 / 10 平铺，各有计数、只显示不展开**。
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
- **M4 实施计划定稿**（`a44cdac`，现 2376 行；执行期间已走过 7 次「修订计划本体 → 重切简报」闭环，本次是第 7 次）：`docs/superpowers/plans/2026-10-07-m4-unified-search.md`，9 个任务，每个都是「失败测试 → 跑红 → 最小实现 → 跑绿 → 变异取证 → 门禁 → 按名 stage 提交」的可照抄粒度。

### 2.2 今天（2026-10-08）：16 次提交（本文件那次是第 16 次），Task 1–5 收口

| 任务 | 提交链 | 该任务终态 | 评审结论 |
|---|---|---|---|
| T1 `query_terms` 唯一切词入口 + `MAX_QUERY_CHARS` | `497c0a1` | 106 passed / tokenize 7 | Spec ✅ / Approved，0C 0I 7Minor：3 条折进 T3 的「注释保鲜」Step，其余 4 条当场裁定**不改**（各有理由，见账本） |
| T2 `panic_to_err` 提到 `extract.rs` 做 `pub(crate)` | `f72a5c8` | 106（extract +1、index_job −1） | Spec ✅ / Approved，0C **1I**（plan-mandated）2M。`Cargo.toml:46` 的悬空指针判为**计划本体缺陷**，改在 T5 的 Files+Step 6 里收口 |
| T3 `clean_snippet` 的 `A B AB` 三连折叠 | `e18864a` → 计划修订 `29d390f` → 整改 `2670768` | 108（tokenize 9） | 首轮 Spec ✅ / **Needs fixes**（0C 3I 6M，其中 2 条 Important 是简报本身写错：签名 `E0106`/`E0716`、那条变异**三重蕴含**下永远不红）→ fix round → 复审 7 条全 ADDRESSED |
| T4 `search::unified_bundle` 三段聚合 + 段内聚簇 + 三个截断计数；`index_store::indexed_project_count` | `899b58c` → 计划修订 `9dca06c` → 整改 `91476b9` → 计划修订 `cdc5163` | **120**（search 20、index_store 15） | 首轮 Spec ✅ / **Needs fixes**（0C **3I** 5M）→ fix round 1 → **定向复审：5 条 finding 全 ADDRESSED，无新 C/I**，剩 7 条文本级/登记类 open |
| T5 `doc_preview.rs` 重抽原文 + 窗口拼接 + UTF-16 码元区间 | 计划修订 `c5ed0b1` → 实现 `614f1a2` → 计划修订 `ea4187d` → 整改 `d544175` | **134**（doc_preview 14） | 派发前预检切掉两处计划原文自相矛盾（坑 59）；首轮 Spec ✅ / **Needs fixes**（0C **2I** 5M，三条具名风险全 SETTLED）→ fix round 1 → **定向复审：5 条全 ADDRESSED、无新 C/I**，具名风险 SETTLED（复审自己复现了变异读数并给出「多一扇窗 ⇒ 其后每个命中起点位移 ≥3 码元」的一般化论证） |

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

**SDD 工件现状**（`.superpowers/sdd/2026-10-07-m4-unified-search/`）：账本 `progress.md`（预检 10 行表 + 红线 + T1–T5 五个 complete 节 + 全部 `Ruling:` + 两个暂停点 + `## Task 5 派发前预检` + `## Task 5 首轮评审裁定` + `## Task 5 fix round 1 复审（round 1/5 → complete）`）；简报 T1–T6 已生成（T5 **551 行**、T6 **144 行**，都是 T5 复审收口后按修订重切的）；报告 T1–T5；评审包 8 份 diff。**T5 已 complete**：代码 `614f1a2` + fix round 1 `d544175`，全仓 **134 passed**、`doc_preview::tests` 14 条、两条 clippy exit=0 零 warning、三条 grep 门 exit=1。

---

## 3. 当前卡在哪

**没有被技术问题卡住。** 10-08 下午恢复后 T5 已走完「派发前预检 → 实现 → 首轮评审 → 计划二次修订 → fix round 1 → 定向复审」并收口为 complete，下一个动作是派发 **Task 6**（简报已按最新计划重切）。恢复时不需要重新理解上下文，账本、简报、评审包、报告都在。

需要**人点头**的两件事（都不阻塞 T5–T8）：

1. **「结果被取数上限截过」要不要让用户看得见？** 本轮裁定是不加 `docsCapped` 布尔位（理由见 §2.2 的 I-2）。如果需求方认为必须让用户知道，最小改动是在 T8 文案里、当 docs 段恰好吃满 20 簇×10 条时追加一句「可能还有更多」，**不需要动线格式**；要精确数字就得在 T5/T6 之后再加一条 count 查询。M4 会按现方案收口，除非有人改这个裁定。
2. **T9 的真机点击只有人能完成**：原生目录选择对话框、`revealItemInDir`、`window.confirm` 这三条路在 CDP 里点不通（M3 已实测确认）。到 T9 需要需求方亲手点一轮，其余真机步骤（`search_all` 往返计时、5 万行沙盒、D6 上限裁定）都由代理自动跑。

已登记的有名缺口与归属（**都不阻塞，且都已写明为什么不在本轮补**）：

| 项 | 归属 | 现状 |
|---|---|---|
| 簇序第三级 `id` 键无测试可红 | M5 | 注释已说实话，取证条目写成「预期不红」。补法要么给 `projects.id` 开测试注入点，要么在测试里裸 SQL 固定 id |
| 台账侧 `MAX_CLUSTERS_PER_SECTION` 截断无独立用例（要 21 个有台账命中的项目） | M4 终审按需 | 第 11 条已证明 `ledger_rows` 真喂进 `cluster_by_project`；未证的只剩台账侧簇数计数，与正文侧共用 `search.rs:292-295` 同一段代码，风险窄 |
| `⋯` 字面量本身无独立测试（`[ ]` 由 `:520` 接住） | M4 终审 | 唯一守护者是新写进 `tokenize.rs:120` 的「来源 = `index_store::SELECT_SQL` 的 `snippet()` 实参」注释 |
| `MAX_QUERY_CHARS` 下边界（`>` 改 `>=`）无守护 | 已裁定不做 | 上侧被拒由 T4 的 129 字 `invalid_input` 用例钉住；补它要再动整条链 +1，代价大于收益 |
| `indexed_projects` 每次查询一次 `count(DISTINCT project_id)`（索引 `ix_index_docs_project` 对这个形状不帮忙） | T9 真机出成本证据 | spec §三 定死的字段，量级不达标再谈缓存 |
| 纯静默拒绝 WAL 那条分支（自 M0 起）本机不可构造 | 无测试守护，有名记录 | 见 `rusqlite` 那条坑（§5 第 1 条） |
| `lower_first` 折叠展开型映射（ß/İ）本机无夹具 | 无测试守护，有名记录 | 不可构造，不是漏写 |
| 预览 `text` 载荷**没有长度上界**（并窗链式合并可把一扇窗拉到接近整篇；`MAX_PREVIEW_WINDOWS` 封的是窗数不是窗长） | T9 真机观感 + M4 终审 | T5 复审 Minor-1。裁度是**接受**：加长度上限就得在超限处断开，而断开处必然相接或重叠，正是 Important-1 修掉的「正文吐两遍 + 假省略号」失效形态。判据写在 T9 Step 2 第 7 条（`text` > 6 万码元**且**明显卡顿才升级为终审 finding） |
| 重叠带（间距 4000–8000 char）的「重复段落」症状无**专用**夹具 | M4 终审（复审裁定不阻塞 T5） | 并窗判据本身有守护（`near_hits…` 的 8002 边界夹具 + 两格变异都红），缺的是症状的直接断言。复审给的结构性理由：条数链 14/134 与 Important-2 裁定「断言文本/测试名/`body` 签名全部不动」把它裁在 T5 之外，且加带内夹具不多抓一类变异 |

旧交接文档里的三个尾巴，现在的状态：

1. ~~todo 只建到 #35~~ → **已补齐**（#34–#42 对应 T1–T9，T4 刚收口）。恢复地图仍然是账本，不是 todo。
2. ~~T2–T9 简报未生成~~ → **按需生成**（T2–T6 已有；T7–T9 刻意留到派发前现切，因为计划文本会被修订，`task-brief` 是按任务正文从计划里切的）。
3. **归属口径不一致（这条值得优先记住）**：`docs/开发进度.md` 的 M3 已知缺口第 6–11 条（`107-112` 行）以及 `125` 行写「归 M4」，但 spec §十 已把同一批（`m-3` 措辞、`m-4` emit 重复终态、`m-5` 外层 `report_failure` 无再上兜底、`m-6` 裸字符串比较判改指、`set_root_dir` 的 `DELETE`+`INSERT` 窄竞态、`ON DELETE CASCADE` 另一半）收回 **M5 / 待真机**，M4 计划里**没有**这些活。另外该文件 `13` 行的 M4 状态还写着「未开始」。**T9 文档收口时必须一起改**，否则这笔账会在两个文档之间反复弹。

---

## 4. 下一步计划

### 4.1 恢复入口（照抄即可）

```bash
cd /e/zero/demo/2026/project-files-manage
git branch --show-current          # 应为 feat/m4-unified-search
git rev-parse --short HEAD         # 交接时是 d544175（T5 fix round 1）
cd src-tauri && cargo test --lib   # 应为 134 passed / 0 failed
tail -40 ../.superpowers/sdd/2026-10-07-m4-unified-search/progress.md   # 账本最后一节 = Task 5 fix round 1 复审（complete）
```

账本里没有 `Task N: complete` 的第一个任务就是下一个要派的：**Task 6**（简报 `.superpowers/sdd/2026-10-07-m4-unified-search/task-6-brief.md`，144 行）。**注意**：T5 这一轮走过两次「改计划本体 → 重切简报」闭环——派发前预检切掉两处会让 T5 通不过自家门禁的原文（`mod doc_preview;` 被划给了 T6；文件头注释带着自家 grep 门要扫的字面量），T5 首轮复审后又切掉五处（两条 Important 的裁定、变异 3「红点是两处」的不实措辞、`stitch` 里那句根本编译不过的 `collect::<Vec<char>`、`PREVIEW_WINDOW_CHARS` 的 doc 注释、以及新增的 T6 活）；权威文本在账本 `## Task 5 派发前预检` 与 `## Task 5 fix round 1 复审` 两节。简报都是修订后重切的，别再按本文件 §4.2/§4.3 的旧描述派发。派发前记 BASE（`git rev-parse HEAD`），派发词只带：一句话上下文 + 简报路径 + T1–T5 产出的接口名 + 红线 + 报告路径。

助手脚本（都在 SDD skill 目录下）：`scripts/task-brief PLAN_FILE N`（切简报）、`scripts/review-package PLAN_FILE BASE HEAD`（评审包，BASE 必须是**派发前**记的 HEAD，绝不 `HEAD~1`）、`scripts/sdd-workspace PLAN_FILE`（工作目录）。

### 4.2 剩下的四个任务 + 终审（T5 已 complete）

| 任务 | 动什么 | 该任务终态 |
|---|---|---|
| ~~T5~~ **已完成** | 新模块 `doc_preview.rs`：重抽原文 + 窗口拼接 + UTF-16 码元区间；`lib.rs` 的 `mod doc_preview;`；`Cargo.toml:46` 与 `extract.rs:577` 两处悬空注释；一块 T6 回收的 `#![allow(dead_code)]`。fix round 1 把并窗判据从「命中起点」改成「新窗左沿」（Important-1）并把 `near_hits…` 的夹具按半宽重算（Important-2） | **134**（doc_preview 14），提交 `614f1a2` + `d544175` |
| T6 | 两条 IPC 命令接线（`search_all` 锁内查库；`doc_preview` **锁内查库、锁外抽盘**，刻意不写成一条持锁调用）；回收 `search.rs` 与 `doc_preview.rs` 两块豁免；**另加两处注释/字符串级残留**（`doc_preview.rs` 里 `PREVIEW_WINDOW_CHARS` 的 doc 注释同步成 6 行版、`extract.rs:603` 断言消息里的 `crate::panic_to_err`）→ Files 与 `git add` 是**五个**文件 | **134**，handler 39 → **41** |
| T7 | 前端类型 `src/types/search.ts` + `api.ts` 的 `searchAll`/`docPreview` + 纯逻辑 `src/lib/search-order.ts` + `tests/*.test.ts` | Rust 仍 **134**、`node --test` **pass 8** |
| T8 | store 的 seq 守卫、`doc-preview-dialog.tsx`、`search-bundle.tsx`、`pages/search.tsx` 薄壳 | `npm run build` exit 0，不新增单测 |
| T9 | 真机验收 + D6 上限实测裁定 + 文档收口（含 §3 那条归属口径） | 见 4.4 |
| 终审 | 整分支 code review（最重的一次），一次性整改 + 一次定向复审 | 134 全链 |

计划正文在执行期间不改；若评审裁定某任务**文本本身**写错，走「改计划本体 → 重新生成该任务简报 → 账本记 `Ruling:` → 派发 fix round」的闭环（今天 T3/T4 各走过一次），不要为此多开一轮评审，也不要让实现者自己发挥。

### 4.3 T6 有六处**不可省**的活（今天为它新增的，别按旧简报做）

1. **新增的 Step 4**：T4 定向复审登记的 6 条逐字替换块（4 条文本级 + 1 条测试强度 + 1 条旧任务号指针）——含把键集合断言补成 `assert_eq!(obj.len(), 7, ...)`，配一条证伪取证：临时往 `SearchBundle` 加一个 `pub docs_capped: bool` 这条**必须红**，然后**删干净**。这道闸是给「M4 不加线格式字段」那条裁定上的，不是装饰。
2. **回收两块临时豁免**：删掉 `unified_bundle` 上方的 `#[allow(dead_code)]`（现在在 `search.rs:308`，按「紧贴 `pub fn unified_bundle` 的那行」定位，行号会漂），**以及 `doc_preview.rs` 文件头那块 5 行的 `#![allow(dead_code)]`**（Task 5 按预检修订开的，形态同形）。门禁 `grep -rn "allow(dead_code)" src/search.rs src/doc_preview.rs` 期望 **exit=1（0 命中）**，一次扫两个文件。
3. **同一个 `doc_preview.rs` 里还要同步一句 doc 注释**（T5 复审 Minor-1）：`PREVIEW_WINDOW_CHARS` 上方那句「单窗最长…」在并窗判据换形后成了假话（链式合并能把一扇窗拉到接近整篇），整段换成计划 Task 5 Step 1 里那份 6 行写法，**逐字照抄、不要自己重述**。这也是为什么简报里那句「除这 5 行之外的任何字节都不许变」已经改口成两件事。
4. **`extract.rs` 多一行字符串**（T5 复审登记的越界残留）：`release_profile_does_not_abort_so_panic_guards_work` 里 `assert_ne!` 的尾句把 `crate::panic_to_err` 改成裸 `panic_to_err`（`f72a5c8` 把边界助手搬进 `extract.rs` 后这个路径根本不存在；T5 只被授权改同文件 `:577` 那句，所以漏到这里）。纯字符串，不许动那个函数体里任何其他表达式。
5. **Step 3 的门从四条变五条**：新增 `grep -rn "crate::panic_to_err\|index_job::panic_to_err" . --include=*.rs --include=*.toml` 期望 **exit=1**。T5 的门只扫 `index_job::` 前缀，所以 `crate::` 那种写法一路绿到今天（坑 60）。
6. Files 里已点名**五个**文件（`lib.rs` / `search.rs` / `doc_preview.rs` / `index_store.rs` / `extract.rs`，后四者只动注释、那一处断言、那 5 行豁免与那 1 行字符串），`git add` 也是这五个 —— 别让实现者以为无权改它们。Step 5 的字面量对账：`grep -rn "\"search_all\"\|\"doc_preview\"" ../src/lib/api.ts` 在 **T7 落地前必然 exit=1**，T7 之后必须 exit=0 —— T7 要重跑这条并写进它的 Expected。

### 4.4 完成判据（计划里的原话，可逐条核对）

- `cargo test --lib` → **134 passed; 0 failed**（分模块：tokenize 9 / extract 15 / index_job 12 / index_store 15 / search 20 / doc_preview 14 / db 8 / index_scan 10 / ledger 12 / project 10 / vault 9）
- `node --test "tests/**/*.test.ts"` → **pass 8 / fail 0 / exit=0**；`npm test` 与 `npm run build` 两条 exit 0
- 两道 clippy（`--lib -- -D warnings`、`--lib --all-targets -- -D warnings`）都 exit 0 且输出里 0 条 warning
- 结构 grep 门禁（**注意方向**：期望 0 命中的那几条，正确结果是 `exit=1`，不是命令失败）：`grep -rn "chars().count() > 128" src/` → exit 1；`grep -rn "crate::panic_to_err\|index_job::panic_to_err" src/`（两种前缀都要扫，见坑 60）→ exit 1；`grep -rn "\.cut(" src/tokenize.rs` → 恰好 1 行；`grep -rn "File::open" src/doc_preview.rs` → exit 1；`grep -rn "\.cut(\|cut_for_search" src/doc_preview.rs` → exit 1；另有一条在 T6 Step 3 而不是完成判据里：`grep -rn "allow(dead_code)" src/search.rs src/doc_preview.rs` → exit 1（就是 §4.3 第 2 条那两块豁免的回收证据）
- `generate_handler!` 条数 = 41；9 个提交、每个只含该任务 Files 点名的文件；`git status --porcelain` 干净
- `%APPDATA%\dev.zero.pfm\` 逐文件与真机基线快照一致；`%TEMP%` 无本轮残留

### 4.5 T9 里已经预先写死的裁定（不用再问人）

沙盒灌 5 万行 `index_docs`（正文 1–2 KB 随机 CJK，全在 `%TEMP%`）后量两件事，只回数字：

- **一次 `search_all` 往返 > 800 ms** ⇒ `DOCS_FETCH_LIMIT` 200 → 100，并在 `docs/开发进度.md` 登记「降到 100，因为 X ms」。
- **触顶簇占比 > 30%** ⇒ `MAX_ITEMS_PER_CLUSTER = 10` 太小，三个数一起上调 20/10/10 → 30/15/15，并同步改 T4 的三条截断测试期望值与 spec §四。
- 两条都没触到就维持原值，只记数字。**不许**为了「看起来更快」偷偷放宽 `truncated`/`hidden` 的语义。
- **预览载荷长度**（T5 复审 Minor-1 的有名缺口，判据也已写死）：沙盒造一份 ≥4 万 char、命中间隔约 8000 char（刻意落进合并带）的长文，`doc_preview` 后只回 `text` 码元长度、`ranges.len()`、`truncated` 三个数字。**`text` > 6 万码元且页面渲染明显卡顿或滚动失效** ⇒ 升级为终审的独立 finding（议题：给合并加长度上限、上限处宁可多插一个 `⋯`）；不卡就记数字收口，不在 T9 里顺手改算术。

收口文档必须含「没能自动验证」清单（逐条点名，不许省略）：JSX 渲染正确性、`revealItemInDir` 那一步要人点、`isNewest` 的 `>=` 变异假绿、`lower_first` 折叠展开型映射（ß/İ）在本机无夹具、纯静默拒绝 WAL 那条分支自 M0 起就无测试守护、5 万行延迟只是沙盒数字、第三级簇序键无守护（M5）、预览「重复段落」症状在重叠带内（4000–8000 char）无专用夹具（只由并窗不变量间接守）、`PREVIEW_GAP` 的字面值本身无守护（T5 复审裁定：它不是线上协议，为常量写断言等于重言式）。

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
22. `package.json` 现在**还没有 `test` 脚本**（只有 dev/build/preview/tauri），`tests/` 目录也还不存在 —— 两者都是 T7 的产出，不是前置条件。

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

### G. 今天（10-08）新加的十一条

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

---

## 6. 指针

| 东西 | 路径 | 关键位置 |
|---|---|---|
| M4 spec（决策权威） | `docs/superpowers/specs/2026-10-07-m4-unified-search-design.md` | 决策表 `20`；契约 `32`；排序与截断 `87`；预览通路 `95`；测试策略 `138`；真机与性能 `161`；**不在本轮 `171`**；风险 `194`（共 199 行） |
| M4 实施计划（执行权威） | `docs/superpowers/plans/2026-10-07-m4-unified-search.md` | Global Constraints `13`；**已实测事实 `37`、条数链 `58`**；文件结构 `62`；T1–T9 `88/240/324/536/1003/1554/1698/1970/2288`；**完成判据 `2355`**；本轮不做 `2368`（共 2376 行；行号随计划修订漂动，用 `grep -n "^### Task"` 现取） |
| SDD 账本（恢复地图） | `.superpowers/sdd/2026-10-07-m4-unified-search/progress.md` | 预检 10 行表 `9-20`；红线 `24`；T1–T4 complete `55/67/81/110`；**T5 派发前预检 `129`、T5 首轮评审裁定 `147`、暂停点 `163`、T5 fix round 1 复审与 complete `171`**（共 186 行，`grep -n "^## " progress.md \| tail -6` 现取） |
| 下一个任务的简报 | `.superpowers/sdd/2026-10-07-m4-unified-search/task-6-brief.md` | 144 行（T5 复审收口后重切），期望 134、handler 41、五个文件 |
| T1–T5 实现者报告 | 同目录 `task-{1,2,3,4,5}-report.md` | T4 报告 391 行（尾部 §八 = fix round 1 与全部变异实测）；**T5 报告 §九 = fix round 1**（原始门禁输出 + `exit=` 行、两格变异读数、7 处还原的 `diff -q` 证据） |
| 评审包 | 同目录 `review-*.diff` | 8 份，命名即 BASE..HEAD |
| 里程碑与证据史 | `docs/开发进度.md` | 里程碑表 `5-20`（**M4 行 `13` 还写「未开始」，T9 收口时改**）；环境结论 `22-45`；M3 验收证据 `70`；M3 已知缺口 `107-112`（**归属待按 spec §十 更正**） |
| 技术方案（更早的权威） | `docs/技术方案.md` | §3.4「不做假的归一化」是 D2/D3 的依据 |
| 上一版交接（10-08 11:33 增量刷新） | `git show 9108c47:docs/HANDOFF.md` | 51 条坑、且 §1–§4 仍是 10-07 的「未开始」状态；本文件已含那 51 条并扩到 62 条 |
