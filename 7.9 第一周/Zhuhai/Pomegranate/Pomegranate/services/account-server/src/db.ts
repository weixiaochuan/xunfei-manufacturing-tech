import { Pool } from "pg";
import type { AccountServerConfig } from "./config.js";

export function createDatabasePool(config: AccountServerConfig["database"]): Pool {
  const pool = new Pool({
    host: config.host,
    port: config.port,
    database: config.database,
    user: config.user,
    password: config.password,
    max: 10,
    idleTimeoutMillis: 30_000,
    connectionTimeoutMillis: config.connectionTimeoutMillis,
    application_name: "pomegranate-account-server",
  });

  pool.on("error", () => {
    console.error("数据库连接池中的空闲连接发生错误");
  });

  return pool;
}

export async function checkDatabase(pool: Pool): Promise<void> {
  await pool.query("SELECT 1");
}

export async function closeDatabasePool(pool: Pool): Promise<void> {
  await pool.end();
}
