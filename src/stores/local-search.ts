import { create } from "zustand";
import * as api from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import type { FieldHit } from "@/types/search";

interface LocalSearchState {
  query: string;
  hits: FieldHit[];
  searched: boolean;
  busy: boolean;
  error: AppErrorShape | null;
  run: (query: string) => Promise<void>;
  clear: () => void;
}

// 连打两个字会发两次请求，后回来的旧结果不能盖掉新结果，所以带一个自增序号。
let seq = 0;

export const useLocalSearchStore = create<LocalSearchState>((set) => ({
  query: "",
  hits: [],
  searched: false,
  busy: false,
  error: null,

  run: async (query) => {
    const mine = ++seq;
    set({ query, busy: true, error: null });
    if (!query.trim()) {
      set({ hits: [], searched: false, busy: false });
      return;
    }
    try {
      const hits = await api.searchLocal(query);
      if (mine === seq) set({ hits, searched: true });
    } catch (e) {
      if (mine === seq) set({ error: toAppError(e), hits: [], searched: true });
    } finally {
      if (mine === seq) set({ busy: false });
    }
  },

  clear: () => {
    seq++;
    set({ query: "", hits: [], searched: false, busy: false, error: null });
  },
}));
