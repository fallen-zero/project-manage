# M14 设计：项目信息修改操作日志

**状态**：需求方 2026-10-23 逐题拍定（D1–D10），设计稿已确认，本文件是规格；实施计划另落 `docs/superpowers/plans/`。
**基线**：`master` = M13 收口点 `6e478f2`。M13 终态读数：cargo **359 passed**、node **66/66/0**、命令 **69**、async **7**、`TARGET_VERSION: i64 = 9`、`allow(dead_code)` **1**、`Cargo.lock` **616**、collect **9**、依赖 diff **空**、merge commit **0**。本批 V10 后 `TARGET_VERSION` 变 **10**、命令变 **70**，其余结构门不变。

## 一、需求出处与本轮兑现范围

需求原文：`docs/思维导图.md:75`「**项目信息修改操作日志**」（§四.3 可选迭代增强）。

本轮做**项目基础信息**的追加式审计：新建、编辑、删除项目时各写一条日志；编辑只记录实际变化的字段，保存项目名的快照，按字段存旧值与新值。日志在独立的「日志」页按项目/操作类型筛选、分页查看；永久保留，不提供手动删除。

**明确不做（见 §十一）**：目录映射、台账、待办、里程碑、日报、到期事项、模板、备份导入的操作日志；失败操作审计；操作人/多用户字段；日志搜索、导出、手动清空/删除、自动裁剪；项目详情页重复放日志块。

## 二、需求方拍定的口径

- **D1 记录范围**：只记项目基础信息的 `create` / `update` / `delete`。基础信息字段 = `name` / `code` / `manager` / `customer` / `contactName` / `contactPhone` / `contractNo` / `contractPeriod` / `deliveryDeadline` / `status` / `tags` / `parentId`。
- **D2 成功才记**：只有命令成功后才写日志；校验失败、未知 id、有子项目导致删除被拒、事务回滚都不写。
- **D3 无变化不记**：编辑后所有基础字段（按归一化后比较）都没有变化 ⇒ 不写日志；保存动作本身不构成一条日志。
- **D4 字段级差异**：`changes` 是字段数组，每项含字段 key、中文 label、旧值、新值；新建记初始非空字段（`name`/`status` 必有），删除记删除前非空字段快照。
- **D5 原值存储**：字段旧值/新值存归一化后的**原值**，含联系人、联系方式；不脱敏、不截断。`tags` 按 trim/去空/去重后**排序**比较与展示；`parentId` 存当时的父项目名（空值显示「无（顶层项目）」），不把 UUID 直接展示给用户。
- **D6 项目名快照**：每条日志存 `projectName`（create/update 用新名，delete 用删除前名）；项目改名或删除后，历史日志仍显示当时的名字。
- **D7 存储与保留**：V10 新建只追加的 `project_audit_logs`；无 `updated_at`/`deleted_at`、无外键、无自动裁剪、无手动删除；项目软删后日志保留。
- **D8 查询**：新增一条同步命令 `audit_list`（`generate_handler!` **69→70**，本仓第 8 次命令数破例；async 仍 **7**）：按项目/操作类型筛选、`limit/offset` 分页，返回 `{ items, hasMore, projects }`；默认每页 **200** 条，最新在前。
- **D9 页面**：nav 第 7 格「日志」→ `/audit`，全局列表；不新增项目详情日志块。项目筛选选项来自日志表（含已删除项目），不是当前项目列表。
- **D10 备份与检索**：日志随整库快照进备份包；`backup::Counts` 仍 **7 键**；包 `schemaVersion` **10**，旧包导入跑 V10 建空表；`warn_plaintext_columns` 补「项目修改日志的旧值/新值」；日志不进全局检索，`collect` 仍 **9**。

## 三、数据模型与 V10 迁移

```sql
CREATE TABLE project_audit_logs (
    id           TEXT PRIMARY KEY,
    project_id   TEXT NOT NULL,
    project_name TEXT NOT NULL,
    action       TEXT NOT NULL CHECK (action IN ('create','update','delete')),
    changes      TEXT NOT NULL DEFAULT '[]',
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX ix_audit_created ON project_audit_logs(created_at);
CREATE INDEX ix_audit_project ON project_audit_logs(project_id, created_at);
```

- `TARGET_VERSION` **9→10**；`migrate` 的 `match` 加 `10 => V10`；**V10 零回填**（迁移里只有 DDL，不写任何日志行）。
- **不建外键**：项目是软删，日志必须跨项目删除存活；`project_id` 只是快照关联，不保证项目当前仍存在。
- **append-only**：没有 `updated_at` / `deleted_at`；没有任何 UPDATE/DELETE 命令。日志不参与项目软删级联。
- `changes` 是 JSON 数组，元素形状固定：
  ```json
  [
    {"field":"customer","label":"客户名称","oldValue":"甲客户","newValue":"乙客户"}
  ]
  ```
  `oldValue` / `newValue` 是 `string | null`；新建项 `oldValue` 为 null，删除项 `newValue` 为 null；可选字段为空用 null，前端显示「—」。`parentId` 字段的 null 值在存储层写成 `"无（顶层项目）"` 字符串（父项名称同理），这样历史日志在父项目改名/删除后仍可读。
- 排序：`ORDER BY created_at DESC, rowid DESC`（`created_at` 只到秒，rowid 让同一秒内的日志保持插入顺序）。
- 日志表随整库快照进备份包；`strip_index` 不碰它。

## 四、写入语义与事务

### 4.1 字段归一化与差异

- 文本字段按 `project.rs` 的现有 `normalize` 规则：trim；空串 ⇒ null。
- `name` 去首尾空白且非空；`status` 保持枚举值。
- `tags`：trim、去空、按首次出现去重，再按字符串排序后比较/展示；展示用「、」连接。
- `parentId`：比较用 id；展示时在写入事务内解析父项目当前名称；空 ⇒ `"无（顶层项目）"`；父项目缺失/已删（防御）⇒ `"已删除项目"`。
- 新建日志：取归一化后的新值，记录所有**非空**字段（`name`、`status` 必有；可选空字段不产生 changes 项）。
- 编辑日志：逐字段比较归一化后的旧值与新值，只记录不同的字段；清空字段产生 `oldValue=旧值, newValue=null`。
- 删除日志：记录删除前所有非空字段，`newValue=null`；含删除前的 `name`/`status`。
- 编辑无变化：不插入日志；业务更新是否照常执行沿用现有 `update_project` 行为（`updated_at` 仍更新）。

### 4.2 原子性

- `create_project` / `update_project` / `delete_project` 各自用 `unchecked_transaction` 包住业务写入、tags 替换与日志插入；任一步失败整体回滚，**不会留下“项目改了但日志没写”或反之**。
- `update_project` 在事务内先读旧 `Project`，再做现有校验/更新；`delete_project` 在事务内读旧 `Project` 并数子项；`get_project` 的返回仍在提交后读取（与现有形制一致）。
- 日志插入失败（例如 JSON 序列化异常）返回 `db_failed`，事务回滚；项目的既有错误码（`invalid_input` / `not_found`）不变。

### 4.3 模块边界

- 新增 `src-tauri/src/project_audit.rs`：
  - 类型 `AuditChange` / `AuditEntry` / `AuditProjectOption` / `AuditPage`（serde camelCase）。
  - `record_create(conn, project_id, input)` / `record_update(conn, old_project, input)` / `record_delete(conn, old_project)`：计算 diff、解析父项显示名、写日志行。
  - `list(conn, project_id, action, limit, offset)`：过滤、分页、`projects` 选项。
  - 模块内单测覆盖 diff/归一化/JSON/分页。
- `project.rs` 只在成功写入路径调用 `project_audit::record_*`；日志表不进入 `search.rs`、不进入 `backup::Counts`。

## 五、命令契约

| 命令 | 入参 | 返回 | 说明 |
|---|---|---|---|
| `audit_list` | `{ projectId?: string, action?: string, limit?: number, offset?: number }` | `AuditPage` | 同步；筛选 + 分页；最新在前 |

**线格式**（Rust `#[serde(rename_all = "camelCase")]`，TS 一一对应）：

```ts
export type AuditAction = "create" | "update" | "delete";

export interface AuditChange {
  field: string;
  label: string;
  oldValue: string | null;
  newValue: string | null;
}

export interface AuditEntry {
  id: string;
  projectId: string;
  projectName: string;
  action: AuditAction;
  changes: AuditChange[];
  createdAt: string;
}

export interface AuditProjectOption {
  projectId: string;
  projectName: string;
}

export interface AuditPage {
  items: AuditEntry[];
  hasMore: boolean;
  projects: AuditProjectOption[];
}
```

- `limit` 默认 **200**，允许范围 **1..=200**；`offset` 必须 ≥ 0；`action` 只接受 `create`/`update`/`delete`（或省略）；非法值 `invalid_input`。
- 查询取 `limit + 1` 行判断 `hasMore`，返回时截到 `limit`。
- `projects` 是日志表中每个 `project_id` 的最新快照名（按 project_name 排序），包含已删除项目；不来自当前 `project_list`。
- 命令一律同步；`generate_handler!` **70**；`tauri::command(async)` 仍 **7**；无新 `allow(dead_code)` 豁免。

## 六、前端交互

### 6.1 类型、API 与页面

- `src/types/project-audit.ts`：上表类型。
- `src/lib/api.ts`：`listAudit({ projectId?, action?, limit?, offset? })` → `invoke("audit_list", ...)`。
- `src/pages/audit.tsx`：页面级状态（items/loading/hasMore/projects/filters/offset），不使用全局 store；筛选变化重置 offset 并重新拉取；「加载更多」追加下一页。用请求序号/取消标记忽略过期响应，避免快速切筛选时旧结果覆盖新结果。
- `src/router.tsx` 加 `/audit` 路由；`src/components/app-shell.tsx` 的 nav 加第 7 格「日志」。

### 6.2 展示

- 标题「操作日志」，副标题说明「只记录项目基础信息的新建/编辑/删除，永久保留」。
- 两个筛选：项目（「全部项目」+ `projects` 选项）与操作类型（「全部操作」/新建/编辑/删除）。
- 每条：本地时间（库里 UTC，展示时转本地 `YYYY-MM-DD HH:mm`）、项目名快照、操作徽标、字段差异。
  - create：`字段：值`（初始值）
  - update：`字段：旧值 → 新值`（null 显示「—」）
  - delete：`字段：删除前值`
- 空态「还没有操作日志」；无更多时隐藏「加载更多」；请求失败在页面错误条显示 `message + hint`。
- 不做日志内容搜索、导出、清空/删除、日期范围筛选。

### 6.3 纯函数

- `src/lib/audit-view.ts`（或页面同目录的纯函数）至少包含：
  - `actionLabel(action)`：新建/编辑/删除。
  - `formatChangeText(action, change)`：按 create/update/delete 生成差异文案，null → 「—」。
  - `formatAuditTime(utc)`：把 SQLite `YYYY-MM-DD HH:MM:SS` 按 UTC 解析后转本地展示；非法输入原样返回。
- 纯函数由 `node --test` 守；页面接线靠真机格。

## 七、备份、检索与既有不变量

- **备份**：`project_audit_logs` 随整库一致快照进包；`Counts` 七键不变；包 `schemaVersion=10`；旧包（9）导入时尾部 `migrate` 建 V10 空表；`warn_plaintext_columns` 改为点名「项目修改日志的字段旧值/新值（含联系人、联系方式）」。
- **检索**：日志不进 `search.rs`；collect 仍 9；搜索不到日志里的旧值（旧值恰好只存在于日志时）。
- **项目删除**：软删项目后日志保留；日志页仍可按项目筛选到已删项目。
- **模板/父项**：模板 CRUD 不写日志；套模板产生的项目写操作按最终 `ProjectInput` 正常记一条（无字段变化则不记）；父项变更新旧值显示名称。
- **结构门**：命令 **70** / async **7** / `TARGET_VERSION: i64 = 10` / 豁免 **1** / `Cargo.lock` **616** / collect **9**；`Cargo.toml`/`Cargo.lock`/`package.json` 对 `master` diff **空**；merge commit **0**。

## 八、验收网格

| # | 格 | 判据 | 档 |
|---|---|---|---|
| 1 | 日志页入口 | nav「日志」→ `/audit`；空库显示「还没有操作日志」 | 真机 |
| 2 | 新建日志 | 新建项目 ⇒ 一条 create；显示初始的非空字段与项目名快照 | 真机 + cargo |
| 3 | 编辑差异 | 只改客户与状态 ⇒ 一条 update，changes 只有这两项，旧→新正确 | 真机 + cargo |
| 4 | 无变化不记 | 打开编辑直接保存 ⇒ 不新增日志 | 真机 + cargo |
| 5 | 清空字段 | 把客户清空 ⇒ update 显示「客户：旧值 → —」 | 真机 + cargo |
| 6 | tags 归一化 | 仅调整 tags 顺序/空格 ⇒ 不记日志；真正增删 tag ⇒ 记录排序后的差异 | 真机 + cargo |
| 7 | 父项名称 | 改所属总包 ⇒ update 的 old/new 是父项目名（空为「无（顶层项目）」），不是 UUID | 真机 + cargo |
| 8 | 删除快照与保留 | 删除项目 ⇒ 一条 delete，含删除前快照；项目列表消失但日志仍在、可筛到 | 真机 + cargo |
| 9 | 筛选与已删项目 | 项目/操作类型筛选生效；已删项目仍出现在项目选项 | 真机 + cargo |
| 10 | 分页 | `audit_list` 的 limit/offset/hasMore 正确；日志页「加载更多」追加且不重复 | cargo + 真机（API 分页；UI 若日志量不足则记限制） |
| 11 | 备份与明文告警 | 包内 `project_audit_logs` 行存在；`Counts` 7 键；schema 10；告警含日志旧值/新值 | cargo + 真机 |
| 12 | 失败不记 / 检索隔离 | 校验失败、有子禁删、无变化都不写；`search_all(仅日志旧值)` 0 命中；collect 9 | cargo + 真机 |

## 九、变异靶（至少八处，每处具名打红）

| # | 变异 | 期望红在哪 |
|---|---|---|
| M① | create 不写日志 | create 日志测试 |
| M② | update 不写日志 | update 差异测试 |
| M③ | update 记录整行/未变字段 | 编辑差异测试（未变字段不得出现） |
| M④ | 无变化也写日志 | 无变化不记测试 |
| M⑤ | 父项差异写 UUID | 父项名称测试 |
| M⑥ | tags 不 trim/排序 | tags 归一化测试 |
| M⑦ | delete 不写快照或先删后读导致快照空 | delete 快照测试 |
| M⑧ | V10 改成空 DDL | V10 表/索引测试 |
| M⑨ | `audit_list` 忽略 project/action 筛选 | 筛选测试 |
| M⑩ | `audit_list` 忽略 limit/offset 或 hasMore 恒 false | 分页测试 |
| M⑪ | 明文告警删「项目修改日志」 | 备份告警测试 |

## 十、明确不做

- 目录映射、台账、待办、里程碑、日报、到期事项、模板、备份导入的操作日志。
- 失败操作、被拒操作的审计；操作人/账号/多用户字段；IP/设备信息。
- 日志的手动删除/清空、自动裁剪、回收站；日志编辑。
- 日志全文搜索、Excel/Markdown 导出、日期范围筛选、项目详情页日志块。
- 旧数据的日志回填（V10 零回填；上线前的修改没有日志）。

## 十一、回写清单（交付时）

- `docs/技术方案.md`：§四数据模型加 `project_audit_logs`；§八加 M14 行。
- `docs/开发进度.md`：里程碑表 M14 行 + 「## M14 验收证据」节 + 变更日志。
- `docs/HANDOFF.md`：快照头、§0 M14 bullet、§3 下一格、§4-A 期望数（命令 70 / TARGET 10 / cargo/node 起增）、§6 指针。
- 项目记忆：M14 口径与交付终态。
- 证据目录 `.superpowers/sdd/2026-10-23-m14-audit-log/`（git-ignored，收口前别清）。
