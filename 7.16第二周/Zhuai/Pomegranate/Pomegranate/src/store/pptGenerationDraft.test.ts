import type {
  PptChunkUnderstandingDraft,
  PptMaterialChunkPlan,
  PptUnderstandingDraft,
} from "../types/index.ts";
import { usePptGenerationDraftStore } from "./pptGenerationDraft.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const understanding: PptUnderstandingDraft = {
  understandingSummary: "已有摘要",
  keyPriorities: "已有重点",
  narrativeMainline: "已有主线",
  suggestedPageStructure: "已有结构",
  visualExpressionAdvice: "已有视觉",
  openQuestions: "已有问题",
};

function chunkDraft(chunkIndex: number): PptChunkUnderstandingDraft {
  return {
    chunkId: `ppt-material-${chunkIndex}`,
    chunkIndex,
    understandingSummary: `摘要 ${chunkIndex}`,
    keyPriorities: `重点 ${chunkIndex}`,
    narrativeMainline: `主线 ${chunkIndex}`,
    suggestedPageStructure: `结构 ${chunkIndex}`,
    visualExpressionAdvice: `视觉 ${chunkIndex}`,
    openQuestions: `问题 ${chunkIndex}`,
  };
}

const chunkPlan: PptMaterialChunkPlan = {
  chunks: [1, 2].map((index) => ({
    id: `ppt-material-${index}`,
    index,
    total: 2,
    text: `原始素材第 ${index} 部分`,
    sourceTitles: ["测试来源"],
    headingContext: [],
    startCharacter: (index - 1) * 10,
    endCharacter: index * 10,
    estimatedTokens: 10,
  })),
  totalCharacters: 20,
  totalEstimatedTokens: 20,
  chunkTokenBudget: 100,
  promptOverheadTokens: 20,
  metadataReserveTokens: 10,
  outputReserveTokens: 50,
  modelMaxContextTokens: 500,
};

usePptGenerationDraftStore.getState().resetPptDraft();
let state = usePptGenerationDraftStore.getState();
const runtimeState = state as unknown as Record<string, unknown>;
assert(!("materialCleaningStatus" in runtimeState), "Store 必须删除 AI 清洗状态");
assert(!("materialMap" in runtimeState), "Store 必须删除素材地图状态");
assert(!("materialChunkAnalyses" in runtimeState), "Store 必须删除旧通用分段分析状态");

state.setManualRawMaterial("需要分段阅读的完整原始素材");
state = usePptGenerationDraftStore.getState();
const analysisRevision = state.materialRevision;
const firstRunId = state.beginMaterialAnalysis("chunked", analysisRevision);
state.setMaterialChunkPlan(chunkPlan, analysisRevision, firstRunId);
state.setMaterialAnalysisStage(
  "analyzing",
  { current: 1, total: 2, stage: "analyzing" },
  analysisRevision,
  firstRunId,
);
state.cacheChunkUnderstandingDraft(chunkDraft(1), analysisRevision, firstRunId);
state.setMaterialAnalysisError("第二部分失败", [2], analysisRevision, firstRunId);
state = usePptGenerationDraftStore.getState();
assert(state.materialAnalysisStatus === "error", "分段失败必须进入全局错误状态");
assert(state.failedChunkIndexes.join(",") === "2", "全局状态必须记录失败部分序号");
assert(state.chunkUnderstandingDrafts.length === 1, "失败时必须保留已成功的六维草稿");
assert(
  state.manualRawMaterial === "需要分段阅读的完整原始素材",
  "分段分析不得清洗、截断或覆盖原始素材",
);

const retryRunId = state.beginMaterialAnalysis("chunked", analysisRevision, true);
state = usePptGenerationDraftStore.getState();
assert(state.chunkUnderstandingDrafts.length === 1, "重试必须复用已成功草稿，而非从头开始");
assert(state.materialChunkPlan?.chunks.length === 2, "重试必须复用同一素材修订的分段计划");
state.cacheChunkUnderstandingDraft(chunkDraft(2), analysisRevision, retryRunId);
state = usePptGenerationDraftStore.getState();
assert(state.chunkUnderstandingDrafts.length === 2, "失败部分重试成功后缓存应完整");
assert(
  state.chunkUnderstandingDrafts.map((draft) => draft.chunkIndex).join(",") === "1,2",
  "六维草稿在 Store 中必须按原始顺序排列",
);

state.setBasicFields({
  audience: "企业客户",
  pageCount: "20 页",
  style: "学术简洁",
  extraRequirements: "突出案例",
});
state = usePptGenerationDraftStore.getState();
assert(state.chunkAnalysisRevision === analysisRevision, "只修改基础信息不得让分段分析失效");
assert(state.chunkUnderstandingDrafts.length === 2, "只修改受众、页数、风格时必须复用六维草稿");

state.setActiveMode("advanced");
state.setActiveMode("smart");
state = usePptGenerationDraftStore.getState();
assert(state.chunkUnderstandingDrafts.length === 2, "普通页面状态切换不得清空已完成草稿");

state.setUnderstandingDraft(understanding, analysisRevision);
state.updateUnderstandingField("keyPriorities", "1. 用户保留第一项\n2. 用户保留第二项");
state.setActiveMode("advanced");
state.setActiveMode("smart");
state = usePptGenerationDraftStore.getState();
assert(
  state.understandingDraft?.keyPriorities === "1. 用户保留第一项\n2. 用户保留第二项",
  "用户编辑后的换行必须原样写入 Store，并在页面状态切换后保留",
);
const failureRunId = state.beginMaterialAnalysis("chunked", analysisRevision, true);
state.setMaterialAnalysisError("最终整理失败", [], analysisRevision, failureRunId);
state = usePptGenerationDraftStore.getState();
assert(state.understandingDraft?.understandingSummary === "已有摘要", "失败不得覆盖已有有效六维理解");

const runBeforeCancel = state.beginMaterialAnalysis("chunked", analysisRevision, true);
state.cancelMaterialAnalysis();
state.cacheChunkUnderstandingDraft(
  { ...chunkDraft(1), understandingSummary: "取消后迟到响应" },
  analysisRevision,
  runBeforeCancel,
);
state = usePptGenerationDraftStore.getState();
assert(
  !state.chunkUnderstandingDrafts.some((draft) => draft.understandingSummary === "取消后迟到响应"),
  "取消后的迟到响应必须由运行编号拦截",
);
assert(state.chunkUnderstandingDrafts.length === 2, "取消后已成功草稿可以继续保留");

state.setManualRawMaterial("用户已经修改原始素材");
state = usePptGenerationDraftStore.getState();
assert(state.chunkAnalysisRevision === null, "素材改变必须使分段分析修订失效");
assert(state.materialChunkPlan === null, "素材改变必须清除旧分段计划");
assert(state.chunkUnderstandingDrafts.length === 0, "素材改变必须清除全部旧六维草稿");
assert(state.failedChunkIndexes.length === 0, "素材改变必须清除旧失败记录");

state.setMaterialInputMode("internal");
state.replaceInternalMaterial(
  [
    {
      id: 7,
      sourceType: "document",
      title: "数据库原文",
      plainText: "数据库中的完整正文",
    },
  ],
  "【来源：数据库原文】\n数据库中的完整正文",
  false,
);
state = usePptGenerationDraftStore.getState();
const originalSourceText = state.resolvedMaterialSources[0]?.plainText;
const internalRevision = state.materialRevision;
const internalRunId = state.beginMaterialAnalysis("direct", internalRevision);
state.finishMaterialAnalysis(internalRevision, internalRunId);
state = usePptGenerationDraftStore.getState();
assert(state.resolvedMaterialSources[0]?.plainText === originalSourceText, "分析不得修改导入来源或 notes 正文");
