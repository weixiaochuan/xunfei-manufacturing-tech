import { resolveDocumentSource, type DocumentSource } from "./documentSourcePolicy";

export type { DocumentSource } from "./documentSourcePolicy";

/** 只有显式设置 VITE_DOCUMENT_SOURCE=local 时才读取原 SQLite。 */
export const documentSource: DocumentSource = resolveDocumentSource(
  import.meta.env.VITE_DOCUMENT_SOURCE,
);

export const isAccountDocumentSource = documentSource === "account";
