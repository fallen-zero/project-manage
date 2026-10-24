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
  /** V11 起 NOT NULL，读出来恒有值。 */
  workspaceId: string;
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
  /** 缺省（`undefined`/`null`）= **不改归属**；`create` 一律忽略这一格（后端恒落活动区，
   *  spec §六 第 1 条）。写成可选属性而不是 `string | null`，是为了不破坏
   *  `apply-template.ts` / `template-manager.tsx:31` 那几处已有的 `ProjectInput` 字面量。 */
  workspaceId?: string | null;
  tags: string[];
}

// 新建表单的初值不含 workspaceId：create 恒落活动区（M15 spec §六），传了也会被后端忽略。
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
    workspaceId: p.workspaceId,
    tags: p.tags,
  };
}
