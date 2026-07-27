import { List, Tag, Typography, Collapse } from "antd";
import {
  ClockCircleOutlined,
  LoadingOutlined,
  QuestionCircleOutlined,
  CheckCircleFilled,
  MinusCircleOutlined,
  CloseCircleFilled,
} from "@ant-design/icons";
import { useAppStore } from "@/store";
import type { PhaseStatus } from "@/types";

const { Text } = Typography;

const PHASE_ICONS: Record<PhaseStatus, React.ReactNode> = {
  pending: <ClockCircleOutlined style={{ color: "#bfbfbf" }} />,
  running: <LoadingOutlined style={{ color: "#1677ff" }} />,
  waiting_confirm: <QuestionCircleOutlined style={{ color: "#fa8c16" }} />,
  completed: <CheckCircleFilled style={{ color: "#52c41a" }} />,
  skipped: <MinusCircleOutlined style={{ color: "#bfbfbf" }} />,
  failed: <CloseCircleFilled style={{ color: "#ff4d4f" }} />,
};

const PHASE_LABELS: Record<PhaseStatus, string> = {
  pending: "待执行",
  running: "执行中",
  waiting_confirm: "等待确认",
  completed: "已完成",
  skipped: "已跳过",
  failed: "失败",
};

export default function TaskSessionPlanSidebar() {
  const session = useAppStore((s) => s.activeSession);
  const phases = useAppStore((s) => s.activeSessionPhases);

  if (!session) return null;

  // 已完成项（用于 Collapse 展开查看摘要）
  const completedItems = phases
    .filter((p) => p.status === "completed" && p.result_summary)
    .map((p) => ({
      key: p.id,
      label: <span className="flex items-center gap-1">{PHASE_ICONS.completed} {p.name}</span>,
      children: <Text type="secondary" className="text-xs">{p.result_summary}</Text>,
    }));

  return (
    <div className="flex flex-col h-full border-r border-gray-200 dark:border-gray-700">
      {/* Phase 列表 */}
      <div className="flex-1 overflow-auto">
        <List
          dataSource={phases}
          size="small"
          renderItem={(phase) => {
            const isCurrent = phase.index_num === session.current_phase_index;
            return (
              <List.Item
                className={`px-3 cursor-pointer ${isCurrent ? "bg-blue-50 dark:bg-blue-900/20" : ""}`}
                key={phase.id}
              >
                <div className="flex items-center gap-2 w-full text-xs">
                  <span className="flex-shrink-0">{PHASE_ICONS[phase.status]}</span>
                  <div className="flex-1 min-w-0">
                    <div className={`truncate ${phase.status === "completed" ? "line-through text-gray-400" : ""}`}>
                      {phase.name}
                    </div>
                    {phase.description && (
                      <Text type="secondary" className="text-[10px] line-clamp-1">{phase.description}</Text>
                    )}
                  </div>
                  <Tag className="text-[10px] leading-none px-1" color={
                    phase.status === "completed" ? "success" :
                    phase.status === "running" ? "processing" :
                    phase.status === "failed" ? "error" :
                    "default"
                  }>
                    {PHASE_LABELS[phase.status]}
                  </Tag>
                </div>
              </List.Item>
            );
          }}
        />
      </div>

      {/* 已完成 Phase 摘要 */}
      {completedItems.length > 0 && (
        <div className="border-t border-gray-200 dark:border-gray-700">
          <Collapse
            ghost
            size="small"
            items={[{
              key: "summary",
              label: <Text type="secondary" className="text-xs">已完成摘要 ({completedItems.length})</Text>,
              children: (
                <List
                  dataSource={completedItems}
                  size="small"
                  renderItem={(item) => (
                    <List.Item key={item.key}>
                      <div className="text-xs w-full">
                        <div className="font-medium">{item.label}</div>
                        <div className="text-gray-400 mt-0.5">{item.children}</div>
                      </div>
                    </List.Item>
                  )}
                />
              ),
            }]}
          />
        </div>
      )}
    </div>
  );
}
