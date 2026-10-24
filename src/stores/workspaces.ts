import { create } from "zustand";
import * as api from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { activeWorkspaceId } from "@/lib/workspace-view";
import type { Workspace } from "@/types/workspace";

// 后端的每个写命令都回整行 Workspace，直接覆盖本地行，不在前端重算 projectCount。
function upsert(items: Workspace[], w: Workspace): Workspace[] {
  const i = items.findIndex((x) => x.id === w.id);
  if (i === -1) return [...items, w];
  const next = items.slice();
  next[i] = w;
  return next;
}

interface WorkspacesState {
  items: Workspace[];
  activeId: string | null;
  loading: boolean;
  saving: boolean;
  error: AppErrorShape | null;
  refresh: () => Promise<void>;
  create: (name: string) => Promise<boolean>;
  rename: (id: string, name: string) => Promise<boolean>;
  remove: (id: string) => Promise<boolean>;
  switchTo: (id: string) => Promise<boolean>;
  clearError: () => void;
}

export const useWorkspacesStore = create<WorkspacesState>((set, get) => {
  const run = async (fn: () => Promise<Workspace | void>): Promise<boolean> => {
    set({ saving: true, error: null });
    try {
      const w = await fn();
      if (w) set({ items: upsert(get().items, w) });
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
    activeId: null,
    loading: true,
    saving: false,
    error: null,

    refresh: async () => {
      set({ loading: true, error: null });
      try {
        const items = await api.listWorkspaces();
        set({ items, activeId: activeWorkspaceId(items) });
      } catch (e) {
        set({ error: toAppError(e) });
      } finally {
        set({ loading: false });
      }
    },

    // 新建**不自动切过去**（spec §五）：run() 里只 upsert 一行，activeId 原样不动。
    create: (name) => run(() => api.createWorkspace(name)),

    rename: (id, name) => run(() => api.renameWorkspace(id, name)),

    remove: (id) =>
      run(async () => {
        await api.deleteWorkspace(id);
        set({ items: get().items.filter((w) => w.id !== id) });
      }),

    // 切换成功后整份重取：区名没变，但 isActive 与各页可见数据全变了。
    // 在前端手改标志位等于造第二个真值（spec §8.2）。顶栏靠 activeId 变化触发 <Outlet> 重挂载。
    switchTo: async (id) => {
      set({ saving: true, error: null });
      try {
        await api.switchWorkspace(id);
      } catch (e) {
        set({ error: toAppError(e) });
        return false;
      } finally {
        set({ saving: false });
      }
      await get().refresh();
      // refresh 的失败只写 error 不抛；这里如实回报，免得调用方把「后端已换区、本地仍旧区」当成功。
      return get().error === null;
    },

    clearError: () => set({ error: null }),
  };
});
