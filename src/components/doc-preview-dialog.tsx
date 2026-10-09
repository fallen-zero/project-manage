import { useEffect, useState, type ReactNode } from "react";
import { Link } from "@tanstack/react-router";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import * as api from "@/lib/api";
import { toAppError } from "@/lib/ipc";
import type { DocPreview } from "@/types/search";

interface Props {
  docId: string | null;
  // 簇上的项目 id 由调用方传入：DocPreview 线格式里没有 projectId（Rust 侧只有
  // docId/path/projectName），为了一个跳转按钮去加字段会改掉 Task 4/5 已定稿的线格式
  // 和它们的 10+14 条测试。
  projectId: string | null;
  query: string;
  onOpenChange: (open: boolean) => void;
}

/** 按后端给的码元区间切片上色：`ranges` 就是 `String.prototype.slice` 的下标，零换算。 */
function Highlighted({ text, ranges }: { text: string; ranges: [number, number][] }) {
  const parts: ReactNode[] = [];
  let cursor = 0;
  ranges.forEach(([from, to], i) => {
    if (from > cursor) parts.push(text.slice(cursor, from));
    parts.push(<mark key={i}>{text.slice(from, to)}</mark>);
    cursor = Math.max(cursor, to);
  });
  if (cursor < text.length) parts.push(text.slice(cursor));
  return <pre className="max-h-[55vh] overflow-auto whitespace-pre-wrap break-words rounded-md bg-muted/40 p-3 font-mono text-xs leading-relaxed">{parts}</pre>;
}

export function DocPreviewDialog({ docId, projectId, query, onOpenChange }: Props) {
  // 没有 busy 这一格：定稿的 JSX 里没有任何读取方，加载中由 DialogDescription 的
  // `data === null` 分支说出去。留着就是 noUnusedLocals 的 TS6133，按「谁没被用就删谁」删掉。
  const [data, setData] = useState<DocPreview | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!docId) {
      setData(null);
      setError(null);
      return;
    }
    let alive = true;
    setError(null);
    api
      .docPreview(docId, query)
      .then((d) => alive && setData(d))
      // 预览失败只让对话框变红，绝不让整段结果消失（spec §七）。
      .catch((e) => alive && setError(toAppError(e).message));
    return () => {
      alive = false;
    };
  }, [docId, query]);

  return (
    <Dialog open={docId !== null} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle className="font-mono text-sm">{data?.path ?? "原文预览"}</DialogTitle>
          <DialogDescription className="text-xs">
            {data ? `${data.projectName} · 窗口拼接的原文，不是整篇` : "正在从磁盘重抽原文…"}
          </DialogDescription>
        </DialogHeader>
        {error && <p className="text-sm text-destructive">{error}</p>}
        {!error && data && <Highlighted text={data.text} ranges={data.ranges} />}
        {!error && data && (
          <p className="text-xs text-muted-foreground">
            {data.ranges.length === 0
              ? "正文里没有这个词，命中的是文件名。"
              : data.truncated
                ? "只显示命中附近的窗口，中间以 ⋯ 分隔。"
                : "整篇已在上面。"}
          </p>
        )}
        <DialogFooter className="flex-wrap gap-x-4">
          {/* 刻意不给「用外部程序打开」：openPath 把文件交给外部应用，而那边随时可能保存回写，
              这条工具的立身边界是只读源文件。所以只有选中、复制路径、跳项目三个动作。 */}
          {data && (
            <>
              <button className="text-xs underline" onClick={() => void api.revealFolder(data.path)}>
                在资源管理器中选中
              </button>
              <button className="text-xs underline" onClick={() => void api.copyText(data.path)}>
                复制完整路径
              </button>
              {projectId && (
                <Link className="text-xs underline" to="/projects/$projectId" params={{ projectId }}>
                  打开项目
                </Link>
              )}
            </>
          )}
          <button className="text-xs text-muted-foreground" onClick={() => onOpenChange(false)}>
            关闭
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
