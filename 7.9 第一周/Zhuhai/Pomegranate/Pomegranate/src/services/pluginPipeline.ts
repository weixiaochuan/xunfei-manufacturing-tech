import { pluginApi } from "@/lib/api";
import type {
  PluginExecutionContext,
  PluginScene,
  ResolvedEnhancementContribution,
  ResolvedPluginContributions,
} from "@/types";

export interface PluginPipelineRequest {
  scene: PluginScene;
  feature: string;
  input?: unknown;
  prompt?: string;
  userRole?: "student" | "teacher" | "unknown";
  userId?: string;
  workspaceId?: string;
  sessionId?: string;
  selectedResources?: string[];
  metadata?: Record<string, unknown>;
  sessionOverrides?: Record<string, boolean>;
}

export interface PluginPipelineBeforeResult {
  context: PluginExecutionContext;
  originalInput: unknown;
  input: unknown;
  prompt: string;
  resolved: ResolvedPluginContributions;
  executedContributionIds: string[];
  warnings: string[];
}

export interface PluginPipelineAfterResult {
  output: string;
  uiContributions: ResolvedEnhancementContribution[];
  executedContributionIds: string[];
  warnings: string[];
}

function requestId() {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `plugin-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function toInputText(input: unknown) {
  if (typeof input === "string") return input;
  if (input === undefined) return "";
  try {
    return JSON.stringify(input);
  } catch {
    return String(input);
  }
}

export function pluginPipelineTextInput(
  before: PluginPipelineBeforeResult,
  fallback: string,
) {
  return typeof before.input === "string" && before.input.trim()
    ? before.input
    : fallback;
}

async function readDeclarativeResource(item: ResolvedEnhancementContribution) {
  if (item.contribution.handler.kind !== "declarative") {
    throw new Error(`不支持的增强处理器：${item.contribution.handler.kind}`);
  }
  return pluginApi.readEnhancementResource(item.pluginId, item.contribution.id);
}

function appendPrompt(base: string, heading: string, content: string) {
  const section = `${heading}\n${content.trim()}`;
  return base.trim() ? `${base.trim()}\n\n${section}` : section;
}

/**
 * 统一的模型调用前流水线。正式插件只读取声明式资源，不执行包内 JavaScript，
 * 所有启用、场景、功能、权限、依赖、冲突与顺序判断均由 Rust 解析器完成。
 */
export async function runPluginPipelineBeforeModel(
  request: PluginPipelineRequest,
): Promise<PluginPipelineBeforeResult> {
  const context: PluginExecutionContext = {
    scene: request.scene,
    feature: request.feature,
    userRole: request.userRole ?? "unknown",
    userId: request.userId,
    workspaceId: request.workspaceId,
    sessionId: request.sessionId,
    requestId: requestId(),
    input: request.input,
    selectedResources: request.selectedResources ?? [],
    metadata: request.metadata ?? {},
    sessionOverrides: request.sessionOverrides ?? {},
  };
  let resolved: ResolvedPluginContributions;
  try {
    resolved = await pluginApi.resolveEnabledContributions(context);
  } catch (error) {
    // Plugin discovery must never make the host AI feature unavailable.
    resolved = {
      context,
      activePlugins: [],
      features: [],
      agents: [],
      tools: [],
      enhancements: [],
      warnings: [`插件增强解析失败，已按原始功能继续：${String(error)}`],
    };
  }
  let input: unknown = request.input;
  let prompt = request.prompt ?? "";
  const executedContributionIds: string[] = [];
  const warnings = [...resolved.warnings];

  for (const item of resolved.enhancements) {
    const hook = item.contribution.hook;
    if (!["inputProcessor", "contextProvider", "promptEnhancer"].includes(hook)) continue;
    const startedAt = Date.now();
    try {
      const resource = await readDeclarativeResource(item);
      const contributionKey = `${item.pluginId}:${item.contribution.id}`;
      if (hook === "inputProcessor") {
        input = resource.includes("{{input}}")
          ? resource.split("{{input}}").join(toInputText(input))
          : `${toInputText(input)}\n${resource}`.trim();
      } else {
        prompt = appendPrompt(prompt, `[插件增强 ${contributionKey}]`, resource);
      }
      executedContributionIds.push(contributionKey);
      await pluginApi.recordExecution({
        pluginId: item.pluginId,
        contributionId: item.contribution.id,
        hook: item.contribution.hook,
        context,
        status: "success",
        durationMs: Date.now() - startedAt,
      }).catch((error) => {
        warnings.push(`${contributionKey} 执行成功，但日志写入失败：${String(error)}`);
      });
    } catch (error) {
      const contributionKey = `${item.pluginId}:${item.contribution.id}`;
      warnings.push(`${contributionKey} 执行失败：${String(error)}`);
      await pluginApi.recordExecution({
        pluginId: item.pluginId,
        contributionId: item.contribution.id,
        hook: item.contribution.hook,
        context,
        status: "failed",
        durationMs: Date.now() - startedAt,
        errorMessage: String(error),
      }).catch(() => undefined);
    }
  }

  return {
    context,
    originalInput: request.input,
    input,
    prompt,
    resolved,
    executedContributionIds,
    warnings,
  };
}

/** 模型返回后的 outputProcessor/uiContribution 阶段。 */
export async function runPluginPipelineAfterModel(
  before: PluginPipelineBeforeResult,
  modelOutput: string,
): Promise<PluginPipelineAfterResult> {
  let output = modelOutput;
  const executedContributionIds = [...before.executedContributionIds];
  const warnings = [...before.warnings];
  const uiContributions = before.resolved.enhancements.filter(
    (item) => item.contribution.hook === "uiContribution",
  );

  for (const item of before.resolved.enhancements) {
    if (item.contribution.hook !== "outputProcessor") continue;
    const startedAt = Date.now();
    try {
      const resource = await readDeclarativeResource(item);
      output = resource.includes("{{output}}")
        ? resource.split("{{output}}").join(output)
        : `${output}\n\n${resource}`.trim();
      executedContributionIds.push(`${item.pluginId}:${item.contribution.id}`);
      await pluginApi.recordExecution({
        pluginId: item.pluginId,
        contributionId: item.contribution.id,
        hook: item.contribution.hook,
        context: before.context,
        status: "success",
        durationMs: Date.now() - startedAt,
      }).catch((error) => {
        warnings.push(`${item.pluginId}:${item.contribution.id} 日志写入失败：${String(error)}`);
      });
    } catch (error) {
      warnings.push(`${item.pluginId}:${item.contribution.id} 执行失败：${String(error)}`);
      await pluginApi.recordExecution({
        pluginId: item.pluginId,
        contributionId: item.contribution.id,
        hook: item.contribution.hook,
        context: before.context,
        status: "failed",
        durationMs: Date.now() - startedAt,
        errorMessage: String(error),
      }).catch(() => undefined);
    }
  }

  return { output, uiContributions, executedContributionIds, warnings };
}
