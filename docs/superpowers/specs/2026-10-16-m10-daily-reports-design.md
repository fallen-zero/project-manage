# M10 设计：日报管理 + 工时统计 + Excel 双向导入导出

日期：2026-10-16
需求原文：`docs/思维导图.md` §三（日报管理，其中「Excel 导入导出」原文标注为核心）＋ §四.1（全局检索把「日报内容」列为匹配目标）
基线：`master` = `e590a6d`，M0–M9 已全部合入。现跑读数 `cargo test -j 1 --lib` **256 passed / exit=0**、`node --test "tests/**/*.test.ts"` **31 tests / 31 pass / 0 fail**。这两格在本轮结束时各自变多，任何按 256/31 取数的脚本要一起改。

## 一、本轮要解决的问题

软件已经管住了「项目是什么、资料在哪、要做什么」（M0–M9），但没有一处回答「这个项目这周花了多少工时」。项目部的周报/月报现在靠人工回忆拼，且无法交回一份能二次编辑的台账文件。M10 补的是工作量这一维：一条日报 = 某天在某项目上做的某件事花了多久，页面按日/周/月/项目聚合，并且这堆数据能整表导成 `.xlsx`、也能从填好的 `.xlsx` 批量导回。

## 二、需求方当次拍定的十条

按提问顺序，每条都是当时只有一个真决策的一格。

| # | 决策 | 内容 |
|---|---|---|
| D1 | Excel 载体 | **真 `.xlsx` 双向**：导出手写表头，导入按表头识别，日期同时接受 Excel 日期单元与 `YYYY-MM-DD` 文本 |
| D2 | 日报粒度 | **最小条目一行**：一天、一个项目可以有多行，每行一个工作内容 + 一个时长。表上没有 `UNIQUE(日期,项目)` |
| D3 | 项目归属 | **必填**，与 M9 的 todos/milestones 同形（`NOT NULL REFERENCES projects(id)`），非项目工时自己建一个「日常事务」项目装 |
| D4 | 全局检索 | **日报正文与待办标题一起进**台账检索通路（§四.1 点名日报内容；待办 M9 欠的那格一并兑，见 §九的裁定反转） |
| D5 | 导入语义 | **两段式**：`import_preview` 只读文件逐行校验并返回结构化拒绝理由，人看过再 `import_apply`；`apply` 重新解析同一文件、一个事务全成全败 |
| D6 | 归档项目 | **照常列、照常算**：日报的列表与工时汇总里不出现任何按项目状态的过滤，只排除软删项目。与 M9 判据⑤（预警跳过归档）形状相反，是有意为之 |
| D7 | 范围 | M10 只做 §三 的日报双向。§四.2「导出 Excel/Markdown **台账**文件」整格推给 M11（那格导出的是台账五表，里面躺着账号密码密文列，先要单独定红线） |
| D8 | IPC 批准 | 新增 **7 条**，`generate_handler!` **53→60**（本仓第四次批准破例）；导出与导入三条挂 `async`，门 **4→7** |
| D9 | 聚合侧 | **前端纯函数**：沿用 M9 D7「SQL 不解释日期」。`daily_report_list` 不带 `LIMIT`（先例：`todo::list` 就是全量拉回前端），按日/周/月分桶与求和全在 `src/lib/hours-summary.ts`，用 `node --test` 守 |
| D10 | 复制上一天 | **直接落库**：点一下把「所选日期之前最近一个有日报的那天」的条目逐行写成所选日期的日报，回执报条数与源日期；反悔走明细列表逐条删。（需求方当次拍的是「直接落库」这一半；源日期取最近有记录的一天而非机械的前一天，是控制方在 spec 自审里收紧的，理由见 §七.复制上一天） |

## 三、本轮当场核实到的免费能力（决定了上面多条的代价）

这几条不写下来，后面的人会以为它们要新写：

1. **Excel 读写两把库早就在树里**。`calamine 0.36.1` 是**直接依赖**，`extract.rs:176` 已经在用它抽 xlsx 正文；`rust_xlsxwriter 0.99.1` 已在 `Cargo.lock:3870`，只是落在 `[dev-dependencies]`（`extract.rs:419` 拿它造 xlsx 测试夹具）。本轮唯一的依赖动作是把后者从 dev 提到直接依赖，按 M8 `sha2` 那格的既有写法在 `Cargo.toml` 注明「不新增下载、不新增编译单元」。**`Cargo.lock` 的 `[[package]]` 616 保持不变**，这条要有断言位。
2. **Excel 序列号换算不用自己写，`chrono` 不用引**。calamine 的 `Data::DateTime` 装的是它自己的 `ExcelDateTime`（`datatype.rs:690` 起），公开方法 `to_ymd_hms_milli() -> (u16,u8,u8,u8,u8,u8,u16)` 直接给年月日，1900/1904 日期系统与序列号偏移都是库内部的事。本仓因此**不引 `chrono`**，「日期只存形状、SQL 不解释日期」的口径不必为 Excel 破例。注意 `datatype.rs:791` 那个 `as_datetime()` 返回 `chrono::NaiveDateTime` —— **不许用它**，一用就把 chrono 拖进我们的代码面。
3. **台账全局检索不是 FTS，是六条同形状的 per-table LIKE**。`search.rs:181-259` 六次 `collect(source, sql, conn, pattern)` 往一个 `Vec<FieldHit>` 里追加，每条都是 `JOIN projects p ... AND p.deleted_at IS NULL WHERE x.deleted_at IS NULL AND (col LIKE ?1 ESCAPE '\\') ORDER BY updated_at DESC LIMIT ?2`。日报进检索 = 加第七条、`source` 字面量多一个，**不走 `index_docs`/`index_docs_fts`、不碰 `backup::strip_index`、不需要任何重建通路**。
4. **搜索结果的跳转是免费的**。`search-bundle.tsx:22-28` 把每个 `FieldHit` 一律渲染成 `Link to="/projects/$projectId"`，日报行有 `projectId` ⇒ 点一条日报命中就跳它的所属项目详情页，零新代码。
5. **导出落点守卫是 `pub fn`**。`backup::guard_destination(data_dir, dst)`（`backup.rs:162`）直接可调，失败码 `backup_destination_unsafe` 复用不改名（§八）。
6. **Excel 往返测试夹具的造法在树上**。`extract.rs:419` 已经是「用 `rust_xlsxwriter` 写一份 xlsx、再喂 `calamine` 读回来断言文本」的闭环，M10 的导入测试复用同一手法，不需要人手准备文件。

## 四、架构与模块切分

| 单元 | 职责 | 与既有代码的关系 |
|---|---|---|
| `src-tauri/src/daily_report.rs`（新） | 表 CRUD、日期与工时判据、`write_xlsx`/`read_rows` 两个私有函数、preview/apply | 模块形状抄 `todo.rs`；`is_iso_date`/`norm_date` 走 `crate::todo::` 跨模块调用（先例：`milestone.rs` 已这么用 `norm_date`） |
| `src-tauri/src/todo.rs` | **只多一个 `req_date(column, raw) -> AppResult<String>`** | 必填日期列专用，与 `norm_date` 的「空白折 NULL」分家，理由见 §五.2 |
| `src-tauri/src/search.rs` | 台账段 `collect()` 六条 → **八条**（`daily`、`todo`） | 现有 SQL 逐字不动，只在 `ledger_hits` 尾部追加两条同形状 |
| `src-tauri/src/lib.rs` | `mod daily_report;` + 7 条命令注册（3 条 async） | `generate_handler!` 尾项无逗号的老坑 |
| `src/types/daily-report.ts`（新） | `DailyReport`(7 键)、`DailyReportInput`、`DailyReportEdit`、`ImportPreview`、`ImportOutcome`、`MAX_HOURS=24` | 键集钉住测试有先例（manifest 8 键 / SearchBundle 7 键 / DocPreview 6 键） |
| `src/lib/api.ts` | 7 个包装，可选参数一律显式传 `null` | M9 已立的线格式规矩 |
| `src/lib/hours-summary.ts`（新） | `isoWeekKey`/`monthKey`/`sumHours`/`groupByProject`/`buildHoursSummary`/`scopeDailyReports` | 纯函数，只 `import type`，`node --test` 可守、不需要 jsdom |
| `src/pages/daily-reports.tsx`（新） | 汇总区 + 明细列表 + 筛选条 + CRUD + 复制上一天 + 导入导出 | `todos.tsx` 的页面骨架，含它 `run(): Promise<boolean>` 的失败不清态规矩 |
| `src/router.tsx`、`src/components/app-shell.tsx` | 第 6 格 nav `/daily-reports` | — |
| `src/types/search.ts` | `SOURCE_LABELS` 加 `daily`、`todo` 两项 | 线格式键数不变 |

日报的读/查/改一律不落到 `project-detail.tsx`：跨项目统一管理是 §三 的定位（「独立一级栏目，工作量填报」），项目详情页本期不放日报 Tab。

## 五、后端判据

### 1. 数据模型：V6 迁移，零回填

```sql
CREATE TABLE daily_reports (
  id           TEXT PRIMARY KEY,
  project_id   TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  report_date  TEXT NOT NULL,
  content      TEXT NOT NULL,
  hours        REAL NOT NULL,
  created_at   TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at   TEXT NOT NULL DEFAULT (datetime('now')),
  deleted_at   TEXT
);
CREATE INDEX ix_daily_report_project ON daily_reports(project_id);
```

- `TARGET_VERSION` **5→6**，`apply_version` 加 `6 => V6`，`CREATE TABLE` 不写 `IF NOT EXISTS`（本仓规矩）。
- **V6 零回填**：延续 M9 立的规矩，迁移里不写任何业务行。
- **不建日期索引**：单人几年的台账是几千行量级，且 M9 两张表也没为 `due_date`/`plan_date` 建索引，口径一致。
- **`hours` 不加 `CHECK`**：表由本程序唯一写入，Rust 校验已覆盖；CHECK 违例冒出来是 `db_failed` 而不是 `invalid_input`，只会把校验失败伪装成库故障。
- 删除是软删（`deleted_at`），与 `project.rs:163-173` 同规；`ON DELETE CASCADE` 因 `delete_project` 是软删而永不触发，所以跨项目的每条读都必须自带 `p.deleted_at IS NULL`（M9 坑 35 那一族）。

### 2. 日期与工时：两处必须先于 SQL 拒掉

- **`report_date` 必填**，走新函数 `todo::req_date`：先 trim，**空白直接 `invalid_input`**，非空再 `is_iso_date` 校形状，hint 用 D7 那句「日期要用 YYYY-MM-DD」。不能复用 `norm_date` —— 它把空白折成 `NULL`，`NULL` 进到 `report_date NOT NULL` 会由 SQLite 冒成 `db_failed`，用户看到的是一句「数据库操作失败」而不是「你的日期没填」。错误文本必须点名「日报日期」。
- **`hours` 先于 SQL 校**：`0 < h ≤ 24`、最多两位小数，违例 `invalid_input` 且文本点名「工时」。
- `content` trim 后非空，`project_id` 走既有 `project_exists`。
- 排序：`ORDER BY d.report_date, d.created_at, d.id`。后两级是 M9 N3 的教训（`datetime('now')` 只到秒，同秒两行原先全靠 `rowid`）。

### 3. 列表只排除软删，绝不按状态过滤

```
FROM daily_reports d
JOIN projects p ON p.id = d.project_id AND p.deleted_at IS NULL
WHERE d.deleted_at IS NULL AND (?1 IS NULL OR d.project_id = ?1)
```

SQL 里出现任何 `p.status` 判断都算缺陷。这一格有自己可红的断言（M①）：给一个 `status='归档'` 的项目写日报，断它既在列表里、也在按项目汇总里、也在导出的行里。理由是 D6 —— 归档是项目管理上的退役，工时是已经发生的事实；跳过归档等于给领导的台账凭空少一截，而且「看不见」不等于「没写进库」。

### 4. 编辑可以改所属项目

`DailyReportEdit{projectId, reportDate, content, hours}` 四样全可改，改项目要再走一次 `project_exists`。**这与 M9 的 `TodoEdit{title,dueDate,done}` 不同**，理由是项目归属是日报最容易填错的字段，且导入按编号/名称匹配、项目改名后需要能把已导错的行挪走；代价是多一条判据和一条测试。

### 5. 命令面（7 条，53→60）

| 命令 | 形态 | 说明 |
|---|---|---|
| `daily_report_list(project_id?)` | sync | 无 `LIMIT`，全量返回（D9） |
| `daily_report_create(input)` | sync | |
| `daily_report_update(id, edit)` | sync | |
| `daily_report_delete(id)` | sync | 软删 |
| `daily_report_export(path, rows)` | **async** | 前端把当前筛选后的行传进来；`rows` 为空 = 下载空白模板 |
| `daily_report_import_preview(path)` | **async** | 只读，不写库 |
| `daily_report_import_apply(path)` | **async** | 重新解析 + 单事务落库，返回 `ImportOutcome{inserted}` |

三条 async 的**函数体一律零 `await`**。这沿用的是既有规则而不是新发明：现存的 4 条 async（`doc_preview`、`backup_export`、`backup_inspect`、`backup_import`）恰好就是碰文件的那一家，理由是 `MutexGuard` 与 `rusqlite::Connection` 都不是 `Send`，一旦 `await` 就横跨悬挂点（HANDOFF 坑 21 / M8 D11）。异步签名把命令体挪出消息泵那一个线程，所以大文件的解析与写盘不会冻住整个界面；DB 那侧仍然由全局 `Mutex` 串行，这是既有代价，本轮不改变。

`daily_report_list/create/update/delete` 不加 `not_indexing` 守卫：那条守卫是为「整库替换」准备的（`backup_import`），日报只追加/改自己的行，与索引 job 写 `index_docs` 不冲突。

## 六、Excel 双向

### 1. 导出

- 工作表名 `日报`，表头五列固定：`日期`｜`项目编号`｜`所属项目`｜`工作内容`｜`时长（小时）`。第二行起是数据。
- 日期写成**文本 `YYYY-MM-DD`**，不写成 Excel 日期单元。理由：本仓的日期口径就是「只存形状」，写成文本可以让「导出 → 用户编辑 → 再导回来」这条环不经过序列号，从源头少一个可错点。
- 「下载模板」= 同一条命令传空 `rows`，只有表头 + 冻结首行。**原先这里写的是「附一行灰底示例」，本轮撤掉**（2026-10-16 写计划时发现）：示例行会被用户忘记删，导回来就是一条挂在不存在项目上的假日报 —— 预检能拒它，但那是让界面自己造一条注定被拒的数据。
- 导出的「所属项目」列写 **`projects.name` 原值**，不是界面上的 `projectLabel`（后者在有编号时是「名（编号）」）。这一列是导入的名称匹配对象，拼过编号就对不上。
- **命令只收行、不收查询条件**：`daily_report_export(path, rows)`。这不是省事，是不能再造第二份日期比较 —— `report_date` 是 TEXT，SQL 里的 `BETWEEN` 和前端用的形状判断迟早分叉（M9 终审 I2 就是分叉的账）。导出因此严格是所见即所得。
- 入参 `rows` 来自客户端，反序列化要拒异常形状（沿用 M9 N5 的口径：按码点复验，不接受数组塞进标量字段）。
- 落点先过 `backup::guard_destination` 再碰文件：`File::create` 是 CREATE_ALWAYS 截断，把落点指进 `%APPDATA%\dev.zero.pfm\` 会覆盖真库（`backup_export` 里那句注释写的同一个坑）。

### 2. 导入

- `calamine::open_workbook_auto` 取第一个工作表的 `Range`（用法先例 `extract.rs:176-178`，必须 `use calamine::Reader as _`）。多 sheet 只读第一个，不做 sheet 选择 UI。
- **按表头文字定位列，不按列序硬数**：用户在模板里插一列不该错位。认不出必需列 ⇒ `invalid_input`，错误文本点名缺哪一列。
- 日期单元格：`Data::DateTime(e)` ⇒ `e.to_ymd_hms_milli()` 取年月日、截断时分秒（截断这件事在 preview 里记一条提示，不静默）；`Data::String(s)` ⇒ 走 `is_iso_date`；`Data::Float(_)` 出现在日期列 ⇒ 按具名行号拒绝。
- 时长单元格：`Float` 直接收；`String` 按 `f64` parse 并拒千分位与全角；**`DateTime`（用户在 Excel 里写成 `8:30` 时间格式）不换算成小时**，按行号报错「时长要填小时数（8.5），不要用时间格式（8:30）」。
- 项目匹配两级：**先按 `项目编号` 精确匹配，命中即用；否则按 `所属项目` 名称精确匹配**。`SELECT id, code, name FROM projects WHERE deleted_at IS NULL` 拉回后在 Rust 侧比，匹配 0 个 ⇒ 报「第 X 行：项目「…」找不到」；匹配 ≥2 个同名 ⇒ 报「第 X 行：项目名「…」重名 2 个，请填项目编号」。`code` 为空的行走不进编号那一级，属正常降级。
- **行号一律是 Excel 里的绝对行号**（含表头偏移），不是数组下标 —— 用户看的是 Excel，报 0-based 下标等于没报。
- `preview` 返回 `{ total, ok, rejected: [{line, field, reason}] }`，并做**重复提示**：`(report_date, project_id, content, hours)` 四字段与库里某条未删行全等 ⇒ 记为重复行、算进 `ok`（D5 说了只提示不拦，因为 D2 之后「同一天同项目同内容」不再是错误）。
- 单次上限 **2000 行**，超了 `invalid_input` 提示分批。
- `apply` 重新解析同一路径、重新校验，全部通过才在一个 `transaction()` 里逐行插入；任一行不过 ⇒ 整批回滚并把第一次的拒绝理由原样报出去。不锁文件、不缓存 preview 结果 ⇒ 两步之间文件被改过就按新内容算（D5 当次写明的代价）。
- 落库后前端重新拉一次 `daily_report_list`，后端不做第二份真值（先例：`backup.rs:89` 那句注释）。

## 七、前端：录入、筛选、聚合

页面 `/daily-reports` 上下两块：**工时汇总在上**（按日 / 按周 / 按月三档切换 + 按项目一栏），**明细在下**（筛选条：日期范围 / 项目 / 关键词；行内编辑与删除；「复制上一天」；导出 / 下载模板 / 导入）。汇总在上是因为 §三 把这一栏定位为「工作量填报」，明细是手段。

`src/lib/hours-summary.ts` 的判据：

- `isoWeekKey(date)` → `YYYY-Www`。**周归属必须按 ISO-8601**（周一起算，包含周四的那一周属于该年）。三条边界断言：`2026-01-01`（周四）→ `2026-W01`，`2026-12-31`（周四，2026 是 53 周年）→ `2026-W53`，`2027-01-01`（周五，同属那一周）→ `2026-W53`。这是本轮最容易被写错且写错不报的地方。
- `monthKey(date)` → `YYYY-MM`；日桶直接用原串。
- `sumHours(rows)` = `Σ Math.round(h*100) / 100` 后再除 100。夹具十条 `0.1`，断言 `=== 1`（写成 `toBeCloseTo` 会恒绿）。
- `groupByProject(rows)` 组序用 `Intl.Collator("zh")`（M9 已装好的手法）。
- `scopeDailyReports(rows, filter)` 与 `buildHoursSummary(scoped)`：`localToday()` 仍是唯一时钟读取者，沿用 M9 的 `dueStateOf` 教训 —— **凡「几份输入要一起缩」的地方抽成一个纯函数**，好让「筛到甲项目只看见甲的工时」这一格有可红处（M9 终审 I1 那一格就是预警块吃了未筛选全量）。

「复制上一天」（D10）：源日期不是机械的「所选日期减一天」，而是**所选日期之前最近一个有日报的那天** —— 周一上班点这个按钮要复制的是上周五那几条，按前一天取会拿到周末的 0 条，等于按钮失灵。落库日期 = 筛选条里选中的那天（默认今天）。逐行 `create`，回执「从 <源日期> 复制了 N 条到 <目标日期>」，源日期要出现在回执里，否则用户看不出它到底抄的是哪天。前端串行发，且**沿用 `run(): Promise<boolean>` 的写法 —— 记一笔成功后才继续/清态**，M9 的 I3 就是连点两次插了重复行。按钮落库后不反悔：撤销走明细列表逐条删。库里一条日报都没有 ⇒ 按钮禁用并说明原因，不静默写 0 条。

`/todos` 的到期预警块与日报页无交集（D6：`report_date` 不进预警，M11 盯的是维保/域名/证书）。

## 八、错误码

**本轮一个新码都不开**（下表五个全是本仓在用的码）。

| 码 | 用在哪 | 出处 |
|---|---|---|
| `invalid_input` | 日期形状与日期空白、工时越界/精度、内容为空、表头认不出、行超限、项目匹配不上或重名、`rows` 形状异常 | CRUD 家族既有码 |
| `not_found` | 按 id 改/删不存在的日报；`project_id` 指向不存在或已软删的项目 | 同上 |
| `backup_destination_unsafe` | 导出落点落在数据目录里 | `backup.rs:162` 的 `guard_destination`，本来就是 `pub fn` |
| `fs_failed` | 写 `.xlsx` 失败（磁盘满、没权限、路径被占用） | `error.rs:21` 的 `AppError::io(path, e)`，文案已是「访问 X 失败 / 确认该路径存在且可读写」 |
| `extract_failed` | 打开 `.xlsx` 失败（损坏、受密码保护、伪装成表格的其它格式） | 与 `extract.rs:179` 同码同文案：同一个「这份表打不开」在两条通路上不该有两种说法 |

`backup_destination_unsafe` 这个名字里的 `backup` 是 M8 的历史命名，语义到本轮已经是「任何写文件命令的落点守卫」——本 spec 明写这一点，免得下轮以为是日报专用的第四个域。

本节原先写的是「错误码只有三个」，那是控制方漏算了文件读写这一档：`fs_failed` 与 `extract_failed` 都是既有码，不需要批准，但也不许假装它们不存在（2026-10-16 写计划时核实并改准）。

## 九、撞上的既有不变量（逐条裁定）

1. `backup::Counts` 的 7 字段 camelCase 线格式 ⇒ **日报不进摘要**，沿用 M9 D9 的理由（M8 旧包没这项，补默认值只能显示 `0`，而「这个包里有 0 条日报」对一份装着日报的旧包是句假话）。代价写进 §十一。
2. `warn_plaintext_columns()` ⇒ 那句明文告警加「日报内容」。这是同一段话第三次改（M8 立、M9 加待办与里程碑备注、本轮加日报）。
3. 前端 nav 现 5 格 ⇒ 加第 6 格 `/daily-reports`。
4. `generate_handler!` **53→60**：D8 当次批准，本仓第四次破例。
5. `tauri::command(async)` **4→7**：本轮新增，函数体零 `await`（§五.5）。
6. `TARGET_VERSION` **5→6**；新立：**V6 零回填**。
7. `SearchBundle` 的 7 键线格式**不变**，台账段多两类 `source` ⇒ `src/types/search.ts` 的 `SOURCE_LABELS` 加 `daily`/`todo`。
8. **M9 spec §十一 那句「待办正文不进任何检索通路」被本轮反转**。D4 是需求方在 M10 当次改的口径，不是 M9 做错；M9 spec、技术方案 §九、HANDOFF 三处活文档要跟着回写并写明反转来源。待办侧的检索 SQL 只搜 `title`（M9 已裁定 `todos` 表没有 note 列，正文就是 title）。
9. `Cargo.lock` 的 `[[package]]` **616 不变**（`rust_xlsxwriter` 从 dev 提直接依赖不新增包）。
10. 两道计数门在 HANDOFF §4-A 与坑 21 各出现一次，两处都要跟着改，否则下轮把「破例」读成「漏注册」。

## 十、验收判据（真机与自动化各自要绿的格）

1. 空库启动 → V6 落地 → 日报页能新增一条（日期默认今天、项目必填、工时 `7.5`）→ 刷新还在。
2. 日期留空提交 ⇒ 界面报错点名「日报日期」，**不是**「数据库操作失败」。
3. 工时填 `24.5`、`0`、`0.001` ⇒ 三条都被拒且文本点名「工时」。
4. 给一个 `归档` 状态的项目写日报 ⇒ 列表看得见、按项目汇总里有它、导出里有它。
5. 筛到某一个项目 ⇒ 汇总区的日/周/月数字与按项目数字**只含该项目**（计数与空态也吃 scoped，M9 I1 同格）。
6. 跨年周：给 `2026-12-31` 与 `2027-01-01` 各写一条 ⇒ 两条落进同一个周桶 `2026-W53`。
7. 十条 `0.1` 的日报 ⇒ 总工时显示 `1`。
8. 导出 ⇒ 用 Excel 打开是真 xlsx（不是改名的 csv）；把落点指进 `%APPDATA%\dev.zero.pfm\` ⇒ 报 `backup_destination_unsafe` 且**目录里没多出文件**。
9. 下载空白模板 ⇒ 表头五列齐；照模板填 3 行（其中一行故意写坏日期、一行故意写不存在的项目）⇒ preview 报出两个**绝对行号**与各自原因，`ok=1`；点 apply ⇒ 只写 1 条。
10. 把 3 行都修好再 preview/apply ⇒ 3 条全进；把同一个文件**再 apply 一次** ⇒ 事务仍然成功（重复靠提示不靠拦），库里 6 条，preview 这次把 3 行标成重复。
11. 故意让 3 行里的第 2 行不合格再 apply ⇒ 库里**一条都没多**（M⑦ 打的就是这格）。
12. 「复制上一天」：周五写 3 条、周六周日不写，周一打开点它 ⇒ 复制的是**周五**那 3 条（按前一天取会拿到周日的 0 条，这一格打的就是源日期选择）；一条都没有时按钮禁用；连点两次 ⇒ 变 6 条且状态按 `run()` 的布尔回执走、不错清。
13. 全局搜索一个只在日报正文里出现过的词 ⇒ 台账段出现该命中，`source` 标签显示「日报」，点开跳所属项目详情页。
14. 真机全程：真实数据目录只读红线守住（前后快照字节一致），沙盒用一次性 identifier 并跑完删净。

## 十一、明确不做与本轮欠账

- **§四.2 的项目台账 Excel/Markdown 导出**推给 M11（D7）。那格导出的是台账五张表，`ledger_credentials`/`ledger_servers` 里躺着密文列，导成文件等于把明文交到用户手里，得先单独定红线。
- **备份摘要看不出日报条数**（§九.1 沿用 M9 D9）：备份包里有日报（整库快照自动带着），但 `/backup` 的条数摘要不显示它。
- **一行只挂一个项目**（D2/D3 的直接后果）：一天内在三个项目上干活要写三条。
- **Excel 里的时间格式时长不换算**（`8:30` 不认成 8.5 小时），只按行号报错。多 sheet 只读第一个。
- **`report_date` 不参与到期预警**：M11 那一格盯的是维保/域名/证书到期，那三类字段至今没建，欠账不变。
- **没有「按人」维度**：单机单用户，日报默认全是本机这人填的。
- **反转进来的代价**（D4）：待办标题会出现在全局搜索结果页的明文列表里。M9 裁掉它本来是范围决定，不是技术做不到；本轮按需求原文把它接进来，敏感描述若日后要遮，得另开一格。
- **导入期间文件被人改动**（D5/D6 的两步式后果）：`preview` 与 `apply` 之间不锁文件，按新内容算。

## 十二、测试与门禁策略

- Rust 侧：V6 迁移断言（表存在、列齐、`report_date NOT NULL` 被真拒、软删语义、`req_date` 的两支、`hours` 校验、八条 `collect` 里日报那条能命中未软删项目）。基线现跑 256 passed，本轮预期 +24 上下。
- 前端侧：`tests/hours-summary.test.ts`（周/月/日三档归属、ISO 跨年周三条边界、求和整数化、按项目分组序、筛选后聚合只含 scoped）。基线现跑 31，本轮预期 +12 上下。
- 线格式键集钉住：`DailyReport` 7 键、`ImportPreview`/`ImportOutcome` 各一套键。
- 结构门：`generate_handler!` 60、`tauri::command(async)` 7、`TARGET_VERSION` 6、`allow(dead_code)` 仍 1（`db.rs:30`）、`Cargo.lock` `[[package]]` 仍 616。取数用 HANDOFF §4-A 那串命令现跑，别抄本文的数字。
- 变异取证八条，逐条要「红在预期的那一格」并 `cmp` 验字节一致还原：M① 状态过滤、M② 空白日期折 NULL、M③ 摘掉 hours 校验、M④ 周首漂成周日、M⑤ 去掉求和整数化、M⑥ 聚合吃全量、M⑦ 事务拆成逐行、M⑧ 表头按列序硬数。M②/M⑦ 是本轮最值钱的两条 —— 它们的失败形态都不报错，只静默进脏数据或半套台账。
- 真机：§十 十四格，CDP 驱动；`tauri dev` 只由控制方跑，子代理一律不碰。
