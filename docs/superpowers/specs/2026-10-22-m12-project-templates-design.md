# M12 设计：项目模板（新建项目一键填充）

**状态**：需求方 2026-10-22 逐题拍定（D1–D10），设计稿已确认，本文件是规格；实施计划另落 `docs/superpowers/plans/`。
**基线**：`master` = M11-b 收口点（代码合并点 `3efbc18`，收口文档 `7e7b078`）。开工前实测：cargo **337 passed**、node **52/52/0**、命令 **65**、async **7**、`TARGET_VERSION: i64 = 7`、`allow(dead_code)` **1**、`Cargo.lock` **616**、collect **9**、依赖 diff **空**、merge commit **0**。本批 V8 后 `TARGET_VERSION` 变 **8**、命令 **69**，其余结构门不变。

## 一、需求出处与本轮兑现范围

需求原文：`docs/思维导图.md` §四.3（可选迭代增强）「**项目信息模板（新建项目一键填充）**」。

本轮做**用户自建、可增删改的模板**：模板保存一整套**项目基础信息**；新建项目时从下拉选模板，把模板字段预填进现有 `ProjectForm`，用户补项目名后照常保存。模板不参与项目创建之外的任何通路。

**明确不做（见 §十一）**：从现有项目另存为模板、模板复制/排序/默认模板、模板套用到已有项目、台账行/目录映射/待办/里程碑进模板、模板进全局检索或备份 `Counts`、软删模板、新增设置页。

## 二、需求方拍定的口径

- **D1 模板内容**：只带项目基础字段 —— `code` / `manager` / `customer` / `contactName` / `contactPhone` / `contractNo` / `contractPeriod` / `deliveryDeadline` / `status` / `tags`；**不含项目名**。模板名（`name`）只是模板的标识，绝不写进项目名。
- **D2 管理入口**：项目页右上角加「模板」按钮 → 管理弹窗（列表 / 新建 / 编辑 / 删除确认）；**新建项目**对话框顶部加「从模板填充」下拉；**编辑已有项目不显示模板下拉**。
- **D3 套用语义**：模板里**有值**的字段覆盖表单当前值；**项目名永不动**；模板里为 `null`/空白的字段**不清空**已有值；`status` 总是覆盖（模板的 status 是必填枚举）；`tags` 只在模板非空时覆盖；选模板后可继续手改，也可以再切另一个模板。
- **D4 模板创建**：本轮只做**独立模板表单**（在管理弹窗里新建/编辑），不做「从现有项目另存为模板」。
- **D5 存储**：V8 新建 `project_templates` 表；**模板硬删除**（无 `deleted_at`），因为它是配置型快捷方式、不是业务记录、没有外键引用；模板名 `UNIQUE`；tags 存 JSON 数组。
- **D6 命令**：`template_list` / `template_create` / `template_update` / `template_delete` 四条**同步**命令；`generate_handler!` **65→69**（本仓第 7 次批准破例）；`tauri::command(async)` 仍 **7**。
- **D7 备份**：模板随整库快照自动进备份包；`backup::Counts` **仍是七张表**（不加模板计数，沿用 M9/M11 的新表口径）；`warn_plaintext_columns` 补「项目模板」；包内 `schemaVersion` 变 **8**，旧包（7）仍可导入并跑 V8 迁移。
- **D8 检索**：模板**不进**全局检索；`search.rs` 的 `collect` 仍 **9** 条。
- **D9 排序**：`template_list` 按 `created_at, id`（创建顺序，同一秒用 id 兜底）；本轮不做手动排序。
- **D10 错误面**：只用 `invalid_input` / `not_found`；重名返回 `invalid_input` 并点名模板名；非法 status 返回 `invalid_input`；更新/删除不存在的 id 返回 `not_found`。

## 三、数据模型与 V8 迁移

```sql
CREATE TABLE project_templates (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL UNIQUE,
    code              TEXT,
    manager           TEXT,
    customer          TEXT,
    contact_name      TEXT,
    contact_phone     TEXT,
    contract_no       TEXT,
    contract_period   TEXT,
    delivery_deadline TEXT,
    status            TEXT NOT NULL DEFAULT '立项'
                      CHECK (status IN ('立项','需求','开发','测试','预发','上线','运维','归档')),
    tags              TEXT NOT NULL DEFAULT '[]',
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now'))
);
```

- `TARGET_VERSION` **7→8**；`migrate` 的 `match` 加 `8 => V8`；**V8 零回填**（不猜任何模板；老库迁移后模板表为空）。
- 除 `name` 的 UNIQUE 自动索引外不建额外索引：模板是几十行以内的配置表，列表只按 `created_at, id` 全表读。
- **硬删除**：`template_delete` 直接 `DELETE`；`changes()==0` 即 `not_found`。删除后同名模板可以重建（UNIQUE 不残留）。这与「业务表软删三件套」相反，理由是本表不是业务记录；spec 明确记录，避免下一个对话误加 `deleted_at`。
- **tags 是 JSON 数组**：写入侧 `serde_json::to_string`（先 trim、去空、按首次出现去重），读出侧 `serde_json::from_str::<Vec<String>>`；若库被手改成非法 JSON 则返回 `db_failed`（消息点名「模板标签不是合法 JSON」，hint 提示检查是否手改过库），**不静默丢标签**。
- `name` 唯一性：应用层先查重（`SELECT COUNT(*) ... WHERE name = ? AND id <> ?`，新建时 `id` 用空串占位）返回 `invalid_input`；DB `UNIQUE` 只作绕过命令直写的兜底（落成 `db_failed` 可接受，命令路径不会走到）。
- 备份边界：`backup::Counts` 的七字段**不动**；`export` 的整库快照自动包含本表；`strip_index` 只剥 `index_docs`/`index_docs_fts`，不碰本表；`backup::warn_plaintext_columns` 改成：
  `"包内非 *_cipher 列为明文：项目名/编号/负责人/客户/联系人/合同号（含项目模板的同类字段）、服务器 ip 与堡垒机、各类 url、各表 note、待办内容、里程碑备注、日报内容、到期事项名称与备注。"`
  旧包（schema 7）通过 `schema_version > TARGET_VERSION` 判据仍可导入，导入尾部 `db::migrate` 会建 V8 表。

## 四、命令契约

| 命令 | 入参 | 返回 | 说明 |
|---|---|---|---|
| `template_list` | 无 | `ProjectTemplate[]` | `ORDER BY created_at, id`；无软删过滤 |
| `template_create` | `{ input: TemplateInput }` | `ProjectTemplate` | 返回新建行；重名/空名/非法状态均 `invalid_input` |
| `template_update` | `{ id: String, input: TemplateInput }` | `ProjectTemplate` | 整行覆盖；id 不存在 `not_found`；重名（排除自己）`invalid_input` |
| `template_delete` | `{ id: String }` | `void` | 硬删；id 不存在 `not_found` |

**线格式**（Rust `#[serde(rename_all = "camelCase")]`，TS 一一对应）：

```ts
interface ProjectTemplate {
  id: string;
  name: string;                 // 模板名，不是项目名
  code: string | null;
  manager: string | null;
  customer: string | null;
  contactName: string | null;
  contactPhone: string | null;
  contractNo: string | null;
  contractPeriod: string | null;
  deliveryDeadline: string | null;
  status: string;
  tags: string[];
}
interface TemplateInput {
  name: string;
  code: string | null;
  manager: string | null;
  customer: string | null;
  contactName: string | null;
  contactPhone: string | null;
  contractNo: string | null;
  contractPeriod: string | null;
  deliveryDeadline: string | null;
  status: string;
  tags: string[];
}
```

- 命令一律同步、函数体零 `await`；`db(&state)` 的 `MutexGuard` 不跨 await（沿用仓内既有形制）。
- 校验顺序（执行期订正，按实现）：`normalize` 先做 `name` trim 非空、status 枚举与 tags 归一化 → 重名（更新时排除自己）→ 写库。重名与非法 status 同时出现时先报 status；该优先级只在直接 IPC/测试里可见，UI 的 status 来自枚举下拉、构造不出组合。错误文案点名具体字段（「模板名称」「状态」）与合法值。
- `create/update` 的成功返回都重新读库（`get` 形制），不在 Rust 里手拼行，保证默认值/`updated_at` 与库一致。
- 命令数：**69**；`command(async)` 仍 **7**；全仓 `allow(dead_code)` 仍 **1**（新模块的四个入口都已注册）。

## 五、前端交互

### 5.1 类型、API 与状态

- `src/types/project-template.ts`：`ProjectTemplate` / `TemplateInput` / `EMPTY_TEMPLATE_INPUT`（`name: ""`、`status: "立项"`、其余字段空串、`tags: []`）。
- `src/lib/api.ts`：`listTemplates()` / `createTemplate(input)` / `updateTemplate(id, input)` / `deleteTemplate(id)`，全部 `invoke` 到 §四 的命令。
- `src/stores/templates.ts`：轻量 zustand store（`items` / `loading` / `saving` / `error` / `refresh` / `save(id|null, input)` / `remove(id)`），形制照 `stores/projects.ts`：每个写操作返回 boolean、失败把 `AppErrorShape` 留给 UI、成功用后端返回的整行 upsert。项目页挂载时 `refresh()`。

### 5.2 复用 `ProjectForm`

- `src/components/project-form.tsx` 增加两个**可选**文案 prop：`nameLabel?: string`（默认「项目名称」）与 `nameRequiredMessage?: string`（默认「项目名称不能为空」），以及一个可选 `onChange?: (input: ProjectInput) => void`（把 name/status/FIELDS/tags 的 live 值回传）；`project-detail.tsx` 与模板管理弹窗不传，行为逐字不变；**新建项目对话框传 `onChange` 仅同步 live 值**，用户可见行为不变（Task 6 修复轮，见 §五.4）。
- 模板管理弹窗用同一个 `ProjectForm` 渲染模板表单：`nameLabel="模板名称"`、`nameRequiredMessage="模板名称不能为空"`；`initial` 用 `templateToFormInput(t)`（把 `ProjectTemplate.name` 放进 `ProjectInput.name` 这一格，其余字段一一映射）。提交时把 `ProjectInput` 直接当 `TemplateInput` 交给 store（结构同形）。

### 5.3 模板管理弹窗

- 项目页标题行加「模板」按钮（`variant="outline"`，与「新建项目」并排），点击打开 `TemplateManager`。
- 弹窗内一个视图状态：列表 / 表单。列表显示模板名、状态徽标、tags 徽标、编辑与删除按钮；空列表显示「还没有模板」与「新建模板」。
- 新建/编辑在同一弹窗内切到 `ProjectForm`；保存成功回列表并刷新 `items`。错误（重名/空名/状态）在弹窗内显示 `error.message + hint`，不写到项目页的全局错误条。
- 删除用一个 shadcn `Dialog` 二次确认（**不用 `window.confirm`**）；确认后 `remove(id)`，列表与新建项目下拉同步消失。
- 模板管理器只处理模板；不改项目，不引入新的设置页或 nav 格。

### 5.4 新建项目对话框与套用

- `ProjectsPage` 的新建/编辑弹窗在 `editing.id === null` 时，`ProjectForm` 上方渲染「从模板填充」`Select`：首项「不使用模板」，其余是 `templates.items` 的模板名；无模板时下拉禁用并提示「暂无模板，先在「模板」里新建」。
- 选中模板 `t` 时：`setEditing(prev => ({ ...prev, input: applyTemplate(prev.input, t) }))`，并递增 `formKey`；`<ProjectForm key={formKey} initial={editing.input} …>` 以新初值重挂。递增 key 是承重细节：`ProjectForm` 用 `useState(initial)` 初始化，直接改 `initial` 不会刷新内部 state。
- **`applyTemplate(input, template)` 纯函数**（`src/lib/apply-template.ts`）：
  - `name: input.name`（**永不使用 `template.name`**）；
  - 可选字段：模板值非 `null` 且 trim 后非空 ⇒ 用模板值，否则保留 `input` 当前值；
  - `status: template.status`（总覆盖）；
  - `tags: template.tags.length > 0 ? [...template.tags] : [...input.tags]`；
  - 返回新对象，不修改入参。
- **live 值同步（Task 6 修复轮）**：`ProjectForm` 通过可选 `onChange` 把当前表单的 name/status/FIELDS/tags 回传；新建项目对话框用它更新 `editing.input`，因此先输项目名再选模板时 `applyTemplate(prev.input, t)` 拿到的是最新名字，`formKey` 重挂不会丢。
- 应用后把下拉重置回「不使用模板」，这样再选同一个模板仍能触发 `onValueChange` 重新套用。
- 编辑已有项目（`editing.id !== null`）不渲染模板下拉；模板只服务新建。

### 5.5 页面层边界

页面/弹窗没有 jsdom 自动化网（spec §十一 不引 jsdom）；按钮、下拉、重名错误、删除确认、预填结果全部靠真机格（§八）。`applyTemplate` 是纯函数，由 `node --test` 守。

## 六、不变量（逐条要有落点）

1. **模板只含基础字段**：不出现项目名、台账行、目录映射、待办、里程碑、密文；`Template` 线格式没有这些键。
2. **项目名与模板名不串**：`applyTemplate` 只写 `input.name` 以外的字段；真机格「先输项目名再选模板」必须看到项目名不变。
3. **套用语义**：有值覆盖、空值不清空、status 总覆盖、tags 非空才覆盖；纯函数不可变；node 断言逐条。
4. **校验双轨**：应用层给 `invalid_input`（点名模板名/状态），DB 的 `UNIQUE`/`CHECK` 只挡直写库；命令路径不落 `db_failed`。
5. **硬删除**：删后表内行数为 0、列表/下拉不再返回、同名可重建；不出现 `deleted_at` 过滤。
6. **tags JSON 往返**：写前 trim/去空/去重；读回同序；非法 JSON 返回 `db_failed`，不静默丢。
7. **模板不进检索**：`grep -c 'hits.append(&mut collect(' src-tauri/src/search.rs` 仍 **9**；`search_all(模板名)` 真机 0 命中。
8. **备份口径**：模板随整库快照进包；`backup::Counts` 仍 7 键（无模板计数）；`warn_plaintext_columns` 含「项目模板」；包 `schemaVersion=8`；旧包（7）可导入。
9. **结构门**：命令 **69** / async **7** / `TARGET_VERSION: i64 = 8` / `allow(dead_code)` **1** / `Cargo.lock` **616** / collect **9**；`Cargo.toml`/`Cargo.lock`/`package.json` 对 `master` diff **空**；merge commit **0**。
10. **既有表单不变**：`ProjectForm` 的 `nameLabel`/`nameRequiredMessage`/`onChange` 都是可选；`project-detail.tsx` 与模板管理弹窗不传，原有新建/编辑行为与文案逐字不变；新建项目对话框只传 `onChange` 同步 live 值（Task 6 修复轮），用户可见行为不变。

## 七、测试与取证档

**cargo（主体）**：
- V8：`schema_meta` 到 8、`project_templates` 表与 `name` 唯一索引存在、迁移后模板行数为 0（零回填）。
- `template.rs`：create/list/update/delete 往返（全字段 + tags）；重名（新建与更新排除自己）；空名；非法 status；未知 id 的 update/delete ⇒ `not_found`；tags trim/去空/去重与 JSON 往返；硬删除后行数与同名重建；`list` 的 `created_at, id` 稳定顺序。
- `backup.rs`：`plaintext_warning_names_every_plain_text_table_since_m9` 增加「项目模板」断言；`Counts` 七键测试保持 7（不加模板字段）。再加一支**包内容**测试：建一个含模板的文件库，`backup::export` 到临时 zip，打开包内 `backup.db` 断言 `project_templates` 行存在、`Counts` 仍 7 键、manifest `schemaVersion=8`（补上格 11 的 cargo 半边）。
- 具体测试条数在实施计划里逐支数出（不先写死）；预期 cargo 从 337 起增，最终以实跑 `test result` 为准。

**node**：
- 新增 `tests/apply-template.test.ts`：项目名保留、非空字段覆盖、空/null 字段不清空、status 总覆盖、tags 非空才覆盖且空 tags 保留原值、返回新对象不改入参。具体条数在计划里数出（node 52 起增）。

**真机（沙盒 `dev.zero.pfm.m12test` + CDP 9333；真实数据目录三文件前后 `size+mtime+sha256` 逐字一致；跑完删净）**：见 §八。

## 八、验收网格

| # | 格 | 判据 | 档 |
|---|---|---|---|
| 1 | 模板管理入口 | 项目页「模板」按钮打开弹窗；空库显示「还没有模板」与「新建模板」 | 真机 |
| 2 | 新建模板 | 模板名 + 经理/客户/合同/状态/标签保存后，列表出现且字段/标签正确 | 真机 + cargo |
| 3 | 编辑模板 | 改经理与标签后列表更新；新建项目下拉预填新值 | 真机 + cargo |
| 4 | 重名模板 | 再建同名模板 ⇒ `invalid_input` 点名模板名，弹窗内显示错误 | 真机 + cargo |
| 5 | 新建项目预填 | 选模板 ⇒ 模板字段预填、项目名为空；输入项目名保存 ⇒ 项目行字段与模板一致 | 真机 |
| 6 | 项目名保留 | 先输入项目名再选模板 ⇒ 项目名不变，模板字段覆盖已有值 | 真机 + node |
| 7 | 空字段不清空 | 模板 B 的 `customer` 为空，选 B 后模板 A 预填的 customer 仍保留 | 真机 + node |
| 8 | 编辑项目无模板 | 编辑已有项目时没有「从模板填充」下拉 | 真机 |
| 9 | 删除模板 | 删除确认后列表与下拉消失；同名可重建（硬删） | 真机 + cargo |
| 10 | 模板不进检索 | `search_all(模板名)` 0 命中；collect 仍 9 | 真机 + cargo |
| 11 | 备份包含模板 | `backup_export` 后包内 `project_templates` 有行；`Counts` 仍 7 键；`schemaVersion=8` | 真机 + cargo |
| 12 | 迁移与校验 | V8 表/唯一索引；空名/非法 status/未知 id 的 cargo 拒绝 | cargo |

## 九、变异靶（至少六处，每处具名打红）

| # | 变异 | 期望红在哪 |
|---|---|---|
| M① | 删应用层重名检查 | 重名测试由 `invalid_input` 变 `db_failed`/成功 |
| M② | 删空名 trim/非空校验 | 空名测试 |
| M③ | 删 `project::STATUSES` 校验 | 非法 status 测试 |
| M④ | 删 tags trim/去空/去重 | tags 往返测试 |
| M⑤ | 把硬删改成不删或软删 | 删除后行数/同名重建测试 |
| M⑥ | `applyTemplate` 用 `template.name` 写 `input.name` | node「项目名保留」 |
| M⑦ | `applyTemplate` 用空模板字段覆盖已有值 | node「空字段不清空」 |
| M⑧ | V8 不建 `project_templates` | db 表/唯一索引测试 |
| M⑨ | 明文告警句删「项目模板」 | `plaintext_warning...` 测试 |
| M⑩ | `template_list` 去掉 `ORDER BY` | 稳定顺序测试（构造乱序 created_at） |

## 十、备份、检索与既有不变量

- **备份**：模板随整库快照进包；`Counts` 七键不变（备份页计数行不新增）；`warn_plaintext_columns` 改成点名项目模板的同类字段；`schemaVersion` 8；旧包（7）可导入，导入尾部迁移建 V8 表；新包在旧版本应用上按 `schema_version > TARGET_VERSION` 拒绝。
- **检索**：模板不进 `search.rs`；`collect` 9 条不变；不做 FTS。
- **命令/结构门**：69 / 7 / 8 / 1 / 616 / 9；零新依赖；merge commit 0。
- **既有表单**：`ProjectForm` 的三个可选 prop 有默认值；`project-detail.tsx` 与模板管理弹窗不变，新建项目对话框只多传 `onChange` 同步 live 值。
- **真实数据目录**：只读红线不变；真机只用沙盒 identifier。

## 十一、明确不做

- 从现有项目「另存为模板」；模板复制/排序/默认模板/导入导出。
- 台账行、目录映射、待办、里程碑、凭据进模板。
- 把模板套用到已有项目（只在新建时）。
- 模板进全局检索、FTS、备份 `Counts`。
- 模板软删/回收站；模板版本历史。
- 新增设置页或 nav 格；不做模板权限/多用户共享。
- 模板名的拼音排序（按 `created_at, id`）。

## 十二、回写清单（交付时）

- `docs/技术方案.md`：§四 数据模型加 `project_templates` 一行；§八 M12 行。
- `docs/开发进度.md`：里程碑表 M12 行 + 「## M12 验收证据」节 + 变更日志。
- `docs/HANDOFF.md`：快照头、§0 M12 bullet、§3 下一格、§4-A 期望数（命令 69 / TARGET 8 / cargo 起增 / node 起增）、§6 指针。
- 项目记忆：M12 口径与交付终态。
- 证据目录 `.superpowers/sdd/2026-10-22-m12-project-templates/`（git-ignored，收口前别清）。
