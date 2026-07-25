import { create } from "zustand";
import { Store } from "@tauri-apps/plugin-store";

/**
 * 首页可排序 Widget 标识（6 个独立项）
 *
 * 旧版（v1）合并 widget：todo-recent / pinned-ai
 * 新版（v2）拆分独立：todo / recent / pinned / ai
 */
export type HomeWidgetId =
  | "quick-note"
  | "todo"
  | "recent"
  | "pinned"
  | "ai"
  | "writing-stat";

/** Widget 宽度档位（对应 12 栅格列数） */
export type WidgetSize = "sm" | "md" | "lg" | "xl";

/** Widget 高度档位 */
export type WidgetHeight = "compact" | "normal" | "tall";

/** 列宽映射：sm=4(1/3) md=6(1/2) lg=8(2/3) xl=12(满宽)。一行最多 3 个 sm */
export const COL_SPAN_MAP: Record<WidgetSize, number> = {
  sm: 4,
  md: 6,
  lg: 8,
  xl: 12,
};

/** 高度映射（min-height px） */
export const HEIGHT_MAP: Record<WidgetHeight, number> = {
  compact: 160,
  normal: 260,
  tall: 380,
};

export interface WidgetLayout {
  id: HomeWidgetId;
  colSpan: WidgetSize;
  heightLevel: WidgetHeight;
}

/** 每种组件的安全回退尺寸。兼容旧布局时，非默认组件仍需要可用尺寸。 */
const WIDGET_DEFAULTS: Record<
  HomeWidgetId,
  Pick<WidgetLayout, "colSpan" | "heightLevel">
> = {
  "quick-note": { colSpan: "xl", heightLevel: "compact" },
  "writing-stat": { colSpan: "xl", heightLevel: "compact" },
  recent: { colSpan: "lg", heightLevel: "tall" },
  todo: { colSpan: "sm", heightLevel: "tall" },
  pinned: { colSpan: "md", heightLevel: "normal" },
  ai: { colSpan: "md", heightLevel: "normal" },
};

/**
 * 新安装或无持久化记录时使用的精简布局：
 *   ① 快速记一笔  满宽
 *   ② 写作活力    满宽
 *   ③ 最近编辑    2/3（高卡片）
 *   ④ 待办速览    1/3（高卡片）
 *
 * 置顶文档和问 AI 只从首页布局移除；对应业务功能和路由仍保留在其他入口。
 */
export const DEFAULT_LAYOUT: WidgetLayout[] = [
  { id: "quick-note", colSpan: "xl", heightLevel: "compact" },
  { id: "writing-stat", colSpan: "xl", heightLevel: "compact" },
  { id: "recent", colSpan: "lg", heightLevel: "tall" },
  { id: "todo", colSpan: "sm", heightLevel: "tall" },
];

const STORE_FILE = "home-layout.json";

function isHomeWidgetId(id: unknown): id is HomeWidgetId {
  return (
    id === "quick-note" ||
    id === "todo" ||
    id === "recent" ||
    id === "pinned" ||
    id === "ai" ||
    id === "writing-stat"
  );
}

/** 将 v1 合并卡片顺序迁移到当前四卡片结构。 */
export function migrateLegacyHomeOrder(legacy: string[]): WidgetLayout[] {
  if (
    legacy.includes("pinned-ai") ||
    legacy.includes("pinned") ||
    legacy.includes("ai")
  ) {
    return DEFAULT_LAYOUT.map((item) => ({ ...item }));
  }
  const expanded: HomeWidgetId[] = [];
  const append = (id: HomeWidgetId) => {
    if (!expanded.includes(id)) expanded.push(id);
  };

  for (const id of legacy) {
    if (id === "todo-recent") {
      append("todo");
      append("recent");
    } else if (id === "pinned-ai") {
      append("pinned");
      append("ai");
    } else if (isHomeWidgetId(id)) {
      append(id);
    }
  }
  for (const def of DEFAULT_LAYOUT) append(def.id);

  return expanded.map((id) => ({ id, ...WIDGET_DEFAULTS[id] }));
}

/** 校验持久化布局；检测到旧首页卡片时迁移为当前四卡片结构。 */
export function normalizeHomeWidgets(input: unknown): WidgetLayout[] {
  if (!Array.isArray(input)) return DEFAULT_LAYOUT.map((item) => ({ ...item }));
  const containsRetiredWidget = input.some((item) => {
    if (!item || typeof item !== "object") return false;
    const id = (item as Partial<WidgetLayout>).id;
    return id === "pinned" || id === "ai";
  });
  if (containsRetiredWidget) {
    return DEFAULT_LAYOUT.map((item) => ({ ...item }));
  }
  const seen = new Set<HomeWidgetId>();
  const result: WidgetLayout[] = [];
  for (const item of input) {
    if (!item || typeof item !== "object") continue;
    const widget = item as Partial<WidgetLayout>;
    if (!isHomeWidgetId(widget.id) || seen.has(widget.id)) continue;
    seen.add(widget.id);
    const fallback = WIDGET_DEFAULTS[widget.id];
    result.push({
      id: widget.id,
      colSpan:
        widget.colSpan && widget.colSpan in COL_SPAN_MAP
          ? widget.colSpan
          : fallback.colSpan,
      heightLevel:
        widget.heightLevel && widget.heightLevel in HEIGHT_MAP
          ? widget.heightLevel
          : fallback.heightLevel,
    });
  }
  for (const def of DEFAULT_LAYOUT) {
    if (!seen.has(def.id)) result.push({ ...def });
  }
  return result;
}

interface HomeLayoutState {
  /** Widget 布局列表（顺序 + 宽度 + 高度） */
  widgets: WidgetLayout[];
  /** 折叠状态 */
  collapsed: Partial<Record<HomeWidgetId, boolean>>;

  /** 从持久化恢复（首次调用） */
  hydrate: () => Promise<void>;
  /** 拖拽结束更新顺序（保留 colSpan / heightLevel） */
  setOrder: (ids: HomeWidgetId[]) => void;
  /** 修改某个 widget 的宽度 */
  setColSpan: (id: HomeWidgetId, size: WidgetSize) => void;
  /** 修改某个 widget 的高度 */
  setHeight: (id: HomeWidgetId, h: WidgetHeight) => void;
  /** 切换折叠 */
  toggleCollapse: (id: HomeWidgetId) => void;
  /** 重置为默认布局 */
  resetLayout: () => void;
}

export const useHomeLayoutStore = create<HomeLayoutState>((set, get) => {
  let storePromise: Promise<Store> | null = null;

  function getStore(): Promise<Store> {
    if (!storePromise) storePromise = Store.load(STORE_FILE);
    return storePromise;
  }

  async function persist() {
    const { widgets, collapsed } = get();
    const store = await getStore();
    await store.set("widgets", widgets);
    await store.set("collapsed", collapsed);
    await store.save();
  }

  return {
    widgets: [...DEFAULT_LAYOUT],
    collapsed: {},

    hydrate: async () => {
      try {
        const store = await getStore();
        const savedWidgets = await store.get<WidgetLayout[]>("widgets");
        const legacyOrder = await store.get<string[]>("order");
        const savedCollapsed = await store.get<Record<string, boolean>>(
          "collapsed",
        );

        let widgets: WidgetLayout[];
        if (savedWidgets) {
          widgets = normalizeHomeWidgets(savedWidgets);
        } else if (Array.isArray(legacyOrder)) {
          widgets = migrateLegacyHomeOrder(legacyOrder);
        } else {
          widgets = [...DEFAULT_LAYOUT];
        }

        const collapsed = Object.fromEntries(
          Object.entries(savedCollapsed ?? {}).filter(
            ([id]) => id !== "pinned" && id !== "ai",
          ),
        ) as Partial<Record<HomeWidgetId, boolean>>;

        set({ widgets, collapsed });

        const layoutChanged =
          (savedWidgets &&
            JSON.stringify(savedWidgets) !== JSON.stringify(widgets)) ||
          (!savedWidgets && Array.isArray(legacyOrder));
        const collapsedChanged =
          JSON.stringify(savedCollapsed ?? {}) !== JSON.stringify(collapsed);
        if (layoutChanged || collapsedChanged) {
          await store.set("widgets", widgets);
          await store.set("collapsed", collapsed);
          await store.save();
        }
      } catch {
        // 文件不存在等，用默认值
      }
    },

    setOrder: (ids) => {
      set((s) => {
        const map = new Map(s.widgets.map((w) => [w.id, w]));
        const next = ids
          .map((id) => map.get(id))
          .filter((w): w is WidgetLayout => !!w);
        // 兜底：把未出现在 ids 里的 widget 追加到末尾，避免丢失
        for (const w of s.widgets) {
          if (!next.find((n) => n.id === w.id)) next.push(w);
        }
        return { widgets: next };
      });
      void persist();
    },

    setColSpan: (id, size) => {
      set((s) => ({
        widgets: s.widgets.map((w) =>
          w.id === id ? { ...w, colSpan: size } : w,
        ),
      }));
      void persist();
    },

    setHeight: (id, h) => {
      set((s) => ({
        widgets: s.widgets.map((w) =>
          w.id === id ? { ...w, heightLevel: h } : w,
        ),
      }));
      void persist();
    },

    toggleCollapse: (id) => {
      set((s) => ({
        collapsed: { ...s.collapsed, [id]: !s.collapsed[id] },
      }));
      void persist();
    },

    resetLayout: () => {
      set({ widgets: [...DEFAULT_LAYOUT], collapsed: {} });
      void persist();
    },
  };
});
