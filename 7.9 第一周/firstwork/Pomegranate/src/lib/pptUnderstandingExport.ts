import type { PptUnderstandingDraft, ResolvedPptMaterialSource } from "@/types";

interface BuildPptUnderstandingMarkdownInput {
  title: string;
  audience: string;
  pageCount: string;
  style: string;
  generationMode: "agent" | "template";
  understandingDraft: PptUnderstandingDraft;
  materialSources: ResolvedPptMaterialSource[];
  materialInputMode: "manual" | "internal";
  exportedAt: Date;
  stale: boolean;
}

function sanitizeFilenamePart(value: string): string {
  const cleaned = value
    .trim()
    .replace(/[\\/:*?"<>|]/g, "-")
    .replace(/\s+/g, "_");
  return cleaned || "ppt-understanding";
}

function formatTimestamp(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  const hours = String(date.getHours()).padStart(2, "0");
  const minutes = String(date.getMinutes()).padStart(2, "0");
  const seconds = String(date.getSeconds()).padStart(2, "0");
  return `${year}-${month}-${day} ${hours}:${minutes}:${seconds}`;
}

function buildSourceList(sources: ResolvedPptMaterialSource[]): string {
  if (sources.length === 0) return "- 无";
  return sources
    .map((source) => {
      const kind = source.sourceType === "diary" ? "日记" : "文档";
      const details: string[] = [];
      if (source.dailyDate) details.push(source.dailyDate);
      if (typeof source.wordCount === "number" && source.wordCount > 0) {
        details.push(`${source.wordCount} 字`);
      }
      if (source.hasUnsavedChanges) details.push("包含未保存修改");
      const suffix = details.length > 0 ? `（${details.join("，")}）` : "";
      return `- ${kind}：${source.title}${suffix}`;
    })
    .join("\n");
}

function buildSection(title: string, content: string): string {
  return `## ${title}\n\n${content.trim() || "暂无"}\n`;
}

export function buildPptUnderstandingMarkdown(
  input: BuildPptUnderstandingMarkdownInput,
): { filename: string; content: string } {
  const exportedAt = formatTimestamp(input.exportedAt);
  const filename = `${sanitizeFilenamePart(input.title)}-ppt-understanding.md`;
  const modeLabel = input.generationMode === "agent" ? "智能生成" : "模板生成";
  const materialModeLabel =
    input.materialInputMode === "internal" ? "软件内素材" : "手动输入";
  const staleBanner = input.stale
    ? "> 警告：导出时素材已变更，以下 AI 理解结果可能不是当前素材的最新版本。\n\n"
    : "";

  const content = [
    `# ${input.title || "PPT 需求理解"}`,
    "",
    staleBanner.trimEnd(),
    "| 字段 | 内容 |",
    "| --- | --- |",
    `| 导出时间 | ${exportedAt} |`,
    `| 汇报对象 | ${input.audience} |`,
    `| 页数 | ${input.pageCount} |`,
    `| 风格 | ${input.style} |`,
    `| 生成模式 | ${modeLabel} |`,
    `| 素材输入方式 | ${materialModeLabel} |`,
    "",
    "## 素材来源",
    "",
    buildSourceList(input.materialSources),
    "",
    buildSection("AI 理解摘要", input.understandingDraft.understandingSummary),
    buildSection("关键信息优先级", input.understandingDraft.keyPriorities),
    buildSection("叙事主线", input.understandingDraft.narrativeMainline),
    buildSection("建议页面结构", input.understandingDraft.suggestedPageStructure),
    buildSection("视觉表达建议", input.understandingDraft.visualExpressionAdvice),
    buildSection("待确认问题", input.understandingDraft.openQuestions),
  ]
    .filter(Boolean)
    .join("\n");

  return { filename, content: `${content}\n` };
}
