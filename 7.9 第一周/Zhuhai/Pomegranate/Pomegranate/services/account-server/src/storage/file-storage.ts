import type { Readable } from "node:stream";

export interface StoredFileWrite {
  storageKey: string;
  sizeBytes: number;
  sha256: string;
}

export interface FileVerification {
  exists: boolean;
  sizeMatches: boolean;
  sha256Matches: boolean;
  actualSizeBytes: number | null;
  actualSha256: string | null;
}

export interface FileStorage {
  readonly backend: "filesystem";
  readonly root: string;
  initialize(): Promise<void>;
  resolveStorageKey(storageKey: string): string;
  writeFile(
    content: AsyncIterable<Buffer | string> | Iterable<Buffer | string>,
    maxBytes: number,
  ): Promise<StoredFileWrite>;
  createReadStream(storageKey: string): Promise<Readable>;
  fileExists(storageKey: string): Promise<boolean>;
  deleteFile(storageKey: string): Promise<boolean>;
  verifyFile(storageKey: string, sizeBytes: number, sha256: string): Promise<FileVerification>;
}

export class FileStorageLimitError extends Error {}
