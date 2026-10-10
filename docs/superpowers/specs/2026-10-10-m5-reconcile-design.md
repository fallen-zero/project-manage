# M5 磁盘追平 —— 差集回收 + 启动自动增量轮

- 状态：已与需求方逐题确认（2026-10-10，八问定案。其中 D7 是控制方按本轮新机制重算后**翻转了需求方初选**的一格，翻转理由写在表里。）
- 上游 spec：`docs/技术方案.md` §6.1「变更检测」那一行（M5 的定义）、§6.1「索引行的生命周期以登记的 root 行为作用域」那一行、§八 里程碑表 M5 行、§九 默认值里「`notify` 监听默认开，设置页可关」那一行；`docs/superpowers/specs/2026-10-07-m4-unified-search-design.md` §十（M3/M4 划过来的账）与 §十一 风险 5、6（I-4 尺寸门、I-5 并发闸门）；`docs/开发进度.md`「M4 留下的已知缺口」9 条与 M3 已知缺口 m-6 / 残留 11。
- 本文是 M5 的实现依据；与 `docs/技术方案.md` 分歧时以那份为准并回写它（要回写与要更正的逐条在第十一节）。
- **行号纪律**：本文所有 `文件:行号` 都是 2026-10-10 在 `master`（M4 收口后）的当下读数，**只作对照，取数只认命令**（本仓同形错误已记五次，`docs/HANDOFF.md` 坑 65/67/73/86/92）。落点一律用符号定位，例：`grep -n "fn run_pass" src-tauri/src/index_job.rs`。
- **执行约束**：实现走 `superpowers:subagent-driven-development`；每个派发词一律带 `ponytail`（最懒可行方案：删除优先、复用仓库既有写法、不为假想需求加抽象、一条断言能守住就不开两条）与 `docs/HANDOFF.md` §1 那六条红线的原文。

## 一、本轮交付与边界

交付面五件，而**对外契约的净变化是零** —— 不新增依赖（`src-tauri/Cargo.toml` 与 `Cargo.lock` 一行不动）、不新增 IPC 命令（`generate_handler!` 保持 41 条）、不新增线格式字段（`RunSummary` / `SearchBundle` / `DocPreview` 一个键都不加）、不新增迁移（`missing` 早就在 `index_docs` 的 CHECK 里，`TARGET_VERSION` 仍是 4）。

1. `index_status='missing'` 第一次真的有数据：某行登记的 path 在本轮完整扫描的集合里不存在 ⇒ 标 `missing`，并**按 rowid 点删它的 FTS 行**，元数据行留着（界面要能回答「这个文件曾经被索引、现在磁盘上没了」）。
2. 每个项目在本轮的队列循环之后做一次差集回收，两条硬闸门与一条熔断全收在一个纯函数里。
3. 应用启动时在 setup 里自动跑一轮增量（`rebuild = false`），复用现成的线程壳、现成的 `running` 闸门与现成的进度 UI。
4. 预览的两格 M4 债务：抽取前**当场 stat 一次**判尺寸门（I-4）+ 一份 `AtomicBool` 单飞闸门（I-5）。
5. `project::set_root_dir` 那两条 `DELETE` + `INSERT` 包进一个事务（M3 残留 11 那个窄竞态）。

前端只动 `src/pages/index-status.tsx` 三格：状态筛选下拉加 `missing`、项目行念出 `missing` 计数、失效行挂一句悬浮说明（第五节末尾有 2026-10-10 的改判记录）。

**边界**：不引入 `notify`（技术方案那句「notify 监听 + 启动 reconcile」本轮只做后半，前半整批推到 M5.5，理由见第九节第 1 行）；不写第二套扫描通路（复用 `scan_root` 与 `run_pass`）；不给自动轮做独立观感；不做自动轮的开关。

## 二、决策与理由

| # | 决定 | 被否掉的替代与为什么 |
|---|---|---|
| D1 | 追平机制 = **差集回收 + 启动自动增量轮**，不引入 `notify` | ① `notify` 常驻监听：`Cargo.toml` 与 `Cargo.lock` 今天对它零命中，等于同一轮里同时付「首次引入依赖 + 首次引入常驻线程（`AppState` 四格里没有线程句柄字段）+ Windows `ReadDirectoryChangesW` 在网络盘/SMB 上丢事件 + 去抖 + 『监听线程写索引』这条新不变量」；② 只做反方向核对、新文件不追：与需求方选的「启动自动追平」不符；③ 前端挂载触发：把「库该不该追平磁盘」绑到 UI 生命周期上，刷新与热重载会反复重跑，且没打开过 `/index` 就永远不追平。 |
| D2 | 差集比对的落点 = **`run_pass` 内、每个 target 的队列循环之后** | ① 轮末统一比对：要把各项目的集合跨项目攒在内存里，且 `root_missing` 的项目集合本来就是空的、混在一个循环里更容易漏闸门；② 独立第二遍扫描：`walkdir` 成本翻倍，而本轮要的集合正是刚扫出来的那份。附一条形状事实供实现者省事：`queue` 是 `outcome.files` / `outcome.over_size` 的 `.iter().cloned()` 造的，所以队列循环走完后 `outcome` 仍在作用域内，不必额外 clone。 |
| D3 | 失效行的处置 = **标 `missing` + 点删 FTS 行 + 保留元数据行** | ① 只改 status、双侧行都留着：FTS 里的死正文继续占体积、继续进 bm25 的文档频率统计，而 M3 终审 I3 当年正是用这个理由否掉「只压症状」；② 双侧删行（M3 计划预留的 `delete_doc`）：库最干净，但「失效」在 UI 上直接不可见，与技术方案给 `index_status` 定的初衷相反；③ 靠 `ON DELETE CASCADE`：FTS5 虚表没有外键、不参与级联，压根不是现成机制。 |
| D4 | `skip_reason` 在 `missing` 行**留 NULL**，只动 `index_status` 与 `updated_at` | 顺手写一个 `'file_gone'` 被否：`skip_reason` 的既有语义在代码里很窄 —— `write_doc` 只在 `DocOutcome::Skipped` 那一个 arm 写它，取值域是 `too_large` / `empty_text` / `type_unsupported`；给一个无出处的新值会让界面在同一行上多挂一枚红标。`size` / `mtime` / `error_msg` / `indexed_at` 一律不动：那是「这一行最近一次被抽时的读数」，而回填自愈靠的正是 `size`/`mtime` 与新扫到的值不匹配。 |
| D5 | 防误标 = **两条硬闸门 + 一条比例熔断**，`walk_errors` 刻意不做闸门 | ① 把 `walk_errors` 非空也当硬闸门：`index_scan.rs` 的注释自己写着「权限/重解析点失败在 Windows 上是日常」，拿它当闸门等于把本轮主功能对真实语料关掉；② 只要两条硬闸门、不要熔断：一次意外（盘只挂了一半、`walkdir` 早期崩）就能成片标 missing 并点删成片 FTS 行，而那些行只能靠重抽回来。三支全实现在这同一个函数里、各自有独立夹具 —— 这样「删掉其中一支」必然让对应那条测试红（第七节第 2、3 条）。原第三条「本轮零扫到」与 `root_missing` 是同一个条件，合并的经过与代价写在第四节第一条。 |
| D6 | 回填**不新开设机制**：文件回来后由既有增量判据自动重抽回 `ok` | 显式 `restore` / 复活通路被否：`current_rowid` 的谓词带 `AND index_status = 'ok'`，missing 行天然匹配不上 ⇒ 下一轮必然走抽取 ⇒ `write_doc` 的 `ON CONFLICT (project_id, path)` 更新的是同一行。再加一个「恢复」函数是重复机制，还会让人误以为需要用户动作。 |
| D7 | 路径**不折叠、不 canonicalize**，只做事务 | 需求方初选是「给改指判定加大小写/尾分隔符折叠 + 包事务」，控制方按 D1/D3 落地后的新机制重算后翻转：库里那行的 `path` 与本轮扫到的串**同源**（都出自 `WalkDir::new(登记的 root 串)`，`scan_root` 保留传入串原样），所以折叠不是差集的正确性前提；而一旦折叠让「同一目录换写法」不再触发 `clear_project`，`write_doc` 的冲突键 `UNIQUE (project_id, path)` 是 BINARY 裸文本比较 ⇒ 插一条新行、旧行被差集判成「磁盘上没了」而**永久谎标 missing**（Windows 上那两个串是同一个文件）。折叠的正确前置是把 `path` 列归一化存储，那要 V5 迁移 + 全量重抽。今天那个「换写法 ⇒ 整项目回收重抽」看着像缺陷，在差集回收上线之后它正好是防两串并存的机制。 |
| D8 | 零新线格式字段、零新 IPC 命令 | ① 给 `RunSummary` 加「本轮回收了几行」：加键要连带改 `src/types/index.ts`、store、页面与 IPC 四张网，而 `missing` 计数本来就能从 `status_counts` 的 `GROUP BY index_status` 免费拿到（`index_overview` 的 `pick("missing")` 已在等，前端 `ProjectState` 的字段也已存在）；② 给 reconcile 开一条 IPC：本轮没有需要用户单独触发的动作，「建立索引」（`rebuild = false`）就是它。代价照实记：熔断跳过时界面不可观测，见第四节末与风险 6。 |
| D9 | 单飞闸门**新建 `PreviewShared`**，不塞进 `IndexShared` | 塞进 `IndexShared` 少两行，但那四格（`cancel` / `running` / `summary` / `last_error`）语义全是索引作业的，混进一格预览状态会让下一个读者重新发现一遍归属；而 `AppState` 已经由 `manage` 装好，加一格字段 + 一行 `manage` 就够，命令签名吃同一个 `State<AppState>`。 |
| D10 | 预览失败**复用 `preview_unavailable`**，不扩码表 | 加 `preview_too_large` / `preview_busy` 更清楚，但 M4 §七 把「前端只认 `preview_*` 两个码」定成了契约，扩码要连前端分派一起改。本轮用 `hint` 分成因，代价与终审议题写在风险 5。 |

## 三、状态口径与契约

`index_status` 五值在 M5 之后的写入点：

| 值 | 写入点 | 本轮 |
|---|---|---|
| `pending` | 列默认值，全仓无生产写入点（M3 的 m3） | 不动，分批断点续建仍归 M5.5 |
| `ok` | `write_doc` 的 `DocOutcome::Ok` | 不动 |
| `skipped` | `too_large` / `empty_text` / `type_unsupported` 三个 `skip_reason` | 不动，取值域也不扩 |
| `failed` | `write_doc` 的 `Failed` arm | 不动 |
| `missing` | **唯一写入点 = 本轮新增的 `index_store::mark_missing`** | 新增 |

**`missing` 的定义写成一句**：该行登记的 path 在**本项目本轮完整扫描**的集合里不存在。它不等于 `skipped`（文件在、没抽），也不等于 `failed`（文件在、抽炸了）。

Rust 侧三个新接口，签名给到能照抄的粒度，实现者不要自己发明第二种形状。行级读写归 `index_store`、作用域判断归 `index_job`，这是 `clear_project`（store）与 `sweep_unrooted_projects`（job）已有的分工：

```rust
// src-tauri/src/index_store.rs
/// (doc_rowid, path)。谓词按 project_id 取，并排除已是 missing 的行 —— 幂等：
/// 不重复 UPDATE、也不重复计入熔断的分母。
pub fn doc_rows_for_project(conn: &Connection, project_id: &str) -> AppResult<Vec<(i64, String)>>;

/// UPDATE index_docs SET index_status = 'missing', updated_at = datetime('now') WHERE doc_rowid = ?1
/// 与 DELETE FROM index_docs_fts WHERE rowid = ?1，两句包在 unchecked_transaction 里。
/// SET 子句里不许出现 skip_reason / error_msg / size / mtime / indexed_at（D4）。
pub fn mark_missing(conn: &Connection, doc_rowid: i64) -> AppResult<()>;

// src-tauri/src/index_job.rs
/// 三条闸门全部在这个函数内部：`root_missing`（移动盘/网络盘没挂载）、`outcome.capped`（配额
/// break 出来的不完整集合）、熔断（一次要标掉过半）。返回值就是标掉的行数，0 表示一格没动 ——
/// 不另设 `enum Reclaim`：每条闸门各自有夹具，`assert_eq!(.., 0)` 已经能钉住它，
/// 多一个类型只是让「哪个分支挡下来的」变成断言之外的信息。
pub fn reclaim_missing(
    conn: &Connection,
    project_id: &str,
    outcome: &ScanOutcome,
    root_missing: bool,
) -> AppResult<usize>;
```

熔断的算术固定写成 `candidates * 2 > tracked.len() && candidates > MISSING_FUSE_MIN_ROWS`（整数比较，不用浮点、不用 `tracked.len() / 2` 那种有截断歧义的形态）。两个量的定义**只有一种读法**：`tracked` = `doc_rows_for_project` 返回的行集合（已排除 `missing`），`candidates` = 其中 path 不在 `seen` 里的那些。`MISSING_FUSE_MIN_ROWS: usize = 20` 的家在 `index_job.rs` 顶部、与 `PROGRESS_EVERY` 同处，**不许在两处各写一个字面量**（M4 §四 的家规）。

**契约净变化的闸门形态**：`SearchBundle` 的 `len() == 7` 与 `DocPreview` 的 `len() == 6` 那两条既有键集合断言在 M5 必须照旧绿 —— 谁在 M5 动了线格式，这两条当场红。这就是 D8「零新字段」的可执行版本，不需要另造门禁。

## 四、判据（本轮的规格核心）

- **两条硬闸门**，任一命中则该项目本轮**完全不比对**：① `root_missing` —— 这种情况下 `run_pass` 手里的 `outcome` 是 `Default::default()` 的空集合（注释原话「根目录不在（盘没挂载）时连 walk 都不发起」），不拦就等于把整个项目的行全标 missing，而移动盘没插是本工具的常态用法；② `outcome.capped` —— 集合是 `break` 出来的，只含配额内前 N 个。
  **原设计的第三条「本轮一个文件都没扫到而库里有行」并入 ①**（2026-10-10 写计划时的可证伪性自查）：root 不可达时 `seen` 必然为空，两支是同一个条件，拆成两条 `if` 之后「删掉其中一支」永远红不了。并进来的代价照实记：一个**真的被清空的目录**（root 在、里面零文件）也会被 ① 之外的熔断挡下——小项目过不了熔断的 20 行地板，于是那一格不回收。这是保守漏做不是误做，兜底是手动「全量重建」。
- **一条熔断**：`candidates * 2 > tracked.len() && candidates > MISSING_FUSE_MIN_ROWS` ⇒ 判为「疑似不可达而非真删」，本轮跳过。反过来它也定义了 ① 的必要性：库里只剩 3 行、盘没挂载，candidates=3 过不了 `> 20` 那条地板，只有 ① 挡得住。
- **`seen` 必须含 `over_size` 桶**：超限文件照样建行（`too_large`），漏了这一桶就等于把每个超限文件误标 missing。这是本轮最容易写错的一格，第七节第 5 条专门守它。
- **未支持类型两侧都不在**（`kind_of` 为 `None` 时既不建行也不占配额），不影响差集。
- **cancel 的两条早退路径天然不经过回收**（`run_pass` 循环开头那条与循环内那条 `return`），与「一轮被用户取消时不做额外写库」的既有取舍同形 —— 这是取舍不是漏写。
- **`walk_errors` 不做闸门**，代价点名写死：无权限子树里的文件会被误标 `missing`，且因为下一轮同样扫不到它而**不会自愈**；`to_scanned` 里 `entry.metadata().ok()?`（读元数据失败就静默跳过该 entry）也是同一类「扫到了却没进 `seen`」的口子。根治要 `ScanOutcome` 带出出错目录的路径（改形状），归 M5.5。
- **四条互不干涉，实现时不许顺手改**：改名 = 旧行 missing + 新串行建行（认改名只省重抽、不改变正确性；技术方案对改名的承诺本来也只是「路径失效标记」）；文件放回 ⇒ 同一 `doc_rowid` 自动重抽回 `ok`；软删项目的行不碰（`targets()` 已按 `deleted_at IS NULL` 排除，与轮末 sweep 同口径，那些行是留给项目恢复用的）；`max_depth(16)` 之外的文件从不建行 ⇒ 本来就不在差集两侧，M3 的 m1 与本轮无关。
- **回收失败不静默**：`reclaim_missing` 用 `?` 传播，一次 rusqlite 错让整轮落 `error` 终态并写 `last_error`，与 `write_doc` / `clear_project` 的传播形状一致。吞掉回收失败 = 界面谎称「已追平」。
- **`missing` 只有一个写入点**这件事用行为断言守，不用 grep 门：第七节第 1 条同时断「`index_docs` 那一行还在」与「它的 FTS 行没了」。本仓的教训是散文与 grep 门都会说谎，而一条能被打红的断言不会。

## 五、落点（八格，每格点名要借的既有形状）

| # | 落点 | 做什么 | 借谁的形状 |
|---|---|---|---|
| 1 | `index_store::doc_rows_for_project`（新，读） | 取该项目非 missing 行的 `(doc_rowid, path)` | `clear_project` 里那段 `SELECT doc_rowid FROM index_docs WHERE project_id = ?1` 就是同一形状；`ix_index_docs_project` 已存在，按 project 取不走全表 |
| 2 | `index_store::mark_missing`（新，写） | UPDATE 那一行的 status + `DELETE FROM index_docs_fts WHERE rowid` | `write_doc` 的 `unchecked_transaction` + 按 rowid 点删（注释已写明「按 `doc_id` 列删虚表是全表扫，按 rowid 是点删」） |
| 3 | `index_job::reclaim_missing`（新，纯函数） | 从 `outcome` 的两桶构本轮 path 集合、做集合比对、两条硬闸门 + 熔断、逐行调 `mark_missing`，回「标掉的行数」 | `run_pass` 本身就是可内存库全测的纯函数（收 `&Connection` + `&mut dyn FnMut(Progress)`），沿用同一签名风格 |
| 4 | `run_pass` 每个 target 的队列循环之后 | 把 `&outcome` 与 `root_missing` 交给第 3 格，写成 `let _ = reclaim_missing(...)?;` —— 与轮末那句 `let _ = sweep_unrooted_projects(conn)?;` **同一个形状**（不接住返回值就不会有未使用变量告警，也不必为它开线格式位） | 轮末 sweep 的注释已写明「被清项目数刻意丢弃：塞进 `RunSummary` 要连带改 types/store/页面与 IPC 契约」，同一取舍 |
| 5 | `lib.rs` 的 setup 之后 | `app.handle().clone()` → 走现成的 `index_job::start`，`rebuild = false` | 现有线程壳（用 `thread::Builder::name` 而非裸 spawn，`running.swap(true)` 在 spawn 之前就置起，所以自动轮撞上用户手动点时，手动那次收到既有的 `index_running` 拒绝） |
| 6 | `doc_preview` 抽取之前 | 锁内取行时顺带取 `ScanOptions`；**锁外先 `std::fs::metadata` 一次**拿当前尺寸判门，超限直接 `preview_unavailable` | `index_overview` 已是「锁内读库、出锁才 `is_dir()`」的两段写，沿用同一条出锁纪律。注意 `metadata` 不是 `File::open` ⇒ M4 那条「`doc_preview.rs` 里 `File::open` 应零命中」的门照旧成立，不要顺手改它 |
| 7 | 新增 `PreviewShared { busy: AtomicBool }` + RAII 守卫 | 单飞闸门：占用中第二条预览立刻 `preview_unavailable` + hint | `RunningGuard` 的 Drop 复位 —— 本仓已有一条测试专门钉「作业 panic 展开也复位」，照它的形态再来一条 |
| 8 | `project::set_root_dir` | 那两条 `DELETE` + `INSERT` 包进 `unchecked_transaction`，`clear_project` 留在事务外 | `write_doc` / `clear_project` 的事务形态。**签名不变**：`rusqlite::Connection::unchecked_transaction(&self)` 收 `&Connection`，vendored 源码 `transaction.rs` 的 `new_unchecked` 里 `drop_behavior: DropBehavior::Rollback`，`Drop for Transaction` 走 `finish_()` ⇒ `rollback_()` ⇒ `execute_batch("ROLLBACK")`。所以「事务里第二条失败 ⇒ 第一条被回滚」是有源码出处的，不是推断 |

前端三格都在 `src/pages/index-status.tsx`：状态筛选下拉（现只有 all/ok/skipped/failed）加 `missing`；项目行的计数（现只念 ok/skipped/failed 三档）念出 `missing`；文件行那一格 **2026-10-10 写计划时改判** —— Badge 的文案本来就是 `r.status`，`missing` 与 `skipped` 靠文字已分开，而把 `missing` 塞进 `destructive` 会跟 `failed` 混成「出错了」（失效是磁盘上的事实，不是错误），再造一个分支就是把 `outline` 换成另一个中性色；所以第三格落地为**挂一句悬浮 `title`**（为什么它不在结果里 + 放回后重跑一轮会自动回填），不动 `variant`、不动设计系统。`src/types/index.ts` 的 `ProjectState` 已带 `pending`/`missing` 字段，零改动。

## 六、错误处理

| 面 | 口径 | 依据 |
|---|---|---|
| `mark_missing` / `doc_rows_for_project` 的 rusqlite 错 | `?` 传播 ⇒ 整轮落 `error` 终态 + `last_error`，不静默吞 | `AppError` 的 `From<rusqlite::Error>` 走既有 `db_failed`；吞掉回收失败等于界面谎称「已追平」 |
| 两条闸门 / 熔断跳过 | **不是错误**：不写 `last_error`、不动 `summary`，函数只回 `Ok(0)`。「被哪一支挡下」只写在注释与测试名里，不变成类型 | 一轮被取消时不做额外写库是本仓既有取舍 |
| I-4 尺寸超限 | `preview_unavailable`，`hint` 里的上限数从 `opts.max_file_bytes` 格式化而来 | M4 §七 把「前端只认 `preview_*` 两个码」定成契约；且 message/hint **不许写 `20971520` 裸字面量** —— HANDOFF 坑 12/49：注释与字符串同样本仓 grep 门的命中范围 |
| I-5 单飞占用 | 同一条 `preview_unavailable`，`hint`「已有一份预览正在抽取，稍后再试」 | 三成因共用一码是 D10 的有意复用，见风险 5 |
| 文件当场已不在 | 走既有 `preview_file_missing`（`fs_failed` 那层翻译） | M4 §七；与 reconcile 的语义天然呼应，不需要新码 |

原则照 M4：**预览失败只让对话框变红，绝不让整段结果消失**；差集回收的失败属于索引作业失败，走 `last_error` 那条既有通路。

## 七、测试策略

Rust 单测一律 `db::open_in_memory()` + `tempfile::tempdir()` 夹具，绝不碰任何真实目录（红线 1、2）。每条都要能被它所守护的那个表达式打红，写之前先做反值心算（HANDOFF 坑 10）。

1. **差集主路径**：库 3 行全 `ok` 且各带 FTS 词，`seen` 只含其中 2 个 path ⇒ 断那 1 行 `index_status='missing'`、`skip_reason` 仍是 NULL、`index_docs` 那一行**还在**（`count = 1`）、而它的 FTS 行没了（按该 rowid 查 `index_docs_fts` 为空 + `fts_orphan_rows() == 0`）。变异两次各红一处：删掉 `DELETE FROM index_docs_fts` ⇒ 红在孤儿行数；删掉 UPDATE ⇒ 红在 status。
2. **两条硬闸门各一条测试，不许合并**。这是可证伪性的硬要求：合并成一条时「只删其中一支」照样绿（另一支还在跳过）。`root_missing` 那条用**库里只剩 3 行**的小夹具 —— 少了这支，3 个候选过不了熔断的 `> 20` 地板、会被真的标掉，于是这条测试能红；`capped` 那条用「本轮扫到 2 条、库里有 3 条」的夹具，少了那支就标 1 条。两支各自只触发自己那一支、另一支不触发，并各配一次「删掉对应闸门 ⇒ 红」。
3. **熔断要有阳性对照，而且两个条件各一条**（写成两条测试，互为对照）：一条「25 行缺 24 ⇒ 一格都不动」守「过半」那一半，一条「10 行缺 6 ⇒ 标掉 6」守「过地板」那一半。只有前者时，把 `MISSING_FUSE_MIN_ROWS` 改成 0、或整支熔断写成恒跳过，测试照样全绿 —— 因为 24 个候选本来就远超过地板。
4. **回填自愈**：跨两轮 —— 第一轮标 missing，把文件放回原 path，第二轮（`rebuild = false`）断**同一 `doc_rowid`** 回到 `ok` 且 FTS 行回来。变异：删掉 `current_rowid` 里的 `AND index_status = 'ok'` ⇒ 该行 `size/mtime` 相同就走 `unchanged`、永久 missing ⇒ 这条红。
5. **`over_size` 桶必须在 `seen` 里**：一个超限文件有 `too_large` 行且本轮在 `over_size` 桶里 ⇒ 断它**没被**标 missing。变异：构 `seen` 时只取 `files` ⇒ 红。这条守的是第四节点名的「本轮最容易写错的一格」。
6. **`set_root_dir` 的事务原子性**：让 INSERT 撞 `ux_project_root`（先给同项目塞第二条 root 行）⇒ 断**旧 root 行还在**。没包事务时 DELETE 已自动提交、这必红；包了才绿。`clear_project` 留在事务外那一格由既有 `repointing_the_root_drops_rows_of_the_old_path` 守着，不重开。
7. **I-4 尺寸门**：tempdir 造一个真实文件，库里那一行故意留小得多的登记 `size` 且状态 `ok`（正是「索引后膨胀」那一格），拿一个低于实际字节数的上限调生产入口 ⇒ 断 `preview_unavailable`，并配一条把上限放宽就必须正常抽出的一次对照。变异：删掉那次 `std::fs::metadata` 判门 ⇒ 它会去抽那个文件并成功返回 ⇒ 红。**2026-10-10 写计划时定的形状**：判门放在生产入口 `render_preview`、上限由命令侧从 `ScanOptions` 取好后**作参数传入**，`render_preview_with` 一字不动 —— 它被测试模块直接调用约 8 处，而那条路刻意不碰磁盘（否则一次签名改动波及 8 个调用点，还把「抽取器可注入」这格 M4 已裁的形状弄浑）。stat 失败一律放行：读不到元数据不代表超限，既有那条 `io_errors_are_translated…` 用假路径走的就是这一格。
8. **I-5 两条**：占用中第二条被拒（断 `code`）；以及「抽取体 panic 后闸门已复位、第二次能进」，借既有 `running_flag_is_released_even_when_the_job_panics` 的形态用 `catch_unwind` 包。少了后一条，一次 panic 就永久锁死预览而**不报任何错**。
9. **不写的两条，理由要落档**（HANDOFF 坑 17：「不该有测试」也要显式记）：① 「missing 行不可预览」—— `row_for_preview` 的谓词是**正向** `d.index_status = 'ok'`，既有 `a_skipped_row_is_preview_unavailable` 守的就是它，再加一条 missing 版本不多抓一类变异；② 「missing 退出召回」—— 同一条正向谓词在 `SELECT_SQL` 里，加上 M3 既有的 `non_ok_outcomes_record_status_but_write_no_fts_rows`。
10. **启动触发那一格无单测**（要真实 `App` 实例）⇒ 与 M3 缺口 2「`index_start` 的线程壳与 `app.emit` 连通性没有 Rust 单测，只有真机证据」同口径，进第八节的「没能自动验证」。
11. **条数不在本文写死**（HANDOFF 坑 9：期望条数只能实跑数出来，不能累加）。总数由实施计划逐任务数，派发前过 `verify-plan-test-count-expectations` 当场数一遍再同步整条聚合链。
12. 前端三格是 JSX 与常量，`node --test` 那张网不覆盖（M4 §八 已定），**不新增测试文件、不新增依赖**，进「没能自动验证」。

## 八、真机验收

沿用 M4 全部红线：真实数据目录 `%APPDATA%\dev.zero.pfm\` 只读、前后按 size+mtime+SHA-256 三行逐字对照；一次性 identifier `dev.zero.pfm.m5test`，跑完连沙盒目录与孤儿进程（vite 1520 / `msedgewebview2.exe` 9222）一起删净；子代理一律不跑 `tauri dev`，真机由控制方亲自执行；`-c` 覆盖走 npm 的 `--` 且动手前先做那两个必然失败的编译前探针；探针每步单独一次 `evaluate`、只回布尔/条数/错误码名，**绝不打印抽出的正文**。

十条可判据断言：

1. 自动轮真跑了：空沙盒启动后进 `/index` 前读到 `overview.running = true`，或计数从 0 变非 0。
2. 自动轮不拖慢首屏：自动轮进行中并发一次 `db_status`，回毫秒数（依据是 worker 自己开连接那条注释）。
3. **主承诺**：删掉一个已索引文件 → 重启 → 该行 `index_status = 'missing'`、`search_all` 正文段不再命中它、`doc_preview` 回 `preview_unavailable`。
4. 改名：改文件名 → 重启 → 旧行 missing + 新行 `ok` 各一（照实读数，不当成缺陷）。
5. **不可达 root 零误标**：某项目 root 指到 `Z:\未挂载\的移动盘`（M1 的既有夹具形态）→ 重启 → 该项目一格都不变（断 `missing` 计数为 0、`ok` 计数不变）。这条守最危险那一格。
6. 熔断两侧各量一次：25 行项目缺 24 个 ⇒ 一格不标；恢复成只缺 3 个 ⇒ 标 3 条。
7. 自愈：把第 3 条删掉的文件放回 → 重启 → 断同一 `doc_rowid` 回 `ok` 且正文又能命中。
8. I-5：连点两份慢文档 → 第二条**立即**回 `preview_unavailable`（不用等 M4 量到的那 266 秒）。
9. I-4：把已索引的 `.txt` 追加到超过上限 → 当场预览即拒（不等下一轮扫描降级）。
10. 顺手量自动轮进行中的一次 `search_all` 毫秒数（WAL 写密集下的读延迟，风险 4 的证据），并核对清场。

**没能自动验证（逐条点名，不许省略）**：setup 那一行的启动触发（无单测）；自动轮在几十 GB 真实语料上的整轮耗时与磁盘 IO 峰值（沙盒数字不等于真实目录）；无权限子树导致的单点误标是否真会发生（本机造不出稳定的权限样本）；`missing` 行长期累积后的库体积；前端三格的 JSX 渲染；D10 三成因共用一条码在真实界面上读起来分不分得开。

## 九、不在本轮（含两处要改既有归属的理由）

| 项 | 归属 | 为什么不在本轮 |
|---|---|---|
| `notify` 实时监听 | M5.5 | 首次引入依赖 + 首次引入常驻线程句柄 + SMB/网络盘丢事件 + 去抖 + 「监听线程写索引」的新不变量，五项都在同一轮付；而本轮的启动自动轮已覆盖「追平」这件事的正确性部分，只欠实时性 |
| m3 分批断点续建 / `pending` 的写入点 | M5.5 | 需要一个跨轮进度游标，与本轮的差集无耦合 |
| settings 写 IPC + 设置页（技术方案的「监听开关」） | M5.5 | `settings` 今天只有读入口 `ScanOptions::load`，41 条命令里零写命令；做开关要先补这一格，而本轮自动轮是默认开、无开关 |
| m2 另一半：`ON DELETE CASCADE` + FTS 触发器 + **每轮全表孤儿体检** | M5.5 | 硬删项目今天仍不可达；全表 `NOT IN` 体检的成本未测。**这一条是对 M3 计划「FTS 孤儿行由 M5 的启动对账统一回收」那句的缩量**，本轮只保证 `mark_missing` 自己不产孤儿（点删），显式点名 |
| I-6 分段渲染 / 合并那 6 条 LIKE | M5.5 | 架构级，与本轮 Files 面正交；M4 已写明换 async 不是解法 |
| 簇内 offset 分页（D6 那格，`hidden = 190` 的真解） | M5.5 | offset 与「段内聚簇」纠缠，M4 已两次记为 deferred |
| `toAppError` 对 JS `Error` 显示成 `{}`；`DocPreviewDialog` 的 `guard` 缺 seq/挂载守卫；台账侧 `MAX_CLUSTERS_PER_SECTION` 无独立用例；簇序第三级 `id` 键无守护；`DocPreview.ranges` 值形状；`src/types/index.ts` 那句 `lib.rs:66-73` 漂一行 | M5.5 清账 | M4 那张 9 条缺口表的其余项，本轮一格不带（需求方裁定） |
| 路径 canonicalize / `path` 列归一化存储 | 债务表，随折叠一起 | 要 V5 迁移 + 全量重抽 |
| 换写法后的旧串行处置 | 不做 | 因为 D7 不折叠，这个问题不存在 |
| 熔断/闸门跳过的界面可观测 | 不做，记为有名缺口 | D8 零新线格式字段；与本仓 sweep 回收条数「界面上看不到」同口径 |
| 回收后触发 `VACUUM` 缩库 | 不做 | 体积不是本轮承诺 |
| 改名识别 | 不做 | 只省重抽、不改变正确性，还会把「两个不同文件折叠成同一个键」的风险引进 BINARY UNIQUE |
| 自动轮的可关开关 | 不做 | 前提格（settings 写 IPC + 设置页）不在本轮 |

## 十、风险

1. **「一启动就全盘遍历 + 写库」成为常态**，而技术方案 §十.1 那条「性能与体积数字均未做运行时验证」还挂着 —— 本轮把一次未实测的行为变成默认承重。三条缓解（root 不可达零遍历、`unchanged` 不开文件、取消按钮已在）都不是实测数字。
2. **首轮自动索引是分钟级，而 `/index` 不是首屏**：用户可能几小时没察觉索引在跑；那段时间正文段是逐轮补齐的（不是谎，是不全）。
3. **无权限子树的单点误标不自愈**，熔断只挡成片（第四节已点名）。
4. **WAL 写密集期间的首屏检索延迟**：M4 量到的 612–737 ms 是静场读数；本轮启动即有写在跑。第八节第 10 条出数字，越线才立 finding。
5. **三成因共用 `preview_unavailable`** = 同一句红字三个意思（行不 `ok` / 尺寸超限 / 单飞占用），只靠 `hint` 分。终审若判不可接受，正解是加一个 `preview_*` 码 ⇒ 那要**先改 M4 §七 的码表契约**再实现，不许实现者临场加。
6. **熔断跳过时界面与不跳过时一模一样**，用户看不出被挡（D8 的直接代价）。缓解只有一句静态界面文案，不是数据。
7. **`missing` 行只增不减**：永久扫不到的那些会累积，因为本轮保留元数据行兑现可见性、又不给删除入口。M5.5 若做「失效清单一键清除」要新 IPC 命令。
8. **`unchecked_transaction` 走 `BEGIN DEFERRED`**（vendored `rusqlite-0.40.2/src/transaction.rs` 的 `new_unchecked` match 臂）。SQLite 在 DEFERRED 事务里把读升级为写失败时，据说是**立即**回 `SQLITE_BUSY` 而不走 busy handler —— **此条未验证**，核对路径是 SQLite 官方事务/锁定文档，以及计划里加一条「自动轮写密集时做一次改指」的并发探针。备选缓解两个：改用 `Immediate`（`set_transaction_behavior` 要 `&mut Connection`，签名得动）或失败重试一次。落点 8 的实现要按这次核对结论定。

## 十一、要回写与要更正的文档

- **回写活文档** `docs/技术方案.md`：§6.1「变更检测」那一行按实际交付改写（启动自动增量轮 + 差集回收已做，`notify` 监听延后到 M5.5 并写明理由）；§八 里程碑表 M5 那一行的内容描述随之改准；§九「`notify` 监听默认开，设置页可关」加一条 dated 附注（本轮的自动轮默认开、无开关，开关随 M5.5 的 settings 写入口一起做）。
- **不回写历史规格**：M3 计划里「`delete_doc` 归 M5 补」「FTS 孤儿行由 M5 启动对账统一回收」那两句是当时切片的记录，按本仓分层规则不做 retro-edit；它们的**归属变化记在本文第九节**（第三句已经写明是缩量），M5 收口时同步进行程文档即可。
- **M5 收口时要动**：`docs/开发进度.md` 里程碑表 M5 行 + 新增「M5 验收证据」整节 + 缺口归属；`docs/HANDOFF.md` 第 12 版（含本轮新坑）；`.superpowers/sdd/2026-10-07-m4-unified-search/` **不动**（里面是被已提交文档按路径引用的证据）。
