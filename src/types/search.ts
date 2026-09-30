// 与 Rust 侧 search::FieldHit 一一对应（serde 已转 camelCase）。
// source 只用来分组，不参与任何权限判断。
export type HitSource = "project" | "env" | "credential" | "server" | "link" | "note";

export interface FieldHit {
  source: HitSource;
  id: string;
  projectId: string;
  projectName: string;
  title: string;
  /** 由明文列拼出的一行副标题；敏感列的值不会出现在这里。 */
  detail: string;
}

export const SOURCE_LABELS: Record<HitSource, string> = {
  project: "项目档案",
  env: "环境地址",
  credential: "账号凭据",
  server: "服务器",
  link: "外部链接",
  note: "备注台账",
};

// 分组顺序固定，M4 要把文件正文结果插进来时改这里即可。
export const SOURCE_ORDER: HitSource[] = ["project", "env", "credential", "server", "link", "note"];
