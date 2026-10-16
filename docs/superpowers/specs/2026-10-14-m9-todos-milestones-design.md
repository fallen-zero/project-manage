# M9 设计 —— 待办清单 + 项目里程碑 + 到期预警

**时间**：2026-10-14。**前置**：M8.1 已 fast-forward 合入 `master`（合并点 `d203156`，收口 `3d510d2`）。
**需求原文出处**（不是本文件，也不是技术方案的里程碑表 —— HANDOFF 坑 34）：

- `docs/思维导图.md:38-45`（§二 项目待办清单：独立一级栏目、跨项目统一管理、新增/编辑/删除/改内容、未完成-已完成勾选切换、关联所属项目可按项目筛选、全部/按项目分组/按截止日期排序）；
- `docs/思维导图.md:33-36`（§一.4 项目过程管理 → 项目里程碑记录：立项、评审、联调、上线、验收，可记录日期、备注）；
- `docs/思维导图.md:73`（§四.3 可选迭代增强 → 到期预警（维保、域名、证书到期提醒））；
- `docs/技术方案.md:126` 数据模型草图里那张从未实现的 `milestones 里程碑：阶段、日期、备注`。

## 一、本轮交付与边界

交付三件：**两张业务表（V5 迁移）**、**八条 IPC 命令**、**待办页 + 项目详情里程碑区 + 到期预警块**。

本轮交付的「到期预警」与需求原文那格**口径不同，这是需求方当次改选的结果，不是笔误**：

| | 需求原文（`思维导图.md:73`） | 本轮实际兑现 |
|---|---|---|
| 盯什么日期 | 维保、域名、证书到期 | **里程碑计划日期 + 项目交付截止 + 未完成待办截止** |
| 未做的部分 | —— | **维保/域名/证书那三类到期字段一个也没建**（库里现在只有 `contract_period`、`delivery_deadline`，没有三类到期日列） |

⇒ 这是**缩量**：M11 那格不能因为「M9 做过预警」就当已兑。技术方案 §九 的 M11 行要在本轮回写时点名剩下的三类（见 §十三）。

范围决定：**单批全做**（需求方 2026-10-14 选定，理由：这批全是新功能本体、没有历史债务清账要跟 M5.5 那次 13 项单批同形；而预警要一次写对就得让三来源都在，拆开会逼着回头改刚验收的 SQL 和界面）。

## 二、决策与理由

**D1 预警的判据在前端纯函数，不在 SQL。** 本仓第一次有「按日期语义解释数据」的功能，故意让 SQL 不解释日期：SQLite 的 `strftime('now')` / `date('now')` 是 **UTC**（`backup.rs:139-140` 那句注释是为可移植性故意选 UTC 的），而「维保今天到期」按 UTC 会让本机用户看到**错一天的红/黄**。前端 `new Date()` 天然是本地日历日。先例：`src/lib/search-order.ts` 与 `src/lib/backup-name.ts` 都是「只 `import type`、不 import 运行时值」的纯函数，所以 `node --test` 能直跑、不用给仓库添 jsdom。
**代价（有名登记）**：阈值 7 天这个常量活在前端；以后若 M10/M11 要通知或导出，得把这个函数搬去 Rust 或共享，而不是调一条命令。

**D2 两张表，不合并成一张 `agenda`。** 需求原文里这两件东西分属两格（§二 vs §一.4），字段语义分叉（待办有 `title`/`due_date`、里程碑有 `stage`/`note`/`plan_date`）。合并要靠 `kind` 分支写复合 CHECK，是过早抽象，也与本仓「一表一形制」相反。

**D3 V5 零回填。** 迁移只追加 DDL，不往既有项目插里程碑行。需求方选定「用户逐条加」；本仓至今**没有任何一支迁移写过业务行**，V5 不开这个头。

**D4 `stage` 是 CHECK 枚举，取需求原文那五个词。** 与本仓形制一致（`projects.status` 就是 CHECK 八段）。
**两套词表重叠但不一致，不是漏改**：`status` = 立项/需求/开发/测试/预发/上线/运维/归档（项目当前态），`stage` = 立项/评审/联调/上线/验收（过程节点）。共有「立项」「上线」两个词、语义不同。界面文案不许把它们并排成一组下拉。

**D5 两张表都带 `done`（勾选完成）。** 待办的 `done` 是需求原文（§二「未完成/已完成勾选切换」）。**里程碑的 `done` 是我最初选项没想透、需求方看完翻转后改选的**：立项/评审的计划日期天然都在过去，那是历史不是欠的事，按判据会变成一堆误红把预警格子刷废；加一列勾选之后，判据④（见 §八）对三个来源统一成立，不用给 milestone 写特例。⇒ 这一列需求原文没有，是有代价的加法，换掉的是「预警撒谎」。

**D6 待办不要 `note` 列。** 需求原文只有「待办内容」，`title` 一列承载；`note` 是控制方在设计 1/2 里多加的，需求方当场删掉。里程碑保留 `note`（原文 §一.4 明写「可记录日期、**备注**」）。

**D7 零新错误码。** 设计 1/2 里我写过「新码 `todo_bad_date` / `milestone_bad_date`」——**这与仓内形制相反，已改回**：同族的 CRUD 一律复用共享码 `invalid_input`（`ledger.rs:15`）与 `not_found`（`ledger.rs:19`），只有故障域才另立码（`backup_*`/`vault_*`/`index_*`/`db_*` 那 40 个是故障域，不是输入校验）。待办的日期形状不对是**输入校验**，不是新故障域 ⇒ `invalid_input` + 一句 hint「日期要用 YYYY-MM-DD」。

**D8 软删项目不级联、靠过滤隐藏。** `PRAGMA foreign_keys=ON` 是开着的（`db.rs:56`，`db.rs:338` 有测试注释专门记这件事），但 `delete_project` 是软删（`UPDATE projects SET deleted_at`，`project.rs:163-173`）⇒ 表上的 `ON DELETE CASCADE` 对现有五张台账表**从来没触发过**。新表照同一形制：删项目 ⇒ 待办与里程碑的行留着、从所有跨项目视图与预警里消失，靠查询里的 `p.deleted_at IS NULL`（先例 `index_job.rs:129`、`doc_preview.rs:77`、`index_store.rs:212`）。项目恢复即原样回来。**级联软删不做的理由**：恢复项目时无法区分「本来就被用户删掉的待办」和「随项目隐掉的待办」，而且 M8.1 刚定过同族口径（撤销包只加可见性、不自动删改数据）。

**D9 新表不进 `backup::Counts`。** `Counts` 是 7 字段 camelCase 线格式（`backup.rs:39-47`）、`read_counts` 逐表现数（`:125`）。新表照样进包（备份是整库快照，自动跟着走），只是摘要不数它们。需求方当次批准不进。
**为什么不进的真理由不是「省事」**：旧包（M8/M8.1 导出的那批）的 manifest 里没有这两项，补字段只能给 `#[serde(default)]` ⇒ 显示 `0`，而「这个包里有 0 条待办」对一份装着 12 条待办的旧包是句**假话** —— 与 M8 终审 I1（库已覆写之后 `vault_status` 继续报 `unlocked: true`）同族的界面撒谎。要诚实地进就得 `Option<i64>` + 「未记录」文案，或 `formatVersion` 升 2，两者都比本轮要换来的多。
⇒ 这一条就是 HANDOFF §4-B ①要求的「显式写明为什么不进」。

**D10 待办/里程碑的写不套 `not_indexing` 闸门。** 那扇门（M8 为 `backup_import` 加的）挡的是整库覆写这种大动作；待办写是单行短事务，与 `ledger_env_create` 同量级，后者没有闸门。

**D11 命令面 45 → 53，本仓第三次批准破例**（前两次：M7 `libreoffice_install` **41→42**、M8 备份三条 **42→45**）。命名照 `ledger_env_*` 那族；函数体一律同步 `#[tauri::command] fn`（照 `project_create`/`ledger_env_create`），**不碰 `tauri::command(async)` 那 4 条**（`MutexGuard` 与 `rusqlite::Connection` 都不是 `Send`，见 M8 spec D11）。结构门脚本与文档里所有按 45 取数的地方本轮一起改。

**D12 勾选完成不单开命令**，走 `todo_update` / `milestone_update` 的 `done` 字段。少一条命令、少一处「`done` 有两处写点」的账。

## 三、数据模型（V5）

`db.rs`：`TARGET_VERSION` 4 → **5**，`apply_version` 的 match 加 `5 => V5`，DDL 常量 `const V5: &str` 照 V3/V4 形制（`CREATE TABLE` **不写** `IF NOT EXISTS`，靠一版本一事务；重放同版本的现状语义不变，`db.rs:134` 那条注释已说明）。

```sql
CREATE TABLE todos (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    due_date   TEXT,
    done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0,1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at TEXT
);

CREATE TABLE milestones (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    stage      TEXT NOT NULL CHECK (stage IN ('立项','评审','联调','上线','验收')),
    plan_date  TEXT,
    note       TEXT NOT NULL DEFAULT '',
    done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0,1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at TEXT
);

CREATE INDEX ix_todo_project ON todos(project_id);
CREATE INDEX ix_milestone_project ON milestones(project_id);
```

四列设计账：

1. **`due_date` / `plan_date` 可空**。需求的「按截止日期排序」与「可记录日期」都不要求必填；空 = 不参与预警、排序落最后。
2. **形状是承重判据，不是格式洁癖**：预警与排序全靠 `YYYY-MM-DD` 的字典序 == 时间序。一条 `2026-10-14` 混进去**不会报错**，但会静默排到错位置 —— 正是「预警少亮一条」的成因。⇒ 写入侧硬校验（见 §五），录入侧 `<input type="date">`（浏览器强制 ISO 形状）。
3. **`done INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0,1))`**，出库 serde 成 `bool`（先例：`ledger.rs:47` `username_set: bool` → `usernameSet`；`IndexOverview.ocr_available` → `ocrAvailable`）。
4. **`id` 用 `uuid::Uuid::new_v4().to_string()`**（`ledger.rs:350` 等五处先例），零新依赖（`uuid` 已在树里）。

## 四、命令契约与结构门

| 命令 | 签名 | 返回 | 说明 |
|---|---|---|---|
| `todo_list` | `(state, project_id: Option<String>)` | `Vec<Todo>` | `None` = 跨项目全部。一律 `AND t.deleted_at IS NULL` + `JOIN projects p … AND p.deleted_at IS NULL` |
| `todo_create` | `(state, input: TodoInput)` | `Todo` | `TodoInput { project_id, title, due_date }` |
| `todo_update` | `(state, id, input: TodoEdit)` | `()` | `TodoEdit { title, due_date, done }`，**不含 `project_id`**（照 `ledger_env_update` 的先例：`EnvInput` 里就没有 `projectId`，子表 update 不改归属）；勾选完成走这里的 `done` |
| `todo_delete` | `(state, id)` | `()` | 软删，二次删返回 `not_found`（照 `soft_delete`，`ledger.rs:207`） |
| `milestone_list` | `(state, project_id: Option<String>)` | `Vec<Milestone>` | 同 `todo_list`，`None` 供预警用 |
| `milestone_create` | `(state, input: MilestoneInput)` | `Milestone` | `{ project_id, stage, plan_date, note }` |
| `milestone_update` | `(state, id, input: MilestoneEdit)` | `()` | `MilestoneEdit { stage, plan_date, note, done }`，同样不含 `project_id` |
| `milestone_delete` | `(state, id)` | `()` | 软删 |

线格式（`#[serde(rename_all = "camelCase")]`，`deleted_at` 不出库）：

```
Todo      { id, projectId, title, dueDate: Option<String>, done: bool, createdAt, updatedAt }
Milestone { id, projectId, stage, planDate: Option<String>, note: String, done: bool, createdAt, updatedAt }
```

结构门（本轮终态，**全部现取不写死**）：`generate_handler!` **53**、`tauri::command(async)` 仍 **4**、`TARGET_VERSION` **5**、全仓 `dead_code` 豁免仍 **1**（`db.rs:30`）、`Cargo.lock` **零新 `[[package]]`**（零新依赖）、`SUPPORTED`/`index_*` 等检索门不受影响。

**偏差登记（终审 N10，代码不改）**：`milestone_list` 的实测 `ORDER BY` 是 `plan_date IS NULL, plan_date, created_at, id`，而上面那张表写的是「同 `todo_list`」⇒ 它比 `todo_list` 多排了「计划日期升序 + 空日期落最后」。这一形更合理（面板不必自己再排），但**没有单独授权**，故在此记一句偏差而不改代码。两张表末尾那个 `id` tiebreaker 是终审 N3 补的（`datetime('now')` 只到秒，同秒建的行原先全靠 rowid 给序），两侧各有一条多行次序断言守着，摘掉即红。

## 五、失败面（写入侧五道，全在 Rust）

按顺序，每道都有正反两支测试（判据一律写成**吃参数的纯函数**，这样命令层才有可测落点 —— M8 终审 C1 的教训③）：

1. **`project_id` 必须指向未软删的项目**：照 `project_exists` 形制（`ledger.rs:218`，`COUNT(*) … AND deleted_at IS NULL`），命中 0 ⇒ `not_found`。新模块各留一份私有 `invalid`/`not_found`/`project_exists`（本仓 `project.rs` 与 `ledger.rs` 已经各有一份 `not_found`，不为三行 helper 造共享缝）。
2. **`title` 非空**：`trim` 后空串 ⇒ `invalid_input`（照 `ledger.rs:23` 的 `norm`）。
3. **`stage` 白名单**：应用层先判五个词 ⇒ `invalid_input`（否则落到 SQLite 的 CHECK 会变成 `db_failed`，用户看不出是自己填错）。CHECK 仍留着，它是「绕过命令直写库」那层的持久门。
4. **`due_date`/`plan_date` 形状**：`fn norm_date(v: Option<String>) -> AppResult<Option<String>>`：`None` 或 trim 后空串 ⇒ `NULL`；否则必须匹配 `^\d{4}-\d{2}-\d{2}$` ⇒ 不匹配 `invalid_input`（hint「日期要用 YYYY-MM-DD」）。**作用域要说准**：这一道只覆盖 V5 新表那两列，**不覆盖 `projects.delivery_deadline`**（既有列、既有录入路径，本轮不动它，见 §十二 风险 8）。**不做历法有效性判断**（`2026-13-45` 这种形状对、历法错的输入不在本轮：SQLite 的 `julianday` 会回 `NULL` 而不是报错，加这层校验要引第三方历法判断，换来的收益小，登记为残余 §十二 风险 5）。
5. **`done` 只吃 0/1**：serde `bool` 已在类型层挡掉非布尔输入，CHECK 挡直写库。

读侧无失败面（列表空就是空）。

## 六、前端：预警纯函数与三视图

**`src/lib/due-warning.ts`**（只 `import type`，可被 `node --test` 直跑）：

```ts
export type DueKind = "todo" | "milestone" | "deadline";
export type DueState = "overdue" | "soon";
export interface DueItem {
  kind: DueKind;
  id: string;            // todo/milestone 的行 id；deadline 那一类用 project id
  projectId: string;
  projectLabel: string;  // 「项目名（编号）」，编号缺就只写名
  label: string;         // 待办 title / 「上线」阶段名 / 「交付截止」
  date: string;          // YYYY-MM-DD
  state: DueState;
  days: number;          // 距今天数：overdue 为负，含今天为 0
}
export const DUE_HORIZON_DAYS = 7;
export function buildDueWarnings(
  input: { todos: Todo[]; milestones: Milestone[]; projects: Project[] },
  today: string,                       // 调用方传本地 YYYY-MM-DD，函数不读机器时钟
  horizonDays?: number,
): { items: DueItem[]; unparsed: number }
```

判据（每条都能被固定 `today` 证伪）：① 只算日期非空的行；② 日期不匹配 `^\d{4}-\d{2}-\d{2}$` 的行**跳过并计入 `unparsed`**，界面写「N 条日期无法解析」—— 响亮而不是静默（M8.1 ④b 同口径）。**这条不是纵深，对 `deadline` 类它是唯一防线**：`projects.delivery_deadline` 从来是自由文本（写入只走 `project.rs:125` 的 `normalize`，那只 trim 不校形状；界面是 `project-form.tsx:27` 那一格普通 input，placeholder 写着 `2026-06-30` 但不强制），库里完全可能躺着「10月底前」「Q4」这类值，而预警比的是字典序 —— 那种值参与比较会**静默排错**，不是报错。对 `todos`/`milestones` 两表它才是纵深（§五 第 4 道已在写入侧挡掉）；③ **`todo`/`milestone` 两类过 `done` 判据，勾掉的算完成、不进预警**；④ `date < today` ⇒ `overdue`，`today <= date <= today + horizonDays` ⇒ `soon`；⑤ `deadline` 那一类只取项目的 `deliveryDeadline`，且**跳过 `status === "归档"` 的项目**（归档 = 不再催事；这一类没有 `done` 勾选态，用「未归档」当它的等价门）。

**本文件实际导出的东西（终审 I2/I1 落地后补全，此前只写了 `buildDueWarnings` 一件）**：

- `dueStateOf(date, today, horizonDays?)` —— **判据④ 的唯一实现**（返回 `{ state, days }`，形状不对或超出 horizon 返回 `null`）。`milestone-panel.tsx` 的那个徽标转调它，不再自己写第二份比较：原先面板写的是「字符串 `m.planDate < today` + 自备 `shiftDate`」的第二份实现，把那里的 `<` 改成 `<=` **谁也不会红**（T7 变异 M⑥ 打不红就是这条的读数）；共用之后它落进 `node --test` 的射程（新读数见 `.superpowers/sdd/2026-10-14-m9-todos-milestones/mutation-evidence.txt`）。**这一改推翻了执行期「不合并两处实现」的裁定**（那条只对「不为一个小徽标去跑三来源汇总」成立，终审把「判据」这一半翻回来了）。副产物：形状对但历法错的值（`2026-09-45`）过去是「页面判黄、面板判红」的真分叉，现在两处同判 `soon`。
- `scopeDueInput(input, projectId | null)` —— 把 `todos/milestones/projects` **三份一起**按项目筛小（终审 I1）。预警块与「共 N 条未完成」那行原先吃的是未筛选的全量，只有列表行走过 scoped ⇒ 筛到甲项目仍看见乙/丙的红黄与全局 `unparsed`。抽成纯函数不是为了复用，是为了让这一格有可红的断言（页面本身只有 `tsc` + `vite build` 两道门，没有组件测试网）。
**输入即已过滤**：软删项目/软删行在 SQL 侧挡掉（§四），前端**不再重复判**（这条防线只有一处，见 §十二 风险 3）。
排序：`overdue` 在前，再按 `date` 升序，再按 `projectLabel` 升序（稳定）。

**界面落点**：

- `src/components/app-shell.tsx:8-13` 的 `nav` 数组加第 5 格 `{ to: "/todos", label: "待办" }`；
- `src/router.tsx`：新 `todosRoute`（照 `backupRoute` 那三行形制），加进 `routeTree.addChildren`（`router.tsx:42`）；
- `src/pages/todos.tsx`：顶部预警块（红/描边两组 + 计数 + `unparsed` 那句）→ 视图切换（全部 / 按项目分组 / 按截止排序，纯前端排，先例 `search-order.ts`）→ 项目筛选下拉（数据来自 `listProjects()`）→ 新增/编辑/勾选/删除；**预警块、`unparsed` 那句与「共 N 条未完成」都吃 `scopeDueInput` 筛过的三份**（终审 I1：筛到某项目时不许还看见别项目的红黄）；
- `src/pages/project-detail.tsx`：加里程碑区（列该项目 `milestone_list`、**增 / 删 / 勾选 `done`**、命中预警的行标色）。
  **这里刻意没有「改」的界面（终审 I4 裁定，不许留沉默）**：**改 = 勾选完成**（面板上唯一的「改」就是翻 `done`，走 `milestone_update`）；**纠正一个错值 = 删了重建**（记错日期或挑错阶段就删掉那行再记一笔）。这与 §十一「改归属只能删重建」同口径，也和 `todos` 编辑表单不给改归属一致。⇒ `MilestoneEdit { stage, plan_date, note, done }` 里**只有 `done` 有界面消费者**，那三个字段今天仅由 Rust 单测消费（`milestone.rs::update_sets_done_without_touching_the_project`）：命令面按线格式送全字段、界面只用其中一格，是有意的取舍而不是漏做。
  **黄档那一档的实际颜色是 `outline`（描边），不是 amber/warning**（仓内没有该设计令牌）⇒ 面板与待办页两处文案一律写「红=已逾期，描边=今天起 7 天内」（终审 I5：不新增设计令牌，改文案）。
- `src/lib/api.ts`：八个 camelCase 封装（照 `listProjects`/`createEnv` 那族的 `invoke<T>("命令名", { … })` 写法，注意 Tauri 的 JS 参数是 camelCase、Rust 侧参数 snake_case，照现成写法别自创）；
- `src/types/todo.ts`、`src/types/milestone.ts`（照 `src/types/backup.ts`）。

## 七、落点（按文件）

```
src-tauri/src/db.rs          + V5 常量、match 加 5 => V5、TARGET_VERSION 5、迁移测试
src-tauri/src/todo.rs        新模块：Todo/TodoInput + list/create/update/delete + 五道校验 + 模块测试
src-tauri/src/milestone.rs   新模块：Milestone/MilestoneInput + 同族四操作 + stage 白名单 + 模块测试
src-tauri/src/lib.rs         八条 #[tauri::command] fn + generate_handler! 45→53
src/lib/due-warning.ts       新纯函数 buildDueWarnings + DUE_HORIZON_DAYS
src/lib/api.ts               八个封装
src/components/app-shell.tsx nav 第 5 格；src/router.tsx todosRoute
src/pages/todos.tsx          新页：预警块 + 三视图 + 筛选 + CRUD
src/pages/project-detail.tsx 里程碑区
src/types/todo.ts / milestone.ts  两个线类型
tests/due-warning.test.ts    node --test 覆盖判据①-⑤与排序
```

## 八、判据（本轮「做对了」的定义）

1. `cargo test -j 1 --lib` 全绿，`npm run build`（含 `tsc`）`error TS` = 0，两条 clippy `exit=0`，`node --test` 全绿。
2. 预警红/黄**能被证伪**：`buildDueWarnings` 吃传入 `today`，测试喂 `2026-10-14` 常量，逐档边界各有断言（昨天、今天、第 7 天、第 8 天）。
3. `p.deleted_at IS NULL` 这条过滤有专属断言，且**删掉那个条件测试当场红**（变异取证；照 `index_job.rs:1218-1225` 那段「为什么这条不许省也不许写反」的注释形制写）。
4. 日期形状校验有正反两支：`2026-01-05` 收、`2026/1/5` 与 `2026-01-05` 带尾空格拒。
5. 阶段枚举拒词表外的值，且**拒的是应用层那条 `invalid_input`**，不是落到 `db_failed`。

## 九、测试策略（三档，聚合数由实跑数出，不许凭印象累加 —— HANDOFF 坑 5）

**档一 · Rust 单元**（`db.rs` + 两模块 + `lib.rs`）：V5 迁移（`TARGET_VERSION=5`、两表建出、两个索引在、CHECK 拒非法 `stage` 与 `done=2`、FK 拒不存在项目）；CRUD 九格（必填、trim、软删后 list 不见、update 改 `done`、二次删 `not_found`、跨项目 list 的软删项目过滤）；`norm_date` 正反；`stage` 白名单正反。**每格 Expected 由 `cargo test -j 1 --lib` 的 `test result` 行现数**。
**档二 · node 纯函数**：`tests/due-warning.test.ts` 覆盖判据①-⑤ + 排序稳定 + `unparsed` 计数（含「`deadline` 类填『10月底前』这种非 ISO 值 → 不亮但计数 +1」这一支，它是那条判据的**唯一**可证伪落点）+ `status === "归档"` 的项目交付截止不进预警 + `horizonDays` 传参（默认 7 与显式 0 各一支）。
**档三 · 真机 CDP**（见 §十）。

**变异取证清单**（写完必须逐条实测打红，跑完 `cp` 还原 + `cmp` 验字节一致 + 复跑全量）：① 摘 `p.deleted_at IS NULL`；② 摘 `norm_date` 的正则判据；③ 摘预警里 `done` 那一条；④ 把 `state` 的 `<` 改成 `<=`（今天该黄却变红）；⑤ 摘「跳过 `status === "归档"`」那一条。
⇒ 注意 D1 的结构性后果：**判据①-⑤ 的变异全在 `src/lib/`，只有 `node --test` 能红，`cargo test` 一律不动**。别拿「cargo 全绿」当这套判据有牙的证据（坑 35 同款：防线换了位置，取证也要换地方）。

## 十、真机验收（沙盒 `dev.zero.pfm.m9test`，一次性 identifier，跑完删净；真实目录只读；子代理不跑 dev，控制方执行；CDP 走 `__TAURI_INTERNALS__.invoke`）

本轮**零原生对话框** ⇒ 这一格比 M8 好验，主承诺能端到端驱动：

1. 开工前后各一次真实库三文件（`ledger.db/-wal/-shm`）的 `size+mtime+sha256` 快照，逐字一致且**无新增文件**（红线，坑 9）。
2. 建两个项目、各加两条待办（一条填未来 3 天、一条填昨天）、项目详情页加两条里程碑（一条「上线」+3 天、一条「立项」+ 过去日期）。
3. `/todos` 预警块读数：黄含「上线（3 天内）」与待办（3 天内），红含昨天那条待办；**未勾选的历史里程碑（如已过 40 天的「立项」）本来就该亮红**——判据④ 就是这么定的，**D5 的兑现点是「勾掉 `done` 之后它从预警消失」**，不是「过去的自动不亮」。（本条原文写的是「过去日期的里程碑不亮红」，是控制方写规格时的判据文字错误；真机按原话打成了「NO(误红!)」，代码无错，2026-10-14 按实测读数改准。）
4. 勾掉那条「立项」⇒ 预警不出现；勾掉未来的「上线」⇒ 黄色项当场消失。
5. 三视图切换读数（全部 / 按项目分组 / 按截止排序），`dueDate` 空的行落最后。
6. 筛选某一项目 ⇒ 预警块同步只剩该项目。（**2026-10-14 终审修复波的实态**：这一格在代码里本就没兑现 —— 预警块与计数吃的是未筛选的全量，只有列表行走过 scoped ⇒ 已按 I1 修成 `scopeDueInput` 三份一起缩，并补了 `node --test` 那侧的断言；**页面接线本身仍无自动化网**（只有 `tsc` + `vite build`），真机也从未跑到这一格 ⇒ 登记在 `docs/开发进度.md`「没能自动验证」。）
7. 删除其中一条待办 ⇒ 列表与预警同时消失；软删整个项目 ⇒ 它的待办与里程碑从预警消失，`todo_list` 跨项目也读不到，但**库里行还在**（D8 的兑现点：直查 `SELECT COUNT(*) FROM todos WHERE project_id=…` 非 0）。
8. 把日期字段手填成 `2026-10-14` 形状（走 `invoke` 绕开 `<input type="date">`）⇒ 界面显示 `invalid_input` 那句 hint。
9. 底栏 `schema=5`、`journal_mode=wal`。
10. 收尾：沙盒目录与 `%TEMP%` 夹具删净，进程按 PID 精确回收（坑 1/28）。

## 十一、不在本轮

- **维保/域名/证书三类到期字段**（需求原文 `:73` 括号里那三类，一个也没建）⇒ 仍归 M11，回写时要点名。
- **待办的正文不进任何检索通路**：本轮不加进 `search_local`（库内字段检索，`思维导图.md:34` 那格列的是项目名/客户/网址/备注/联系人/日报内容，**没有待办**），也不进全文索引 `index_docs`（那张表按磁盘文件建行，待办是库内行）。⇒ 「待办内容能不能被搜到」在 M9 的答案是**不能**，需求原文没要求，别当漏做，也别顺手加。
- **待办/里程碑改归属**（update 的 input 不含 `project_id`，照 `EnvInput` 先例）。要挪就删了重建；登记在这里，别当 bug 报。
- **里程碑的「改」没有界面**（终审 I4 的裁定，正文在 §六 那条界面落点里）：**改 = 勾选完成**，**纠正错值 = 删了重建**。`MilestoneEdit` 的 `stage/plan_date/note` 三个字段只有 Rust 单测消费、无界面消费者。不补内联编辑不算漏做，补了才是偏离（与本节的「改归属只能删重建」同口径）。
- **「按截止排序」里「勾掉的沉到最后」这一条是超出项**（终审 N2 认账）：需求原文（`思维导图.md:38-45`）只说「按截止日期排序」，spec §六 的排序判据也没写这一条 —— 它是 `src/lib/todo-view.ts` 的 `sortTodos` 在实现期多出来的行为。裁定：**保留行为**（改产品语义的代价比认账大），但补上真正能红的那条断言（`tests/todo-view.test.ts` 里加一行 `done: true` 且截止日期最早的行；原来四行全 `done: false`，那条分支是死代码，终审数出来的）。
- **不把 `project-form.tsx:27` 那格「交付截止」换成日期控件**：那是 M1 的表单契约，且旧值里非 ISO 的那批换了控件也显示不出来。本轮只在预警侧「解析不了就跳过并计数」（见 §十二 风险 8）。
- 待办重复/提醒推送/通知（个人单机工具，无定时器，产品边界同 M0–M8）。
- 父子项目关联、`思维导图.md:45`（M11）、多工作区隔离（M11）、操作日志（M11）。
- 待办/里程碑导出（`:41` 属 M10 的 Excel 那格）。
- 跨机器同步与多人协作（M1 当年已定「只留软删列」）。
- `milestones` 与 `projects.status` 的词表统一（D4，两套词各有各的语义，不改）。

## 十二、风险

1. **预警口径提前从 M11 挪进 M9，且挪的是「不同的日期集合」**（§一 那张表）。最大风险不是代码，是**下一个对话以为 M11 那格已兑**。⇒ 靠 §十三 的回写挡。
2. **判据活在前端，没有后端契约**。以后加通知/导出/别的页要复用同一判据时，得搬函数或共享，而不是调命令（D1 的代价）。
3. **`p.deleted_at IS NULL` 只有一处防线**（后端 list），漏改就沉默地把已删项目的待办放回预警；而预警块又是「少亮一条」比「多亮一条」更难被发现的那类。⇒ 靠 §八 判据 3 的变异取证挡，别只靠断言写着。
4. **两套枚举词表共有两个词**（D4）。界面若把 `projects.status` 的下拉与 `milestone.stage` 的下拉做成同一组件复用，会静默地给出错词表。⇒ 两个下拉各自取各自的来源，`project_status_options`（现成命令）**不许**喂里程碑。
5. **历法无效的日期形状能过、值不合法**（`2026-13-45`）：形状正则收下，`date('now')` 类比较会得 `NULL`，预警里那一行静默不算。登记为有名残余（本机无历法库，引 `chrono` 要破「零新依赖」这条本轮硬门）。
6. **`Counts` 不含新表** ⇒ `/backup` 界面看不到待办/里程碑行数，用户看不出包里到底有没有这两档（D9 是当次批准的取舍，不是漏做）。要验就导一份包、`backup_inspect` 之后直查包内 `backup.db` 的两张表行数。
7. **命令面 53**（D11 第三次破例）。按 45 取数的脚本/文档本轮一起改，漏改会把「破例」读成「漏注册」（坑 21）。
8. **「交付截止」那一格是自由文本**（`project-form.tsx:27` 是普通 input，写入只 `normalize` 不校形状，见 §六 判据②）⇒ 真实库里完全可能已有「10月底前」这类值，它们在预警里**不亮**（只进 `unparsed` 计数）。后果是用户填得越随意、红/黄越少，而「少亮一条」比「多亮一条」难发现。本轮的处置是**计数并写明**，不治本（治本要动 M1 表单契约 + 迁移历史值）。

## 十三、要回写与要更正的文档

- `docs/技术方案.md`：§四 数据模型那两行（`milestones` 由「从未实现」改为已实现、补 `todos` 一行）；§九 里程碑表 M9 行 + **M11 行要点名「到期预警已提前在 M9 兑现 milestone/待办/交付截止三类，维保/域名/证书三类仍未做」**；§6.2 备份口径复核一句「新增两张业务表进包但不进 `counts` 摘要」（D9）。
- `docs/开发进度.md`：M9 表行 + 「## M9 验收证据」节（含真机十格读数与「没能自动验证」清单）+ 变更日志。
- `docs/HANDOFF.md`：§0/§3/§4 改到 M9 收口态；§4-B 那份「M9 必撞四条不变量」要更新为**实际撞上的结果**（① `Counts` 已裁定不进、② 新表自动进包已复核、③ nav 第 5 格已破、④ 命令 45→53 已批准）；坑 35 那条「新加的靠前校验会遮蔽旧断言」在本轮的同族是 D1（判据搬前端 ⇒ `cargo test` 不再守住它），要在坑里补一句。
- `docs/思维导图.md`：**不改**（它是需求原文，不是实现账）。
