import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath, openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { Project, ProjectInput } from "@/types/project";
import type { AuditAction, AuditPage } from "@/types/project-audit";
import type { Workspace } from "@/types/workspace";
import type { ProjectTemplate, TemplateInput } from "@/types/project-template";
import type { DocPreview, SearchBundle } from "@/types/search";
import type {
  CredentialInput,
  EnvInput,
  Ledger,
  LedgerExportInput,
  LedgerExportSummary,
  LinkInput,
  NoteInput,
  SecretField,
  SecretKind,
  ServerInput,
} from "@/types/ledger";
import type { DocHit, DocRow, IndexOverview, IndexProgress } from "@/types/index";
import type { BackupSummary, ImportOutcome, PackageInfo } from "@/types/backup";
import type { Todo, TodoInput, TodoEdit } from "@/types/todo";
import type { Milestone, MilestoneInput, MilestoneEdit } from "@/types/milestone";
import type { Expiry, ExpiryInput, ExpiryUpdate } from "@/types/expiry";
import type {
  DailyReport,
  DailyReportEdit,
  DailyReportExportRow,
  DailyReportInput,
  ImportPreview,
  // 备份包那边已经占用了 `ImportOutcome` 这个名字（上面 `@/types/backup` 那一行），
  // 这里必须起别名：同名两条 import 语句是 tsc 的重复标识符错误，而不是「谁在后面谁生效」。
  ImportOutcome as DailyReportImportOutcome,
} from "@/types/daily-report";

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

// —— M14 项目操作日志 ——
export const listAudit = (input: {
  projectId?: string;
  action?: AuditAction;
  limit?: number;
  offset?: number;
} = {}) => invoke<AuditPage>("audit_list", input);

// —— M15 工作区（五条同步命令；切换器与「管理工作区」弹窗的唯一出口）——
export const listWorkspaces = () => invoke<Workspace[]>("workspace_list");
export const createWorkspace = (name: string) => invoke<Workspace>("workspace_create", { name });
export const renameWorkspace = (id: string, name: string) =>
  invoke<Workspace>("workspace_rename", { id, name });
export const deleteWorkspace = (id: string) => invoke<void>("workspace_delete", { id });
export const switchWorkspace = (id: string) => invoke<string>("workspace_switch", { id });

// —— M12 项目模板 ——
export const listTemplates = () => invoke<ProjectTemplate[]>("template_list");
export const createTemplate = (input: TemplateInput) =>
  invoke<ProjectTemplate>("template_create", { input });
export const updateTemplate = (id: string, input: TemplateInput) =>
  invoke<ProjectTemplate>("template_update", { id, input });
export const deleteTemplate = (id: string) => invoke<void>("template_delete", { id });

export const searchAll = (query: string) => invoke<SearchBundle>("search_all", { query });
// 点开正文命中：后端锁内查库、锁外重抽原文，返回窗口文本与码元区间。
export const docPreview = (docId: string, query: string) =>
  invoke<DocPreview>("doc_preview", { docId, query });

// 打开目录本身；打不开时把后端错误原样抛出，由调用方提示，不静默失败。
export const openFolder = (path: string) => openPath(path);
export const revealFolder = (path: string) => revealItemInDir(path);

// 原生目录选择器。取消返回 null。
export async function pickFolder(): Promise<string | null> {
  const picked = await openDialog({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}

// —— M8 备份：三条命令 + 两个原生对话框封装 ——
// 取消不是错误：save/open 返回 null 时由调用方直接 return，绝不写进 error。
// dialog:default 实测含 allow-save / allow-open，补 save 不需要新权限。
export async function pickBackupPath(defaultName: string): Promise<string | null> {
  const picked = await saveDialog({
    defaultPath: defaultName,
    filters: [{ name: "备份包", extensions: ["zip"] }],
  });
  return typeof picked === "string" ? picked : null;
}

export async function pickPackagePath(): Promise<string | null> {
  const picked = await openDialog({
    multiple: false,
    filters: [{ name: "备份包", extensions: ["zip"] }],
  });
  return typeof picked === "string" ? picked : null;
}

export const backupExport = (path: string) => invoke<BackupSummary>("backup_export", { path });
export const backupInspect = (path: string) => invoke<PackageInfo>("backup_inspect", { path });
export const backupImport = (path: string) => invoke<ImportOutcome>("backup_import", { path });

// —— M9 待办与里程碑：八个封装（lib.rs 里 todo_*/milestone_* 四对命令的前端接线口）——
// list 的 projectId 传显式 `null` 而不是省键：后端形参是 Option<String>，
// 省键要依赖 Tauri 对缺失参数落 None 的行为，本仓没有先例，别赌。
export const listTodos = (projectId: string | null) =>
  invoke<Todo[]>("todo_list", { projectId });
export const createTodo = (input: TodoInput) => invoke<Todo>("todo_create", { input });
export const updateTodo = (id: string, input: TodoEdit) => invoke<void>("todo_update", { id, input });
export const deleteTodo = (id: string) => invoke<void>("todo_delete", { id });

export const listMilestones = (projectId: string | null) =>
  invoke<Milestone[]>("milestone_list", { projectId });
export const createMilestone = (input: MilestoneInput) =>
  invoke<Milestone>("milestone_create", { input });
export const updateMilestone = (id: string, input: MilestoneEdit) =>
  invoke<void>("milestone_update", { id, input });
export const deleteMilestone = (id: string) => invoke<void>("milestone_delete", { id });

// —— M10 日报：四个封装（导出/导入的三个在 Task 6/7 各自那一格补，别提前塞）——
// list 的 projectId 传显式 `null` 而不是省键（同 M9 那条告诫）。
export const listDailyReports = (projectId: string | null) =>
  invoke<DailyReport[]>("daily_report_list", { projectId });
export const createDailyReport = (input: DailyReportInput) =>
  invoke<DailyReport>("daily_report_create", { input });
export const updateDailyReport = (id: string, input: DailyReportEdit) =>
  invoke<void>("daily_report_update", { id, input });
export const deleteDailyReport = (id: string) => invoke<void>("daily_report_delete", { id });

// —— M11-a 到期事项：四个封装 ——
// list 的 projectId 传显式 `null` 而不是省键（同 M9/M10 那条告诫）。
export const listExpiries = (projectId: string | null) => invoke<Expiry[]>("expiry_list", { projectId });
export const createExpiry = (input: ExpiryInput) => invoke<Expiry>("expiry_create", { input });
export const updateExpiry = (id: string, input: ExpiryUpdate) => invoke<void>("expiry_update", { id, input });
export const deleteExpiry = (id: string) => invoke<void>("expiry_delete", { id });

// —— M10 日报导出 ——
export async function pickXlsxSavePath(defaultName: string): Promise<string | null> {
  const picked = await saveDialog({
    defaultPath: defaultName,
    filters: [{ name: "Excel 工作簿", extensions: ["xlsx"] }],
  });
  return typeof picked === "string" ? picked : null;
}

export const exportDailyReports = (path: string, rows: DailyReportExportRow[]) =>
  invoke<number>("daily_report_export", { path, rows });

// —— M10 日报导入（Task 7，两段式）——
// 预检只读、确认才写：前端只把**路径**发给后端，绝不把预检结果回传（后端 apply 自己重新解析同一个文件，
// 判据因此只有一处，且 apply 不信任客户端）。
// 取消不是错误：open 返回 null 时由调用方直接 return，绝不写进 error（与 `pickPackagePath` 同形制）。
export async function pickXlsxOpenPath(): Promise<string | null> {
  const picked = await openDialog({
    multiple: false,
    filters: [{ name: "Excel 工作簿", extensions: ["xlsx"] }],
  });
  return typeof picked === "string" ? picked : null;
}

export const importDailyReportsPreview = (path: string) =>
  invoke<ImportPreview>("daily_report_import_preview", { path });
export const importDailyReportsApply = (path: string) =>
  invoke<DailyReportImportOutcome>("daily_report_import_apply", { path });

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

// —— M3 全文索引：5 个命令 + 1 个进度事件（Task 10 的 aff828d）——
// 形参按 camelCase 传，Tauri 会自动映射到 Rust 的 snake_case 形参（removeDir/ledgerList/vaultInit 已是先例）。
export const indexStart = (projectId?: string, rebuild = false) =>
  invoke<void>("index_start", { projectId: projectId ?? null, rebuild });
export const indexCancel = () => invoke<void>("index_cancel");
export const indexOverview = () => invoke<IndexOverview>("index_overview");
export const indexDocs = (projectId: string, status: string | null, limit = 200, offset = 0) =>
  invoke<DocRow[]>("index_docs", { projectId, status, limit, offset });
export const searchDocs = (query: string, limit?: number) =>
  invoke<DocHit[]>("search_docs", { query, limit: limit ?? null });
// `index://progress` 就是 Rust 侧的 index_job::PROGRESS_EVENT（index_job.rs:22）：
// 两侧都是字面量、没有共享常量，拼错事件名 tsc 抓不到，只能由真机点验确认连通。
export const onIndexProgress = (handler: (p: IndexProgress) => void) =>
  listen<IndexProgress>("index://progress", (e) => handler(e.payload));

export interface LibreOfficeInstallOutcome {
  startedViaWinget: boolean;
}

/** 一键安装 LibreOffice（无参数：包 ID 硬编码在 Rust 侧）。true = 已弹出 winget 窗口，进度与 UAC 都在
 *  那个窗口里，本调用不等安装结束；false = 本机没有 winget，调用方要退到打开官网下载页。 */
export const installLibreOffice = () =>
  invoke<LibreOfficeInstallOutcome>("libreoffice_install");

/** winget 不在时的退路：用系统默认浏览器打开 LibreOffice 下载页（opener:default 已含 https://* scope）。 */
export const openLibreOfficeDownloadPage = () =>
  openUrl("https://www.libreoffice.org/download/download-libreoffice/");

// —— M11-b 台账导出（spec §四/§七）——
// 意图（格式/明文/落点）都由前端收集，取数与写盘全在后端；`path` 来自保存对话框的当场选择。
export const exportLedger = (input: LedgerExportInput) =>
  invoke<LedgerExportSummary>("ledger_export", { input });
