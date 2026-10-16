// 与 src-tauri/src/daily_report.rs 的出参逐字段对齐（camelCase 来自 #[serde(rename_all = "camelCase")]）。
// 七键由 daily_report.rs::daily_report_line_format_has_exactly_seven_keys 钉住；
// xlsx 表头五列由 daily_report.rs::EXPORT_HEADERS 钉住，两处不一致时以 Rust 侧为真值。
export interface DailyReport {
  id: string;
  projectId: string;
  reportDate: string;
  content: string;
  hours: number;
  createdAt: string;
  updatedAt: string;
}

/// `reportDate` 传 `null` 是合法的请求形状 —— 后端 `req_date` 会把它连同空白一起
/// 当成「你没填日期」响亮拒掉（错误文本点名「日报日期」）。前端不做二次校验来「省一次往返」，
/// 因为判据只有一处（后端），界面自己先拦会漂成两套口径。
export interface DailyReportInput {
  projectId: string;
  reportDate: string | null;
  content: string;
  hours: number;
}

/// 与 Rust 的 DailyReportEdit 对齐：**含 projectId**（改归属在本轮，spec §五.4，与 TodoEdit 不同）。
export interface DailyReportEdit {
  projectId: string;
  reportDate: string | null;
  content: string;
  hours: number;
}

/// 与 Rust 侧 `daily_report::MAX_HOURS_PER_ROW` 同值；两格各钉各的断言，漂了就红。
export const MAX_HOURS_PER_ROW = 24;

/// 表单里的空串折成 null（同 `todo.ts` 的 `dueOrNull` 形制）。
export const dateOrNull = (raw: string): string | null => (raw.trim() === "" ? null : raw.trim());

/// 与 Rust 的 ExportRow 对齐。`projectName` 必须是 `Project.name` 原值 ——
/// 它是导入侧名称匹配的对象，写成 `projectLabel`（拼了编号）会导出去再导回来对不上。
export interface DailyReportExportRow {
  reportDate: string;
  projectCode: string | null;
  projectName: string;
  content: string;
  hours: number;
}

export interface ImportReject {
  /** Excel 里的绝对行号（表头是第 1 行）。 */
  line: number;
  field: string;
  reason: string;
}

/// 与 Rust 的 ImportPreview 对齐（六键，`import_preview_line_format_has_exactly_six_keys` 钉着）。
export interface ImportPreview {
  total: number;
  ok: number;
  duplicates: number;
  duplicateLines: number[];
  truncatedLines: number[];
  rejected: ImportReject[];
}

export interface ImportOutcome {
  inserted: number;
}
