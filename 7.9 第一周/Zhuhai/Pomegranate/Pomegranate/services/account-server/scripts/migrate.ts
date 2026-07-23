import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import type { PoolClient } from "pg";
import { getSafeErrorMessage, loadConfig } from "../src/config.js";
import { closeDatabasePool, createDatabasePool } from "../src/db.js";

const migrationsDirectory = fileURLToPath(new URL("../../migrations", import.meta.url));
const migrationFilePattern = /^\d{3,}-[a-z0-9-]+\.sql$/;

interface AppliedMigration {
  checksum: string;
}

async function ensureMigrationTable(client: PoolClient): Promise<void> {
  await client.query(`
    CREATE TABLE IF NOT EXISTS schema_migrations (
      version TEXT PRIMARY KEY,
      checksum TEXT NOT NULL,
      applied_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
    )
  `);
}

async function applyMigration(client: PoolClient, filename: string): Promise<"applied" | "skipped"> {
  const sql = await readFile(`${migrationsDirectory}/${filename}`, "utf8");
  const checksum = createHash("sha256").update(sql).digest("hex");
  const existing = await client.query<AppliedMigration>(
    "SELECT checksum FROM schema_migrations WHERE version = $1",
    [filename],
  );

  if (existing.rowCount === 1) {
    if (existing.rows[0]?.checksum !== checksum) {
      throw new Error(`已执行的 migration 内容发生变化：${filename}。请新增 migration，不要修改历史文件`);
    }
    return "skipped";
  }

  await client.query(sql);
  await client.query(
    "INSERT INTO schema_migrations (version, checksum) VALUES ($1, $2)",
    [filename, checksum],
  );
  return "applied";
}

async function migrate(): Promise<void> {
  const config = loadConfig();
  const pool = createDatabasePool(config.database);
  let client: PoolClient | undefined;

  try {
    client = await pool.connect();
    await client.query("BEGIN");
    await client.query("SELECT pg_advisory_xact_lock($1)", [1_984_061_805]);
    await ensureMigrationTable(client);

    const filenames = (await readdir(migrationsDirectory))
      .filter((filename) => migrationFilePattern.test(filename))
      .sort();

    if (filenames.length === 0) {
      throw new Error("未找到 SQL migration 文件");
    }

    for (const filename of filenames) {
      const result = await applyMigration(client, filename);
      console.info(`${result === "applied" ? "已执行" : "已跳过"} migration：${filename}`);
    }

    await client.query("COMMIT");
    console.info("数据库 migration 完成");
  } catch (error) {
    if (client) {
      await client.query("ROLLBACK").catch(() => undefined);
    }
    if (error instanceof Error && error.message.startsWith("已执行的 migration")) {
      throw error;
    }
    if (error instanceof Error && error.message === "未找到 SQL migration 文件") {
      throw error;
    }
    throw new Error(getSafeErrorMessage(error));
  } finally {
    client?.release();
    await closeDatabasePool(pool);
  }
}

migrate().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : "未知错误";
  console.error(`数据库 migration 失败：${message}`);
  process.exitCode = 1;
});
