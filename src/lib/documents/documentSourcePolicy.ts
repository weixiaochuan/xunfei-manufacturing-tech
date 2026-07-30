export type DocumentSource = "account" | "local";

/** Account Server is the default; SQLite is reachable only through an explicit opt-in. */
export function resolveDocumentSource(value: string | undefined): DocumentSource {
  return value === "local" ? "local" : "account";
}
