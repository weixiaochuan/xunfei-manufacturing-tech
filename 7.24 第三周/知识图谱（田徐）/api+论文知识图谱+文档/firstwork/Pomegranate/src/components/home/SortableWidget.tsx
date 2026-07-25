import { type ReactNode } from "react";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";

interface SortableWidgetProps {
  id: string;
  children: (props: {
    dragHandleProps: Record<string, unknown>;
    /** 是否处于拖拽中（用于视觉反馈） */
    isDragging: boolean;
  }) => ReactNode;
}

/**
 * dnd-kit useSortable 包装器。
 *
 * 将 sortable 的 attributes / listeners / transform 集中处理，
 * 仅透传 dragHandleProps 给子组件，避免每个 Widget 重复样板代码。
 */
export function SortableWidget({ id, children }: SortableWidgetProps) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id });

  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.4 : 1,
  };

  const dragHandleProps: Record<string, unknown> = (listeners as Record<string, unknown>) ?? {};

  return (
    <div ref={setNodeRef} style={style} {...attributes}>
      {children({ dragHandleProps, isDragging })}
    </div>
  );
}
