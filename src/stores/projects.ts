import { create } from "zustand";
import * as api from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import type { Project, ProjectInput } from "@/types/project";

// 后端每个写命令都返回最新的 Project 整体，直接用它覆盖本地行，
// 不在前端重算标签/目录，避免出现两套真值。
function upsert(items: Project[], project: Project): Project[] {
  const i = items.findIndex((x) => x.id === project.id);
  if (i === -1) return [project, ...items];
  const next = items.slice();
  next[i] = project;
  return next;
}

interface ProjectsState {
  items: Project[];
  statuses: string[];
  loading: boolean;
  saving: boolean;
  error: AppErrorShape | null;
  refresh: () => Promise<void>;
  save: (id: string | null, input: ProjectInput) => Promise<boolean>;
  remove: (id: string) => Promise<boolean>;
  setRoot: (id: string, path: string, label?: string) => Promise<boolean>;
  addEntry: (id: string, path: string, label?: string) => Promise<boolean>;
  removeDir: (id: string, dirId: string) => Promise<boolean>;
  clearError: () => void;
}

export const useProjectsStore = create<ProjectsState>((set, get) => {
  // 每个动作同一种收口：成功覆盖本地行、失败把 AppError 三段式留给 UI
  const run = async (fn: () => Promise<Project | null | void>): Promise<boolean> => {
    set({ saving: true, error: null });
    try {
      const project = await fn();
      if (project) set({ items: upsert(get().items, project) });
      return true;
    } catch (e) {
      set({ error: toAppError(e) });
      return false;
    } finally {
      set({ saving: false });
    }
  };

  return {
    items: [],
    statuses: [],
    loading: true,
    saving: false,
    error: null,

    refresh: async () => {
      set({ loading: true, error: null });
      try {
        const [items, statuses] = await Promise.all([api.listProjects(), api.statusOptions()]);
        set({ items, statuses: statuses.length ? statuses : [] });
      } catch (e) {
        set({ error: toAppError(e) });
      } finally {
        set({ loading: false });
      }
    },

    save: (id, input) => run(() => (id ? api.updateProject(id, input) : api.createProject(input))),

    remove: async (id) => {
      set({ saving: true, error: null });
      try {
        await api.deleteProject(id);
        set({ items: get().items.filter((p) => p.id !== id) });
        return true;
      } catch (e) {
        set({ error: toAppError(e) });
        return false;
      } finally {
        set({ saving: false });
      }
    },

    setRoot: (id, path, label) => run(() => api.setRootDir(id, path, label)),
    addEntry: (id, path, label) => run(() => api.addEntryDir(id, path, label)),
    removeDir: (id, dirId) =>
      run(async () => {
        await api.removeDir(id, dirId);
        return api.getProject(id);
      }),

    clearError: () => set({ error: null }),
  };
});
