export type ShortcutScope = "global" | "app" | "editor";

export interface ShortcutRegistryItem {
  id: string;
  title: string;
  desc?: string;
  group: string;
  scope: ShortcutScope;
  defaultAccel: string;
}

export const SHORTCUT_GROUPS = ["全局", "应用", "编辑器"];

export const SHORTCUT_REGISTRY: ShortcutRegistryItem[] = [
  {
    id: "global.quickCapture",
    title: "剪贴板快速捕获",
    desc: "从剪贴板快速创建笔记",
    group: "全局",
    scope: "global",
    defaultAccel: "CommandOrControl+Shift+N",
  },
  {
    id: "global.showWindow",
    title: "显示主窗口",
    desc: "显示并聚焦主窗口",
    group: "全局",
    scope: "global",
    defaultAccel: "CommandOrControl+Alt+K",
  },
  {
    id: "global.openDaily",
    title: "打开今日日记",
    desc: "打开今日日记",
    group: "全局",
    scope: "global",
    defaultAccel: "CommandOrControl+Alt+D",
  },
  {
    id: "global.openSearch",
    title: "打开全局搜索",
    desc: "打开全局搜索",
    group: "全局",
    scope: "global",
    defaultAccel: "CommandOrControl+Alt+F",
  },
  {
    id: "app.commandPalette",
    title: "命令面板",
    desc: "打开命令面板",
    group: "应用",
    scope: "app",
    defaultAccel: "CommandOrControl+K",
  },
  {
    id: "app.shortcutsHelp",
    title: "快捷键帮助",
    desc: "打开快捷键帮助面板",
    group: "应用",
    scope: "app",
    defaultAccel: "F1",
  },
  {
    id: "editor.compareNotes",
    title: "对比文档",
    desc: "与其他文档对比并合并",
    group: "编辑器",
    scope: "editor",
    defaultAccel: "CommandOrControl+Shift+M",
  },
];

export type ShortcutDef = ShortcutRegistryItem;
export const SHORTCUTS = SHORTCUT_REGISTRY;

export function findShortcut(id: string) {
  return SHORTCUT_REGISTRY.find((item) => item.id === id) ?? null;
}

export function accelToKeys(accel: string): string[] {
  return accel ? accel.split("+") : [];
}

export function keyboardEventToAccel(event: KeyboardEvent | React.KeyboardEvent): string {
  const keys: string[] = [];
  if (event.ctrlKey) keys.push("Ctrl");
  if (event.shiftKey) keys.push("Shift");
  if (event.altKey) keys.push("Alt");
  keys.push(event.key);
  return keys.join("+");
}

export function isMacPlatform() {
  return navigator.platform.toLowerCase().includes("mac");
}
