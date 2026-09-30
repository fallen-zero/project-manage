import { create } from "zustand";
import * as api from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";

interface VaultState {
  status: api.VaultStatus | null;
  loading: boolean;
  busy: boolean;
  error: AppErrorShape | null;
  // 恢复码与"新主密码"只在成功那一刻出现在这里：不写 localStorage、不进日志，
  // 用户抄走之后点「我已保存」就清空。
  oneTimeCode: string | null;
  refresh: () => Promise<void>;
  init: (mainPassword: string) => Promise<boolean>;
  unlock: (secret: string) => Promise<boolean>;
  lock: () => Promise<void>;
  changePassword: (oldPassword: string, newPassword: string) => Promise<boolean>;
  rotateRecoveryCode: () => Promise<boolean>;
  dismissCode: () => void;
  clearError: () => void;
}

export const useVaultStore = create<VaultState>((set) => {
  // 解锁相关的每个动作同一种收口：失败留下三段式错误，成功重新读一次状态。
  const act = async (fn: () => Promise<string | null | void>): Promise<boolean> => {
    set({ busy: true, error: null });
    try {
      const code = await fn();
      if (code) set({ oneTimeCode: code });
      set({ status: await api.vaultStatus() });
      return true;
    } catch (e) {
      set({ error: toAppError(e) });
      return false;
    } finally {
      set({ busy: false });
    }
  };

  return {
    status: null,
    loading: true,
    busy: false,
    error: null,
    oneTimeCode: null,

    refresh: async () => {
      set({ loading: true });
      try {
        set({ status: await api.vaultStatus() });
      } catch (e) {
        set({ error: toAppError(e) });
      } finally {
        set({ loading: false });
      }
    },

    init: (mainPassword) => act(() => api.vaultInit(mainPassword)),
    unlock: (secret) => act(() => api.vaultUnlock(secret)),
    lock: async () => {
      await act(api.vaultLock);
    },
    changePassword: (oldPassword, newPassword) =>
      act(() => api.vaultChangePassword(oldPassword, newPassword)),
    rotateRecoveryCode: () => act(api.vaultRotateRecoveryCode),

    dismissCode: () => set({ oneTimeCode: null }),
    clearError: () => set({ error: null }),
  };
});

// 台账面板只需要这一个判定，单独导出避免每个调用点都写可选链。
export const useVaultUnlocked = () => useVaultStore((s) => s.status?.unlocked ?? false);
