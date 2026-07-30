import { extname } from "node:path";

export const ALLOWED_UPLOAD_EXTENSIONS = Object.freeze([
  "doc", "docx", "xls", "xlsx", "csv", "ppt", "pptx", "pdf",
  "md", "markdown", "mdx", "mdxl", "txt", "rtf", "json", "xml", "yaml", "yml",
  "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg",
] as const);

export const BLOCKED_UPLOAD_EXTENSIONS = Object.freeze([
  "exe", "msi", "dll", "bat", "cmd", "com", "scr", "ps1", "vbs", "js", "jse",
  "jar", "reg", "lnk",
] as const);

const allowed = new Set<string>(ALLOWED_UPLOAD_EXTENSIONS);
const blocked = new Set<string>(BLOCKED_UPLOAD_EXTENSIONS);

export function uploadExtension(filename: string): string | null {
  const extension = extname(filename).slice(1).toLocaleLowerCase("en-US");
  return extension || null;
}

export function isAllowedUploadFilename(filename: string): boolean {
  const extension = uploadExtension(filename);
  return extension !== null && !blocked.has(extension) && allowed.has(extension);
}
