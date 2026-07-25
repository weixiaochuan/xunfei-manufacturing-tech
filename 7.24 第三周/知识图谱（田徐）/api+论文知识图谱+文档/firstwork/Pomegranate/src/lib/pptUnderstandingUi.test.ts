import {
  formatPptAnalysisProgress,
  formatPptFailedParts,
  formatPptFeeIntroduction,
  PPT_UNDERSTANDING_FIELD_DESCRIPTIONS,
  PPT_UNDERSTANDING_UI_COPY,
} from "./pptUnderstandingUi.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const mainInterfaceCopy = [
  ...Object.values(PPT_UNDERSTANDING_UI_COPY),
  formatPptFeeIntroduction(3),
  formatPptAnalysisProgress(2, 3),
  formatPptFailedParts([2]),
].join("\n");
for (const forbidden of [
  "token",
  "最大上下文",
  "可用输入预算",
  "素材地图",
  "分块",
  "chunk",
  "materialRevision",
  "全局组织",
  "safe",
  "exceeded",
  "缓存块",
]) {
  assert(!mainInterfaceCopy.includes(forbidden), `主界面文案不得包含工程术语：${forbidden}`);
}

assert(PPT_UNDERSTANDING_UI_COPY.longMaterialTitle === "这份材料比较长", "长材料提示标题必须通俗");
assert(
  PPT_UNDERSTANDING_UI_COPY.originalMaterialProtection === "原始材料不会被删除、截断或修改。",
  "长材料提示必须明确保护原始素材",
);
assert(
  formatPptFeeIntroduction(3).includes("自动分成 3 个部分依次阅读"),
  "费用确认必须用普通语言说明阅读部分数量",
);
assert(formatPptAnalysisProgress(2, 3) === "正在阅读第 2 / 3 部分", "进度文案必须符合产品要求");
assert(
  formatPptFailedParts([2]) === "材料的第 2 部分暂时未能完成分析。",
  "失败文案必须指出未完成的部分",
);
assert(PPT_UNDERSTANDING_UI_COPY.progressCancel === "停止分析", "停止按钮必须使用简短文案");
assert(PPT_UNDERSTANDING_UI_COPY.retryFailedPart === "重试这一部分", "失败重试按钮必须准确");
assert(
  PPT_UNDERSTANDING_FIELD_DESCRIPTIONS.openQuestions ===
    "仅列出必须由你补充或决定的信息。页面结构、内容比例和版式设计将由系统自动完成。",
  "仍需确认的问题必须说明系统会自行完成规划决策",
);
assert(
  PPT_UNDERSTANDING_FIELD_DESCRIPTIONS.suggestedPageStructure.includes("不需要用户自行设计版式"),
  "页面结构辅助文案必须解除用户的版式设计负担",
);
assert(
  PPT_UNDERSTANDING_FIELD_DESCRIPTIONS.visualExpressionAdvice.includes("信息层级、页面比例和视觉表达"),
  "视觉建议辅助文案必须说明系统自动处理范围",
);
