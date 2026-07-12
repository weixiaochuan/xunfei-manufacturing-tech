import { createHashRouter, RouterProvider } from "react-router-dom";
import { Suspense, lazy } from "react";
import { Spin } from "antd";
import { LayoutSwitch } from "@/components/layout/LayoutSwitch";
import { RouteErrorFallback } from "@/components/ui/ErrorBoundary";
// 首屏常用页面 — 同步加载，不拆 chunk
import HomePage from "@/pages/home";
import NoteListPage from "@/pages/notes";
import NoteEditorPage from "@/pages/notes/editor";
import SearchPage from "@/pages/search";
import TagsPage from "@/pages/tags";
import TrashPage from "@/pages/trash";
import DailyPage from "@/pages/daily";
import SettingsPage from "@/pages/settings";
import AboutPage from "@/pages/about";
// 非首屏页面 — React.lazy 动态加载，减少主 bundle 体积
const GraphPage = lazy(() => import("@/pages/graph"));
const AiChatPage = lazy(() => import("@/pages/ai"));
const MobileAiChat = lazy(() => import("@/pages/ai/MobileAiChat").then(m => ({ default: m.MobileAiChat })));
const MobileTaskDetail = lazy(() => import("@/pages/tasks/MobileTaskDetail").then(m => ({ default: m.MobileTaskDetail })));
const MobileSync = lazy(() => import("@/pages/sync/MobileSync").then(m => ({ default: m.MobileSync })));
const TasksPage = lazy(() => import("@/pages/tasks"));
const CardsPage = lazy(() => import("@/pages/cards"));
const PromptsPage = lazy(() => import("@/pages/prompts"));
const PluginsPage = lazy(() => import("@/pages/plugins"));
const PptGenerationPage = lazy(() => import("@/pages/ppt-generation"));
const LearningAssistantPage = lazy(() => import("@/pages/learning-assistant"));
const PluginPanelViewPage = lazy(() => import("@/pages/plugins/PluginPanelViewPage"));
const HiddenPage = lazy(() => import("@/pages/hidden"));
const QuickCreatePage = lazy(() => import("@/pages/quick-create"));
const QuickCapturePage = lazy(() => import("@/pages/quick-capture"));
const FeatureTogglePage = lazy(() => import("@/pages/feature-toggle"));
const MigrationSplash = lazy(() => import("@/pages/migration-splash"));
const EmergencyReminderPage = lazy(() => import("@/pages/emergency-reminder"));
const TaskSessionPage = lazy(() => import("@/pages/task-session"));

/** 懒加载页面统一 Suspense fallback */
function LazyPage({ children }: { children: React.ReactNode }) {
  return (
    <Suspense
      fallback={
        <div className="flex items-center justify-center h-full min-h-[200px]">
          <Spin />
        </div>
      }
    >
      {children}
    </Suspense>
  );
}

// 路由级 errorElement：路由内任何同步渲染异常（如 TipTap 在老 WebView 上
// 的 lookbehind 正则解析失败）都会被 RouteErrorFallback 接管，给用户友好
// 提示而非 react-router v7 默认的"Hey developer"开发警告页。
const router = createHashRouter([
  // T-013 完整版：迁移 splash 独立 URL，不走 AppLayout（启动期 db 还没初始化）
  {
    path: "/migration-splash",
    element: <LazyPage><MigrationSplash /></LazyPage>,
    errorElement: <RouteErrorFallback />,
  },
  // 紧急待办接管窗口：独立 URL，不挂 AppLayout，避免 Sider/Header 跑出来
  {
    path: "/emergency-reminder/:id",
    element: <LazyPage><EmergencyReminderPage /></LazyPage>,
    errorElement: <RouteErrorFallback />,
  },
  // 移动端 AI 聊天页：独立全屏路由（不走 MobileLayout / AppLayout）
  {
    path: "/ai-chat/:id",
    element: <LazyPage><MobileAiChat /></LazyPage>,
    errorElement: <RouteErrorFallback />,
  },
  // 闪念捕获：沉浸式橙色全屏，独立路由（不显示底栏）
  {
    path: "/quick-capture",
    element: <LazyPage><QuickCapturePage /></LazyPage>,
    errorElement: <RouteErrorFallback />,
  },
  // 任务详情：沉浸式全屏，独立路由
  {
    path: "/task-detail/:id",
    element: <LazyPage><MobileTaskDetail /></LazyPage>,
    errorElement: <RouteErrorFallback />,
  },
  // 移动端云端同步：沉浸式全屏（独立顶层路由）
  {
    path: "/sync",
    element: <LazyPage><MobileSync /></LazyPage>,
    errorElement: <RouteErrorFallback />,
  },
  {
    path: "/",
    element: <LayoutSwitch />,
    errorElement: <RouteErrorFallback />,
    children: [
      { index: true, element: <HomePage /> },
      { path: "notes", element: <NoteListPage /> },
      { path: "notes/:id", element: <NoteEditorPage /> },
      { path: "search", element: <SearchPage /> },
      { path: "tags", element: <TagsPage /> },
      { path: "trash", element: <TrashPage /> },
      { path: "hidden", element: <LazyPage><HiddenPage /></LazyPage> },
      { path: "daily", element: <DailyPage /> },
      { path: "graph", element: <LazyPage><GraphPage /></LazyPage> },
      { path: "ai", element: <LazyPage><AiChatPage /></LazyPage> },
      { path: "prompts", element: <LazyPage><PromptsPage /></LazyPage> },
      { path: "plugins", element: <LazyPage><PluginsPage /></LazyPage> },
      { path: "ppt-generation", element: <LazyPage><PptGenerationPage /></LazyPage> },
      { path: "learning-assistant", element: <LazyPage><LearningAssistantPage /></LazyPage> },
      { path: "plugin-view/:viewId", element: <LazyPage><PluginPanelViewPage /></LazyPage> },
      { path: "tasks", element: <LazyPage><TasksPage /></LazyPage> },
      { path: "task-session", element: <LazyPage><TaskSessionPage /></LazyPage> },
      { path: "cards", element: <LazyPage><CardsPage /></LazyPage> },
      { path: "settings", element: <SettingsPage /> },
      { path: "about", element: <AboutPage /> },
      { path: "quick-create", element: <LazyPage><QuickCreatePage /></LazyPage> },
      { path: "feature-toggle", element: <LazyPage><FeatureTogglePage /></LazyPage> },
    ],
  },
]);

export function AppRouter() {
  return <RouterProvider router={router} />;
}
