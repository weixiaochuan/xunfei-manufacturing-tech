import { type ReactNode } from "react";
import { Card, Select, Tooltip } from "antd";
import { GripVertical, ChevronDown, ChevronRight } from "lucide-react";
import type { WidgetSize, WidgetHeight } from "@/store/homeLayout";

export interface HomeWidgetProps {
  id: string;
  title: string;
  icon?: ReactNode;
  collapsed: boolean;
  onToggle: () => void;
  /** 拖拽手柄 props（由 dnd-kit useSortable 注入） */
  dragHandleProps?: Record<string, unknown>;
  /** antd Card 底部额外元素 */
  extra?: ReactNode;
  /** 当前宽度档位 */
  colSpan?: WidgetSize;
  /** 当前高度档位 */
  heightLevel?: WidgetHeight;
  /** 宽度切换回调 */
  onColSpanChange?: (size: WidgetSize) => void;
  /** 高度切换回调 */
  onHeightChange?: (h: WidgetHeight) => void;
  children: ReactNode;
}

/**
 * 首页可折叠 + 可拖拽 + 可调宽高的 Widget 容器。
 */
export function HomeWidget({
  title,
  icon,
  collapsed,
  onToggle,
  dragHandleProps,
  extra,
  colSpan,
  heightLevel,
  onColSpanChange,
  onHeightChange,
  children,
}: HomeWidgetProps) {
  return (
    <Card
      size="small"
      style={{ height: "100%", display: "flex", flexDirection: "column" }}
      styles={{
        body: {
          padding: collapsed ? 0 : undefined,
          flex: 1,
          minHeight: 0,
          overflow: "hidden",
        },
      }}
      title={
        <div
          className="flex items-center gap-2 cursor-grab active:cursor-grabbing select-none"
          {...dragHandleProps}
        >
          <GripVertical size={14} className="text-gray-400 shrink-0" />
          {icon}
          <span style={{ fontSize: 13, fontWeight: 600, userSelect: "none" }}>
            {title}
          </span>
        </div>
      }
      extra={
        <div className="flex items-center gap-2">
          {extra}
          {/* 宽度下拉：阻止 mousedown 冒泡，避免误触 dnd-kit 拖拽 */}
          {onColSpanChange && colSpan && (
            <Tooltip title="宽度" mouseEnterDelay={0.4}>
              <span onMouseDown={(e) => e.stopPropagation()}>
                <Select
                  size="small"
                  value={colSpan}
                  onChange={(v) => onColSpanChange(v as WidgetSize)}
                  style={{ width: 78 }}
                  options={[
                    { label: "1/3", value: "sm" },
                    { label: "1/2", value: "md" },
                    { label: "2/3", value: "lg" },
                    { label: "满", value: "xl" },
                  ]}
                />
              </span>
            </Tooltip>
          )}
          {/* 高度下拉：阻止 mousedown 冒泡，避免误触 dnd-kit 拖拽 */}
          {onHeightChange && heightLevel && (
            <Tooltip title="高度" mouseEnterDelay={0.4}>
              <span onMouseDown={(e) => e.stopPropagation()}>
                <Select
                  size="small"
                  value={heightLevel}
                  onChange={(v) => onHeightChange(v as WidgetHeight)}
                  style={{ width: 72 }}
                  options={[
                    { label: "矮", value: "compact" },
                    { label: "中", value: "normal" },
                    { label: "高", value: "tall" },
                  ]}
                />
              </span>
            </Tooltip>
          )}
          {/* 折叠/展开 */}
          <button
            type="button"
            className="flex items-center justify-center w-6 h-6 rounded hover:bg-black/5 dark:hover:bg-white/10"
            onClick={(e) => {
              e.stopPropagation();
              onToggle();
            }}
            aria-label={collapsed ? "展开" : "折叠"}
          >
            {collapsed ? (
              <ChevronRight size={14} className="text-gray-400" />
            ) : (
              <ChevronDown size={14} className="text-gray-400" />
            )}
          </button>
        </div>
      }
    >
      {!collapsed && children}
    </Card>
  );
}
