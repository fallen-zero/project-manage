import { useEffect, useRef, useState } from "react";
import { Link } from "@tanstack/react-router";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { indexDocs, searchDocs } from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { isTerminal, useIndexJobStore } from "@/stores/index-job";
import type { DocHit, DocRow, IndexProgress } from "@/types/index";

// 一次取这么多行；Rust 侧 index_docs 的 limit 会被 clamp 到 1..500，取满就要提示「还有下一页」。
const DOCS_LIMIT = 200;

// skipReason → 中文：这一层回答的是「为什么这个文件搜不到」。
// 有写入方的只有这三档（index_job.rs 的 too_large / type_unsupported，write_doc 的 empty_text）。
// 刻意不给 over_project_cap 留映射：触顶被丢弃的文件根本不建行（scan_root 在 break 之后什么都不返回），
// 留着映射等于告诉用户「这里本该有一行」，是假的可观测性 —— 上限这件事由项目级 capped 标注回答。
const REASON_LABEL: Record<string, string> = {
  too_large: "超出单文件上限",
  empty_text: "抽取到的正文为空（扫描件没文字层属正常，不是失败）",
  type_unsupported: "类型在支持清单内，但当前版本没有对应的正文抽取器",
};

// 未认识的原因原样念出来，不静默吞掉：新加一档跳过原因时界面上至少还有个看得见的英文键。
const reasonLabel = (r: string) => REASON_LABEL[r] || r;

const STATE_LABEL: Record<IndexProgress["state"], string> = {
  running: "进行中",
  done: "已完成",
  cancelled: "已取消",
  error: "失败",
};

// 状态筛选下拉的取值："" 代表全部，api 侧传 null，Rust 再按 `?2 IS NULL` 放开条件。
const STATUS_FILTERS: { value: string; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "ok", label: "ok（已入索引）" },
  { value: "skipped", label: "skipped（跳过）" },
  { value: "failed", label: "failed（失败）" },
];

function fmtBytes(n: number): string {
  if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${n} B`;
}

export function IndexStatusPage() {
  const overview = useIndexJobStore((s) => s.overview);
  const progress = useIndexJobStore((s) => s.progress);
  const error = useIndexJobStore((s) => s.error);
  const load = useIndexJobStore((s) => s.load);
  const start = useIndexJobStore((s) => s.start);
  const cancel = useIndexJobStore((s) => s.cancel);
  const subscribe = useIndexJobStore((s) => s.subscribe);

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [statusFilter, setStatusFilter] = useState("all");
  const [docs, setDocs] = useState<DocRow[]>([]);
  // 这批行属于哪个项目：切换项目时旧行会闪一下，所以只在归属相符时才渲染。
  const [docsFor, setDocsFor] = useState<string | null>(null);
  const [docsBusy, setDocsBusy] = useState(false);
  const [docsError, setDocsError] = useState<AppErrorShape | null>(null);
  const [docsReload, setDocsReload] = useState(0);

  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<DocHit[]>([]);
  const [searchBusy, setSearchBusy] = useState(false);
  const [searched, setSearched] = useState(false);
  const [searchError, setSearchError] = useState<AppErrorShape | null>(null);

  const [cancelRequested, setCancelRequested] = useState(false);
  // error 终止事件不带计数（report_failure 发的是 terminal("error", &[], …)），
  // 「跑到第几个项目挂的」只能从失败前最后一条 running 事件里取，所以把它单独记住。
  const [lastRunning, setLastRunning] = useState<{ projectName: string; current: string } | null>(
    null,
  );
  // 订阅本身失败的原因（listen 在非 Tauri 环境会 reject）：与「这一次点击没被受理」「上一轮挂了」并列的第三条来源。
  const [subscribeError, setSubscribeError] = useState<AppErrorShape | null>(null);
  const terminalSeen = useRef<IndexProgress | null>(null);

  // 生命周期：挂载先取总览（running 的真值只在 IPC 里），再订阅进度；卸载一定 unlisten。
  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | null = null;
    void load();
    void subscribe()
      .then((fn) => {
        if (alive) unlisten = fn;
        else fn(); // 订阅回来时页面已卸载，当场退订，不留悬空监听
      })
      // 与本页其余异步路径同一口径记 toAppError：吞掉 reject 就等于「进度条一动不动，界面上半点提示都没有」，
      // 而那正是本页最难自查的失效形态（浏览器里直接 npm run dev 时没有 __TAURI_INTERNALS__，listen 必拒）。
      .catch((e) => {
        if (alive) setSubscribeError(toAppError(e));
      });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [load, subscribe]);

  // 收到终态事件后再拉一次总览：lastRun / lastError 只有 IPC 里有，
  // 少了这一步，界面上「这一轮挂了」和「为什么挂」就分家。
  // 用 ref 比对事件对象本身，保证同一条终态事件只刷一次、下一条终态事件仍会刷。
  useEffect(() => {
    if (!isTerminal(progress)) return;
    if (terminalSeen.current === progress) return;
    terminalSeen.current = progress;
    setCancelRequested(false);
    setDocsReload((n) => n + 1); // 清单里该出现新的行了
    void load();
  }, [progress, load]);

  useEffect(() => {
    if (progress && progress.state === "running") {
      setLastRunning({ projectName: progress.projectName, current: progress.current });
    }
  }, [progress]);

  // 选中项目后取文件行；status 走参数（Rust 侧不拼 SQL）。
  useEffect(() => {
    if (!selectedId) return;
    let alive = true;
    setDocsBusy(true);
    void indexDocs(selectedId, statusFilter === "all" ? null : statusFilter, DOCS_LIMIT, 0)
      .then((rows) => {
        if (!alive) return;
        setDocs(rows);
        setDocsFor(selectedId);
        setDocsError(null);
      })
      .catch((e) => {
        if (alive) setDocsError(toAppError(e));
      })
      .finally(() => {
        if (alive) setDocsBusy(false);
      });
    return () => {
      alive = false;
    };
  }, [selectedId, statusFilter, docsReload]);

  const running = overview?.running ?? false;
  // 总数取自当前项目的 scannedTotal，扫描没结束前是 0 —— 这时说「正在扫描…」，不画假百分比。
  const scanning = progress !== null && progress.state === "running" && progress.total === 0;
  const pct =
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.done / progress.total) * 100))
      : null;

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-4 px-6 py-8">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="flex items-center gap-2 text-xl font-semibold">
            全文索引
            <Badge variant={running ? "secondary" : "outline"}>
              {running ? "索引进行中" : "空闲"}
            </Badge>
          </h1>
          <p className="mt-1 text-xs text-muted-foreground">
            只读取文件内容建索引：不创建、不移动、不改名、不改写任何被索引的原始文件。
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            disabled={running}
            onClick={() => {
              setCancelRequested(false);
              // 发 IPC 之前先复位进度：store 的 progress 只在事件回调里写、从不清，页面也不清的话，
              // 点击到下一条 running 事件之间（PROGRESS_EVERY = 20，第 20 个已处理文件才回传）进度卡会
              // 继续念上一轮的终态；更坏的是新轮在发出任何 running 事件前就失败，此时 lastRunning 还是
              // 上一轮的项目名与文件，「失败发生在项目…」会把诊断指到别的轮次去。
              setLastRunning(null);
              useIndexJobStore.setState({ progress: null });
              // IPC 一落地就再取一次总览。running 的唯一口径是 overview.running，而 load() 原本只在挂载与
              // 终态事件两处跑，少这一步的话：从本页发起的那一整轮里 overview.running 全程停在 false ⇒
              // 徽章念「空闲」、两个 start 按钮一直可点，而 取消 是 disabled={!running} —— 这次挂载里永远点不动，
              // 用户从本页发起的作业没法从本页取消。这不是赌时序：index_job.rs:335 的
              // running.swap(true, SeqCst) 在 spawn 之前、命令返回之前就完成，所以 `.then` 里读到的是 true；
              // 轮次极短、线程已经跑完时读到 false 也是对的。被 index_running 拒绝时 store 的 start 把错误
              // 收进 error、promise 照样 resolve，此时真值本来就是 true ⇒ 两个 start 按钮随即变灰，
              // 也就没机会再点一次、把上面那行复位打在正在显示的进度上。
              void start(undefined, false).then(() => load());
            }}
          >
            建立索引
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={running}
            onClick={() => {
              setCancelRequested(false);
              // 同上：全量重建也要先复位，否则本轮头几秒念的是上一轮的数字。
              setLastRunning(null);
              useIndexJobStore.setState({ progress: null });
              // 同上：start 落地后补一次 load()，让 overview.running 回到真值，徽章与取消按钮才跟得上这一轮。
              void start(undefined, true).then(() => load());
            }}
          >
            全量重建
          </Button>
          <Button
            size="sm"
            variant="destructive"
            disabled={!running || cancelRequested}
            onClick={() => {
              setCancelRequested(true);
              void cancel();
            }}
          >
            {cancelRequested ? "已请求取消" : "取消"}
          </Button>
        </div>
      </div>

      {/* 两条错误来源分开摆：下面这块是「这一次点击没被受理」（IPC 拒绝，有 code 可分支）。 */}
      {error &&
        (error.code === "index_running" ? (
          <div className="rounded-lg border border-amber-400/50 bg-amber-100/60 px-3 py-2 text-xs text-foreground dark:bg-amber-950/40">
            <div className="font-medium">{error.message}</div>
            {error.hint && <div className="mt-0.5 text-muted-foreground">建议：{error.hint}</div>}
            <div className="mt-0.5 text-muted-foreground">
              这不是索引出错，只是上一轮还没结束。
            </div>
          </div>
        ) : (
          <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            <div>{error.message}</div>
            {error.hint && <div className="mt-0.5 opacity-80">建议：{error.hint}</div>}
          </div>
        ))}

      {overview && !overview.fts5Available && (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          本构建没有编进 SQLite FTS5，全文索引建不起来。这属于打包问题，不是数据问题。
        </div>
      )}

      {/* 第三条来源：进度事件订阅没成功。作业本身照跑，只是这一页一条进度都收不到，所以摆在进度卡之前。 */}
      {subscribeError && (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          <div className="font-medium">进度事件订阅失败：这一页收不到进度，进度条不会动。</div>
          <div className="mt-0.5 break-all" title={subscribeError.hint ?? undefined}>
            {subscribeError.message}
            {subscribeError.hint ? `（${subscribeError.hint}）` : ""}
          </div>
          <div className="mt-0.5 opacity-80">
            索引作业本身不受影响；这多半说明当前不是 Tauri 运行时（在浏览器里直接开前端就是这样）。
          </div>
        </div>
      )}

      <Card size="sm">
        <CardHeader>
          <CardTitle>本轮进度</CardTitle>
          <CardDescription>
            每处理 20 个文件回一次事件；页面中途打开只会看到之后的事件，累计数以总览为准。
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-2">
          {progress === null ? (
            // 空态文案要跟着 running 分支：start 之后 progress 被复位成 null，而补的那次 load() 已把
            // overview.running 置回 true。这时还写「点上方「建立索引」开始」，就和三格外的「索引进行中」徽章打架。
            <p className="text-xs text-muted-foreground">
              {running
                ? "本轮已开始，正在等第一条进度事件。每处理 20 个文件才回传一次，所以头几秒这里是空的。"
                : "本轮还没收到进度事件。点上方「建立索引」开始。"}
            </p>
          ) : (
            <>
              <div className="flex flex-wrap items-center gap-2 text-xs">
                <Badge variant={progress.state === "error" ? "destructive" : "outline"}>
                  {STATE_LABEL[progress.state]}
                </Badge>
                <span className="font-medium">{progress.projectName || "（轮级汇总）"}</span>
                {scanning ? (
                  <span className="text-muted-foreground">正在扫描…（扫完当前项目才有总数）</span>
                ) : (
                  pct !== null && (
                    <span className="tabular-nums">
                      {progress.done} / {progress.total}（{pct}%）
                    </span>
                  )
                )}
                <span className="text-muted-foreground">
                  ok {progress.ok} · skipped {progress.skipped} · failed {progress.failed}
                </span>
              </div>
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full bg-primary transition-all"
                  style={{ width: `${pct ?? 0}%` }}
                />
              </div>
              {progress.current && (
                <p
                  className="max-w-full truncate font-mono text-xs text-muted-foreground"
                  title={progress.current}
                >
                  当前文件：{progress.current}
                </p>
              )}
              {progress.error && (
                <p className="text-xs text-destructive" title={progress.error}>
                  本轮失败原因（实时）：{progress.error}
                </p>
              )}
              {progress.state === "error" && lastRunning && (
                <p className="text-xs text-muted-foreground">
                  失败发生在项目「{lastRunning.projectName || "（未报出项目名）"}」
                  {lastRunning.current ? (
                    <>
                      ，正在处理 <span className="font-mono">{lastRunning.current}</span>
                    </>
                  ) : (
                    "，处理到项目边界"
                  )}
                  。终止事件不带计数，这里的数字看总览。
                </p>
              )}
            </>
          )}
        </CardContent>
      </Card>

      {/* 另一条错误来源：上一轮作业失败的原因串（刷新后唯一还留下的证据），没有 code，整段念。 */}
      {overview?.lastError && (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          <div className="font-medium">上一轮索引失败</div>
          <div className="mt-0.5 break-all" title={overview.lastError}>
            {overview.lastError}
          </div>
        </div>
      )}

      <Card size="sm">
        <CardHeader>
          <CardTitle>支持类型与上限</CardTitle>
          <CardDescription>
            清单之外的类型压根不会进扫描队列，也就没有索引行。
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-2 text-xs">
          <div className="flex flex-wrap items-center gap-1">
            <span className="text-muted-foreground">支持类型：</span>
            {(overview?.supportedExts ?? []).map((e) => (
              <Badge key={e} variant="outline">
                .{e}
              </Badge>
            ))}
          </div>
          <p className="text-muted-foreground">
            清单之外的类型（图片、.doc/.ppt、压缩包）不会建行：图片等归 M6 OCR，.doc/.ppt 归 M7。
          </p>
          <div className="flex flex-wrap gap-x-4 gap-y-1">
            <span>
              单文件上限：
              {overview
                ? `${fmtBytes(overview.maxFileBytes)}（${overview.maxFileBytes} 字节，超出后落 skipped/too_large）`
                : "—"}
            </span>
            <span>
              单项目文件上限：{overview ? overview.maxFilesPerProject : "—"}
              （超出后本轮不再处理其余文件）
            </span>
          </div>
          <div className="flex flex-wrap items-center gap-1">
            <span className="text-muted-foreground">排除目录：</span>
            {(overview?.excludeDirs ?? []).map((d) => (
              <Badge key={d} variant="secondary">
                {d}
              </Badge>
            ))}
            {(overview?.excludeDirs ?? []).length === 0 && (
              <span className="text-muted-foreground">（无）</span>
            )}
          </div>
        </CardContent>
      </Card>

      <Card size="sm">
        <CardHeader>
          <CardTitle>按项目的索引状态</CardTitle>
          <CardDescription>
            点一行看它的文件清单。计数只念 ok / skipped / failed 三档：另外两档在本阶段压根没有写入方，
            摆两个 0 上去会被读成「都处理完了」，所以不给它们占栏位。
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-1.5">
          {overview === null ? (
            <p className="text-xs text-muted-foreground">读取索引总览…</p>
          ) : overview.projects.length === 0 ? (
            <p className="text-xs text-muted-foreground">
              还没有带主根目录的项目，索引没有可扫的东西。
              <Link to="/projects" className="ml-1 underline">
                去项目档案登记
              </Link>
            </p>
          ) : (
            overview.projects.map((p) => (
              <Button
                key={p.projectId}
                variant={selectedId === p.projectId ? "secondary" : "ghost"}
                className="h-auto w-full justify-start rounded-lg border py-2"
                onClick={() => setSelectedId(p.projectId)}
              >
                <span className="grid w-full gap-1 text-left">
                  <span className="flex flex-wrap items-center gap-2">
                    <span className="text-sm font-medium">{p.projectName}</span>
                    {p.rootExists ? null : (
                      <Badge variant="destructive">根目录当前不可达，通常是盘没挂载</Badge>
                    )}
                    <span className="ml-auto tabular-nums text-xs text-muted-foreground">
                      共 {p.total} 行
                    </span>
                  </span>
                  <span
                    className={`max-w-full truncate font-mono text-xs ${
                      p.rootExists ? "text-muted-foreground" : "text-destructive"
                    }`}
                    title={p.rootPath}
                  >
                    {p.rootPath}
                  </span>
                  <span className="flex gap-3 text-xs tabular-nums">
                    <span>ok {p.ok}</span>
                    <span>skipped {p.skipped}</span>
                    <span className={p.failed > 0 ? "text-destructive" : ""}>failed {p.failed}</span>
                  </span>
                </span>
              </Button>
            ))
          )}

          {selectedId && (
            <div className="mt-2 grid gap-2 border-t pt-2">
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground">状态筛选</span>
                <Select value={statusFilter} onValueChange={(v) => setStatusFilter(String(v))}>
                  <SelectTrigger size="sm" className="w-44">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {STATUS_FILTERS.map((f) => (
                      <SelectItem key={f.value} value={f.value}>
                        {f.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setDocsReload((n) => n + 1)}
                  title="重新读取当前项目的文件清单"
                >
                  刷新清单
                </Button>
              </div>

              {docsError && (
                <p className="text-xs text-destructive" title={docsError.hint ?? undefined}>
                  {docsError.message}
                  {docsError.hint ? `（${docsError.hint}）` : ""}
                </p>
              )}
              {docsBusy && <p className="text-xs text-muted-foreground">读取文件清单…</p>}
              {!docsBusy && docsFor === selectedId && docs.length === 0 && (
                <p className="text-xs text-muted-foreground">
                  这个项目在当前筛选下没有索引行（还没索引过，或筛选条件太窄）。
                </p>
              )}
              {!docsBusy && docsFor === selectedId && docs.length > 0 && (
                <ul className="grid gap-1">
                  {docs.map((r) => (
                    <li
                      key={r.id}
                      className="grid gap-0.5 rounded-md border bg-background px-2.5 py-1.5 text-xs"
                    >
                      <div className="flex flex-wrap items-center gap-2">
                        <Badge
                          variant={
                            r.status === "ok" ? "secondary" : r.status === "failed" ? "destructive" : "outline"
                          }
                        >
                          {r.status}
                        </Badge>
                        <span
                          className="min-w-0 max-w-full flex-1 truncate font-mono"
                          title={r.path}
                        >
                          {r.path}
                        </span>
                        <span className="tabular-nums text-muted-foreground">{fmtBytes(r.size)}</span>
                        <span className="text-muted-foreground">.{r.ext}</span>
                      </div>
                      {(r.skipReason || r.errorMsg || r.indexedAt) && (
                        <div className="flex flex-wrap gap-x-3 text-muted-foreground">
                          {r.skipReason && <span>原因：{reasonLabel(r.skipReason)}</span>}
                          {r.errorMsg && (
                            <span className="min-w-0 max-w-full truncate text-destructive" title={r.errorMsg}>
                              {r.errorMsg}
                            </span>
                          )}
                          {r.indexedAt && <span>索引于 {r.indexedAt}</span>}
                        </div>
                      )}
                    </li>
                  ))}
                  {docs.length === DOCS_LIMIT && (
                    <li className="text-xs text-muted-foreground">
                      已达本次 {DOCS_LIMIT} 行上限，可能还有更多。
                    </li>
                  )}
                </ul>
              )}
            </div>
          )}
        </CardContent>
      </Card>

      <Card size="sm">
        <CardHeader>
          <CardTitle>上一轮摘要</CardTitle>
          <CardDescription>
            「本轮扫描」是处理配额之前判的数，含超限跳过的行，不等于「索引了 N 个」。
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-1.5 text-xs">
          {overview?.lastRun == null ? (
            <p className="text-muted-foreground">
              这个进程里还没有跑成过一轮（彻底失败的那轮不留摘要，原因见上方红块）。
            </p>
          ) : (
            <>
              <div className="flex flex-wrap gap-2">
                <Badge variant="outline">状态 {STATE_LABEL[overview.lastRun.state]}</Badge>
                <span className="text-muted-foreground">
                  起 {overview.lastRun.startedAt} · 止 {overview.lastRun.finishedAt}
                </span>
              </div>
              {overview.lastRun.results.map((r) => (
                <div key={r.projectId} className="grid gap-1 rounded-md border px-2.5 py-1.5">
                  <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                    <span className="font-medium">{r.projectName}</span>
                    <span className="tabular-nums text-muted-foreground">
                      本轮扫描 {r.scannedTotal} · ok {r.ok} · skipped {r.skipped} · failed {r.failed} · 未变{" "}
                      {r.unchanged}
                    </span>
                  </div>
                  {r.capped && (
                    <p className="text-destructive">
                      已达单项目文件上限，本轮只处理前 {r.scannedTotal} 个（含超限跳过的行），其余文件本轮没有行
                    </p>
                  )}
                  {r.rootMissing && (
                    <p className="text-muted-foreground">
                      根目录本轮不可达，没有发起扫描（通常是盘没挂载）
                    </p>
                  )}
                  {r.walkErrors > 0 && (
                    <p className="text-muted-foreground">
                      目录遍历中有 {r.walkErrors} 个条目读不了（权限或短名一类，不影响其余文件）
                    </p>
                  )}
                </div>
              ))}
            </>
          )}
        </CardContent>
      </Card>

      <Card size="sm">
        <CardHeader>
          <CardTitle>试搜正文</CardTitle>
          <CardDescription>
            直接吃 search_docs：命中的是建索引时分好词的正文，命中词用 [ ] 包住。这一区只求能验，正式搜索面在
            M4。
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-2">
          <form
            className="flex items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              const q = query.trim();
              if (!q) return;
              setSearchBusy(true);
              void searchDocs(q)
                .then((r) => {
                  setHits(r);
                  setSearched(true);
                  setSearchError(null);
                })
                .catch((err) => setSearchError(toAppError(err)))
                .finally(() => setSearchBusy(false));
            }}
          >
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="搜正文里的词，例如：验收指标"
              className="h-8"
            />
            <Button size="sm" type="submit" disabled={searchBusy}>
              {searchBusy ? "检索中…" : "搜索"}
            </Button>
          </form>

          {searchError && (
            <p className="text-xs text-destructive" title={searchError.hint ?? undefined}>
              {searchError.message}
              {searchError.hint ? `（${searchError.hint}）` : ""}
            </p>
          )}
          {!searchError && searched && hits.length === 0 && (
            <p className="text-xs text-muted-foreground">
              没有命中。先确认上面「支持类型」里有没有它，再看项目表里它的行是 ok 还是 skipped。
            </p>
          )}
          {hits.length > 0 && (
            <ul className="grid gap-1.5">
              {hits.map((h) => (
                <li key={`${h.docId}:${h.path}`} className="grid gap-0.5 rounded-md border px-2.5 py-1.5 text-xs">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="min-w-0 flex-1 truncate font-mono" title={h.path}>
                      {h.path}
                    </span>
                    <Badge variant="outline">{h.projectName}</Badge>
                    {h.matchedBy === "prefix" && <Badge variant="secondary">宽松匹配</Badge>}
                  </div>
                  <p className="break-words text-muted-foreground">{h.snippet || "（摘要为空）"}</p>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
