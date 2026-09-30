// 与 Rust 侧 #[serde(rename_all = "camelCase")] 的字段一一对应。
// 敏感列在读取时只回「有没有值」（usernameSet 等），明文一律走 ledger_reveal 单字段解密，
// 所以这里的类型里根本不存在密码字段 —— 前端拿不到，也就不会缓存到 zustand 里。

export type EnvKind = "dev" | "test" | "pre" | "prod";
export type LinkKind = "jira" | "zentao" | "wiki" | "doc" | "repo" | "other";

export interface LedgerEnv {
  id: string;
  env: EnvKind;
  name: string;
  url: string | null;
  port: string | null;
  note: string | null;
}

export interface Credential {
  id: string;
  envId: string | null;
  title: string;
  url: string | null;
  note: string | null;
  usernameSet: boolean;
  passwordSet: boolean;
}

export interface Server {
  id: string;
  name: string;
  ip: string | null;
  bastion: string | null;
  note: string | null;
  accountSet: boolean;
  passwordSet: boolean;
}

export interface Link {
  id: string;
  kind: LinkKind;
  title: string;
  url: string;
  note: string | null;
}

export interface Note {
  id: string;
  title: string;
  body: string;
  tag: string | null;
}

export interface Ledger {
  envs: LedgerEnv[];
  credentials: Credential[];
  servers: Server[];
  links: Link[];
  notes: Note[];
}

export const emptyLedger = (): Ledger => ({
  envs: [],
  credentials: [],
  servers: [],
  links: [],
  notes: [],
});

export interface EnvInput {
  env: EnvKind;
  name: string;
  url: string | null;
  port: string | null;
  note: string | null;
}

export interface CredentialInput {
  envId: string | null;
  title: string;
  username: string | null;
  password: string | null;
  url: string | null;
  note: string | null;
}

export interface ServerInput {
  name: string;
  ip: string | null;
  bastion: string | null;
  account: string | null;
  password: string | null;
  note: string | null;
}

export interface LinkInput {
  kind: LinkKind;
  title: string;
  url: string;
  note: string | null;
}

export interface NoteInput {
  title: string;
  body: string | null;
  tag: string | null;
}

// reveal 的可解密坐标：与 Rust 的 SECRET_FIELDS 白名单一致，写错后端直接拒绝。
export type SecretKind = "credential" | "server";
export type SecretField = "username" | "password" | "account";

export const ENV_LABELS: Record<EnvKind, string> = {
  dev: "开发",
  test: "测试",
  pre: "预发",
  prod: "生产",
};

export const LINK_LABELS: Record<LinkKind, string> = {
  jira: "Jira",
  zentao: "禅道",
  wiki: "Wiki",
  doc: "文档",
  repo: "代码库",
  other: "其他",
};

// 表单里的空串一律按「没有内容」处理，避免把 "  " 存进库。
export const blank = (v: string): string | null => (v.trim() === "" ? null : v.trim());
