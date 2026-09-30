import { useEffect, useState } from "react";
import { Link } from "@tanstack/react-router";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { useLocalSearchStore } from "@/stores/local-search";
import { SOURCE_LABELS, SOURCE_ORDER, type FieldHit, type HitSource } from "@/types/search";

// 输入即搜，但要按住一会儿：库里是 LIKE 扫描，每个键都发一次没必要。
const DEBOUNCE_MS = 250;

function group(hits: FieldHit[]) {
  const by = new Map<HitSource, FieldHit[]>();
  for (const h of hits) {
    const list = by.get(h.source) ?? [];
    list.push(h);
    by.set(h.source, list);
  }
  return SOURCE_ORDER.filter((s) => by.has(s)).map((s) => ({ source: s, items: by.get(s)! }));
}

export function SearchPage() {
  const [text, setText] = useState("");
  const { hits, searched, busy, error, run, clear } = useLocalSearchStore();

  useEffect(() => {
    const id = window.setTimeout(() => void run(text), DEBOUNCE_MS);
    return () => window.clearTimeout(id);
  }, [text, run]);

  useEffect(() => clear, [clear]);

  const groups = group(hits);

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
          在项目档案与台账的明文列里找；账号、密码这些加密列不参与检索，也不出现在结果里。
          文档正文检索要到 M3 建好索引后接入。
        </p>
      ) : busy ? (
        <p className="text-sm text-muted-foreground">检索中…</p>
      ) : searched && groups.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          没有命中「{text.trim()}」。只登记过路径的项目不会凭空出现，正文要等 M3。
        </p>
      ) : (
        <div className="grid gap-4">
          {groups.map((g) => (
            <section key={g.source} className="grid gap-1.5">
              <h2 className="text-xs font-medium text-muted-foreground">
                {SOURCE_LABELS[g.source]}
                <span className="ml-1">{g.items.length}</span>
              </h2>
              {g.items.map((h) => (
                <div
                  key={h.id}
                  className="flex items-center gap-2 rounded-md border bg-background px-3 py-2 text-sm"
                >
                  <div className="min-w-0 flex-1">
                    <p className="truncate">{h.title}</p>
                    {h.detail && (
                      <p className="truncate font-mono text-xs text-muted-foreground" title={h.detail}>
                        {h.detail}
                      </p>
                    )}
                  </div>
                  <Badge variant="outline" className="shrink-0">
                    {h.projectName}
                  </Badge>
                  <Link
                    to="/projects/$projectId"
                    params={{ projectId: h.projectId }}
                    className="shrink-0 text-xs underline text-muted-foreground hover:text-foreground"
                  >
                    打开项目
                  </Link>
                </div>
              ))}
            </section>
          ))}
        </div>
      )}
    </div>
  );
}
