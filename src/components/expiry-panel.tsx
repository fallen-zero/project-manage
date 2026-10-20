// 到期事项（需求 思维导图.md:73「维保、域名、证书到期提醒」+ spec D7 当次加的「其他」）。
// 与里程碑面板差在两处：① 没有 done 勾选态（D2：续期 = 改日期，不留勾选态）；
// ② 行上提供「改日期 / 改备注」两件事，因为续期是本表的主操作 —— 种类与名称录错走删了重建，
// 所以这里**不出现**改 kind/label 的入口（spec §二 自拍①，ExpiryUpdate 里也没有那两个字段）。
import { useCallback, useEffect, useState } from "react";
import { Plus, Save, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { createExpiry, deleteExpiry, listExpiries, updateExpiry } from "@/lib/api";
import { dueStateOf, horizonOf, localToday, type DueState } from "@/lib/due-warning";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { EXPIRY_KINDS, type Expiry } from "@/types/expiry";

// 下拉默认值取词表第一格：四类只在 `EXPIRY_KINDS` 一份，面板里不许再抄一遍（三处同源由 lib.rs 那条文本门钉）。
const KIND = EXPIRY_KINDS[0];

export function ExpiryPanel({ projectId }: { projectId: string }) {
  const [rows, setRows] = useState<Expiry[]>([]);
  const [kind, setKind] = useState<string>(KIND);
  const [label, setLabel] = useState("");
  const [expireDate, setExpireDate] = useState("");
  const [note, setNote] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftDate, setDraftDate] = useState("");
  const [draftNote, setDraftNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);

  const reload = useCallback(async () => {
    try {
      setRows(await listExpiries(projectId));
      setError(null);
    } catch (e) {
      setError(toAppError(e));
    }
  }, [projectId]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // run 的契约与 milestone-panel/todos 页同一份：吞异常进 state、回报成败，
  // 所以「成功后才清输入框」必须靠 `if (!ok) return`，不能挂在 .then 上无条件执行。
  const run = async (fn: () => Promise<unknown>): Promise<boolean> => {
    setBusy(true);
    try {
      await fn();
      await reload();
      return true;
    } catch (e) {
      setError(toAppError(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const today = localToday();
  // 与预警块**同一份**判据与同一个窗口（`horizonOf("expiry")` = 30 天）：面板自己不许再写一份日期比较。
  const stateOf = (date: string): { state: DueState; days: number } | null =>
    dueStateOf(date, today, horizonOf("expiry"));

  return (
    <div>
      <div className="mb-1 flex items-center justify-between">
        <h2 className="text-sm font-medium">到期事项</h2>
        <span className="text-xs text-muted-foreground">
          种类固定四类（维保/域名/证书/其他）；续期就改日期，不再生效就删掉。天数徽标与预警块同判据同两档色：红=已逾期，描边=今天起 30 天内
        </span>
      </div>

      {error && (
        <div className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}

      <div className="mb-3 flex flex-wrap items-center gap-2">
        <Select value={kind} onValueChange={setKind}>
          <SelectTrigger className="w-28">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {EXPIRY_KINDS.map((k) => (
              <SelectItem key={k} value={k}>
                {k}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Input
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          placeholder="名称（如 oa.gov.cn、年度维保合同）"
          className="min-w-40 flex-1"
        />
        {/* 到期日的空值不在面板里折 NULL：`req_date` 要收到 "" 才能回一句点名「到期日期」的 invalid_input，
            所以这里原样传字符串（后端 Option<String>，空串与缺键同样拒）。 */}
        <Input type="date" value={expireDate} onChange={(e) => setExpireDate(e.target.value)} className="w-44" />
        <Input
          value={note}
          onChange={(e) => setNote(e.target.value)}
          placeholder="备注（可留空）"
          className="min-w-40 flex-1"
        />
        <Button
          size="sm"
          disabled={busy}
          onClick={() =>
            void run(() => createExpiry({ projectId, kind, label: label.trim(), expireDate, note: note.trim() }))
              .then((ok) => {
                if (!ok) return; // 被后端拒了就留着用户刚敲的内容（同 milestone-panel 终审 I3 那条契约）
                setLabel("");
                setExpireDate("");
                setNote("");
              })
          }
        >
          <Plus /> 记一笔
        </Button>
      </div>

      {rows.length === 0 && <p className="py-4 text-center text-sm text-muted-foreground">还没有到期事项。</p>}

      {rows.map((e) => {
        const hit = stateOf(e.expireDate);
        const editing = editingId === e.id;
        return (
          <div key={e.id} className="flex flex-wrap items-center gap-2 border-b px-1 py-2 last:border-b-0">
            <Badge variant="secondary">{e.kind}</Badge>
            {/* 第二枚徽标念「距今多少天」（spec §七），两档色沿用预警块那一份写法（`todos.tsx` 行里同一串：
                逾期红、今天与 30 天内描边）。`hit === null` 只可能是库里躺着非 YYYY-MM-DD 的历史值 ——
                后端的 `req_date` 只挡新写入，整库快照导入进来的旧包不重过这道门，所以给它第三档、不染红不描边。 */}
            <Badge
              variant={
                hit?.state === "overdue" ? "destructive" : hit?.state === "soon" ? "outline" : "secondary"
              }
            >
              {hit === null
                ? "形状未知"
                : hit.state === "overdue"
                  ? `逾期 ${-hit.days} 天`
                  : hit.days === 0
                    ? "今天"
                    : `${hit.days} 天后`}
            </Badge>
            <span className="min-w-0 flex-1 truncate text-sm" title={e.label}>
              {e.label}
            </span>
            {editing ? (
              <>
                <Input type="date" value={draftDate} onChange={(ev) => setDraftDate(ev.target.value)} className="w-44" />
                <Input value={draftNote} onChange={(ev) => setDraftNote(ev.target.value)} placeholder="备注" className="min-w-32 flex-1" />
                <Button
                  size="sm"
                  disabled={busy}
                  onClick={() =>
                    void run(() => updateExpiry(e.id, { expireDate: draftDate, note: draftNote.trim() })).then((ok) => {
                      if (ok) setEditingId(null);
                    })
                  }
                >
                  <Save /> 存
                </Button>
                <Button size="sm" variant="ghost" disabled={busy} onClick={() => setEditingId(null)}>
                  取消
                </Button>
              </>
            ) : (
              <>
                <span className="w-24 shrink-0 font-mono text-xs text-muted-foreground">{e.expireDate}</span>
                <span className="min-w-0 max-w-40 truncate text-xs text-muted-foreground" title={e.note}>
                  {e.note}
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => {
                    setEditingId(e.id);
                    setDraftDate(e.expireDate);
                    setDraftNote(e.note);
                  }}
                >
                  续期/改备注
                </Button>
                <Button size="sm" variant="ghost" disabled={busy} onClick={() => void run(() => deleteExpiry(e.id))}>
                  <Trash2 />
                </Button>
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
