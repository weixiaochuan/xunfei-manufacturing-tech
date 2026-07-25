import { Button, Space, App } from "antd";
import {
  PlayCircleOutlined,
  PauseCircleOutlined,
  CheckCircleOutlined,
  ForwardOutlined,
  ReloadOutlined,
  ExportOutlined,
  DeleteOutlined,
} from "@ant-design/icons";
import { useAppStore } from "@/store";
import { sessionApi } from "@/lib/api";
import { useNavigate } from "react-router-dom";

export default function TaskSessionControlBar() {
  const { message, modal } = App.useApp();
  const navigate = useNavigate();
  const session = useAppStore((s) => s.activeSession);
  const setActiveSession = useAppStore((s) => s.setActiveSession);
  const setSessionExecuting = useAppStore((s) => s.setSessionExecuting);
  const sessionId = session?.id;

  if (!session) return null;

  const status = session.status;

  async function handleStart() {
    if (!sessionId) return;
    try {
      setSessionExecuting(true);
      const phaseIdx = session?.current_phase_index ?? 0;
      await sessionApi.startPhase(sessionId, phaseIdx);
      // 重新加载 session
      const detail = await sessionApi.get(sessionId);
      setActiveSession(detail.session, detail.phases);
    } catch (e) {
      message.error(String(e));
    } finally {
      setSessionExecuting(false);
    }
  }

  async function handlePause() {
    if (!sessionId) return;
    try {
      await sessionApi.pause(sessionId);
      const detail = await sessionApi.get(sessionId);
      setActiveSession(detail.session, detail.phases);
      message.success("已暂停");
    } catch (e) {
      message.error(String(e));
    }
  }

  async function handleResume() {
    if (!sessionId) return;
    try {
      await sessionApi.resume(sessionId);
      const detail = await sessionApi.get(sessionId);
      setActiveSession(detail.session, detail.phases);
    } catch (e) {
      message.error(String(e));
    }
  }

  async function handleConfirm() {
    if (!sessionId) return;
    try {
      await sessionApi.confirmPhase(sessionId);
      const detail = await sessionApi.get(sessionId);
      setActiveSession(detail.session, detail.phases);
      message.success("已确认，进入下一阶段");
    } catch (e) {
      message.error(String(e));
    }
  }

  async function handleSkip() {
    if (!sessionId) return;
    const phaseIdx = session?.current_phase_index ?? 0;
    try {
      await sessionApi.skipPhase(sessionId, phaseIdx);
      const detail = await sessionApi.get(sessionId);
      setActiveSession(detail.session, detail.phases);
      message.success("已跳过");
    } catch (e) {
      message.error(String(e));
    }
  }

  async function handleRetry() {
    if (!sessionId) return;
    const phaseIdx = session?.current_phase_index ?? 0;
    try {
      await sessionApi.retryPhase(sessionId, phaseIdx);
      const detail = await sessionApi.get(sessionId);
      setActiveSession(detail.session, detail.phases);
      message.success("已重置当前阶段");
    } catch (e) {
      message.error(String(e));
    }
  }

  async function handleExport() {
    if (!sessionId) return;
    try {
      const path = await sessionApi.exportLogs(sessionId);
      message.success(`日志已导出到: ${path}`);
    } catch (e) {
      message.error(String(e));
    }
  }

  function handleDelete() {
    if (!sessionId) return;
    modal.confirm({
      title: "删除会话",
      content: "确定要删除此任务执行会话吗？执行日志将一并删除。",
      okText: "删除",
      okType: "danger",
      cancelText: "取消",
      onOk: async () => {
        try {
          await sessionApi.delete(sessionId);
          setActiveSession(null);
          navigate("/task-session");
          message.success("会话已删除");
        } catch (e) {
          message.error(String(e));
        }
      },
    });
  }

  const renderButtons = () => {
    switch (status) {
      case "idle":
        return (
          <Button type="primary" icon={<PlayCircleOutlined />} onClick={handleStart}>
            开始执行
          </Button>
        );
      case "running":
        return [
          <Button key="pause" icon={<PauseCircleOutlined />} onClick={handlePause}>
            暂停
          </Button>,
        ];
      case "waiting_confirm":
        return [
          <Button
            key="confirm"
            type="primary"
            icon={<CheckCircleOutlined />}
            onClick={handleConfirm}
          >
            确认继续
          </Button>,
          <Button key="skip" icon={<ForwardOutlined />} onClick={handleSkip}>
            跳过
          </Button>,
          <Button key="retry" icon={<ReloadOutlined />} onClick={handleRetry}>
            重试
          </Button>,
          <Button key="pause" icon={<PauseCircleOutlined />} onClick={handlePause}>
            暂停
          </Button>,
        ];
      case "paused":
        return (
          <Button type="primary" icon={<PlayCircleOutlined />} onClick={handleResume}>
            继续
          </Button>
        );
      case "completed":
        return [
          <Button key="export" icon={<ExportOutlined />} onClick={handleExport}>
            导出日志
          </Button>,
          <Button key="delete" danger icon={<DeleteOutlined />} onClick={handleDelete}>
            删除会话
          </Button>,
        ];
      default:
        return null;
    }
  };

  const buttons = renderButtons();
  if (!buttons) return null;

  return (
    <div className="flex items-center gap-2 px-4 py-2 border-t border-gray-200 dark:border-gray-700">
      <Space>{Array.isArray(buttons) ? buttons : [buttons]}</Space>
    </div>
  );
}
