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

/**
 * 默认布局（用户需求版本）：
 *   ① 快速记一笔  满宽
 *   ② 置顶文档 + 待办速览  各 1/2
 *   ③ 最近文档 + 问 AI      各 1/2
 *   ④ 写作活力              满宽
 */
export const DEFAULT_LAYOUT: WidgetLayout[] = [
  { id: "quick-note", colSpan: "xl", heightLevel: "compact" },
  { id: "pinned", colSpan: "md", heightLevel: "normal" },
  { id: "todo", colSpan: "md", heightLevel: "normal" },
  { id: "recent", colSpan: "md", heightLevel: "normal" },
  { id: "ai", colSpan: "md", heightLevel: "normal" },
  { id: "writing-stat", colSpan: "xl", heightLevel: "compact" },
];

const STORE_FILE = "home-layout.json";

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

  /** v1 → v2 迁移：把旧的合并 widget 拆为独立 widget */
  function migrateLegacyOrder(legacy: string[]): WidgetLayout[] {
    const expanded: HomeWidgetId[] = [];
    for (const id of legacy) {
      if (id === "todo-recent") {
        expanded.push("todo", "recent");
      } else if (id === "pinned-ai") {
        expanded.push("pinned", "ai");
      } else if (
        id === "quick-note" ||
        id === "writing-stat" ||
        id === "todo" ||
        id === "recent" ||
        id === "pinned" ||
        id === "ai"
      ) {
        expanded.push(id);
      }
    }
    // 补齐缺失 widget，确保 6 个都存在
    for (const def of DEFAULT_LAYOUT) {
      if (!expanded.includes(def.id)) expanded.push(def.id);
    }
    return expanded.map((id) => {
      const def = DEFAULT_LAYOUT.find((w) => w.id === id)!;
      return { id, colSpan: def.colSpan, heightLevel: def.heightLevel };
    });
  }

  /** 校验 + 补齐：确保 widgets 数组包含全部 6 个 id（去重 / 补缺失） */
  function normalizeWidgets(input: unknown): WidgetLayout[] {
    if (!Array.isArray(input)) return [...DEFAULT_LAYOUT];
    const seen = new Set<HomeWidgetId>();
    const result: WidgetLayout[] = [];
    for (const item of input) {
      if (!item || typeof item !== "object") continue;
      const w = item as Partial<WidgetLayout>;
      const id = w.id;
      if (
        id !== "quick-note" &&
        id !== "todo" &&
        id !== "recent" &&
        id !== "pinned" &&
        id !== "ai" &&
        id !== "writing-stat"
      )
        continue;
      if (seen.has(id)) continue;
      seen.add(id);
      const def = DEFAULT_LAYOUT.find((d) => d.id === id)!;
      result.push({
        id,
        colSpan: w.colSpan && w.colSpan in COL_SPAN_MAP ? w.colSpan : def.colSpan,
        heightLevel:
          w.heightLevel && w.heightLevel in HEIGHT_MAP
            ? w.heightLevel
            : def.heightLevel,
      });
    }
    // 补齐缺失
    for (const def of DEFAULT_LAYOUT) {
      if (!seen.has(def.id)) result.push({ ...def });
    }
    return result;
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
          widgets = normalizeWidgets(savedWidgets);
        } else if (Array.isArray(legacyOrder)) {
          widgets = migrateLegacyOrder(legacyOrder);
        } else {
          widgets = [...DEFAULT_LAYOUT];
        }

        set({
          widgets,
          collapsed:
            (savedCollapsed as Partial<Record<HomeWidgetId, boolean>>) ?? {},
        });
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
