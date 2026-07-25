import type {
  PptChunkUnderstandingDraft,
  PptMaterialChunk,
  PptUnderstandingDraft,
} from "../types/index.ts";
import {
  executePptChunkUnderstandingWorkflow,
  MAX_PPT_CHUNK_UNDERSTANDING_CONCURRENCY,
} from "./pptChunkUnderstandingWorkflow.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const chunks: PptMaterialChunk[] = Array.from({ length: 4 }, (_, zeroBasedIndex) => {
  const index = zeroBasedIndex + 1;
  return {
    id: `ppt-material-${index}`,
    index,
    total: 4,
    text: `原文第 ${index} 部分`,
    sourceTitles: [`来源 ${index}`],
    headingContext: [`标题 ${index}`],
    startCharacter: zeroBasedIndex * 8,
    endCharacter: index * 8,
    estimatedTokens: 8,
  };
});

function draftFor(chunk: PptMaterialChunk): PptChunkUnderstandingDraft {
  return {
    chunkId: chunk.id,
    chunkIndex: chunk.index,
    understandingSummary: `摘要 ${chunk.index}`,
    keyPriorities: `重点 ${chunk.index}`,
    narrativeMainline: `主线 ${chunk.index}`,
    suggestedPageStructure: `结构 ${chunk.index}`,
    visualExpressionAdvice: `视觉 ${chunk.index}`,
    openQuestions: `问题 ${chunk.index}`,
  };
}

const finalDraft: PptUnderstandingDraft = {
  understandingSummary: "最终摘要",
  keyPriorities: "最终重点",
  narrativeMainline: "最终主线",
  suggestedPageStructure: "最终结构",
  visualExpressionAdvice: "最终视觉",
  openQuestions: "最终问题",
};

let activeRequests = 0;
let maximumActiveRequests = 0;
let mergeCalls = 0;
const complete = await executePptChunkUnderstandingWorkflow({
  chunks,
  cachedDrafts: [],
  isCancelled: () => false,
  analyzeChunk: async (chunk) => {
    activeRequests += 1;
    maximumActiveRequests = Math.max(maximumActiveRequests, activeRequests);
    await new Promise((resolve) => setTimeout(resolve, 5 - chunk.index));
    activeRequests -= 1;
    return draftFor(chunk);
  },
  mergeDrafts: async (drafts) => {
    mergeCalls += 1;
    assert(
      drafts.map((draft) => draft.chunkIndex).join(",") === "1,2,3,4",
      "最终合并前必须按 chunkIndex 排序",
    );
    return finalDraft;
  },
});
assert(
  maximumActiveRequests <= MAX_PPT_CHUNK_UNDERSTANDING_CONCURRENCY &&
    MAX_PPT_CHUNK_UNDERSTANDING_CONCURRENCY === 2,
  "分段理解最大并发必须集中定义且不超过 2",
);
assert(complete.analysisRequestCount === chunks.length, "N 段必须发出 N 次分段理解请求");
assert(complete.mergeRequestCount === 1 && mergeCalls === 1, "全部成功后只能合并一次");
assert(
  complete.analysisRequestCount + complete.mergeRequestCount === chunks.length + 1,
  "首次长素材请求总数必须严格为 N + 1",
);

let failedMergeCalls = 0;
const failed = await executePptChunkUnderstandingWorkflow({
  chunks,
  cachedDrafts: [],
  isCancelled: () => false,
  analyzeChunk: async (chunk) => {
    if (chunk.index === 2) throw new Error("模拟第二部分失败");
    return draftFor(chunk);
  },
  mergeDrafts: async () => {
    failedMergeCalls += 1;
    return finalDraft;
  },
});
assert(failed.failedChunkIndexes.join(",") === "2", "必须准确记录失败部分");
assert(failedMergeCalls === 0 && failed.mergeRequestCount === 0, "任意部分失败时不得最终合并");
assert(failed.drafts.length === 3, "失败时必须保留其他成功部分的六维草稿");

const retriedIndexes: number[] = [];
let retryMergeCalls = 0;
const retried = await executePptChunkUnderstandingWorkflow({
  chunks,
  cachedDrafts: failed.drafts,
  isCancelled: () => false,
  analyzeChunk: async (chunk) => {
    retriedIndexes.push(chunk.index);
    return draftFor(chunk);
  },
  mergeDrafts: async () => {
    retryMergeCalls += 1;
    return finalDraft;
  },
});
assert(retriedIndexes.join(",") === "2", "重试只能请求失败部分，不得重新分析成功部分");
assert(retried.analysisRequestCount === 1, "重试只能增加一次分段请求");
assert(retryMergeCalls === 1 && retried.mergeRequestCount === 1, "补齐失败部分后必须只合并一次");

const hierarchicalMergeCount = await executePptChunkUnderstandingWorkflow({
  chunks,
  cachedDrafts: chunks.map(draftFor),
  isCancelled: () => false,
  analyzeChunk: async (chunk) => draftFor(chunk),
  mergeDrafts: async () => ({ finalDraft, requestCount: 3 }),
});
assert(
  hierarchicalMergeCount.analysisRequestCount === 0,
  "已有全部分段草稿时不得重复请求分段理解",
);
assert(
  hierarchicalMergeCount.mergeRequestCount === 3,
  "分层合并时必须返回真实合并请求次数",
);

let cancelledCalls = 0;
const cancelledBeforeStart = await executePptChunkUnderstandingWorkflow({
  chunks,
  cachedDrafts: [],
  isCancelled: () => true,
  analyzeChunk: async (chunk) => {
    cancelledCalls += 1;
    return draftFor(chunk);
  },
  mergeDrafts: async () => finalDraft,
});
assert(cancelledCalls === 0, "用户在费用确认处取消时不得发出任何模型请求");
assert(cancelledBeforeStart.mergeRequestCount === 0, "取消后不得进入最终合并");

let cancelled = false;
const startedAfterCancel: number[] = [];
const cancelledDuringRun = await executePptChunkUnderstandingWorkflow({
  chunks,
  cachedDrafts: [],
  isCancelled: () => cancelled,
  analyzeChunk: async (chunk) => {
    startedAfterCancel.push(chunk.index);
    cancelled = true;
    await Promise.resolve();
    return draftFor(chunk);
  },
  mergeDrafts: async () => finalDraft,
});
assert(startedAfterCancel.length <= 2, "取消后不得继续启动新请求");
assert(cancelledDuringRun.cancelled, "运行中取消必须返回取消状态");
assert(cancelledDuringRun.mergeRequestCount === 0, "运行中取消不得进入最终合并");
