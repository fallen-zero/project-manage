import type { Workspace } from "@/types/workspace";

// 与后端 `workspace.rs` 的 `NAME_MAX_CHARS` 同一个数：改了这边没改那边，
// 结果就是「前端放行、后端当场拒」，用户看见的是一条莫名其妙的错误。
export const WORKSPACE_NAME_MAX = 40;
export const DEFAULT_WORKSPACE_ID = "default";

/** 区名的可见判据。后端是唯一的真门（本项目只在边界校验一次），
 *  这一格只为弹窗提前给一句人话，不是第二道校验。 */
export function nameHint(name: string): string | null {
  const trimmed = name.trim();
  if (!trimmed) return "工作区名称不能为空";
  if ([...trimmed].length > WORKSPACE_NAME_MAX) {
    return `工作区名称不能超过 ${WORKSPACE_NAME_MAX} 个字符`;
  }
  return null;
}

/** 删除按钮的禁用原因；`null` = 可删。
 *  三条原因的顺序与文案与后端 `workspace::delete` 的三道守卫逐字对应（M15 spec §五）。 */
export function deleteGuard(ws: Workspace, activeId: string | null): string | null {
  if (ws.id === DEFAULT_WORKSPACE_ID) return "默认工作区不可删除";
  if (activeId !== null && ws.id === activeId) return "当前工作区不可删除，请先切换到其他工作区";
  if (ws.projectCount > 0) return "该工作区下还有项目（含已删除项目），不能删除";
  return null;
}

/** 当前区 id 的唯一取法：`items` 里 `isActive` 那格。store 不另存一份真值。 */
export function activeWorkspaceId(items: Workspace[]): string | null {
  return items.find((w) => w.isActive)?.id ?? null;
}
