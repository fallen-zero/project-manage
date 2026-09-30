import { invoke } from "@tauri-apps/api/core";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { Project, ProjectInput } from "@/types/project";
import type { FieldHit } from "@/types/search";
import type {
  CredentialInput,
  EnvInput,
  Ledger,
  LinkInput,
  NoteInput,
  SecretField,
  SecretKind,
  ServerInput,
} from "@/types/ledger";

export const listProjects = () => invoke<Project[]>("project_list");
export const getProject = (id: string) => invoke<Project>("project_get", { id });
export const createProject = (input: ProjectInput) =>
  invoke<Project>("project_create", { input });
export const updateProject = (id: string, input: ProjectInput) =>
  invoke<Project>("project_update", { id, input });
export const deleteProject = (id: string) => invoke<void>("project_delete", { id });
export const setRootDir = (id: string, path: string, label?: string) =>
  invoke<Project>("project_set_root", { id, input: { path, label } });
export const addEntryDir = (id: string, path: string, label?: string) =>
  invoke<Project>("project_add_entry", { id, input: { path, label } });
export const removeDir = (id: string, dirId: string) =>
  invoke<void>("project_remove_dir", { id, dirId });
export const statusOptions = () => invoke<string[]>("project_status_options");

// 库内字段检索：只扫明文列，未解锁也能用（SQL 里压根没有密文列）。
export const searchLocal = (query: string) => invoke<FieldHit[]>("search_local", { query });

// 打开目录本身；打不开时把后端错误原样抛出，由调用方提示，不静默失败。
export const openFolder = (path: string) => openPath(path);
export const revealFolder = (path: string) => revealItemInDir(path);

// 原生目录选择器。取消返回 null。
export async function pickFolder(): Promise<string | null> {
  const picked = await openDialog({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}

// 走 Tauri 的原生剪贴板而不是 navigator.clipboard：后者要求文档处于焦点态，
// 窗口刚失焦就会静默抛错，而"一键复制路径/密码"正是这个软件的核心动作。
export async function copyText(text: string): Promise<void> {
  await writeText(text);
}

export interface VaultStatus {
  initialized: boolean;
  unlocked: boolean;
}

export const vaultStatus = () => invoke<VaultStatus>("vault_status");
// 初始化返回一次性恢复码，只在这一刻存在于内存与界面上，不写进任何 store 或本地存储。
export const vaultInit = (mainPassword: string) => invoke<string>("vault_init", { mainPassword });
export const vaultUnlock = (secret: string) => invoke<void>("vault_unlock", { secret });
export const vaultLock = () => invoke<void>("vault_lock");
export const vaultChangePassword = (oldPassword: string, newPassword: string) =>
  invoke<void>("vault_change_password", { oldPassword, newPassword });
export const vaultRotateRecoveryCode = () => invoke<string>("vault_rotate_recovery_code");

export const ledgerList = (projectId: string) => invoke<Ledger>("ledger_list", { projectId });
export const ledgerReveal = (kind: SecretKind, id: string, field: SecretField) =>
  invoke<string>("ledger_reveal", { kind, id, field });

export const createEnv = (projectId: string, input: EnvInput) =>
  invoke<Ledger["envs"][number]>("ledger_env_create", { projectId, input });
export const updateEnv = (id: string, input: EnvInput) =>
  invoke<void>("ledger_env_update", { id, input });
export const deleteEnv = (id: string) => invoke<void>("ledger_env_delete", { id });

export const createCredential = (projectId: string, input: CredentialInput) =>
  invoke<Ledger["credentials"][number]>("ledger_credential_create", { projectId, input });
export const updateCredential = (id: string, input: CredentialInput) =>
  invoke<void>("ledger_credential_update", { id, input });
export const deleteCredential = (id: string) => invoke<void>("ledger_credential_delete", { id });

export const createServer = (projectId: string, input: ServerInput) =>
  invoke<Ledger["servers"][number]>("ledger_server_create", { projectId, input });
export const updateServer = (id: string, input: ServerInput) =>
  invoke<void>("ledger_server_update", { id, input });
export const deleteServer = (id: string) => invoke<void>("ledger_server_delete", { id });

export const createLink = (projectId: string, input: LinkInput) =>
  invoke<Ledger["links"][number]>("ledger_link_create", { projectId, input });
export const updateLink = (id: string, input: LinkInput) => invoke<void>("ledger_link_update", { id, input });
export const deleteLink = (id: string) => invoke<void>("ledger_link_delete", { id });

export const createNote = (projectId: string, input: NoteInput) =>
  invoke<Ledger["notes"][number]>("ledger_note_create", { projectId, input });
export const updateNote = (id: string, input: NoteInput) => invoke<void>("ledger_note_update", { id, input });
export const deleteNote = (id: string) => invoke<void>("ledger_note_delete", { id });
