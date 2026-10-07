// 与 Task 10 的 5 个 IPC 命令 + `index://progress` 事件载荷一一对应：
// Rust 侧全部 `#[serde(rename_all = "camelCase")]`，所以这里的字段名就是序列化名。
// 例外是 `src/lib/ipc.ts` 的 DbStatus（`lib.rs:66-73` 没有 rename_all，仍是 snake_case），两者并存是现状。

export interface ProjectState {
  projectId: string;
  projectName: string;
  rootPath: string;
  rootExists: boolean;
  ok: number;
  skipped: number;
  failed: number;
  pending: number;
  missing: number;
  total: number;
}

export interface ProjectResult {
  projectId: string;
  projectName: string;
  scannedTotal: number;
  ok: number;
  skipped: number;
  failed: number;
  unchanged: number;
  walkErrors: number;
  capped: boolean;
  rootMissing: boolean;
}

export interface RunSummary {
  state: "done" | "cancelled" | "error";
  results: ProjectResult[];
  startedAt: string;
  finishedAt: string;
}

export interface IndexProgress {
  state: "running" | "done" | "cancelled" | "error";
  projectId: string;
  projectName: string;
  total: number;
  done: number;
  ok: number;
  skipped: number;
  failed: number;
  current: string;
  /** 只有 `state === "error"` 时有值（`[code] message（hint）` 整串），其余状态是 null。
   *  Rust 侧 `Progress.error: Option<String>` 没有 `skip_serializing_if`，所以这一格**永远在载荷里**，
   *  TS 类型必须写 `string | null` 而不是可选 —— 漏了它，`tsc` 这一步照样绿（前端不读就不报错），
   *  而界面上「这一轮挂了」和「为什么挂」就接不上。 */
  error: string | null;
}

export interface IndexOverview {
  running: boolean;
  fts5Available: boolean;
  lastRun: RunSummary | null;
  /** 一轮彻底失败的原因串（同上：Rust 侧 `Option<String>`，载荷里恒有这一格，写成 `string | null`）。
   *  与 `lastRun` 是互补的：成功轮 `lastRun` 有值、`lastError` 是 null；彻底失败的轮 `lastRun` 被置回
   *  null 而 `lastError` 有值（`start` 在成功收口时把 `last_error` 清掉，所以不必自己判陈旧）。 */
  lastError: string | null;
  projects: ProjectState[];
  excludeDirs: string[];
  maxFileBytes: number;
  maxFilesPerProject: number;
  supportedExts: string[];
}

export interface DocRow {
  id: string;
  path: string;
  ext: string;
  size: number;
  status: string;
  skipReason: string | null;
  errorMsg: string | null;
  indexedAt: string | null;
}

export interface DocHit {
  docId: string;
  projectId: string;
  projectName: string;
  path: string;
  snippet: string;
  matchedBy: "exact" | "prefix";
  score: number;
}
