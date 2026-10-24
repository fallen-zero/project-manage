# M15 设计：多工作区分类隔离

日期：2026-10-23
分支：`feat/m15-workspaces`（自 `master` `f714735` 切出）
前置：M13 父子项目（V9 `parent_id`）、M14 操作日志（V10 `project_audit_logs`）均已合入 `master`。

## 一、需求出处与本轮兑现范围

`docs/思维导图.md:76` 在「四.3 还可以继续做的事」里留了最后一格候选：

> 多工作区分类隔离（按客户/项目类型）

本轮兑现的是**同一份 SQLite 库内的逻辑分区**：把「项目 + 项目的关联数据」切成若干互不可见的工作区（按客户、按项目类型），用顶栏一个全局切换器决定当前看哪一区，切换即换整个界面视野。

本轮**不兑现**的是每区一套物理隔离（独立库文件、独立备份包、独立索引任务）——需求方在拍定时明确选了「项目及关联数据隔离」而不是「每区独立库」，理由与代价见 §十二。

## 二、需求方拍定的口径

| # | 议题 | 拍定 | 一句话理由 |
|---|------|------|-----------|
| D1 | 隔离粒度 | 项目及其关联数据（待办/里程碑/到期/日报/台账/正文索引/操作日志）跟着区走；模板与备份全局 | 分区要解决的是「手上并行的几个客户别互相串」，模板是跨客户复用的资产 |
| D2 | 既有数据 | 全部回填进一个默认工作区 | 升级后打开应用看到的必须和升级前一模一样，不能「项目不见了」 |
| D3 | 工作区管理 | 可增、可改名、可删；默认工作区不可删 | 删除默认区会让 `projects.workspace_id` 的 DEFAULT 指向不存在的行 |
| D4 | 项目换区 | 编辑项目时可以改归属 | 客户转手是真实动作，不改归属就得删了重建，日志与子项目全丢 |
| D5 | 强制点 | 后端强制隔离（不是前端过滤） | 见 §四：所有读写命令自己认活动区，客户端传什么都不越界 |
| D6 | 当前区存哪 | 后端 `settings` 表记住当前区，新增 5 条命令（list/create/rename/delete/switch） | 重启后回到上次看的区；前端刷新不丢状态 |
| D7 | 换区与父子树 | 根项目换区时整棵子树随迁；有父项目的项目本身不允许单独换区 | 允许子节点单独换区会造出跨区父子边，`validate_parent`（`project.rs:93`）与树视图都会裂 |
| D8 | 入口形态 | 顶栏全局下拉 + 「管理工作区」弹窗 | 区是全局视野，不该藏在某个页面里 |

## 三、数据模型与 V11 迁移

`src-tauri/src/db.rs:98` 的 `pub(crate) const TARGET_VERSION: i64 = 10;` 升到 **11**，`apply_version` 的匹配链加 `11 => V11`。

```rust
/// V11：多工作区。一张 `workspaces` + `projects.workspace_id` + 一个当前区设置行。
/// 顺序是刻意的：先建表并种默认区，再给 projects 加带 DEFAULT 的列，
/// 这样「已有项目回填到默认区」由 SQLite 的 ADD COLUMN 语义天然完成，不写 UPDATE。
const V11: &str = "
CREATE TABLE workspaces (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO workspaces (id, name) VALUES ('default', '默认工作区');

ALTER TABLE projects ADD COLUMN workspace_id TEXT NOT NULL DEFAULT 'default';

CREATE INDEX ix_project_workspace ON projects(workspace_id);

INSERT OR IGNORE INTO settings (key, value) VALUES ('active_workspace_id', 'default');
";
```

三条迁移语义钉死：

1. **回填零 UPDATE**：`ADD COLUMN ... NOT NULL DEFAULT 'default'` 在 SQLite 里会把既有行该列填成 `'default'`（与 V9 给既有行填 `NULL` 同一条规则，只是常量不同）。既有项目的 id、名称、标签、目录、父子边一律不动。
2. **`INSERT OR IGNORE` 而不是 `INSERT`**：`settings` 是 V1 就有的表，键位可能已被备份导入带进来；冲突必须静默跳过，否则重放/导入直接失败。而 `workspaces` 那条用的是普通 `INSERT`——表刚建出来必然为空，写成 `OR IGNORE` 反而会把「默认区被人为删过」这种真损坏吞掉。
3. **迁移是原子的**：V11 任一句失败，整段回滚、`schema_meta` 不留行（沿用 `db.rs:589` 那条 `apply_version` 原子性探针的前提）。所以不存在「`projects` 有新列但 `workspaces` 没建」的中间态。

`projects.workspace_id` 类型是 `TEXT NOT NULL DEFAULT 'default'`，**不带外键**：本表外键只在 `project_dirs`/`todos` 等子表上用于级联删，工作区删除是硬删且被 §五 的非空守卫挡住，加外键只会把「备份导入旧包」变成外键失败。列的 `DEFAULT` 是最后一道兜底：任何绕过 `create_project` 的裸 SQL 插入都落进默认区，不会落进 `NULL` 而在过滤里变成「哪个区都不显示」。

**旧备份包导入**：`backup.rs:556` 的 `import` 在灌完页后会跑 `crate::db::migrate(conn)`，所以 schema ≤10 的包在本机导入时会自动补 V11——包里的工作区表是空的、项目列是缺的，`migrate` 负责把两者补齐并把包里的项目放进默认区。这条要有测试守着（§十 T-14 说明覆盖方式：`migrate` 层的「库停在 V10 → 补齐到 V11」由 §十 T-2 直接证，`import` 侧沿用 `backup.rs:556` 既有的那次 `crate::db::migrate` 调用与既有导入测试，不为本格伪造一份 v10 旧包夹具）。

## 四、活动工作区：后端怎么知道「现在看哪一区」

新增模块 `src-tauri/src/workspace.rs`，对外三个读侧 API（其余是命令实现）：

```rust
pub const DEFAULT_ID: &str = "default";
pub const ACTIVE_KEY: &str = "active_workspace_id";

/// 当前活动区 id。读 `settings`，读不到时回 `DEFAULT_ID`。
pub fn active_id(conn: &Connection) -> AppResult<String>;

/// 断言 `project_id` 属于活动区且未软删；不属于就回 `not_found`。
pub fn ensure_active_project(conn: &Connection, project_id: &str) -> AppResult<()>;

/// 某一行（表名 + 行 id）背后的项目是否属于活动区。跨区或行不存在一律 `not_found`。
pub fn ensure_active_row(conn: &Connection, table: &'static str, id: &str) -> AppResult<()>;
```

`ensure_active_row` 内部 SQL 形如 `SELECT 1 FROM {table} t JOIN projects p ON p.id = t.project_id WHERE t.id = ?1 AND p.workspace_id = ?2 AND t.deleted_at IS NULL`；`table` 是 `&'static str` 常量、不是运行期字符串，所以拼进 SQL 也不是注入面（与 `ledger.rs:212 soft_delete(conn, table, id)` 同一种「表名由调用方以常量传入」的既有写法，五张台账表名在 `ledger.rs` 里已经是编译期常量）。**跨区与不存在共用 `not_found`，不新增错误码**：一个 crafted id 的读/写请求得到的答复和 id 打错完全一样，界面也只需要一条文案。

### 4.1 读路径过滤（全部在后端）

| 落点 | 现有位置 | 加什么 |
|------|---------|--------|
| 项目列表 | `project.rs:264 list_projects` | `WHERE p.workspace_id = ?活动区`（软删谓词照旧） |
| 项目详情 | `project.rs:335 get_project` | 同上谓词；跨区 id 直接 `not_found`，`/project/$id` 深链天然翻不到别区项目 |
| 库信息检索 | `search.rs:144 field_hits` 的 9 段（`:159` 项目、`:181` env、`:198` credential、`:214` server、`:230` link、`:246` note、`:263` 日报、`:279` 待办、`:298` 到期） | 9 段**全部**已 `FROM/JOIN projects p`，所以只改一处签名：`search.rs:125 collect(source, sql, conn, pattern)` 加一个 `workspace: &str` 实参、每条 SQL 用 `?3` 绑它，谓词写 `AND p.workspace_id = ?3`（项目段进 WHERE，其余八段进已有的 JOIN ON）。一处改、九处用，`hits.append(&mut collect(` 的个数仍为 9 |
| 正文命中 | `index_store.rs` 的 `SELECT_SQL`（`doc_hits` 用） | 已有 `JOIN projects p`，加 `p.workspace_id = ?N` |
| 首屏空态谓词 | `index_store.rs:207 indexed_project_count` | 同上，**必须与 `SELECT_SQL` 同口径**（那句 M3 起就在的注释要求少半个谓词都不行） |
| 索引页 | `index_store.rs:232 list_docs`、`status_counts` | 入口先 `ensure_active_project(project_id)`；每项目清单天然只列活动区项目 |
| 待办 / 里程碑 / 到期 / 日报 | `todo.rs:139`、`milestone.rs:92`、`expiry.rs:98`、`daily_report.rs:111` 的 `list`（都不带区参数） | JOIN projects 加区谓词；这三张表的 `list` 允许 `project_id=None` 拉全库，改完就是拉活动区全部 |
| 各自的 `get` | `todo.rs:157`、`milestone.rs:110`、`expiry.rs:116`、`daily_report.rs:129` | 走 `ensure_active_row` |
| 台账 | `ledger.rs:235 list` | 入口 `ensure_active_project(project_id)` |
| 操作日志 | `project_audit.rs:228 list`、`:283 project_options` | JOIN projects 加区谓词（日志表只有 `project_id` 快照列、无区列） |
| 日报导入 | `daily_report.rs:529 load_projects`、`:542 existing_keys` | `load_projects` 的 SQL 加 `AND workspace_id = ?1`、`existing_keys` 改成 `JOIN projects p` 后加区谓词：预检与落库只认当前区的项目，表里指向别区项目的行走既有「找不到项目」那条拒绝理由 |
| 台账导出 | `lib.rs:818 ledger_export` 命令体 | `ledger_export::collect` 之前一句 `workspace::ensure_active_project(&conn, &input.project_id)?`，形状同它上面那道 `backup::guard_destination`（写盘前先挡） |
| 正文预览 | `doc_preview.rs:72 row_for_preview` | 已有 `JOIN projects p`，加 `AND p.workspace_id = ?2`；跨区 `doc_id` 走它现有的「记录可能来自已注销的目录」那条 `unavailable` 文案，不新增错误码 |
| 索引总览 | `lib.rs:501 index_overview` | `index_job::targets(&conn, None)` 的结果先用一次 `SELECT id FROM projects WHERE workspace_id = ?1` 取到的 `HashSet<String>` 过滤，**再进 `status_counts` 循环**（少排一段无用的 per-项目查询）；worker 自己那条 `targets` 调用一字不动 |

**日报 Excel 导出不需要 SQL 改动**：`lib.rs:909 daily_report_export` 收的是前端已经按区取好的 `rows: Vec<ExportRow>`，列表窄了导出就窄。

不改的一侧：**索引 worker**。`index_job.rs:98 targets()` 照旧全库扫所有未删项目的根目录。索引是重活、按文件 mtime 增量，跟着 UI 反复重扫会把 worker 变成后台灾难；隔离做的是「按当前区呈现与可写」，正文抓取保持全局。这条取舍写进 `docs/技术方案.md` 并在 §十二 里再点名一次。

### 4.2 写路径守卫

- **create 类**（往某个项目下挂数据）：把区断言加在**已有的那五处「项目存在」谓词**上，各改一行即可覆盖全部 create：
  `todo.rs:124`、`milestone.rs:77`、`expiry.rs:85`、`daily_report.rs:98`、`ledger.rs:223 project_exists`。
  SQL 由 `... AND deleted_at IS NULL` 变成 `... AND deleted_at IS NULL AND workspace_id = ?活动区`。
- **update / delete 类（按行 id）**：`todo`/`milestone`/`expiry`/`daily_report` 的 12 个 mutator、`ledger` 的 10 个 mutator，各在函数首行加一句 `ensure_active_row(...)?;`；`ledger` 五类行 id 的表名沿用 `ledger.rs` 里已有的常量表。
- **目录三个 mutator**：`project.rs:440 set_root_dir`、`:470 add_entry_dir`、`:500 remove_dir` 首行加 `ensure_active_project(project_id)?;`。
- **项目 update / delete**：已经先 `get_project`（`project.rs:199`/`:236`），过滤继承自 §4.1，不需要额外一句。
- **索引删除**：`index_store.rs:120 clear_project` 只被 `delete_project` 的内部流程调用，调用方已过守卫，不加。

守卫的效果是**双向**的：既挡住「给别区的项目写待办」，也挡住「在本区改了别区的行」。D5 选后端强制，这就是它的兑现。

### 4.3 活动区行的缺失与损坏

`active_id` 读不到 `settings.active_workspace_id` 时回 `DEFAULT_ID`，不报错。理由：V11 已经种了这一行，读不到只可能是用户手工 `DELETE FROM settings`，此时报错会让应用整个界面打不开（所有命令都失败），回默认区则至少看得见数据。这个兜底要有测试（§十 T-5），并明确它是**兜底不是主路径**。

`workspace_switch` 只接受真实存在的区 id，所以活动区指针不会悬空：删除区时又必须先切走（§五），两者合起来保证 `settings` 里的 id 永远指向一行存在的 `workspaces`。

## 五、工作区命令契约（5 条，全部同步）

`generate_handler!`（`lib.rs` 末尾现在是 `audit_list`）追加五条，命令数 **70 → 75**；五条都用 `#[tauri::command]`（不带 async），async 门**保持 7**。

```rust
#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub project_count: i64,
    pub is_active: bool,   // 这一行是不是当前区：前端首屏要知道活动区，
                           // 但不为它单开第 6 条命令（D6 拍的是 5 条）
}

workspace_list() -> AppResult<Vec<Workspace>>
workspace_create(name: String) -> AppResult<Workspace>
workspace_rename(id: String, name: String) -> AppResult<Workspace>
workspace_delete(id: String) -> AppResult<()>
workspace_switch(id: String) -> AppResult<String>   // 回切换后的活动区 id
```

- `workspace_list`：按 `created_at, id` 定序（`default` 因此恒在最前），`project_count` 数的是**未软删**项目，只用于弹窗展示与删除禁用判据；`is_active` 由同一条 SQL 之外的一次 `active_id` 读出来打标（最多一行为 `true`）。
- `workspace_create`：`name` trim 后非空、长度 ≤ 40 字符（`invalid_input`，理由：它进顶栏下拉，超长会把 header 撑破；后端一次校验，前端不加 `maxLength`）；与既有名重名回 `invalid_input`「工作区名称已存在」（`workspaces.name` 上有 UNIQUE 兜底，冲突要先查再插，才能把 `db_failed` 变成人话）。**创建后不自动切过去**（需求方口径：先看列表，切不切由人决定）。id 用 `uuid::Uuid::new_v4()`（沿用 `project.rs:173` 的既有依赖，零新依赖）。
- `workspace_rename`：同套名字规则；`default` 可以改名（只是不可删）。id 不存在回 `not_found`。
- `workspace_delete`：三道守卫按顺序——① `id == "default"` 回 `invalid_input`「默认工作区不可删除」；② `id == 活动区` 回 `invalid_input`「当前工作区不可删除，请先切换到其他工作区」；③ `SELECT COUNT(*) FROM projects WHERE workspace_id = ?1` **含软删行** > 0 回 `invalid_input`「该工作区下还有项目（含已删除项目），不能删除」。含软删是因为软删行仍占着 `workspace_id`，删掉区会让它指向不存在的区、连带把它的日志从所有视野里抹掉。硬删 `workspaces` 行，**不级联**（本来就只允许删空区）。
- `workspace_switch`：id 必须存在，否则 `not_found`；写 `settings` 用 `INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value`（`settings` 主键是 `key`，见 `db.rs:156`），重复切同一个区幂等成功。

## 六、项目归属与子树随迁

`ProjectInput` 加 `#[serde(default)] pub workspace_id: Option<String>`；`Project` 加 `pub workspace_id: String`（列是 NOT NULL，读出来恒有值）。

- **create（`project.rs:168`）**：写死活动区。`input.workspace_id` 在 create 里**被忽略**，值取 `active_id(conn)`。这是刻意的：如果接受客户端传来的区 id，就能用一条 IPC 把项目塞进当前视野看不见的区，造出「数据存在但哪一区都找不到」的孤儿。要有测试专门钉这条（§十 T-8）。
- **update（`project.rs:199`）**：`input.workspace_id` 为 `None` 或与旧值相同 → 不迁；不同 → 走换区流程：
  1. 目标区必须真实存在（`invalid_input`）；
  2. **有父就拒绝单独换区**：`old.parent_id.is_some() || input.parent_id.is_some()` → `invalid_input`「子项目不能单独换区，请编辑它的父项目（整棵子树一起随迁）」。「本次提交里刚指定了父项」也算有父，否则「同一次保存里既摘父又换区」会绕过 §六.3 的子树前提；前端在 `form.parentId !== null` 时就把该行禁用，正常情况下走不到这条错误；
  3. 目标区收整棵子树：递归收集 `id` 及全部后代，一条 `UPDATE projects SET workspace_id = ?2 WHERE id IN (...)` 落地。收集优先用 `WITH RECURSIVE`；若 bundled SQLite 不接受 `WITH` 前缀的 UPDATE（本项目未实测过这一形态），退化成 Rust 侧按 `parent_id` 逐层 `SELECT` 收集到 `Vec<String>`，**两者都必须在 `write_tx`（`project.rs:159`，BEGIN IMMEDIATE）这一个事务里完成**——半途失败不能出现「父在本区、子在别区」。计划阶段先跑一次 `WITH ... UPDATE` 的可行性探针，按结果二选一，不两条都留。
  4. **换区成功后活动区跟随到目标区**：同一个事务里把 `settings.active_workspace_id` 写成目标区。这不是顺手多做的事，是返回契约的前提——`update_project` 提交后要 `get_project(conn, id)` 回给命令层（`project.rs:231`），而 `get_project` 已按活动区过滤；不跟随的话「把项目挪到别区」这一次保存会**成功但回一个 `not_found`**，界面报失败、数据却已经改了。跟随之后的效果是顶栏下拉停在目标区、用户当场看见整棵移过去的子树；要回原区点一下下拉即可。
  5. **同区不变量的闭合**：一条父子边要么整条在、要么不存在（`validate_parent` 的同区断言，见下条），所以「把父项迁到它某个子节点所在的区」这种形状在数据里不可能出现，随迁流程**不需要**为它加额外检查。这句写在这里是为了防止实现者多加一层校验或为它写测试。
- **`validate_parent`（`project.rs:93`）加同区断言**：父项存在性那条 SQL 补 `AND workspace_id = ?活动区`，跨区父直接回「所属总包不存在或已删除」。加上 §4.1 的列表过滤，前后端都不可能出现跨区父子边。
- **树视图与候选**：`parentCandidates`（`src/lib/project-tree.ts`）拿到的列表已是活动区，无需改逻辑；跨区项目的父行在列表里不可见时，`ProjectForm` 现有的「候选里找不到就把父归到无」的归一化会把它当顶层项目渲染，而提交会被后端拒——界面对此的口径见 §八（顶栏下拉是当前区的唯一入口，不存在「父在别区、子在特区」的合法状态，真出现即数据损坏，走备份回滚）。
- **删除守卫不变**：有子仍禁删（M13），区不改变这条。

## 七、操作日志（M14）联动

- `project_audit.rs` 的 `input_fields`（`:92`）/`project_fields`（`:108`）各加一格 `workspaceId` / 标签「工作区」，值用**区名**而不是 id（`fn workspace_name(conn, id) -> AppResult<String>`，查不到回「已删除工作区」——与 `parent_name`（`:72`）同构；该区有项目时删不掉，所以这一支实际不可达，写它是为了让 `changes` 永不含裸 id）。
- 换区因此**自动进差异**：旧值/新值是两区的显示名，日志行只在**被编辑的那个根项目**下记一条。子树随迁的后代**不各自记日志**（否则一次换区产 N 条噪声行，而它们的 `workspace_id` 变化完全由父那条解释）——这条是有意的口径，写进验收格（§十 T-15）。
- `create`/`delete` 两条日志的 `changes` 里同样带「工作区」，`create` 记的是落进去的那个区。
- `audit_list` 的过滤见 §4.1：跨区日志在别区视野里不可见。已软删项目（`projects.deleted_at` 有值）的行仍留在 `projects` 表、仍带 `workspace_id`，所以它的日志照常出现在所属区；被**硬删**的项目行（只在测试/清理脚本里发生）会让它的日志从所有区视野消失——接受，M14 的「永久保留」承诺针对行不针对可见性，此处点名而不藏。

## 八、前端交互

### 8.1 类型与 API

- 新增 `src/types/workspace.ts`：`export interface Workspace { id: string; name: string; projectCount: number; isActive: boolean }`（字段名与 §五 的 camelCase 序列化逐一对齐）。
- `src/lib/api.ts` 加 5 个 invoke 包装：`listWorkspaces` / `createWorkspace` / `renameWorkspace` / `deleteWorkspace` / `switchWorkspace`，命令名字面量与 §五 一致。
- `src/types/project.ts`：`Project` 加 `workspaceId: string`（后端列 NOT NULL，恒有值）；`ProjectInput` 加 `workspaceId: string | null`（`null` = 沿用当前区）；`EMPTY_INPUT`（`:44`）补 `workspaceId: null`，`toInput`（`:59`）把 `p.workspaceId` 带进去。

### 8.2 store

新增 `src/stores/workspaces.ts`，zustand，照 `src/stores/templates.ts` 那套 `run()` 收口（成功覆盖本地行、失败留 `AppError` 三段式、`saving` 标志）。状态：`items: Workspace[]`、`activeId: string | null`（从 `items` 里 `isActive` 那格取，不另存一份真值）、`loading`、`saving`、`error`；动作：`refresh` / `create` / `rename` / `remove` / `switchTo`。`switchTo` 成功后 `refresh()` 一次（区名没变但 `isActive` 与列表可见性都变了，重取比在前端改标志更不易漂）。

### 8.3 顶栏切换器与管理弹窗

`src/components/app-shell.tsx`：header 里 `VaultControl`（`:42`）左侧加 `<WorkspaceSwitcher />`——一个 shadcn `Select`（当前区名直显，候选为全部区，切换即 `switchTo`）+ 一个「管理」按钮打开 `<WorkspaceManager />`。弹窗照 `src/components/template-manager.tsx` 的 Dialog 形态：一行一个区（名称可改名、`projectCount` 直显、删除按钮），底部一个新建输入行。删除按钮在「默认区 / 当前区 / `projectCount > 0`」三种情况下禁用（§十 T-16 的 `deleteGuard` 给判据），禁用时 `title` 写原因。

`AppShell` 现有的 mount effect（`app-shell.tsx:22-24` 那个 `useEffect`）加一句 `void refreshWorkspaces();`——顶栏下拉与项目表单的区候选首屏就得有值。

**换区即换整个视野**：`<main>`（`app-shell.tsx:46`，`<Outlet />` 在 `:47`）里把 `<Outlet />` 包一层 `<div key={activeId ?? "boot"}>`。活动区一变，整棵页面子树重挂载，各页现有的 `useEffect` 自然重新取数——比在每个页面/ store 里手写订阅少五处改动，也不会有「某页忘了订阅」的漏。活动区还没加载完（`activeId` 为 `null`）时子树照常挂载并发请求：后端已按活动区过滤，早到的请求不会串数据，加载完成后 key 从 `"boot"` 变成真 id 触发一次重挂载。这条取舍写明，不藏。

### 8.4 项目表单

`src/components/project-form.tsx` 在「所属总包」那一行附近加「工作区」一行，三种形态：

- **新建**：整行不渲染（后端恒落活动区，§六 第 1 条），提交 `workspaceId: null`。
- **编辑且 `parentId === null`**：`Select`，候选 = `workspaces.items`，初值 = `project.workspaceId`，提交所选 id。
- **编辑且 `parentId !== null`**：`Select` 禁用、只显示当前区名，下方灰字「子项目随父项目一起换区，请编辑它的父项目」，提交 `workspaceId: null`（不改归属）。

`src/pages/project-detail.tsx` 与 `src/pages/projects.tsx` 的父子区/树视图逻辑零改动——跨区数据在它们的取数里根本不出现。

### 8.5 纯函数与跟随语义

新增 `src/lib/workspace-view.ts`（前端可测的部分只有这三条，其余是接线）：

```ts
export function deleteGuard(ws: Workspace, activeId: string | null): string | null;
export function nameHint(name: string): string | null;   // 与后端同一判据：trim 非空、≤40 字符
export function activeWorkspaceId(items: Workspace[]): string | null;  // items 里 isActive 那格的 id
```

`deleteGuard` 返回禁用原因的中文文案（`"默认工作区不可删除"` / `"当前工作区不可删除，请先切换到其他工作区"` / `"该工作区下还有项目（含已删除项目），不能删除"`），可删时返回 `null`。三条原因与 §五 后端三条守卫逐字对应，前端只是提前把按钮置灰，**不是**第二道校验（判据仍只在后端一次）。

**换区跟随不需要前端特判**（§六.4）：后端把活动区跟着子树走了，`update_project` 返回的项目就在当前视野里，`useProjectsStore` 现有的 `upsert` 覆盖本地行仍然正确；紧接着 `<div key={activeId}>` 因 `switchTo`/`refresh` 后的活动区变化重挂载一次，列表按新区重取。**不要**在这里加「返回项目区号不等于当前区就从列表里摘掉」这类分支——那个状态在新契约下不会出现。

## 九、备份、检索与既有不变量

- **`Counts` 七键不变**（`backup.rs:127-133`）：`workspaces` 是新表，沿用 M12/M13/M14 的裁定——新表不进 Counts。`settings` 那一键会 +1（V11 种了 `active_workspace_id`），只影响数字不影响结构。
- **schema 版本**：`backup.rs` 里两处 `assert_eq!(sum.schema_version, 10)` 改成 **11**（`backup.rs:868` 与 `:895`，含那句「M14 起包内 schema 必须是 10」同步改字）。
- **明文告警**：`warn_plaintext_columns` 那句清单补「工作区名称」——它是业务命名，可能带客户名。
- **不进检索**：工作区名、`workspaces` 表不进 `field_hits`/FTS；操作日志照 M14 不进检索。
- **命令数门**：`generate_handler!` 70 → **75**；`tauri::command(async)` 仍 **7**；`allow(dead_code)` 仍 **1**（`db.rs`）；`Cargo.lock` 的 `[[package]]` 仍 **616**（零新依赖，`uuid`/`rusqlite` 都已在了）；`hits.append(&mut collect(` 仍 **9**（只改已有段的 SQL，不加新段）；`TARGET_VERSION` 10 → **11**。
- **索引 worker 不受活动区约束**（§4.1），所以 `index_overview` 里「本机总共扫了多少文件」这类总量口径不变；变的只是「按项目翻清单」的可见范围。
- **备份/恢复后活动区**：`settings` 行随包进出，导旧包时由 V11 补种 `default`；导入后重启前不主动切区（`backup_import` 之后界面走一次 `refresh`，活动区以后端 `settings` 为准）。

## 十、验收网格（后端 cargo 单测 + 前端 `node --test`，逐条具名）

| # | 归属 | 用例名 | 断什么 |
|---|------|--------|--------|
| T-1 | `db.rs` | `v11_schema_meta_and_tables_exist` | `workspaces`/`projects.workspace_id`/`ix_project_workspace` 三件都在，`MAX(version)=TARGET_VERSION` |
| T-2 | `db.rs` | `v11_backfills_existing_projects_into_default` | 迁移前沿默认区插入的裸 SQL 项目，迁移后 `workspace_id='default'` 且名称/标签/目录不变 |
| T-3 | `db.rs` | `v11_seeds_default_workspace_and_active_setting` | `workspaces` 有且仅有 `default`/`默认工作区`；`settings.active_workspace_id='default'` |
| T-4 | `db.rs` | `v11_is_atomic`（沿用 `apply_version` 探针形态） | V11 中途失败 → 无 `workspaces` 表、无列、无 `schema_meta` 行 |
| T-5 | `workspace.rs` | `active_id_defaults_when_setting_row_missing` | 手工 `DELETE` 掉设置行后 `active_id` 回 `default`，且 `project_list` 仍能出默认区项目 |
| T-6 | `workspace.rs` | `switch_writes_and_reads_back` | `switch` 到新建区 → `active_id` 变；`project_list` 立刻变空；切回后原样 |
| T-7 | `workspace.rs` | `create_rename_delete_guards` | 重名/空名/超 40 字/未知 id 四类错误码；删空区成功、删非空区被拒、删 `default` 被拒、删当前区被拒 |
| T-8 | `project.rs` | `create_project_ignores_input_workspace` | 输入里塞别区 id 仍落活动区（§六 第 1 条的孤儿防御） |
| T-9 | `project.rs` | `list_and_get_are_scoped_to_active_workspace` | 两区各一项目：当前区列表只 1 条，跨区 `get_project` 回 `not_found` |
| T-10 | `project.rs` | `move_subtree_follows_root_and_keeps_return_visible` | 三层树随根换区；**换区后活动区已是目标区，`update_project` 的返回值仍读得到**（§六.4 那条契约）；`switch` 回旧区后列表空、新区三层齐；父项 SQL 层断「跨区父被 `validate_parent` 拒」 |
| T-17 | 命令层守卫 | `row_for_preview_refuses_a_foreign_workspace_doc` + `workspace_guards_are_wired_before_the_work_in_command_bodies` | `ledger_export::collect` 前的 `ensure_active_project` 对跨区 project_id 回 `not_found` 且不写盘；`doc_preview::row_for_preview` 对跨区 doc_id 回 `preview_unavailable`；`index_overview` 的候选只含活动区项目（原单名在执行期拆成两条用例） |
| T-11 | `project.rs` | `child_project_cannot_move_alone` | 有 `parent_id` 的项目改 `workspace_id` → `invalid_input`，且数据未变 |
| T-12 | 关联表 | `todo_list_and_mutators_are_scoped` + `milestone_list_and_mutators_are_scoped` + `expiry_list_and_mutators_are_scoped` + `daily_report_list_and_mutators_are_scoped` + `ledger_read_and_write_paths_are_scoped` | 待办/里程碑/到期/日报四张表的 `list` 只出活动区；跨区 `create` 与跨区 id 的 `update`/`delete` 全回错误；`ledger` 的 `list`/`reveal`/`soft_delete`/五个 `update_*` 同口径（原单名在计划里拆成四条模块用例 + 台账一条，执行期已按此落地） |
| T-18 | `daily_report.rs` | `import_matches_only_active_workspace_projects` | 别区项目的行在 `preview` 里进 `rejected`、在 `apply` 里整份拒绝且库里一条不增 |
| T-13 | 检索 | `field_hits_are_scoped_in_every_section` + `doc_hits_and_indexed_project_count_use_the_same_scope` | 两区同名串：`field_hits`、`doc_hits`、`indexed_project_count` 都只回活动区，且首屏空态谓词与 `SELECT_SQL` 同口径：当前区一个项目都没有时 `indexed_projects` 回 0，即使别区有已建好正文的文档（原单名在执行期拆成两条模块用例） |
| T-14 | `backup.rs` | `backup_package_carries_workspaces_without_counting_them` + 既有两条断言 | 包内 schema=**11**（改既有两处 `assert_eq!(sum.schema_version, 10)`）、`workspaces` 行随整库快照进包、Counts 仍 7 键、告警句含「工作区名称」。**旧包导入那一半的覆盖方式**：`db.rs` 的 T-2 证「停在 V10 的库跑 migrate 补齐 V11」，`backup.rs:556` 那次 `crate::db::migrate(conn)` 调用与既有导入测试证「导入会跑 migrate」；本仓没有构造 v10 旧包的夹具，不为此提 `apply_version` 可见性（§三 已按此改口径） |
| T-15 | `project_audit.rs` | `workspace_change_is_logged_once_for_root` | 根换区 → 恰 1 条 update 日志、`changes` 里「工作区」旧名/新名正确；后代不各自出日志；`audit_list` 只出活动区日志 |
| T-16 | 新增 `src/lib/workspace-view.ts` 纯函数（§8.5） | `deleteGuard` / `nameHint` / `activeWorkspaceId` | `deleteGuard(ws, activeId)` 回三种禁用原因（默认区 / 当前区 / `projectCount>0`）与 `null`（可删）；`nameHint(name)` 对 trim 后空名/超 40 字回文案、合法回 `null`；`activeWorkspaceId` 在无 `isActive` 行时回 `null` |

前端另需在 `tests/workspace-view.test.ts` 里用 `node --test` + `node:assert/strict` 覆盖 §8.5 那三个纯函数（store 本身在本仓没有测试先例，不为此单开一套 mock 框架）。基线 **实测**：cargo 372 passed / node 71 tests 71 pass 0 fail（2026-10-23，`feat/m15-workspaces` 切出自 `master f714735`）；本轮新增条数在计划里逐 target 数实数后再写聚合数（不在 spec 里凭印象写总数）。

## 十一、真机验收格（CDP，一次性 identifier 沙盒）

沙盒纪律沿用 M13/M14：临时改 `tauri.conf.json` 的 `identifier` 编译、跑完逐字节还原、真实目录 `%APPDATA%\dev.zero.pfm\` 三文件前后 `size+mtime+sha256` 逐字一致；子代理不跑 `tauri dev`，进程只按记下的 PID 精确回收。

| 格 | 动作 | 期望读数 |
|----|------|---------|
| C-1 | 升级后首启（沙盒里先建 2 个默认区项目） | 顶栏下拉显示「默认工作区」，2 个项目照常可见 |
| C-2 | 弹窗新建「客户乙」 | 列表出现该区、`project_count=0`，**当前区仍是默认区**（不自动跳） |
| C-3 | 切到「客户乙」 | 项目列表变空；搜索页搜默认区项目名 0 命中；/todos、/daily-reports、/audit 全空 |
| C-4 | 在乙区新建项目 | 落进乙区；切回默认区后它消失，再切回来还在 |
| C-5 | 编辑项目改区（根项目、带 2 层子） | 提示换区成功；切到目标区后整棵子树三层齐；旧区列表无残留 |
| C-6 | 尝试给子项目改区 | 该区下拉禁用 + 文案点名「子项目随父项目一起换区」，保存不改动数据 |
| C-7 | 把项目父项选成别区项目 | 候选下拉里根本没有别区项目（列表已过滤），无法构造；后端侧以 T-10 的 SQL 断言代证 |
| C-8 | 日志页 | 只出当前区日志，「工作区」那条差异显示两区中文名 |
| C-9 | 删除非空区 / 删当前区 / 删默认区 | 三条 `invalid_input` 文案分别可见，区与项目都还在 |
| C-10 | 删空区 | 成功消失 |
| C-11 | 切区后 /index 页 | 项目清单只剩当前区项目；worker 总量口径不变 |
| C-12 | 备份导出 → 切区 → 导入该包 | 导入后活动区与区结构原样恢复，footer `schema=v11` |

## 十二、明确不做

1. **每区独立库文件 / 独立备份包 / 独立索引任务**：需求方选了逻辑隔离。物理隔离要动 `db::open` 的单连接前提（`AppState.conn` 一把 `Mutex<Connection>`）和备份的整库快照格式，代价与本需求不成比例。
2. **跨区父子边**：`validate_parent` 同区断言 + 列表过滤双堵（§六）。跨区「总包 + 分包」的视图本轮没有。
3. **跨区搜索 / 「全部工作区」聚合视野**：搜索只认当前区。要做聚合得给 `search_all` 加分区参数并改前端簇渲染，另立里程碑。
4. **模板按区分组**（D1 拍定全局）；`template_list`/`template_*` 一律不加区过滤。
5. **工作区颜色、图标、排序、归档、层级（区的区）**。
6. **多用户/权限**：仍是单机应用，区不是安全边界——同库同进程，任何拿到 `ledger.db` 的人都能看到全部区。这是逻辑分类，不是访问控制，交付文档必须照这句写，不能让「隔离」两个字被读成加密。
7. **区级待办/日报汇总看板**：换区即换视野，不做跨区统计。
8. **索引抓取按区开关**：worker 照旧全库扫（§4.1）。
9. **不回填、不迁移任何项目业务字段**：V11 只动 `workspace_id` 一列，其余列字节不变。

## 十三、回写清单（交付时）

- `docs/技术方案.md`：数据模型段加 `workspaces`（V11）与 `projects.workspace_id`；里程碑表加 M15 行；「隔离是逻辑分区、不是访问控制」与「索引 worker 全局扫、UI 按区呈现」两句写进架构口径。
- `docs/开发进度.md`：M15 验收证据小节（门禁串、变异清单、真机 12 格、spec↔代码映射）。
- `docs/HANDOFF.md`：M15 收口点、门禁期望串 `命令 75 / async 7 / TARGET_VERSION 11 / 豁免 1 / Cargo.lock 616 / collect 9 / cargo <实测> / node <实测>`、下一格候选（思维导图剩余项已清空，需与需求方重新取题）。
- `src-tauri/src/db.rs` 顶部迁移注释、`backup.rs` 告警句与两处 schema 断言。
- 变异靶与 deferred minor 记进 `.superpowers/sdd/2026-10-23-m15-workspaces/progress.md`（账本），至少覆盖：① 去掉 `list_projects` 区谓词、② 去掉 `validate_parent` 同区断言、③ 子项目单独换区不拒绝、④ `create` 改为信任 `input.workspace_id`、⑤ 去掉某个 `ensure_active_row` 守卫、⑥ `field_hits` 项目段与 JOIN 段只改一处（同口径谓词漏半边）、⑦ 删区非空守卫去掉、⑧ `indexed_project_count` 与 `SELECT_SQL` 谓词不同步、⑨ 换区不在同一事务（半途失败可见裂口）、⑩ V11 的 `settings` 改成 `INSERT`（非 IGNORE）。
