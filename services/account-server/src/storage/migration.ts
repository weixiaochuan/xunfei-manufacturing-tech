import { mkdir, open } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { LocalFilesystemStorage, copyVerifiedFile } from "./local-filesystem-storage.js";

export interface StorageMigrationRecord {
  storageKey: string;
  sizeBytes: number;
  sha256: string;
  deleted: boolean;
}

export type StorageMigrationMode = "dry-run" | "copy" | "verify";

export interface StorageManifestItem {
  storageKey: string;
  sizeBytes: number;
  sha256: string;
  recordState: "active" | "soft-deleted";
  status: "would-copy" | "copied" | "skipped" | "verified" | "missing-deleted" | "failed";
}

export interface StorageMigrationManifest {
  version: 1;
  createdAt: string;
  mode: StorageMigrationMode;
  source: "filesystem:legacy";
  target: "filesystem:primary";
  fileCount: number;
  totalBytes: number;
  copied: number;
  skipped: number;
  verified: number;
  failed: number;
  missing: number;
  items: StorageManifestItem[];
}

export async function migrateFileStorage(options: {
  sourceRoot: string;
  targetRoot: string;
  records: StorageMigrationRecord[];
  mode: StorageMigrationMode;
}): Promise<StorageMigrationManifest> {
  const sourceRoot = resolve(options.sourceRoot);
  const targetRoot = resolve(options.targetRoot);
  if (sourceRoot.toLowerCase() === targetRoot.toLowerCase()) throw new Error("storage_roots_must_differ");
  const source = new LocalFilesystemStorage(sourceRoot);
  const target = new LocalFilesystemStorage(targetRoot);
  await source.initialize();
  await target.initialize();

  const manifest: StorageMigrationManifest = {
    version: 1,
    createdAt: new Date().toISOString(),
    mode: options.mode,
    source: "filesystem:legacy",
    target: "filesystem:primary",
    fileCount: 0,
    totalBytes: 0,
    copied: 0,
    skipped: 0,
    verified: 0,
    failed: 0,
    missing: 0,
    items: [],
  };

  for (const record of options.records) {
    const itemBase = {
      storageKey: record.storageKey,
      sizeBytes: record.sizeBytes,
      sha256: record.sha256,
      recordState: record.deleted ? "soft-deleted" as const : "active" as const,
    };
    const sourceCheck = await source.verifyFile(record.storageKey, record.sizeBytes, record.sha256);
    if (!sourceCheck.exists) {
      manifest.missing += 1;
      if (record.deleted) {
        manifest.items.push({ ...itemBase, status: "missing-deleted" });
        continue;
      }
      manifest.failed += 1;
      manifest.items.push({ ...itemBase, status: "failed" });
      continue;
    }
    if (!sourceCheck.sizeMatches || !sourceCheck.sha256Matches) {
      manifest.failed += 1;
      manifest.items.push({ ...itemBase, status: "failed" });
      continue;
    }
    manifest.fileCount += 1;
    manifest.totalBytes += record.sizeBytes;

    try {
      if (options.mode === "dry-run") {
        const targetCheck = await target.verifyFile(record.storageKey, record.sizeBytes, record.sha256);
        if (targetCheck.exists && (!targetCheck.sizeMatches || !targetCheck.sha256Matches)) {
          throw new Error("target_storage_conflict");
        }
        const status = targetCheck.exists ? "skipped" : "would-copy";
        if (status === "skipped") manifest.skipped += 1;
        manifest.items.push({ ...itemBase, status });
      } else if (options.mode === "copy") {
        const status = await copyVerifiedFile(source, target, record.storageKey, record.sizeBytes, record.sha256);
        manifest[status] += 1;
        manifest.items.push({ ...itemBase, status });
      } else {
        const targetCheck = await target.verifyFile(record.storageKey, record.sizeBytes, record.sha256);
        if (!targetCheck.exists || !targetCheck.sizeMatches || !targetCheck.sha256Matches) {
          throw new Error("target_storage_verification_failed");
        }
        manifest.verified += 1;
        manifest.items.push({ ...itemBase, status: "verified" });
      }
    } catch {
      manifest.failed += 1;
      manifest.items.push({ ...itemBase, status: "failed" });
      break;
    }
  }
  return manifest;
}

export async function writeStorageManifest(path: string, manifest: StorageMigrationManifest): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  const handle = await open(path, "wx");
  try {
    await handle.writeFile(`${JSON.stringify(manifest, null, 2)}\n`, "utf8");
    await handle.sync();
  } finally {
    await handle.close();
  }
}
