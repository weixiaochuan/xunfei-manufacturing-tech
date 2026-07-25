import type { PptMaterialSourceRef, PptUnderstandingDraft } from "@/types";
import type { PptGenerationMode, PptMaterialInputMode } from "@/store/pptGenerationDraft";

export interface PptUnderstandingExportInput {
  title: string;
  audience: string;
  pageCount: string;
  style: string;
  generationMode: PptGenerationMode;
  understandingDraft: PptUnderstandingDraft;
  materialSources: PptMaterialSourceRef[];
  materialInputMode: PptMaterialInputMode;
  exportedAt: Date;
  stale?: boolean;
}

export interface PptUnderstandingExportOutput {
  filename: string;
  content: string;
}

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

export function formatPptUnderstandingExportTimestamp(date: Date): {
  filenameStamp: string;
  display: string;
} {
  const year = date.getFullYear();
  const month = pad2(date.getMonth() + 1);
  const day = pad2(date.getDate());
  const hour = pad2(date.getHours());
  const minute = pad2(date.getMinutes());
  return {
    filenameStamp: `${year}${month}${day}_${hour}${minute}`,
    display: `${year}-${month}-${day} ${hour}:${minute}`,
  };
}

export function sanitizeMarkdownExportFilename(title: string, exportedAt: Date): string {
  const { filenameStamp } = formatPptUnderstandingExportTimestamp(exportedAt);
  const safeTitle = title
    .replace(/[\\/:*?"<>|\u0000-\u001F]/g, " ")
    .replace(/\s+/g, " ")
    .replace(/[. ]+$/g, "")
    .replace(/^[. ]+/g, "")
    .trim()
    .slice(0, 60)
    .replace(/[. ]+$/g, "");
  return safeTitle
    ? `${safeTitle}_AI需求理解_${filenameStamp}.md`
    : `PPT需求理解_${filenameStamp}.md`;
}

function readableValue(value: string | null | undefined): string {
  const normalized = value?.trim();
  return normalized ? normalized : "暂无";
}

function singleLineValue(value: string): string {
  return readableValue(value).replace(/\s*\n\s*/g, " ");
}

function generationModeLabel(mode: PptGenerationMode): string {
  return mode === "agent" ? "ppt-master 原生实验模式" : "稳定模式";
}

function materialSourceLines(input: PptUnderstandingExportInput): string[] {
  if (input.materialInputMode === "manual") {
    return ["- 直接输入的文字材料"];
  }
  if (input.materialSources.length === 0) {
    return ["- 暂无"];
  }
  return input.materialSources.map((source) =>
    `- ${source.sourceType === "diary" ? "日记" : "文档"}｜${singleLineValue(source.title)}`,
  );
}

export function buildPptUnderstandingMarkdown(
  input: PptUnderstandingExportInput,
): PptUnderstandingExportOutput {
  const timestamp = formatPptUnderstandingExportTimestamp(input.exportedAt);
  const warning = input.stale
    ? "\n> ⚠️ 注意：本理解稿基于较早版本的素材，当前素材已发生变化。\n"
    : "";
  const content = [
    "# PPT 需求理解确认稿",
    warning.trim(),
    "## 基本信息",
    "",
    `- **PPT 主题：** ${singleLineValue(input.title)}`,
    `- **汇报对象：** ${singleLineValue(input.audience)}`,
    `- **建议页数：** ${singleLineValue(input.pageCount)}`,
    `- **视觉风格：** ${singleLineValue(input.style)}`,
    `- **生成模式：** ${generationModeLabel(input.generationMode)}`,
    `- **导出时间：** ${timestamp.display}`,
    "",
    "## AI 理解摘要",
    "",
    readableValue(input.understandingDraft.understandingSummary),
    "",
    "## 重点取舍",
    "",
    readableValue(input.understandingDraft.keyPriorities),
    "",
    "## 叙事主线",
    "",
    readableValue(input.understandingDraft.narrativeMainline),
    "",
    "## 建议页面结构",
    "",
    readableValue(input.understandingDraft.suggestedPageStructure),
    "",
    "## 视觉与表达建议",
    "",
    readableValue(input.understandingDraft.visualExpressionAdvice),
    "",
    "## 仍需确认的问题",
    "",
    readableValue(input.understandingDraft.openQuestions),
    "",
    "## 素材来源",
    "",
    ...materialSourceLines(input),
    "",
    "## 备注",
    "",
    "本文件是 AI 初步分析并经用户检查、修改后的 PPT 规划确认稿。",
    "最终生成效果仍可能根据内容密度和版式适配进行局部调整。",
    "",
  ]
    .filter((line, index, lines) => line !== "" || lines[index - 1] !== "")
    .join("\n");

  return {
    filename: sanitizeMarkdownExportFilename(input.title, input.exportedAt),
    content,
  };
}
