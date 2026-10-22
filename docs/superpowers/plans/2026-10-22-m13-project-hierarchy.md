# 父子项目关联（M13）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给项目加单父归属，形成一棵可多层嵌套的项目树；列表可按总包分组、详情能看到父子关系，并且有子项目的项目删不掉。

**Architecture:** `projects.parent_id` 加一列（V9，零回填、无外键），现有 `project_create/update/delete/list/get` 五条命令的契约内扩 `parentId`，不新增 IPC；父项存在性、自指/成环与删除守卫全在 Rust 命令层。前端用 `project-tree.ts` 三个纯函数建森林/扁平化/候选排除，`ProjectForm` 加可选「所属总包」下拉，项目列表加「全部 / 按总包」两视图，详情页加父子关系区。

**Tech Stack:** Tauri 2.12 / Rust rusqlite 0.40.2 / React 19 + TypeScript / zustand / shadcn Select、Dialog / node:test。

**Spec:** `docs/superpowers/specs/2026-10-22-m13-project-hierarchy-design.md`

## Global Constraints

- 单父树：`projects.parent_id` 可空，层级不限；不建多父/DAG、不做级联删除、不做拖拽改归属、不加新导航页。
- 父项必须存在且未删除；**归档项目可以当父项**；禁止自指、禁止挂到自己的后代下（沿父链上溯，遇环即拒）。
- 有**未删除的直接子项目**时禁止删除父项目，`invalid_input` 点名子项目数量；已软删子项不阻塞；不级联。
- V9 只做 `ALTER TABLE projects ADD COLUMN parent_id TEXT` + `CREATE INDEX idx_projects_parent_id ON projects(parent_id)`；**零回填**；`TARGET_VERSION` **8→9**。
- 命令数仍 **69**、`tauri::command(async)` 仍 **7**、collect 仍 **9**、`allow(dead_code)` 仍 **1**、`Cargo.lock` `[[package]]` 仍 **616**；`Cargo.toml` / `Cargo.lock` / `package.json` 对 `master` diff 必须空。
- 备份：`Counts` 仍 **7 键**；包 `schemaVersion` **9**；旧包（8）可导入并跑 V9；`warn_plaintext_columns` 文本不变。
- 模板：不保存归属；模板管理弹窗不显示父项；`applyTemplate` 保留 `parentId`；模板保存时多出的 `parentId` 键被 `TemplateInput` 忽略。
- 错误面只用 `invalid_input` / `not_found`；文案点名「所属总包」「子项目」，可直接展示。
- 前端 `ProjectForm` 的 `parentOptions` 是**可选** prop：不传即不渲染；模板管理弹窗不传，现有表单行为不变。
- 同一 worktree 只允许一个会提交的实现者；Task 3 与 Task 5 都改 `src/pages/projects.tsx`，必须串行。
- 子代理一律不跑 `tauri dev` / `npm run dev`；真机格只由控制方用一次性 identifier 沙盒跑，真实数据目录三文件前后 `size+mtime+sha256` 逐字一致。
- 退出码真取：`cmd > log 2>&1; echo "exit=$?"`，绝不 `cmd | tail; echo $?`；cargo 失败是 **101**，node 失败是 **1**。
- 本机 cwd 会在工具调用之间残留：每条批量命令显式 `cd "E:/zero/demo/2026/project-files-manage"` 或 `cd src-tauri`，不靠上一条的目录。
- 文档与源码保持 CRLF（`git ls-files --eol` 应见 `i/lf w/crlf`）；提交前 `git diff --check`。
- 不 stage / 不删除未跟踪的 `Python/`；证据目录 `.superpowers/sdd/2026-10-22-m13-project-hierarchy/` 被文档按路径引用，收口前不 `git clean -fdx`。

## 任务总览与数链

| Task | 交付 | 新增 cargo 测试 | cargo 累计 | 新增 node 测试 | node 累计 | 结构门 |
|---|---|---|---|---|---|---|
| 基线 | M12 已合 master（`0ff5df6`） | — | **346** | — | **57** | 69 / 7 / 8 / 1 / 616 / 9 |
| 1 | V9 迁移 + 备份/DB 测试 | +2 | **348** | 0 | 57 | TARGET 变 **9** |
| 2 | 后端 parent 字段 + 校验 + 删除守卫 | +10（9 project + 1 backup） | **358** | 0 | 57 | 命令仍 69 |
| 3 | 类型 / `project-tree` / 模板边界 | 0 | 358 | +9（8 tree + 1 template） | **66** | 不变 |
| 4 | `ProjectForm` 父项下拉 | 0 | 358 | 0 | 66 | 不变 |
| 5 | 列表两视图 + 行组件 + 候选接线 | 0 | 358 | 0 | 66 | 不变 |
| 6 | 详情父子区 + 编辑候选 | 0 | 358 | 0 | 66 | 不变 |
| 7 | 门禁 + 十处变异 + 真机 12 格 + 映射（控制方自跑） | 0 | 358 | 0 | 66 | 终态 69 / 7 / 9 / 1 / 616 / 9 |
| 8 | 活文档回写 | 0 | 358 | 0 | 66 | 不变 |

新增 cargo **12** 支（Task 1 的 2 支 + Task 2 的 10 支）；新增 node **9** 支。任一格对不上先查测试函数个数与过滤词，不要硬凑。

## 派发前预检（控制方填）

- [x] V9 SQL 在 Python `sqlite3` 临时库实跑：先建 `projects(id,name,deleted_at)` 并插一行，再 `ALTER TABLE ... ADD COLUMN parent_id TEXT` + 建索引；读数 `existing_parent=None`、显式索引只有 `('idx_projects_parent_id',)`。
- [x] `project-tree` 算法在临时 `.mjs` 实跑（ASCII 夹具）：多层 `roots=['root']`、children `['a','b']`、孙 depth 2、扁平 `root:0/a:1/a1:2/b:1`；`parentCandidates(items,'a')` = `root:0/b:1`；`parentCandidates(items,'root')` 为空；缺父 `['x']`；自指 `['s:0']`；双节点环 `['c1:0','c2:0']` 且扁平各出现一次。**首版把「链能到顶」误当根判定（全部变根），已改成 `inCycle` 判据并复跑通过**，计划里的代码是修好的版本。
- [x] 基线现跑：`cargo test -j 1 --lib` = **346 passed / exit=0**；仓库根 `node --test "tests/**/*.test.ts"` = **57/57/0**；结构门 `cmd=69 async=7 TARGET_VERSION: i64 = 8 collect=9 dead=1 lock=616 merges=0`。
- [x] 测试函数计数：Task 1 新增 2 支（`v9_projects_parent_id_and_index_exist`、`v9_parent_id_is_added_null_for_existing_projects`；`v8_schema_meta...` 是既有测试改名，不增数）；Task 2 新增 9 支 project + 1 支 backup；Task 3 node 新增 8 + 1 支。
- [x] 修改点盘点：`ProjectInput` 结构体字面量共 4 处（`project.rs` 测试 helper、`ledger.rs`、`ledger_export.rs`、`search.rs`）；`Project` 字面量只在 `project.rs` 的 list/get 与前端 `emptySeed`；`Project`/`ProjectInput` 的 TS 构造点只有 `types/project.ts`、`template-manager.tsx`、`projects.tsx`。
- [x] 本机 CRLF：新文件写完后 `unix2dos`，提交前 `git diff --check`。

---

### Task 1: V9 迁移 + 备份/DB 测试

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/backup.rs:856-880`（`backup_package_carries_project_templates_without_counting_them` 的 schema 断言）
- Modify: `src-tauri/src/lib.rs:1073`（测试里的版本注释）

**Interfaces:**
- Produces: `projects.parent_id TEXT` 与 `idx_projects_parent_id`；`TARGET_VERSION: i64 = 9`；备份包 `schemaVersion=9`。

- [ ] **Step 1: 先写失败测试（db.rs）**

在 `src-tauri/src/db.rs` 的 `mod tests` 里，把现有 `v8_schema_meta_reaches_target_version_eight` 整支替换为：

```rust
    #[test]
    fn v9_schema_meta_reaches_target_version_nine() {
        let conn = open_in_memory().unwrap();
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 9, "迁移必须跑到 9");
        assert_eq!(TARGET_VERSION, 9, "= 上一条，两处别各写各的");
    }
```

在它下面追加两支：

```rust
    /// V9 必须给 projects 加 parent_id 列，并且只建这一条显式索引。
    /// `sqlite_autoindex_*`（sql 为 NULL）不算：TEXT 主键每张表都有。
    #[test]
    fn v9_projects_parent_id_and_index_exist() {
        let conn = open_in_memory().unwrap();
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('projects')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert!(cols.iter().any(|c| c == "parent_id"), "V9 必须加 parent_id 列：{cols:?}");

        let idx: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master
                  WHERE type = 'index' AND tbl_name = 'projects' AND sql IS NOT NULL
                  ORDER BY name",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(idx, vec!["idx_projects_parent_id"], "projects 上只该有这一条显式索引：{idx:?}");
    }

    /// V9 零回填：先手工把库建到 V8、落一条存量项目，再跑 V9；
    /// 存量行必须保持 `parent_id IS NULL`（迁移里只有 DDL，不猜归属）。
    #[test]
    fn v9_parent_id_is_added_null_for_existing_projects() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_meta (version INTEGER NOT NULL, applied_at TEXT NOT NULL);",
        )
        .unwrap();
        for (target, ddl) in [
            (1, V1),
            (2, V2),
            (3, V3),
            (4, V4),
            (5, V5),
            (6, V6),
            (7, V7),
            (8, V8),
        ] {
            apply_version(&conn, target, ddl).unwrap();
        }
        conn.execute(
            "INSERT INTO projects (id, name) VALUES ('p-old', '存量项目')",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap();

        let parent: Option<String> = conn
            .query_row(
                "SELECT parent_id FROM projects WHERE id = 'p-old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(parent, None, "V9 零回填：存量项目必须保持顶层");
        let ver: i64 = conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, 9);
    }
```

同时把 `backup.rs` 里 `backup_package_carries_project_templates_without_counting_them` 的两行改成：

```rust
        assert_eq!(sum.schema_version, crate::db::TARGET_VERSION);
        assert_eq!(sum.schema_version, 9, "M13 起包内 schema 必须是 9");
```

把 `lib.rs:1073` 的断言消息改成：

```rust
        assert_eq!(ver, crate::db::TARGET_VERSION, "= db.rs 的 TARGET_VERSION，链头已随 M13 V9 走到 9");
```

- [ ] **Step 2: 跑测试确认红**

```bash
cd "E:/zero/demo/2026/project-files-manage/src-tauri"
cargo test -j 1 --lib v9 > /tmp/m13-t1-red.log 2>&1; echo "exit=$?"
grep -E "test result|error\[|failed" /tmp/m13-t1-red.log | tail -10
cargo test -j 1 --lib backup_package_carries_project_templates_without_counting_them > /tmp/m13-t1-red2.log 2>&1; echo "exit=$?"
grep -E "test result|panicked|assertion" /tmp/m13-t1-red2.log | tail -8
```

Expected：第一条 `exit=101`（`TARGET_VERSION` 仍是 8、`parent_id` 不存在）；第二条 `exit=101`（schema 仍是 8）。若测试根本没编译，先检查测试代码拼写，不要跳过红。

- [ ] **Step 3: 实现 V9**

在 `db.rs` 里把 `TARGET_VERSION` 改成 9，`migrate` 的 `match` 加一行 `9 => V9,`，并在 `V8` 常量后面追加：

```rust
/// v9：父子项目关联（M13 spec §三）。单父树；不建外键、不级联，环与父项有效性
/// 由 `project::validate_parent` 在命令层判。**零回填**：存量项目保持顶层。
const V9: &str = "
ALTER TABLE projects ADD COLUMN parent_id TEXT;
CREATE INDEX idx_projects_parent_id ON projects(parent_id);
";
```

- [ ] **Step 4: 跑绿并提交**

```bash
cd "E:/zero/demo/2026/project-files-manage/src-tauri"
cargo test -j 1 --lib v9 > /tmp/m13-t1-green.log 2>&1; echo "exit=$?"
grep "test result" /tmp/m13-t1-green.log
cargo test -j 1 --lib backup_package_carries_project_templates_without_counting_them > /tmp/m13-t1-green2.log 2>&1; echo "exit=$?"
grep "test result" /tmp/m13-t1-green2.log
cd "E:/zero/demo/2026/project-files-manage"
git add src-tauri/src/db.rs src-tauri/src/backup.rs src-tauri/src/lib.rs
git diff --cached --check
git commit -m "feat(m13): V9 projects.parent_id 与索引（零回填）+ 备份 schema 9"
```

Expected：两条 `test result: ok`、`exit=0`。Task 1 结束时 `cargo test -j 1 --lib` 应为 **348 passed**。

---

### Task 2: 后端 parent 字段 + 校验 + 删除守卫

**Files:**
- Modify: `src-tauri/src/project.rs`（结构体、`validate_parent`、create/update/delete/list/get、测试）
- Modify: `src-tauri/src/ledger.rs:690`、`src-tauri/src/ledger_export.rs:440`、`src-tauri/src/search.rs:406`（测试夹具补 `parent_id: None`）
- Modify: `src-tauri/src/backup.rs`（新增父子字段随包往返测试）

**Interfaces:**
- Consumes: Task 1 的 `projects.parent_id` 列。
- Produces: `Project.parent_id: Option<String>`、`ProjectInput.parent_id: Option<String>`（线格式 `parentId`）；`create/update/list/get` 都读写该列；`delete_project` 有子项守卫。

- [ ] **Step 1: 写失败测试（project.rs）**

先在 `project.rs` 的测试 helper `input(name)` 里补 `parent_id: None`，然后追加 9 支测试：

```rust
    #[test]
    fn create_with_parent_roundtrips_in_get_and_list() {
        let conn = crate::db::open_in_memory().unwrap();
        let parent = create_project(&conn, &input("总包")).unwrap();
        let mut child_input = input("子项目");
        child_input.parent_id = Some(parent.id.clone());
        let child = create_project(&conn, &child_input).unwrap();

        assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
        assert_eq!(
            get_project(&conn, &child.id).unwrap().parent_id.as_deref(),
            Some(parent.id.as_str())
        );
        let listed = list_projects(&conn).unwrap();
        assert_eq!(
            listed.iter().find(|p| p.id == child.id).unwrap().parent_id.as_deref(),
            Some(parent.id.as_str())
        );
    }

    #[test]
    fn update_moves_project_and_can_clear_parent() {
        let conn = crate::db::open_in_memory().unwrap();
        let a = create_project(&conn, &input("总包甲")).unwrap();
        let b = create_project(&conn, &input("总包乙")).unwrap();
        let mut child_input = input("子项目");
        child_input.parent_id = Some(a.id.clone());
        let child = create_project(&conn, &child_input).unwrap();

        let mut move_input = input("子项目");
        move_input.parent_id = Some(b.id.clone());
        let moved = update_project(&conn, &child.id, &move_input).unwrap();
        assert_eq!(moved.parent_id.as_deref(), Some(b.id.as_str()));

        let mut clear_input = input("子项目");
        clear_input.parent_id = None;
        let cleared = update_project(&conn, &child.id, &clear_input).unwrap();
        assert_eq!(cleared.parent_id, None);
    }

    #[test]
    fn self_parent_is_rejected() {
        let conn = crate::db::open_in_memory().unwrap();
        let p = create_project(&conn, &input("自指项目")).unwrap();
        let mut self_input = input("自指项目");
        self_input.parent_id = Some(p.id.clone());

        let err = update_project(&conn, &p.id, &self_input).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("自己"), "{}", err.message);
    }

    #[test]
    fn descendant_parent_is_rejected_as_a_cycle() {
        let conn = crate::db::open_in_memory().unwrap();
        let a = create_project(&conn, &input("总包")).unwrap();
        let mut b_input = input("子项目");
        b_input.parent_id = Some(a.id.clone());
        let b = create_project(&conn, &b_input).unwrap();
        let mut c_input = input("孙项目");
        c_input.parent_id = Some(b.id.clone());
        let c = create_project(&conn, &c_input).unwrap();

        let mut cycle_input = input("总包");
        cycle_input.parent_id = Some(c.id.clone());
        let err = update_project(&conn, &a.id, &cycle_input).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("循环"), "{}", err.message);

        let mut direct_input = input("总包");
        direct_input.parent_id = Some(b.id.clone());
        assert_eq!(
            update_project(&conn, &a.id, &direct_input).unwrap_err().code,
            "invalid_input",
            "直接挂到自己的子项目也必须拒"
        );
    }

    #[test]
    fn missing_or_deleted_parent_is_rejected() {
        let conn = crate::db::open_in_memory().unwrap();
        let mut orphan_input = input("孤儿项目");
        orphan_input.parent_id = Some("no-such-parent".into());
        let err = create_project(&conn, &orphan_input).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("不存在"), "{}", err.message);

        let p = create_project(&conn, &input("待删总包")).unwrap();
        delete_project(&conn, &p.id).unwrap();
        let mut deleted_input = input("新子项目");
        deleted_input.parent_id = Some(p.id.clone());
        let err = create_project(&conn, &deleted_input).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("已删除"), "{}", err.message);
    }

    #[test]
    fn archived_project_can_still_be_a_parent() {
        let conn = crate::db::open_in_memory().unwrap();
        let mut archived_input = input("已归档总包");
        archived_input.status = "归档".into();
        let parent = create_project(&conn, &archived_input).unwrap();

        let mut child_input = input("子项目");
        child_input.parent_id = Some(parent.id.clone());
        let child = create_project(&conn, &child_input).unwrap();
        assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
    }

    #[test]
    fn delete_is_blocked_while_live_children_exist() {
        let conn = crate::db::open_in_memory().unwrap();
        let parent = create_project(&conn, &input("总包")).unwrap();
        let mut child_input = input("子项目");
        child_input.parent_id = Some(parent.id.clone());
        let child = create_project(&conn, &child_input).unwrap();

        let err = delete_project(&conn, &parent.id).unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(err.message.contains("1"), "文案必须点名子项数量：{}", err.message);
        assert!(get_project(&conn, &parent.id).is_ok(), "被拒后父项目必须还在");

        delete_project(&conn, &child.id).unwrap();
        delete_project(&conn, &parent.id).unwrap();
        assert_eq!(get_project(&conn, &parent.id).unwrap_err().code, "not_found");
    }

    #[test]
    fn a_soft_deleted_child_does_not_block_the_parent() {
        let conn = crate::db::open_in_memory().unwrap();
        let parent = create_project(&conn, &input("总包")).unwrap();
        let mut child_input = input("子项目");
        child_input.parent_id = Some(parent.id.clone());
        let child = create_project(&conn, &child_input).unwrap();

        delete_project(&conn, &child.id).unwrap();
        delete_project(&conn, &parent.id).unwrap_or_else(|e| {
            panic!("已软删的子项目不该阻塞父项目删除：{e}");
        });
    }

    #[test]
    fn update_of_missing_project_reports_not_found_before_parent_errors() {
        let conn = crate::db::open_in_memory().unwrap();
        let mut bad_input = input("不存在项目");
        bad_input.parent_id = Some("no-such-parent".into());

        let err = update_project(&conn, "no-such-id", &bad_input).unwrap_err();
        assert_eq!(err.code, "not_found", "未知 id 必须先报 not_found，父项校验不能抢先");
    }
```

在 `backup.rs` 的测试模块里追加：

```rust
    /// M13 格 12 的 cargo 半边：父字段随整库快照进包。
    #[test]
    fn parent_id_survives_the_backup_snapshot() {
        let live = crate::db::open_in_memory().unwrap();
        live.execute(
            "INSERT INTO projects (id, name, status) VALUES ('p-parent', '总包', '立项')",
            [],
        )
        .unwrap();
        live.execute(
            "INSERT INTO projects (id, name, status, parent_id)
             VALUES ('p-child', '子项目', '立项', 'p-parent')",
            [],
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("hierarchy.zip");
        export(&live, &dst).unwrap();

        let (_unpacked, got) = unpack_db(&dst);
        let pid: Option<String> = got
            .query_row(
                "SELECT parent_id FROM projects WHERE id = 'p-child'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pid.as_deref(), Some("p-parent"));
    }
```

- [ ] **Step 2: 跑测试确认红**

```bash
cd "E:/zero/demo/2026/project-files-manage/src-tauri"
cargo test -j 1 --lib > /tmp/m13-t2-red.log 2>&1; echo "exit=$?"
grep -E "^error|test result|failed" /tmp/m13-t2-red.log | tail -12
```

Expected：`exit=101`，编译错误指向 `Project` / `ProjectInput` 没有 `parent_id` 字段（测试先写、结构体后加，红在编译层即预期）。

- [ ] **Step 3: 实现结构体与 SQL**

在 `project.rs` 顶部把集合导入改成：

```rust
use std::collections::{HashMap, HashSet};
```

`Project` 在 `status` 后加：

```rust
    pub parent_id: Option<String>,
```

`ProjectInput` 在 `status` 后加：

```rust
    #[serde(default)]
    pub parent_id: Option<String>,
```

在 `validate` 函数后面加父项校验：

```rust
fn validate_parent(
    conn: &Connection,
    self_id: Option<&str>,
    parent_id: Option<&str>,
) -> AppResult<()> {
    let Some(pid) = parent_id else {
        return Ok(());
    };
    if self_id == Some(pid) {
        return Err(invalid("项目不能把自己设为所属总包"));
    }
    let exists = conn
        .query_row(
            "SELECT 1 FROM projects WHERE id = ?1 AND deleted_at IS NULL",
            [pid],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(invalid("所属总包不存在或已删除"));
    }
    if let Some(self_id) = self_id {
        let mut cursor = Some(pid.to_owned());
        let mut visited = HashSet::new();
        while let Some(cur) = cursor {
            if cur == self_id {
                return Err(invalid("不能把项目挂到自己的子项目下（会形成循环）"));
            }
            if !visited.insert(cur.clone()) {
                return Err(invalid("父子关系存在循环，请先解除循环后再保存"));
            }
            cursor = conn
                .query_row(
                    "SELECT parent_id FROM projects WHERE id = ?1 AND deleted_at IS NULL",
                    [&cur],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten();
        }
    }
    Ok(())
}
```

`create_project` 改成：

```rust
pub fn create_project(conn: &Connection, input: &ProjectInput) -> AppResult<Project> {
    validate(input)?;
    let parent_id = normalize(input.parent_id.clone());
    validate_parent(conn, None, parent_id.as_deref())?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO projects (id, name, code, manager, customer, contact_name, contact_phone,
                               contract_no, contract_period, delivery_deadline, status, parent_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            id,
            input.name.trim(),
            normalize(input.code.clone()),
            normalize(input.manager.clone()),
            normalize(input.customer.clone()),
            normalize(input.contact_name.clone()),
            normalize(input.contact_phone.clone()),
            normalize(input.contract_no.clone()),
            normalize(input.contract_period.clone()),
            normalize(input.delivery_deadline.clone()),
            input.status,
            parent_id,
        ],
    )?;
    replace_tags(conn, &id, &input.tags)?;
    get_project(conn, &id)
}
```

`update_project` 改成：

```rust
pub fn update_project(conn: &Connection, id: &str, input: &ProjectInput) -> AppResult<Project> {
    validate(input)?;
    let parent_id = normalize(input.parent_id.clone());
    let exists = conn
        .query_row(
            "SELECT 1 FROM projects WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(not_found(id));
    }
    validate_parent(conn, Some(id), parent_id.as_deref())?;
    let changed = conn.execute(
        "UPDATE projects SET name=?2, code=?3, manager=?4, customer=?5, contact_name=?6,
                contact_phone=?7, contract_no=?8, contract_period=?9, delivery_deadline=?10,
                status=?11, parent_id=?12, updated_at=datetime('now')
         WHERE id=?1 AND deleted_at IS NULL",
        params![
            id,
            input.name.trim(),
            normalize(input.code.clone()),
            normalize(input.manager.clone()),
            normalize(input.customer.clone()),
            normalize(input.contact_name.clone()),
            normalize(input.contact_phone.clone()),
            normalize(input.contract_no.clone()),
            normalize(input.contract_period.clone()),
            normalize(input.delivery_deadline.clone()),
            input.status,
            parent_id,
        ],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    replace_tags(conn, id, &input.tags)?;
    get_project(conn, id)
}
```

`delete_project` 改成：

```rust
pub fn delete_project(conn: &Connection, id: &str) -> AppResult<()> {
    let name: String = conn
        .query_row(
            "SELECT name FROM projects WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| not_found(id))?;
    let children: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE parent_id = ?1 AND deleted_at IS NULL",
        [id],
        |r| r.get(0),
    )?;
    if children > 0 {
        return Err(AppError::new(
            "invalid_input",
            &format!("项目「{name}」下还有 {children} 个未删除的子项目，请先调整归属或删除子项目"),
            Some("子项目删除或改挂到其他项目后，本项才能删除"),
        ));
    }
    let changed = conn.execute(
        "UPDATE projects SET deleted_at = datetime('now'), updated_at = datetime('now')
         WHERE id = ?1 AND deleted_at IS NULL",
        [id],
    )?;
    if changed == 0 {
        return Err(not_found(id));
    }
    Ok(())
}
```

`list_projects` 的 SELECT 与构造改成：

```rust
        "SELECT id, name, code, manager, customer, contact_name, contact_phone,
                contract_no, contract_period, delivery_deadline, status, parent_id
         FROM projects WHERE deleted_at IS NULL ORDER BY updated_at DESC",
```

```rust
                status: r.get(10)?,
                parent_id: r.get(11)?,
                tags: Vec::new(),
                dirs: Vec::new(),
```

`get_project` 的 SELECT 与元组改成：

```rust
            "SELECT name, code, manager, customer, contact_name, contact_phone,
                    contract_no, contract_period, delivery_deadline, status, parent_id
             FROM projects WHERE id = ?1 AND deleted_at IS NULL",
```

```rust
                    r.get::<_, String>(9)?,
                    r.get::<_, Option<String>>(10)?,
                ))
```

`get_project` 返回的 `Project` 里加：

```rust
        status: base.9,
        parent_id: base.10,
        tags,
        dirs,
```

- [ ] **Step 4: 补四处测试夹具**

`src-tauri/src/ledger.rs`、`src-tauri/src/ledger_export.rs`、`src-tauri/src/search.rs` 里每一处 `project::ProjectInput { ... }` / `crate::project::ProjectInput { ... }` 都在 `tags: ...` 后补一行：

```rust
                parent_id: None,
```

（缩进按所在文件对齐；这三处只在 `#[cfg(test)]` 里。）

- [ ] **Step 5: 跑全量 cargo 并提交**

```bash
cd "E:/zero/demo/2026/project-files-manage/src-tauri"
cargo test -j 1 --lib > /tmp/m13-t2-green.log 2>&1; echo "exit=$?"
grep "test result" /tmp/m13-t2-green.log
cargo clippy -j 1 --lib -- -D warnings > /tmp/m13-t2-clippy.log 2>&1; echo "clippy=$?"
cd "E:/zero/demo/2026/project-files-manage"
git add src-tauri/src/project.rs src-tauri/src/ledger.rs src-tauri/src/ledger_export.rs src-tauri/src/search.rs src-tauri/src/backup.rs
git diff --cached --check
git commit -m "feat(m13): 项目 parentId 读写、父项校验与有子禁删守卫"
```

Expected：cargo **358 passed / exit=0**；clippy `exit=0`。若数不是 358，按测试函数名逐支数（Task 1 的 2 支 + 本任务 10 支）后再查过滤词。

---

### Task 3: 前端类型 / `project-tree` / 模板边界

**Files:**
- Modify: `src/types/project.ts`
- Create: `src/lib/project-tree.ts`
- Modify: `src/lib/apply-template.ts`
- Modify: `src/components/template-manager.tsx`
- Modify: `src/pages/projects.tsx`（只补 `emptySeed` 的 `parentId`，让 tsc 过关；列表 UI 在 Task 5）
- Test: `tests/project-tree.test.ts`（新建）、`tests/apply-template.test.ts`

**Interfaces:**
- Consumes: Task 2 的 `parentId` 线格式。
- Produces:
  - `Project` / `ProjectInput` 带 `parentId: string | null`；`EMPTY_INPUT.parentId = null`；`toInput` 映射。
  - `ProjectTreeNode`、`ParentOption`、`buildProjectForest`、`flattenProjectTree`、`parentCandidates`。
  - `applyTemplate` 保留 `parentId`。

- [ ] **Step 1: 写失败测试**

新建 `tests/project-tree.test.ts`：

```ts
import test from "node:test";
import assert from "node:assert/strict";
import {
  buildProjectForest,
  flattenProjectTree,
  parentCandidates,
} from "../src/lib/project-tree.ts";
import type { Project } from "../src/types/project.ts";

const project = (id: string, name: string, parentId: string | null = null): Project => ({
  id,
  name,
  code: null,
  manager: null,
  customer: null,
  contactName: null,
  contactPhone: null,
  contractNo: null,
  contractPeriod: null,
  deliveryDeadline: null,
  status: "立项",
  tags: [],
  dirs: [],
  parentId,
});

const sample = () => [
  project("root", "总包"),
  project("a", "子A", "root"),
  project("a1", "孙A1", "a"),
  project("b", "子B", "root"),
];

test("多层森林保留输入同级顺序，并算出 depth", () => {
  const forest = buildProjectForest(sample());
  assert.deepEqual(forest.map((n) => n.project.id), ["root"]);
  assert.deepEqual(forest[0].children.map((n) => n.project.id), ["a", "b"]);
  assert.equal(forest[0].children[0].children[0].project.id, "a1");
  assert.equal(forest[0].children[0].children[0].depth, 2);
  assert.deepEqual(forest[0].children[1].children, []);
});

test("父项目缺失时按根处理，不丢项目", () => {
  const forest = buildProjectForest([project("x", "孤儿", "ghost")]);
  assert.deepEqual(forest.map((n) => n.project.id), ["x"]);
  assert.equal(forest[0].depth, 0);
  assert.deepEqual(forest[0].children, []);
});

test("自指项目按根处理且只出现一次", () => {
  const forest = buildProjectForest([project("x", "自指", "x")]);
  assert.deepEqual(forest.map((n) => n.project.id), ["x"]);
  assert.deepEqual(forest[0].children, []);
});

test("双节点环不递归死循环，两项都按根各出现一次", () => {
  const forest = buildProjectForest([
    project("c1", "环一", "c2"),
    project("c2", "环二", "c1"),
  ]);
  assert.deepEqual(forest.map((n) => n.project.id), ["c1", "c2"]);
  assert.deepEqual(forest.map((n) => n.children.length), [0, 0]);
  assert.deepEqual(flattenProjectTree(forest).map((n) => n.project.id), ["c1", "c2"]);
});

test("扁平化是先序，depth 与树一致", () => {
  const flat = flattenProjectTree(buildProjectForest(sample()));
  assert.deepEqual(flat.map((n) => n.project.id), ["root", "a", "a1", "b"]);
  assert.deepEqual(flat.map((n) => n.depth), [0, 1, 2, 1]);
});

test("新建项目的父项候选是全部项目的先序扁平表", () => {
  const options = parentCandidates(sample(), null);
  assert.deepEqual(options.map((o) => o.id), ["root", "a", "a1", "b"]);
  assert.deepEqual(options.map((o) => o.depth), [0, 1, 2, 1]);
  assert.equal(options[0].name, "总包");
  assert.equal(options[0].status, "立项");
});

test("编辑时排除自己与全部后代，但保留父/祖先/兄弟", () => {
  const options = parentCandidates(sample(), "a");
  assert.deepEqual(options.map((o) => o.id), ["root", "b"]);
  assert.deepEqual(options.map((o) => o.depth), [0, 1]);
  assert.deepEqual(parentCandidates(sample(), "root"), []);
});

test("构建森林不改动入参数组与对象", () => {
  const items = sample();
  const before = JSON.stringify(items);
  buildProjectForest(items);
  parentCandidates(items, "a");
  assert.equal(JSON.stringify(items), before);
});
```

在 `tests/apply-template.test.ts` 的 `base` 里补 `parentId: "parent-1",`，并追加：

```ts
test("套用模板保留当前 parentId", () => {
  const got = applyTemplate(base, tpl({ manager: "模板经理" }));
  assert.equal(got.parentId, "parent-1");
  const empty = applyTemplate({ ...base, parentId: null }, tpl());
  assert.equal(empty.parentId, null);
});
```

- [ ] **Step 2: 跑测试确认红**

```bash
cd "E:/zero/demo/2026/project-files-manage"
node --test "tests/project-tree.test.ts" > /tmp/m13-t3-red.log 2>&1; echo "exit=$?"
tail -20 /tmp/m13-t3-red.log
node --test "tests/apply-template.test.ts" > /tmp/m13-t3-red2.log 2>&1; echo "exit=$?"
grep -E "fail|Error" /tmp/m13-t3-red2.log | head -8
```

Expected：第一条 `exit=1`（模块不存在/导出缺失）；第二条 `exit=1`（`got.parentId` 为 undefined）。

- [ ] **Step 3: 改类型与纯函数**

`src/types/project.ts` 改成（完整文件）：

```ts
// 与 Rust 侧 #[serde(rename_all = "camelCase")] 的字段一一对应；
// Option<String> 序列化为 null，所以可空字段写成 string | null 而不是可选属性。
export interface ProjectDir {
  id: string;
  kind: "root" | "entry";
  path: string;
  label: string | null;
  sort: number;
  exists: boolean;
}

export interface Project {
  id: string;
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
  parentId: string | null;
  tags: string[];
  dirs: ProjectDir[];
}

export interface ProjectInput {
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
  parentId: string | null;
  tags: string[];
}

export const EMPTY_INPUT: ProjectInput = {
  name: "",
  code: "",
  manager: "",
  customer: "",
  contactName: "",
  contactPhone: "",
  contractNo: "",
  contractPeriod: "",
  deliveryDeadline: "",
  status: "立项",
  parentId: null,
  tags: [],
};

export function toInput(p: Project): ProjectInput {
  return {
    name: p.name,
    code: p.code ?? "",
    manager: p.manager ?? "",
    customer: p.customer ?? "",
    contactName: p.contactName ?? "",
    contactPhone: p.contactPhone ?? "",
    contractNo: p.contractNo ?? "",
    contractPeriod: p.contractPeriod ?? "",
    deliveryDeadline: p.deliveryDeadline ?? "",
    status: p.status,
    parentId: p.parentId ?? null,
    tags: p.tags,
  };
}
```

新建 `src/lib/project-tree.ts`：

```ts
import type { Project } from "@/types/project";

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

function parentIdOf(p: Project): string | null {
  return p.parentId && p.parentId !== p.id ? p.parentId : null;
}

// 链进环 = 永远到不了顶。这类节点按根展示，既不递归挂死，也不丢节点。
function inCycle(start: Project, byId: Map<string, Project>): boolean {
  const seen = new Set<string>();
  let cur = start;
  for (;;) {
    const pid = parentIdOf(cur);
    if (!pid) return false;
    if (seen.has(cur.id)) return true;
    seen.add(cur.id);
    const parent = byId.get(pid);
    if (!parent) return false;
    cur = parent;
  }
}

export function buildProjectForest(projects: Project[]): ProjectTreeNode[] {
  const byId = new Map(projects.map((p) => [p.id, p]));
  const nodes = new Map<string, ProjectTreeNode>(
    projects.map((p): [string, ProjectTreeNode] => [
      p.id,
      { project: p, depth: 0, children: [] },
    ]),
  );
  const isRoot = new Map<string, boolean>(
    projects.map((p): [string, boolean] => {
      const pid = parentIdOf(p);
      return [p.id, !pid || !byId.has(pid) || inCycle(p, byId)];
    }),
  );
  const roots: ProjectTreeNode[] = [];

  for (const p of projects) {
    const node = nodes.get(p.id);
    if (!node) continue;
    const pid = parentIdOf(p);
    if (isRoot.get(p.id) || !pid) {
      roots.push(node);
      continue;
    }
    const parent = nodes.get(pid);
    if (!parent) {
      roots.push(node);
      continue;
    }
    parent.children.push(node);
  }

  const setDepth = (node: ProjectTreeNode, depth: number) => {
    node.depth = depth;
    for (const child of node.children) setDepth(child, depth + 1);
  };
  for (const root of roots) setDepth(root, 0);
  return roots;
}

export function flattenProjectTree(forest: ProjectTreeNode[]): ProjectTreeNode[] {
  const out: ProjectTreeNode[] = [];
  const walk = (nodes: ProjectTreeNode[]) => {
    for (const node of nodes) {
      out.push(node);
      walk(node.children);
    }
  };
  walk(forest);
  return out;
}

export function parentCandidates(projects: Project[], excludeId: string | null): ParentOption[] {
  const flat = flattenProjectTree(buildProjectForest(projects));
  const excluded = new Set<string>();
  if (excludeId) {
    const start = flat.find((n) => n.project.id === excludeId);
    if (start) {
      const stack = [start];
      while (stack.length > 0) {
        const node = stack.pop();
        if (!node) break;
        excluded.add(node.project.id);
        stack.push(...node.children);
      }
    } else {
      excluded.add(excludeId);
    }
  }
  return flat
    .filter((n) => !excluded.has(n.project.id))
    .map((n) => ({
      id: n.project.id,
      name: n.project.name,
      status: n.project.status,
      depth: n.depth,
    }));
}
```

`src/lib/apply-template.ts` 的返回对象在 `name: input.name,` 后加：

```ts
    parentId: input.parentId,
```

`src/components/template-manager.tsx` 两处：

```ts
    status: t.status,
    parentId: null,
    tags: t.tags,
```

```tsx
                <Button size="sm" onClick={() => setEditing({ id: null, input: { ...EMPTY_TEMPLATE_INPUT, parentId: null, tags: [] } })}>
```

同时把模板管理弹窗的说明改成下面这句（避免「基础信息」被理解成包含归属）：

```tsx
            <DialogDescription>
              模板只保存项目基础信息，新建项目时可以一键填充；不包含项目名、所属总包、台账或目录。
            </DialogDescription>
```

`src/pages/projects.tsx` 的 `emptySeed()` 返回对象在 `status: "立项",` 后加：

```ts
    parentId: null,
```

- [ ] **Step 4: 跑绿并提交**

```bash
cd "E:/zero/demo/2026/project-files-manage"
node --test "tests/**/*.test.ts" > /tmp/m13-t3-green.log 2>&1; echo "exit=$?"
grep -E "^ℹ (tests|pass|fail)" /tmp/m13-t3-green.log
npm run build > /tmp/m13-t3-web.log 2>&1; echo "web=$?"; grep -c "error TS" /tmp/m13-t3-web.log
git add src/types/project.ts src/lib/project-tree.ts src/lib/apply-template.ts src/components/template-manager.tsx src/pages/projects.tsx tests/project-tree.test.ts tests/apply-template.test.ts
git diff --cached --check
git commit -m "feat(m13): 前端 parentId 与项目树纯函数（森林/扁平/候选排除）"
```

Expected：node **66/66/0 / exit=0**；web `exit=0` 且 `error TS` 计数 0。

---

### Task 4: `ProjectForm` 父项下拉

**Files:**
- Modify: `src/components/project-form.tsx`

**Interfaces:**
- Consumes: Task 3 的 `ParentOption`、`ProjectInput.parentId`。
- Produces: `ProjectForm` 可选 prop `parentOptions?: ParentOption[]`；不传不渲染，传则渲染「所属总包」Select。

- [ ] **Step 1: 改表单**

`src/components/project-form.tsx` 完整改成：

```tsx
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { EMPTY_INPUT, type ProjectInput } from "@/types/project";
import type { ParentOption } from "@/lib/project-tree";

// 中英文逗号、顿号、空格都当分隔符：台账里的标签是人手打的，分隔符不会统一。
function parseTags(raw: string): string[] {
  return [...new Set(raw.split(/[,，、\s]+/).map((t) => t.trim()).filter(Boolean))];
}

const FIELDS: { key: keyof ProjectInput; label: string; placeholder?: string }[] = [
  { key: "code", label: "项目编号", placeholder: "P-2026-001" },
  { key: "manager", label: "项目经理" },
  { key: "customer", label: "客户名称" },
  { key: "contactName", label: "对接联系人" },
  { key: "contactPhone", label: "联系方式" },
  { key: "contractNo", label: "合同编号" },
  { key: "contractPeriod", label: "合同周期", placeholder: "2026-01 ~ 2026-06" },
  { key: "deliveryDeadline", label: "交付截止", placeholder: "2026-06-30" },
];

export function ProjectForm({
  initial,
  statuses,
  saving,
  onSubmit,
  onCancel,
  nameLabel = "项目名称",
  nameRequiredMessage = "项目名称不能为空",
  onChange,
  parentOptions,
}: {
  initial?: ProjectInput;
  statuses: string[];
  saving: boolean;
  onSubmit: (input: ProjectInput) => Promise<boolean>;
  onCancel: () => void;
  nameLabel?: string;
  nameRequiredMessage?: string;
  onChange?: (input: ProjectInput) => void;
  parentOptions?: ParentOption[];
}) {
  const [form, setForm] = useState<ProjectInput>(initial ?? EMPTY_INPUT);
  const [tagsText, setTagsText] = useState((initial ?? EMPTY_INPUT).tags.join("，"));
  const [nameError, setNameError] = useState<string | null>(null);

  const emit = (next: ProjectInput, tags: string) =>
    onChange?.({ ...next, tags: parseTags(tags) });
  const patch = (key: keyof ProjectInput, value: string) => {
    const next = { ...form, [key]: value };
    setForm(next);
    emit(next, tagsText);
  };
  const onTagsChange = (value: string) => {
    setTagsText(value);
    emit(form, value);
  };
  const setParent = (value: string) => {
    const parentId = value === "none" ? null : value;
    const next = { ...form, parentId };
    setForm(next);
    emit(next, tagsText);
  };

  const currentParentId = form.parentId;
  const parentValue =
    currentParentId !== null && parentOptions?.some((o) => o.id === currentParentId)
      ? currentParentId
      : "none";

  const submit = async () => {
    if (!form.name.trim()) {
      setNameError(nameRequiredMessage);
      return;
    }
    const parentId =
      parentOptions && form.parentId && !parentOptions.some((o) => o.id === form.parentId)
        ? null
        : form.parentId;
    const ok = await onSubmit({ ...form, parentId, tags: parseTags(tagsText) });
    if (!ok) setNameError("保存失败，详见下方错误提示");
  };

  return (
    <div className="grid gap-3">
      <div className="grid gap-2">
        <Label htmlFor="p-name">
          {nameLabel} <span className="text-destructive">*</span>
        </Label>
        <Input
          id="p-name"
          value={form.name}
          onChange={(e) => {
            patch("name", e.target.value);
            setNameError(null);
          }}
          placeholder="例：某单位运维平台二期"
        />
        {nameError && <p className="text-xs text-destructive">{nameError}</p>}
      </div>

      <div className="grid gap-2">
        <Label>项目状态</Label>
        <Select value={form.status} onValueChange={(v) => patch("status", String(v))}>
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {statuses.map((s) => (
              <SelectItem key={s} value={s}>
                {s}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {parentOptions && (
        <div className="grid gap-2">
          <Label>所属总包</Label>
          <Select
            value={parentValue}
            onValueChange={setParent}
            disabled={parentOptions.length === 0}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">无（顶层项目）</SelectItem>
              {parentOptions.map((o) => (
                <SelectItem key={o.id} value={o.id}>
                  {"— ".repeat(o.depth)}
                  {o.name}（{o.status}）
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      <div className="grid grid-cols-2 gap-3">
        {FIELDS.map((f) => (
          <div key={f.key} className="grid gap-1.5">
            <Label htmlFor={f.key} className="text-xs text-muted-foreground">
              {f.label}
            </Label>
            <Input
              id={f.key}
              value={(form[f.key] as string | null) ?? ""}
              placeholder={f.placeholder}
              onChange={(e) => patch(f.key, e.target.value)}
            />
          </div>
        ))}
      </div>

      <div className="grid gap-2">
        <Label htmlFor="p-tags" className="text-xs text-muted-foreground">
          自定义标签（逗号或空格分隔）
        </Label>
        <Input
          id="p-tags"
          value={tagsText}
          placeholder="政务，维保中"
          onChange={(e) => onTagsChange(e.target.value)}
        />
      </div>

      <div className="mt-2 flex justify-end gap-2">
        <Button variant="ghost" onClick={onCancel} disabled={saving}>
          取消
        </Button>
        <Button onClick={() => void submit()} disabled={saving}>
          {saving ? "保存中…" : "保存"}
        </Button>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: 构建并提交**

```bash
cd "E:/zero/demo/2026/project-files-manage"
npm run build > /tmp/m13-t4-web.log 2>&1; echo "web=$?"; grep -c "error TS" /tmp/m13-t4-web.log
git add src/components/project-form.tsx
git diff --cached --check
git commit -m "feat(m13): ProjectForm 可选所属总包下拉（缺省不渲染）"
```

Expected：web `exit=0`、`error TS` 0。没有自动化 UI 测试，父项下拉的显示与保存由 Task 7 真机格守。

---

### Task 5: 项目列表两视图 + 行组件抽取

**Files:**
- Create: `src/components/project-row.tsx`
- Modify: `src/pages/projects.tsx`

**Interfaces:**
- Consumes: Task 3 的 `buildProjectForest` / `flattenProjectTree` / `parentCandidates`、Task 4 的 `parentOptions`。
- Produces: 列表「全部 / 按总包」切换；两种视图共用 `ProjectRow`；编辑/新建弹窗的父项候选排除自身与后代。

- [ ] **Step 1: 新建行组件**

`src/components/project-row.tsx`：

```tsx
import { Link } from "@tanstack/react-router";
import { Copy, FolderOpen, Pencil, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { Project } from "@/types/project";

export function ProjectRow({
  project,
  depth,
  copiedId,
  onOpenRoot,
  onCopyRoot,
  onEdit,
  onDelete,
}: {
  project: Project;
  depth: number;
  copiedId: string | null;
  onOpenRoot: (p: Project) => void;
  onCopyRoot: (p: Project) => void;
  onEdit: (p: Project) => void;
  onDelete: (p: Project) => void;
}) {
  const root = project.dirs.find((d) => d.kind === "root");
  return (
    <li
      className="grid grid-cols-[minmax(0,1.4fr)_auto] items-center gap-3 rounded-xl border bg-background py-3 pr-4"
      style={{ paddingLeft: 16 + depth * 20 }}
    >
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <Link
            to="/projects/$projectId"
            params={{ projectId: project.id }}
            className="truncate text-sm font-medium hover:underline"
          >
            {project.name}
          </Link>
          <Badge variant="secondary">{project.status}</Badge>
          {project.tags.map((t) => (
            <Badge key={t} variant="outline">
              {t}
            </Badge>
          ))}
        </div>
        <div className="mt-1 flex items-center gap-2 text-xs text-muted-foreground">
          <span>{[project.code, project.customer, project.manager].filter(Boolean).join(" · ") || "—"}</span>
          {root && (
            <span
              className={`max-w-[46ch] truncate font-mono ${root.exists ? "" : "text-destructive"}`}
              title={root.exists ? root.path : `${root.path}（当前不可达）`}
            >
              {root.path}
            </span>
          )}
        </div>
      </div>

      <div className="flex items-center gap-1">
        <Button
          size="icon"
          variant="ghost"
          disabled={!root}
          title="打开主根目录"
          onClick={() => onOpenRoot(project)}
        >
          <FolderOpen />
        </Button>
        <Button
          size="icon"
          variant="ghost"
          disabled={!root}
          title={copiedId === project.id ? "已复制" : "复制路径"}
          onClick={() => onCopyRoot(project)}
        >
          <Copy />
        </Button>
        <Button size="icon" variant="ghost" title="编辑" onClick={() => onEdit(project)}>
          <Pencil />
        </Button>
        <Button size="icon" variant="ghost" title="删除" onClick={() => onDelete(project)}>
          <Trash2 />
        </Button>
      </div>
    </li>
  );
}
```

- [ ] **Step 2: 改项目页**

`src/pages/projects.tsx` 按下面逐块改。

顶部 import 改成：

```tsx
import { useEffect, useState } from "react";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ProjectForm } from "@/components/project-form";
import { ProjectRow } from "@/components/project-row";
import { TemplateManager } from "@/components/template-manager";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { applyTemplate } from "@/lib/apply-template";
import {
  buildProjectForest,
  flattenProjectTree,
  parentCandidates,
  type ProjectTreeNode,
} from "@/lib/project-tree";
import { copyText, openFolder } from "@/lib/api";
import { useProjectsStore } from "@/stores/projects";
import { useTemplatesStore } from "@/stores/templates";
import { toInput, type Project, type ProjectInput } from "@/types/project";
```

state 区在 `selectedTemplate` 后加：

```tsx
  const [view, setView] = useState<"all" | "tree">("all");
```

标题行右侧那组按钮之前加视图切换：

```tsx
        <div className="flex items-center gap-1 rounded-lg border bg-background p-0.5">
          <Button
            size="sm"
            variant={view === "all" ? "secondary" : "ghost"}
            onClick={() => setView("all")}
          >
            全部
          </Button>
          <Button
            size="sm"
            variant={view === "tree" ? "secondary" : "ghost"}
            onClick={() => setView("tree")}
          >
            按总包
          </Button>
        </div>
```

把原来的 `<ul className="grid gap-2">…</ul>`（`items.map` 那一整块）替换成：

```tsx
        view === "all" ? (
          <ul className="grid gap-2">
            {items.map((p) => (
              <ProjectRow
                key={p.id}
                project={p}
                depth={0}
                copiedId={copiedId}
                onOpenRoot={(x) => void openRoot(x)}
                onCopyRoot={(x) => void copyRoot(x)}
                onEdit={(x) => setEditing({ id: x.id, input: toInput(x) })}
                onDelete={(x) => setConfirming(x)}
              />
            ))}
          </ul>
        ) : (
          <div className="grid gap-3">
            {buildProjectForest(items).map((node) => (
              <TreeGroup
                key={node.project.id}
                node={node}
                copiedId={copiedId}
                onOpenRoot={(x) => void openRoot(x)}
                onCopyRoot={(x) => void copyRoot(x)}
                onEdit={(x) => setEditing({ id: x.id, input: toInput(x) })}
                onDelete={(x) => setConfirming(x)}
              />
            ))}
          </div>
        )
```

给 `ProjectForm` 加 `parentOptions`（新建与编辑共用）：

```tsx
              <ProjectForm
                key={formKey}
                initial={editing.input}
                statuses={statuses}
                saving={saving}
                parentOptions={parentCandidates(items, editing.id)}
                onChange={(input) => setEditing((prev) => (prev ? { ...prev, input } : prev))}
```

在 `emptySeed()` 之前加本地组件：

```tsx
function TreeGroup({
  node,
  copiedId,
  onOpenRoot,
  onCopyRoot,
  onEdit,
  onDelete,
}: {
  node: ProjectTreeNode;
  copiedId: string | null;
  onOpenRoot: (p: Project) => void;
  onCopyRoot: (p: Project) => void;
  onEdit: (p: Project) => void;
  onDelete: (p: Project) => void;
}) {
  const nodes = flattenProjectTree([node]);
  return (
    <section className="rounded-xl border bg-background p-3">
      <div className="mb-2 flex items-center gap-2 px-1 text-xs text-muted-foreground">
        <span>总包</span>
        <span>共 {nodes.length} 项</span>
      </div>
      <ul className="grid gap-2">
        {nodes.map((n) => (
          <ProjectRow
            key={n.project.id}
            project={n.project}
            depth={n.depth}
            copiedId={copiedId}
            onOpenRoot={onOpenRoot}
            onCopyRoot={onCopyRoot}
            onEdit={onEdit}
            onDelete={onDelete}
          />
        ))}
      </ul>
    </section>
  );
}
```

- [ ] **Step 3: 构建并提交**

```bash
cd "E:/zero/demo/2026/project-files-manage"
npm run build > /tmp/m13-t5-web.log 2>&1; echo "web=$?"; grep -c "error TS" /tmp/m13-t5-web.log
git add src/components/project-row.tsx src/pages/projects.tsx
git diff --cached --check
git commit -m "feat(m13): 项目列表全部/按总包两视图与父项候选接线"
```

Expected：web `exit=0`、`error TS` 0。树的递归缩进、组计数与候选排除由 Task 7 真机格守。

---

### Task 6: 详情页父子区 + 编辑候选

**Files:**
- Modify: `src/pages/project-detail.tsx`

**Interfaces:**
- Consumes: Task 3 的 `parentCandidates`、Task 4 的 `parentOptions`。
- Produces: 详情页「父子关系」区（所属总包链接 / 子项目直接子项列表）；编辑弹窗父项候选排除自身与后代。

- [ ] **Step 1: 加计算与区块**

在 `project-detail.tsx` 的 import 里加：

```tsx
import { parentCandidates } from "@/lib/project-tree";
```

在 `const entries = ...` 后加：

```tsx
  const parent =
    project.parentId && project.parentId !== project.id
      ? items.find((p) => p.id === project.parentId)
      : undefined;
  const children = items.filter((p) => p.parentId === project.id && p.id !== project.id);
```

在「基础信息」`</section>` 后、「本地目录映射」`<section>` 前插入：

```tsx
      <section className="mb-4 rounded-xl border bg-background p-4">
        <h2 className="mb-3 text-sm font-medium">父子关系</h2>
        <div className="mb-3 flex items-center gap-2 text-sm">
          <span className="text-xs text-muted-foreground">所属总包</span>
          {parent ? (
            <>
              <Link
                to="/projects/$projectId"
                params={{ projectId: parent.id }}
                className="truncate font-medium hover:underline"
              >
                {parent.name}
              </Link>
              <Badge variant="secondary">{parent.status}</Badge>
            </>
          ) : project.parentId ? (
            <span className="text-xs text-muted-foreground">所属总包已删除或不存在</span>
          ) : (
            <span className="text-xs text-muted-foreground">无（顶层项目）</span>
          )}
        </div>
        <div className="mb-2 flex items-center gap-2">
          <span className="text-xs text-muted-foreground">子项目</span>
          <span className="text-xs text-muted-foreground">共 {children.length} 项</span>
        </div>
        {children.length === 0 ? (
          <p className="text-xs text-muted-foreground">暂无子项目。</p>
        ) : (
          <ul className="grid gap-1.5">
            {children.map((c) => (
              <li key={c.id} className="flex items-center gap-2 rounded-lg border px-3 py-2">
                <Link
                  to="/projects/$projectId"
                  params={{ projectId: c.id }}
                  className="min-w-0 truncate text-sm font-medium hover:underline"
                >
                  {c.name}
                </Link>
                <Badge variant="secondary">{c.status}</Badge>
                <span className="truncate text-xs text-muted-foreground">{c.code || "—"}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
```

给编辑弹窗的 `ProjectForm` 加：

```tsx
          <ProjectForm
            initial={toInput(project)}
            statuses={statuses}
            saving={saving}
            parentOptions={parentCandidates(items, project.id)}
```

- [ ] **Step 2: 构建并提交**

```bash
cd "E:/zero/demo/2026/project-files-manage"
npm run build > /tmp/m13-t6-web.log 2>&1; echo "web=$?"; grep -c "error TS" /tmp/m13-t6-web.log
git add src/pages/project-detail.tsx
git diff --cached --check
git commit -m "feat(m13): 项目详情父子关系区与编辑父项候选"
```

Expected：web `exit=0`、`error TS` 0。

---

### Task 7: 门禁 + 十处变异 + 真机 12 格 + 映射（控制方自跑）

**Files:**
- Create: `.superpowers/sdd/2026-10-22-m13-project-hierarchy/progress.md`
- Create: `.superpowers/sdd/2026-10-22-m13-project-hierarchy/mutation-evidence.txt`
- Create: `.superpowers/sdd/2026-10-22-m13-project-hierarchy/real-machine-readings.md`
- Create: `.superpowers/sdd/2026-10-22-m13-project-hierarchy/spec-plan-mapping.md`
- Create: `.superpowers/sdd/2026-10-22-m13-project-hierarchy/probe/`（CDP 脚本与读数）

- [ ] **Step 1: 全量门禁（退出码逐个真取）**

```bash
cd "E:/zero/demo/2026/project-files-manage/src-tauri"
cargo test -j 1 --lib > /tmp/m13-gate-cargo.log 2>&1; echo "cargo=$?"
grep "test result" /tmp/m13-gate-cargo.log
cargo clippy -j 1 --lib -- -D warnings > /tmp/m13-gate-cl1.log 2>&1; echo "clippy-lib=$?"
cargo clippy -j 1 --all-targets --all-features -- -D warnings > /tmp/m13-gate-cl2.log 2>&1; echo "clippy-all=$?"
cargo build -j 1 --lib > /tmp/m13-gate-build.log 2>&1; echo "rust-build=$?"
cd "E:/zero/demo/2026/project-files-manage"
node --test "tests/**/*.test.ts" > /tmp/m13-gate-node.log 2>&1; echo "node=$?"
grep -E "^ℹ (tests|pass|fail)" /tmp/m13-gate-node.log
npm run build > /tmp/m13-gate-web.log 2>&1; echo "web=$?"; grep -c "error TS" /tmp/m13-gate-web.log
echo "cmd=$(sed -n '/generate_handler/,/^\s*\])/p' src-tauri/src/lib.rs | grep -cE '^ +[a-z_]+,?$') async=$(grep -c 'tauri::command(async)' src-tauri/src/lib.rs) dead=$(grep -rn 'allow(dead_code)' src-tauri/src/ | wc -l) pkg=$(grep -c '^\[\[package\]\]' src-tauri/Cargo.lock) collect=$(grep -c 'hits.append(&mut collect(' src-tauri/src/search.rs)"
grep -oE 'TARGET_VERSION: [a-z0-9]+ = [0-9]+' src-tauri/src/db.rs
git diff --stat master..HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock package.json
git rev-list --merges --count --all
git status --porcelain
```

Expected：cargo **358**；node **66**；两条 clippy 0；build 0；web 0 错；`cmd=69 async=7 dead=1 pkg=616 collect=9`；`TARGET_VERSION: i64 = 9`；依赖 diff 空；merge 0；状态只剩 `?? Python/` 与 git-ignored 的证据目录。

- [ ] **Step 2: 十处变异（每处备份 → 打红 → `cmp` 还原）**

| # | 变异 | 期望红在哪 |
|---|---|---|
| M① | `create_project` 的 INSERT 去掉 `parent_id` 列与参数 | `create_with_parent_roundtrips_in_get_and_list` |
| M② | `list_projects` / `get_project` 不读 `parent_id` | 同上（get/list 两断言） |
| M③ | 删 `validate_parent` 的存在性检查 | `missing_or_deleted_parent_is_rejected` |
| M④ | 删 `validate_parent` 里 `self_id == Some(pid)` 与祖先链 `cur == self_id` 两处 | `self_parent_is_rejected` / `descendant_parent_is_rejected_as_a_cycle` |
| M⑤ | 删 `delete_project` 的子项计数守卫 | `delete_is_blocked_while_live_children_exist` |
| M⑥ | 子项计数去掉 `deleted_at IS NULL` | `a_soft_deleted_child_does_not_block_the_parent` |
| M⑦ | V9 改成空 DDL | `v9_projects_parent_id_and_index_exist` |
| M⑧ | `buildProjectForest` 的 `inCycle` 恒 false（环内互挂，根为空） | node 双节点环测试（根集合断言红） |
| M⑨ | `parentCandidates` 不排除后代 | node「编辑时排除自己与全部后代」 |
| M⑩ | `applyTemplate` 去掉 `parentId` | node「套用模板保留当前 parentId」 |

每处流程：

```bash
cd "E:/zero/demo/2026/project-files-manage"
cp <被改文件> /tmp/m13-mut-backup
# 改一处
<定向测试> > /tmp/m13-mut-N.log 2>&1; echo "exit=$?"   # 期望 cargo 101 / node 1
cp /tmp/m13-mut-backup <被改文件>
cmp <被改文件> /tmp/m13-mut-backup; echo "cmp=$?"        # 期望 0
```

把每处的变异描述、测试命令、真退出码、关键失败行、`cmp=0` 写入 `mutation-evidence.txt`。全部还原后复跑 cargo 358 / node 66。

- [ ] **Step 3: 真机网格（§八 12 格）**

沙盒 identifier：`dev.zero.pfm.m13test`（一次性，跑完删净）；WebView2 CDP 端口 **9333**。真实数据目录 `%APPDATA%\dev.zero.pfm\` 三文件 `size+mtime+sha256` 开工/收工逐字一致。子代理不跑 `tauri dev`，本步由控制方执行。

覆盖与判据：

1. **新建总包**：不选父保存 ⇒ 「全部」出现；详情显示「无（顶层项目）」。
2. **新建子项目**：在新建项目表单选总包保存 ⇒ 子项目详情「所属总包」链接到总包；总包详情「子项目」出现它。
3. **孙项目/多层**：以子项目为父再建一项 ⇒「按总包」视图三级缩进，根组「共 3 项」。
4. **候选排除**：编辑总包/子项目时下拉不含自己与全部后代；含父/兄弟/祖先。
5. **归档父项**：总包置「归档」后，新建子项目下拉仍能选中它。
6. **禁止成环**：直接 IPC 把 A 挂到后代 B ⇒ `invalid_input`；UI 构造不出该组合（cargo 已守，真机只记录 UI 无入口）。
7. **删除守卫**：删除有子项的总包 ⇒ `invalid_input` 文案含子项数量，项目仍在。
8. **解除后可删**：先删子项目，再删总包 ⇒ 成功；两视图都不再出现。
9. **列表两视图**：「全部」顺序与改动前一致；「按总包」分组、缩进、根组计数正确。
10. **详情父子区**：子项目详情有父链接；父详情子项列表正确；无子项显示空态。
11. **模板边界**：模板弹窗没有「所属总包」；新建项目先选父再套模板，父项目保留。
12. **备份与检索**：`backup_export` 后包内 `projects.parent_id` 保留、`schemaVersion=9`、`Counts` 7 键；`search_all` 不因父子关系增删命中（collect 9）。

探针用真 DOM 点击：页头「新建项目」、表单「所属总包」Select（Radix 需 pointerdown）、「保存」、视图切换「全部 / 按总包」、详情「编辑」、删除确认。只回布尔/条数/字段名/错误码名，不打印项目正文或路径全文。

- [ ] **Step 4: 写映射与账本**

`spec-plan-mapping.md`：spec §二 D1–D10、§六 10 条不变量、§八 12 格 ↔ 本计划 Task 1–8 与真机读数；任何未跑格明写「未验 + 为什么」。`progress.md` 记 Task 1–6 的提交、评审、修复轮与裁定。证据目录 git-ignored，被活文档按路径引用后不许清。

---

### Task 8: 活文档回写

**Files:**
- Modify: `docs/技术方案.md`
- Modify: `docs/开发进度.md`
- Modify: `docs/HANDOFF.md`
- Modify: 项目记忆 `project-m13-project-hierarchy-design-decisions.md`

- [ ] **Step 1: 技术方案两处**

`docs/技术方案.md:116` 的 `projects` 行末尾补：

```text
、parent_id（自引用，M13 新增）
```

在 `docs/技术方案.md` §八里程碑表 M12 行后追加 M13 行：

```markdown
| M13 | 父子项目关联（单父树）：V9 `projects.parent_id` + 索引（零回填）+ 项目表单「所属总包」下拉（编辑排除自身与全部后代、归档父项可选）+ 列表「全部 / 按总包」两视图（根分组、任意层递归缩进）+ 详情父子区 + 有未删除子项时禁止删除 | ✅ 覆盖「总包下面挂子项目」 | 代码 + 十处变异 + 真机 12 格取证；`TARGET_VERSION` 8→9；命令仍 69、collect 仍 9、`Counts` 仍 7 键；终态读数见 `docs/开发进度.md`「## M13 验收证据」 |
```

- [ ] **Step 2: 开发进度三处**

里程碑表 M13 行（照 M12 行形制）：

```markdown
| M13 | 父子项目关联（单父树）：V9 `projects.parent_id`（零回填）+ 表单「所属总包」下拉（候选排除自己与后代、归档父项可选）+ 列表「全部 / 按总包」递归缩进 + 详情父子区 + 有子禁删（`invalid_input` 点名数量） | 代码 + 十处变异 + 真机 12 格取证（2026-10-22）；**待整分支终审 → 合并** | 见下节「## M13 验收证据」：cargo **358 passed** / node **66/66/0** / 结构门 **69·7·9·1·616·9** / 依赖 diff **空** / merge **0** |
```

新增验收节（放在 M12 节后）：

```markdown
## M13 验收证据（父子项目关联，待整分支终审/合并；cargo 358 / node 66）

M13 = `docs/思维导图.md` §四.3「父子项目关联」。口径权威：`docs/superpowers/specs/2026-10-22-m13-project-hierarchy-design.md`（D1–D10）；计划：`docs/superpowers/plans/2026-10-22-m13-project-hierarchy.md`（8 格）。

交付：V9 `projects.parent_id` + `idx_projects_parent_id`（零回填、`TARGET_VERSION 9`）；`project_create/update/list/get` 契约内扩 `parentId`，父项存在性/自指/成环校验与有子禁删全在命令层（命令仍 69，无新 IPC）；前端 `project-tree.ts` 森林/扁平/候选排除纯函数、`ProjectForm` 可选父项下拉、列表「全部 / 按总包」两视图、详情父子区；模板不存归属且 `applyTemplate` 保留 `parentId`。

门禁终态（Task 7 现跑）：cargo **358 passed / exit=0**、node **66/66/0**、两条 clippy 0/0、build 0、web 0 错；结构门 **69 / 7 / `TARGET_VERSION: i64 = 9` / 豁免 1 / `Cargo.lock` 616 / collect 9**；依赖 diff 空、merge 0。十处变异全部具名打红并 `cmp=identical` 还原；真机 1–12 格读数见同目录证据。真实数据目录三文件开工/收工逐字一致。

未验/限制：页面仍无自动化测试网（jsdom 不引）；树视图的极深层级观感、几十项目时的渲染性能未量；异常备份包造成的缺失父项/环只有 cargo/node 防御性测试，没有真机异常库。
```

变更日志加一条（照现有日期条）：

```markdown
- 2026-10-22：**M13 父子项目关联八格全完**（分支 `feat/m13-project-hierarchy`，待整分支终审/合并）。交付 V9 `parent_id`、后端父项校验/删除守卫、前端树纯函数与两视图/详情父子区；命令仍 69。门禁 cargo **358** / node **66** / 结构门 **69·7·9·1·616·9**；十处变异全红并还原；真机 1–12 格。证据：`.superpowers/sdd/2026-10-22-m13-project-hierarchy/{progress.md,real-machine-readings.md,mutation-evidence.txt,spec-plan-mapping.md,probe/}`。
```

- [ ] **Step 3: HANDOFF 五处**

- 快照头：改成「M13 代码 + 取证完成，待整分支终审/合并；M12 及以前已合入 master」。
- §0：加 M13 bullet（V9、父项校验、有子禁删、两视图、命令 69、collect 9、`Counts` 7、`schemaVersion 9`）。
- §3：把「M12 收口点」换成「M13 集成点」：Task 1–8 状态、终审结论待填、下一步「整分支终审 → 合并批准」。
- §4-A：期望串改成 `命令 69 / async 7 / TARGET_VERSION 9 / 豁免 1 / Cargo.lock 616 / collect 9 / cargo 358 / node 66`；命令注释里的 `TARGET_VERSION` 读数改成 9。
- §4-B：剩余候选改成 **操作日志 / 多工作区**；把 M13 的「已勘察事实」压成三行（单父树、归档父项可选、有子禁删）。
- §6：补 M13 spec/plan/证据目录指针；坑区补一条「新增必填字段会同时打穿 4 处 Rust `ProjectInput` 夹具与 1 处前端 `emptySeed`，grep 全量构造点再动手」。

- [ ] **Step 4: 记忆与文档门**

更新 `C:\Users\zero\.qoder\projects\E--zero-demo-2026-project-files-manage\memory\project-m13-project-hierarchy-design-decisions.md`：把「spec 待审、代码未开」改成「代码+取证完成、待终审/合并」，补 cargo 358 / node 66 / 命令 69 / TARGET 9 / 十处变异 / 真机 12 格。合并批准后再补合并点与 master 复跑读数。

```bash
cd "E:/zero/demo/2026/project-files-manage"
grep -c "M13" docs/技术方案.md docs/开发进度.md docs/HANDOFF.md
grep -rn "待合并\|合并点 = " docs/HANDOFF.md | wc -l
git status --porcelain
git diff --check
git add docs/技术方案.md docs/开发进度.md docs/HANDOFF.md
git commit -m "docs(m13): 活文档回写 —— 技术方案/开发进度/HANDOFF（代码+取证全完、待终审）"
```

Expected：三份文档 M13 计数均 ≥1；HANDOFF 无「待合并/合并点 = 」的占位句；`git diff --check` 0；只多本任务改的文档与记忆。

**合并后的收口**（不在本任务的提交里，需需求方当次批准后另做）：`git merge --ff-only` 合入 `master` 后，在 `master` 树上复跑 §Task 7 Step 1 整套门；再用一个 docs 提交把三份活文档与 M13 记忆里的「待终审/待合并」改成实际合并点与 `master` 复跑读数。**不要回头改 spec/plan 正文**。

---

## 自审记录（控制方写完后现跑现数）

- **Spec 覆盖**：D1→Task 1/2（单父、V9）；D2→Task 4/5/6（表单入口）；D3→Task 4/5/6（候选排除、归档可选）；D4→Task 2（删除守卫）；D5→Task 3/5（森林、两视图）；D6→Task 6（详情父子区）；D7→Task 2（校验）；D8→Task 1/2（命令 69、契约扩展）；D9→Task 1/2（备份 9、collect 9、Counts 7）；D10→Task 3（模板边界）。
- **类型一致性**：Rust `Project.parent_id` / `ProjectInput.parent_id` ↔ TS `parentId`；`ParentOption` 由 `project-tree.ts` 定义、`ProjectForm` 只做 type import；`buildProjectForest` 返回 `ProjectTreeNode[]`，Task 5 的 `TreeGroup` 直接用；`parentCandidates` 三处调用（列表新建/编辑、详情编辑）签名一致。
- **数链**：基线 346/57；Task 1 +2=348；Task 2 +10=358；Task 3 node +9=66；结构门 TARGET 8→9、命令仍 69、collect 9、死代码 1、lock 616。
- **预检结果**：V9 SQL 与树算法读数已回填「派发前预检」；首版树算法的根判定缺陷已在本文件修正，不要把 `inCycle` 改回「链能到顶」。
- **风险点**：`ProjectInput` 新增必填字段会打穿 4 处 Rust 夹具 + 1 处前端 `emptySeed`（已逐处列在 Task 2/3）；`ProjectForm` 的 `parentOptions` 必须保持可选，模板弹窗不传；`Select` 的失效 `parentId` 在提交时归一化为 null。
