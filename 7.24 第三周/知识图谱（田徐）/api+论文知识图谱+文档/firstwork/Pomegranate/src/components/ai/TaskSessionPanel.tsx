import { useEffect, useState } from "react";
import { Typography, Progress } from "antd";
import { useSearchParams, useNavigate } from "react-router-dom";
import { useAppStore } from "@/store";
import { sessionApi } from "@/lib/api";
import TaskSessionPlanSidebar from "./TaskSessionPlanSidebar";
import TaskSessionChat from "./TaskSessionChat";
import TaskSessionControlBar from "./TaskSessionControlBar";
import TaskSessionStatusBar from "./TaskSessionStatusBar";
import SessionInitModal from "./SessionInitModal";

const { Text } = Typography;

function calcPercent(phases: { status: string }[]) {
  if (!phases.length) return 0;
  const done = phases.filter((p) => p.status === "completed").length;
  return Math.round((done / phases.length) * 100);
}

export default function TaskSessionPanel() {
  const [searchParams] = useSearchParams();
  const sessionId = searchParams.get("sessionId");

  const setActiveSession = useAppStore((s) => s.setActiveSession);
  const activeSession = useAppStore((s) => s.activeSession);
  const activePhases = useAppStore((s) => s.activeSessionPhases);
  const setSessionExecuting = useAppStore((s) => s.setSessionExecuting);

  const [loading, setLoading] = useState(false);
  const [initModalOpen, setInitModalOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 加载已有会话，或显示空状态
  useEffect(() => {
    if (!sessionId) {
      setActiveSession(null);
      return;
    }

    setLoading(true);
    setError(null);
    sessionApi
      .get(sessionId)
      .then((detail) => {
        setActiveSession(detail.session, detail.phases);
        setSessionExecuting(false);
      })
      .catch((e) => {
        setError(String(e));
        setActiveSession(null);
      })
      .finally(() => setLoading(false));
  }, [sessionId, setActiveSession, setSessionExecuting]);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full text-gray-400">
        加载中...
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4">
        <Text type="danger">加载失败: {error}</Text>
        <SessionInitModal open={initModalOpen} onClose={() => setInitModalOpen(false)} />
      </div>
    );
  }

  if (!activeSession && !sessionId) {
    // 空状态：提示创建或查看会话
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4 text-gray-400">
        <Text type="secondary">暂无活跃的任务执行会话</Text>
        <div className="flex gap-2">
          <TaskSessionListModal />
        </div>
      </div>
    );
  }

  if (!activeSession) {
    return (
      <div className="flex items-center justify-center h-full text-gray-400">
        会话数据为空
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      {/* 顶部标题栏 */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-gray-200 dark:border-gray-700">
        <Text strong className="text-sm">{activeSession.plan_name}</Text>
        <div className="flex items-center gap-2">
          <Text type="secondary" className="text-xs">
            {activeSession.current_phase_index}/{activeSession.total_phases} phases
          </Text>
        </div>
      </div>

      {/* 主体内容：Chat + 右侧任务进度面板 */}
      <div className="flex flex-1 min-h-0">
        <div className="flex-1 min-w-0">
          <TaskSessionChat />
        </div>
        <div className="w-[260px] flex-shrink-0 border-l border-gray-200 dark:border-gray-700 flex flex-col min-h-0">
          {/* 右侧上方进度 */}
          <div className="flex flex-col items-center gap-2 p-4 border-b border-gray-200 dark:border-gray-700">
            <Progress
              type="circle"
              percent={calcPercent(activePhases)}
              size={64}
              strokeWidth={6}
              format={(_p) => (
                <span className="text-xs font-medium">
                  {activePhases.filter((ph) => ph.status === "completed").length}/{activePhases.length}
                </span>
              )}
            />
            <div className="text-xs text-gray-400 text-center leading-tight">
              {activePhases.filter((ph) => ph.status === "completed").length === activePhases.length
                ? "已完成"
                : activePhases.some((ph) => ph.status === "running")
                  ? "执行中"
                  : "待执行"}
            </div>
          </div>
          {/* 右侧下方任务计划 */}
          <div className="flex-1 min-h-0">
            <TaskSessionPlanSidebar />
          </div>
        </div>
      </div>

      {/* 底部栏 */}
      <TaskSessionControlBar />
      <TaskSessionStatusBar />

      {/* 新建 Modal 供外部使用 */}
      <SessionInitModal open={initModalOpen} onClose={() => setInitModalOpen(false)} />
    </div>
  );
}

/** 会话列表选择器（空状态时显示） */
function TaskSessionListModal() {
  const [sessions, setSessions] = useState<{ id: string; name: string }[]>([]);
  const navigate = useNavigate();

  useEffect(() => {
    sessionApi.list().then((list) => {
      setSessions(
        list.map((s) => ({ id: s.id, name: s.plan_name }))
      );
    }).catch(() => {});
  }, []);

  if (sessions.length === 0) return null;

  return (
    <div className="text-center">
      <Text type="secondary" className="text-xs block mb-2">
        或选择已有会话：
      </Text>
      <div className="flex flex-wrap gap-1 justify-center">
        {sessions.slice(0, 5).map((s) => (
          <button
            key={s.id}
            onClick={() => navigate(`/task-session?sessionId=${s.id}`)}
            className="px-2 py-1 text-xs bg-gray-100 dark:bg-gray-800 rounded hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors"
          >
            {s.name}
          </button>
        ))}
      </div>
    </div>
  );
}
