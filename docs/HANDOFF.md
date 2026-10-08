# 交接文档 —— M4 首屏统一检索

**快照时间**：2026-10-07 约 17:40（本地）
**分支**：`feat/m4-unified-search`，HEAD `a44cdac`，`git status --porcelain` 干净
**门禁基线**：`cd src-tauri && cargo test --lib` → **104 passed / 0 failed / exit 0**（暂停前在分支上实跑的数字，不是推算）
**一句话状态**：M4 的 spec 与 9 任务实施计划都已定稿并提交，SDD 执行环境已就绪（分支、账本、预检表、Task 1 简报齐备），**还没有写一行 M4 代码** —— 停在「派发 Task 1 实现者」这一步，被要求暂停。

---

## 1. 我们在做什么任务

做的是本地桌面工具「项目资料管理」（Tauri 2 + React + rusqlite bundled FTS5 + jieba-rs），按里程碑推进：M0 脚手架与 WAL 校验、M1 项目档案与目录映射、M2 信息台账与列加密、M3 全文索引管线都已 ✅ 完成。**M4 = 首屏搜索框统一结果**，做完 M4 就是 MVP（M0→M4）。

M4 本轮交付的形状（权威定义在 spec，不要从本文件重述去实现）：

- **两条 IPC 命令**：`search_all` 返回一个 `SearchBundle`（三段：项目档案 / 信息台账 / 文档正文），`doc_preview(doc_id, query)` 返回窗口拼接的原文 + UTF-16 码元高亮区间。
- **段内按项目聚簇，不跨段合并**：`bm25` 与 `LIKE` 的分数不可比，spec §3.4 明令禁止假归一化。
- **三级截断 20 簇 / 10 条 / 10 平铺，各有计数、只显示不展开**。
- **预览**：词只从 `tokenize::query_terms` 拿（入库/查询同词器是这套检索的地基红线），`ranges` 用码元下标让前端 `text.slice` 零换算。

执行方式是 `superpowers:subagent-driven-development`：每个任务一个全新实现者子代理 + 一个任务评审（spec 合规 + 代码质量双 verdict）+ 修复轮（≤5 轮，1–3 轮 resume 原实现者，4–5 轮换更强模型），全部任务完成后做一次整分支终审。**连续执行，不在任务之间停下来问「要不要继续」**；只有四类情况才停：不可逆/破坏性操作、安全敏感动作、超出本 worktree 的副作用（merge/push/publish）、计划坏到每条前进路径都是猜。

绑定红线（每条派发词里都要带上，违反即返工）：

1. **工具绝不动被索引的原始文件** —— 只登记路径、只读文件。夹具只能在 `tempfile::tempdir()` 与 `db::open_in_memory()` 里造。
2. **真实数据目录 `%APPDATA%\dev.zero.pfm\`（库文件 `ledger.db`）是只读红线**；真机验证一律用一次性 identifier `dev.zero.pfm.m4test`，跑完删净，绝不为验证改 `src-tauri/tauri.conf.json`。
3. **子代理一律不许跑 `tauri dev` / `npm run dev`**；「UI 实际渲染未验证」必须如实写进报告。
4. **密文列不参与任何检索** —— 新增 SQL 里不许出现 `*_cipher` / `*_nonce`。
5. **验证脚本只回断言结果**（布尔 / 条数 / `matchedBy` / 文案前缀 / 尺寸），绝不打印抽出的正文或任何解密内容。
6. **Git：无远端，只本地提交，绝不 push、绝不 amend、不 `--no-verify`、不 `git add -A`**，按文件 name 逐个 stage。输出语言一律中文（错误码、SQL 字面量、JSON key 除外）。

---

## 2. 已经完成了什么

今天 22 次提交，分两段。

### 2.1 上午～午后：M3 收口（已 ✅）

- Task 11（索引状态页）三轮修复：`start` 前复位进度状态、`subscribe` 拒绝挂 `.catch`、`start` 落地后补一次 `load()` 取回 `overview.running`、`start` 后补的那次 `load` 不再擦掉本次点击的拒绝（`3f5332d`、`e3c161a`、`d6a699e`）。
- `index_overview` 改两段式，把文件系统探测移出连接锁（`39a1099`、`a94817c`）。
- 终审整改 C1/I1/I2/I3：panic 边界补齐并**去掉 release 的 `panic=abort`**、`index_store::fts_orphan_rows` 全表不变量（三处断言为 0）、注销/换指根目录时双侧回收索引行（`d8f8062`、`c8faf9b`）。
- 文档按真机实测口径改写（`b5faca6`、`c97a683`、`a093214`、`5b36dac`）。
- 注意：`docs/开发进度.md` 里程碑表里 M3 记的是 **96 passed**，那是 M3 主体验收那一次的实跑数；其后终审与残留整改各补了测试，所以今天基线是 **104**。两个数字都对，不矛盾。

### 2.2 午后：M4 设计与计划（已定稿、已提交）

- **spec 定稿**（`807e3c1`）：`docs/superpowers/specs/2026-10-07-m4-unified-search-design.md`。决策 D1–D7 全部带被否掉的备选与否掉理由；§十把 M3 划过来的账逐条写明为什么不进本轮；§十一列了 4 条风险（其中最硬的一条：5 万行量级的 `LIKE` 全表扫是首屏新引入的每键开销）。
- **写计划前的修订**（`34a48e1`）：`Cluster<T>` 改泛型避免三份重复结构、摘要观感问题定成 `A B AB` 三连折叠、「两处同源」从行为断言改成 grep 文本门禁、`fs_failed` 翻译规则写死。
- **实施计划定稿**（`a44cdac`，2143 行）：`docs/superpowers/plans/2026-10-07-m4-unified-search.md`，9 个任务，每个任务都是「失败测试 → 跑红 → 最小实现 → 跑绿 → 变异取证 → 门禁 → 按名 stage 提交」的可照抄粒度，含 Global Constraints 与「已实测事实」表。
- **计划自评审当场修掉的 5 类缺陷**（这些正是下一个实现者会踩的，已写进正文）：
  1. 条数链第一版凭累加写成 109/120/134 —— 改成逐 target 实数：104 → T1 106 → T2 106 → T3 108 → T4 119 → T5 133 → T6 133 → T7 Rust 仍 133 + `node --test` pass 8。
     **2026-10-08 更正**：Task 4 首轮评审查出「台账段三段的三分之一零守护」，补了第 11 条测试，链改为 …T4 **120** → T5 **134** → T6 **134** → T7 Rust 仍 **134** + `node --test` pass 8（终态 search 由 19 变 **20**，合计 **134**）。教训写进了账本：**凡改一条链，要连同下游每个写死的 Expected 一起改**，计划正文与本文档都算。
  2. Task 5 测试夹具为绕 `&'static str` 快 invent 了一个 `static_str` 助手 —— 统一成 `fn body(text: String)`，9 个调用点全部 `.to_owned()`。
  3. Task 8 的预览对话框把「打开项目」写成 `params={{ projectId: data.docId }}` —— **错**：`docId` 是文档行 id。改成 `Props` 加 `projectId: string | null`、由 `search-bundle.tsx` 从 `cluster.projectId` 传入（不为一个按钮去改 Task 4/5 已定稿的线格式和它们的 10+14 条测试）。同时把 `React.ReactNode` 补成 `import { type ReactNode }`、删掉本项目路由不用的 `search={undefined}`、两个 `DialogFooter` 并成一个。
  4. Task 7 承诺 `pass 7` 但写出来的文件有 8 条测试 —— 5 处 + 链数一起改成 `pass 8`。
  5. spec §五.5 把 `PREVIEW_GAP = "\n⋯\n"` 说成 5 个字符 —— 实际 3 个；`PREVIEW_WINDOW_CHARS = 4000` 补明是**半宽**。
- **执行准备完成**：从 `master@a44cdac` 切出 `feat/m4-unified-search`；SDD workspace 与账本就位；派发前预检（10 行跨任务接口一致性表 + 红线清单）写进账本；Task 1 简报已生成（152 行）；本轮的 4 个 `%TEMP%` 探针目录（jieba / node-ts / ns / qt）已删除。

---

## 3. 当前卡在哪

**2026-10-08 更新（原文写于 10-07 暂停点，进度已往前走，别让下一个读者按旧状态行事）**：Task 1 / 2 / 3 已完整收口（实现 → 双 verdict 评审 → 修复轮 → 定向复审 → 账本 `Task N: complete`），Task 4 首轮实现已提交（`899b58c`）、评审给了 Spec ✅ / Needs fixes，正在跑第 1 轮整改。**没有被技术问题卡住**；唯一需要人点头的两件事是 ① 「结果被取数上限截过」要不要让用户看得见（`docsCapped` 线格式位，控制方本轮的裁定是不加、只在注释与文案措辞上讲实话），② Task 9 的真机点击（原生目录对话框、`revealItemInDir`、窗口 confirm）只能人做。

（以下 10-07 原文保留作历史。）**没有被技术问题卡住** —— 是被要求暂停在「Task 1 实现者尚未派发」这一步。恢复时不需要重新理解上下文，账本和文件都在。

三个明确的尾巴，都不影响正确性、恢复后顺手处理：

1. ~~**任务清单只建到 #34 / #35**~~（10-08 已补齐到 Task 9；#34/#35 之后是 #36–#42）。Task 3–9 的 todo 还没建。恢复地图是账本不是 todo，所以这不是阻塞项；但要么补齐、要么就只靠账本，别混着记。
2. ~~**Task 2–9 的简报未生成**~~（10-08：T2–T6 已按需生成；**计划文本一旦修订就要用 `task-brief` 重新切那一条**，Task 4 就是这么在整改前重生成过一次 467 行的简报）。这是刻意的：简报必须在派发前现生成，提前批量导出后一旦计划文本被修订就过期（`task-brief` 是按任务正文从计划里切的）。
3. **归属口径不一致（这条值得优先记住）**：`docs/开发进度.md` 的 M3 已知缺口第 6–11 条（`107-112` 行）以及 `125` 行都写着「归 M4」，但 spec §十已把同一批（`m-3` 措辞、`m-4` emit 重复终态、`m-5` 外层 `report_failure` 无再上兜底、`m-6` 裸字符串比较判改指、`set_root_dir` 的 `DELETE`+`INSERT` 窄竞态、`ON DELETE CASCADE` 另一半）收回 **M5 / 待真机**。M4 计划里**没有**这些活。下一个读者若按 `开发进度.md` 去 M4 找，会找不到。**Task 9 文档收口时必须把 `docs/开发进度.md` 那几行的归属改成与 spec §十一致**，否则这笔账会在两个文档之间反复弹。

---

## 4. 下一步计划

### 4.1 恢复入口（照抄即可）

```bash
cd /e/zero/demo/2026/project-files-manage
git branch --show-current          # 应为 feat/m4-unified-search
git rev-parse HEAD                 # 见账本最后一节的提交号；已写完的任务都带 `Task N: complete`
cat .superpowers/sdd/2026-10-07-m4-unified-search/progress.md   # 账本第一行认计划路径 = 这张图没串线
# 账本里没有 `Task N: complete` 的任务就是没做过的，从第一个没做过的接着派
bash "C:/Users/zero/.qoder/plugins/cache/qoder-marketplace/superpowers/6.3.0/skills/subagent-driven-development/scripts/task-brief" \
  docs/superpowers/plans/2026-10-07-m4-unified-search.md 1        # 简报（Task 1 已生成过，重复生成无害）
```

同目录下的另两个助手：`scripts/review-package PLAN_FILE BASE HEAD`（生成评审包，BASE 必须是**派发前**记的 HEAD，绝不 `HEAD~1`，否则多提交任务的 diff 会被截掉）、`scripts/sdd-workspace PLAN_FILE`（工作目录路径）。

### 4.2 九个任务的顺序与门禁

| 任务 | 动什么 | 该任务终态 |
|---|---|---|
| T1 | `tokenize.rs`：查询侧唯一切词 `query_terms` + `MAX_QUERY_CHARS` | 106 passed |
| T2 | `panic_to_err` 从 `index_job.rs` 提到 `extract.rs` 做 `pub(crate)` | 106（extract +1、index_job −1） |
| T3 | `clean_snippet` 的 `A B AB` 三连折叠 | 108 |
| T4 | `search::unified_bundle` 三段聚合 + 段内聚簇 + 三个截断计数；`index_store::indexed_project_count` | **120**（评审整改后 search 11+9=20） |
| T5 | 新模块 `doc_preview.rs`：重抽原文 + 窗口拼接 + UTF-16 码元区间 | **134** |
| T6 | 两条 IPC 命令接线（锁内查库 / 锁内取行、锁外抽盘） | **134**，handler 39 → **41**；并回收 Task 4 那行 `#[allow(dead_code)]`（有 grep 门禁） |
| T7 | 前端类型 + `api.ts` + 纯逻辑 `lib/search-order.ts` + `tests/*.test.ts` | Rust 仍 **134**、`node --test` **pass 8** |
| T8 | store 的 seq 守卫、`doc-preview-dialog.tsx`、`search-bundle.tsx`、`pages/search.tsx` 薄壳 | `npm run build` exit 0，不新增单测 |
| T9 | 真机验收 + D6 上限实测裁定 + 文档收口 | 见 4.4 |

每个任务的评审都要 **spec 合规 + 代码质量双 verdict**，缺一律退回。计划正文在执行期间不改；若评审裁定某任务**文本本身**写错，走「改计划本体 → 重新生成该任务简报 → 账本记 `Ruling:` → 派发 fix round」的闭环，不要为此多开一轮评审，也不要让实现者自己发挥。

### 4.3 完成判据（计划里的原话，可逐条核对）

- `cargo test --lib` → **134 passed; 0 failed**（分模块：tokenize 9 / extract 15 / index_job 12 / index_store 15 / search 20 / doc_preview 14 / db 8 / index_scan 10 / ledger 12 / project 10 / vault 9）
- `node --test "tests/**/*.test.ts"` → **pass 8 / fail 0 / exit=0**；`npm test` 与 `npm run build` 两条 exit 0
- 两道 clippy（`--lib -- -D warnings`、`--lib --all-targets -- -D warnings`）都 exit 0 且输出里 0 条 warning
- 四条结构 grep：`chars().count() > 128` → exit 1；`index_job::panic_to_err` → exit 1；`\.cut\(` 在 `tokenize.rs` 恰好 1 行；`File::open` 在 `doc_preview.rs` → exit 1
- `generate_handler!` 条数 = 41；9 个提交、每个只含该任务 Files 点名的文件；`git status --porcelain` 干净
- `%APPDATA%\dev.zero.pfm\` 逐文件与真机基线快照一致；`%TEMP%` 无本轮残留

### 4.4 Task 9 里已经预先写死的裁定（不用再问人）

沙盒灌 5 万行 `index_docs`（正文 1–2 KB 随机 CJK，全在 `%TEMP%`）后量两件事，只回数字：

- **一次 `search_all` 往返 > 800 ms** ⇒ `DOCS_FETCH_LIMIT` 200 → 100，并在 `docs/开发进度.md` 登记「降到 100，因为 X ms」。
- **触顶簇占比 > 30%** ⇒ `MAX_ITEMS_PER_CLUSTER = 10` 太小，三个数一起上调 20/10/10 → 30/15/15，并同步改 Task 4 的三条截断测试期望值与 spec §四。
- 两条都没触到就维持原值，只记数字。**不许**为了「看起来更快」偷偷放宽 `truncated`/`hidden` 的语义。

收口文档必须含「没能自动验证」清单（逐条点名，不许省略）：JSX 渲染正确性、`revealItemInDir` 那一步要人点、`isNewest` 的 `>=` 变异假绿、`lower_first` 折叠展开型映射（ß/İ）在本机无夹具、纯静默拒绝 WAL 那条分支自 M0 起就无测试守护、5 万行延迟只是沙盒数字。

### 4.5 本轮明确不做（别顺手加）

结果分页/展开按钮（要做就得给 `search_all` 加 offset，而 offset 会和「段内聚簇」纠缠）、失效核对写回 `missing`（M5）、`/index` 页 `scanning` 文案（M5）、路径归一化（M5）、图片 OCR（M6）。完整表在 spec §十。

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
8. **rusqlite 取 `usize`**：`from_sql_integral!(usize)` 在 `rusqlite-0.40.2/src/types/from_sql.rs:146`（不是 `src/types.rs`）。换版本前先复核第 1 条那两个源码位置。

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
36. **仓库源文件是 CRLF**：脚本处理要保留换行（Python 侧 `newline=''`），`git` 会刷 `LF will be replaced by CRLF` 警告，属正常，不是有文件坏了。
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
47. **`cargo test --lib <模块名>` 的聚合行不是模块真数**：筛选走全路径子串匹配，`search` 会连 `extract::tests::real_pdf_yields_searchable_chinese_text` 这类同名巧合一起算进去（首轮报出 22 passed，模块真数是 19）。数模块条数要从全量输出按 `^test <模块>::tests::` 行首数，或 `-- --list`。
48. **`#[allow(dead_code)]` 这类临时豁免必须在本里程碑内被一条 grep 门禁回收**（M4 的形态：Task 6 落地后 `grep -rn "allow(dead_code)" src/search.rs` 期望 exit=1）。只写在注释里的「Task N 记得删」等于没人负责。
49. **注释与测试文本也算进 grep 门禁的命中范围**：计划里给实现者的示例注释若含 `unwrap_or(50)`、`, 200)` 这类字面量，会直接把自己的门禁弄红。指针一律写「函数名 + 行号」，不写被门扫描的字面量形态。
50. **数字链一改就要全链同步**：补一条测试 = 该模块 +1、该任务终态 +1、后续每个任务的 Expected、完成判据、交接文档表格与分模块清单全部跟着改。控制方用 `grep -n "旧数"` 收口，别指望实现者发现。
51. **随机 id 让「删除排序键」这种变异变成抛硬币**：`project::create_project` 用 UUID v4（`project.rs:110`），并列簇在去掉名字键后按随机 id 序排，实测 10 次只红 2 次。做排序键的变异取证要用**反转比较方向**，不要用删行；确实无法守护的键（如第三级 id）就写成「预期不红」的取证条目并登记有名缺口，别伪造确定性。

---

## 6. 指针

| 东西 | 路径 | 关键位置 |
|---|---|---|
| M4 spec（决策权威） | `docs/superpowers/specs/2026-10-07-m4-unified-search-design.md` | 决策表 `24-30`；契约 `32`；排序与截断 `87`；预览通路 `95`；测试策略 `138`；真机与性能 `161`；**不在本轮 `171`**；风险 `194` |
| M4 实施计划（执行权威） | `docs/superpowers/plans/2026-10-07-m4-unified-search.md` | Global Constraints `13`；**已实测事实 `37`、条数链 `58`**；文件结构 `62`；T1–T9 `88/240/324/536/1003/1498/1580/1852/2170`；**完成判据 `2236`**；本轮不做 `2249`（行号随计划修订漂动，用 `grep -n "^### Task"` 现取） |
| SDD 账本（恢复地图） | `.superpowers/sdd/2026-10-07-m4-unified-search/progress.md` | 预检 10 行表 + 红线 + 进度节 |
| Task 1 简报 | `.superpowers/sdd/2026-10-07-m4-unified-search/task-1-brief.md` | 152 行，从计划正文切的 |
| 里程碑与证据史 | `docs/开发进度.md` | 里程碑表 `8-20`；环境结论 `22-45`；M3 验收证据 `70`；M3 已知缺口 `107-112`（**归属待按 spec §十 更正**） |
| 技术方案（更早的权威） | `docs/技术方案.md` | §3.4「不做假的归一化」是 D2/D3 的依据 |
