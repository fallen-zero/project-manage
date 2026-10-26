import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { autostartGet, autostartSet } from "@/lib/api";
import { toAppError, type AppErrorShape } from "@/lib/ipc";

export function SettingsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [enabled, setEnabled] = useState(false);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorShape | null>(null);

  useEffect(() => {
    if (!open) return;
    let alive = true;
    setLoading(true);
    setError(null);
    void autostartGet()
      .then((value) => {
        if (alive) setEnabled(value);
      })
      .catch((e) => {
        if (alive) setError(toAppError(e));
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [open]);

  const toggle = async (next: boolean) => {
    setBusy(true);
    setError(null);
    try {
      await autostartSet(next);
      setEnabled(await autostartGet());
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>设置</DialogTitle>
          <DialogDescription>本机偏好，不随备份包迁移。</DialogDescription>
        </DialogHeader>

        <label className="flex cursor-pointer items-start gap-2 text-sm">
          <input
            type="checkbox"
            className="mt-0.5 size-4"
            checked={enabled}
            disabled={loading || busy}
            onChange={(e) => void toggle(e.target.checked)}
          />
          <span>
            开机自启
            <span className="block text-xs text-muted-foreground">
              登录 Windows 后自动启动。移动或重装应用后请重新开关一次。
            </span>
          </span>
        </label>

        {loading && <p className="text-xs text-muted-foreground">读取系统设置…</p>}
        {busy && <p className="text-xs text-muted-foreground">保存中…</p>}
        {error && (
          <p className="text-sm text-destructive" title={error.hint ?? undefined}>
            {error.message}
            {error.hint ? `（${error.hint}）` : ""}
          </p>
        )}

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            关闭
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
