import { useState, useEffect, useCallback } from "react";
import {
  Button,
  Tooltip,
  Tag,
  Empty,
  message,
  Dropdown,
  Modal,
  Input,
  Form,
} from "antd";
import {
  Plus,
  X,
  FolderOpen,
  GitBranch,
  MoreHorizontal,
  Send,
  FolderSearch,
} from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { projectSessionApi } from "@/lib/api";
import type { ProjectSession, ProjectSessionMessage } from "@/types";

const STATUS_COLOR: Record<string, string> = {
  idle: "default",
  loading: "processing",
  active: "green",
  error: "red",
};

const STATUS_LABEL: Record<string, string> = {
  idle: "待命",
  loading: "加载中",
  active: "活跃",
  error: "异常",
};

export default function ProjectSessionsPage() {
  const [sessions, setSessions] = useState<ProjectSession[]>([]);
  const [activeKey, setActiveKey] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [messages, setMessages] = useState<Record<string, ProjectSessionMessage[]>>({});
  const [createOpen, setCreateOpen] = useState(false);

  // 启动时恢复已打开 Tab
  useEffect(() => {
    loadOpenSessions();
  }, []);

  // 切换 Tab 时加载消息
  useEffect(() => {
    if (activeKey && !messages[activeKey]) {
      loadMessages(activeKey);
    }
  }, [activeKey]);

  async function loadOpenSessions() {
    try {
      const list = await projectSessionApi.listOpen();
      setSessions(list);
      if (list.length > 0) {
        setActiveKey((prev) => prev || list[0].id);
      }
    } catch (e) {
      console.error("加载已打开项目失败:", e);
    }
  }

  async function loadMessages(sessionId: string) {
    try {
      const msgs = await projectSessionApi.listMessages(sessionId);
      setMessages((prev) => ({ ...prev, [sessionId]: msgs }));
    } catch (e) {
      console.error("加载消息失败:", e);
    }
  }

  async function handleCreateProject(values: { projectName: string; projectPath: string }) {
    setLoading(true);
    try {
      const session = await projectSessionApi.open(values.projectPath, values.projectName || undefined);
      setSessions((prev) => {
        const exists = prev.find((s) => s.id === session.id);
        if (exists) return prev;
        return [...prev, session];
      });
      setActiveKey(session.id);
      setCreateOpen(false);
    } catch (e) {
      message.error(String(e));
    } finally {
      setLoading(false);
    }
  }

  async function handleCloseTab(sessionId: string) {
    try {
      await projectSessionApi.close(sessionId);
    } catch { /* ok */ }
    setSessions((prev) => prev.filter((s) => s.id !== sessionId));
    setMessages((prev) => {
      const copy = { ...prev };
      delete copy[sessionId];
      return copy;
    });
    if (activeKey === sessionId) {
      const remaining = sessions.filter((s) => s.id !== sessionId);
      setActiveKey(remaining.length > 0 ? remaining[0].id : "");
    }
  }

  async function handleTabChange(key: string) {
    setActiveKey(key);
    try {
      await projectSessionApi.setActive(key);
    } catch { /* ok */ }
  }

  async function handleSendMessage(role: string, content: string) {
    if (!activeKey) return;
    try {
      const msg = await projectSessionApi.appendMessage(activeKey, role, content);
      setMessages((prev) => ({
        ...prev,
        [activeKey]: [...(prev[activeKey] || []), msg],
      }));
    } catch (e) {
      message.error(String(e));
    }
  }

  const activeSession = sessions.find((s) => s.id === activeKey);

  const tabMoreMenu = {
    items: [
      { key: "close-others", label: "关闭其他",
        onClick: async () => {
          const others = sessions.filter((s) => s.id !== activeKey);
          for (const s of others) {
            try { await projectSessionApi.close(s.id); } catch { /* ok */ }
          }
          setSessions((prev) => prev.filter((s) => s.id === activeKey));
        },
      },
      { key: "close-all", label: "关闭全部",
        onClick: async () => {
          for (const s of sessions) {
            try { await projectSessionApi.close(s.id); } catch { /* ok */ }
          }
          setSessions([]);
          setMessages({});
          setActiveKey("");
        },
      },
    ],
  };

  return (
    <div className="flex flex-col h-full bg-white dark:bg-gray-900">
      {/* ─── Tab 栏 ─── */}
      <div className="flex items-center border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-850 px-1 select-none">
        <Tooltip title="新建项目会话">
          <Button
            type="text"
            icon={<Plus size={15} />}
            onClick={() => setCreateOpen(true)}
            className="text-gray-500 hover:text-blue-500 shrink-0 mx-0.5"
          />
        </Tooltip>
        <div
          className="flex-1 flex items-center overflow-x-auto gap-0.5"
          style={{ scrollbarWidth: "thin" }}
        >
          {sessions.map((s) => (
            <Tooltip key={s.id} title={s.projectPath} mouseEnterDelay={0.6}>
              <div
                onClick={() => handleTabChange(s.id)}
                className={`group flex items-center gap-1.5 px-3 py-1.5 text-[13px] cursor-pointer border-b-[2px] shrink-0 select-none transition-colors rounded-t ${
                  activeKey === s.id
                    ? "border-blue-500 text-blue-600 bg-white dark:bg-gray-800"
                    : "border-transparent text-gray-500 hover:text-gray-700 hover:bg-gray-100 dark:hover:bg-gray-800"
                }`}
              >
                <FolderOpen size={13} className="shrink-0" />
                <span className="max-w-[140px] truncate">{s.projectName}</span>
                <span
                  className="w-1.5 h-1.5 rounded-full shrink-0"
                  style={{
                    backgroundColor:
                      s.status === "active" ? "#52c41a" :
                      s.status === "loading" ? "#1677ff" :
                      s.status === "error" ? "#ff4d4f" : "#d9d9d9",
                  }}
                />
                <button
                  onClick={(e) => { e.stopPropagation(); handleCloseTab(s.id); }}
                  className="p-0.5 rounded hover:bg-gray-200 dark:hover:bg-gray-600 opacity-0 group-hover:opacity-100 transition-opacity ml-0.5"
                >
                  <X size={12} />
                </button>
              </div>
            </Tooltip>
          ))}
        </div>
        {sessions.length > 0 && (
          <Dropdown menu={tabMoreMenu} trigger={["click"]} placement="bottomRight">
            <Button type="text" size="small" icon={<MoreHorizontal size={14} />} />
          </Dropdown>
        )}
      </div>

      {/* ─── 项目信息栏 ─── */}
      {activeSession && (
        <div className="flex items-center gap-3 px-4 py-1.5 text-[12px] text-gray-400 bg-white dark:bg-gray-900 border-b border-gray-100 dark:border-gray-800">
          <span className="text-gray-600 dark:text-gray-300 font-medium text-[13px]">
            {activeSession.projectName}
          </span>
          <Tooltip title={activeSession.projectPath}>
            <span className="truncate max-w-[300px] cursor-default">
              {activeSession.projectPath}
            </span>
          </Tooltip>
          {activeSession.gitBranch && (
            <span className="flex items-center gap-1 text-gray-400">
              <GitBranch size={11} />
              {activeSession.gitBranch}
            </span>
          )}
          <Tag
            color={STATUS_COLOR[activeSession.status] || "default"}
            className="!m-0 !text-[10px] !leading-none !px-1.5 !py-0"
          >
            {STATUS_LABEL[activeSession.status] || activeSession.status}
          </Tag>
        </div>
      )}

      {/* ─── 主会话区 ─── */}
      <div className="flex-1 overflow-auto">
        {sessions.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <Empty description="暂无打开的项目">
              <Button type="primary" icon={<Plus size={14} />} onClick={() => setCreateOpen(true)}>
                新建项目会话
              </Button>
            </Empty>
          </div>
        ) : activeSession ? (
          <SessionChat
            session={activeSession}
            messages={messages[activeKey] || []}
            onSend={handleSendMessage}
            onRefresh={() => loadMessages(activeKey)}
          />
        ) : (
          <Empty description="选择一个项目开始对话" />
        )}
      </div>

      {/* ─── 新建项目 Modal ─── */}
      <CreateProjectModal
        open={createOpen}
        loading={loading}
        onOk={handleCreateProject}
        onCancel={() => setCreateOpen(false)}
      />
    </div>
  );
}

/** 新建项目会话弹窗 */
function CreateProjectModal({
  open,
  loading,
  onOk,
  onCancel,
}: {
  open: boolean;
  loading: boolean;
  onOk: (values: { projectName: string; projectPath: string }) => void;
  onCancel: () => void;
}) {
  const [form] = Form.useForm();
  const [picking, setPicking] = useState(false);

  async function pickFolder() {
    setPicking(true);
    try {
      const selected = await openDialog({ directory: true, multiple: false });
      if (selected) {
        const p = Array.isArray(selected) ? selected[0] : selected;
        form.setFieldsValue({ projectPath: p });
        // 自动用文件夹名填充项目名
        const name = p.split(/[/\\]/).filter(Boolean).pop() || "";
        if (!form.getFieldValue("projectName")) {
          form.setFieldsValue({ projectName: name });
        }
      }
    } catch { /* dialog cancelled */ }
    finally { setPicking(false); }
  }

  function handleOk() {
    form.validateFields().then((values) => {
      onOk(values);
      form.resetFields();
    });
  }

  return (
    <Modal
      title="新建项目会话"
      open={open}
      onOk={handleOk}
      onCancel={onCancel}
      confirmLoading={loading}
      okText="确定"
      cancelText="取消"
      destroyOnClose
    >
      <Form form={form} layout="vertical" className="mt-4">
        <Form.Item
          name="projectName"
          label="项目名称"
          rules={[{ required: true, message: "请输入项目名称" }]}
        >
          <Input placeholder="例如：W-NoteBook" maxLength={60} />
        </Form.Item>
        <Form.Item
          name="projectPath"
          label="项目路径"
          rules={[{ required: true, message: "请选择项目文件夹" }]}
        >
          <Input
            readOnly
            placeholder="点击右侧按钮选择文件夹"
            addonAfter={
              <Button
                type="text"
                size="small"
                icon={<FolderSearch size={14} />}
                onClick={pickFolder}
                loading={picking}
                className="!-m-1"
              />
            }
          />
        </Form.Item>
      </Form>
    </Modal>
  );
}

/** 会话消息聊天区 */
function SessionChat({
  session,
  messages,
  onSend,
  onRefresh,
}: {
  session: ProjectSession;
  messages: ProjectSessionMessage[];
  onSend: (role: string, content: string) => Promise<void>;
  onRefresh: () => void;
}) {
  const [input, setInput] = useState("");
  const [sending, setSending] = useState(false);

  const doSend = useCallback(async () => {
    const text = input.trim();
    if (!text || sending) return;
    setInput("");
    setSending(true);
    try {
      await onSend("user", text);
      onRefresh();
    } catch { /* handled by parent */ }
    finally { setSending(false); }
  }, [input, sending, onSend, onRefresh]);

  return (
    <div className="flex flex-col h-full">
      {/* 消息列表 */}
      <div className="flex-1 overflow-auto px-4 py-3 space-y-2.5">
        {messages.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-gray-400 gap-2">
            <FolderOpen size={32} className="opacity-30" />
            <span className="text-sm">开始与 {session.projectName} 对话</span>
          </div>
        ) : (
          messages.map((m) => (
            <div key={m.id} className={`flex ${m.role === "user" ? "justify-end" : "justify-start"}`}>
              <div
                className={`max-w-[75%] px-3 py-2 rounded-xl text-sm leading-relaxed ${
                  m.role === "user"
                    ? "bg-blue-500 text-white rounded-br-md"
                    : "bg-gray-100 dark:bg-gray-800 text-gray-800 dark:text-gray-200 rounded-bl-md"
                }`}
              >
                <div className="whitespace-pre-wrap break-words">{m.content}</div>
              </div>
            </div>
          ))
        )}
      </div>

      {/* 输入栏 */}
      <div className="border-t border-gray-100 dark:border-gray-800 px-4 py-3">
        <div className="flex gap-2 items-end">
          <Input.TextArea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                doSend();
              }
            }}
            placeholder={`向 ${session.projectName} 发送消息...`}
            autoSize={{ minRows: 1, maxRows: 5 }}
            className="flex-1"
          />
          <Button
            type="primary"
            icon={<Send size={14} />}
            onClick={doSend}
            loading={sending}
          />
        </div>
      </div>
    </div>
  );
}
