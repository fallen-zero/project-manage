// 日报管理：跨项目的工作量填报（需求 思维导图.md:47-59）。汇总在上、明细在下（spec §七）。
// 数据一次拉全（listDailyReports(null) + listProjects()）：沿用待办页口径 —— 筛选、排序、聚合都在前端，
// 比按视图来回打后端简单也更省往返（`daily_report_list` 不带 LIMIT，先例 `todo::list`）。
import { useCallback, useEffect, useMemo, useState } from "react";
import { Copy, Plus, Trash2 } from "lucide-react";
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
import { Separator } from "@/components/ui/separator";
import {
  createDailyReport,
  deleteDailyReport,
  exportDailyReports,
  importDailyReportsApply,
  importDailyReportsPreview,
  listDailyReports,
  listProjects,
  pickXlsxOpenPath,
  pickXlsxSavePath,
  updateDailyReport,
} from "@/lib/api";
import { localToday, projectLabel } from "@/lib/due-warning";
import {
  previousReportDay,
  scopeDailyReports,
  scopedHoursSummary,
  type HoursBucket,
  type HoursFilter,
} from "@/lib/hours-summary";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import {
  dateOrNull,
  type DailyReport,
  type DailyReportEdit,
  type DailyReportExportRow,
  type DailyReportInput,
  type ImportPreview,
} from "@/types/daily-report";
import type { Project } from "@/types/project";

const ALL_PROJECTS = "__all__";

const BUCKETS: { key: HoursBucket; label: string }[] = [
  { key: "day", label: "按日" },
  { key: "week", label: "按周" },
  { key: "month", label: "按月" },
];

export function DailyReportsPage() {
  const [reports, setReports] = useState<DailyReport[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // 一份筛选同时喂汇总与明细：`scopedHoursSummary` 内部先缩再聚，页面没有第二份比较可写错（spec §七）。
  const [filterProject, setFilterProject] = useState(ALL_PROJECTS);
  const [filterFrom, setFilterFrom] = useState("");
  const [filterTo, setFilterTo] = useState("");
  const [keyword, setKeyword] = useState("");
  const [bucket, setBucket] = useState<HoursBucket>("week");

  const [newDate, setNewDate] = useState(() => localToday());
  const [newProject, setNewProject] = useState("");
  const [newContent, setNewContent] = useState("");
  const [newHours, setNewHours] = useState("");

  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftDate, setDraftDate] = useState("");
  const [draftProject, setDraftProject] = useState("");
  const [draftContent, setDraftContent] = useState("");
  const [draftHours, setDraftHours] = useState("");

  // 导入两段式（Task 7，D5）：前端只存**文件路径**与后端算回来的预检结果。
  // 预检的行不回传给确认那一步 —— apply 在后端重新解析同一个路径，前端没有第二份判据可漂。
  const [importPath, setImportPath] = useState<string | null>(null);
  const [previewData, setPreviewData] = useState<ImportPreview | null>(null);

  const reload = useCallback(async () => {
    try {
      const [rows, ps] = await Promise.all([listDailyReports(null), listProjects()]);
      setReports(rows);
      setProjects(ps);
      setError(null);
    } catch (e) {
      setError(toAppError(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const labelOf = useMemo(() => {
    // 显示名只有一份形状（due-warning 的 projectLabel，有编号时是「名（编号）」），这里不重拼。
    const byId = new Map(projects.map((p) => [p.id, projectLabel(p)]));
    return (id: string) => byId.get(id) ?? "（项目缺失）";
  }, [projects]);

  const filter = useMemo<HoursFilter>(
    () => ({
      projectId: filterProject === ALL_PROJECTS ? null : filterProject,
      from: filterFrom.trim(),
      to: filterTo.trim(),
      keyword,
    }),
    [filterProject, filterFrom, filterTo, keyword],
  );
  const summary = useMemo(
    () => scopedHoursSummary(reports, filter, bucket, projects),
    [reports, filter, bucket, projects],
  );
  const visible = useMemo(() => scopeDailyReports(reports, filter), [reports, filter]);

  // 复制上一天的源日期：最近一个有日报的那天（D10）。null ⇒ 按钮禁用。
  const targetDate = dateOrNull(newDate) ?? localToday();
  const copySource = useMemo(() => previousReportDay(reports, targetDate), [reports, targetDate]);

  // run 必须回报成败：它把异常吞进 state、promise 永不 reject，任何 `.then(清理)` 都会无条件执行
  // —— M9 的 I3（失败的新增照样清空输入、连点两次插重复行）就是这么来的。调用方一律 `if (ok)` 再清理。
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

  // `Number("abc")` 是 NaN，而 `JSON.stringify(NaN)` 出 `null` ⇒ IPC 层会报「参数不合法」那种看不懂的错。
  // 折成 0 让它走进后端 `check_hours`（0 不合法），报出来就点名「工时」—— 判据仍然只在后端一处。
  const hoursOf = (raw: string): number => {
    const n = Number(raw.trim());
    return Number.isFinite(n) ? n : 0;
  };

  const onAdd = () => {
    if (newProject === "") {
      // 这不算「第二份校验」，而是「别把一个空 id 发出去」：`projectId: ""` 到后端报的是
      // `not_found()`，用户看不出是自己没选项目。（`AppErrorShape.hint` 是 `string | null`，不是可选键。）
      setError({ code: "invalid_input", message: "要先选所属项目", hint: "日报必须挂一个项目" });
      return;
    }
    const date = dateOrNull(newDate);
    const input: DailyReportInput = {
      projectId: newProject,
      reportDate: date,
      content: newContent,
      hours: hoursOf(newHours),
    };
    // 只清内容与工时：日期与项目是「正在填的上下文」，留着才连得上 §三 的连续填报。
    // 成功回执写在 `run` 的 fn 里 ⇒ create 抛错时根本走不到这几行（M9 I3 的规矩）。
    void run(async () => {
      await createDailyReport(input);
      setNewContent("");
      setNewHours("");
      setNotice(`已记一条 ${date ?? "（日期没填，这条会被后端拒）"} 的日报`);
    });
  };

  const onCopyPreviousDay = () => {
    const source = copySource;
    if (source === null) return; // 按钮此时 disabled，这一句只是防御
    const src = reports.filter((r) => r.reportDate === source);
    void (async () => {
      // **逐条走 `run()`，不走「一个 run 里循环」**：`run()` 只在成功路径里 `await reload()`，
      // 循环版一旦中途被后端拒绝（或落库后 reload 自己失败），已经进库的那几条就**不会回到列表里** ——
      // 用户看不见它们，既没法按 D10 说的「逐条删」撤销，也会再点一次「复制上一天」把同一批**再复制一遍**。
      // Task 5 复审抓的就是这一条（单条写入的 `todos.tsx` 不会摊上这事，多条循环才会）。
      // 代价是 N 次 reload，几十条量级无所谓；`copied` 只在自己 resolve 之后才加，所以不会多报。
      let copied = 0;
      for (const r of src) {
        const ok = await run(() =>
          createDailyReport({
            projectId: r.projectId,
            reportDate: targetDate,
            content: r.content,
            hours: r.hours,
          }),
        );
        if (!ok) break; // 失败已由 run() 落进 error，且前面成功的几条已经被 run() 内部的 reload 拉回列表
        copied += 1;
      }
      if (copied > 0) setNotice(`从 ${source} 复制了 ${copied} 条到 ${targetDate}`);
    })();
  };

  const projectById = useMemo(() => new Map(projects.map((p) => [p.id, p])), [projects]);

  // 导出**所见即所得**：`visible` 是筛过的行（spec §六.1，命令不收查询条件）。
  // 名称列写 `p.name` 而不是界面上的 label —— 它是导入匹配的对象。
  const onExport = (templateOnly: boolean) => {
    const defaultName = templateOnly ? "日报模板.xlsx" : `日报-${localToday().replace(/-/g, "")}.xlsx`;
    void (async () => {
      const path = await pickXlsxSavePath(defaultName);
      if (path === null) return; // 取消不是错误：绝不写进 error（M8 的同一告诫）
      const rows: DailyReportExportRow[] = (templateOnly ? [] : visible).map((r) => {
        const p = projectById.get(r.projectId);
        return {
          reportDate: r.reportDate,
          projectCode: p?.code ?? null,
          projectName: p?.name ?? "（项目缺失）",
          content: r.content,
          hours: r.hours,
        };
      });
      const ok = await run(() => exportDailyReports(path, rows));
      if (ok) setNotice(`已导出 ${rows.length} 条到 ${path}`);
    })();
  };

  const onPickImport = async () => {
    const path = await pickXlsxOpenPath();
    if (path === null) return; // 取消不是错误
    setPreviewData(null);
    const ok = await run(async () => setPreviewData(await importDailyReportsPreview(path)));
    if (ok) setImportPath(path);
    else setImportPath(null);
  };

  const onApplyImport = () => {
    const path = importPath;
    if (path === null) return;
    void run(async () => {
      const out = await importDailyReportsApply(path);
      setNotice(`已导入 ${out.inserted} 条`);
      setPreviewData(null);
      setImportPath(null);
    });
  };

  const onStartEdit = (r: DailyReport) => {
    setEditingId(r.id);
    setDraftDate(r.reportDate);
    setDraftProject(r.projectId);
    setDraftContent(r.content);
    setDraftHours(String(r.hours));
    setNotice(null);
  };

  const onSaveEdit = (id: string) => {
    const edit: DailyReportEdit = {
      projectId: draftProject,
      reportDate: dateOrNull(draftDate),
      content: draftContent,
      hours: hoursOf(draftHours),
    };
    void run(async () => {
      await updateDailyReport(id, edit);
      setEditingId(null); // 只有成功才退出编辑态（M9 I3 同规）
    });
  };

  const onDelete = (r: DailyReport) => {
    void run(() => deleteDailyReport(r.id));
  };

  return (
    <div className="space-y-4">
      <section className="rounded-xl border bg-background p-4">
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <h2 className="text-sm font-medium">日报</h2>
          <span className="text-xs text-muted-foreground">
            共 {reports.length} 条 · 当前范围 {summary.count} 条 / {summary.total} 小时
          </span>
          <div className="ml-auto flex items-center gap-2">
            {BUCKETS.map((b) => (
              <Button
                key={b.key}
                size="sm"
                variant={bucket === b.key ? "default" : "outline"}
                onClick={() => setBucket(b.key)}
              >
                {b.label}
              </Button>
            ))}
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Select value={filterProject} onValueChange={setFilterProject}>
            <SelectTrigger className="w-48">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL_PROJECTS}>全部项目</SelectItem>
              {projects.map((p) => (
                <SelectItem key={p.id} value={p.id}>
                  {labelOf(p.id)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Input
            className="w-36"
            type="date"
            value={filterFrom}
            onChange={(e) => setFilterFrom(e.target.value)}
            aria-label="起始日期"
          />
          <Input
            className="w-36"
            type="date"
            value={filterTo}
            onChange={(e) => setFilterTo(e.target.value)}
            aria-label="结束日期"
          />
          <Input
            className="w-48"
            placeholder="工作内容关键词"
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
          />
        </div>
        {summary.unparsed > 0 && (
          <p className="mt-2 text-xs text-destructive">
            {summary.unparsed} 条日期不是 YYYY-MM-DD，没有进汇总（界面上仍能在明细里看到它们）
          </p>
        )}
      </section>

      {error && (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}
      {notice && !error && (
        <div className="rounded-lg border bg-muted px-3 py-2 text-xs text-muted-foreground">{notice}</div>
      )}

      {previewData && (
        <section className="rounded-xl border bg-background p-4">
          <h2 className="text-sm font-medium">导入预检</h2>
          <p className="mt-1 text-xs text-muted-foreground">
            {previewData.total} 行里 {previewData.ok} 行可导入 · {previewData.rejected.length} 行不合格
            {previewData.duplicates > 0 && ` · ${previewData.duplicates} 行与库里已有记录完全相同`}
            {previewData.truncatedLines.length > 0 &&
              ` · ${previewData.truncatedLines.length} 行的日期带时分秒，将截到「日」`}
          </p>
          {previewData.rejected.length > 0 && (
            <ul className="mt-2 space-y-1 text-xs">
              {previewData.rejected.map((r) => (
                <li key={`${r.line}-${r.field}`}>
                  <span className="font-mono">第 {r.line} 行</span> · {r.field}：{r.reason}
                </li>
              ))}
            </ul>
          )}
          <div className="mt-3 flex items-center gap-2">
            <Button size="sm" disabled={busy || previewData.ok === 0} onClick={onApplyImport}>
              确认导入 {previewData.ok} 条
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                setPreviewData(null);
                setImportPath(null);
              }}
            >
              取消
            </Button>
            <span className="text-xs text-muted-foreground">{importPath}</span>
          </div>
          <p className="mt-2 text-xs text-muted-foreground">
            任何一行不合格 ⇒ 整批不写。确认时后端会重新读一遍这个文件，所以改过的内容按新的算。
          </p>
        </section>
      )}

      <section className="rounded-xl border bg-background p-4">
        <h2 className="text-sm font-medium">工时汇总</h2>
        <table className="mt-2 w-full text-sm">
          <thead className="text-left text-xs text-muted-foreground">
            <tr>
              <th className="py-1">{BUCKETS.find((b) => b.key === bucket)?.label}</th>
              <th className="py-1">条目数</th>
              <th className="py-1">小时</th>
            </tr>
          </thead>
          <tbody>
            {summary.buckets.length === 0 ? (
              <tr>
                <td colSpan={3} className="py-2 text-muted-foreground">
                  当前范围里没有可汇总的日报。
                </td>
              </tr>
            ) : (
              summary.buckets.map((b) => (
                <tr key={b.key} className="border-t">
                  <td className="py-1 font-mono text-xs">{b.key}</td>
                  <td className="py-1">{b.count}</td>
                  <td className="py-1">{b.hours}</td>
                </tr>
              ))
            )}
          </tbody>
        </table>
        <Separator className="my-3" />
        <h3 className="text-sm font-medium">按项目</h3>
        <ul className="mt-2 space-y-1">
          {summary.byProject.length === 0 ? (
            <li className="text-sm text-muted-foreground">当前范围里没有可汇总的日报。</li>
          ) : (
            summary.byProject.map((p) => (
              <li key={p.projectId} className="flex items-center gap-2 text-sm">
                <Badge variant="outline">{p.hours} h</Badge>
                <span className="min-w-0 flex-1 truncate">{p.projectLabel}</span>
                <span className="text-xs text-muted-foreground">{p.count} 条</span>
              </li>
            ))
          )}
        </ul>
      </section>

      <section className="rounded-xl border bg-background p-4">
        <h2 className="text-sm font-medium">明细</h2>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <Input
            className="w-36"
            type="date"
            value={newDate}
            onChange={(e) => setNewDate(e.target.value)}
            aria-label="日报日期"
          />
          <Select value={newProject} onValueChange={setNewProject}>
            <SelectTrigger className="w-48">
              <SelectValue placeholder="所属项目" />
            </SelectTrigger>
            <SelectContent>
              {projects.map((p) => (
                <SelectItem key={p.id} value={p.id}>
                  {labelOf(p.id)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Input
            className="min-w-48 flex-1"
            placeholder="工作内容"
            value={newContent}
            onChange={(e) => setNewContent(e.target.value)}
          />
          <Input
            className="w-24"
            inputMode="decimal"
            placeholder="小时"
            value={newHours}
            onChange={(e) => setNewHours(e.target.value)}
            aria-label="工时"
          />
          <Button size="sm" disabled={busy || newProject === ""} onClick={onAdd}>
            <Plus className="mr-1 size-4" />
            新增
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={busy || copySource === null}
            onClick={onCopyPreviousDay}
            title={copySource === null ? "之前还没有任何一天记过日报" : `复制 ${copySource}`}
          >
            <Copy className="mr-1 size-4" />
            复制上一天
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => onExport(false)}>
            导出当前范围
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => onExport(true)}>
            下载模板
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void onPickImport()}>
            从 Excel 导入
          </Button>
        </div>

        <ul className="mt-3 space-y-1">
          {visible.length === 0 ? (
            <li className="py-2 text-sm text-muted-foreground">
              {reports.length === 0 ? "还没有日报。" : "当前筛选条件下没有日报。"}
            </li>
          ) : (
            visible.map((r) => {
              const editing = editingId === r.id;
              return (
                <li key={r.id} className="flex items-center gap-2 border-b py-2 last:border-b-0">
                  {editing ? (
                    <>
                      <Input
                        className="w-36"
                        type="date"
                        value={draftDate}
                        onChange={(e) => setDraftDate(e.target.value)}
                        aria-label="日报日期"
                      />
                      <Select value={draftProject} onValueChange={setDraftProject}>
                        <SelectTrigger className="w-48">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {projects.map((p) => (
                            <SelectItem key={p.id} value={p.id}>
                              {labelOf(p.id)}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                      <Input
                        className="min-w-48 flex-1"
                        value={draftContent}
                        onChange={(e) => setDraftContent(e.target.value)}
                      />
                      <Input
                        className="w-24"
                        inputMode="decimal"
                        value={draftHours}
                        onChange={(e) => setDraftHours(e.target.value)}
                        aria-label="工时"
                      />
                      <Button size="sm" disabled={busy} onClick={() => onSaveEdit(r.id)}>
                        保存
                      </Button>
                      <Button size="sm" variant="outline" onClick={() => setEditingId(null)}>
                        取消
                      </Button>
                    </>
                  ) : (
                    <>
                      <span className="font-mono text-xs text-muted-foreground">{r.reportDate}</span>
                      <Badge variant="outline">{r.hours} h</Badge>
                      <span className="min-w-0 flex-1 truncate">{r.content}</span>
                      <span className="max-w-40 truncate text-xs text-muted-foreground">
                        {labelOf(r.projectId)}
                      </span>
                      <Button size="sm" variant="ghost" disabled={busy} onClick={() => onStartEdit(r)}>
                        编辑
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={busy}
                        onClick={() => onDelete(r)}
                        aria-label="删除这条日报"
                      >
                        <Trash2 className="size-4" />
                      </Button>
                    </>
                  )}
                </li>
              );
            })
          )}
        </ul>
      </section>
    </div>
  );
}
