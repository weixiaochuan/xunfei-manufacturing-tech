import { buildPptChunkUnderstandingPromptParts } from "./pptChunkUnderstandingPrompt.ts";
import { estimatePptTextTokens } from "./pptContextBudget.ts";
import {
  planPptMaterialChunks,
  resolvePptMaterialRequestPlan,
} from "./pptMaterialChunking.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const promptContext = {
  topic: "制造业数字化转型",
  audience: "大学教师",
  pageCount: "12 页",
  style: "科技蓝",
  extraRequirements: "保留全部数据",
};
const paragraph = (label: string) =>
  `${label}。该段保留人物、机构、地点、事件、观点和证据。数据为 2026 年 7 月 15 日、增长 37.5%，不得丢失。\n\n`;
const sourceOne = `# 来源 1：文档｜第一份资料\n\n## 背景\n\n${Array.from(
  { length: 40 },
  (_, index) => paragraph(`背景事实 ${index + 1}`),
).join("")}## 方案\n\n${Array.from(
  { length: 35 },
  (_, index) => paragraph(`方案事实 ${index + 1}`),
).join("")}`;
const sourceTwo = `# 来源 2：日记｜第二份资料\n\n## 反馈\n\n${Array.from(
  { length: 50 },
  (_, index) => `- 反馈 ${index + 1}：保留原始数字 ${1000 + index}\n`,
).join("")}\n${Array.from(
  { length: 30 },
  (_, index) => paragraph(`后续事项 ${index + 1}`),
).join("")}`;
const rawMaterial = `${sourceOne}\n\n---\n\n${sourceTwo}`;

const smallPlan = planPptMaterialChunks({
  rawMaterial,
  modelMaxContextTokens: 7_000,
  reservedOutputTokens: 1_000,
  promptContext,
});
const largePlan = planPptMaterialChunks({
  rawMaterial,
  modelMaxContextTokens: 12_000,
  reservedOutputTokens: 1_000,
  promptContext,
});

assert(smallPlan.chunks.length > 1, "长素材必须生成动态多段计划");
assert(
  largePlan.chunks.length < smallPlan.chunks.length,
  "模型容量变大后分段数量应动态减少，而不是固定段数",
);
assert(
  smallPlan.chunks.map((chunk) => chunk.text).join("") === rawMaterial,
  "所有段落按序拼接后必须与原素材逐字一致，不丢失也不重复",
);
assert(
  smallPlan.chunks.every(
    (chunk, index) =>
      chunk.index === index + 1 &&
      chunk.total === smallPlan.chunks.length &&
      chunk.startCharacter < chunk.endCharacter,
  ),
  "每段必须包含稳定序号、总数和字符范围",
);
assert(
  smallPlan.chunks.some((chunk) => chunk.sourceTitles.includes("第一份资料")) &&
    smallPlan.chunks.some((chunk) => chunk.sourceTitles.includes("第二份资料")),
  "来源标题必须进入分段元数据",
);
assert(
  smallPlan.chunks.some((chunk) => chunk.headingContext.includes("背景")) &&
    smallPlan.chunks.some((chunk) => chunk.headingContext.includes("反馈")),
  "Markdown 标题上下文必须进入分段元数据",
);
assert(
  smallPlan.chunks.every(
    (chunk) =>
      estimatePptTextTokens(
        buildPptChunkUnderstandingPromptParts(promptContext, chunk).fullPrompt,
      ) <= smallPlan.modelMaxContextTokens - smallPlan.outputReserveTokens,
  ),
  "每段真实六维理解 Prompt 都必须在当前模型可用输入预算内",
);

let unknownContextRejected = false;
try {
  planPptMaterialChunks({ rawMaterial, modelMaxContextTokens: null, promptContext });
} catch (error) {
  unknownContextRejected = String(error).includes("请先完善模型配置");
}
assert(unknownContextRejected, "未知模型容量时必须阻止不安全分段并给出明确引导");

for (const contextStatus of ["safe", "near_limit"] as const) {
  const requestPlan = resolvePptMaterialRequestPlan({ contextStatus });
  assert(requestPlan.mode === "direct", `${contextStatus} 必须沿用单次直接理解`);
  assert(requestPlan.minimumTotalRequests === 1, `${contextStatus} 只应规划一次模型请求`);
  assert(!requestPlan.requiresFeeConfirmation, `${contextStatus} 不应显示多次调用确认`);
}

const chunkedRequests = resolvePptMaterialRequestPlan({
  contextStatus: "exceeded",
  totalChunks: 5,
  cachedChunks: 0,
});
assert(chunkedRequests.mode === "chunked", "超过容量时必须进入分段模式");
assert(chunkedRequests.chunkRequests === 5, "首次分析必须读取全部五段");
assert(chunkedRequests.minimumTotalRequests === 6, "五段后必须且只能再有一次最终合并");
assert(chunkedRequests.requiresFeeConfirmation, "首次长素材分析前必须要求费用确认");

const retryRequests = resolvePptMaterialRequestPlan({
  contextStatus: "exceeded",
  totalChunks: 5,
  cachedChunks: 4,
});
assert(retryRequests.chunkRequests === 1, "重试只应请求尚未成功的一段");
assert(retryRequests.minimumTotalRequests === 2, "补齐失败段后只再执行一次最终合并");
assert(!retryRequests.requiresFeeConfirmation, "已有成功结果的重试不应重复弹出首次费用确认");

const metadataOnlyRequests = resolvePptMaterialRequestPlan({
  contextStatus: "exceeded",
  totalChunks: 5,
  cachedChunks: 5,
});
assert(metadataOnlyRequests.minimumTotalRequests === 1, "复用全部六维草稿时只需最终合并请求");
assert(!metadataOnlyRequests.requiresFeeConfirmation, "只重新合并时不应显示多次调用确认");
