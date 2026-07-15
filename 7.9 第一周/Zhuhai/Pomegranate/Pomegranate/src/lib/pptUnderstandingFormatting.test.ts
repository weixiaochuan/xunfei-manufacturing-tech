import type { PptUnderstandingDraft } from "../types/index.ts";
import {
  normalizePptUnderstandingDraftFormatting,
  normalizePptUnderstandingFormatting,
  PPT_NO_OPEN_QUESTIONS_TEXT,
  preparePptUnderstandingDraftForDisplay,
  validatePptUnderstandingOutputBoundaries,
} from "./pptUnderstandingFormatting.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function expectThrows(callback: () => unknown, message: string): void {
  try {
    callback();
  } catch {
    return;
  }
  throw new Error(message);
}

const inline = "1. 出生与成长 2. 革命奠基 3. 建国与探索";
const normalized = normalizePptUnderstandingFormatting(inline);
assert(
  normalized === "1. 出生与成长 \n2. 革命奠基 \n3. 建国与探索",
  "同一行中的编号项目必须一项一行",
);
assert(
  normalizePptUnderstandingFormatting("1、早期思想 2、革命实践") ===
    "1、早期思想 \n2、革命实践",
  "必须支持中文顿号编号",
);
assert(
  normalizePptUnderstandingFormatting("（1）早期经历 （2）晚年经历") ===
    "（1）早期经历 \n（2）晚年经历",
  "必须支持中文括号编号",
);
assert(
  normalizePptUnderstandingFormatting("第一阶段\n1. 已有换行\n2. 保持换行") ===
    "第一阶段\n1. 已有换行\n2. 保持换行",
  "已有正常换行不得被改变或重复插入",
);
assert(
  normalizePptUnderstandingFormatting("1893年出生，1976年逝世。") ===
    "1893年出生，1976年逝世。",
  "年份不得被错误换行",
);
assert(
  normalizePptUnderstandingFormatting("圆周率约为 3.14，增长率为 2.75%。") ===
    "圆周率约为 3.14，增长率为 2.75%。",
  "小数不得被错误切开",
);
assert(
  normalizePptUnderstandingFormatting(normalized) === normalized,
  "格式化函数重复执行必须保持结果一致",
);

const draft: PptUnderstandingDraft = {
  understandingSummary: "自然段摘要。",
  keyPriorities: inline,
  narrativeMainline: "1. 起点 2. 转折 3. 结论",
  suggestedPageStructure: "1. 封面 2. 生平 3. 评价",
  visualExpressionAdvice: "1、使用时间线 2、使用数据图",
  openQuestions: "",
};
const normalizedDraft = normalizePptUnderstandingDraftFormatting(draft);
assert(normalizedDraft.keyPriorities.split("\n").length === 3, "重点取舍必须逐项换行");
assert(normalizedDraft.narrativeMainline.split("\n").length === 3, "多阶段叙事主线必须逐项换行");
assert(normalizedDraft.suggestedPageStructure.split("\n").length === 3, "页面结构必须逐项换行");
assert(normalizedDraft.visualExpressionAdvice.split("\n").length === 2, "视觉建议必须逐项换行");
assert(normalizedDraft.openQuestions === PPT_NO_OPEN_QUESTIONS_TEXT, "没有用户问题时必须使用统一说明");
assert(
  normalizePptUnderstandingDraftFormatting({ ...draft, openQuestions: "暂无。" }).openQuestions ===
    PPT_NO_OPEN_QUESTIONS_TEXT,
  "模型返回常见的暂无问题表述时也必须统一为产品指定说明",
);

for (const internalTerm of ["第一块", "第二块", "分块", "chunk", "技术片段"]) {
  expectThrows(
    () =>
      validatePptUnderstandingOutputBoundaries({
        ...normalizedDraft,
        keyPriorities: `${internalTerm}介绍生平`,
      }),
    `最终结果不得包含内部读取术语：${internalTerm}`,
  );
}
for (const systemQuestion of [
  "各部分占多少比例？",
  "是否需要合并某些页面？",
  "哪些内容应该放在同一页？",
  "如何避免页面拥挤？",
  "使用什么图表比较合适？",
  "页面层级怎么设计？",
  "正面和负面内容如何布局？",
]) {
  expectThrows(
    () =>
      validatePptUnderstandingOutputBoundaries({
        ...normalizedDraft,
        openQuestions: systemQuestion,
      }),
    `系统规划问题不得进入 openQuestions：${systemQuestion}`,
  );
}
validatePptUnderstandingOutputBoundaries({
  ...normalizedDraft,
  openQuestions: "两处互相冲突的出生日期应采用哪一个？",
});
validatePptUnderstandingOutputBoundaries({
  ...normalizedDraft,
  openQuestions: "是否允许展示敏感个人信息？汇报立场应保持中立还是批判？",
});
assert(
  preparePptUnderstandingDraftForDisplay(draft).openQuestions === PPT_NO_OPEN_QUESTIONS_TEXT,
  "最终展示准备必须同时执行格式兜底和问题边界检查",
);
