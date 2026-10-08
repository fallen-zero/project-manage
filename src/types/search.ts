import type { DocHit } from "@/types/index";

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

// —— M4 统一检索的线格式，与 Rust 侧 search.rs / doc_preview.rs 逐字段对应（serde camelCase）。

export interface Cluster<T> {
  projectId: string;
  projectName: string;
  items: T[];
  /** 簇内被 `MAX_ITEMS_PER_CLUSTER` 截掉的条数：只显示不展开。 */
  hidden: number;
}

export interface ClusterSection<T> {
  clusters: T[];
  /** 整簇被段级上限扔掉的簇数。 */
  hiddenClusters: number;
}

export interface SearchBundle {
  query: string;
  projects: FieldHit[];
  projectsHidden: number;
  ledger: ClusterSection<Cluster<FieldHit>>;
  docs: ClusterSection<Cluster<DocHit>>;
  /** 正文段存在 matchedBy === "prefix"：段级说明只出一行（D7）。 */
  relaxed: boolean;
  /** 有 ok 正文行的项目数：空态文案靠它区分「还没建索引」与「没命中」。 */
  indexedProjects: number;
}

export interface DocPreview {
  docId: string;
  path: string;
  projectName: string;
  /** 窗口拼接后的原文，不是整篇；窗之间以 `⋯` 行分隔。 */
  text: string;
  /** UTF-16 码元区间，后端算好，`text.slice()` 直接可用。 */
  ranges: [number, number][];
  truncated: boolean;
}
