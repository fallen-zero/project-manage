import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { WorkspaceManager } from "@/components/workspace-manager";
import { useWorkspacesStore } from "@/stores/workspaces";

/// 顶栏的全局工作区切换器 + 「管理工作区」入口（spec §8.3）。
/// 值取 store 的 activeId（也就是 `items` 里 isActive 那一格），本地不另存一份。
export function WorkspaceSwitcher() {
  const { items, activeId, loading, saving, switchTo } = useWorkspacesStore();
  const [managerOpen, setManagerOpen] = useState(false);

  return (
    <div className="flex items-center gap-1">
      <Select
        value={activeId ?? undefined}
        onValueChange={(v) => void switchTo(String(v))}
        disabled={loading || items.length === 0 || saving}
      >
        <SelectTrigger className="h-8 w-44" title="当前工作区">
          <SelectValue placeholder="工作区" />
        </SelectTrigger>
        <SelectContent>
          {items.map((w) => (
            <SelectItem key={w.id} value={w.id}>
              {w.name}（{w.projectCount}）
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Button size="sm" variant="ghost" onClick={() => setManagerOpen(true)}>
        管理
      </Button>
      <WorkspaceManager open={managerOpen} onOpenChange={setManagerOpen} />
    </div>
  );
}
