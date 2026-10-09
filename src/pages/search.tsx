import { useEffect, useState } from "react";
import { Link } from "@tanstack/react-router";
import { Input } from "@/components/ui/input";
import { SearchBundleView } from "@/components/search-bundle";
import { emptyStateKind } from "@/lib/search-order";
import { useLocalSearchStore } from "@/stores/local-search";

// 输入即搜，但要按住一会儿：库里是 LIKE 扫描，每个键都发一次没必要。
const DEBOUNCE_MS = 250;

export function SearchPage() {
  const [text, setText] = useState("");
  const { bundle, searched, busy, error, run, clear } = useLocalSearchStore();

  useEffect(() => {
    const id = window.setTimeout(() => void run(text), DEBOUNCE_MS);
    return () => window.clearTimeout(id);
  }, [text, run]);

  useEffect(() => clear, [clear]);

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-4 px-6 py-12">
      <h1 className="text-2xl font-semibold">搜索</h1>
      <Input
        autoFocus
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder="搜项目、网址、账号用途、备注…"
        className="h-12 text-base"
      />

      {error && (
        <p className="text-sm text-destructive" title={error.hint ?? undefined}>
          {error.message}
          {error.hint ? `（${error.hint}）` : ""}
        </p>
      )}

      {!text.trim() ? (
        <p className="text-sm text-muted-foreground">
          一次搜三处：项目档案、信息台账的明文列，以及已建索引的文档正文。账号密码这些加密列不参与检索，也不出现在结果里。
        </p>
      ) : busy ? (
        <p className="text-sm text-muted-foreground">检索中…</p>
      ) : searched && bundle && emptyStateKind(bundle) === "not-indexed" ? (
        <p className="text-sm text-muted-foreground">
          正文还没建索引，去 <Link to="/index" className="underline">/index</Link> 启动一轮；项目档案与台账的明文检索不受影响。
        </p>
      ) : searched && bundle && emptyStateKind(bundle) === "no-hit" ? (
        <p className="text-sm text-muted-foreground">
          没有命中「{text.trim()}」。只登记过路径的项目不会凭空出现在正文里。
        </p>
      ) : bundle ? (
        <SearchBundleView bundle={bundle} />
      ) : null}
    </div>
  );
}
