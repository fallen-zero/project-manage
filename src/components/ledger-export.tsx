// 台账导出（M11-b spec §七）：格式下拉 + 明文勾选 + 二次确认 + 保存对话框。
// 导出逻辑全在后端；这里只收集意图。勾了明文必须先过一次 in-app 确认 ——
// `window.confirm` 在 WebView2 下是原生窗口，脚本与自动化都点不到（HANDOFF 坑 12）。
import { useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { exportLedger } from "@/lib/api";
import { defaultFileName } from "@/lib/ledger-export-name";
import { toAppError, type AppErrorShape } from "@/lib/ipc";

type Format = "xlsx" | "md";

const todayIso = () => {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};

export function LedgerExport({ projectId, projectName }: { projectId: string; projectName: string }) {
  const [format, setFormat] = useState<Format>("xlsx");
  const [includeSecrets, setIncludeSecrets] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);
  const [ok, setOk] = useState<string | null>(null);

  const runExport = async () => {
    setConfirmOpen(false);
    setBusy(true);
    setError(null);
    setOk(null);
    try {
      const path = await saveDialog({
        defaultPath: defaultFileName(projectName, format, todayIso()),
        filters: [
          format === "xlsx"
            ? { name: "Excel 工作簿", extensions: ["xlsx"] }
            : { name: "Markdown", extensions: ["md"] },
        ],
      });
      if (path === null) return; // 取消不是错误（M4 的取消语义同一口径）
      const s = await exportLedger({ projectId, format, includeSecrets, path });
      setOk(`已导出 ${s.sheets} 张表 / 共 ${s.rows} 行${s.includedSecrets ? "（含明文）" : ""}`);
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mt-3 space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <Select value={format} onValueChange={(v) => setFormat(v as Format)}>
          <SelectTrigger className="w-32">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="xlsx">Excel</SelectItem>
            <SelectItem value="md">Markdown</SelectItem>
          </SelectContent>
        </Select>
        <Label className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <input
            type="checkbox"
            className="size-4 accent-primary"
            checked={includeSecrets}
            onChange={(e) => setIncludeSecrets(e.target.checked)}
          />
          包含账号与密码明文
        </Label>
        <Button
          size="sm"
          disabled={busy}
          onClick={() => (includeSecrets ? setConfirmOpen(true) : void runExport())}
        >
          导出台账
        </Button>
      </div>

      {error && (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error.message}
          {error.hint && <span className="ml-1 opacity-80">建议：{error.hint}</span>}
        </div>
      )}
      {ok && <p className="text-xs text-muted-foreground">{ok}</p>}

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>导出文件里会含账号与密码明文</DialogTitle>
            <DialogDescription>
              导出文件<strong>不受主密码保护</strong> —— 任何拿到这个文件的人都能直接看到明文。只在你确实需要时才继续。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setConfirmOpen(false)}>
              取消
            </Button>
            <Button variant="destructive" disabled={busy} onClick={() => void runExport()}>
              我明白，继续导出
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
