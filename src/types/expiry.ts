export interface Expiry {
  id: string;
  projectId: string;
  kind: string;
  label: string;
  expireDate: string;
  note: string;
  createdAt: string;
  updatedAt: string;
}

// expireDate 传 string | null（后端 Option<String>）：空白与缺键都要走后端的 invalid_input，
// 不许在前端偷偷折成 undefined —— 缺键会先炸在 Tauri 参数层，回一句没有 code 的裸串。
export interface ExpiryInput {
  projectId: string;
  kind: string;
  label: string;
  expireDate: string | null;
  note: string | null;
}

// 只有这两件有界面消费者（spec §二 控制方自拍① / §七）：种类与名称录错走删了重建，
// 所以这里**故意不含** kind / label / projectId。别「顺手」补上凑齐字段 —— M9 终审 I4 就是那一格。
export interface ExpiryUpdate {
  expireDate: string | null;
  note: string | null;
}

// 需求原文 `docs/思维导图.md:73` 是前三类；「其他」是 spec D7 当次加的一格
// （ICP 备案、等保测评、软件订阅、服务器续费这些没处放，硬塞进「证书」就丢了语义）。
// lib.rs 的 kind_words_are_identical_in_rust_and_the_frontend_type 钉这两份与 V7 的 CHECK 逐字一致。
export const EXPIRY_KINDS = ["维保", "域名", "证书", "其他"] as const;
