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

/** 表单里空串要折成 null，否则后端把 "" 当非法形状拒掉。 */
export const dueOrNull = (raw: string): string | null => (raw.trim() === "" ? null : raw.trim());
