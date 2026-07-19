import type { ResolvedPptMaterialSource } from "@/types";

const HTML_ENTITIES: Record<string, string> = {
  "&nbsp;": " ",
  "&amp;": "&",
  "&lt;": "<",
  "&gt;": ">",
  "&quot;": '"',
  "&#39;": "'",
};

/** Keep Markdown structure useful to the planner while removing editor/HTML noise. */
export function normalizePptMaterialText(content: string): string {
  if (!content) return "";

  let text = content.replace(/\r\n?/g, "\n").replace(/[\u200B-\u200D\uFEFF]/g, "");
  text = text
    .replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, "")
    .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, "")
    .replace(/<(?:br|\/p|\/div|\/li|\/blockquote|\/h[1-6])\s*\/?>/gi, "\n")
    .replace(/<li\b[^>]*>/gi, "- ")
    .replace(/<blockquote\b[^>]*>/gi, "> ")
    .replace(/<[^>]+>/g, "")
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, (_match, alt: string) =>
      alt.trim() ? `[图片：${alt.trim()}]` : "",
    )
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1");

  for (const [entity, value] of Object.entries(HTML_ENTITIES)) {
    text = text.replace(new RegExp(entity, "gi"), value);
  }

  return text
    .split("\n")
    .map((line) => line.replace(/[\t ]+/g, " ").trimEnd())
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

export function mergePptMaterialSources(sources: ResolvedPptMaterialSource[]): string {
  return sources
    .map((source, index) => {
      const typeLabel = source.sourceType === "diary" ? "日记" : "文档";
      return `# 来源 ${index + 1}：${typeLabel}｜${source.title}\n\n${source.plainText}`;
    })
    .join("\n\n---\n\n")
    .trim();
}

export function hasSubstantivePptMaterial(text: string): boolean {
  const body = text
    .replace(/^#\s*来源\s*\d+：.*$/gm, "")
    .replace(/^---$/gm, "")
    .replace(/[\s#>*_`\-|：:，。！？、,.!?()[\]{}]/g, "");
  return body.length > 0;
}
