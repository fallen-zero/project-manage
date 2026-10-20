// 项目里程碑（需求 思维导图.md:33-36：立项、评审、联调、上线、验收，可记录日期、备注）。
// done 那一列是 spec D5 加的：立项/评审的计划日期天然在过去，没勾选态就会把预警刷成一屏误红。
import { useCallback, useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
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
import { createMilestone, deleteMilestone, listMilestones, updateMilestone } from "@/lib/api";
import { dueStateOf, localToday, DUE_HORIZON_DAYS, type DueState } from "@/lib/due-warning";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { MILESTONE_STAGES, type Milestone } from "@/types/milestone";

// 与 todos 页同形的 dateOrNull：两格逻辑一样但文件不同，不为两行代码跨模块引用来引用过去。
// 后端 norm_date 会把空白折成 NULL（不是拒收空串）；这里显式折一次，为了不把 "" 送进请求。
const dateOrNull = (raw: string): string | null => (raw.trim() === "" ? null : raw.trim());

export function MilestonePanel({ projectId }: { projectId: string }) {
  const [rows, setRows] = useState<Milestone[]>([]);
  const [stage, setStage] = useState<string>(MILESTONE_STAGES[0]);
  const [planDate, setPlanDate] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);

  const reload = useCallback(async () => {
    try {
      setRows(await listMilestones(projectId));
      setError(null);
    } catch (e) {
      setError(toAppError(e));
    }
  }, [projectId]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // run 必须回报成败：它把异常吞进 state、promise 永不 reject，
  // 所以任何 `.then(清理)` 都会无条件执行 —— 与 todos.tsx 的同名 helper 保持同一份契约（T5 评审据此打回两条 Important）。
  // 终审 I3 给了它第一个真正的消费点：记一笔**成功**之后才清 planDate/note，
  // 失败时留着用户刚敲的内容（原先连点两次「记一笔」会插重复行，因为输入框从来没清过）。
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

  // 与预警块**同一份**单行判据（终审 I2 抽成 `dueStateOf` 之前这里是第二份实现：字符串比较 + 自己的 shiftDate，
  // 把 `<` 写成 `<=` 谁也不会红）。面板手里只有本项目的里程碑，所以只判「这一行自己」是不是命中，
  // 不去跑三来源汇总 —— 共享的是判据本身，不是汇总。
  const today = localToday();
  const stateOf = (m: Milestone): DueState | null => {
    if (m.done || m.planDate === null) return null;
    return dueStateOf(m.planDate, today)?.state ?? null;
  };

  return (
    <div>
      <div className="mb-1 flex items-center justify-between">
        <h2 className="text-sm font-medium">项目里程碑</h2>
        <span className="text-xs text-muted-foreground">
          阶段固定五段；勾掉=已完成，不再进预警。徽标与预警块同判据：红=已逾期，描边=今天起 {DUE_HORIZON_DAYS} 天内
        </span>
      </div>

      {error && (
        <div className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}

      <div className="mb-3 flex flex-wrap items-center gap-2">
        <Select value={stage} onValueChange={setStage}>
          <SelectTrigger className="w-32">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {MILESTONE_STAGES.map((s) => (
              <SelectItem key={s} value={s}>
                {s}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Input type="date" value={planDate} onChange={(e) => setPlanDate(e.target.value)} className="w-44" />
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
            void run(() =>
              createMilestone({ projectId, stage, planDate: dateOrNull(planDate), note: note.trim() === "" ? null : note.trim() }),
            ).then((ok) => {
              if (!ok) return; // 被后端拒了就留着用户刚敲的日期与备注（终审 I3）
              setPlanDate("");
              setNote("");
            })
          }
        >
          <Plus /> 记一笔
        </Button>
      </div>

      {rows.length === 0 && <p className="py-4 text-center text-sm text-muted-foreground">还没有里程碑记录。</p>}

      {rows.map((m) => {
        const hit = stateOf(m);
        return (
          <div key={m.id} className="flex items-center gap-2 border-b px-1 py-2 last:border-b-0">
            <input
              type="checkbox"
              className="size-4 accent-primary"
              checked={m.done}
              disabled={busy}
              onChange={() => void run(() => updateMilestone(m.id, { stage: m.stage, planDate: m.planDate, note: m.note, done: !m.done }))}
            />
            <Badge variant={hit === "overdue" ? "destructive" : hit === "soon" ? "outline" : "secondary"}>
              {m.stage}
            </Badge>
            <span className="w-24 shrink-0 font-mono text-xs text-muted-foreground">{m.planDate ?? "无日期"}</span>
            <span className="min-w-0 flex-1 truncate text-sm">{m.note}</span>
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => void run(() => deleteMilestone(m.id))}>
              <Trash2 />
            </Button>
          </div>
        );
      })}
    </div>
  );
}
