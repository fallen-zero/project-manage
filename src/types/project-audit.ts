export type AuditAction = "create" | "update" | "delete";

export interface AuditChange {
  field: string;
  label: string;
  oldValue: string | null;
  newValue: string | null;
}

export interface AuditEntry {
  id: string;
  projectId: string;
  projectName: string;
  action: AuditAction;
  changes: AuditChange[];
  createdAt: string;
}

export interface AuditProjectOption {
  projectId: string;
  projectName: string;
}

export interface AuditPage {
  items: AuditEntry[];
  hasMore: boolean;
  projects: AuditProjectOption[];
}
