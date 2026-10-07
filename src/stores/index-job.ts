import { create } from "zustand";
import { indexCancel, indexOverview, indexStart, onIndexProgress } from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import type { IndexOverview, IndexProgress } from "@/types/index";

interface IndexJobState {
  overview: IndexOverview | null;
  progress: IndexProgress | null;
  /** IPC 调用本身被拒（`index_running` / `index_spawn_failed` / 参数校验）时才有值。
   *  与 `overview.lastError` 是两件事：那个是**上一轮作业**失败的原因串，这个是**这一次点击**没被受理。 */
  error: AppErrorShape | null;
  load: () => Promise<void>;
  start: (projectId?: string, rebuild?: boolean) => Promise<void>;
  cancel: () => Promise<void>;
  subscribe: () => Promise<() => void>;
}

export const useIndexJobStore = create<IndexJobState>((set) => ({
  overview: null,
  progress: null,
  error: null,
  load: async () => {
    try {
      set({ overview: await indexOverview(), error: null });
    } catch (e) {
      set({ error: toAppError(e) });
    }
  },
  start: async (projectId, rebuild) => {
    try {
      await indexStart(projectId, rebuild);
      set({ error: null });
    } catch (e) {
      set({ error: toAppError(e) });
    }
  },
  // 刻意不 try/catch：index_cancel 只做一次原子置位（store(true) + Ok(())），
  // 给它包一层 catch 等于声明了一个不可能的失败路径。
  cancel: async () => {
    await indexCancel();
  },
  subscribe: async () => {
    const unlisten = await onIndexProgress((p) => set({ progress: p }));
    return () => unlisten();
  },
}));

// 供页面在进度走到终态时刷新总览
export const isTerminal = (p: IndexProgress | null) =>
  p === null ? false : p.state === "done" || p.state === "cancelled" || p.state === "error";
