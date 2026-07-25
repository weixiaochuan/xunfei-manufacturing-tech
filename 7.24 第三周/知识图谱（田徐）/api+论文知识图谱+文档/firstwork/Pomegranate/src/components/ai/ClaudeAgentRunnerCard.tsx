import { useState } from "react";
import { Card, Tag, Button, Input, Select, Typography } from "antd";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ClaudeAgentSession, ClaudeAgentEventPayload, StartClaudeAgentInput } from "@/types";

const { Text } = Typography;
const STATUS_COLORS: Record<string, string> = {
  pending: "default",
  running: "blue",
  completed: "green",
  failed: "red",
  cancelled: "orange",
};

export default function ClaudeAgentRunnerCard() {
  const [checkResult, setCheckResult] = useState<string | null>(null);
  const [cliReady, setCliReady] = useState(false);

  const [projectPath, setProjectPath] = useState("");
  const [prompt, setPrompt] = useState("");
  const [permMode, setPermMode] = useState("readonly");
  const [starting, setStarting] = useState(false);

  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [output, setOutput] = useState<string[]>([]);
  const [sessionStatus, setSessionStatus] = useState<string | null>(null);

  const [history, setHistory] = useState<ClaudeAgentSession[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);

  async function loadHistory() {
    setHistoryLoading(true);
    try {
      const s = await invoke<ClaudeAgentSession[]>("list_claude_agent_sessions", {});
      setHistory(s);
    } catch { /* ignore */ }
    finally { setHistoryLoading(false); }
  }

  function fillFromHistory(s: ClaudeAgentSession) {
    setProjectPath(s.project_path);
    setPrompt(s.prompt);
    setPermMode(s.permission_mode);
    setActiveSessionId(null);
    setOutput([]);
    setSessionStatus(null);
  }

  async function handleCheck() {
    setCliReady(false);
    try {
      const r = await invoke<string>("claude_agent_check_cli");
      setCheckResult(r);
      setCliReady(true);
      loadHistory();
    } catch (e) {
      setCheckResult(String(e));
    }
  }

  async function handleStart() {
    if (!projectPath.trim() || !prompt.trim()) return;
    setStarting(true);
    setOutput([]);
    setSessionStatus("starting");

    const ul: (() => void)[] = [];
    async function listenOnce(
      evt: string,
      cb: (p: ClaudeAgentEventPayload) => void,
    ) { ul.push(await listen<ClaudeAgentEventPayload>(evt, (e) => cb(e.payload))); }

    await listenOnce("claude-agent:started", (p) => {
      if (!activeSessionId) setActiveSessionId(p.sessionId);
      setOutput((o) => [...o, `[started] ${p.content}`]);
      setSessionStatus("running");
    });
    await listenOnce("claude-agent:chunk", (p) => {
      setOutput((o) => [...o, p.content]);
    });
    await listenOnce("claude-agent:stderr", (p) => {
      setOutput((o) => [...o, `[stderr] ${p.content}`]);
    });
    await listenOnce("claude-agent:done", (p) => {
      setOutput((o) => [...o, `[done] ${p.content}`]);
      setSessionStatus(p.content);
      ul.forEach((fn) => fn());
      loadHistory();
    });

    try {
      const input: StartClaudeAgentInput = {
        project_path: projectPath,
        prompt,
        permission_mode: permMode,
      };
      const session = await invoke<ClaudeAgentSession>("start_claude_agent_session", { input });
      setActiveSessionId(session.id);
    } catch (e) {
      setOutput((o) => [...o, `[error] ${String(e)}`]);
      setSessionStatus("failed");
      ul.forEach((fn) => fn());
    } finally {
      setStarting(false);
    }
  }

  async function handleStop() {
    if (!activeSessionId) return;
    try {
      await invoke("stop_claude_agent_session", { sessionId: activeSessionId });
    } catch (e) {
      setOutput((o) => [...o, `[error] 停止失败: ${String(e)}`]);
    }
  }

  return (
    <Card title="Claude Code Agent Runner" size="small">
      {!cliReady ? (
        <div>
          <Text type="secondary" style={{ fontSize: 13 }}>
            Claude Code 可在指定项目目录中执行开发任务。需要 CLI 已安装且 MCP 已配置。
          </Text>
          <div className="mt-2">
            <Button size="small" loading={cliReady === undefined} onClick={handleCheck}>
              检测 Claude Code CLI
            </Button>
          </div>
          {checkResult && (
            <div className="mt-2">
              <Text type={cliReady ? "success" : "danger"} style={{ fontSize: 12 }}>{checkResult}</Text>
            </div>
          )}
        </div>
      ) : (
        <>
          {!activeSessionId && sessionStatus !== "running" && (
            <div className="space-y-3">
              <div>
                <div className="mb-1 text-xs text-gray-500">项目路径</div>
                <Input size="small" placeholder="D:\\AI\\W-NoteBook" value={projectPath} onChange={(e) => setProjectPath(e.target.value)} />
              </div>
              <div>
                <div className="mb-1 text-xs text-gray-500">任务描述</div>
                <Input.TextArea size="small" rows={2} placeholder="请修复模型设置页面显示问题" value={prompt} onChange={(e) => setPrompt(e.target.value)} />
              </div>
              <div className="flex items-center gap-2">
                <Select size="small" value={permMode} onChange={setPermMode} style={{ width: 120 }}
                  options={[
                    { value: "readonly", label: "只读" },
                    { value: "ask", label: "询问" },
                    { value: "workspace_write", label: "可写" },
                  ]}
                />
                <Button type="primary" size="small" loading={starting}
                  disabled={!projectPath.trim() || !prompt.trim()} onClick={handleStart}>
                  启动 Agent
                </Button>
              </div>
              <Text type="secondary" style={{ fontSize: 11 }}>CLI 版本: {checkResult}</Text>
            </div>
          )}

          {(activeSessionId || sessionStatus === "running") && (
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <Tag color={STATUS_COLORS[sessionStatus ?? "running"] ?? "blue"}>
                  {sessionStatus ?? "running"}
                </Tag>
                <Button size="small" danger onClick={handleStop}>停止</Button>
              </div>
              <Card size="small" className="bg-gray-900 text-green-400 font-mono text-xs max-h-60 overflow-y-auto">
                <pre className="whitespace-pre-wrap m-0">{output.join("\n") || "等待输出..."}</pre>
              </Card>
            </div>
          )}

          <div className="mt-4 pt-4 border-t border-gray-100">
            <div className="flex items-center justify-between mb-2">
              <Text strong style={{ fontSize: 13 }}>历史会话</Text>
              <Button size="small" loading={historyLoading} onClick={loadHistory}>刷新</Button>
            </div>
            {history.length === 0 ? (
              <Text type="secondary" style={{ fontSize: 12 }}>暂无历史</Text>
            ) : (
              <div className="space-y-1.5 max-h-48 overflow-y-auto">
                {history.slice(0, 10).map((s) => (
                  <div key={s.id}
                    className="flex items-center gap-2 text-xs p-1.5 rounded hover:bg-gray-50 cursor-pointer"
                    onClick={() => fillFromHistory(s)}
                  >
                    <Tag color={STATUS_COLORS[s.status] ?? "default"} style={{ fontSize: 11, margin: 0 }}>{s.status}</Tag>
                    <span className="truncate flex-1 min-w-0" title={s.prompt}>{s.prompt.slice(0, 60)}</span>
                    <Text type="secondary" style={{ fontSize: 11 }}>{s.created_at?.slice(0, 16)}</Text>
                  </div>
                ))}
              </div>
            )}
          </div>
        </>
      )}
    </Card>
  );
}
