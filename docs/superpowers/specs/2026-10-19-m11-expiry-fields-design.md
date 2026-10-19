# M11-a 设计：到期事项（维保 / 域名 / 证书 / 其他）+ 到期预警的后半

**状态**：需求方 2026-10-19 逐题拍定（D1–D7 各一问 + D8 命令数破例随设计 1/2 确认），设计 1/2 与 2/2 均已逐段过目。本文件是**规格**，不含实施计划；计划另由 `writing-plans` 落 `docs/superpowers/plans/`。
**基线**：`master` = 收口文档提交，M0–M10 全部合入，**本轮开工前的实测读数**为 `cargo test -j 1 --lib` **301 passed**、`node --test` **41/41/0**、结构门 `generate_handler!` **60** / `tauri::command(async)` **7** / `TARGET_VERSION` **6** / 全仓 `allow(dead_code)` 豁免 **1**（`db.rs:30`）/ `Cargo.lock` `[[package]]` **616**。这些数会漂，动手前用 `docs/HANDOFF.md` §4-A 那串命令现取。

## 一、需求出处与本轮兑现范围

需求原文只有一句：**`docs/思维导图.md:73`「到期预警（维保、域名、证书到期提醒）」**（§四.3 可选迭代增强）。M9 当轮是**缩量兑现**，这一点 M9 spec §一 的对照表已明写（`docs/superpowers/specs/2026-10-14-m9-todos-milestones-design.md:18-24`）：

| | 需求原文 | M9 实际兑现 | M11-a（本轮） |
|---|---|---|---|
| 盯什么日期 | 维保、域名、证书到期 | 里程碑计划日期 + 项目交付截止 + 未完成待办截止 | **补上前半：新建一类「到期事项」，四类 kind 进预警** |
| 未做的部分 | —— | 那三类到期字段一个也没建（库里只有 `contract_period` 自由文本与 `delivery_deadline`） | **本轮把它建掉，M9 的欠账在这一格结清** |

本轮名字带 `-a`，因为 §四.2「导出 Excel/Markdown **台账**文件」那半格仍推在后面（见 §十一），两格不绑：台账导出里躺着账号密码密文列，红线要单独定。

## 二、需求方裁定的八格口径

- **D1 数据形状 = 一张表、一行一件事**。新表 `project_expiries`，`kind ∈ 维保/域名/证书/其他`，一个项目可以有三行域名、两张证书。选它的理由是真实形状：`ledger_envs` 早就是「一项目多行网址」，`projects` 上再加三个日期列则一类只能存一条、多域名场景只能塞备注。形制照抄 M9 的 `milestones`。代价（当次摊开过）：4 条新命令 + V7 迁移 + 一个面板。
- **D2 续完之后 = 改日期，不留勾选态**。表里**不放 `done` 列**。这一行的语义是「这个东西的有效期到 X」，不是「一件事」；续期就是把 X 改成新年份，预警当场重算；不再生效的东西走软删。好处是永远只有一个日期是真的，不会出现「勾了完成但旧日期还挂着」；明写的代价：**看不到哪一年续过**（要历史得另开一行，本轮不做）。
- **D3 窗口 = 这三类单独 30 天，其余三类沿用 7 天**。待办 / 里程碑 / 交付截止仍 `DUE_HORIZON_DAYS = 7`（[src/lib/due-warning.ts:7](../../../src/lib/due-warning.ts)）；到期事项用 `EXPIRY_HORIZON_DAYS = 30`。理由：域名与证书续费要走合同与审批，7 天窗口等于到期当天才亮红、那天已经晚了。成本几乎为零，因为 `dueStateOf(date, today, horizonDays)`（同文件:66）本来就把窗口当参数。
- **D4 归档项目照常亮**。归档只表示管理上退役，而域名 / 证书 / 维保是还在计费的客观事实；政务项目验收转运维恰恰是最容易漏续的一批。这与 M10 D6（归档项目的日报照常列、照常算工时）同形。代价：预警块会出现归档项目的条目，**行尾必须念出「（归档项目）」**，不能让人以为是录错了。
- **D5 落点 = 项目详情页面板 + `/todos` 顶部预警块加第四类**。增删改在面板，提醒进预警块；**M9 定的「预警不在首屏」不变**，项目列表页不加角标。
- **D6 名称与备注进全局检索**。与需求原文 `:62`「关键词全局搜索匹配项目名、客户、网址、备注…等」和 M10 D4 同形；能回答「oa.gov.cn 这个域名在哪个项目」。
- **D7 `kind` = 封闭四类（三类 + 「其他」）**。加「其他」的理由是真实的：ICP 备案、等保测评、软件订阅、服务器续费这些到期物不属于那三类，没「其他」它们会被硬塞进证书或干脆不录，而录了才进得了预警。代价：以后要第五类得改迁移（不是本轮的事）。
- **D8 批准 4 条新 IPC 命令，`generate_handler!` 60→64**（本仓第 5 次批准破例；前四次是 M7 的 42、M8 的 45、M9 的 53、M10 的 60）。理由与 M8/M9 同处境：新增一类一等公民数据，没有现成宿主命令可挂。

**控制方自拍、并在设计呈现中一并确认的三条**：① `ExpiryUpdate` **只有 `expireDate` 与 `note`** 两个字段（面板只提供这两件事，按界面反推接口，不留「有字段没消费者」的沉默格 —— M9 终审 I4 就是这一格）；② 命令命名 `expiry_*` 而非 `domain_*`/`cert_*`（一类命令管四种 kind）；③ `expire_date` **必填**（比 `milestone.plan_date` 的 `Option` 紧一档：一行到期事项没日期就没有意义）。

## 三、本轮核实到的「树上免费」能力

这一节的存在是为了让下一轮不要重新论证：

- **预警的共用判据件已经在了**：`dueStateOf` 是 M9 终审 I2 从两处比较里抽出来的单份（[src/lib/due-warning.ts:59-75](../../../src/lib/due-warning.ts) 的注释写明了来历），加第四类来源不需要新判据，只需要传它的第三个参数。
- **`scopeDueInput` 已经是「几份一起缩」的形状**（同文件:81），加 `expiries` 是往那个返回对象里多塞一份，不是新写一份筛选。
- **`Expiry` 面板有整段可照的形制**：[src/components/milestone-panel.tsx](../../../src/components/milestone-panel.tsx)（151 行）已经含 `<Input type="date">`（该文件:101）、行内新增与删除的跑法。
- **`req_date` 有现成的**：M10 为 `report_date NOT NULL` 新写的必填日期校验（`todo::req_date`，空白直接 `invalid_input` 且点名列名），到期日用它、只要把列名换成「到期日期」。
- **检索加一类 = 一条 `collect`**：`collect(source, sql, conn, pattern)`（[src-tauri/src/search.rs:125](../../../src-tauri/src/search.rs)）固定收 7 个 SELECT 列 + `join_bits` 拼 detail；台账侧现在是 **8 条**同形状 per-table LIKE，本轮变 9 条。`PER_GROUP_LIMIT = 50`（同文件:19）与上游配额都不用动。
- **命中跳转零新代码**：结果页早统一渲染成指向所属项目的 `Link to /projects/$projectId`（M10 §三 核过），到期行带 `projectId` 就能跳。
- **`SOURCE_LABELS` 漏加会编译不过**：它是 `Record<HitSource, string>`（[src/types/search.ts:25](../../../src/types/search.ts)）的穷举映射 ⇒ `HitSource` 加成员而标签没加，`tsc` 直接红。这条不用另写断言。
- **备份自动带新表**：M8 起备份是整库一致快照，`strip_index` 只剥两张索引表 ⇒ 新表不需要任何接线就进包；`backup::Counts` 那七个字段本轮**不加第八个**（沿用 M9 D9 与 M10 缩量二）。

## 四、数据模型与 V7 迁移

```sql
CREATE TABLE project_expiries (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('维保','域名','证书','其他')),
    label       TEXT NOT NULL,
    expire_date TEXT NOT NULL,
    note        TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at  TEXT
);
CREATE INDEX ix_expiry_project ON project_expiries(project_id);
```

- `TARGET_VERSION` 6→7，`const V7: &str`，一版本一个事务，`CREATE TABLE` 不带 `IF NOT EXISTS`（线性追加的既有规矩）。
- **零回填**：本仓至今没有任何迁移写过业务行（M9 立的规矩，M10 续着）。老项目的维保/域名信息**不由迁移猜**，由用户自己录。
- **`expire_date` 不加 CHECK**：形状校验只在 Rust 一处。延续 M10 的裁定 —— CHECK 违例会被 SQLite 冒成 `db_failed`，把「你的日期没填」伪装成「数据库操作失败」。
- **只建项目维度的索引，不建日期索引**：单人几十个项目、每项目几行，`ORDER BY expire_date` 走小表全扫；M9 的两张表也没为 `due_date`/`plan_date` 建日期索引。
- **软删与 `ON DELETE CASCADE`**：`PRAGMA foreign_keys=ON` 挂着，但 `delete_project` 是软删 UPDATE ⇒ CASCADE 永不触发，所以每条跨项目读**必须**带 `p.deleted_at IS NULL`。既有落点可参照 `todo::list`、`milestone::list`、`daily_report::list` 与台账检索各条 collect（本轮不重新数这一共有几处，只保证新增的两条读路径与那条检索都带上这道门，并由 §十二 的 M① 变异守住）。

## 五、后端判据与失败面（一条判据只写一处）

| 判据 | 唯一落点 | 错误码 | 文本要求 |
|---|---|---|---|
| `kind` 合法值 | `expiry::check_kind`（照 `milestone::check_stage` 形状） | `invalid_input` | 点名「种类」并**列出四个合法值** |
| `label` 非空 | `expiry::create`/`update` 里用既有 `norm` | `invalid_input` | 点名「名称」 |
| `expire_date` 形状与必填 | 复用 `todo::req_date`，列名传「到期日期」 | `invalid_input` | 含 `YYYY-MM-DD` 示例与非空要求 |
| 行不存在 / 已软删 | `expiry::get` 那一路 | `not_found` | 沿用 `todo`/`milestone` 现文本 |
| 项目不存在或已软删 | `project_exists` 既有那道 | `invalid_input` | 沿用现文本 |

- **不新开错误码**：只用 `invalid_input` / `not_found`（M10 §八 那份五码表不扩）。
- **SQL 里零日期比较**：30 天、7 天、逾期与否全在前端纯函数（M9 D7 / M10 D9 那条不变量，本轮续着）。
- **历法有效性不在本轮**：`2026-02-31` 这种「形状对、历法错」的值仍按既有处置走（`dueStateOf` 用 `Date.UTC` 把它滚成下一月，两处同判 —— M9 终审已把那次分叉合掉）。引历法库就破「零新依赖」，登记为已知边界，不悄悄做。

## 六、命令契约与线格式

4 条**同步**命令，函数体零 `await`，形制逐字照 [src-tauri/src/lib.rs:731](../../../src-tauri/src/lib.rs) 的 `milestone_*`：

| 命令 | 签名 | 返回 |
|---|---|---|
| `expiry_list` | `(project_id: Option<String>)` | `Vec<Expiry>` |
| `expiry_create` | `(input: ExpiryInput)` | `Expiry` |
| `expiry_update` | `(id: String, input: ExpiryUpdate)` | `()` |
| `expiry_delete` | `(id: String)` | `()` |

```rust
pub struct Expiry       { id, project_id, kind, label, expire_date, note, created_at, updated_at }  // 8 个键，camelCase 上线
pub struct ExpiryInput  { project_id, kind, label, expire_date: String, note: Option<String> }
pub struct ExpiryUpdate { expire_date: String, note: Option<String> }   // 只有这两件有界面消费者（§二 控制方自拍①）
```

- **`deleted_at` 永不上线**，线格式是 8 个键 ⇒ 照 M10/M9 的先例写一条**键集合钉死断言**（多一键少一键都红）。
- `expiry_list` 的排序：`ORDER BY e.expire_date, e.kind, e.id`（最近的先看到；`expire_date` NOT NULL，所以不需要 `milestone` 那句 `plan_date IS NULL` 前置）。
- **`tauri::command(async)` 门不动，仍 7**：这四条不碰文件、取的是 `MutexGuard`，挂 async 只会把坑 21 那件事再解释一遍。
- 结构门本轮的期望值：命令 **64**、async **7**、`TARGET_VERSION` **7**、`dead_code` 豁免 **1**、`Cargo.lock` `[[package]]` **616**（**零新依赖**）。

## 七、前端：面板与预警

- **类型层**：新增 `src/types/expiry.ts`（先例 `src/types/milestone.ts`）、`src/lib/api.ts` 加 4 个 wrapper、`src/types/search.ts` 的 `HitSource` 加 `"expiry"` + `SOURCE_LABELS` 加「到期事项」。
- **面板** `src/components/expiry-panel.tsx`，挂在项目详情页里程碑面板之后。行按 `expireDate` 升序；每行显示 `kind` 徽标、名称、日期、距今多少天（逾期红 / 30 天内描边，沿用预警块现有的两档颜色：`bg-destructive/10 + text-destructive` / outline，仓内没有 amber 令牌）、备注。行上只提供两件事：**续期改日期**、**改备注**；种类与名称录错走删了重建。
- **预警**（`src/lib/due-warning.ts`）：`DueKind` 加 `"expiry"`、`DueInput` 加第四份 `expiries`、`scopeDueInput` 把**四份一起缩**、`DueItem` 加 `archived: boolean`（界面据此念「（归档项目）」）。窗口选择只在一处：新增 `EXPIRY_HORIZON_DAYS = 30` 与一个 `horizonOf(kind)`，`emit()` 收这个参数；比较本体仍是 `dueStateOf`，**不许出现第二份日期比较**（M9 终审 I2 的账）。
- **归档门只包交付截止那一类**（既有实态，本轮不改）：`p.status === "归档"` 那道 continue（[src/lib/due-warning.ts:130](../../../src/lib/due-warning.ts)）只挡住 deadline 类；归档项目的未完成待办与未勾选里程碑**今天就照常进预警**（`todo::list` 的 SQL 没有 status 过滤，[src-tauri/src/todo.rs:139](../../../src-tauri/src/todo.rs) 已核）。所以「归档不进预警」这句从来只对交付截止成立 —— 本轮到期类不加归档门（D4），反而与待办/里程碑的实态一致。
- 排序规则不动：`state → date → 项目名拼音序（`localeCompare(…, "zh")`）→ id`。30 天窗口的条目日期通常更远，会自然排在逾期条目之后。

## 八、检索第 9 条与备份那句明文告警

- **检索**：新增一条 `collect("expiry", …)`。`SELECT` 七列按 `collect` 现形对齐：`id`、`project_id`、`project_name`、`title = e.label`，detail 的 bits 给 `e.kind` 与 `e.expire_date` 与 `e.note`；`WHERE` **只在 `e.label` 与 `e.note` 上做 LIKE** —— **日期不参与匹配**（否则搜「10」能捞出一堆命中，那是噪声不是结果）；软删门 `p.deleted_at IS NULL` 与 `e.deleted_at IS NULL` 两道都要。
- **备份那句明文告警必须改**（这是 M10 终审 I1 的**同一格**）：[src-tauri/src/backup.rs:23](../../../src-tauri/src/backup.rs) 现在结尾是「…待办内容、里程碑备注、日报内容。」，本轮补「到期事项名称与备注」。M10 那次是九格里没有任何一格分配到这句话，所以九次任务级评审结构性看不见它 ⇒ **本轮把它写成一条不变量（§九.6）并钉进某一格的必改文件清单与验收格（§十.14）**，不再靠运气。
- `backup::Counts` 七字段**不进**新表条数（沿用 M9 D9 与 M10 缩量二：旧包没这项、补默认值只能显示 `0`，而「这个包里有 0 条到期事项」对一份装着到期事项的旧包是句假话）。

## 九、撞上的既有不变量（逐条裁定）

1. `TARGET_VERSION` 6→**7**，线性追加、一版本一事务、**零回填**。
2. `generate_handler!` **60→64**（当次批准，第 5 次破例）；`tauri::command(async)` **仍 7**；`allow(dead_code)` 豁免**仍 1**；`Cargo.lock` `[[package]]` **仍 616**（零新依赖，`Cargo.toml`/`package.json` 应零字节差异）。
3. 软删跨项目读门 `p.deleted_at IS NULL` 要在本轮**两条**新读路径（`expiry_list` / 检索那条 collect）与 `expiry_get` 上各自落一次，由 M① 变异守。
4. `SearchBundle` 的线格式七键**不动**；`FieldHit.source` 只是新增一个取值，且 `SOURCE_LABELS` 是穷举映射 ⇒ 漏加编译期红。
5. `DueKind` / `DueInput` 加成员 ⇒ 其所有消费者（`/todos` 页面、`scopeDueInput`、`buildDueWarnings`）由 `tsc` 挡；**页面接线本身没有自动化网**（M9 I1 / M10 坑 35 同族，见 §十一）。
6. `warn_plaintext_columns()` 那句必须同步扩，且**本轮分配到具体一格**（承 §八）。
7. 备份是整库快照 ⇒ 新表自动进包；`strip_index` 不碰它；`Counts` 不加字段。
8. SQL 里零日期比较（M9 D7 / M10 D9）。
9. 日期形状判据只在 `req_date` 一处（M10）。
10. `deleted_at` 不上线，线格式 8 键钉死。
11. **写盘落点白名单不变**：本轮不新增任何落盘路径，M8 那条红线与 `guard_destination` 不参与。
12. **不反转任何既有裁定**（对比 M10 D4 那次）；本轮是**补** M9 的欠账，回写时 M9 spec 顶部加一行指针、§十一 原文不动（沿用 M10 的处理法）。

## 十、验收网格

**规矩（M10 坑 40 的直接教训）**：本节是**权威网格**，实施计划的真机格必须**逐格映射**到这里的 1–14；收口前交一张「spec 格 ↔ 计划格」双向映射表，凡这里有的格没跑掉，要么补跑要么在文档里明写未验，**不许写「全部有读数」这类按编号的总括句**。

| # | 格 | 判据 | 取证档 |
|---|---|---|---|
| 1 | V7 落地 | 底栏/`db_status` 念 `schema=7` 且 `journal_mode=wal` | 真机 + cargo |
| 2 | 新增一条域名事项 | 名称 `oa.gov.cn`、日期 = 今天 +20 ⇒ 面板出现、预警块亮黄、线格式恰好 8 键 | 真机 + cargo |
| 3 | 到期日留空 | 拒收，文本点名「到期日期」并含 `YYYY-MM-DD`，**库里没写进任何行** | 真机 + cargo |
| 4 | `kind` 传非法值（如 `备案`） | `invalid_input` 且列出四个合法值 | cargo |
| 5 | 30 天边界 | +30 天亮、+31 天**不亮**（两边都要断言） | node |
| 6 | 逾期 | 过去的到期日亮 `overdue`、`days` 为负 | node |
| 7 | 归档项目的到期行 | 照常进 items，且 `archived = true`、界面行尾念出「（归档项目）」 | 真机 + node |
| 8 | 归档项目的交付截止 | **仍不亮**（M9 判据⑤ 回归，不许被本轮顺手扩走） | node |
| 9 | 软删项目 | 到期行从 `expiry_list`、检索、预警里三处都读不到，而**库里的行还在** | 真机 + cargo |
| 10 | 筛选同步 | `/todos` 筛到甲项目 ⇒ 预警块与「共 N 条」连第四类一起缩 | 真机 + node |
| 11 | 续期改日期 | 改到 +200 天 ⇒ 面板与预警块当场同步，旧日期不再亮 | 真机 + cargo |
| 12 | 改备注 | `note` 改成功且 `expireDate` 一字不变 | 真机 + cargo |
| 13 | 检索 | 搜名称/备注命中并跳所属项目详情页；**搜日期数字（如 `10`）不因此命中** | 真机 + cargo |
| 14 | 备份 | 新表随整库快照进包；`Counts` 七字段不变；界面那句明文告警念出「到期事项名称与备注」 | 真机 + cargo 文本门 |

## 十一、明确不做

- **台账 Excel/Markdown 导出**（`思维导图.md` §四.2）：仍是 M11 后半，且要先单独定密文列落盘红线。**这三类到期事项也不进 Excel 导入导出模板** —— 与那半格不绑。
- 每行自定义提前量（D3 选的是全局 30 天，不是逐行）。
- 系统通知 / 推送 / 定时轮询：本仓至今没有任何定时器与推送通路，预警永远是「打开界面才念」。
- 父项目聚合子项目到期视图（父子关联 `:74` 未建）。
- 项目信息模板（`:72`）、操作日志（`:75`）、多工作区（`:76`）：都不在本轮。
- 历法有效性校验（见 §五）。
- 到期事项的「续期历史」流水（D2 明写的代价，不做）。
- 页面与路由层的组件测试网（引 jsdom 就破零新依赖）：面板与预警块的**接线**仍只有 `tsc` + `vite build` + 真机人眼，这条在「没能自动验证」清单里如实挂着，不许假称通过。

## 十二、测试与变异取证

- **三档**：cargo（V7 三条 —— 列形状 / `expire_date NOT NULL` 在 SQL 层真生效 / 零回填；`expiry.rs` 的 `check_kind`、`req_date`、软删门、归档项目照常 list、`update` 只动两字段、键集合钉死）；`node --test`（第四类进 items、30 天边界两向、逾期负数、归档项目的到期行仍亮、四份一起缩、`horizonOf` 没把 7 天那三类带偏）；真机沙盒（一次性 identifier `dev.zero.pfm.m11test`，跑完删净；真实数据目录 `%APPDATA%\dev.zero.pfm\` 验证前后三文件 `size + mtime + sha256` 逐字一致）。
- **测试期望数由实跑数出来，逐格现取**，不在本文件里猜（HANDOFF 坑 5）。
- **变异清单（至少这七处，每处都要具名打红）**：M① 摘掉 `expiry_list` 的 `p.deleted_at IS NULL`；M② 摘掉 `check_kind` 的合法值门；M③ 把 `EXPIRY_HORIZON_DAYS` 从 30 改成 7（或让 `horizonOf` 退化成常量 7）；M④ 摘掉 `scopeDueInput` 里 `expiries` 那一份；M⑤ 让 `expire_date` 走 `norm_date`（空白折 NULL）；M⑥ 检索那条把 `label` 换成 `expire_date` 去 LIKE；M⑦ 摘掉 `warn_plaintext_columns()` 新加的那半句。跑变异用 `cp` + `cmp` 验字节还原，不复述未跑出来的读数。

## 十三、回写清单

- `docs/技术方案.md`：§四 数据模型补 `project_expiries` 一行；§九 里程碑表 M11 行改成「到期预警的后半已兑，台账导出仍推后」；M9 那格「预警不在首屏」的口径注明本轮沿用。
- `docs/开发进度.md`：里程碑表加 M11-a 行（含验收证据）+ 变更日志一条。
- `docs/HANDOFF.md`：§0/§3/§4-A/§4-B 改成「M11-a 已合入，下一格 = 台账导出（要先定密文红线）」；§6 指针表补 spec/计划/账本三行；结构门口径从 60/7/6 改成本轮终态。
- M9 spec：顶部加一行指针「§一 对照表与 §十一 里推给 M11 的那三类字段已由 M11-a 兑现，原文不改」，落点在 M11-a spec §九.12。
