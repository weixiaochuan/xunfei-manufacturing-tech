export const PPT_UNDERSTANDING_UI_COPY = {
  longMaterialTitle: "这份材料比较长",
  longMaterialDescription:
    "当前材料无法一次完成分析，系统会自动分成几部分依次阅读，再统一生成完整的 PPT 需求理解。",
  originalMaterialProtection: "原始材料不会被删除、截断或修改。",
  feeTitle: "材料较长，将分段分析",
  feeProtection: "整个过程中不会删除、截断或修改你的原始材料。",
  feeExplanation: "由于需要多次调用 AI，可能产生额外的模型使用费用。是否继续？",
  feeDetailsTitle: "查看预计调用次数",
  feeCancel: "取消",
  feeConfirm: "开始分析",
  progressPlanning: "正在准备材料",
  progressDirect: "正在阅读材料",
  progressMerging: "正在整理全部内容",
  progressDescription: "系统会在全部阅读完成后统一生成需求理解。",
  progressCancel: "停止分析",
  success: "材料已全部阅读完成，系统已生成完整的 PPT 需求理解。",
  successDescription: "本次材料采用分段阅读。",
  failure: "分析未完成",
  failureDescription: "已经完成的部分会被保留，不需要重新开始。",
  retryFailedPart: "重试这一部分",
  retry: "重新尝试",
} as const;

export const PPT_UNDERSTANDING_FIELD_DESCRIPTIONS = {
  suggestedPageStructure:
    "系统会根据页数和材料重点自动安排页面，不需要用户自行设计版式。",
  visualExpressionAdvice: "系统将自动处理信息层级、页面比例和视觉表达。",
  openQuestions:
    "仅列出必须由你补充或决定的信息。页面结构、内容比例和版式设计将由系统自动完成。",
} as const;

export function formatPptFeeIntroduction(totalParts: number): string {
  return `这份材料较长，系统会自动分成 ${totalParts} 个部分依次阅读，再统一整理为完整的 PPT 需求理解。`;
}

export function formatPptAnalysisProgress(current: number, total: number): string {
  return `正在阅读第 ${current} / ${total} 部分`;
}

export function formatPptFailedParts(indexes: number[]): string {
  return `材料的第 ${indexes.join("、")} 部分暂时未能完成分析。`;
}
