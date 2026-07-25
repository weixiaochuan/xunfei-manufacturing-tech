import type { PptChunkUnderstandingDraft, PptMaterialChunk } from "../types/index.ts";
import {
  buildPptChunkUnderstandingPrompt,
  buildPptUnderstandingMergePrompt,
  parsePptChunkUnderstandingResponse,
  parsePptUnderstandingMergeResponse,
} from "./pptChunkUnderstandingPrompt.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function expectThrows(callback: () => unknown, expectedMessage: string): void {
  try {
    callback();
  } catch {
    return;
  }
  throw new Error(expectedMessage);
}

const context = {
  topic: "智能制造",
  audience: "大学教师",
  pageCount: "10 页",
  style: "简洁",
  extraRequirements: "保留数据与案例",
};
const chunk: PptMaterialChunk = {
  id: "ppt-material-1",
  index: 1,
  total: 2,
  text: "绝不能出现在最终合并请求的原始正文：2026 年产量增长 37.5%。",
  sourceTitles: ["调研报告"],
  headingContext: ["现状", "核心数据"],
  startCharacter: 0,
  endCharacter: 35,
  estimatedTokens: 20,
};

const prompt = buildPptChunkUnderstandingPrompt(context, chunk);
for (const required of [
  "一份完整 PPT 材料的其中一部分",
  "分段只是为了适应模型单次读取容量",
  "不代表原文的章节、板块、主题或逻辑划分",
  "不得根据分段数量推断原文有几个板块",
  "不添加外部知识",
  "不改变原文事实",
  "日期、数字、人物、机构",
  "不把当前部分压缩成一句话",
  "不生成整份 PPT 的最终方案",
  "跨部分引用",
  "understandingSummary",
  "keyPriorities",
  "narrativeMainline",
  "suggestedPageStructure",
  "visualExpressionAdvice",
  "openQuestions",
]) {
  assert(prompt.includes(required), `分段六维 Prompt 缺少约束：${required}`);
}

const validJson = JSON.stringify({
  chunkId: chunk.id,
  chunkIndex: chunk.index,
  understandingSummary: "本段说明产量变化及其意义。",
  keyPriorities: "2026 年产量增长 37.5%。",
  narrativeMainline: "先说明现状，再解释增长。",
  suggestedPageStructure: "可支持现状页和数据页。",
  visualExpressionAdvice: "使用柱状图展示增长。",
  openQuestions: "需与下一部分核对增长原因。",
});
const parsed = parsePptChunkUnderstandingResponse(validJson, chunk);
assert(parsed.chunkId === chunk.id && parsed.chunkIndex === 1, "必须保留并校验当前段编号");
assert(parsed.keyPriorities.includes("37.5%"), "六维草稿必须保留段内关键数据");

expectThrows(
  () => parsePptChunkUnderstandingResponse(`结果如下：${validJson}`, chunk),
  "不得从普通说明文字中猜测 JSON",
);
expectThrows(
  () => parsePptChunkUnderstandingResponse("{not-json}", chunk),
  "非法 JSON 必须使当前段失败",
);
expectThrows(
  () =>
    parsePptChunkUnderstandingResponse(
      JSON.stringify({ ...JSON.parse(validJson), openQuestions: "" }),
      chunk,
    ),
  "六个字段中的任何一个为空时必须失败",
);
expectThrows(
  () =>
    parsePptChunkUnderstandingResponse(
      JSON.stringify({ ...JSON.parse(validJson), chunkId: "wrong-id" }),
      chunk,
    ),
  "响应段编号不一致时必须失败",
);

const secondDraft: PptChunkUnderstandingDraft = {
  chunkId: "ppt-material-2",
  chunkIndex: 2,
  understandingSummary: "第二段说明实施方案。",
  keyPriorities: "分三阶段推进。",
  narrativeMainline: "从试点进入推广。",
  suggestedPageStructure: "可支持实施路径页。",
  visualExpressionAdvice: "使用三阶段路线图。",
  openQuestions: "暂无。",
};
const mergePrompt = buildPptUnderstandingMergePrompt({
  ...context,
  chunks: [
    {
      chunkId: chunk.id,
      chunkIndex: chunk.index,
      sourceTitles: chunk.sourceTitles,
      headingContext: chunk.headingContext,
      draft: parsed,
    },
    {
      chunkId: secondDraft.chunkId,
      chunkIndex: secondDraft.chunkIndex,
      sourceTitles: ["调研报告"],
      headingContext: ["核心数据", "后续进展"],
      draft: secondDraft,
    },
  ],
});
assert(!mergePrompt.includes(chunk.text), "最终合并请求不得再次包含完整原始正文");
assert(
  !mergePrompt.includes(chunk.id) && !mergePrompt.includes(secondDraft.chunkId),
  "最终合并输入不得暴露内部读取编号",
);
assert(mergePrompt.includes("调研报告"), "最终合并必须包含来源标题");
assert(mergePrompt.includes("现状") && mergePrompt.includes("后续进展"), "最终合并必须包含原文标题上下文");
assert(
  mergePrompt.indexOf('"readingOrder":1') < mergePrompt.indexOf('"readingOrder":2'),
  "草稿必须按原始顺序进入最终合并",
);
for (const required of [
  "合并重复内容但不得丢失重要事实",
  "冲突时在 openQuestions 中明确说明",
  "总页数",
  "统一叙事主线",
  "不增加输入草稿中不存在的事实",
  "分段只是为了适应模型单次读取容量",
  "不得根据分段数量推断原文有几个板块",
  "不得依据 chunkIndex、chunkCount、请求次数或技术切割位置判断原文结构",
  "相邻草稿来自同一个来源",
  "必须视为同一份连续材料",
  "不能理解为两篇文章或两大板块",
  "原文真实标题、来源标题、时间线、内容语义和用户要求",
  "当前部分”“本部分”“前半部分”“后半部分",
  "不得出现第一块、第二块、分块、片段、chunk",
  "每一项必须单独一行",
  "页面排版、页面安排、内容比例",
  "暂无需要用户补充的信息，系统将根据现有材料自动完成内容组织与版式规划。",
  "输出前在本次请求内部自检并修正，不得发起额外请求",
  "是否错误地把技术分段当作原文板块",
  "是否遗漏跨分段的时间、因果和上下文关系",
]) {
  assert(mergePrompt.includes(required), `最终合并 Prompt 缺少约束：${required}`);
}

const finalDraft = parsePptUnderstandingMergeResponse(
  JSON.stringify({
    understandingSummary: "最终摘要",
    keyPriorities: "1. 最终重点一 2. 最终重点二",
    narrativeMainline: "1. 起点 2. 发展",
    suggestedPageStructure: "1. 首页 2. 总结页",
    visualExpressionAdvice: "1、时间线 2、数据图",
    openQuestions: "暂无需要用户补充的信息，系统将根据现有材料自动完成内容组织与版式规划。",
  }),
);
assert(finalDraft.keyPriorities.includes("\n2. 最终重点二"), "最终重点列表必须自动逐项换行");
assert(finalDraft.suggestedPageStructure.includes("\n2. 总结页"), "最终页面结构必须自动逐项换行");
expectThrows(
  () =>
    parsePptUnderstandingMergeResponse(
      JSON.stringify({
        ...finalDraft,
        understandingSummary: "第一块介绍早年经历，第二块介绍晚年经历。",
      }),
    ),
  "最终解析不得接受内部技术分段术语",
);
