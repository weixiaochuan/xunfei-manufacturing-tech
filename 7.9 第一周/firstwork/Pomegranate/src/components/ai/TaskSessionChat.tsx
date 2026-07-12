import { useState, useEffect, useRef, useCallback } from "react";
import { Input, Button, Typography, Spin, message } from "antd";
import { SendOutlined, LoadingOutlined } from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "@/store";
import { sessionApi } from "@/lib/api";

const { Text } = Typography;

interface ChatMessage {
  role: "user" | "assistant";
  content: string;
  hasConfirmPrompt?: boolean;
}

export default function TaskSessionChat() {
  const session = useAppStore((s) => s.activeSession);
  const phases = useAppStore((s) => s.activeSessionPhases);
  const setActiveSession = useAppStore((s) => s.setActiveSession);
  const isExecuting = useAppStore((s) => s.isSessionExecuting);

  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [streamingText, setStreamingText] = useState("");
  const [isStreaming, setIsStreaming] = useState(false);
  const [convId, setConvId] = useState<number | null>(null);
  const [convCreating, setConvCreating] = useState(false);
  const [systemPromptSent, setSystemPromptSent] = useState(false);

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const unlistenRef = useRef<(() => void)[]>([]);
  const streamingTextRef = useRef("");

  // 保持 streamingText 引用同步
  useEffect(() => {
    streamingTextRef.current = streamingText;
  }, [streamingText]);

  // 构建 System Prompt
  const buildSystemPrompt = useCallback(() => {
    if (!session || !phases.length) return "";
    const currentPhase = phases.find(
      (p) => p.index_num === session.current_phase_index
    );
    const planLines = phases.map(
      (p) => `Phase ${p.index_num}: ${p.name} [${p.status}]`
    );
    const phaseName = currentPhase?.name ?? "未知";
    const phaseDesc = currentPhase?.description ?? "";
    return `你正在执行一个多阶段开发任务。当前任务计划如下：

${planLines.join("\n")}

执行规则：
1. 从当前 Phase 开始执行
2. 完成当前 Phase 的所有操作后，输出执行摘要
3. 每完成一个 Phase，必须以【Phase X/Y 完成】开头输出摘要
4. 摘要格式：
   【Phase X/Y 完成】
   - 操作摘要：...
   - 修改文件：file1, file2, ...
   - 状态：等待确认
5. 必须等待用户确认后才能继续下一 Phase
6. 如果遇到错误，立即停止并报告

当前正在执行 Phase ${session.current_phase_index}: ${phaseName}
${phaseDesc ? `描述：${phaseDesc}` : ""}`;
  }, [session, phases]);

  // 创建 AI 对话
  const ensureConv = useCallback(async () => {
    if (convId || convCreating) return convId;
    setConvCreating(true);
    try {
      const conversation = await invoke<{ id: number }>("create_ai_conversation", {
        title: `[会话执行] ${session?.plan_name ?? ""}`,
        modelId: null,
      });
      setConvId(conversation.id);
      return conversation.id;
    } catch (e) {
      message.error(`创建 AI 对话失败: ${e}`);
      return null;
    } finally {
      setConvCreating(false);
    }
  }, [convId, convCreating, session?.plan_name]);

  // 自动滚动
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingText]);

  // 订阅 AI 流式事件
  useEffect(() => {
    let cancelled = false;

    async function setupListeners() {
      const u1 = await listen<{ token: string }>("ai:token", () => {
        if (cancelled) return;
        // token 已经通过 streamingText 累积
      });

      const u2 = await listen<{ content?: string }>("ai:done", (event) => {
        if (cancelled) return;
        const content = event.payload.content ?? streamingTextRef.current;
        setStreamingText("");
        streamingTextRef.current = "";
        setIsStreaming(false);

        const hasConfirm = /Phase \d+\/\d+ 完成/.test(content) || /等待确认/.test(content);
        setMessages((prev) => [
          ...prev,
          { role: "assistant", content, hasConfirmPrompt: hasConfirm },
        ]);

        if (session?.id) {
          sessionApi.get(session.id).then((detail) => {
            setActiveSession(detail.session, detail.phases);
          }).catch(() => {});
        }
      });

      const u3 = await listen<string>("ai:error", () => {
        if (cancelled) return;
        setStreamingText("");
        streamingTextRef.current = "";
        setIsStreaming(false);
        setMessages((prev) => [
          ...prev,
          { role: "assistant", content: "❌ AI 请求出错，请重试" },
        ]);
      });

      unlistenRef.current = [u1, u2, u3];
    }

    setupListeners();
    return () => {
      cancelled = true;
      unlistenRef.current.forEach((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session?.id]);

  async function handleSend() {
    if (!input.trim() || !session?.id || isStreaming) return;
    const userMessage = input.trim();
    setInput("");
    setMessages((prev) => [...prev, { role: "user", content: userMessage }]);
    setIsStreaming(true);
    setStreamingText("");

    try {
      const id = await ensureConv();
      if (!id) throw new Error("无法创建 AI 对话");

      // 首次消息：将 system prompt 作为前缀注入
      let finalMessage = userMessage;
      if (!systemPromptSent) {
        const sysPrompt = buildSystemPrompt();
        if (sysPrompt) {
          finalMessage = `${sysPrompt}\n\n---\n用户消息：${userMessage}`;
          setSystemPromptSent(true);
        }
      }

      // 通过现有 AI 消息流发送
      await invoke("send_ai_message", {
        conversationId: id,
        message: finalMessage,
        useRag: false,
        useSkills: null,
        attachments: null,
      });
    } catch (e) {
      message.error(String(e));
      setIsStreaming(false);
    }
  }

  function handleConfirmInline() {
    if (!session?.id) return;
    sessionApi.confirmPhase(session.id).then(() => {
      return sessionApi.get(session.id!);
    }).then((detail) => {
      setActiveSession(detail.session, detail.phases);
      message.success("已确认，进入下一阶段");
    }).catch((e) => {
      message.error(String(e));
    });
  }

  if (!session) {
    return (
      <div className="flex items-center justify-center h-full text-gray-400">
        请先创建或选择一个会话
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      {/* 消息列表 */}
      <div className="flex-1 overflow-auto p-4 space-y-3">
        <div className="text-center">
          <Text type="secondary" className="text-xs bg-gray-100 dark:bg-gray-800 rounded px-2 py-1">
            会话已创建 - 计划: {session.plan_name} ({session.total_phases} 个阶段)
          </Text>
        </div>

        {messages.map((msg, i) => (
          <div key={i} className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}>
            <div
              className={`max-w-[85%] rounded-lg px-3 py-2 text-sm ${
                msg.role === "user"
                  ? "bg-blue-500 text-white"
                  : "bg-gray-100 dark:bg-gray-800"
              }`}
            >
              {/^【Phase \d+\/\d+ 完成】/m.test(msg.content) ? (
                <div>
                  <div className="whitespace-pre-wrap">{msg.content}</div>
                  {msg.hasConfirmPrompt && (
                    <div className="mt-2 pt-2 border-t border-gray-300">
                      <Button
                        type="primary"
                        size="small"
                        onClick={handleConfirmInline}
                        icon={<SendOutlined />}
                      >
                        确认继续
                      </Button>
                    </div>
                  )}
                </div>
              ) : (
                <div className="whitespace-pre-wrap">{msg.content}</div>
              )}
            </div>
          </div>
        ))}

        {/* 流式输出 */}
        {isStreaming && streamingText && (
          <div className="flex justify-start">
            <div className="max-w-[85%] rounded-lg px-3 py-2 bg-gray-100 dark:bg-gray-800">
              <div className="whitespace-pre-wrap text-sm">{streamingText}</div>
              <LoadingOutlined className="text-xs text-gray-400 mt-1" spin />
            </div>
          </div>
        )}

        {isStreaming && !streamingText && (
          <div className="flex justify-center">
            <Spin size="small" />
          </div>
        )}

        <div ref={messagesEndRef} />
      </div>

      {/* 输入区 */}
      <div className="border-t border-gray-200 dark:border-gray-700 p-3">
        <div className="flex gap-2">
          <Input.TextArea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onPressEnter={(e) => {
              if (!e.shiftKey) {
                e.preventDefault();
                handleSend();
              }
            }}
            placeholder={
              isExecuting
                ? "AI 正在执行，请等待..."
                : "输入消息，Enter 发送..."
            }
            disabled={isStreaming || isExecuting || convCreating}
            autoSize={{ minRows: 1, maxRows: 4 }}
            className="text-sm"
          />
          <Button
            type="primary"
            icon={<SendOutlined />}
            onClick={handleSend}
            disabled={isStreaming || !input.trim() || isExecuting || convCreating}
            loading={convCreating}
          />
        </div>
        <Text type="secondary" className="text-[10px]">
          Enter 发送，Shift+Enter 换行
        </Text>
      </div>
    </div>
  );
}
