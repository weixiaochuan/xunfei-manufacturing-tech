export type MobileTabKey =
  | "home"
  | "notes"
  | "daily"
  | "tasks"
  | "cards"
  | "ai"
  | "research-assistant"
  | "ai-resources"
  | "marketplace"
  | "tags"
  | "prompts"
  | "hidden"
  | "graph"
  | "search"
  | "trash"
  | "settings";

export interface MobileTabMeta {
  key: MobileTabKey;
  label: string;
  path: string;
  matchPrefixes: string[];
  activeColor?: "primary" | "accent";
}

export const MOBILE_TAB_SLOT_COUNT = 4;

export const MOBILE_TAB_KEYS: MobileTabKey[] = [
  "home",
  "notes",
  "daily",
  "tasks",
  "cards",
  "ai-resources",
  "research-assistant",
  "ai",
  "marketplace",
  "tags",
  "prompts",
  "hidden",
  "graph",
  "search",
  "trash",
];

export const DEFAULT_MOBILE_TAB_KEYS: MobileTabKey[] = ["home", "notes", "daily", "tasks"];

export const MOBILE_TAB_REGISTRY: Record<MobileTabKey, MobileTabMeta> = {
  home: {
    key: "home",
    label: "首页",
    path: "/",
    matchPrefixes: ["/"],
  },
  notes: {
    key: "notes",
    label: "文档",
    path: "/notes",
    matchPrefixes: ["/notes"],
  },
  daily: {
    key: "daily",
    label: "日记",
    path: "/daily",
    matchPrefixes: ["/daily"],
  },
  tasks: {
    key: "tasks",
    label: "待办",
    path: "/tasks",
    matchPrefixes: ["/tasks", "/emergency-reminder"],
  },
  cards: {
    key: "cards",
    label: "闪卡",
    path: "/cards",
    matchPrefixes: ["/cards"],
  },
  ai: {
    key: "ai",
    label: "AI",
    path: "/ai",
    matchPrefixes: ["/ai", "/ai-chat", "/task-session", "/project-sessions"],
    activeColor: "accent",
  },
  "research-assistant": {
    key: "research-assistant",
    label: "助研",
    path: "/research-assistant",
    matchPrefixes: ["/research-assistant"],
    activeColor: "accent",
  },
  "ai-resources": {
    key: "ai-resources",
    label: "资源",
    path: "/ai-resources",
    matchPrefixes: ["/ai-resources"],
    activeColor: "accent",
  },
  marketplace: {
    key: "marketplace",
    label: "市场",
    path: "/marketplace",
    matchPrefixes: ["/marketplace"],
    activeColor: "accent",
  },
  tags: {
    key: "tags",
    label: "标签",
    path: "/tags",
    matchPrefixes: ["/tags"],
  },
  prompts: {
    key: "prompts",
    label: "提示词",
    path: "/prompts",
    matchPrefixes: ["/prompts"],
    activeColor: "accent",
  },
  hidden: {
    key: "hidden",
    label: "隐藏",
    path: "/hidden",
    matchPrefixes: ["/hidden"],
  },
  graph: {
    key: "graph",
    label: "图谱",
    path: "/graph",
    matchPrefixes: ["/graph"],
  },
  search: {
    key: "search",
    label: "搜索",
    path: "/search",
    matchPrefixes: ["/search"],
  },
  trash: {
    key: "trash",
    label: "回收站",
    path: "/trash",
    matchPrefixes: ["/trash"],
  },
  settings: {
    key: "settings",
    label: "我的",
    path: "/settings",
    matchPrefixes: ["/settings", "/about", "/feature-toggle"],
  },
};
