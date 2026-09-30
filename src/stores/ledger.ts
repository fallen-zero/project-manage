import { create } from "zustand";
import * as api from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import {
  emptyLedger,
  type CredentialInput,
  type EnvInput,
  type Ledger,
  type LinkInput,
  type NoteInput,
  type ServerInput,
} from "@/types/ledger";

// 五类台账实体的输入，键名与 Ledger 里的数组字段一致，
// 这样一个 dispatch 表就能覆盖全部增删改，不必写十五个方法。
export type EntityInputs = {
  envs: EnvInput;
  credentials: CredentialInput;
  servers: ServerInput;
  links: LinkInput;
  notes: NoteInput;
};
export type Entity = keyof EntityInputs;

type Ops = {
  [K in keyof EntityInputs]: {
    create: (projectId: string, input: EntityInputs[K]) => Promise<unknown>;
    update: (id: string, input: EntityInputs[K]) => Promise<void>;
    remove: (id: string) => Promise<void>;
  };
};

const OPS: Ops = {
  envs: { create: (p, i) => api.createEnv(p, i), update: api.updateEnv, remove: api.deleteEnv },
  credentials: {
    create: (p, i) => api.createCredential(p, i),
    update: api.updateCredential,
    remove: api.deleteCredential,
  },
  servers: {
    create: (p, i) => api.createServer(p, i),
    update: api.updateServer,
    remove: api.deleteServer,
  },
  links: { create: (p, i) => api.createLink(p, i), update: api.updateLink, remove: api.deleteLink },
  notes: { create: (p, i) => api.createNote(p, i), update: api.updateNote, remove: api.deleteNote },
};

interface LedgerState {
  projectId: string | null;
  data: Ledger;
  loading: boolean;
  saving: boolean;
  error: AppErrorShape | null;
  load: (projectId: string) => Promise<void>;
  reload: () => Promise<void>;
  add: <K extends Entity>(entity: K, input: EntityInputs[K]) => Promise<boolean>;
  patch: <K extends Entity>(entity: K, id: string, input: EntityInputs[K]) => Promise<boolean>;
  drop: (entity: Entity, id: string) => Promise<boolean>;
  clear: () => void;
  clearError: () => void;
}

export const useLedgerStore = create<LedgerState>((set, get) => {
  // 写成功后整表重读：库里读回来的就是唯一真值，前端不猜新增行的顺序，也不本地拼对象。
  const write = async (fn: () => Promise<void>): Promise<boolean> => {
    if (!get().projectId) return false;
    set({ saving: true, error: null });
    try {
      await fn();
      await get().reload();
      return true;
    } catch (e) {
      set({ error: toAppError(e) });
      return false;
    } finally {
      set({ saving: false });
    }
  };

  return {
    projectId: null,
    data: emptyLedger(),
    loading: false,
    saving: false,
    error: null,

    load: async (projectId) => {
      set({ projectId, loading: true, error: null });
      await get().reload();
    },

    reload: async () => {
      const pid = get().projectId;
      if (!pid) return;
      try {
        const data = await api.ledgerList(pid);
        // 切换项目时旧数据会闪一下，所以只在还是同一个项目时才覆盖。
        if (get().projectId === pid) set({ data, loading: false });
      } catch (e) {
        set({ error: toAppError(e), loading: false });
      }
    },

    add: (entity, input) =>
      write(async () => {
        await OPS[entity].create(get().projectId as string, input);
      }),

    patch: (entity, id, input) =>
      write(() => OPS[entity].update(id, input)),

    drop: (entity, id) => write(() => OPS[entity].remove(id)),

    clear: () => set({ projectId: null, data: emptyLedger(), error: null }),
    clearError: () => set({ error: null }),
  };
});
