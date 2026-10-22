# M13 设计：父子项目关联（单父树）

**状态**：需求方 2026-10-22 逐题拍定（D1–D10），设计稿已确认，本文件是规格；实施计划另落 `docs/superpowers/plans/`。
**基线**：`master` = M12 收口点 `0ff5df6`。M12 收口读数：cargo **346 passed**、node **57/57/0**、命令 **69**、async **7**、`TARGET_VERSION: i64 = 8`、`allow(dead_code)` **1**、`Cargo.lock` **616**、collect **9**、依赖 diff **空**、merge commit **0**。本批 V9 后 `TARGET_VERSION` 变 **9**，其余结构门不变（命令仍 69）。

## 一、需求出处与本轮兑现范围

需求原文：`docs/思维导图.md` §四.3（可选迭代增强）「**父子项目关联**」。

本轮做**单父树**：一个项目最多有一个「所属总包」，总包可以继续有上层，层级不限；项目列表可按根总包分组查看，项目详情能看到直接父项目与直接子项目。父子关系只影响**展示与删除守卫**，不改变项目自身的台账、目录、检索、统计与导出。

**明确不做（见 §十一）**：多父/DAG、级联删除、拖拽改归属、按总包汇总工时/台账/搜索、模板保存归属、新增 IPC 命令或导航页。

## 二、需求方拍定的口径

- **D1 树形**：一棵树，单父。`projects.parent_id` 可空；无父即顶层总包；允许多层（孙项目、曾孙项目……），不做深度上限。
- **D2 设置入口**：在现有项目表单里加「所属总包」下拉；新建/编辑都用它。不另做「调整归属」命令或详情页操作。
- **D3 父项候选**：所有**未删除**项目都可当父项目，**含「归档」状态**；编辑时排除自己与自己的全部后代（不能形成环）。
- **D4 删除**：一个项目还有**未删除的直接子项目**时禁止删除，返回 `invalid_input` 并点名子项目数量；不级联、不自动提升孤儿。删掉/移走全部子项目后即可删除。
- **D5 列表视图**：项目列表加「全部 / 按总包」两种视图，默认「全部」保持现有平铺与 `updated_at DESC`；「按总包」按根分组，子/孙**递归缩进、任意层**，根组显示项目总数。
- **D6 详情视图**：详情页加「所属总包」链接与「子项目」直接子项列表；不加新导航页、不做整棵树的可视化。
- **D7 校验**：父项目必须存在且未删除；禁止自指、禁止挂到自己的后代下（沿父链上溯，遇到环即拒）；错误码只用 `invalid_input` / `not_found`。
- **D8 命令与结构**：不新增 IPC 命令，`project_create` / `project_update` / `project_delete` / `project_list` / `project_get` 的契约内加 `parentId`；`generate_handler!` 仍 **69**、`tauri::command(async)` 仍 **7**。
- **D9 备份与检索**：`parent_id` 是普通列，随整库快照进备份包；`backup::Counts` 仍 **7 键**；包内 `schemaVersion` 变 **9**，旧包（8）仍可导入并跑 V9 迁移；检索 `collect` 仍 **9**，父子关系**不进**全文检索。
- **D10 模板边界**：项目模板**不保存归属**；模板管理弹窗不显示「所属总包」；`applyTemplate` 保留表单当前的 `parentId`。

## 三、数据模型与 V9 迁移

```sql
ALTER TABLE projects ADD COLUMN parent_id TEXT;
CREATE INDEX idx_projects_parent_id ON projects(parent_id);
```

- `TARGET_VERSION` **8→9**；`migrate` 的 `match` 加 `9 => V9`；**V9 零回填**（迁移里只有 DDL，不写任何业务行）。
- 不建外键、不写 `ON DELETE`：项目本来就是软删，级联会绕过删除守卫；父子的完整性与环由应用层命令校验（§四）。
- 不建 `UNIQUE`：同一父项目下允许重名子项目，沿用「项目名不唯一」的现状。
- 索引只建 `parent_id` 一条：删除守卫数子项、列表/详情按父过滤都要用它；不建 `(parent_id, deleted_at)` 复合索引（几十到几百行规模，YAGNI）。
- `Project` / `ProjectInput` 加 `parentId: Option<String>`（Rust）与 `parentId: string | null`（TS）；线格式经 `#[serde(rename_all = "camelCase")]` 为 `parentId`。旧前端不传该键时 `Option` 反序列化为 `None`，命令不报错。
- `projects` 的列表顺序、软删语义、`updated_at` 维护均不变。

## 四、后端规则与命令契约

### 4.1 父项校验

`parent_id` 输入先归一化：`trim` 后为空 ⇒ `None`。

- **创建**：`validate(name/status)` → 校验父项 → `INSERT`（含 `parent_id`）。
- **更新**：`validate(name/status)` → 先确认目标项目存在且未删除（不存在返回 `not_found`，**早于**父项校验）→ 校验父项（排除自己与后代）→ `UPDATE`（含 `parent_id`）→ 重读返回。
- **父项必须存在且未删除**：`SELECT 1 FROM projects WHERE id = ?1 AND deleted_at IS NULL`；缺失/已删返回
  `invalid_input`「所属总包不存在或已删除」+ hint「刷新项目列表后重新选择」。
- **禁止自指**：`parentId == 当前项目 id` ⇒ `invalid_input`「项目不能把自己设为所属总包」。
- **禁止成环**：从 `parentId` 出发沿 `parent_id` 上溯；遇到当前项目 id ⇒ `invalid_input`「不能把项目挂到自己的子项目下（会形成循环）」。上溯时用 visited 集合防死循环；若发现既有数据本身成环，同样返回 `invalid_input` 并提示先解除循环。
- **祖先缺失/已删**：上溯链上遇到不存在的行或 `deleted_at` 非空即停止（视作该链到顶），不因历史脏数据拒绝本次无关保存。
- **归档父项**：`归档` 只是状态，不参与父项有效性判断，可以继续被选为父项目。
- **改父项的影响面**：更新只改当前项目这一行的 `parent_id`；它已有的子项目仍挂在它下面，整棵子树随它移动，不逐个改写后代的 `parent_id`。

### 4.2 删除守卫

`project_delete(conn, id)` 顺序：

1. 目标不存在或已删 ⇒ `not_found`（保持现有行为）。
2. `SELECT COUNT(*) FROM projects WHERE parent_id = ?1 AND deleted_at IS NULL`；`> 0` 时返回
   `invalid_input`「项目「{name}」下还有 {n} 个未删除的子项目，请先调整归属或删除子项目」+ hint「子项目删除后本项才能删除」。
3. 执行现有软删（`deleted_at` / `updated_at`）。

统计**只算未删除的直接子项目**：已软删的子项不再阻塞；孙项目由它自己的父项负责，不重复计数。

### 4.3 命令契约

| 命令 | 入参变化 | 返回 | 说明 |
|---|---|---|---|
| `project_list` | 无 | `Project[]` | 每行多 `parentId`；顺序仍 `updated_at DESC` |
| `project_get` | 无 | `Project` | 多 `parentId` |
| `project_create` | `ProjectInput` 多 `parentId` | `Project` | 父项校验见 §4.1 |
| `project_update` | `ProjectInput` 多 `parentId` | `Project` | 先 `not_found`，再父项校验 |
| `project_delete` | 无 | `void` | 有未删除子项时 `invalid_input` |

- 命令一律同步；`generate_handler!` 仍 **69**；`tauri::command(async)` 仍 **7**；无新模块入口，`allow(dead_code)` 仍 **1**。
- 错误码只用 `invalid_input` / `not_found`；错误文案点名「所属总包」「子项目」，可直接展示给用户。

## 五、前端交互

### 5.1 类型与纯函数

- `src/types/project.ts`：`Project` / `ProjectInput` 加 `parentId: string | null`；`EMPTY_INPUT.parentId = null`；`toInput(p)` 返回 `p.parentId ?? null`。
- 新增 `src/lib/project-tree.ts`：

```ts
export interface ProjectTreeNode {
  project: Project;
  depth: number;
  children: ProjectTreeNode[];
}
export interface ParentOption {
  id: string;
  name: string;
  status: string;
  depth: number;
}

export function buildProjectForest(projects: Project[]): ProjectTreeNode[];
export function flattenProjectTree(forest: ProjectTreeNode[]): ProjectTreeNode[];
export function parentCandidates(projects: Project[], excludeId: string | null): ParentOption[];
```

  - 输入顺序即输出同级顺序（后端已按 `updated_at DESC`）；不修改入参数组/对象。
  - 根判定：`parentId` 为空、父项目不在未删除集合内、父项目是自己、或上溯链成环且无法到顶。异常数据按根展示，**绝不递归死循环**，且每个项目只出现一次。
  - `parentCandidates`：先建森林，再按先序扁平化；`excludeId` 非空时剔除该项目及其全部后代；返回 `depth` 供下拉缩进。
- `applyTemplate` 返回 `parentId: input.parentId`（模板不存归属，套模板不改归属）；新增/更新其 node 测试。

### 5.2 `ProjectForm` 的「所属总包」

- 新增可选 prop `parentOptions?: ParentOption[]`。**不传即不渲染**该字段（模板管理弹窗走这条路）；传空数组时仍渲染「无（顶层项目）」并禁用整个下拉（没有其他项目可选）。
- 渲染位置：状态下拉之后。首项 `"none"` = 「无（顶层项目）」；其余按 `depth` 用 `— ` 前缀缩进，文案为「名称（状态）」。
- 表单值：`form.parentId`；若当前值不在候选项里（父项目已删/异常包），显示为「无（顶层项目）」，提交时归一化为 `null`，不把失效 id 再写回。
- 变更走独立的 `setParent(value: string | null)`，同步 `setForm` 与 `onChange` live 回传；`patch` 仍只服务字符串字段。
- 新建项目：候选项 = 所有未删除项目（`parentCandidates(items, null)`），可选归档父项。
- 列表页/详情页编辑：候选项 = `parentCandidates(items, project.id)`，天然排除自己与全部后代。
- 模板管理弹窗：`toForm(t)` 与新建模板初值都补 `parentId: null`；`ProjectForm` 不传 `parentOptions`，字段不出现；模板保存时多出的 `parentId` 键被 `TemplateInput` 反序列化忽略。

### 5.3 项目列表两种视图

- 标题行「模板」「新建项目」左侧加视图切换：`全部` / `按总包`，默认 `全部`。
- **全部**：保持现有平铺渲染与顺序，逐字不变。
- **按总包**：`buildProjectForest(items)` 后按根分组；每个根组是一个卡片：根项目行显示名称/状态/tags/操作，并带「共 N 项」徽标（N 含根）；组内按 `depth` 缩进渲染后代行；组与组之间沿用现有间距。
- 行内操作（打开主根、复制路径、编辑、删除）在树视图逐行可用；抽出的行组件同时服务两种视图，避免两套标记漂移。
- 无项目时空态不变；树视图下所有项目都有父或父缺失时，分别归入对应根组，不隐藏任何项目。

### 5.4 项目详情

- 「基础信息」下方新增「父子关系」区块：
  - **所属总包**：`project.parentId` 能在 `items` 里找到 ⇒ 链接到父详情并显示名称/状态；`parentId` 为空 ⇒「无（顶层项目）」；有 id 但找不到（父已删/异常包）⇒「所属总包已删除或不存在」，不给链接。
  - **子项目**：`items.filter(p => p.parentId === project.id)` 的直接子项列表，每项链接 + 状态徽标 + 编号；为空显示「暂无子项目」。
- 详情页数据仍来自现有 store `items`（深链直达时已有的 `refresh` 守卫负责拉取），不新增命令、不额外请求。
- 编辑弹窗复用 §5.2 的候选排除规则。

### 5.5 页面层边界

页面/弹窗仍没有 jsdom 自动化网；树分组、下拉候选、删除守卫提示、详情父子区全部靠真机格（§八）。`project-tree.ts` 是纯函数，由 `node --test` 守。

## 六、不变量（逐条要有落点）

1. **单父树**：每个项目最多一个 `parent_id`；同一项目在树视图只出现一次；无环。
2. **候选排除**：编辑时，候选项不含自己与全部后代；新建时可选所有未删除项目（含归档）。
3. **父项有效性**：父项必须未删除；归档可选；缺失/已删父项在保存时被 `invalid_input` 拒绝。
4. **删除守卫**：有未删除直接子项时删除失败且文案含数量；移走/删掉子项后可删；已软删子项不阻塞；不级联。
5. **顺序不变**：`project_list` 仍 `updated_at DESC`；森林同级顺序与输入一致。
6. **模板边界**：模板不存归属；模板弹窗无父项下拉；`applyTemplate` 保留 `parentId`；模板保存不因多出的键报错。
7. **异常数据防御**：缺失父项/自指/成环的数据在列表树中按根展示、不递归死循环；详情显示父项缺失文案；保存时失效 `parentId` 归一化为 `null`。
8. **备份口径**：`Counts` 仍 7 键；包 `schemaVersion=9`；旧包（8）可导入；`warn_plaintext_columns` 文本不变（`parent_id` 不是明文业务字段）。
9. **结构门**：命令 **69** / async **7** / `TARGET_VERSION: i64 = 9` / `allow(dead_code)` **1** / `Cargo.lock` **616** / collect **9**；`Cargo.toml`/`Cargo.lock`/`package.json` 对 `master` diff **空**；merge commit **0**。
10. **既有行为**：项目平铺列表、编辑表单其余字段、台账/目录/模板/导出通路逐字不变；不加导航格。

## 七、测试与取证档

**cargo（主体）**：
- V9：`schema_meta` 到 9、`projects.parent_id` 列存在、`idx_projects_parent_id` 索引存在、`TARGET_VERSION == 9`；**零回填**：先把库建到 V8、插一条存量项目、再跑 V9，断言存量行 `parent_id IS NULL`。
- `project.rs`：带父创建/更新/清空往返（`get` 与 `list` 都含 `parentId`）；自指拒绝；后代成环拒绝（A→B 后把 A 挂到 B）；父项缺失拒绝；父项软删拒绝；归档父项可选；未知 id 更新返回 `not_found`（即使 `parentId` 也非法）；有未删除子项时删除被拒且文案含数量；删除子项后父项可删；已软删子项不阻塞。
- `backup.rs`：包 `schemaVersion` 断言 8→9；`Counts` 七键不变；模板包内容测试同步 schema 9；补一支父子字段随包往返的断言（导出包内 `projects.parent_id` 保留）。
- 具体测试条数在实施计划里逐支数出（不先写死）；最终以实跑 `test result` 为准。

**node**：
- 新增 `tests/project-tree.test.ts`：多层森林与同级顺序、缺父按根、自指/双节点成环不挂死且各出现一次、`flattenProjectTree` 先序与 depth、`parentCandidates` 全量先序、编辑排除自己与全部后代但保留兄弟/父/祖先、不修改入参。
- `tests/apply-template.test.ts`：模板不改变 `parentId`（有值保留、空值保留）。
- 具体条数在计划里数出（node 57 起增）。

**真机（沙盒 `dev.zero.pfm.m13test` + CDP 9333；真实数据目录三文件前后 `size+mtime+sha256` 逐字一致；跑完删净）**：见 §八。

## 八、验收网格

| # | 格 | 判据 | 档 |
|---|---|---|---|
| 1 | 新建总包 | 不选父项目保存 ⇒ 列表「全部」出现，详情显示「无（顶层项目）」 | 真机 |
| 2 | 新建子项目 | 选总包保存 ⇒ 详情显示所属总包链接；总包详情出现该子项目 | 真机 + cargo |
| 3 | 孙项目/多层 | 以子项目为父再建一项 ⇒「按总包」视图三级缩进，根组「共 3 项」 | 真机 + node |
| 4 | 候选排除 | 编辑总包/子项目时，下拉不含自己及全部后代，包含父/兄弟/祖先 | 真机 + node |
| 5 | 归档父项 | 把总包置为「归档」后，新建子项目的下拉仍能选中它 | 真机 + cargo |
| 6 | 禁止成环 | 直接 IPC 把 A 挂到其后代 B ⇒ `invalid_input`；UI 构造不出该组合 | cargo |
| 7 | 删除守卫 | 删除有子项的总包 ⇒ `invalid_input` 文案含子项数量，项目仍在 | 真机 + cargo |
| 8 | 解除后可删 | 先删子项目，再删总包 ⇒ 成功；列表与树视图都不再出现 | 真机 + cargo |
| 9 | 列表两视图 | 「全部」顺序与改动前一致；「按总包」分组、缩进、根组计数正确 | 真机 + node |
| 10 | 详情父子区 | 子项目详情有父链接；父详情子项列表正确；无子项显示空态 | 真机 |
| 11 | 模板边界 | 模板弹窗没有「所属总包」；新建项目套模板后仍保留已选父项目 | 真机 + node |
| 12 | 备份与检索 | 导出包 `schemaVersion=9` 且 `projects.parent_id` 保留；`Counts` 7 键；collect 9 | 真机 + cargo |

## 九、变异靶（至少八处，每处具名打红）

| # | 变异 | 期望红在哪 |
|---|---|---|
| M① | create/update 的 SQL 去掉 `parent_id` 写入 | 带父往返测试 |
| M② | `list_projects` / `get_project` 不读 `parent_id` | 列表/详情往返测试 |
| M③ | 删父项存在性校验 | 缺失/已删父项测试 |
| M④ | 删自指/成环校验 | 自指/后代环测试 |
| M⑤ | 删删除守卫的子项计数 | 删除被拒测试 |
| M⑥ | 子项计数不过滤 `deleted_at` | 已软删子项不阻塞测试 |
| M⑦ | V9 不建 `parent_id` 或索引 | V9 列/索引测试 |
| M⑧ | `buildProjectForest` 把缺父项目丢弃/死循环 | node 缺父/成环测试 |
| M⑨ | `parentCandidates` 不排除后代 | node 候选排除测试 |
| M⑩ | `applyTemplate` 不保留 `parentId` | node 模板边界测试 |

## 十、备份、检索与既有不变量

- **备份**：`parent_id` 随整库快照进包；`Counts` 七键不变；`warn_plaintext_columns` 不改；`schemaVersion` 9；旧包（8）导入时尾部 `db::migrate` 自动补列，存量项目 `parent_id` 为 NULL。
- **检索**：父子关系不进 `search.rs`；`collect` 9 条不变；不做 FTS。
- **命令/结构门**：69 / 7 / 9 / 1 / 616 / 9；零新依赖；merge commit 0。
- **既有通路**：模板、台账、目录、日报、工时、到期预警、导出均不读 `parent_id`，行为不变；真实数据目录只读红线不变。
- **前端**：`ProjectForm` 的 `parentOptions` 可选；不传时模板弹窗与现有表单行为逐字不变。

## 十一、明确不做

- 多父/DAG、跨总包共享子项目。
- 级联删除、删除时自动把子项目提升为顶层、回收站/恢复。
- 拖拽改归属、树内拖拽排序、树节点折叠/展开状态持久化。
- 按总包汇总工时、台账、待办、里程碑、搜索结果或导出。
- 模板保存/套用归属；模板弹窗显示父项。
- 新增命令（如 `project_set_parent`）、新增导航页、项目详情整树可视化。
- 父项目/子项目的权限、共享或多用户语义。

## 十二、回写清单（交付时）

- `docs/技术方案.md`：§四 `projects` 行补 `parent_id` 与索引；§八 M13 行。
- `docs/开发进度.md`：里程碑表 M13 行 + 「## M13 验收证据」节 + 变更日志。
- `docs/HANDOFF.md`：快照头、§0 M13 bullet、§3 下一格、§4-A 期望数（命令 69 / TARGET 9 / cargo 起增 / node 起增）、§6 指针与新增坑。
- 项目记忆：M13 口径与交付终态。
- 证据目录 `.superpowers/sdd/2026-10-22-m13-project-hierarchy/`（git-ignored，收口前别清）。
