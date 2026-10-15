// 与 src-tauri/src/todo.rs 的出参逐字段对齐（camelCase 来自 #[serde(rename_all = "camelCase")]）。
// 漏一个键就是运行时 undefined（同 src/types/backup.ts 开头那句注释的告诫）。
export interface Todo {
  id: string;
  projectId: string;
  title: string;
  dueDate: string | null;
  done: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface TodoInput {
  projectId: string;
  title: string;
  dueDate: string | null;
}

/// 与 Rust 的 TodoEdit 对齐：没有 projectId（改归属不在本轮，spec §十一）。
export interface TodoEdit {
  title: string;
  dueDate: string | null;
  done: boolean;
}

/** 表单里的空串在这里折成 null。注意后端 `norm_date` 是「先 trim、空白折成 NULL」而不是拒收空串；
 *  显式折一次是为了让发出的值与线格式（`string | null`）一致，不把 `""` 送进请求。 */
export const dueOrNull = (raw: string): string | null => (raw.trim() === "" ? null : raw.trim());
