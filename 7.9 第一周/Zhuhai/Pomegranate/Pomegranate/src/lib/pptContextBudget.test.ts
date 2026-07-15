import {
  calculatePptContextBudget,
  estimatePptTextTokens,
} from "./pptContextBudget.ts";
import { buildAiUnderstandingPromptParts } from "./pptUnderstandingPrompt.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const longMaterial = "中".repeat(54_236);
const promptParts = buildAiUnderstandingPromptParts({
  topic: "测试主题",
  sourceMaterial: longMaterial,
  audience: "老师/评委",
  pageCount: "8 页",
  style: "科技蓝",
  extraRequirements: "保留事实与数字",
});
const largeContext = calculatePptContextBudget({
  modelMaxContextTokens: 1_000_000,
  rawMaterial: promptParts.rawMaterial,
  promptText: promptParts.promptText,
  metadataText: promptParts.metadataText,
  reservedOutputTokens: 8_192,
});
assert(largeContext.status === "safe", "54,236 字符在 1,000,000 token 上下文中应为 safe");
assert(
  largeContext.estimatedInputTokens < (largeContext.effectiveInputBudget ?? 0),
  "大上下文模型的预计输入不应超限",
);

const smallContext = calculatePptContextBudget({
  modelMaxContextTokens: 12_000,
  rawMaterial: longMaterial,
  promptText: promptParts.promptText,
  metadataText: promptParts.metadataText,
  reservedOutputTokens: 2_000,
});
assert(smallContext.status === "exceeded", "小上下文模型应正确判定 exceeded");

const reservedBudget = calculatePptContextBudget({
  modelMaxContextTokens: 10_000,
  rawMaterial: "简短素材",
  promptText: "固定提示",
  reservedOutputTokens: 2_500,
});
assert(reservedBudget.effectiveInputBudget === 7_500, "输出预留必须从最大上下文中扣除");

const unknownBudget = calculatePptContextBudget({
  modelMaxContextTokens: null,
  rawMaterial: "素材",
  promptText: "提示",
});
assert(unknownBudget.status === "unknown", "未配置上下文时必须为 unknown");
assert(unknownBudget.effectiveInputBudget === null, "unknown 状态不能伪造有效预算");

const mixed = "# 标题\n北京 BIT 2026，增长 42.5%。\nhttps://example.com/a?id=7 **重点**";
const mixedTokens = estimatePptTextTokens(mixed);
assert(mixedTokens > 0, "中英文、数字、URL、Markdown 混合文本必须能估算");
assert(Number.isInteger(mixedTokens), "预计 token 数必须是整数");

const switchedToLargeModel = calculatePptContextBudget({
  modelMaxContextTokens: 1_000_000,
  rawMaterial: longMaterial,
  promptText: promptParts.promptText,
  metadataText: promptParts.metadataText,
  reservedOutputTokens: 2_000,
});
assert(
  smallContext.status === "exceeded" && switchedToLargeModel.status === "safe",
  "切换模型配置后必须根据新的 maxContextTokens 重新判定",
);
