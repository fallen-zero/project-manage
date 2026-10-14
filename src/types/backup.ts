// 与 src-tauri/src/backup.rs 的出参逐字段对齐（camelCase 来自各结构体上的
// #[serde(rename_all = "camelCase")]）。漏一个键就是运行时 undefined（spec §十一.6）。
export interface BackupCounts {
  projects: number;
  ledgerEnvs: number;
  ledgerCredentials: number;
  ledgerServers: number;
  ledgerLinks: number;
  ledgerNotes: number;
  settings: number;
}

export interface BackupSummary {
  path: string;
  entries: string[];
  bytes: number;
  counts: BackupCounts;
  schemaVersion: number;
}

export interface PackageInfo {
  path: string;
  createdAt: string;
  appVersion: string;
  schemaVersion: number;
  counts: BackupCounts;
  bytes: number;
  plaintextWarning: string;
}

export interface ImportOutcome {
  entries: number;
  schemaVersion: number;
  preRestorePath: string;
  counts: BackupCounts;
}
