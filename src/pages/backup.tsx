import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Separator } from "@/components/ui/separator";
import {
  backupExport,
  backupImport,
  backupInspect,
  pickBackupPath,
  pickPackagePath,
} from "@/lib/api";
import { backupFileName } from "@/lib/backup-name";
import { toAppError, type AppErrorShape } from "@/lib/ipc";
import { useVaultStore } from "@/stores/vault";
import type { BackupCounts, BackupSummary, ImportOutcome, PackageInfo } from "@/types/backup";

// 包内七张表的固定行序：与 Rust 侧 Counts 的字段序一致，确认框与页面上都要念全七张，
// 只列三张就是把 D5 那条命令的花费打折（spec §七「看得见包里有什么」的兑现物）。
const COUNT_ROWS: { key: keyof BackupCounts; label: string }[] = [
  { key: "projects", label: "项目档案" },
  { key: "ledgerEnvs", label: "台账环境" },
  { key: "ledgerCredentials", label: "台账凭据" },
  { key: "ledgerServers", label: "台账服务器" },
  { key: "ledgerLinks", label: "台账链接" },
  { key: "ledgerNotes", label: "台账笔记" },
  { key: "settings", label: "设置" },
];

function fmtBytes(n: number): string {
  if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${n} B`;
}

export function BackupPage() {
  // 本地 state 形制照 src/pages/index-status.tsx:74-81 的 installBusy/installError：
  // 刻意不复用索引作业 store 的 `error` —— 备份失败与索引失败是两件事，
  // 混在一个字段里会让 /index 页念到备份的错误。
  // `busy` 用单值联合：backup_export 与 backup_import 之间 Rust 侧没有互斥，
  // 同页双开（导出进行中点导入）这一格由界面在这里挡住。
  const [busy, setBusy] = useState<"export" | "inspect" | "import" | null>(null);
  const [summary, setSummary] = useState<BackupSummary | null>(null);
  const [info, setInfo] = useState<PackageInfo | null>(null);
  const [outcome, setOutcome] = useState<ImportOutcome | null>(null);
  const [error, setError] = useState<AppErrorShape | null>(null);
  // 覆盖确认框：形制跟仓库现成的删除确认（src/pages/projects.tsx:179-204 的 shadcn Dialog），
  // 不引 window.confirm 这第二种。
  const [confirming, setConfirming] = useState(false);

  async function onExport() {
    const path = await pickBackupPath(backupFileName(new Date()));
    if (!path) return; // 用户取消：不是错误，别写 error
    setBusy("export");
    setError(null);
    try {
      setSummary(await backupExport(path));
      setOutcome(null);
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setBusy(null);
    }
  }

  async function onInspect() {
    const path = await pickPackagePath();
    if (!path) return; // 同上：取消对话框不是错误
    setBusy("inspect");
    setError(null);
    try {
      setInfo(await backupInspect(path));
    } catch (e) {
      setInfo(null);
      setError(toAppError(e));
    } finally {
      setBusy(null);
    }
  }

  async function onImport() {
    if (!info) return; // 没验过包就不给按（D5 的界面兑现）
    setConfirming(false);
    setBusy("import");
    setError(null);
    try {
      setOutcome(await backupImport(info.path));
      setInfo(null);
      // 命令层在这趟成功后清了 AppState.mk（vault 已锁），界面必须知道：
      // 唯一真值在 vault store 里，这里只调 refresh，不在备份页自己养第二份解锁状态。
      void useVaultStore.getState().refresh();
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-4 px-6 py-8">
      <div>
        <h1 className="text-xl font-semibold">备份</h1>
        <p className="mt-1 text-xs text-muted-foreground">
          整库导成一个 zip 备份包 / 检查一个包 / 用包覆盖回来。包里只有数据库本体：文档索引不在包里。
          {/* 注 5（D6 的 UI 半边）：这句刻意挂在**无条件**渲染的页头，而不是「inspect 成功之后」的分支里 ——
              只点「导出备份」的人拿走的就是含明文列的包，那一刻屏幕上就必须有这句话。
              列名清单与 src-tauri/src/backup.rs 的 warn_plaintext_columns() 逐字对齐（一份文本两处出），
              不在这里另造一套。检查卡片里那条来自包内的 `plaintextWarning` 保留：它念的是**这份包**
              自己声明的清单，与这句静态文案用途不同，不算重复。 */}
          包内非 *_cipher 列为明文：项目名/编号/负责人/客户/联系人/合同号、服务器 ip 与堡垒机、各类 url、各表 note。备份包本体不密封，别把它当安全介质分发。
        </p>
      </div>

      {error && (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          <div className="font-medium">备份操作失败</div>
          <div
            className="mt-0.5 break-all"
            title={`${error.message}${error.hint ? `（${error.hint}）` : ""}`}
          >
            {error.message}
            {error.hint ? `（${error.hint}）` : ""}
          </div>
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>导出备份包</CardTitle>
          <CardDescription>
            给当前整库打一份一致性快照，选一个存放位置后生成 zip；不碰任何被登记的原文件。
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-2 text-sm">
          <Button
            size="sm"
            className="w-fit"
            disabled={busy !== null}
            onClick={() => void onExport()}
          >
            {busy === "export" ? "导出中…" : "导出备份"}
          </Button>
          {summary && (
            <div className="flex flex-col gap-1 text-xs">
              <div className="break-all text-muted-foreground" title={summary.path}>
                {summary.path}（{fmtBytes(summary.bytes)}，schema v{summary.schemaVersion}，
                {summary.entries.length} 个条目）
              </div>
              <div className="flex flex-wrap gap-x-3 gap-y-1">
                {COUNT_ROWS.map(({ key, label }) => (
                  <span key={key}>
                    {label} {summary.counts[key]}
                  </span>
                ))}
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>检查备份包</CardTitle>
          <CardDescription>
            只读地打开一个 zip 核对 manifest 与表行数，不改本机任何数据。
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-2 text-sm">
          <Button
            size="sm"
            variant="outline"
            className="w-fit"
            disabled={busy !== null}
            onClick={() => void onInspect()}
          >
            {busy === "inspect" ? "检查中…" : "选择备份包并检查"}
          </Button>
          {info && (
            <div className="flex flex-col gap-1 text-xs">
              <div className="break-all text-muted-foreground" title={info.path}>
                {info.path}（{fmtBytes(info.bytes)}）
              </div>
              <div>
                导出于 {info.createdAt}（schema v{info.schemaVersion}，应用 {info.appVersion}）
              </div>
              <div className="flex flex-wrap gap-x-3 gap-y-1">
                {COUNT_ROWS.map(({ key, label }) => (
                  <span key={key}>
                    {label} {info.counts[key]}
                  </span>
                ))}
              </div>
              {info.plaintextWarning !== "" && (
                <Badge variant="secondary" className="w-fit">
                  {info.plaintextWarning}
                </Badge>
              )}
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>用备份包导入</CardTitle>
          <CardDescription>
            整库覆写当前数据；覆写前会自动在你的数据目录里存一份现状，可以撤销。
            {/* M8.1 ①（需求方定的口径：只加可见性，不自动删）：撤销包由工具自己写、应用永不删改，
                所以反复导入会一份份留在数据目录里。界面上说清「谁负责清」，比偷偷删掉更符合这条红线。 */}
            每导入一次就会留一份那样的撤销包（文件名以 <code>pre-restore-</code> 开头），应用不会自动清理 ——
            用不上了你可以在数据目录里自己删掉旧的。
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-2 text-sm">
          <Button
            size="sm"
            variant="destructive"
            className="w-fit"
            disabled={busy !== null || info === null}
            onClick={() => setConfirming(true)}
          >
            {busy === "import" ? "导入中…" : "导入并覆盖当前数据"}
          </Button>
          {info === null && busy === null && (
            <p className="text-xs text-muted-foreground">
              先在上方「检查备份包」里选一个包 —— 没验过的包不给导入。
            </p>
          )}
          {outcome && (
            <div className="flex flex-col gap-1 text-xs">
              {/* 注 6 → M8.1 ④a：这里念的是包内库版本 + 撤销包完整路径，**没有**「包内 N 个条目」那句。
                  原来那句读的是 `ImportOutcome.entries = manifest.files.len()`，manifest 只点名
                  `backup.db` 一条 ⇒ 那个数恒为 1，跟 zip 里实际有 2 个条目（`BackupSummary.entries`）
                  根本不是一回事，念出来会被读成文件数。T8 先把这句从界面上撤掉，M8.1 再把这个
                  没有信息量的字段从出参与 `src/types/backup.ts` 里一并删掉（需求方当次批准），
                  所以现在也不会有人再把它捡回去念。 */}
              <div>
                导入完成：用的这份包是 schema v{outcome.schemaVersion}。
              </div>
              <div className="break-all">
                覆写前的现状已自动存成应急包，随时可以再导回去撤销：{outcome.preRestorePath}
              </div>
              {/* `outcome.counts` 来自 Rust 侧 restore **之前**对当前库现数的 counts
                  （backup.rs:449 的 `counts_before`），所以这七个数是「覆写前」的现数，
                  不是导进来的新数 —— 时点必须念出来，否则用户会以为这是导入后的结果。 */}
              <div>下面七张表是覆写前你库里的行数（万一导错了包，靠这组数认回原样）：</div>
              <div className="flex flex-wrap gap-x-3 gap-y-1">
                {COUNT_ROWS.map(({ key, label }) => (
                  <span key={key}>
                    {label} {outcome.counts[key]}
                  </span>
                ))}
              </div>
              <div className="rounded-lg border border-muted-foreground/30 bg-muted/40 px-3 py-2">
                索引不在备份包里：导入后正文一条都搜不到，需要手动重建。
                {/* D7：重建是几十个项目扫几分钟的开销，不该由一次导入替你决定 ——
                    这里只挂链接去 /index 复用现成的建立索引按钮，本格不起索引轮。 */}
                <Link to="/index" className="ml-1 underline">
                  去索引页建立索引
                </Link>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      <Dialog open={confirming} onOpenChange={(o) => !o && setConfirming(false)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>导入将覆盖当前整库</DialogTitle>
            <DialogDescription>
              {info && (
                <>
                  这个包导出于 {info.createdAt}（schema v{info.schemaVersion}），包含：
                  {/* DialogDescription 渲染成 <p>（radix Description），七张表用行内 span + block
                      排版念，不能在 <p> 里塞 div。 */}
                  {COUNT_ROWS.map(({ key, label }) => (
                    <span key={key} className="block text-xs">
                      · {label} {info.counts[key]} 条
                    </span>
                  ))}
                  索引不在包里，导入后正文一条都搜不到，要手动重建。
                  覆写前会自动在你自己的数据目录里存一份现状（可撤销）。
                </>
              )}
            </DialogDescription>
          </DialogHeader>
          <Separator />
          <DialogFooter>
            <Button variant="ghost" onClick={() => setConfirming(false)}>
              取消
            </Button>
            <Button variant="destructive" disabled={busy !== null} onClick={() => void onImport()}>
              确认导入
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
