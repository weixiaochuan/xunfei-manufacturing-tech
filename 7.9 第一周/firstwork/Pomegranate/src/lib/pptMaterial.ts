import type { ResolvedPptMaterialSource } from "@/types";

function collapseBlankLines(input: string): string {
  return input.replace(/\r\n/g, "\n").replace(/\n{3,}/g, "\n\n").trim();
}

export function normalizePptMaterialText(content: string): string {
  return collapseBlankLines(content ?? "");
}

export function hasSubstantivePptMaterial(content: string): boolean {
  return normalizePptMaterialText(content).length > 0;
}

function formatSourceLabel(source: ResolvedPptMaterialSource): string {
  const kind = source.sourceType === "diary" ? "日记" : "文档";
  const meta: string[] = [];
  if (source.dailyDate) meta.push(source.dailyDate);
  if (typeof source.wordCount === "number" && source.wordCount > 0) {
    meta.push(`${source.wordCount} 字`);
  }
  return meta.length > 0
    ? `${kind}：${source.title}（${meta.join("，")}）`
    : `${kind}：${source.title}`;
}

export function mergePptMaterialSources(
  sources: ResolvedPptMaterialSource[],
): string {
  const sections = sources
    .map((source) => {
      const plainText = normalizePptMaterialText(source.plainText);
      if (!plainText) return null;
      return `## ${formatSourceLabel(source)}\n\n${plainText}`;
    })
    .filter((section): section is string => Boolean(section));

  return sections.join("\n\n---\n\n");
}
