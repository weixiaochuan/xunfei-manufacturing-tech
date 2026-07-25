import { Space, Tag } from "antd";
import {
  ClockCircleOutlined,
  FileTextOutlined,
} from "@ant-design/icons";
import { useAppStore } from "@/store";
import type { SessionStatus } from "@/types";

const STATUS_LABELS: Record<SessionStatus, { color: string; label: string }> = {
  idle: { color: "default", label: "就绪" },
  running: { color: "processing", label: "执行中" },
  waiting_confirm: { color: "warning", label: "等待确认" },
  paused: { color: "default", label: "已暂停" },
  completed: { color: "success", label: "已完成" },
};

export default function TaskSessionStatusBar() {
  const session = useAppStore((s) => s.activeSession);
  const phases = useAppStore((s) => s.activeSessionPhases);

  if (!session) return null;

  const completedPhases = phases.filter((p) => p.status === "completed").length;
  const statusInfo = STATUS_LABELS[session.status] ?? STATUS_LABELS.idle;

  const filesModified = phases
    .filter((p) => p.status === "completed" && p.files_modified)
    .reduce((sum, p) => {
      try {
        const arr: string[] = JSON.parse(p.files_modified!);
        return sum + arr.length;
      } catch {
        return sum;
      }
    }, 0);

  // 简易剩余时间估算（基于已完成的 phase 平均耗时）
  const elapsedPhases = phases.filter(
    (p) => p.started_at && p.finished_at
  );
  let remainingMinutes: number | null = null;
  if (elapsedPhases.length > 0 && completedPhases < phases.length) {
    const avgMs =
      elapsedPhases.reduce((sum, p) => {
        const start = new Date(p.started_at!).getTime();
        const end = new Date(p.finished_at!).getTime();
        return sum + (end - start);
      }, 0) / elapsedPhases.length;
    const remainingPhases = phases.length - completedPhases;
    remainingMinutes = Math.max(1, Math.round((avgMs * remainingPhases) / 60000));
  }

  return (
    <div className="flex items-center gap-4 px-4 py-1.5 text-xs text-gray-400 border-t border-gray-200 dark:border-gray-700">
      <Space size="small">
        <Tag color={statusInfo.color}>{statusInfo.label}</Tag>
      </Space>
      <span>
        Phase: {completedPhases}/{session.total_phases}
      </span>
      {filesModified > 0 && (
        <span className="flex items-center gap-1">
          <FileTextOutlined />
          文件: {filesModified}
        </span>
      )}
      {remainingMinutes !== null && (
        <span className="flex items-center gap-1">
          <ClockCircleOutlined />
          预估剩余: ~{remainingMinutes} 分钟
        </span>
      )}
    </div>
  );
}
