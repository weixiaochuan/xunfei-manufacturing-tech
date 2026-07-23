import { config as loadDotEnv } from "dotenv";
import { posix, win32 } from "node:path";
import { fileURLToPath } from "node:url";

const serviceEnvPath = fileURLToPath(new URL("../../.env", import.meta.url));
loadDotEnv({ path: serviceEnvPath, quiet: true });

type NodeEnvironment = "development" | "test" | "production";
export type DeploymentProfile = "local" | "lan" | "cloud";

export interface OidcConfig {
  baseUrl: string;
  clientId: string;
  clientSecret: string;
  redirectUri: string;
  organization: "pomegranate";
  application: "app-pomegranate";
}

export interface AccountServerConfig {
  deploymentProfile: DeploymentProfile;
  server: {
    host: string;
    port: number;
    publicUrl: string;
  };
  database: {
    host: string;
    port: number;
    database: string;
    user: string;
    password: string;
    connectionTimeoutMillis: number;
  };
  oidc: OidcConfig;
  session: {
    ttlSeconds: number;
  };
  userFiles: {
    backend: "filesystem";
    root: string;
    maxBytes: number;
  };
  nodeEnv: NodeEnvironment;
}

function readRequired(name: string): string {
  const value = process.env[name]?.trim();
  if (!value) {
    throw new Error(`缺少必需环境变量：${name}`);
  }
  return value;
}

function readPort(name: string): number {
  const rawValue = readRequired(name);
  if (!/^\d+$/.test(rawValue)) {
    throw new Error(`环境变量 ${name} 必须是 1 到 65535 之间的整数`);
  }

  const port = Number(rawValue);
  if (!Number.isSafeInteger(port) || port < 1 || port > 65_535) {
    throw new Error(`环境变量 ${name} 必须是 1 到 65535 之间的整数`);
  }
  return port;
}

function readPositiveInteger(name: string): number {
  const rawValue = readRequired(name);
  if (!/^\d+$/.test(rawValue)) {
    throw new Error(`环境变量 ${name} 必须是正整数`);
  }

  const value = Number(rawValue);
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new Error(`环境变量 ${name} 必须是正整数`);
  }
  return value;
}

export interface StoragePathContext {
  platform: "win32" | "linux";
  repositoryRoot: string;
  systemRoot?: string;
}

export interface StoragePathOptions {
  legacyRollbackRoot?: string;
}

function isSameOrWithin(parent: string, candidate: string, pathApi: typeof win32 | typeof posix): boolean {
  const value = pathApi.relative(parent, candidate);
  return value === "" || (!value.startsWith("..") && !pathApi.isAbsolute(value));
}

export function resolveFileStorageRoot(
  configured: string,
  context?: StoragePathContext,
  options?: StoragePathOptions,
): string {
  const platform = context?.platform ?? (process.platform === "win32" ? "win32" : "linux");
  const pathApi = platform === "win32" ? win32 : posix;
  if (!pathApi.isAbsolute(configured)) {
    throw new Error("环境变量 USER_FILES_ROOT 必须是绝对路径");
  }
  const serviceRoot = fileURLToPath(new URL("../../", import.meta.url));
  const repositoryRoot = pathApi.resolve(context?.repositoryRoot ?? pathApi.resolve(serviceRoot, "../.."));
  const normalized = pathApi.resolve(configured);
  const legacyRollbackRoot = options?.legacyRollbackRoot
    ? pathApi.resolve(options.legacyRollbackRoot)
    : undefined;
  const isExplicitLegacyRollback = legacyRollbackRoot !== undefined && normalized === legacyRollbackRoot;
  if (
    normalized === pathApi.parse(normalized).root ||
    (isSameOrWithin(repositoryRoot, normalized, pathApi) && !isExplicitLegacyRollback)
  ) {
    throw new Error("环境变量 USER_FILES_ROOT 不得指向项目源码目录");
  }
  const segments = normalized.toLowerCase().split(/[\\/]+/);
  if (segments.includes("node_modules") || segments.includes("src") || segments.includes("src-tauri")) {
    throw new Error("环境变量 USER_FILES_ROOT 不得指向代码或依赖目录");
  }
  const systemRoot = context?.systemRoot ?? process.env.SystemRoot;
  if (platform === "win32" && systemRoot && isSameOrWithin(pathApi.resolve(systemRoot), normalized, pathApi)) {
    throw new Error("环境变量 USER_FILES_ROOT 不得指向 Windows 系统目录");
  }
  return normalized;
}

function readFileStorageBackend(): "filesystem" {
  return readExact("FILE_STORAGE_BACKEND", "filesystem");
}

function readNodeEnvironment(): NodeEnvironment {
  const value = readRequired("NODE_ENV");
  if (value !== "development" && value !== "test" && value !== "production") {
    throw new Error("环境变量 NODE_ENV 必须是 development、test 或 production");
  }
  return value;
}

function readExact<T extends string>(name: string, expected: T): T {
  const value = readRequired(name);
  if (value !== expected) {
    throw new Error(`环境变量 ${name} 必须是 ${expected}`);
  }
  return expected;
}

function readDeploymentProfile(): DeploymentProfile {
  const value = (process.env.DEPLOYMENT_PROFILE ?? "local").trim();
  if (value !== "local" && value !== "lan" && value !== "cloud") {
    throw new Error("环境变量 DEPLOYMENT_PROFILE 必须是 local、lan 或 cloud");
  }
  return value;
}

function readPublicUrl(name: string, profile: DeploymentProfile, fallback?: string): string {
  const value = process.env[name]?.trim() || fallback;
  if (!value) {
    throw new Error(`缺少必需环境变量：${name}`);
  }
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new Error(`环境变量 ${name} 必须是有效 URL`);
  }

  if (
    (url.pathname !== "/" && url.pathname !== "") ||
    url.username ||
    url.password ||
    url.search ||
    url.hash
  ) {
    throw new Error(`环境变量 ${name} 必须是无路径、查询参数和凭据的公开 URL`);
  }

  const isLoopback = url.hostname === "127.0.0.1" || url.hostname === "localhost";
  if (profile === "local" && (url.protocol !== "http:" || !isLoopback)) {
    throw new Error(`local 环境的 ${name} 必须使用 HTTP 回环地址`);
  }
  if (
    profile === "lan" &&
    (url.protocol !== "http:" || isLoopback || url.hostname === "0.0.0.0")
  ) {
    throw new Error(`lan 环境的 ${name} 必须使用可由其他设备访问的 HTTP 地址`);
  }
  if (profile === "cloud" && url.protocol !== "https:") {
    throw new Error(`cloud 环境的 ${name} 必须使用 HTTPS`);
  }
  return url.origin;
}

function readRedirectUri(accountPublicUrl: string): string {
  const value = readRequired("CASDOOR_REDIRECT_URI");
  const expected = `${accountPublicUrl}/auth/callback`;
  if (value !== expected) {
    throw new Error(`环境变量 CASDOOR_REDIRECT_URI 必须与 Account Server 公开回调地址一致`);
  }
  return value;
}

export function loadConfig(): AccountServerConfig {
  const nodeEnv = readNodeEnvironment();
  const deploymentProfile = readDeploymentProfile();
  const host = readRequired("ACCOUNT_SERVER_HOST");
  if (deploymentProfile === "local" && host !== "127.0.0.1") {
    throw new Error("local 环境的 ACCOUNT_SERVER_HOST 必须是 127.0.0.1");
  }
  if (deploymentProfile === "lan" && host !== "0.0.0.0") {
    throw new Error("lan 环境的 ACCOUNT_SERVER_HOST 必须是 0.0.0.0");
  }

  const serverPort = readPort("ACCOUNT_SERVER_PORT");
  const localAccountFallback = deploymentProfile === "local" ? `http://127.0.0.1:${serverPort}` : undefined;
  const localCasdoorFallback = deploymentProfile === "local" ? process.env.CASDOOR_BASE_URL?.trim() : undefined;
  const accountPublicUrl = readPublicUrl("ACCOUNT_SERVER_PUBLIC_URL", deploymentProfile, localAccountFallback);
  const casdoorPublicUrl = readPublicUrl("CASDOOR_PUBLIC_URL", deploymentProfile, localCasdoorFallback);

  return {
    deploymentProfile,
    server: {
      host,
      port: serverPort,
      publicUrl: accountPublicUrl,
    },
    database: {
      host: readRequired("ACCOUNT_DB_HOST"),
      port: readPort("ACCOUNT_DB_PORT"),
      database: readRequired("ACCOUNT_DB_NAME"),
      user: readRequired("ACCOUNT_DB_USER"),
      password: readRequired("ACCOUNT_DB_PASSWORD"),
      connectionTimeoutMillis: 5_000,
    },
    oidc: {
      baseUrl: casdoorPublicUrl,
      clientId: readRequired("CASDOOR_CLIENT_ID"),
      clientSecret: readRequired("CASDOOR_CLIENT_SECRET"),
      redirectUri: readRedirectUri(accountPublicUrl),
      organization: readExact("CASDOOR_ORGANIZATION", "pomegranate"),
      application: readExact("CASDOOR_APPLICATION", "app-pomegranate"),
    },
    session: {
      ttlSeconds: 7 * 24 * 60 * 60,
    },
    userFiles: {
      backend: readFileStorageBackend(),
      root: resolveFileStorageRoot(
        readRequired("USER_FILES_ROOT"),
        undefined,
        nodeEnv === "development" && process.env.FILE_STORAGE_ALLOW_LEGACY_ROLLBACK === "true"
          ? { legacyRollbackRoot: fileURLToPath(new URL("../../.data/user-files", import.meta.url)) }
          : undefined,
      ),
      maxBytes: readPositiveInteger("USER_FILE_MAX_BYTES"),
    },
    nodeEnv,
  };
}

export function getSafeErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.startsWith("file_storage_")) {
    return "文件存储目录不可安全使用，请检查 USER_FILES_ROOT 是否存在、可读写且不是符号链接";
  }
  if (typeof error === "object" && error !== null && "code" in error) {
    const code = String(error.code);
    if (code === "EACCES" || code === "EPERM" || code === "EROFS") {
      return "文件存储目录不可读写，请检查 USER_FILES_ROOT 的权限";
    }
    if (code === "ECONNREFUSED") {
      return "PostgreSQL 连接被拒绝，请确认本地数据库已启动";
    }
    if (code === "28P01") {
      return "PostgreSQL 身份验证失败，请检查本地服务环境文件中的凭据";
    }
    if (code === "3D000") {
      return "PostgreSQL 数据库不存在，请确认 ACCOUNT_DB_NAME";
    }
  }
  return "PostgreSQL 操作失败，请检查本地数据库状态与服务配置";
}
