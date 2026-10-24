import {
  createHashHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { AppShell } from "@/components/app-shell";
import { SearchPage } from "@/pages/search";
import { ProjectsPage } from "@/pages/projects";
import { ProjectDetailPage } from "@/pages/project-detail";
import { IndexStatusPage } from "@/pages/index-status";
import { BackupPage } from "@/pages/backup";
import { DailyReportsPage } from "@/pages/daily-reports";
import { TodosPage } from "@/pages/todos";
import { AuditPage } from "@/pages/audit";

const rootRoute = createRootRoute({ component: AppShell });

const searchRoute = createRoute({ getParentRoute: () => rootRoute, path: "/", component: SearchPage });

const projectsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/projects",
  component: ProjectsPage,
});

const projectDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/projects/$projectId",
  component: ProjectDetailPage,
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/index",
  component: IndexStatusPage,
});

const backupRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/backup",
  component: BackupPage,
});

const todosRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/todos",
  component: TodosPage,
});

const dailyReportsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/daily-reports",
  component: DailyReportsPage,
});

const auditRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/audit",
  component: AuditPage,
});

const routeTree = rootRoute.addChildren([
  searchRoute,
  projectsRoute,
  projectDetailRoute,
  indexRoute,
  backupRoute,
  todosRoute,
  dailyReportsRoute,
  auditRoute,
]);

// hash history：Tauri 生产环境走打包后的自定义协议，browser history 的深链刷新会丢路由
export const router = createRouter({ routeTree, history: createHashHistory() });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
