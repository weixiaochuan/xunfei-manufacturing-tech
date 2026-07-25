import { useCallback, useEffect, useMemo, useState } from "react";
import Markdown from "react-markdown";
import {
  Alert,
  Button,
  Card,
  Empty,
  Input,
  Modal,
  Progress,
  Space,
  Switch,
  Tabs,
  Tag,
  Tooltip,
  message,
} from "antd";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { FileText, RefreshCw, Save, Trash2 } from "lucide-react";
import { planningApi } from "@/lib/api";
import type { PlanningSessionKind, PlanningWorkspace } from "@/types";

type Props = {
  sessionKind: PlanningSessionKind;
  sessionId?: string | null;
  disabled?: boolean;
  compact?: boolean;
};

const fileLabels: Record<string, string> = {
  "plan.md": "计划",
  "findings.md": "发现",
  "progress.md": "进度",
};

export function stripPlanningUpdateBlock(text: string): string {
  let cleaned = text
    .replace(/<\s*\|+\s*DSML\s*\|+\s*tool_calls\b[\s\S]*?<\s*\/?\s*\|+\s*DSML\s*\|+\s*\/?\s*tool_calls\s*>/gi, "")
    .replace(/<\s*\|+\s*DSML\s*\|+\s*(?:tool_calls|tool_call|invoke|parameter)\b[\s\S]*?<\s*\/?\s*\|+\s*DSML\s*\|+\s*\/?\s*(?:tool_calls|tool_call|invoke|parameter)\s*>/gi, "")
    .replace(/<\s*\/?\s*\|+\s*DSML\s*\|+\s*\/?\s*(?:tool_calls|tool_call|invoke|parameter)\b[^>]*>/gi, "")
    .replace(/<\|DSML\|>/g, "")
    .replace(/<tool_calls>[\s\S]*?<\/tool_calls>/g, "")
    .replace(/<tool_call>[\s\S]*?<\/tool_call>/g, "")
    .replace(/planning_update\s*\([\s\S]*?\)/g, "")
    .trim();
  cleaned = cleaned
    .split(/\r?\n/)
    .filter((line) => {
      const lowered = line.toLowerCase();
      return !(
        lowered.includes("dsml") ||
        lowered.includes("tool_calls") ||
        lowered.includes("<tool_call") ||
        lowered.includes("</tool_call") ||
        lowered.includes("<invoke") ||
        lowered.includes("</invoke") ||
        lowered.includes("<parameter") ||
        lowered.includes("</parameter")
      );
    })
    .join("\n")
    .trim();
  const marker = '"planningUpdate"';
  const idx = cleaned.lastIndexOf(marker);
  if (idx < 0) return cleaned;
  let start = -1;
  for (let i = idx; i >= 0; i -= 1) {
    if (cleaned[i] === "{") {
      start = i;
      break;
    }
  }
  if (start < 0) return cleaned;
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let i = start; i < cleaned.length; i += 1) {
    const ch = cleaned[i];
    if (escaped) {
      escaped = false;
      continue;
    }
    if (ch === "\\" && inString) {
      escaped = true;
      continue;
    }
    if (ch === "\"") {
      inString = !inString;
      continue;
    }
    if (inString) continue;
    if (ch === "{") depth += 1;
    if (ch === "}") {
      depth -= 1;
      if (depth === 0) {
        return `${cleaned.slice(0, start)}${cleaned.slice(i + 1)}`.replace(/```json\s*```/g, "").trim();
      }
    }
  }
  return cleaned;
}

type PlanningUpdatedEvent = {
  workspaceId: string;
  conversationId: string;
  sessionKind: PlanningSessionKind;
  sessionId: string;
  revision: number;
  updatedAt?: string | null;
  changedSections: string[];
  currentStage?: string | null;
  progressPercent?: number;
};

export function PlanningWithFilesPanel({ sessionKind, sessionId, disabled, compact }: Props) {
  const [workspace, setWorkspace] = useState<PlanningWorkspace | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [activeFile, setActiveFile] = useState("plan.md");

  const canLoad = Boolean(sessionId);
  const sessionTitle = sessionKind === "ai" ? "普通 AI 会话" : "外部智能体会话";

  const load = useCallback(async () => {
    if (!sessionId) {
      setWorkspace(null);
      setDrafts({});
      return;
    }
    setLoading(true);
    try {
      const next = await planningApi.getWorkspace(sessionKind, sessionId);
      setWorkspace(next);
      setDrafts(Object.fromEntries(next.files.map((f) => [f.name, f.content])));
    } catch (e) {
      message.error(`读取规划失败：${e}`);
    } finally {
      setLoading(false);
    }
  }, [sessionKind, sessionId]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (!sessionId) return;
    let cancelled = false;
    const unlistens: UnlistenFn[] = [];
    const handleUpdated = (event: { payload: PlanningUpdatedEvent }) => {
      const payload = event.payload;
      if (payload.sessionKind !== sessionKind || payload.sessionId !== sessionId) return;
      void load();
    };
    listen<PlanningUpdatedEvent>("planning://updated", handleUpdated).then((fn) => {
      if (cancelled) fn();
      else unlistens.push(fn);
    });
    listen<PlanningUpdatedEvent>("planning:updated", handleUpdated).then((fn) => {
      if (cancelled) fn();
      else unlistens.push(fn);
    });
    return () => {
      cancelled = true;
      unlistens.forEach((fn) => fn());
    };
  }, [load, sessionKind, sessionId]);

  const pendingPretty = useMemo(() => {
    if (!workspace?.pendingUpdate) return "";
    try {
      return JSON.stringify(JSON.parse(workspace.pendingUpdate), null, 2);
    } catch {
      return workspace.pendingUpdate;
    }
  }, [workspace?.pendingUpdate]);

  async function toggleEnabled(enabled: boolean) {
    if (!sessionId) return;
    setLoading(true);
    try {
      const next = await planningApi.setEnabled(sessionKind, sessionId, enabled);
      setWorkspace(next);
      setDrafts(Object.fromEntries(next.files.map((f) => [f.name, f.content])));
      message.success(enabled ? "Planning with Files 已开启" : "Planning with Files 已关闭");
    } catch (e) {
      message.error(`切换失败：${e}`);
    } finally {
      setLoading(false);
    }
  }

  async function saveFile(fileName = activeFile) {
    if (!sessionId) return;
    setSaving(true);
    try {
      const next = await planningApi.saveFile(
        sessionKind,
        sessionId,
        fileName,
        drafts[fileName] ?? "",
      );
      setWorkspace(next);
      setDrafts(Object.fromEntries(next.files.map((f) => [f.name, f.content])));
      message.success("规划文件已保存");
    } catch (e) {
      message.error(`保存失败：${e}`);
    } finally {
      setSaving(false);
    }
  }

  async function applyUpdate(accept: boolean) {
    if (!sessionId) return;
    try {
      const next = await planningApi.applyUpdate(sessionKind, sessionId, accept);
      setWorkspace(next);
      setDrafts(Object.fromEntries(next.files.map((f) => [f.name, f.content])));
      message.success(accept ? "已应用规划更新" : "已拒绝本次规划更新");
    } catch (e) {
      message.error(`处理更新失败：${e}`);
    }
  }

  function clearPlanning() {
    if (!sessionId) return;
    Modal.confirm({
      title: "清空当前会话规划？",
      content: "这会重置 plan.md、findings.md 和 progress.md，但不会删除会话消息。",
      okText: "清空",
      okButtonProps: { danger: true },
      cancelText: "取消",
      async onOk() {
        const next = await planningApi.clear(sessionKind, sessionId, true);
        setWorkspace(next);
        setDrafts(Object.fromEntries(next.files.map((f) => [f.name, f.content])));
        message.success("规划已清空");
      },
    });
  }

  async function exportFiles() {
    if (!sessionId) return;
    const picked = await openDialog({ directory: true, multiple: false });
    if (!picked || Array.isArray(picked)) return;
    try {
      await planningApi.export(sessionKind, sessionId, picked);
      message.success("规划文件已导出");
    } catch (e) {
      message.error(`导出失败：${e}`);
    }
  }

  if (!canLoad) {
    return (
      <Card size="small" className={compact ? "" : "h-full"} title="Planning with Files">
        <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="选择或创建会话后可开启规划" />
      </Card>
    );
  }

  return (
    <Card
      size="small"
      className={compact ? "" : "h-full"}
      title={
        <Space size={6}>
          <FileText size={15} />
          <span>Planning with Files</span>
        </Space>
      }
      extra={
        <Tooltip title="刷新规划状态">
          <Button size="small" type="text" icon={<RefreshCw size={14} />} onClick={load} />
        </Tooltip>
      }
      loading={loading}
    >
      <Space direction="vertical" className="w-full" size={10}>
        <div className="flex items-center justify-between gap-2">
          <div className="text-xs text-gray-500">{sessionTitle}</div>
          <Switch
            size="small"
            checked={workspace?.enabled ?? false}
            disabled={disabled}
            onChange={toggleEnabled}
          />
        </div>

        {!workspace?.pluginReady && (
          <Alert
            type="warning"
            showIcon
            message="插件尚未就绪"
            description={workspace?.blockedReason ?? "请先在 AI 应用市场获取、安装、授权并启用 Planning with Files。"}
          />
        )}

        {workspace?.enabled ? (
          <>
            <div className="rounded bg-slate-50 p-2 text-xs text-gray-600">
              <div className="truncate" title={workspace.workspacePath}>工作区：{workspace.workspacePath}</div>
              <div className="mt-1 flex items-center gap-2">
                <Tag color="blue">{workspace.currentStage ?? "阶段待定"}</Tag>
                <Progress percent={workspace.progressPercent} size="small" className="flex-1" />
              </div>
              {workspace.blockers.length > 0 && (
                <div className="mt-1 text-red-500">阻塞：{workspace.blockers.join("；")}</div>
              )}
            </div>

            {workspace.pendingUpdate && (
              <Alert
                type="info"
                showIcon
                message="AI 提出了规划文件更新"
                description={
                  <Space direction="vertical" className="w-full">
                    <Input.TextArea value={pendingPretty} readOnly rows={5} />
                    <Space>
                      <Button type="primary" size="small" onClick={() => applyUpdate(true)}>
                        确认保存
                      </Button>
                      <Button size="small" onClick={() => applyUpdate(false)}>
                        拒绝本次更新
                      </Button>
                    </Space>
                  </Space>
                }
              />
            )}

            <Tabs
              size="small"
              activeKey={activeFile}
              onChange={setActiveFile}
              items={(workspace.files.length ? workspace.files : ["plan.md", "findings.md", "progress.md"].map((name) => ({ name, content: "", updatedAt: null }))).map((file) => ({
                key: file.name,
                label: fileLabels[file.name] ?? file.name,
                children: (
                  <Space direction="vertical" className="w-full">
                    <Input.TextArea
                      value={drafts[file.name] ?? ""}
                      onChange={(e) => setDrafts((prev) => ({ ...prev, [file.name]: e.target.value }))}
                      rows={compact ? 8 : 12}
                    />
                    <div className="flex justify-between items-center">
                      <span className="text-xs text-gray-400">更新时间：{file.updatedAt ?? workspace.lastUpdatedAt ?? "未知"}</span>
                      <Button size="small" icon={<Save size={13} />} loading={saving} onClick={() => saveFile(file.name)}>
                        保存
                      </Button>
                    </div>
                    {!compact && (
                      <div className="rounded border border-dashed border-gray-200 p-2 max-h-48 overflow-auto ai-markdown">
                        <Markdown>{drafts[file.name] ?? ""}</Markdown>
                      </div>
                    )}
                  </Space>
                ),
              }))}
            />

            <Space wrap>
              <Button size="small" onClick={exportFiles}>导出规划文件</Button>
              <Button size="small" danger icon={<Trash2 size={13} />} onClick={clearPlanning}>
                清空规划
              </Button>
            </Space>
          </>
        ) : (
          <Alert
            type="info"
            showIcon
            message="当前会话未开启"
            description="开启后会创建 plan.md、findings.md 和 progress.md，并在 AI 调用前注入精简规划上下文。关闭不会删除已有文件。"
          />
        )}
      </Space>
    </Card>
  );
}
