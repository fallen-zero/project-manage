import { invoke } from "@tauri-apps/api/core";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { Project, ProjectInput } from "@/types/project";

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
