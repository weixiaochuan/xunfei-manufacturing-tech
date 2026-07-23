export interface CreateMarkdownInput {
  title?: unknown;
  markdownContent?: unknown;
  folderId?: unknown;
  diaryDate?: unknown;
  isPinned?: unknown;
  isHidden?: unknown;
  sortOrder?: unknown;
  tagIds?: unknown;
}

export interface NormalizedCreateMarkdownInput {
  title: string;
  markdownContent: string;
  folderId: string | null;
  diaryDate: string | null;
  isPinned: boolean;
  isHidden: boolean;
  sortOrder: number;
  tagIds: string[];
}

export function normalizeCreateMarkdownInput(
  input: CreateMarkdownInput,
): NormalizedCreateMarkdownInput {
  const title = typeof input.title === "string" ? input.title.trim() : "";
  if (input.markdownContent !== undefined && typeof input.markdownContent !== "string") {
    throw { code: "markdownContentInvalid" };
  }
  if (input.folderId !== undefined && input.folderId !== null && typeof input.folderId !== "string") {
    throw { code: "folderInvalid" };
  }
  if (input.tagIds !== undefined && !Array.isArray(input.tagIds)) {
    throw { code: "tagsInvalid" };
  }
  if (Array.isArray(input.tagIds) && input.tagIds.some((value) => typeof value !== "string")) {
    throw { code: "tagsInvalid" };
  }

  return {
    title: title || "未命名文档",
    markdownContent: input.markdownContent ?? "",
    folderId: typeof input.folderId === "string" ? input.folderId : null,
    diaryDate:
      typeof input.diaryDate === "string" && input.diaryDate.length > 0
        ? input.diaryDate
        : null,
    isPinned: typeof input.isPinned === "boolean" ? input.isPinned : false,
    isHidden: typeof input.isHidden === "boolean" ? input.isHidden : false,
    sortOrder:
      typeof input.sortOrder === "number" && Number.isSafeInteger(input.sortOrder)
        ? input.sortOrder
        : 0,
    tagIds: Array.isArray(input.tagIds) ? [...new Set(input.tagIds as string[])] : [],
  };
}

export function safeCreateMarkdownShape(input: NormalizedCreateMarkdownInput) {
  return {
    title: typeof input.title,
    markdownContent: typeof input.markdownContent,
    folderId: input.folderId === null ? "null" : typeof input.folderId,
    diaryDate: input.diaryDate === null ? "null" : typeof input.diaryDate,
    isPinned: typeof input.isPinned,
    isHidden: typeof input.isHidden,
    sortOrder: typeof input.sortOrder,
    tagIds: `array(${input.tagIds.length})`,
  };
}
