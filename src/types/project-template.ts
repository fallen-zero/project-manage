// 与 Rust 的 #[serde(rename_all = "camelCase")] 一一对应；name 是模板名，不是项目名。
export interface ProjectTemplate {
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
}

export interface TemplateInput {
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

export const EMPTY_TEMPLATE_INPUT: TemplateInput = {
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
