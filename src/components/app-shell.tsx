import { useEffect } from "react";
import { Link, Outlet } from "@tanstack/react-router";
import { Badge } from "@/components/ui/badge";
import { useAppStore } from "@/stores/app-store";

const nav = [
  { to: "/", label: "搜索" },
  { to: "/projects", label: "项目" },
];

export function AppShell() {
  const { status, error, loading, refresh } = useAppStore();

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div className="flex h-screen flex-col bg-muted/30">
      <header className="flex items-center gap-1 border-b bg-background px-4 py-2">
        <span className="mr-4 text-sm font-semibold">项目资料管理</span>
        {nav.map((n) => (
          <Link
            key={n.to}
            to={n.to}
            className="rounded-md px-3 py-1.5 text-sm text-muted-foreground hover:bg-muted hover:text-foreground"
            activeProps={{ className: "bg-muted font-medium text-foreground" }}
          >
            {n.label}
          </Link>
        ))}
      </header>

      <main className="flex-1 overflow-auto">
        <Outlet />
      </main>

      <footer className="flex items-center gap-2 border-t bg-background px-4 py-1.5 text-xs">
        {loading && <span className="text-muted-foreground">读取本地库…</span>}
        {error && (
          <span className="text-destructive" title={error.hint ?? undefined}>
            {error.message}
          </span>
        )}
        {status && (
          <>
            <Badge variant="outline">journal_mode={status.journal_mode}</Badge>
            <Badge variant="outline">schema=v{status.schema_version}</Badge>
            <Badge variant={status.fts5_available ? "secondary" : "destructive"}>
              FTS5 {status.fts5_available ? "可用" : "不可用"}
            </Badge>
            <span className="ml-auto max-w-[40%] truncate text-muted-foreground" title={status.db_file}>
              {status.db_file}
            </span>
          </>
        )}
      </footer>
    </div>
  );
}
