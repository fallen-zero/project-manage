# M11-b 台账导出（Excel / Markdown，按项目）设计

- **状态**：待需求方评审（2026-10-21 起草；D1–D4 与命令数已由需求方当次逐题拍定）
- **上游需求**：`docs/思维导图.md:70`「项目台账、待办、日报数据一键备份/恢复；**支持导出 Excel/Markdown 台账文件**」（§四.2 数据运维）
- **上一格**：M11-a（到期事项）已合入 `master`（合并点 `b99f154`），本轮不动它的任何口径
- **本轮定位**：M11 的后半格。M10 把「台账导出整格推 M11」的理由是**台账五表里躺着账号密码密文列**，要先单独定红线 —— D3 就是那条红线的落点。

## 一、要解决什么（以及为什么不是「顺手加个导出按钮」）

台账五表（`ledger_envs` / `ledger_credentials` / `ledger_servers` / `ledger_links` / `ledger_notes`）里有两张带密文列：

| 表 | 密文列（cipher + nonce） | 语义 |
|---|---|---|
| `ledger_credentials` | `username_cipher` / `password_cipher` | 账号 / 密码 |
| `ledger_servers` | `account_cipher` / `password_cipher` | 账号 / 密码 |
| 其余三表 | 无 | 全明文 |

它们是**主密码解锁后才可解密**的列。导成文件 = 把库里那层保护搬到用户桌面上一份 —— 所以本轮的承重决策不是格式，是**默认导什么、什么条件下才导明文**。

## 二、决策（D1–D12，逐条）

- **D1 按项目导出**（需求方 2026-10-21 拍定）：入口在项目详情页，一次导这一个项目的五表。**全局多项目导出本轮不做**（`思维导图.md:70` 那半句「一键备份/恢复」由 M8 的整库备份包覆盖，不在这里重复）。
- **D2 双格式**（同日拍定）：`.xlsx` 单文件五个工作表、`.md` 单文件五个小节；**共用同一份导出行模型**，格式层只做渲染。需求原文点名两种格式，缺一种就是缩量。
- **D3 密文红线**（同日拍定，A 案）：默认**只导明文列**，四族密文列**列头保留、值留空**；要含明文必须**当次显式勾选 + 二次确认**（in-app 对话框，不用 `window.confirm` —— WebView2 下点不通）。默认路径上**后端根本不查密文列**（不是查出来再抹掉），这是可证伪的形态：把默认路径的 SQL 改成读密文列并解密，D3 的测试必须红。
- **D4 数据范围**（同日拍定）：**不导软删行**（与界面一致）；**归档项目照常可导**（与 M11-a D4「归档项目的到期事项照常亮」同口径 —— 归档是管理退役，不是数据销毁）。
- **D5 新增 1 条命令**（同日拍定）：`ledger_export` ⇒ `generate_handler!` **64→65**（本仓第 6 次批准破例）；`tauri::command(async)` 仍 **7**（函数体同步：只取 `Mutex<Connection>` 的 guard、不跨 await）。
- **D6 导出逻辑全在 Rust**：新模块 `src-tauri/src/ledger_export.rs` —— `collect`（取数 + 可选解密）+ `write_xlsx` + `write_markdown`。前端只做「勾选 → 确认 → 保存对话框 → invoke」。理由：本仓页面层没有测试网（spec §十一 不引 jsdom），把渲染放前端等于把整格交给真机。
- **D7 落点守卫复用 `backup::guard_destination`**：在**任何写盘之前**调用（M8 坑 31：`File::create` 是 CREATE_ALWAYS，把落点指进数据目录会毁掉用户唯一的持久库）。`guard_destination` 已经是吃 `data_dir + dst` 的纯函数，直接复用，**不新写判据**。
- **D8 文件名**：默认 `台账-<项目名>-<YYYY-MM-DD>.<xlsx|md>`；项目名里的 `\ / : * ? " < > |` 与控制字符替换成 `_`，长度截到 60 个 **Unicode 码点**（避免 Windows 长路径；按 UTF-16 码元截会把 emoji 切出孤立高位代理，该串进 `JSON.stringify` → `serde_json` 直接拒绝，保存对话框的 invoke 失败）；扩展名由 `format` 决定，用户手改也无所谓（写盘按 `format` 走）。**实现落点在前端纯函数** `src/lib/ledger-export-name.ts`（只有保存对话框之前需要这个名字；后端只收最终 `path`，不在 Rust 里再抄一份清洗规则）—— 由 `node --test` 守。
- **D9 Markdown 形态**：`# 台账 — <项目名>`、`- 导出时间：YYYY-MM-DD HH:MM:SS`、`- 含明文：是/否`（**这一行是 D3 的文件内回显**：拿到文件的人要能一眼看出里面有没有明文，不必回头问导出的人）、五个 `##` 小节（环境 / 凭据 / 服务器 / 链接 / 备注），GFM 表格；**空表写「（无记录）」**；单元格里的 `|` 转义成 `\|`、换行（备注正文）转成 `<br>`。
- **D10 同步命令**：见 D5。
- **D11 不新增表、不新增迁移**：`TARGET_VERSION` 仍 **7**；导出是读操作。
- **D12 零新依赖**：`rust_xlsxwriter`（M10 起就是直接依赖）与 `calamine`（已在树里，用于测试读回）都在；`Cargo.toml` / `Cargo.lock` / `package.json` 的 diff 必须 **0 字节**。

## 三、导出行模型与列集合（承重：列头是契约）

```rust
pub struct Sheet { pub name: &'static str, pub headers: &'static [&'static str], pub rows: Vec<Vec<String>> }
pub struct ExportDoc { pub project_name: String, pub generated_at: String, pub sheets: Vec<Sheet>, pub included_secrets: bool }
pub fn collect(conn: &Connection, mk: Option<&MasterKey>, project_id: &str) -> AppResult<ExportDoc>
```

五表固定顺序与列头（**列头逐字**，测试按字面断言）：

| # | 工作表 / 小节 | 列头 | 说明 |
|---|---|---|---|
| 1 | 环境 | 环境 / 名称 / 地址 / 端口 / 备注 | 「环境」导库内原值 `dev/test/pre/prod`（**不复制** `ENV_LABELS` 到 Rust —— 一份词表只活在一处，界面层翻译不回流后端） |
| 2 | 凭据 | 标题 / 所属环境 / 账号 / 密码 / 地址 / 备注 | 「所属环境」按 `env_id` 关联出环境名（`env_id` 为空写空串）；账号/密码两列见 D3 |
| 3 | 服务器 | 名称 / IP / 堡垒机 / 账号 / 密码 / 备注 | 账号/密码两列见 D3 |
| 4 | 链接 | 类型 / 标题 / 地址 / 备注 | 类型导 `kind` 原值 |
| 5 | 备注 | 标题 / 标签 / 正文 | 正文保留换行（xlsx 单元格换行、md 用 `<br>`） |

排序：环境**复用 `ledger::list` 的同一段 `CASE env`（抽成共享常量 `ENV_ORDER_SQL`，两处共用）**；其余四表按 `created_at, id`（`id` 是**确定性兜底** —— `created_at` 只到秒，同秒插入的行在导出里必须可复现；`ledger::list` 的既有顺序不带这个兜底，导出层补上并在注释里说明，不算第二套排序判据）。

`NULL` 一律渲染成空串（xlsx 与 md 同形）；布尔类（`username_set` 等）**不进导出**（它们是界面用的「有没有值」，不是数据）。

## 四、IPC 契约

```rust
#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
struct LedgerExportInput // crate 内部用，无需 pub { pub project_id: String, pub format: String, pub include_secrets: bool, pub path: String }

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct LedgerExportSummary { pub path: String, pub sheets: usize, pub rows: usize, pub included_secrets: bool }
```

- 命令：`ledger_export(state, input: LedgerExportInput) -> AppResult<LedgerExportSummary>`。
- **前置顺序（不可换）**：`format` 合法性（只收 `xlsx` / `md`，否则 `invalid_input` 并点名两个合法值）→ `guard_destination(&state.data_dir, &path)` → `db(&state)` 取连接 →（若勾选明文）`unlocked_mk` → `project_exists`（软删项目 ⇒ `not_found`）→ 取数（含解密）→ 写盘。
  > **锁序是承重的**：`conn` 先于 `mk`（与 `ledger_reveal` 一致，且与 `backup_import` 失败臂的 conn→mk 同向）；反序（mk→conn）会与 `backup_import` 形成跨线程死锁环 —— Task 5 评审 I-1，2026-10-21 订正。
- **解密失败**（密文损坏、或来自另一个密钥库）：整个导出失败并回 `field_decrypt_failed`（沿用 `vault` 的错误码），**不写半份文件** —— 先渲到内存，成功后才 `File::create`。
- 返回 `sheets`（恒 5）与 `rows`（五表行数合计），供回执念数。
- **「不留半份文件」的边界要说准**：取数与解密**全部**成功后才开始写（解密失败 ⇒ 一个字节都没写）；而**写盘本身**失败（磁盘满、权限）可能留下半份文件 —— 这是既有 `write_fail` 的实态，本轮不引入「先写临时文件再改名」的新机制（那会动写盘白名单的形制）。格 13 只断言解密失败那一条。
- 前端封装：`exportLedger(input) => invoke<LedgerExportSummary>("ledger_export", { input })`。

## 五、失败面

| 场景 | 错误码 | 用户看到 |
|---|---|---|
| `format` 不是 xlsx/md | `invalid_input` | 点名「要 xlsx 或 md」 |
| 落点在数据目录内 | `backup_destination_unsafe` | 沿用 M8 现文本（含可操作 hint） |
| 项目不存在或已软删 | `not_found` | 沿用现文本 |
| 解密失败 | `field_decrypt_failed` | 「该字段无法解密」+ 既有 hint |
| 目录不存在 / 无写权限 | `fs_failed` | 沿用 `daily_report::write_fail` 那条（`fs_failed`，消息带 OS 原因 + hint「确认该路径存在且可读写」；`error.rs:21` 的 `AppError::io` 同码） |
| **某个单元格超过 Excel 的 32767 字符上限**（`rust_xlsxwriter` 会报 `MaxStringLengthExceeded`） | `invalid_input` | **写盘前**先查长度并点名「哪张表第几行太长」+ 给一条出路：「改用 Markdown 导出（没有这个上限）」；**不静默截断**（台账被悄悄截掉一段比失败更糟），也不把它落成 `fs_failed`（那个 hint 指错方向） |
| 取消保存对话框 | ——（前端不 invoke） | 什么都不发生 |

**不新开错误码**：`invalid_input` / `not_found` / `backup_destination_unsafe` / `field_decrypt_failed` / `fs_failed` 五支已够（五支都是树里现成的；`fs_failed` 出自 `error.rs:21` 与 `daily_report::write_fail`）。

## 六、结构门与不变量

- `generate_handler!` **64→65**；`tauri::command(async)` **7**；`TARGET_VERSION: i64 = 7`；全仓 `allow(dead_code)` **1**；`Cargo.lock` `[[package]]` **616**；`collect` **9**；`Cargo.toml`/`Cargo.lock`/`package.json` 对 `master` 的 diff **空**。
- **落点守卫只在命令层落一次**（`ledger_export` 里，写盘之前），`ledger_export.rs` 内部**不**再写一份判据；配一条 `include_str!` 文本门守调用序（照 `lib.rs::backup_export_wires_the_guard_before_any_write` 的形制）。
- **默认路径不读密文列**：`collect(conn, None, …)` 的 SQL 不 SELECT 任何 `*_cipher`/`*_nonce` 列；`mk: Some(_)` 时才走 `vault::decrypt_field`。变异 M② 是这条的靶。
- **不新增排序、不新增词表**：环境值与类型值导原值；排序复用 `ledger::list` 的 `CASE env`。
- 写盘白名单不变：本命令只写「用户在保存对话框里当场选的那一个路径」，不写任何其他位置；不删改既有文件。

## 七、前端交互（`project-detail.tsx` 台账面板旁）

1. 台账面板下方**常显一行**导出工具条（不做「先点按钮再展开」：台账面板本来就是一屏内的固定块，常显少一次点击、也少一个需要真机验的折叠态）：格式下拉（Excel / Markdown）、勾选框「包含账号与密码明文」、按钮「导出台账」。
2. 点「导出」：勾了明文 ⇒ 先弹 **in-app 二次确认**（shadcn `Dialog`，文案明确「导出文件不受主密码保护，任何拿到文件的人都能看到明文」）；不勾 ⇒ 直接进保存对话框。
3. 保存对话框复用 `@tauri-apps/plugin-dialog` 的 `save`；默认名按 D8（`台账-<项目名>-<今天>.<ext>`），扩展名过滤器随格式变。
4. 成功回执：`已导出 5 张表 / 共 N 行（含明文）`；失败显示 `toAppError` 的 message + hint。
5. **如实记**：这一层的按钮、对话框、回执都**没有自动化网**（spec §十一 不引 jsdom），取证靠真机格。

## 八、测试与取证档

**cargo（主体）**：
- `collect` 三向：默认（密文列值为空串且**没有**读密文列）、`include_secrets=true`（值与 `reveal` 一致）、软删行不出现。
- 归档项目的行照常出现（D4）。
- 空项目：五表都在、`rows` 合计 0、Markdown 每节写「（无记录）」。
- xlsx 往返：写盘后用 `calamine` 读回，断言五个工作表名、表头、行数与一个含换行的备注正文。
- Markdown：断言五个 `##` 小节、表头行、`|` 被转义成 `\|`、换行成 `<br>`。
- 守卫顺序文本门：`include_str!` 断言 `guard_destination` 出现在第一次 `File::create`/`write` 之前。
- 文件名清洗：**TS 纯函数** `defaultFileName(projectName, format, today)`（`src/lib/ledger-export-name.ts`）的 8 个样本（含 `:`、`/`、超长、空名），`node --test` 守。

**node**：新增 `tests/ledger-export-name.test.ts`（默认文件名与清洗，5 条：含控制字符样本与「59 汉字 + emoji」的代理对边界）；页面层其余部分仍只有 `tsc` + `vite build`。
**真机（沙盒 `dev.zero.pfm.m11btest`，跑完删净；真实数据目录三文件前后逐字一致）**：见 §九。

## 九、验收网格（真机 + cargo，逐格）

| # | 格 | 判据 | 档 |
|---|---|---|---|
| 1 | 默认导出 xlsx | 文件出现、能读回、五个工作表名与列头逐字对 | 真机 + cargo |
| 2 | 默认脱敏 | 账号/密码两列**有列头、值为空**；库里那条凭据确有密文（`username_set=true`） | 真机 + cargo |
| 3 | 显式含明文 | 勾选 + 二次确认后导出 ⇒ 列值非空且长度与 `reveal` 一致（**只回布尔与长度，不打印明文**） | 真机 + cargo |
| 4 | Markdown | 单文件、五个 `##`、GFM 表、`|` 转义、换行 `<br>` | 真机 + cargo |
| 5 | 空表 | 没有链接的项目 ⇒ 链接小节写「（无记录）」；xlsx 该表只有表头 | 真机 + cargo |
| 6 | 软删行 | 软删一条环境后再导 ⇒ 该行不出现（库里行还在） | 真机 + cargo |
| 7 | 归档项目 | 归档项目的台账照常可导 | 真机 + cargo |
| 8 | 落点守卫 | 把保存路径指进数据目录 ⇒ `backup_destination_unsafe`，**文件一个都不写** | 真机 + cargo 文本门 |
| 9 | 取消对话框 | 取消 ⇒ 不收到 invoke、无文件、无报错 | 真机 |
| 10 | 默认文件名 | `台账-<项目名>-<今天>.xlsx`；项目名含 `:` `/` 时被替换成 `_` | 真机 + node |
| 11 | 结构门 | 命令 **65**、async **7**、`TARGET_VERSION 7`、豁免 1、`Cargo.lock` 616、collect 9、依赖 diff 空 | cargo |
| 12 | 空项目/无台账 | 五表照导、回执念 0 行 | 真机 + cargo |
| 13 | 解密失败面 | 造一条密文被改动的行（测试内直写库）⇒ `field_decrypt_failed`，**没有半份文件** | cargo |

## 十、明确不做（本轮）

- 全局多项目导出、导出后直接打开文件/打开所在目录（`revealItemInDir` 是另一格，M8 曾讨论过，未列本轮）。
- 导入台账（本轮只导；M10 的 Excel 双向是日报专属）。
- 自定义列选择、导出模板、CSV。
- 待办/日报/到期事项混进台账导出（需求原文点名的是「台账文件」；待办与日报各有自己的导出通路）。
- 把 `ENV_LABELS`/`LINK_KIND_LABELS` 搬进 Rust 或反向暴露（一份词表一处活）。
- 加密导出文件本身（若用户要「加密的文件」，正确做法是整库备份包，不是这一格）。

## 十一、变异靶（至少六处，每处具名打红）

| 靶 | 怎么改 | 必须红在哪 |
|---|---|---|
| M① | `ledger_export` 里删 `guard_destination` 调用 | 守卫顺序文本门 + 格 8 的 cargo 断言 |
| M② | 默认路径改成也读密文列并解密（即 `mk` 恒 Some） | 「默认路径不读密文列」那条（密文列值必须为空串） |
| M③ | `collect` 的 envs SQL 摘掉 `deleted_at IS NULL` | 软删行不出现那条 |
| M④ | Markdown 转义函数对 `|` 直接跳过 | `\|` 转义断言 |
| M⑤ | 空表分支改成什么都不写 | 「（无记录）」断言 |
| M⑥ | `write_xlsx` 少写「备注」工作表 | 工作表名/往返断言 |
| M⑦ | 脏字符清洗去掉 `:`/`/`（`src/lib/ledger-export-name.ts`） | `node --test` 的文件名断言（含这两个字符的样本） |

## 十二、收口与回写

- 合入 `master` 后在 `master` 树上复跑整套门；`docs/技术方案.md` §四 补一句导出是只读、§九 M11 行改成「M11-b 已兑（按项目导出）」；`docs/开发进度.md` 新增 M11-b 验收证据与未验清单；`docs/HANDOFF.md` 下一格切到「M11 整批收口后的候选增强」。
- 本文件里 D1–D4、D5 的命令数破例、D8 的文件名、D9 的 Markdown 形态，都是**需求方当次拍定**的，回写时不许改口径。
