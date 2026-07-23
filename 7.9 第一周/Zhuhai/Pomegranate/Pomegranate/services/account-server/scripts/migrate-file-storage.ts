import { resolve } from "node:path";
import { loadConfig } from "../src/config.js";
import { closeDatabasePool, createDatabasePool } from "../src/db.js";
import {
  migrateFileStorage,
  writeStorageManifest,
  type StorageMigrationMode,
  type StorageMigrationRecord,
} from "../src/storage/migration.js";

function argument(name: string): string | undefined {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function requiredArgument(name: string): string {
  const value = argument(name)?.trim();
  if (!value) throw new Error(`缺少参数 ${name}`);
  return value;
}

function readMode(): StorageMigrationMode {
  const selected = ["--dry-run", "--copy", "--verify"].filter((name) => process.argv.includes(name));
  if (selected.length !== 1) throw new Error("必须且只能指定 --dry-run、--copy 或 --verify 之一");
  return selected[0] === "--dry-run" ? "dry-run" : selected[0] === "--copy" ? "copy" : "verify";
}

async function main(): Promise<void> {
  const mode = readMode();
  const sourceRoot = resolve(requiredArgument("--source"));
  const targetRoot = resolve(requiredArgument("--target"));
  const config = loadConfig();
  const pool = createDatabasePool(config.database);
  try {
    const result = await pool.query<{
      storage_key: string;
      size_bytes: string | number;
      sha256: string;
      deleted_at: Date | string | null;
    }>("SELECT storage_key, size_bytes, sha256, deleted_at FROM user_files ORDER BY storage_key");
    const records: StorageMigrationRecord[] = result.rows.map((row) => ({
      storageKey: row.storage_key,
      sizeBytes: Number(row.size_bytes),
      sha256: row.sha256,
      deleted: row.deleted_at !== null,
    }));
    const manifest = await migrateFileStorage({ sourceRoot, targetRoot, records, mode });
    const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
    const manifestDirectory = resolve(argument("--manifest-dir") ?? resolve(targetRoot, "..", "..", "backups", "manifests"));
    const manifestPath = resolve(manifestDirectory, `storage-${mode}-${timestamp}.json`);
    await writeStorageManifest(manifestPath, manifest);
    console.log(JSON.stringify({
      status: manifest.failed === 0 ? "ok" : "failed",
      mode,
      fileCount: manifest.fileCount,
      totalBytes: manifest.totalBytes,
      copied: manifest.copied,
      skipped: manifest.skipped,
      verified: manifest.verified,
      missing: manifest.missing,
      failed: manifest.failed,
      manifestPath,
    }));
    if (manifest.failed > 0) process.exitCode = 1;
  } finally {
    await closeDatabasePool(pool);
  }
}

void main().catch(() => {
  console.error("文件存储迁移失败；未覆盖冲突文件，也未修改数据库记录");
  process.exitCode = 1;
});
