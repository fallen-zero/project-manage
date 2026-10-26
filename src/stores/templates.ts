import { create } from "zustand";
import * as api from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import type { ProjectTemplate, TemplateInput } from "@/types/project-template";

function upsert(items: ProjectTemplate[], t: ProjectTemplate): ProjectTemplate[] {
  const i = items.findIndex((x) => x.id === t.id);
  if (i === -1) return [t, ...items];
  const next = items.slice();
  next[i] = t;
  return next;
}

interface TemplatesState {
  items: ProjectTemplate[];
  loading: boolean;
  saving: boolean;
  error: AppErrorShape | null;
  refresh: () => Promise<void>;
  save: (id: string | null, input: TemplateInput) => Promise<boolean>;
  remove: (id: string) => Promise<boolean>;
  clearError: () => void;
}

export const useTemplatesStore = create<TemplatesState>((set, get) => {
  const run = async (fn: () => Promise<ProjectTemplate | null | void>): Promise<boolean> => {
    set({ saving: true, error: null });
    try {
      const t = await fn();
      if (t) set({ items: upsert(get().items, t) });
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
    loading: true,
    saving: false,
    error: null,

    refresh: async () => {
      set({ loading: true, error: null });
      try {
        set({ items: await api.listTemplates() });
      } catch (e) {
        set({ error: toAppError(e) });
      } finally {
        set({ loading: false });
      }
    },

    save: (id, input) => run(() => (id ? api.updateTemplate(id, input) : api.createTemplate(input))),

    remove: async (id) => {
      set({ saving: true, error: null });
      try {
        await api.deleteTemplate(id);
        set({ items: get().items.filter((t) => t.id !== id) });
        return true;
      } catch (e) {
        set({ error: toAppError(e) });
        return false;
      } finally {
        set({ saving: false });
      }
    },

    clearError: () => set({ error: null }),
  };
});
