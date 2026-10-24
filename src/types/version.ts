// 项目版本台账（M16-a）。线格式与 src-tauri/src/version.rs 的 serde camelCase 逐字对齐。
export interface Version {
  id: string;
  projectId: string;
  version: string;
  releasedAt: string;
  content: string;
  createdAt: string;
  updatedAt: string;
}

export interface VersionInput {
  projectId: string;
  version: string;
  releasedAt: string | null;
  content: string | null;
}

export interface VersionUpdate {
  version: string;
  releasedAt: string | null;
  content: string | null;
}
