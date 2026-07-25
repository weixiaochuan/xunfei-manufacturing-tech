import mammoth from "mammoth";
import { noteApi, sourceFileApi } from "@/lib/api";
import type { PdfImportResult } from "@/types";

function bytesFromBase64(value: string) {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

function getFileStem(path: string) {
  const name = path.split(/[\\/]/).pop() ?? path;
  return name.replace(/\.[^.]+$/, "") || "导入的 Word 文档";
}

export async function importWordFiles(
  paths: string[],
  folderId?: number | null,
): Promise<PdfImportResult[]> {
  const results: PdfImportResult[] = [];

  for (const path of paths) {
    try {
      const lower = path.toLowerCase();
      const isDoc = lower.endsWith(".doc") && !lower.endsWith(".docx");
      const base64 = isDoc
        ? await sourceFileApi.convertDocToDocxBase64(path)
        : await sourceFileApi.readFileAsBase64(path);
      const buffer = bytesFromBase64(base64).buffer;
      const extracted = await mammoth.extractRawText({ arrayBuffer: buffer });
      const note = await noteApi.create({
        title: getFileStem(path),
        content: extracted.value.trim(),
        folder_id: folderId ?? null,
      });
      await sourceFileApi.attachSourceFile(note.id, path, isDoc ? "doc" : "docx");

      results.push({
        sourcePath: path,
        noteId: note.id,
        title: note.title,
        error: null,
      });
    } catch (error) {
      results.push({
        sourcePath: path,
        noteId: null,
        title: null,
        error: String(error),
      });
    }
  }

  return results;
}
