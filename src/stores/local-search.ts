import { create } from "zustand";
import * as api from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { isNewest } from "@/lib/search-order";
import type { SearchBundle } from "@/types/search";

interface LocalSearchState {
  bundle: SearchBundle | null;
  searched: boolean;
  busy: boolean;
  error: AppErrorShape | null;
  run: (query: string) => Promise<void>;
  clear: () => void;
}

// 连打两个字会发两次请求，后回来的旧结果不能盖掉新结果，所以带一个自增序号。
let seq = 0;

export const useLocalSearchStore = create<LocalSearchState>((set) => ({
  bundle: null,
  searched: false,
  busy: false,
  error: null,

  run: async (query) => {
    const mine = ++seq;
    set({ busy: true, error: null });
    if (!query.trim()) {
      set({ bundle: null, searched: false, busy: false });
      return;
    }
    try {
      const bundle = await api.searchAll(query);
      if (isNewest(mine, seq)) set({ bundle, searched: true });
    } catch (e) {
      if (isNewest(mine, seq)) set({ error: toAppError(e), bundle: null, searched: true });
    } finally {
      if (isNewest(mine, seq)) set({ busy: false });
    }
  },

  clear: () => {
    seq++;
    set({ bundle: null, searched: false, busy: false, error: null });
  },
}));
