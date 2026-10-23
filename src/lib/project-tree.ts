import type { Project } from "@/types/project";

export interface ProjectTreeNode {
  project: Project;
  depth: number;
  children: ProjectTreeNode[];
}

export interface ParentOption {
  id: string;
  name: string;
  status: string;
  depth: number;
}

function parentIdOf(p: Project): string | null {
  return p.parentId && p.parentId !== p.id ? p.parentId : null;
}

// 链进环 = 永远到不了顶。这类节点按根展示，既不递归挂死，也不丢节点。
function inCycle(start: Project, byId: Map<string, Project>): boolean {
  const seen = new Set<string>();
  let cur = start;
  for (;;) {
    const pid = parentIdOf(cur);
    if (!pid) return false;
    if (seen.has(cur.id)) return true;
    seen.add(cur.id);
    const parent = byId.get(pid);
    if (!parent) return false;
    cur = parent;
  }
}

export function buildProjectForest(projects: Project[]): ProjectTreeNode[] {
  const byId = new Map(projects.map((p) => [p.id, p]));
  const nodes = new Map<string, ProjectTreeNode>(
    projects.map((p): [string, ProjectTreeNode] => [
      p.id,
      { project: p, depth: 0, children: [] },
    ]),
  );
  const isRoot = new Map<string, boolean>(
    projects.map((p): [string, boolean] => {
      const pid = parentIdOf(p);
      return [p.id, !pid || !byId.has(pid) || inCycle(p, byId)];
    }),
  );
  const roots: ProjectTreeNode[] = [];

  for (const p of projects) {
    const node = nodes.get(p.id);
    if (!node) continue;
    const pid = parentIdOf(p);
    if (isRoot.get(p.id) || !pid) {
      roots.push(node);
      continue;
    }
    const parent = nodes.get(pid);
    if (!parent) {
      roots.push(node);
      continue;
    }
    parent.children.push(node);
  }

  const setDepth = (node: ProjectTreeNode, depth: number) => {
    node.depth = depth;
    for (const child of node.children) setDepth(child, depth + 1);
  };
  for (const root of roots) setDepth(root, 0);
  return roots;
}

export function flattenProjectTree(forest: ProjectTreeNode[]): ProjectTreeNode[] {
  const out: ProjectTreeNode[] = [];
  const walk = (nodes: ProjectTreeNode[]) => {
    for (const node of nodes) {
      out.push(node);
      walk(node.children);
    }
  };
  walk(forest);
  return out;
}

export function parentCandidates(projects: Project[], excludeId: string | null): ParentOption[] {
  const flat = flattenProjectTree(buildProjectForest(projects));
  const excluded = new Set<string>();
  if (excludeId) {
    const start = flat.find((n) => n.project.id === excludeId);
    if (start) {
      const stack = [start];
      while (stack.length > 0) {
        const node = stack.pop();
        if (!node) break;
        excluded.add(node.project.id);
        stack.push(...node.children);
      }
    } else {
      excluded.add(excludeId);
    }
  }
  return flat
    .filter((n) => !excluded.has(n.project.id))
    .map((n) => ({
      id: n.project.id,
      name: n.project.name,
      status: n.project.status,
      depth: n.depth,
    }));
}
