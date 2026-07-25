export interface EditorTextStats {
  chars: number;
  charsNoSpace: number;
  charsNoSpaces: number;
  words: number;
  paragraphs: number;
  readMinutes: number;
}

export function calcEditorStats(input: unknown): EditorTextStats {
  const raw =
    typeof input === "string"
      ? input
      : (input as { getText?: () => string })?.getText?.() ?? "";
  const plain = raw.replace(/<[^>]*>/g, " ").trim();
  const charsNoSpaces = plain.replace(/\s/g, "").length;
  return {
    chars: plain.length,
    charsNoSpace: charsNoSpaces,
    charsNoSpaces,
    words: plain ? plain.split(/\s+/).length : 0,
    paragraphs: plain ? plain.split(/\n+/).length : 0,
    readMinutes: Math.max(1, Math.ceil(charsNoSpaces / 500)),
  };
}
