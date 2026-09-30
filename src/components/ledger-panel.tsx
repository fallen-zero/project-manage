import { useEffect, useState, type ReactNode } from "react";
import { Copy, Eye, EyeOff, Pencil, Plus, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { copyText, ledgerReveal } from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { useLedgerStore, type Entity, type EntityInputs } from "@/stores/ledger";
import { useVaultUnlocked } from "@/stores/vault";
import {
  ENV_LABELS,
  LINK_LABELS,
  blank,
  type Credential,
  type EnvKind,
  type Ledger,
  type LedgerEnv,
  type Link,
  type LinkKind,
  type Note,
  type SecretField,
  type SecretKind,
  type Server,
} from "@/types/ledger";

type Form = Record<string, string>;

type FieldKind = "text" | "secret" | "textarea" | "env" | "envOf" | "linkKind";
interface FieldDef {
  key: string;
  label: string;
  kind: FieldKind;
  required?: boolean;
}

// 每个实体一份「字段表 + 表单值→输入对象」：新增与编辑共用同一个表单渲染，
// build 就地写出各实体自己的输入类型，避免一个 switch 返回 never 的写法。
const FORMS: {
  [K in Entity]: { title: string; fields: FieldDef[]; build: (f: Form) => EntityInputs[K] };
} = {
  envs: {
    title: "环境",
    fields: [
      { key: "env", label: "环境", kind: "env", required: true },
      { key: "name", label: "名称", kind: "text", required: true },
      { key: "url", label: "访问地址", kind: "text" },
      { key: "port", label: "端口", kind: "text" },
      { key: "note", label: "备注", kind: "textarea" },
    ],
    build: (f) => ({
      env: f.env as EnvKind,
      name: f.name.trim(),
      url: blank(f.url ?? ""),
      port: blank(f.port ?? ""),
      note: blank(f.note ?? ""),
    }),
  },
  credentials: {
    title: "凭据",
    fields: [
      { key: "title", label: "标题（用途）", kind: "text", required: true },
      { key: "envOf", label: "所属环境", kind: "envOf" },
      { key: "username", label: "账号", kind: "secret" },
      { key: "password", label: "密码", kind: "secret" },
      { key: "url", label: "登录地址", kind: "text" },
      { key: "note", label: "备注", kind: "textarea" },
    ],
    build: (f) => ({
      envId: f.envOf === NONE ? null : (f.envOf ?? null),
      title: f.title.trim(),
      username: blank(f.username ?? ""),
      password: blank(f.password ?? ""),
      url: blank(f.url ?? ""),
      note: blank(f.note ?? ""),
    }),
  },
  servers: {
    title: "服务器",
    fields: [
      { key: "name", label: "名称", kind: "text", required: true },
      { key: "ip", label: "IP / 域名", kind: "text" },
      { key: "bastion", label: "堡垒机", kind: "text" },
      { key: "account", label: "远程账号", kind: "secret" },
      { key: "password", label: "密码 / 私钥口令", kind: "secret" },
      { key: "note", label: "备注", kind: "textarea" },
    ],
    build: (f) => ({
      name: f.name.trim(),
      ip: blank(f.ip ?? ""),
      bastion: blank(f.bastion ?? ""),
      account: blank(f.account ?? ""),
      password: blank(f.password ?? ""),
      note: blank(f.note ?? ""),
    }),
  },
  links: {
    title: "外部链接",
    fields: [
      { key: "title", label: "标题", kind: "text", required: true },
      { key: "url", label: "地址", kind: "text", required: true },
      { key: "kind", label: "归类", kind: "linkKind" },
      { key: "note", label: "备注", kind: "textarea" },
    ],
    build: (f) => ({
      kind: f.kind as LinkKind,
      title: f.title.trim(),
      url: f.url.trim(),
      note: blank(f.note ?? ""),
    }),
  },
  notes: {
    title: "备注",
    fields: [
      { key: "title", label: "标题", kind: "text", required: true },
      { key: "tag", label: "标签", kind: "text" },
      { key: "body", label: "正文", kind: "textarea" },
    ],
    build: (f) => ({
      title: f.title.trim(),
      body: f.body ?? "",
      tag: blank(f.tag ?? ""),
    }),
  },
};

const ENVS: EnvKind[] = ["dev", "test", "pre", "prod"];
const LINK_KINDS: LinkKind[] = ["jira", "zentao", "wiki", "doc", "repo", "other"];
const NONE = "__none__";

// 加密实体的可解密列，与 Rust 侧 SECRET_FIELDS 一一对应。
const SECRETS: Partial<Record<Entity, { kind: SecretKind; fields: SecretField[] }>> = {
  credentials: { kind: "credential", fields: ["username", "password"] },
  servers: { kind: "server", fields: ["account", "password"] },
};

export function LedgerPanel({ projectId }: { projectId: string }) {
  const { data, loading, saving, error, load, clear } = useLedgerStore();
  const unlocked = useVaultUnlocked();
  const [editing, setEditing] = useState<{ entity: Entity; id: string | null } | null>(null);

  useEffect(() => {
    void load(projectId);
    return clear;
  }, [projectId, load, clear]);

  return (
    <section className="grid gap-3">
      <header className="flex items-center gap-2">
        <h2 className="text-sm font-semibold">信息台账</h2>
        {!unlocked && (
          <Badge variant="outline" title="凭据与服务器的新增和编辑需要主密码解锁；环境、链接、备注不受影响">
            未解锁
          </Badge>
        )}
        {loading && <span className="text-xs text-muted-foreground">读取中…</span>}
      </header>

      {error && (
        <p className="text-sm text-destructive" title={error.hint ?? undefined}>
          {error.message}
          {error.hint ? `（${error.hint}）` : ""}
        </p>
      )}

      <EnvsSection envs={data.envs} onAdd={() => setEditing({ entity: "envs", id: null })} onEdit={(id) => setEditing({ entity: "envs", id })} />
      <CredentialsSection
        items={data.credentials}
        envs={data.envs}
        unlocked={unlocked}
        onAdd={() => setEditing({ entity: "credentials", id: null })}
        onEdit={(id) => setEditing({ entity: "credentials", id })}
      />
      <ServersSection
        items={data.servers}
        unlocked={unlocked}
        onAdd={() => setEditing({ entity: "servers", id: null })}
        onEdit={(id) => setEditing({ entity: "servers", id })}
      />
      <LinksSection items={data.links} onAdd={() => setEditing({ entity: "links", id: null })} onEdit={(id) => setEditing({ entity: "links", id })} />
      <NotesSection items={data.notes} onAdd={() => setEditing({ entity: "notes", id: null })} onEdit={(id) => setEditing({ entity: "notes", id })} />

      {editing && (
        <LedgerFormDialog
          entity={editing.entity}
          id={editing.id}
          envs={data.envs}
          saving={saving}
          onClose={() => setEditing(null)}
        />
      )}
    </section>
  );
}

function SectionHeader({
  title,
  count,
  onAdd,
  addLabel,
}: {
  title: string;
  count: number;
  onAdd: () => void;
  addLabel: string;
}) {
  return (
    <div className="flex items-center justify-between">
      <h3 className="text-xs font-medium text-muted-foreground">
        {title}
        <span className="ml-1">{count}</span>
      </h3>
      <Button variant="ghost" size="sm" onClick={onAdd}>
        <Plus className="size-4" />
        {addLabel}
      </Button>
    </div>
  );
}

function Empty({ text }: { text: string }) {
  return <p className="rounded-md border border-dashed p-2 text-xs text-muted-foreground">{text}</p>;
}

function EnvsSection({
  envs,
  onAdd,
  onEdit,
}: {
  envs: LedgerEnv[];
  onAdd: () => void;
  onEdit: (id: string) => void;
}) {
  const { drop } = useLedgerStore();
  return (
    <div className="grid gap-1.5">
      <SectionHeader title="多环境地址" count={envs.length} onAdd={onAdd} addLabel="加环境" />
      {envs.length === 0 ? (
        <Empty text="登记 dev/test/pre/prod 的访问地址，找网址时一眼就能看到。" />
      ) : (
        envs.map((e) => (
          <Row
            key={e.id}
            main={
              <span className="flex items-center gap-2">
                <Badge variant="secondary">{ENV_LABELS[e.env] ?? e.env}</Badge>
                <span>{e.name}</span>
                {e.url && (
                  <span className="truncate font-mono text-xs text-muted-foreground" title={e.url}>
                    {e.url}
                    {e.port ? `:${e.port}` : ""}
                  </span>
                )}
              </span>
            }
            onEdit={() => onEdit(e.id)}
            onDelete={() => void drop("envs", e.id)}
            copy={e.url ? [e.url, e.port ? `${e.url}:${e.port}` : e.url] : []}
          />
        ))
      )}
    </div>
  );
}

function CredentialsSection({
  items,
  envs,
  unlocked,
  onAdd,
  onEdit,
}: {
  items: Credential[];
  envs: LedgerEnv[];
  unlocked: boolean;
  onAdd: () => void;
  onEdit: (id: string) => void;
}) {
  const { drop } = useLedgerStore();
  const envName = (id: string | null) => envs.find((e) => e.id === id);
  return (
    <div className="grid gap-1.5">
      <SectionHeader title="账号凭据" count={items.length} onAdd={onAdd} addLabel="加凭据" />
      {items.length === 0 ? (
        <Empty text="账号与密码进库前用主密钥加密，列表里只标注有没有值。" />
      ) : (
        items.map((c) => {
          const env = envName(c.envId);
          return (
            <Row
              key={c.id}
              main={
                <span className="flex items-center gap-2">
                  <span>{c.title}</span>
                  {env && <Badge variant="outline">{ENV_LABELS[env.env]}</Badge>}
                  {c.url && (
                    <span className="truncate font-mono text-xs text-muted-foreground" title={c.url}>
                      {c.url}
                    </span>
                  )}
                </span>
              }
              extra={
                <>
                  <SecretCell kind="credential" id={c.id} field="username" set={c.usernameSet} unlocked={unlocked} />
                  <SecretCell kind="credential" id={c.id} field="password" set={c.passwordSet} unlocked={unlocked} />
                </>
              }
              onEdit={() => onEdit(c.id)}
              onDelete={() => void drop("credentials", c.id)}
            />
          );
        })
      )}
    </div>
  );
}

function ServersSection({
  items,
  unlocked,
  onAdd,
  onEdit,
}: {
  items: Server[];
  unlocked: boolean;
  onAdd: () => void;
  onEdit: (id: string) => void;
}) {
  const { drop } = useLedgerStore();
  return (
    <div className="grid gap-1.5">
      <SectionHeader title="服务器" count={items.length} onAdd={onAdd} addLabel="加服务器" />
      {items.length === 0 ? (
        <Empty text="IP、堡垒机与远程账号；账号和密码是密文列。" />
      ) : (
        items.map((s) => (
          <Row
            key={s.id}
            main={
              <span className="flex items-center gap-2">
                <span>{s.name}</span>
                {s.ip && <span className="font-mono text-xs">{s.ip}</span>}
                {s.bastion && <span className="text-xs text-muted-foreground">经 {s.bastion}</span>}
              </span>
            }
            extra={
              <>
                <SecretCell kind="server" id={s.id} field="account" set={s.accountSet} unlocked={unlocked} />
                <SecretCell kind="server" id={s.id} field="password" set={s.passwordSet} unlocked={unlocked} />
              </>
            }
            onEdit={() => onEdit(s.id)}
            onDelete={() => void drop("servers", s.id)}
          />
        ))
      )}
    </div>
  );
}

function LinksSection({
  items,
  onAdd,
  onEdit,
}: {
  items: Link[];
  onAdd: () => void;
  onEdit: (id: string) => void;
}) {
  const { drop } = useLedgerStore();
  return (
    <div className="grid gap-1.5">
      <SectionHeader title="外部链接" count={items.length} onAdd={onAdd} addLabel="加链接" />
      {items.length === 0 ? (
        <Empty text="Jira、禅道、Wiki、文档与代码库地址。" />
      ) : (
        items.map((l) => (
          <Row
            key={l.id}
            main={
              <span className="flex items-center gap-2">
                <Badge variant="outline">{LINK_LABELS[l.kind] ?? l.kind}</Badge>
                <span>{l.title}</span>
                <span className="truncate font-mono text-xs text-muted-foreground" title={l.url}>
                  {l.url}
                </span>
              </span>
            }
            onEdit={() => onEdit(l.id)}
            onDelete={() => void drop("links", l.id)}
            copy={[l.url]}
          />
        ))
      )}
    </div>
  );
}

function NotesSection({ items, onAdd, onEdit }: { items: Note[]; onAdd: () => void; onEdit: (id: string) => void }) {
  const { drop } = useLedgerStore();
  return (
    <div className="grid gap-1.5">
      <SectionHeader title="备注台账" count={items.length} onAdd={onAdd} addLabel="加备注" />
      {items.length === 0 ? (
        <Empty text="接口坑点、遗留问题、需求变更这类「当时知道、三个月后想不起来」的事。" />
      ) : (
        items.map((n) => <NoteRow key={n.id} note={n} onEdit={() => onEdit(n.id)} onDelete={() => void drop("notes", n.id)} />)
      )}
    </div>
  );
}

function NoteRow({ note, onEdit, onDelete }: { note: Note; onEdit: () => void; onDelete: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="rounded-md border bg-background px-2 py-1.5 text-sm">
      <div className="flex items-center gap-2">
        <button className="flex-1 text-left" onClick={() => setOpen((v) => !v)}>
          {note.title}
          {note.tag && <Badge variant="secondary" className="ml-2">{note.tag}</Badge>}
        </button>
        <Button variant="ghost" size="icon" onClick={onEdit}>
          <Pencil className="size-4" />
        </Button>
        <Button variant="ghost" size="icon" onClick={onDelete}>
          <Trash2 className="size-4" />
        </Button>
      </div>
      {open && note.body && (
        <p className="mt-1 whitespace-pre-wrap text-xs text-muted-foreground">{note.body}</p>
      )}
    </div>
  );
}

function Row({
  main,
  extra,
  copy: copies = [],
  onEdit,
  onDelete,
}: {
  main: ReactNode;
  extra?: ReactNode;
  copy?: string[];
  onEdit: () => void;
  onDelete: () => void;
}) {
  const [copied, setCopied] = useState<string | null>(null);
  return (
    <div className="flex items-center gap-1.5 rounded-md border bg-background px-2 py-1.5 text-sm">
      <div className="min-w-0 flex-1">{main}</div>
      {extra}
      {copies.map((value) => (
        <Button
          key={value}
          variant="ghost"
          size="icon"
          title="复制"
          onClick={() =>
            void copyText(value).then(() => {
              setCopied(value);
              window.setTimeout(() => setCopied((c) => (c === value ? null : c)), 1500);
            })
          }
        >
          <Copy className="size-4" />
        </Button>
      ))}
      <Button variant="ghost" size="icon" onClick={onEdit}>
        <Pencil className="size-4" />
      </Button>
      <Button variant="ghost" size="icon" onClick={onDelete}>
        <Trash2 className="size-4" />
      </Button>
      {copied && <span className="text-xs text-emerald-700">已复制</span>}
    </div>
  );
}

// 明文只在点击期间待在这个组件的局部 state 里：不进 zustand，不收进列表对象，
// 隐藏或卸载即丢，避免 DevTools 的 store 快照里躺着密码。
function SecretCell({
  kind,
  id,
  field,
  set,
  unlocked,
}: {
  kind: SecretKind;
  id: string;
  field: SecretField;
  set: boolean;
  unlocked: boolean;
}) {
  const [shown, setShown] = useState<string | null>(null);
  const [error, setError] = useState<AppErrorShape | null>(null);

  // 锁定后立刻丢掉已解出的明文，界面上的「锁定后敏感字段立即不可读」才算真的成立。
  useEffect(() => {
    if (!unlocked) setShown(null);
  }, [unlocked]);

  if (!set) return <span className="text-xs text-muted-foreground">无{LABEL[field]}</span>;

  const toggle = async () => {
    if (shown !== null) {
      setShown(null);
      return;
    }
    try {
      setShown(await ledgerReveal(kind, id, field));
    } catch (e) {
      setError(toAppError(e));
    }
  };

  return (
    <span className="flex items-center gap-1">
      {shown === null ? (
        <span className="font-mono text-xs text-muted-foreground">••••••</span>
      ) : (
        <span className="font-mono text-xs">{shown}</span>
      )}
      <Button variant="ghost" size="icon" disabled={!unlocked} title={unlocked ? "显示/隐藏" : "需先解锁主密码库"} onClick={() => void toggle()}>
        {shown === null ? <Eye className="size-4" /> : <EyeOff className="size-4" />}
      </Button>
      {shown !== null && (
        <Button
          variant="ghost"
          size="icon"
          title="复制"
          onClick={() => void copyText(shown)}
        >
          <Copy className="size-4" />
        </Button>
      )}
      {error && (
        <span className="text-xs text-destructive" title={error.hint ?? undefined}>
          {error.message}
        </span>
      )}
    </span>
  );
}

const LABEL = { username: "账号", password: "密码", account: "账号" } as const;

function LedgerFormDialog({
  entity,
  id,
  envs,
  saving,
  onClose,
}: {
  entity: Entity;
  id: string | null;
  envs: LedgerEnv[];
  saving: boolean;
  onClose: () => void;
}) {
  const spec = FORMS[entity];
  const { data, add, patch } = useLedgerStore();
  const [form, setForm] = useState<Form>(() => initialForm(entity, id, data));
  const [error, setError] = useState<AppErrorShape | null>(null);
  const isEdit = id !== null;

  useEffect(() => {
    let alive = true;
    const secret = SECRETS[entity];
    if (!isEdit || !secret) return;
    const row = (entity === "credentials" ? data.credentials : data.servers).find((x) => x.id === id);
    if (!row) return;
    void (async () => {
      const filled: Form = {};
      for (const field of secret.fields) {
        // 读取接口只回 xxxSet 布尔，据此决定要不要为这一列发一次解密。
        if (row[`${field}Set` as keyof typeof row] !== true) continue;
        try {
          filled[field] = await ledgerReveal(secret.kind, id as string, field);
        } catch (e) {
          if (alive) setError(toAppError(e));
        }
      }
      if (alive) setForm((prev) => ({ ...prev, ...filled }));
    })();
    return () => {
      alive = false;
    };
  }, [entity, id, isEdit, data]);

  const patch_ = (key: string, value: string) => setForm((f) => ({ ...f, [key]: value }));

  const submit = async () => {
    setError(null);
    const missing = spec.fields.find((f) => f.required && !form[f.key]?.trim());
    if (missing) {
      setError({ code: "invalid_input", message: `「${missing.label}」不能为空`, hint: null });
      return;
    }
    const input = spec.build(form);
    const ok = isEdit ? await patch(entity, id as string, input) : await add(entity, input);
    if (ok) onClose();
    else setError(useLedgerStore.getState().error);
  };

  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {isEdit ? "编辑" : "新增"}
            {spec.title}
          </DialogTitle>
          <DialogDescription>
            {entity === "credentials" || entity === "servers"
              ? "账号与密码会加密后入库，明文只在解锁后按需解出单个字段。"
              : "这些字段是明文列，会被全文检索捞到；密码请填在加密字段里。"}
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-3">
          {spec.fields.map((f) => (
            <div key={f.key} className="grid gap-1.5">
              <Label className="text-xs text-muted-foreground">
                {f.label}
                {"required" in f && f.required && <span className="text-destructive"> *</span>}
              </Label>
              {f.kind === "textarea" ? (
                <textarea
                  rows={3}
                  className="w-full rounded-md border bg-background px-3 py-2 text-sm"
                  value={form[f.key] ?? ""}
                  onChange={(e) => patch_(f.key, e.target.value)}
                />
              ) : f.kind === "env" ? (
                <Select value={form[f.key] ?? ""} onValueChange={(v) => patch_(f.key, v)}>
                  <SelectTrigger>
                    <SelectValue placeholder="选择环境" />
                  </SelectTrigger>
                  <SelectContent>
                    {ENVS.map((env) => (
                      <SelectItem key={env} value={env}>
                        {ENV_LABELS[env]}（{env}）
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : f.kind === "envOf" ? (
                <Select value={form[f.key] ?? NONE} onValueChange={(v) => patch_(f.key, v)}>
                  <SelectTrigger>
                    <SelectValue placeholder="不绑定环境" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={NONE}>不绑定环境</SelectItem>
                    {envs.map((e) => (
                      <SelectItem key={e.id} value={e.id}>
                        {ENV_LABELS[e.env]} · {e.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : f.kind === "linkKind" ? (
                <Select value={form[f.key] ?? "other"} onValueChange={(v) => patch_(f.key, v)}>
                  <SelectTrigger>
                    <SelectValue placeholder="归类" />
                  </SelectTrigger>
                  <SelectContent>
                    {LINK_KINDS.map((k) => (
                      <SelectItem key={k} value={k}>
                        {LINK_LABELS[k]}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : (
                <Input
                  type={f.kind === "secret" ? "password" : "text"}
                  value={form[f.key] ?? ""}
                  onChange={(e) => patch_(f.key, e.target.value)}
                  autoComplete="off"
                />
              )}
            </div>
          ))}
        </div>

        {error && (
          <p className="text-sm text-destructive" title={error.hint ?? undefined}>
            {error.message}
            {error.hint ? `（${error.hint}）` : ""}
          </p>
        )}

        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            取消
          </Button>
          <Button onClick={() => void submit()} disabled={saving}>
            {saving ? "保存中…" : "保存"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function initialForm(entity: Entity, id: string | null, data: Ledger): Form {
  const row = id ? (data[entity] as { id: string }[]).find((x) => x.id === id) : undefined;
  const r = (row ?? {}) as Record<string, unknown>;
  const form: Form = {};
  for (const f of FORMS[entity].fields) {
    if (f.kind === "envOf") {
      form[f.key] = typeof r.envId === "string" ? r.envId : NONE;
    } else if (f.kind === "secret") {
      form[f.key] = ""; // 现值由解锁后的 reveal 异步填进来
    } else {
      const v = r[f.key];
      form[f.key] = typeof v === "string" ? v : "";
    }
  }
  if (entity === "links" && !form.kind) form.kind = "other";
  return form;
}
