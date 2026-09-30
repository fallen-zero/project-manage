import { useEffect, useState, type ReactNode } from "react";
import { KeyRound, Lock, RotateCcw, ShieldCheck, Copy } from "lucide-react";
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
import { Separator } from "@/components/ui/separator";
import { copyText } from "@/lib/api";
import { useVaultStore } from "@/stores/vault";

type Mode = "init" | "unlock" | "manage";

export function VaultControl() {
  const [open, setOpen] = useState(false);
  const status = useVaultStore((s) => s.status);
  const unlocked = status?.unlocked ?? false;
  const initialized = status?.initialized ?? false;

  // 未初始化时这是个必须走一遍的Setup，而不是可选项：没有主密钥，凭据就无法加密入库。
  const label = !initialized ? "设置主密码" : unlocked ? "已解锁" : "已锁定";

  return (
    <>
      <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
        {unlocked ? <ShieldCheck className="size-4" /> : <Lock className="size-4" />}
        <span className={unlocked ? "text-emerald-700" : "text-muted-foreground"}>{label}</span>
      </Button>
      <VaultDialog open={open} onOpenChange={setOpen} />
    </>
  );
}

export function VaultDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const { status, busy, error, oneTimeCode, init, unlock, lock, changePassword, rotateRecoveryCode, dismissCode, clearError } =
    useVaultStore();
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const [oldPw, setOldPw] = useState("");
  const [notice, setNotice] = useState<string | null>(null);

  const initialized = status?.initialized ?? false;
  const unlocked = status?.unlocked ?? false;
  const mode: Mode = !initialized ? "init" : unlocked ? "manage" : "unlock";

  // 关掉弹窗就把输入框里的明文清掉：密码不该在组件状态里过夜。
  useEffect(() => {
    if (!open) {
      setA("");
      setB("");
      setOldPw("");
      setNotice(null);
      clearError();
    }
  }, [open, clearError]);

  const submit = async () => {
    setNotice(null);
    if (mode === "init") {
      if (a !== b) {
        setNotice("两次输入的主密码不一致");
        return;
      }
      if (await init(a)) {
        setA("");
        setB("");
      }
    } else if (mode === "unlock") {
      if (await unlock(a)) setA("");
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <KeyRound className="size-4" />
            {mode === "init" ? "设置主密码" : mode === "unlock" ? "解锁凭据库" : "凭据库已解锁"}
          </DialogTitle>
          <DialogDescription>
            {mode === "init"
              ? "主密码用来加密台账里的账号与密码。它不入库、不上传，忘了只能靠恢复码重建包裹。"
              : mode === "unlock"
                ? "输入主密码；忘记时可输入当初抄下的恢复码。"
                : "现在可以查看与编辑加密字段。锁定后敏感字段立即不可读。"}
          </DialogDescription>
        </DialogHeader>

        {oneTimeCode ? (
          <div className="grid gap-2 rounded-md border border-amber-300 bg-amber-50 p-3">
            <p className="text-sm font-medium text-amber-900">
              这是你的一次性恢复码，只显示这一次。抄下来存到密码管理器或纸上。
            </p>
            <div className="flex items-center gap-2">
              <code className="flex-1 rounded bg-background px-2 py-1 font-mono text-sm select-all">
                {oneTimeCode}
              </code>
              <Button
                variant="outline"
                size="sm"
                onClick={() => void copyText(oneTimeCode).then(() => setNotice("恢复码已复制"))}
              >
                <Copy className="size-4" />
              </Button>
            </div>
            <Button size="sm" onClick={dismissCode}>
              我已保存
            </Button>
          </div>
        ) : mode === "init" ? (
          <div className="grid gap-3">
            <Field label="主密码（至少 8 个字符）">
              <Input type="password" value={a} onChange={(e) => setA(e.target.value)} autoFocus />
            </Field>
            <Field label="再输一次">
              <Input type="password" value={b} onChange={(e) => setB(e.target.value)} />
            </Field>
          </div>
        ) : mode === "unlock" ? (
          <Field label="主密码或恢复码">
            <Input type="password" value={a} onChange={(e) => setA(e.target.value)} autoFocus />
          </Field>
        ) : (
          <div className="grid gap-3">
            <Separator />
            <div className="grid gap-2">
              <Label className="text-xs text-muted-foreground">更换主密码</Label>
              <div className="grid grid-cols-2 gap-2">
                <Input
                  type="password"
                  placeholder="旧主密码"
                  value={oldPw}
                  onChange={(e) => setOldPw(e.target.value)}
                />
                <Input
                  type="password"
                  placeholder="新主密码"
                  value={a}
                  onChange={(e) => setA(e.target.value)}
                />
              </div>
              <Button
                variant="outline"
                size="sm"
                disabled={busy || !oldPw || !a}
                onClick={async () => {
                  if (await changePassword(oldPw, a)) {
                    setOldPw("");
                    setA("");
                    setNotice("主密码已更换，密文未动（只换了外层包裹）");
                  }
                }}
              >
                更换
              </Button>
            </div>
            <Separator />
            <Button
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={async () => {
                if (await rotateRecoveryCode()) setNotice("已生成新的恢复码，旧码同时作废");
              }}
            >
              <RotateCcw className="size-4" />
              换一张恢复码
            </Button>
          </div>
        )}

        {(notice || error) && (
          <p className={error ? "text-sm text-destructive" : "text-sm text-emerald-700"} title={error?.hint ?? undefined}>
            {error ? `${error.message}${error.hint ? `（${error.hint}）` : ""}` : notice}
          </p>
        )}

        <DialogFooter>
          {mode === "manage" && (
            <Button variant="secondary" onClick={() => void lock()}>
              <Lock className="size-4" />
              立即锁定
            </Button>
          )}
          {mode !== "manage" && (
            <Button onClick={() => void submit()} disabled={busy || !a}>
              {busy ? "处理中…" : mode === "init" ? "创建并解锁" : "解锁"}
            </Button>
          )}
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            关闭
          </Button>
        </DialogFooter>
        {mode === "manage" && <Badge variant="outline">敏感字段可读写</Badge>}
      </DialogContent>
    </Dialog>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <Label className="grid gap-1.5">
      <span className="text-xs text-muted-foreground">{label}</span>
      {children}
    </Label>
  );
}
