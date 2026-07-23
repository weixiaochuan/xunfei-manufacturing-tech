import { createHash, randomBytes } from "node:crypto";
import { constants } from "node:fs";
import { access, lstat, mkdir, open, realpath, rename, unlink } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";
import type { Readable } from "node:stream";
import {
  FileStorageLimitError,
  type FileStorage,
  type FileVerification,
  type StoredFileWrite,
} from "./file-storage.js";

const STORAGE_KEY_PATTERN = /^[a-f0-9]{64}$/;

function isMissing(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error && error.code === "ENOENT";
}

async function digestFile(path: string): Promise<{ sizeBytes: number; sha256: string }> {
  const handle = await open(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  const digest = createHash("sha256");
  let sizeBytes = 0;
  try {
    for await (const chunk of handle.createReadStream({ autoClose: false })) {
      const value = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      sizeBytes += value.length;
      digest.update(value);
    }
  } finally {
    await handle.close();
  }
  return { sizeBytes, sha256: digest.digest("hex") };
}

export class LocalFilesystemStorage implements FileStorage {
  readonly backend = "filesystem" as const;
  readonly root: string;
  private initializedRoot: string | null = null;

  constructor(root: string) {
    this.root = resolve(root);
  }

  async initialize(): Promise<void> {
    await mkdir(this.root, { recursive: true });
    const rootStat = await lstat(this.root);
    if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) {
      throw new Error("file_storage_root_invalid");
    }
    const canonical = await realpath(this.root);
    const probe = resolve(this.root, `.storage-probe-${randomBytes(12).toString("hex")}`);
    const handle = await open(probe, "wx");
    try {
      await handle.write(Buffer.from("ok"));
      await handle.sync();
    } finally {
      await handle.close();
      await unlink(probe).catch(() => undefined);
    }
    await access(this.root, constants.R_OK | constants.W_OK);
    this.initializedRoot = canonical;
  }

  resolveStorageKey(storageKey: string): string {
    if (!STORAGE_KEY_PATTERN.test(storageKey)) {
      throw new Error("invalid_storage_key");
    }
    const candidate = resolve(this.root, storageKey);
    const relativePath = relative(this.root, candidate);
    if (relativePath !== storageKey || isAbsolute(relativePath) || relativePath.startsWith("..")) {
      throw new Error("invalid_storage_path");
    }
    return candidate;
  }

  private assertInitialized(): void {
    if (this.initializedRoot === null) throw new Error("file_storage_not_initialized");
  }

  async writeFile(
    content: AsyncIterable<Buffer | string> | Iterable<Buffer | string>,
    maxBytes: number,
  ): Promise<StoredFileWrite> {
    this.assertInitialized();
    const storageKey = randomBytes(32).toString("hex");
    const temporaryKey = randomBytes(32).toString("hex");
    const finalPath = this.resolveStorageKey(storageKey);
    const temporaryPath = this.resolveStorageKey(temporaryKey);
    const digest = createHash("sha256");
    let sizeBytes = 0;
    let handle;
    try {
      handle = await open(temporaryPath, "wx");
      for await (const value of content) {
        const chunk = Buffer.isBuffer(value) ? value : Buffer.from(value);
        sizeBytes += chunk.length;
        if (sizeBytes > maxBytes) throw new FileStorageLimitError();
        digest.update(chunk);
        await handle.write(chunk);
      }
      await handle.sync();
      await handle.close();
      handle = undefined;
      await rename(temporaryPath, finalPath);
      return { storageKey, sizeBytes, sha256: digest.digest("hex") };
    } catch (error) {
      await handle?.close().catch(() => undefined);
      await unlink(temporaryPath).catch(() => undefined);
      throw error;
    }
  }

  async createReadStream(storageKey: string): Promise<Readable> {
    this.assertInitialized();
    const path = this.resolveStorageKey(storageKey);
    const stat = await lstat(path);
    if (!stat.isFile() || stat.isSymbolicLink()) throw new Error("stored_file_invalid");
    const handle = await open(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    return handle.createReadStream({ autoClose: true });
  }

  async fileExists(storageKey: string): Promise<boolean> {
    this.assertInitialized();
    try {
      const stat = await lstat(this.resolveStorageKey(storageKey));
      return stat.isFile() && !stat.isSymbolicLink();
    } catch (error) {
      if (isMissing(error)) return false;
      throw error;
    }
  }

  async deleteFile(storageKey: string): Promise<boolean> {
    this.assertInitialized();
    try {
      await unlink(this.resolveStorageKey(storageKey));
      return true;
    } catch (error) {
      if (isMissing(error)) return false;
      throw error;
    }
  }

  async verifyFile(storageKey: string, sizeBytes: number, sha256: string): Promise<FileVerification> {
    this.assertInitialized();
    const path = this.resolveStorageKey(storageKey);
    if (!(await this.fileExists(storageKey))) {
      return { exists: false, sizeMatches: false, sha256Matches: false, actualSizeBytes: null, actualSha256: null };
    }
    const actual = await digestFile(path);
    return {
      exists: true,
      sizeMatches: actual.sizeBytes === sizeBytes,
      sha256Matches: actual.sha256 === sha256,
      actualSizeBytes: actual.sizeBytes,
      actualSha256: actual.sha256,
    };
  }
}

export async function copyVerifiedFile(
  source: LocalFilesystemStorage,
  target: LocalFilesystemStorage,
  storageKey: string,
  expectedSize: number,
  expectedSha256: string,
): Promise<"copied" | "skipped"> {
  const existing = await target.verifyFile(storageKey, expectedSize, expectedSha256);
  if (existing.exists) {
    if (existing.sizeMatches && existing.sha256Matches) return "skipped";
    throw new Error("target_storage_conflict");
  }
  const sourceVerification = await source.verifyFile(storageKey, expectedSize, expectedSha256);
  if (!sourceVerification.exists || !sourceVerification.sizeMatches || !sourceVerification.sha256Matches) {
    throw new Error("source_storage_verification_failed");
  }
  const sourcePath = source.resolveStorageKey(storageKey);
  const targetPath = target.resolveStorageKey(storageKey);
  const temporaryPath = `${targetPath}.copy-${randomBytes(12).toString("hex")}`;
  const sourceHandle = await open(sourcePath, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  const targetHandle = await open(temporaryPath, "wx");
  try {
    for await (const chunk of sourceHandle.createReadStream({ autoClose: false })) {
      await targetHandle.write(chunk);
    }
    await targetHandle.sync();
  } finally {
    await sourceHandle.close();
    await targetHandle.close();
  }
  try {
    await rename(temporaryPath, targetPath);
  } catch (error) {
    await unlink(temporaryPath).catch(() => undefined);
    throw error;
  }
  const verification = await target.verifyFile(storageKey, expectedSize, expectedSha256);
  if (!verification.sizeMatches || !verification.sha256Matches) throw new Error("target_storage_verification_failed");
  return "copied";
}
