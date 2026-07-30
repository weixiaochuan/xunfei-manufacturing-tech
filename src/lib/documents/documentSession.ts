export interface DocumentRequestIdentity {
  accountKey: string | null;
  generation: number;
}

let accountKey: string | null = null;
let generation = 0;
const listeners = new Set<() => void>();

export function changeDocumentAccount(nextAccountKey: string | null): void {
  if (accountKey === nextAccountKey) return;
  accountKey = nextAccountKey;
  generation += 1;
  for (const listener of listeners) listener();
}

export function captureDocumentRequest(): DocumentRequestIdentity {
  return { accountKey, generation };
}

export function isCurrentDocumentRequest(identity: DocumentRequestIdentity): boolean {
  return identity.accountKey === accountKey && identity.generation === generation;
}

export function assertCurrentDocumentRequest(identity: DocumentRequestIdentity): void {
  if (!isCurrentDocumentRequest(identity)) {
    throw { code: "staleRequest", message: "账号已切换，旧文档请求已取消" };
  }
}

export function subscribeDocumentAccountReset(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getDocumentAccountKey(): string | null {
  return accountKey;
}
