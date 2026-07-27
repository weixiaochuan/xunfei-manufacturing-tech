import { folderApi, noteApi } from "@/lib/api";
import {
  isSameResearchPaper,
  markdownField,
  normalizeResearchDoi,
  normalizeResearchTitle,
  researchPaperKey,
  researchPaperMarker,
  RESEARCH_LIBRARY_FOLDER,
} from "@/lib/researchKnowledgeBaseCore";
import type { Folder, Note, ResearchAnalysisResult, ResearchPaper } from "@/types";

export {
  isSameResearchPaper,
  markdownField,
  normalizeResearchDoi,
  normalizeResearchTitle,
  researchPaperKey,
  researchPaperMarker,
  RESEARCH_LIBRARY_FOLDER,
};

function findFolderByName(folders: Folder[], name: string): Folder | null {
  for (const folder of folders) {
    if (folder.name.trim() === name) return folder;
    const child = findFolderByName(folder.children ?? [], name);
    if (child) return child;
  }
  return null;
}

export async function getResearchLibraryFolder(): Promise<Folder | null> {
  const folders = await folderApi.list();
  return findFolderByName(folders, RESEARCH_LIBRARY_FOLDER);
}

export async function ensureResearchLibraryFolder(): Promise<number> {
  const existing = await getResearchLibraryFolder();
  if (existing) return existing.id;
  const folderId = await folderApi.ensurePath(RESEARCH_LIBRARY_FOLDER);
  if (folderId === null) {
    throw new Error("无法创建论文知识库文件夹");
  }
  return folderId;
}

export async function listResearchLibraryNotes(folderId?: number): Promise<Note[]> {
  const resolvedFolderId = folderId ?? (await getResearchLibraryFolder())?.id;
  if (resolvedFolderId === undefined) return [];
  const result = await noteApi.list({
    folder_id: resolvedFolderId,
    include_descendants: false,
    page: 1,
    page_size: 200,
    sort_by: "created",
  });
  return result.items;
}

export function analysisKnowledgeNoteContent(
  result: ResearchAnalysisResult,
  projectContext: string,
): string {
  const paperSections = result.papers.map((paper) => {
    const evidence = paper.evidence.length > 0
      ? paper.evidence
          .map((item) => `- "${item.quote}"\n  - 位置：${item.location || "未标注"}`)
          .join("\n")
      : "- 未返回可核验证据摘录";
    return `## ${paper.paperId} · ${paper.title || paper.fileName}

- 原文件：${paper.fileName}
- 研究问题：${paper.researchQuestion || "未提取"}
- 关键词：${paper.keywords.join("、") || "未提取"}
- 方法：${paper.methods.join("；") || "未提取"}
- 数据与实验：${paper.dataAndExperiments.join("；") || "未提取"}
- 评价指标：${paper.metrics.join("；") || "未提取"}
- 主要结论：${paper.conclusions.join("；") || "未提取"}
- 创新点：${paper.innovations.join("；") || "未提取"}
- 局限：${paper.limitations.join("；") || "未提取"}

### 证据

${evidence}`;
  }).join("\n\n");

  const overlaps = result.keywordOverlaps.length > 0
    ? result.keywordOverlaps
        .map((item) => `- ${item.keyword}（${item.paperIds.join("、")}）：${item.analysis}`)
        .join("\n")
    : "- 单篇论文或未发现稳定共同关键词";

  const comparisons = result.comparisons.length > 0
    ? result.comparisons
        .map((item) => `- ${item.dimension}\n  - 共同点：${item.commonPoints.join("；") || "无"}\n  - 差异：${item.differences.join("；") || "无"}\n  - 冲突：${item.conflicts.join("；") || "无"}`)
        .join("\n")
    : "- 单篇论文分析未生成跨论文比较";

  const recommendations = result.recommendations.length > 0
    ? result.recommendations
        .map((item) => `- ${item.title}（置信度 ${Math.round(item.confidence * 100)}%）：${item.action}\n  - 依据：${item.rationale}\n  - 支持论文：${item.supportingPaperIds.join("、") || "未标注"}`)
        .join("\n")
    : "- 暂无项目建议";

  return `<!-- research-analysis -->

# 论文分析笔记

> 由用户在 AI 助研中主动保存。AI 分析仅作为研究辅助，关键结论仍需回到原文核验。

## 项目背景

${projectContext.trim() || "未填写"}

## 项目理解

${result.projectSummary}

## 逐篇论文

${paperSections}

## 共同关键词

${overlaps}

## 跨论文比较

${comparisons}

## 对当前项目的建议

${recommendations}

## 知识图谱 JSON

\`\`\`json
${JSON.stringify({ nodes: result.graphNodes, edges: result.graphEdges }, null, 2)}
\`\`\`
`;
}

export function researchAnalysisNoteTitle(result: ResearchAnalysisResult): string {
  const firstTitle = result.papers[0]?.title || result.papers[0]?.fileName || "论文分析";
  return `论文分析｜${firstTitle}${result.papers.length > 1 ? ` 等 ${result.papers.length} 篇` : ""}`;
}

export type { ResearchPaper };
