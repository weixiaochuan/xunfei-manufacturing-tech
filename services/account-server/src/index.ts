import { getSafeErrorMessage, loadConfig } from "./config.js";
import { checkDatabase, closeDatabasePool, createDatabasePool } from "./db.js";
import { buildServer } from "./server.js";
import { LocalFilesystemStorage } from "./storage/local-filesystem-storage.js";

async function main(): Promise<void> {
  const config = loadConfig();
  const pool = createDatabasePool(config.database);
  const fileStorage = new LocalFilesystemStorage(config.userFiles.root);
  const server = buildServer({ pool, config, fileStorage });
  let shuttingDown = false;

  const shutdown = async (signal: NodeJS.Signals): Promise<void> => {
    if (shuttingDown) {
      return;
    }
    shuttingDown = true;
    server.log.info({ signal }, "正在停止 Account Server");

    try {
      await server.close();
      await closeDatabasePool(pool);
      server.log.info("Account Server 已停止");
      process.exitCode = 0;
    } catch {
      console.error("Account Server 关闭失败");
      process.exitCode = 1;
    }
  };

  process.once("SIGINT", () => {
    void shutdown("SIGINT");
  });
  process.once("SIGTERM", () => {
    void shutdown("SIGTERM");
  });

  try {
    await fileStorage.initialize();
    await checkDatabase(pool);
    server.log.info("PostgreSQL 启动检查通过");
    await server.listen({
      host: config.server.host,
      port: config.server.port,
    });
    server.log.info(
      {
        profile: config.deploymentProfile,
        host: config.server.host,
        port: config.server.port,
        publicUrl: config.server.publicUrl,
      },
      "Account Server 已启动",
    );
  } catch (error) {
    console.error(`Account Server 启动失败：${getSafeErrorMessage(error)}`);
    await server.close().catch(() => undefined);
    await closeDatabasePool(pool).catch(() => undefined);
    process.exitCode = 1;
  }
}

void main().catch(() => {
  console.error("Account Server 启动失败：发生未处理错误");
  process.exitCode = 1;
});
