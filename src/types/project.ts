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
  tags: string[];
}

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
    tags: p.tags,
  };
}
