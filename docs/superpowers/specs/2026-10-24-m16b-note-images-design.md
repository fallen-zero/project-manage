# M16-b 图文笔记与截图 设计

**日期**：2026-10-24
**分支**：`feat/m16b-note-images`
**状态**：设计已由需求方拍定，待 spec review
**需求出处**：`docs/思维导图.md` §一.3「自定义备注台账：图文笔记、支持粘贴项目截图」；备份口径接 M8 §五。

## 一、范围

一句话：**备注能粘截图，截图进备份**。两件必须同一轮交付 —— 只做截图不做备份，恢复后会留下指向不存在文件的残行；只做备份不改存储，也没有可打包的对象。

本轮做：

1. **备注截图**：备注编辑对话框里粘贴 PNG / JPEG / WebP 截图；备注展开行显示缩略图并可单张删除。
2. **附件进备份**：截图以 BLOB 存在库内，随整库快照进 `backup.db`，包的条目形状与 Counts 都不变。

不做（详见 §十）：外部附件目录、Markdown 内联渲染、图片检索/OCR、服务端压缩或缩略图、软删备注的图片清理、图片说明文字/排序。

## 二、需求方拍定的口径

| # | 决定 |
|---|---|
| D1 | b1（截图）+ b2（附件进备份）合成一轮；不做「先截图后备份」的中间态。 |
| D2 | V13 新表 `note_assets`；图片字节存 SQLite **BLOB**（列 `data`），**不建外部附件目录**、不存路径。 |
| D3 | `TARGET_VERSION` 12→13；备份包 schema 13；`Counts` 仍七键；告警句补「备注图片（二进制）」。 |
| D4 | 三条**同步**命令 `note_asset_list/save/delete`；命令 79→82；`tauri::command(async)` 仍 7。 |
| D5 | 三条命令都按活动区守卫；跨区与不存在同走 `not_found`。 |
| D6 | 只收 PNG / JPEG / WebP，**按 magic bytes 判**；单张 ≤ 5 MiB（5,242,880 字节），每条备注 ≤ 10 张；超出回 `invalid_input`。 |
| D7 | IPC 走**base64 data URL**（`data:image/png;base64,…`）；列表回 `dataUrl`，不用 asset protocol、不新增权限。 |
| D8 | 图片**硬删**（无 `deleted_at`）；备注软删时图片行保留但读不到；备注被硬删时 FK `ON DELETE CASCADE` 清图。 |
| D9 | 图片**不进**全局检索、**不写**项目操作日志、**不进** `Counts`。 |
| D10 | 前端：备注展开行显示缩略图 + 单张删除；**只有已保存的备注**能粘贴，新备注先保存再编辑。 |
| D11 | `base64 0.22` 从传递依赖提为直接依赖（`Cargo.lock` 已有 0.22.1）；`[[package]]` 仍 **616**；不新增 npm 依赖。 |
| D12 | 不做图片压缩/服务端缩略图/OCR/搜索；缩略图就是同一张 `dataUrl` 缩到 64px 显示。 |

## 三、数据模型与 V13 迁移

```sql
CREATE TABLE note_assets (
    id         TEXT PRIMARY KEY,
    note_id    TEXT NOT NULL REFERENCES ledger_notes(id) ON DELETE CASCADE,
    mime       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    data       BLOB NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX ix_note_asset_note ON note_assets(note_id, created_at);
```

- 六个列，顺序固定；**没有** `deleted_at` / `updated_at`：图片不编辑，只增删。
- `size` 是**解码后**字节数，不是 base64 串长度。
- `mime` 只可能是 `image/png` / `image/jpeg` / `image/webp`，由服务端嗅探写定，不信客户端声明。
- **零回填**：V13 不写任何业务行；存量备注的图片从空开始。
- V13 常量加入 `db.rs` 的线性迁移 match；`TARGET_VERSION` 改 **13**。
- 旧包（schema ≤12）导入时 `db::migrate` 自动补 V13，新表为空；不为旧包造新夹具。
- `PRAGMA foreign_keys=ON` 在 `db.rs` 已开，所以 `ON DELETE CASCADE` 真的生效；**软删不触发级联**。
- 既有硬编码 12 的断言全部同步改 13：`db.rs` 的 `v12_schema_meta_reaches_target_version_twelve`（测试名一并改 v13）、`backup.rs` 四处 `schema_version == 12`。逐处核对，不许只改一处。

## 四、命令契约（3 条同步命令）

### 线格式

```ts
type NoteAssetMime = "image/png" | "image/jpeg" | "image/webp";

interface NoteAsset {
  id: string;
  noteId: string;
  mime: NoteAssetMime;
  size: number;       // 解码后字节数
  createdAt: string;
  dataUrl: string;    // data:image/...;base64,...
}
```

Rust `NoteAsset` 用 `#[serde(rename_all = "camelCase")]`（`data_url` → `dataUrl`）。

### 命令

| 命令 | 参数 | 行为 |
|---|---|---|
| `note_asset_list` | `note_id: String` | `ensure_active_row("ledger_notes", note_id)` → 按 `created_at, rowid` 列行 → 逐行编码成 `dataUrl` |
| `note_asset_save` | `note_id: String, data_url: String` | 守卫 → 张数上限 → 解析/校验 → INSERT → 回 `()` |
| `note_asset_delete` | `id: String` | 先读该行的 `note_id`（读不到 `not_found`）→ 用 `note_id` 走 `ensure_active_row` → DELETE；`changed==0` 回 `not_found` |

- `save` **不回整行**：整行意味着把刚收到的 5 MiB base64 原路再发一遍；前端成功后只递增一个刷新计数。

- **不要**对 `note_assets` 调 `workspace::ensure_active_row`：那个通用守卫的 SQL 引用 `t.deleted_at`，本表没有该列，会直接 SQL 报错。守卫一律挂在宿主 `ledger_notes` 上。
- 错误码只用既有的 `invalid_input` / `not_found` / `db_failed`，不新增码名。
- 新建模块 `src-tauri/src/note_asset.rs`（形制照 `version.rs`）：`NoteAsset`、`list`、`save`、`delete`，测试同模块；`lib.rs` 三条命令壳只做 `db(&state)?` + 转调。

## 五、校验（data URL / magic bytes / 上限）

`save` 的校验顺序即判据（先便宜后昂贵，且每条都有确定的错误）：

1. **张数**：`SELECT COUNT(*) FROM note_assets WHERE note_id=?1`；`>= 10` 回 `invalid_input("单条备注最多 10 张截图")`。
2. **串长**：`data_url.len() > MAX_DATA_URL_LEN`（7 MiB）回 `invalid_input`，**先于解码**（别为一条恶意串先分配几十 MB）。
3. **头**：`split_once(',')`；头必须**逐字等于** `data:image/png;base64` / `data:image/jpeg;base64` / `data:image/webp;base64`，否则 `invalid_input("只支持 PNG / JPEG / WebP 截图")`。不认 `image/jpg`、不认 GIF/SVG/BMP。
4. **解码**：`base64::engine::general_purpose::STANDARD.decode(payload)`；失败回 `invalid_input("截图不是合法的 base64 数据")`。
5. **非空**：`bytes.is_empty()` ⇒ `invalid_input("截图内容为空")`。
6. **嗅探**：按 magic bytes 得真实 MIME —— PNG `89 50 4E 47 0D 0A 1A 0A`、JPEG `FF D8 FF`、WebP `RIFF????WEBP`；都不是则回第 3 步同一句文案。
7. **一致**：嗅探结果与头声明不一致 ⇒ `invalid_input("图片内容与声明的格式不符")`；入库写**嗅探值**。
8. **大小**：`bytes.len() > 5 * 1024 * 1024` ⇒ `invalid_input("单张截图不能超过 5 MB")`。

- 常量：`MAX_IMAGE_BYTES = 5 * 1024 * 1024`、`MAX_ASSETS_PER_NOTE = 10`、`MAX_DATA_URL_LEN = 7 * 1024 * 1024`（5 MiB 的 base64 上界 6,990,508 字节，加头仍小于 7 MiB）。
- 拒绝 SVG 是安全决定：SVG 可携带脚本，且本轮只做 `<img>` 展示，没有清洗器。
- 存**解码后的字节**而不是原 data URL：省 1/3 体积，也顺手抹平 padding/空白差异；列表时再编码。

## 六、工作区与既有不变量

- 三条命令的宿主备注都必须在活动区、且未软删（M15 §4.2）。
- 跨区备注：`list`/`save` 直接 `not_found`；`delete` 读得到行、但宿主守卫失败 ⇒ `not_found` 且**一行不删**。
- 项目换区时整棵子树一起走，图片归属随备注 → 项目继承，不复制区号到 `note_assets`。
- 备注软删后图片保留但三条命令全 `not_found`；恢复备注后原样可见（与版本台账同口径）。
- **不写审计日志**：图片增删不调 `project_audit`。
- **不进检索**：`search.rs` 九段不动，`hits.append(&mut collect(` 仍 **9**（BLOB 与 base64 都不该进 FTS）。
- 结构门终态：命令 **82**、async **7**、`TARGET_VERSION` **13**、`dead_code` 豁免 **1**、`Cargo.lock` `[[package]]` **616**、collect **9**。

## 七、备份与明文告警

- `note_assets` 随整库快照进 `backup.db`；`strip_index` 只剥两张索引表，不碰本表。
- 包条目**仍只有** `manifest.json` + `backup.db`；`manifest.files` 仍一条；**不改 zip 格式**。
- `Counts` 仍七键；图片不进 Counts（沿用 M12–M16-a 的裁定）。
- `warn_plaintext_columns()` 补「备注图片（二进制，截图内容未加密）」：截图里常有客户名、内网地址、工单号，而包不加密。
- 新包 schema 13；旧版本应用读到会走既有 `backup_schema_too_new` 拒绝路径，不需要新错误码。
- 新增一条备份测试：种一张图 → 导出 → 解包 → 图片字节与 `mime/size` 原样；`Counts` 仍 7；schema 13。

## 八、前端

### 类型与 API

- `src/types/ledger.ts`：加 `NoteAsset` 与 `NoteAssetMime`（§四线格式）。
- `src/lib/api.ts`：
  ```ts
  export const listNoteAssets = (noteId: string) =>
    invoke<NoteAsset[]>("note_asset_list", { noteId });
  export const saveNoteAsset = (noteId: string, dataUrl: string) =>
    invoke<void>("note_asset_save", { noteId, dataUrl });
  export const deleteNoteAsset = (id: string) =>
    invoke<void>("note_asset_delete", { id });
  ```

### 备注展开行（`ledger-panel.tsx` 的 `NoteRow`）

- 只在**展开时**拉一次 `listNoteAssets(note.id)`，收起不请求（避免每个备注一次 IPC）。
- `LedgerPanel` 持一个 `assetsTick` 计数，经 `NotesSection` 传到 `NoteRow`；编辑对话框粘贴成功就 `+1`，让仍展开的那一行自动重拉。`NoteRow` 自己的删除也走本地计数 —— 不为此加 zustand 字段。
- 加载失败在行内显示 `error.message`（带 `title={hint}`），不吞。
- 有图：`grid grid-cols-4 gap-2`，每张 `<img src={a.dataUrl} alt="备注图片" className="h-16 w-16 rounded border object-cover" />`，角标删除按钮 `title="删除截图"`；尺寸文案取 `Math.round(size / 1024) KB`。
- 无图：一行「还没有截图。编辑这条备注时可粘贴截图。」
- 删除成功后重拉该备注的图；失败显示错误、不动本地列表（以库为准）。

### 备注编辑对话框（`LedgerFormDialog`）

- 仅 `entity === "notes" && isEdit` 时挂 `onPaste`：
  - `clipboardData.items` 里没有 `kind==="file"` 的项 ⇒ 不拦截，文本粘贴照常。
  - 有图片项：`preventDefault()`；逐张 `FileReader.readAsDataURL` → `saveNoteAsset(id, url)`；客户端先挡非 PNG/JPEG/WebP（文案同后端），后端仍是真门。
  - 成功显示「已添加 N 张截图，可在备注展开处查看」；失败显示第一张的错误，已成功的那些保留（图片是即时入库的，与「保存/取消」按钮无关，必须写在界面上）。
- 新建备注（`isEdit === false`）：不挂粘贴；描述区加一句「截图请先保存备注，再在编辑时粘贴」。
- 上传中**忽略新的 paste**（不是禁用系统粘贴）并显示「图片保存中…」；不阻塞文字保存。
- 不新增 zustand 字段：图片不进 `useLedgerStore` 的 `Ledger` 快照，避免每次列表刷新都把几十 MB base64 塞进全局 state。
- 缩略图靠 `<img src="data:…">` 直接显示；`tauri.conf.json` 的 CSP 现为 `null`，C-2 用 `naturalWidth > 0` 证明真的解码成功。将来若收紧 CSP，必须加 `img-src data:`，否则图全裂。

## 九、验收网格

### 后端 cargo（新增 11 条：415 → 426）

| # | 用例名 | 断什么 |
|---|---|---|
| T-1 | `v13_note_assets_table_and_index_exist` | 六列顺序 `id/note_id/mime/size/data/created_at`、`ix_note_asset_note` 存在、无 `deleted_at` |
| T-2 | `v13_backfills_nothing` | 先跑到 V12 并种项目+备注，再 `migrate`：备注原样、`note_assets` 0 行、版本到 13 |
| T-3 | `save_and_list_assets_roundtrip` | 1×1 PNG 往返：`mime/size` 正确、`dataUrl` 解码后与原图字节一致（夹具用规范 base64，字符串可逐字相等）、同秒两张按插入序、JSON 六键含 `dataUrl` |
| T-4 | `save_rejects_non_image_payloads` | 文本头 / GIF 头 / SVG 头 / 声明 PNG 实为 JPEG / 非法 base64 全 `invalid_input`，库里 0 行 |
| T-5 | `save_refuses_oversized_and_empty_images` | 空 payload、5 MiB+1 字节、超 `MAX_DATA_URL_LEN` 的串全 `invalid_input` |
| T-6 | `save_refuses_an_eleventh_image` | 10 张成功，第 11 张 `invalid_input`，库内仍 10 行 |
| T-7 | `note_asset_list_and_save_are_scoped` | 乙区备注：`list`/`save` 全 `not_found`、乙区行数不变；切回默认区可读可写 |
| T-8 | `note_asset_delete_is_scoped` | 乙区图片从默认区删 ⇒ `not_found` 且行仍在；切到乙区删成功 |
| T-9 | `delete_removes_the_row_and_missing_is_not_found` | 删除后列表少一张；再删同 id 回 `not_found` |
| T-10 | `hard_deleted_note_cascades_its_assets` | 裸 SQL 硬删备注 ⇒ 图片行 0；另一条软删的备注 ⇒ 图片行仍在，但 `list` 回 `not_found` |
| T-11 | `backup_carries_note_assets_without_counting_them` | schema 13、Counts 7、图片字节与 `mime/size` 原样、告警句含「备注图片（二进制」 |

既有测试同步改：`plaintext_warning_names_every_plain_text_table_since_m9` 加「备注图片（二进制」断言；`db.rs` 的 v12 版本断言与 `backup.rs` 四处 schema 12 断言改 13。

### 前端门

- `npm run build` exit 0 且 `error TS` 0；node 仍 **75**（本批不加 node 测试 —— 粘贴走 `FileReader`/`DataTransfer`，node 侧没有 Web API）。
- 三条 API 包装名与命令名逐字一致。
- UI 无 jsdom 自动化；交互证据走真机。

### 真机格（CDP，一次性 identifier 沙盒，照 M16-a 的隔离口径）

| 格 | 动作 | 读数 |
|---|---|---|
| C-1 | 打开有备注的项目，展开备注 | `emptyHint=true`、`imgCount=0` |
| C-2 | 编辑已知备注，**合成 paste 事件**塞一张 canvas PNG | `imgCount=1`、`srcPrefix="data:image/png;base64,"`、`naturalWidth=1`、`dialogNotice` 含「已添加 1 张」 |
| C-3 | 新建备注对话框里合成 paste | 界面有「先保存备注，再在编辑时粘贴」提示、后端该备注图片数仍 0 |
| C-4 | 删除缩略图 | `imgCount=0`、再展开仍 0 |
| C-5 | 同一备注连塞 11 张 | 前 10 张在，第 11 张界面出现「最多 10 张」错误，库内 10 行 |
| C-6 | 切到乙区的项目详情，再切回默认区项目 | 乙区项目备注 0 条；切回后原备注与缩略图仍在（图片没跟区串） |
| C-7 | invoke 直调导出 → 删图 → 导入 | 导入后 `imgCount=1`、`srcPrefix="data:image/png;base64,"`、`schemaVersion=13`（沙盒内，绕开原生对话框） |
| C-8 | `note_asset_list` 直读 | 键集合恰为 `id/noteId/mime/size/createdAt/dataUrl`，回包不含 `data`/`blob` 键 |

- C-2 的命名缺口：CDP 合成的是 `ClipboardEvent`，证的是**应用内 paste 处理器**；「系统剪贴板 → WebView paste 事件」由 WebView 自己保证，本机不自动验，记录在案。
- C-7 的命名缺口：导出/导入绕开原生文件对话框（对话框接线由 M8 真机格负责）；本轮验的是数据往返。

### 变异靶（≥8 处，逐处下靶 → 跑指定测试 → 记退出码 → 按原字节还原）

| # | 靶 | 必须红的测试 |
|---|---|---|
| M① | `note_asset_list` 首行守卫摘掉 | `note_asset_list_and_save_are_scoped` |
| M② | `note_asset_save` 首行守卫摘掉 | `note_asset_list_and_save_are_scoped` |
| M③ | `note_asset_delete` 的 `ensure_active_row` 摘掉 | `note_asset_delete_is_scoped` |
| M④ | 张数上限判断摘掉 | `save_refuses_an_eleventh_image` |
| M⑤ | 5 MiB 判断摘掉 | `save_refuses_oversized_and_empty_images` |
| M⑥ | magic 嗅探改成「直接用声明头」 | `save_rejects_non_image_payloads` |
| M⑦ | V13 DDL 的 `ON DELETE CASCADE` 摘掉 | `hard_deleted_note_cascades_its_assets` |
| M⑧ | 迁移 match 的 `13 => V13` 摘掉 | `v13_note_assets_table_and_index_exist` |
| M⑨ | 告警句里的「备注图片」删掉 | `plaintext_warning_names_every_plain_text_table_since_m9` |

每处还原后 `git diff --exit-code` 验 0，并用 `sha256sum` 前后对照。变异命令一律 `cd src-tauri && cargo test -j 1 --lib <单个筛选词>`。

### 全量门禁

- cargo `426 passed`、两条 clippy 0、`cargo build --lib` 0。
- node `tests 75 / pass 75 / fail 0`；`npm run build` 的 `error TS` 0。
- 结构门：命令 **82**、async **7**、TARGET **13**、豁免 **1**、Cargo.lock **616**、collect **9**、merge **0**。
- 依赖门改口径：`git diff --stat master..HEAD -- package.json` 必须空；`Cargo.toml` 与 `Cargo.lock` 必然非空（只允许 `base64 = "0.22"` 与根包依赖清单那一处），`grep -c '^\[\[package\]\]' Cargo.lock` 仍 616。

## 十、明确不做

1. 外部附件目录、附件单独 zip 条目、asset protocol / `convertFileSrc`。
2. Markdown 内联渲染（正文里的 `![...]()` 不解析成图）。
3. 图片 OCR、图片进全局检索或 FTS。
4. 服务端压缩、服务端缩略图、EXIF 处理、尺寸裁切。
5. 图片说明文字、排序、拖拽、复制到剪贴板。
6. 软删备注时清理图片（保留到硬删/恢复）。
7. 新备注未保存时暂存截图（先保存再编辑）。
8. 图片总量上限（单张 5 MiB × 10 张是唯一口径）。
9. 备份包加密或单独加密 BLOB。

## 十一、回写清单

- `docs/技术方案.md`：`note_assets` 行改成「BLOB 存库、无附件目录」；M8 那条「附件一并放进 zip」的附注改成「M16-b 起图片以 BLOB 随 `backup.db` 进包」；数据模型加 V13。
- `docs/开发进度.md`：M16-b 验收证据 + 变更日志（命令 82 / TARGET 13 / schema 13）。
- `docs/HANDOFF.md`：§3 快照、§4-A 门禁串、下一格。
- `src-tauri/src/db.rs` 迁移注释、`backup.rs` schema 断言与告警句。
- `.superpowers/sdd/<plan>/progress.md` 记逐格 Ruling 与变异靶；收口前别清证据目录。

## 十二、风险与边界

- **列表回包可能很大**：10 张 × 5 MiB ≈ 50 MiB 原始字节、base64 约 67 MiB。本轮接受（本地 IPC），代价写在风险里；真要收窄就下调上限，不另做分页/缩略图服务。
- BLOB 让库文件膨胀；`VACUUM` 只在导出快照上跑，活库不做自动整理（存量红线：不改用户库的常态行为）。
- 图片是明文二进制，备份包不加密；告警句必须点名。
- 软删备注的图片仍占库；恢复备注即可见 —— 这是刻意的可撤销语义，不是遗漏。
- 前端没有自动化网：粘贴、缩略图、删除只有真机证据；C-2/C-7 的命名缺口如上。
- magic bytes 只判容器，不校验图片完整性/解码成功；畸形但头正确的字节会被存下，展示时可能裂图（不影响数据，删除即可）。
- `base64 0.22.1` 已在锁文件里，提为直接依赖不新增编译单元；若 `cargo update` 漂到别的 0.22.x，`[[package]]` 门读数不变，但要在账本记下解析到的确切版本。
