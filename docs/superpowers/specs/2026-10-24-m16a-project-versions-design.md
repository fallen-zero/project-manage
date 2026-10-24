# M16-a 项目版本台账 + 子文件选择器 设计

**日期**：2026-10-24
**分支**：待开（建议 `feat/m16a-project-versions`）
**状态**：设计已由需求方拍定，待 spec review
**需求出处**：`docs/思维导图.md` §一.4「项目版本台账：版本号、上线时间、本次上线内容」与 §一.2「项目子文件/子目录快捷映射」。M16-b（图文笔记/截图 + 附件进备份）另立 spec，本文件不覆盖。

## 一、范围

本轮只做两件：

1. **项目版本台账**：每个项目一张版本记录，含版本号、上线时间、上线内容；新增/编辑/删除；项目详情页展示。
2. **子文件选择器**：项目详情页「快捷入口」旁增加「选择文件」按钮；后端已接受任意绝对路径，不改 Rust。

不做：版本进全局检索、版本进操作日志、版本导出/导入、版本审批流、版本附件、版本按工作区分组、图文笔记/截图、附件备份。这些留给后续里程碑。

## 二、需求方拍定的口径

| # | 决定 |
|---|---|
| D1 | 版本号与上线时间**必填**；上线内容可空；支持新增、编辑、删除（全 CRUD）。 |
| D2 | 版本号**同一项目内唯一**；软删行不占号，删除后可用同版本号重建。应用层查重回 `invalid_input`；DB 用 partial unique index 兜底。 |
| D3 | V12 新表 `project_versions` + 两个索引；**零回填**；`TARGET_VERSION` 11→12。 |
| D4 | 四条**同步**命令 `version_list/create/update/delete`；命令 75→79；`tauri::command(async)` 仍 7。 |
| D5 | 全部读写按**活动工作区**过滤（M15 硬约束）；跨区与不存在同走 `not_found`。 |
| D6 | 版本**不进**全局检索（`collect` 仍 9）；**不写**项目操作日志（版本不是项目字段变更）。 |
| D7 | 删除是软删；列表按 `released_at DESC, id`。 |
| D8 | 备份包 schema 12；`Counts` 仍 7 键；明文告警补「版本号与上线内容」；版本行随整库快照进包。 |
| D9 | 前端用独立的 `version-panel.tsx`，本地 state（照 `milestone-panel`），不加 zustand store。 |
| D10 | 子文件入口只加前端「选择文件」按钮 + `pickFile()` 包装；`add_entry_dir` 不改。 |
| D11 | 零新依赖；不新增 `dead_code` 豁免；命令、TARGET、collect、Cargo.lock 由结构门逐格现跑。 |

## 三、数据模型与 V12 迁移

```sql
CREATE TABLE project_versions (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    version     TEXT NOT NULL,
    released_at TEXT NOT NULL,
    content     TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at  TEXT
);

CREATE INDEX ix_project_version_project
    ON project_versions(project_id, released_at);

CREATE UNIQUE INDEX ux_project_version_project_version
    ON project_versions(project_id, version)
    WHERE deleted_at IS NULL;
```

- 列顺序固定；`Project.workspace_id` 不复制到本表，归属通过 `project_id` 的 JOIN/守卫继承。
- `released_at` 存 `YYYY-MM-DD`，形状由写入侧 `todo::req_date("上线时间", …)` 判；SQL 不解释日期。
- `version` 写入前 `trim`，空串拒绝；`content` 用 `norm`，空/缺省落 `''`。
- **零回填**：V12 不写任何业务行；老项目版本台账从空开始。
- V12 常量加入 `db.rs` 的线性迁移 match；`TARGET_VERSION` 改 **12**。
- 旧备份包（schema ≤11）导入时 `db::migrate` 自动补 V12；新表为空，`Counts` 仍 7。不为旧包造新夹具，沿用既有导入测试。

## 四、命令契约（4 条同步命令）

### 线格式

```ts
interface Version {
  id: string;
  projectId: string;
  version: string;
  releasedAt: string;   // YYYY-MM-DD
  content: string;      // 可为空串
  createdAt: string;
  updatedAt: string;
}
```

Rust `Version` 用 `#[serde(rename_all = "camelCase")]`；`deleted_at` 不上线。

### 输入

```rust
pub struct VersionInput {
    pub project_id: String,
    pub version: String,
    pub released_at: Option<String>, // 缺键/null/空串都走 req_date 报「上线时间」
    pub content: Option<String>,
}

pub struct VersionUpdate {
    pub version: String,
    pub released_at: Option<String>,
    pub content: Option<String>,
}
```

`released_at` 声明为 `Option<String>` 与既有 `ExpiryInput.expire_date` 同形：声明成 `String` 会先炸在 Tauri 参数层，拿不到本仓统一的 `invalid_input` 文案。

### 命令

| 命令 | 参数 | 行为 |
|---|---|---|
| `version_list` | `project_id: String` | 先 `project_exists`（活动区 + 未软删）；再按项目列未软删行，`ORDER BY released_at DESC, id` |
| `version_create` | `input: VersionInput` | 守卫 → 校验 → 查重 → INSERT → 回整行 |
| `version_update` | `id: String, input: VersionUpdate` | `ensure_active_row` → 校验 → 查重（排除自身）→ UPDATE（不改 project_id）→ 回整行 |
| `version_delete` | `id: String` | `ensure_active_row` → 软删；`changed==0` 回 `not_found` |

错误码只用既有的 `invalid_input` / `not_found` / `db_failed`，不新增码名。

### 校验与查重

- 版本号：`trim` 后非空，否则 `invalid_input("版本号不能为空")`。
- 上线时间：`req_date("上线时间", input.released_at.clone())`。
- 上线内容：`norm(input.content.clone()).unwrap_or_default()`。
- 同项目查重 SQL：`SELECT COUNT(*) FROM project_versions WHERE project_id=?1 AND version=?2 AND deleted_at IS NULL AND id<>?3`；`id` 在 create 传空串、update 传自身 id；命中回 `invalid_input("同一项目下版本号不能重复：{version}")`。
- partial unique index 是兜底；命令路径先查重，正常不会落到 `db_failed`。

## 五、工作区与既有不变量

- `version::project_exists(conn, project_id)` 的 SQL 带 `deleted_at IS NULL AND workspace_id = active_id`。
- `version_list` 先过 `project_exists`，再按项目查；跨区项目直接 `not_found`。
- `version_update` / `version_delete` 首行 `ensure_active_row(conn, "project_versions", id)`；跨区行与不存在行同码。
- `version_create` 的宿主项目必须是活动区项目；不接受客户端传区号（与 create 类同向）。
- 项目软删后版本行保留；`version_list` 因 `project_exists` 拒绝，恢复项目后原样可见。
- **不写审计日志**：版本 CRUD 不调用 `project_audit`；`project_audit_logs` 的 schema/线格式零改动。
- **不进检索**：`search.rs` 九段不动，`hits.append(&mut collect(` 仍 9。
- 结构门终态：命令 **79**、async **7**、`TARGET_VERSION` **12**、豁免 **1**、`Cargo.lock` **616**、collect **9**；零新依赖。

## 六、备份与明文告警

- `project_versions` 随整库快照自动进包；`strip_index` 只剥两张索引表，不碰本表。
- `Counts` 仍七键（`projects`/`ledger_envs`/`ledger_credentials`/`ledger_servers`/`ledger_links`/`ledger_notes`/`settings`）；版本不进 Counts，沿用 M12–M15 的裁定。
- `backup.rs` 既有三处 schema 断言 11→12；新增/修改一条测试断「版本行随包、Counts 仍 7、schema 12」。
- `warn_plaintext_columns()` 补「版本号与上线内容」——它们是业务文本，可能带客户名/域名。
- 旧包导入后 V12 新表为空；不伪造旧包夹具。

## 七、前端

### 版本面板

- 新建 `src/types/version.ts`（线格式与 Rust camelCase 对齐）。
- `src/lib/api.ts` 加 `listVersions/createVersion/updateVersion/deleteVersion` 四条 invoke 包装。
- 新建 `src/components/version-panel.tsx`：
  - 顶部新增行：版本号 `Input`、上线时间 `Input type="date"`、上线内容 `Textarea`、保存按钮；
  - 列表：版本号、上线时间、内容（全文换行展示；不截断）、编辑、删除；
  - 编辑复用同一组字段（行内展开或顶部表单切换），删除成功后从本地列表移除；
  - 错误/提示沿用 `milestone-panel` 的 `notice` + `error` 形态；
  - 不新增 zustand store。
- `src/pages/project-detail.tsx` 在 `MilestonePanel` 之后加 `VersionPanel` 区块，与现有面板同容器样式。
- 新建/编辑必填提示：版本号或日期为空时前端按钮禁用或提示，后端仍是真门。

### 子文件选择器

- `src/lib/api.ts` 加：
  ```ts
  export async function pickFile(): Promise<string | null> {
    const picked = await openDialog({ multiple: false, directory: false });
    return typeof picked === "string" ? picked : null;
  }
  ```
- `src/pages/project-detail.tsx` 的「快捷入口」区在「选择目录」旁加「选择文件」按钮；文件路径同样走 `addEntry`（后端 `add_entry_dir` 已接受任意绝对路径，`validate_path` 只要求绝对路径）。
- 取消返回 `null` 时不报错（既有对话框取消语义）。
- 入口行的打开/定位/复制沿用现有 `openFolder`/`revealFolder`/`copyText`；文件路径同样可用（`openPath`/`revealItemInDir`）。

## 八、验收网格

### 后端 cargo

| # | 用例名 | 断什么 |
|---|---|---|
| T-1 | `v12_project_versions_table_and_indexes_exist` | 八列顺序 `id/project_id/version/released_at/content/created_at/updated_at/deleted_at`、`ix_project_version_project` 与 `ux_project_version_project_version` 存在、partial 条件含 `deleted_at IS NULL` |
| T-2 | `v12_backfills_nothing` | 迁移前既有项目/标签/目录不变；`project_versions` 0 行 |
| T-3 | `create_rejects_blank_version_and_missing_date` | 空版本号、缺/空上线时间都回 `invalid_input` |
| T-4 | `create_and_list_versions_in_release_order` | 两版本按 `released_at DESC`；内容可为空串 |
| T-5 | `duplicate_version_is_rejected_on_create` | 同项目重复版本号回 `invalid_input`，库里仍 1 行 |
| T-6 | `duplicate_version_is_rejected_on_update` | 改成另一行已用版本被拒；改回自身原版本通过 |
| T-7 | `update_and_delete_are_soft_and_allow_recreate` | 更新字段；删除后列表 0 行、原行仍在；同版本号可重建 |
| T-8 | `version_reads_and_mutations_are_scoped` | 项目挪区后 list/create/update/delete 全 `not_found`；切回原区恢复 |
| T-9 | `backup_carries_versions_without_counting_them` | schema 12、Counts 7、版本行随包、告警句含「版本号与上线内容」 |

### 前端门

- `npm run build` exit 0 且 `error TS` 0；node 仍 75（本批不加 node 测试）。
- 四条 API 包装名与命令名逐字一致。
- UI 无 jsdom 自动化；交互证据走真机。

### 真机格（M16-a 收口时补）

| 格 | 动作 | 读数 |
|---|---|---|
| C-1 | 项目详情新增版本 | 行出现，版本号/日期/内容正确 |
| C-2 | 编辑版本 | 行更新；删除后消失 |
| C-3 | 重复版本号 | 界面显示错误，库里不增行 |
| C-4 | 切工作区 | 版本列表只随活动区项目；跨区深链不可见 |
| C-5 | 「选择文件」 | 选文件后快捷入口出现；取消不报错；打开/定位可用 |

## 九、明确不做

1. 图文笔记、粘贴截图、`note_assets`、附件目录、附件进备份（M16-b）。
2. 版本进全局检索第 10 段。
3. 版本变更写操作日志。
4. 版本 Excel/Markdown 导出或导入。
5. 版本审批流、版本状态机、版本附件。
6. 版本按工作区聚合/看板。
7. 版本号跨项目唯一（只要求同项目内唯一）。
8. 版本内容长度上限（本轮不设）。

## 十、回写清单

- `docs/技术方案.md`：数据模型加 `project_versions`（V12）；里程碑表 M16-a 行；迁移链说明。
- `docs/开发进度.md`：M16-a 验收证据 + 变更日志。
- `docs/HANDOFF.md`：§3 快照、§4-A 门禁串（命令 79 / TARGET 12 / 其余不变）、下一格转 M16-b 或需求重取。
- `src-tauri/src/db.rs` 顶部迁移注释、`backup.rs` schema 断言与告警句。
- `.superpowers/sdd/<plan>/progress.md` 记录逐格 Ruling 与变异靶；收口前别清证据目录。

## 十一、风险与边界

- partial unique index + 软删：删除后可重建；应用查重排除 `deleted_at IS NOT NULL` 与自身 id。
- 日期形状只由写入侧判；SQL 不解释日期（沿用 M9/M10/M11 红线）。
- 版本内容是明文业务文本；备份包不加密，告警句必须点名。
- 跨区守卫与 M15 一致；漏一处会让版本行成为跨区泄露面。
- 子文件选择器只改前端；若用户仍手填相对路径，后端 `validate_path` 会回 `invalid_input`，不在此轮放宽。
- UI 无自动化网，真机格与 `npm run build` 是本轮前端唯一证据。
