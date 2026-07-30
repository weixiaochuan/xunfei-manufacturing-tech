export const RESEARCH_LIBRARY_FOLDER = "论文知识库";

export interface ResearchPaperIdentity {
  title: string;
  publicationYear: number;
  doi?: string | null;
}

export interface ResearchNoteIdentity {
  title: string;
  content: string;
}

export function normalizeResearchDoi(value?: string | null): string | null {
  if (!value) return null;
  let normalized = value.trim().toLowerCase();
  normalized = normalized.replace(/^https?:\/\/(?:dx\.)?doi\.org\//, "");
  normalized = normalized.replace(/^doi:\s*/, "");
  normalized = normalized.replace(/[)\].,;，。；、]+$/u, "");
  return normalized.length > 0 ? normalized : null;
}

export function normalizeResearchTitle(value: string): string {
  return value
    .normalize("NFKC")
    .toLocaleLowerCase()
    .replace(/[‘’“”"'`]/g, "")
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export function researchPaperKey(paper: ResearchPaperIdentity): string {
  const doi = normalizeResearchDoi(paper.doi);
  if (doi) return `doi:${doi}`;
  return `title:${normalizeResearchTitle(paper.title)}:${paper.publicationYear}`;
}

export function researchPaperMarker(paper: ResearchPaperIdentity): string {
  return `<!-- research-paper-key: ${researchPaperKey(paper)} -->`;
}

export function markdownField(content: string, label: string): string {
  const escaped = label.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return content.match(new RegExp(`^-\\s*${escaped}\\s*[：:]\\s*(.+)$`, "m"))?.[1]?.trim() ?? "";
}

function normalizeNoteTitle(title: string): string {
  return normalizeResearchTitle(title.replace(/^论文[｜|]\s*/u, ""));
}

export function isSameResearchPaper(
  note: ResearchNoteIdentity,
  paper: ResearchPaperIdentity,
): boolean {
  if (note.content.includes(researchPaperMarker(paper))) return true;

  const paperDoi = normalizeResearchDoi(paper.doi);
  if (paperDoi) {
    const noteDoi = normalizeResearchDoi(markdownField(note.content, "DOI"));
    if (noteDoi === paperDoi) return true;
    if (note.content.toLocaleLowerCase().includes(`doi:${paperDoi}`)) return true;
    if (note.content.toLocaleLowerCase().includes(`doi ${paperDoi}`)) return true;
  }

  const noteTitle = normalizeNoteTitle(note.title);
  const paperTitle = normalizeResearchTitle(paper.title);
  if (!noteTitle || noteTitle !== paperTitle) return false;

  const noteYear = markdownField(note.content, "发表时间") || markdownField(note.content, "年份");
  return !noteYear || noteYear.includes(String(paper.publicationYear));
}
