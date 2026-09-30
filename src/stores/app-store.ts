import { create } from "zustand";
import { dbStatus, toAppError, type AppErrorShape, type DbStatus } from "@/lib/ipc";

interface AppState {
  status: DbStatus | null;
  error: AppErrorShape | null;
  loading: boolean;
  refresh: () => Promise<void>;
}

export const useAppStore = create<AppState>((set) => ({
  status: null,
  error: null,
  loading: true,
  refresh: async () => {
    set({ loading: true });
    try {
      set({ status: await dbStatus(), error: null });
    } catch (e) {
      set({ status: null, error: toAppError(e) });
    } finally {
      set({ loading: false });
    }
  },
}));
