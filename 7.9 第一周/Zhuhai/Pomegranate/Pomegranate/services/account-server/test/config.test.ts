import assert from "node:assert/strict";
import test, { afterEach } from "node:test";
import {
  loadConfig,
  PUBLIC_IP_TEST_ACCOUNT_SERVER_ORIGIN,
  PUBLIC_IP_TEST_CASDOOR_ORIGIN,
  PUBLIC_IP_TEST_REDIRECT_URI,
} from "../src/config.js";

const originalEnvironment = { ...process.env };

afterEach(() => {
  for (const name of Object.keys(process.env)) {
    if (!(name in originalEnvironment)) {
      delete process.env[name];
    }
  }
  Object.assign(process.env, originalEnvironment);
});

function configureBaseEnvironment(): void {
  Object.assign(process.env, {
    DEPLOYMENT_PROFILE: "public-ip-test",
    ALLOW_INSECURE_PUBLIC_IP_TEST: "true",
    ACCOUNT_SERVER_HOST: "0.0.0.0",
    ACCOUNT_SERVER_PORT: "3010",
    ACCOUNT_SERVER_PUBLIC_URL: PUBLIC_IP_TEST_ACCOUNT_SERVER_ORIGIN,
    ACCOUNT_DB_HOST: "postgres",
    ACCOUNT_DB_PORT: "5432",
    ACCOUNT_DB_NAME: "pomegranate_account",
    ACCOUNT_DB_USER: "config_test_account",
    ACCOUNT_DB_PASSWORD: "CONFIG_TEST_ONLY_DATABASE_PASSWORD",
    NODE_ENV: "test",
    CASDOOR_PUBLIC_URL: PUBLIC_IP_TEST_CASDOOR_ORIGIN,
    CASDOOR_CLIENT_ID: "config-test-client-id",
    CASDOOR_CLIENT_SECRET: "CONFIG_TEST_ONLY_CLIENT_SECRET",
    CASDOOR_REDIRECT_URI: PUBLIC_IP_TEST_REDIRECT_URI,
    CASDOOR_ORGANIZATION: "pomegranate",
    CASDOOR_APPLICATION: "app-pomegranate",
    FILE_STORAGE_BACKEND: "filesystem",
    USER_FILES_ROOT:
      process.platform === "win32"
        ? "D:\\PomegranateServer\\public-ip-test-config-tests"
        : "/tmp/pomegranate-public-ip-test-config-tests",
    FILE_STORAGE_ALLOW_LEGACY_ROLLBACK: "false",
    USER_FILE_MAX_BYTES: "20971520",
  });
}

test("public-ip-test accepts only the approved temporary origins", () => {
  configureBaseEnvironment();

  const config = loadConfig();

  assert.equal(config.deploymentProfile, "public-ip-test");
  assert.equal(config.server.publicUrl, PUBLIC_IP_TEST_ACCOUNT_SERVER_ORIGIN);
  assert.equal(config.oidc.baseUrl, PUBLIC_IP_TEST_CASDOOR_ORIGIN);
  assert.equal(config.oidc.redirectUri, PUBLIC_IP_TEST_REDIRECT_URI);
});

test("public-ip-test requires the explicit insecure HTTP opt-in", () => {
  configureBaseEnvironment();
  process.env.ALLOW_INSECURE_PUBLIC_IP_TEST = "false";

  assert.throws(() => loadConfig(), /ALLOW_INSECURE_PUBLIC_IP_TEST=true/);
});

test("public-ip-test rejects unapproved hosts, ports, paths, queries, and credentials", () => {
  const rejectedAccountOrigins = [
    "http://localhost:8080",
    "http://127.0.0.1:8080",
    "http://0.0.0.0:8080",
    "http://10.0.0.8:8080",
    "http://172.16.0.8:8080",
    "http://192.168.1.8:8080",
    "http://169.254.1.8:8080",
    "http://100.64.0.8:8080",
    "http://192.0.2.8:8080",
    "http://82.157.119.201",
    "http://82.157.119.202:8080",
    "http://82.157.119.201:8081",
    "https://82.157.119.201:8080",
    "ftp://82.157.119.201:8080",
    "http://user:password@82.157.119.201:8080",
    "http://82.157.119.201:8080/v1",
    "http://82.157.119.201:8080?debug=true",
  ];

  for (const origin of rejectedAccountOrigins) {
    configureBaseEnvironment();
    process.env.ACCOUNT_SERVER_PUBLIC_URL = origin;
    process.env.CASDOOR_REDIRECT_URI = `${origin}/auth/callback`;
    assert.throws(() => loadConfig(), /公开 URL|批准的临时公网 HTTP 地址/);
  }

  configureBaseEnvironment();
  process.env.CASDOOR_PUBLIC_URL = "http://82.157.119.201:8001";
  assert.throws(() => loadConfig(), /批准的临时公网 HTTP 地址/);
});

test("public-ip-test rejects a callback outside the approved Account Server origin", () => {
  configureBaseEnvironment();
  process.env.CASDOOR_REDIRECT_URI = "http://82.157.119.201:8081/auth/callback";

  assert.throws(() => loadConfig(), /必须与 Account Server 公开回调地址一致/);
});

test("cloud still requires HTTPS and cannot enable the temporary insecure flag", () => {
  configureBaseEnvironment();
  Object.assign(process.env, {
    DEPLOYMENT_PROFILE: "cloud",
    ALLOW_INSECURE_PUBLIC_IP_TEST: "false",
    ACCOUNT_SERVER_PUBLIC_URL: "https://api.stargathering.com",
    CASDOOR_PUBLIC_URL: "https://auth.stargathering.com",
    CASDOOR_REDIRECT_URI: "https://api.stargathering.com/auth/callback",
  });
  assert.equal(loadConfig().deploymentProfile, "cloud");

  process.env.ACCOUNT_SERVER_PUBLIC_URL = PUBLIC_IP_TEST_ACCOUNT_SERVER_ORIGIN;
  process.env.CASDOOR_REDIRECT_URI = PUBLIC_IP_TEST_REDIRECT_URI;
  assert.throws(() => loadConfig(), /cloud 环境的 ACCOUNT_SERVER_PUBLIC_URL 必须使用 HTTPS/);

  process.env.ACCOUNT_SERVER_PUBLIC_URL = "https://api.stargathering.com";
  process.env.CASDOOR_REDIRECT_URI = "https://api.stargathering.com/auth/callback";
  process.env.ALLOW_INSECURE_PUBLIC_IP_TEST = "true";
  assert.throws(() => loadConfig(), /只能用于 public-ip-test/);
});

test("local and lan profiles keep their existing URL behavior", () => {
  configureBaseEnvironment();
  Object.assign(process.env, {
    DEPLOYMENT_PROFILE: "local",
    ALLOW_INSECURE_PUBLIC_IP_TEST: "false",
    ACCOUNT_SERVER_HOST: "127.0.0.1",
    ACCOUNT_SERVER_PUBLIC_URL: "http://127.0.0.1:3010",
    CASDOOR_PUBLIC_URL: "http://127.0.0.1:8000",
    CASDOOR_REDIRECT_URI: "http://127.0.0.1:3010/auth/callback",
  });
  assert.equal(loadConfig().deploymentProfile, "local");

  Object.assign(process.env, {
    DEPLOYMENT_PROFILE: "lan",
    ACCOUNT_SERVER_HOST: "0.0.0.0",
    ACCOUNT_SERVER_PUBLIC_URL: "http://192.168.1.10:3010",
    CASDOOR_PUBLIC_URL: "http://192.168.1.10:8000",
    CASDOOR_REDIRECT_URI: "http://192.168.1.10:3010/auth/callback",
  });
  assert.equal(loadConfig().deploymentProfile, "lan");
});
