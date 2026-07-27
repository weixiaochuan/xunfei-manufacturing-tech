/**
 * PluginToolbarButtons renders both legacy runtime toolbar buttons and
 * marketplace declarative toolbar contributions. Declarative buttons never run
 * third-party JavaScript; they call firstwork-controlled Tauri commands.
 */

import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Alert, Button, Modal, Space, Tooltip, Typography, message } from "antd";
import type { Editor } from "@tiptap/react";
import { externalAgentApi, pluginApi } from "@/lib/api";
import { pluginManager } from "@/services/pluginManager";
import { getActiveEditor } from "@/services/editorBridge";
import { subscribeDeclarativePluginToolbarChanged } from "@/services/declarativePluginEvents";
import {
  pluginPipelineTextInput,
  runPluginPipelineAfterModel,
  runPluginPipelineBeforeModel,
  type PluginPipelineBeforeResult,
} from "@/services/pluginPipeline";
import { resolvePluginIconComponent } from "@/components/plugin/pluginIcons";
import type {
  AgentStreamEvent,
  EditorActionCtx,
  PluginDocumentToolbarButton,
  PluginEditorToolbarButtonDef,
} from "@/types";

type TBEntry = PluginEditorToolbarButtonDef & { pluginId: string };

interface Props {
  editor: Editor | null;
  noteId?: number | null;
  documentTitle?: string;
}

interface SummaryPreviewState {
  open: boolean;
  pluginId: string;
  title: string;
  summary: string;
  providerLabel: string;
  mock: boolean;
  streaming: boolean;
  error?: string;
  requestId?: string;
  sessionId?: string;
  externalAgentId?: string;
}

interface ActiveSummaryPipeline {
  pluginId: string;
  externalAgentId: string;
  sessionId: string;
  requestId: string;
  before: PluginPipelineBeforeResult;
  rawOutput: string;
}

function buildEditorCtx(editor: Editor | null, noteId?: number | null): EditorActionCtx | null {
  const ed = editor ?? getActiveEditor();
  if (!ed) return null;

  const sel = ed.state.selection;
  const selection = sel.empty ? "" : ed.state.doc.textBetween(sel.from, sel.to, "\n");

  return {
    noteId: noteId ?? null,
    selection,
    replaceSelection: (text: string) => {
      const current = ed.state.selection;
      if (current.empty) {
        ed.chain().focus().insertContent(text).run();
      } else {
        ed.chain().focus().deleteSelection().insertContent(text).run();
      }
    },
    insertText: (text: string) => {
      ed.chain().focus().insertContent(text).run();
    },
    getContent: () => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const md = (ed.storage as any).markdown?.getMarkdown?.();
      return typeof md === "string" ? md : ed.getText();
    },
  };
}

export function PluginToolbarButtons({ editor, noteId, documentTitle }: Props) {
  const [legacyItems, setLegacyItems] = useState<TBEntry[]>([]);
  const [declarativeItems, setDeclarativeItems] = useState<PluginDocumentToolbarButton[]>([]);
  const [summaryPreview, setSummaryPreview] = useState<SummaryPreviewState | null>(null);
  const activeRequestRef = useRef<string | null>(null);
  const activePipelineRef = useRef<ActiveSummaryPipeline | null>(null);
  const insertCtxRef = useRef<EditorActionCtx | null>(null);
  const finalizedRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    const refresh = () => setLegacyItems(pluginManager.getRegisteredEditorToolbarButtons());
    refresh();
    return pluginManager.subscribe("editor-toolbar", refresh);
  }, []);

  useEffect(() => {
    let alive = true;
    const refresh = async () => {
      try {
        const buttons = await pluginApi.listDocumentSummaryToolbarButtons();
        if (alive) setDeclarativeItems(buttons);
      } catch (e) {
        if (alive) {
          setDeclarativeItems([]);
          console.warn("[PluginToolbar] failed to load declarative toolbar buttons:", e);
        }
      }
    };
    const onChanged = () => {
      void refresh();
    };

    void refresh();
    const unsubscribeToolbarChanged = subscribeDeclarativePluginToolbarChanged(onChanged);
    window.addEventListener("focus", onChanged);
    return () => {
      alive = false;
      unsubscribeToolbarChanged();
      window.removeEventListener("focus", onChanged);
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;

    listen<AgentStreamEvent>("agent:stream", async (event) => {
      const payload = event.payload;
      const activeRequestId = activeRequestRef.current;
      if (!activeRequestId || payload.requestId !== activeRequestId) return;

      if (payload.event === "started") {
        setSummaryPreview((prev) => prev ? { ...prev, streaming: true, error: undefined } : prev);
      }

      if (payload.event === "text_delta" && payload.delta) {
        if (activePipelineRef.current?.requestId === payload.requestId) {
          activePipelineRef.current.rawOutput += payload.delta;
        }
        setSummaryPreview((prev) =>
          prev ? { ...prev, summary: prev.summary + payload.delta, streaming: true } : prev,
        );
      }

      if (payload.event === "completed" || payload.event === "cancelled" || payload.event === "error") {
        activeRequestRef.current = null;
        const active = activePipelineRef.current;
        activePipelineRef.current = null;
        const status =
          payload.event === "completed"
            ? "completed"
            : payload.event === "cancelled"
              ? "cancelled"
              : "failed";
        let finalOutput = active?.rawOutput ?? "";
        if (payload.event === "completed" && active && finalOutput.trim()) {
          const processed = await runPluginPipelineAfterModel(active.before, finalOutput);
          finalOutput = processed.output;
          if (processed.output !== active.rawOutput) {
            try {
              await externalAgentApi.finalizePluginOutput(
                active.sessionId,
                active.requestId,
                active.rawOutput,
                processed.output,
              );
            } catch (error) {
              message.warning(`摘要增强结果未能写入会话记录，预览仍保留：${String(error)}`);
            }
          }
          if (processed.warnings.length > 0) {
            message.warning(`有 ${processed.warnings.length} 个摘要增强步骤未完全执行`);
          }
        }
        if (active) {
          const key = `${active.pluginId}:${active.requestId}:${status}`;
          if (!finalizedRef.current.has(key)) {
            finalizedRef.current.add(key);
            void pluginApi.finalizeDocumentSummaryAgent({
              pluginId: active.pluginId,
              externalAgentId: active.externalAgentId,
              sessionId: active.sessionId,
              requestId: active.requestId,
              status,
              errorCode: payload.errorCode ?? payload.message ?? null,
            });
          }
        }
        setSummaryPreview((prev) => {
          if (!prev || (prev.requestId && prev.requestId !== payload.requestId)) return prev;
          return {
            ...prev,
            summary: payload.event === "completed" ? finalOutput || prev.summary : prev.summary,
            streaming: false,
            error:
              payload.event === "error"
                ? payload.message || payload.errorCode || "智能体调用失败"
                : payload.event === "cancelled"
                  ? "用户主动取消"
                  : undefined,
          };
        });
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  async function handleDeclarativeSummary(item: PluginDocumentToolbarButton) {
    const ctx = buildEditorCtx(editor, noteId);
    if (!ctx) {
      message.warning("编辑器尚未就绪");
      return;
    }

    try {
      const content = ctx.getContent();
      if (!content.trim()) {
        message.warning("当前文档正文为空，无法生成摘要");
        return;
      }

      const title = documentTitle?.trim() || "未命名文档";
      insertCtxRef.current = ctx;
      const before = await runPluginPipelineBeforeModel({
        scene: "global",
        feature: "document-summary",
        input: content,
        workspaceId: noteId ? `note:${noteId}` : "note:unsaved",
        selectedResources: noteId ? [`note:${noteId}`] : [],
        metadata: {
          pluginId: item.pluginId,
          documentTitle: title,
          documentChars: content.length,
        },
      });
      const effectiveContent = pluginPipelineTextInput(before, content);
      if (before.warnings.length > 0) {
        message.warning(`有 ${before.warnings.length} 个文档增强步骤未完全执行，已继续摘要`);
      }
      const config = await pluginApi.getDocumentSummaryConfig(item.pluginId);

      if (config.mode === "agent") {
        if (!config.externalAgentId) {
          message.warning("未配置摘要智能体，请先在插件设置中选择 AI 资源中心的智能体");
          return;
        }

        const selectedAgent = config.availableAgents.find((agent) => agent.id === config.externalAgentId);
        setSummaryPreview({
          open: true,
          pluginId: item.pluginId,
          title,
          summary: "",
          providerLabel: selectedAgent?.mockMode
            ? "Mock 智能体演示（不是真实 AI 调用）"
            : `统一智能体服务：${selectedAgent?.name ?? config.externalAgentId}`,
          mock: selectedAgent?.mockMode ?? false,
          streaming: true,
          externalAgentId: config.externalAgentId,
        });

        const started = await pluginApi.startDocumentSummaryAgent({
          pluginId: item.pluginId,
          title,
          content,
          externalAgentId: config.externalAgentId,
          effectiveContent,
          pluginSystemContext: before.prompt || null,
          pluginContributionIds: before.executedContributionIds,
        });
        activeRequestRef.current = started.requestId;
        activePipelineRef.current = {
          pluginId: item.pluginId,
          externalAgentId: started.externalAgentId,
          sessionId: started.sessionId,
          requestId: started.requestId,
          before,
          rawOutput: "",
        };
        setSummaryPreview((prev) =>
          prev
            ? {
                ...prev,
                requestId: started.requestId,
                sessionId: started.sessionId,
                externalAgentId: started.externalAgentId,
                mock: started.mock,
                providerLabel: started.mock ? "Mock 智能体演示（不是真实 AI 调用）" : prev.providerLabel,
              }
            : prev,
        );
        return;
      }

      const result = await pluginApi.mockDocumentSummary({
        pluginId: item.pluginId,
        title,
        content: effectiveContent,
      });
      const processed = await runPluginPipelineAfterModel(before, result.summary);
      setSummaryPreview({
        open: true,
        pluginId: item.pluginId,
        title: result.title,
        summary: processed.output,
        providerLabel: result.providerLabel,
        mock: true,
        streaming: false,
      });
    } catch (e) {
      setSummaryPreview((prev) => prev ? { ...prev, streaming: false, error: String(e) } : prev);
      message.error(`AI 摘要失败：${e}`);
    }
  }

  async function cancelSummary() {
    if (!summaryPreview?.requestId) return;
    try {
      await pluginApi.cancelDocumentSummary({
        pluginId: summaryPreview.pluginId,
        requestId: summaryPreview.requestId,
      });
    } catch (e) {
      message.error(`停止生成失败：${e}`);
    }
  }

  async function insertSummary() {
    if (!summaryPreview || summaryPreview.streaming || summaryPreview.error || !summaryPreview.summary.trim()) {
      return;
    }
    const ctx = insertCtxRef.current;
    if (!ctx) {
      message.warning("当前编辑器已不可用，无法插入摘要");
      return;
    }
    try {
      await pluginApi.recordDocumentSummaryInsert({
        pluginId: summaryPreview.pluginId,
        title: summaryPreview.title,
      });
      ctx.insertText(`\n\n## AI 摘要\n\n${summaryPreview.summary}\n`);
      setSummaryPreview(null);
      message.success("AI 摘要已插入当前文档");
    } catch (e) {
      message.error(`插入摘要失败：${e}`);
    }
  }

  if (legacyItems.length === 0 && declarativeItems.length === 0 && !summaryPreview) return null;

  return (
    <>
      {(legacyItems.length > 0 || declarativeItems.length > 0) && (
        <span
          aria-hidden
          style={{
            display: "inline-block",
            width: 1,
            height: 18,
            margin: "0 3px",
            background: "var(--ant-color-border-secondary, #f0f0f0)",
            verticalAlign: "middle",
          }}
        />
      )}

      {legacyItems.map((item) => {
        const Icon = resolvePluginIconComponent(item.icon);
        const pluginName = pluginManager.getPluginName(item.pluginId) ?? item.pluginId;
        return (
          <Tooltip
            key={`${item.pluginId}:${item.id}`}
            title={
              <span>
                <span style={{ opacity: 0.7, fontSize: 11 }}>{pluginName}</span>
                <br />
                {item.tooltip}
              </span>
            }
          >
            <Button
              type="text"
              size="small"
              icon={<Icon size={14} />}
              onClick={async () => {
                const ctx = buildEditorCtx(editor, noteId);
                if (!ctx) {
                  message.warning("编辑器尚未就绪");
                  return;
                }
                try {
                  await item.callback(ctx);
                } catch (e) {
                  console.error(`[PluginToolbar] ${item.pluginId}:${item.id} threw:`, e);
                  pluginManager._logError(item.pluginId, "editor:toolbar", String(e));
                  message.error(`插件「${item.pluginId}」执行失败：${e}`);
                }
              }}
              style={{ minWidth: 26, height: 26, padding: 0 }}
            />
          </Tooltip>
        );
      })}

      {declarativeItems.map((item) => {
        const Icon = resolvePluginIconComponent(item.icon);
        return (
          <Tooltip
            key={`${item.pluginId}:${item.id}`}
            title={
              <span>
                <span style={{ opacity: 0.7, fontSize: 11 }}>{item.pluginName}</span>
                <br />
                {item.tooltip}
              </span>
            }
          >
            <Button
              type="text"
              size="small"
              icon={<Icon size={14} />}
              onClick={() => void handleDeclarativeSummary(item)}
              style={{ minWidth: 26, height: 26, padding: "0 6px" }}
              disabled={summaryPreview?.streaming}
            >
              {item.label}
            </Button>
          </Tooltip>
        );
      })}

      <Modal
        title="AI 摘要预览"
        open={!!summaryPreview?.open}
        width={760}
        onCancel={() => {
          if (summaryPreview?.streaming) {
            void cancelSummary();
          } else {
            setSummaryPreview(null);
          }
        }}
        footer={
          summaryPreview ? (
            <Space>
              {summaryPreview.streaming ? (
                <Button onClick={cancelSummary} disabled={!summaryPreview.requestId}>
                  停止生成
                </Button>
              ) : (
                <Button onClick={() => setSummaryPreview(null)}>仅预览</Button>
              )}
              <Button
                type="primary"
                onClick={insertSummary}
                disabled={summaryPreview.streaming || !!summaryPreview.error || !summaryPreview.summary.trim()}
              >
                插入摘要
              </Button>
            </Space>
          ) : null
        }
      >
        {summaryPreview && (
          <Space direction="vertical" size={12} style={{ width: "100%" }}>
            <Alert
              showIcon
              type={summaryPreview.mock ? "warning" : "info"}
              message={summaryPreview.providerLabel}
              description={
                summaryPreview.mock
                  ? "当前结果来自 Mock 演示，不代表真实 AI 调用成功。"
                  : "当前结果来自 AI 资源中心配置的智能体，调用由 Rust 后端统一服务完成。"
              }
            />
            <Typography.Paragraph>
              <strong>{summaryPreview.title}</strong>
            </Typography.Paragraph>
            {summaryPreview.error && (
              <Alert type="error" showIcon message="摘要生成失败" description={summaryPreview.error} />
            )}
            <Typography.Paragraph style={{ whiteSpace: "pre-wrap", minHeight: 120 }}>
              {summaryPreview.summary || (summaryPreview.streaming ? "正在生成摘要..." : "暂无摘要内容")}
            </Typography.Paragraph>
            <Typography.Text type="secondary">
              {summaryPreview.streaming ? "生成中，请勿重复提交。" : "确认无误后可插入当前文档。"}
            </Typography.Text>
          </Space>
        )}
      </Modal>
    </>
  );
}
