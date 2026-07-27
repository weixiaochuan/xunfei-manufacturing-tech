import { useRef, useSyncExternalStore } from "react";
import { pluginManager } from "@/services/pluginManager";

export interface TaskViewDef {
  id: string;
  label: string;
  icon: string;
  pluginId: string;
}

/**
 * 订阅插件注册的任务视图列表。
 * 在任务页视图切换器中与内置视图合并显示。
 *
 * 使用 useRef 缓存快照引用，避免 getSnapshot 每次返回新数组
 * 导致 useSyncExternalStore 检测到"变化"而无限重渲染。
 */
export function usePluginTaskViews(): TaskViewDef[] {
  const cacheRef = useRef<TaskViewDef[]>([]);

  const subscribe = (onStoreChange: () => void) =>
    pluginManager.subscribe("views", onStoreChange);

  const getSnapshot = () => {
    const raw = pluginManager.getAllTaskViews();
    const next: TaskViewDef[] = raw.map((v) => ({
      id: v.id,
      label: v.label,
      icon: v.icon,
      pluginId: v.pluginId,
    }));
    // 仅当内容真正变化时才更新引用（useSyncExternalStore 用 Object.is 比较）
    const prev = cacheRef.current;
    if (
      prev.length === next.length &&
      prev.every((p, i) => p.id === next[i].id && p.pluginId === next[i].pluginId)
    ) {
      return prev;
    }
    cacheRef.current = next;
    return next;
  };

  return useSyncExternalStore(subscribe, getSnapshot);
}
