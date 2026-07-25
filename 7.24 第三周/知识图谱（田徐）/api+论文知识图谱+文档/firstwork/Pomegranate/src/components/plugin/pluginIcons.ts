/**
 * 插件 UI 共享：把"图标名字符串 → Lucide 组件"映射集中到这里，
 * 让 ActivityBar / RibbonBar / 编辑器扩展按钮都能复用同一份。
 *
 * 扩展方式：直接在 ICON_MAP 加新条目。插件作者用 `icon: "Star"` 等字符串引用。
 * 名字匹配规则：转小写后去掉 `-_空格`，未命中 → Puzzle 兜底。
 */

import {
  Bell,
  Bookmark,
  Bot,
  Calendar,
  CheckSquare,
  Edit3,
  EyeOff,
  FileText,
  Folder,
  GitBranch,
  Globe,
  Heart,
  Home,
  Image,
  Info,
  Layers,
  Link2,
  Map as MapIcon,
  Mic,
  Music,
  NotebookText,
  Pencil,
  Pin,
  Plug,
  Puzzle,
  Search,
  Send,
  Settings,
  Smile,
  Sparkles,
  Star,
  Tags,
  ThumbsUp,
  Trash2,
  Video,
  Zap,
  type LucideIcon,
} from "lucide-react";

export const ICON_MAP: Record<string, LucideIcon> = {
  bell: Bell,
  bookmark: Bookmark,
  bot: Bot,
  calendar: Calendar,
  checksquare: CheckSquare,
  edit3: Edit3,
  eyeoff: EyeOff,
  filetext: FileText,
  folder: Folder,
  gitbranch: GitBranch,
  globe: Globe,
  heart: Heart,
  home: Home,
  image: Image,
  info: Info,
  layers: Layers,
  link2: Link2,
  map: MapIcon,
  mic: Mic,
  music: Music,
  notebooktext: NotebookText,
  pencil: Pencil,
  pin: Pin,
  plug: Plug,
  puzzle: Puzzle,
  search: Search,
  send: Send,
  settings: Settings,
  smile: Smile,
  sparkles: Sparkles,
  star: Star,
  tags: Tags,
  thumbsup: ThumbsUp,
  trash2: Trash2,
  video: Video,
  zap: Zap,
};

/** 解析插件传入的图标名，返回对应 Lucide 组件；未命中返回 Puzzle */
export function resolvePluginIconComponent(iconName: string): LucideIcon {
  const key = iconName.toLowerCase().replace(/[-_\s]/g, "");
  return ICON_MAP[key] ?? Puzzle;
}
