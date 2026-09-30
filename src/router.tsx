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

const routeTree = rootRoute.addChildren([searchRoute, projectsRoute, projectDetailRoute]);

// hash history：Tauri 生产环境走打包后的自定义协议，browser history 的深链刷新会丢路由
export const router = createRouter({ routeTree, history: createHashHistory() });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
