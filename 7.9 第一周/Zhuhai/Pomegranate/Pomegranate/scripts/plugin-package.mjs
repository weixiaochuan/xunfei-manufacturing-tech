#!/usr/bin/env node
import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { inflateRawSync } from "node:zlib";
import {
  evaluateV3ClassificationContributions,
  evaluateV3RequiredPermissions,
  isV3PermissionRuntimeAllowed,
  isV3RuntimeClassificationAllowed,
  loadCapabilityRegistry,
  validateCapabilityRegistry,
  v3RequestableCapabilities,
} from "./plugin-capabilities.mjs";

const MAX_FILES = 5000;
const MAX_FILE_BYTES = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES = 512 * 1024 * 1024;
const CAPABILITY_REGISTRY = validateCapabilityRegistry(loadCapabilityRegistry());
const ALLOWED_PERMISSIONS = new Set(
  v3RequestableCapabilities(CAPABILITY_REGISTRY).map((item) => item.id),
);
export function isV3PermissionAllowed(permission) {
  return ALLOWED_PERMISSIONS.has(permission);
}
const EXECUTABLE_EXTENSIONS = new Set(["js", "mjs", "cjs", "py", "ps1", "bat", "cmd", "exe", "dll", "so", "dylib"]);
const PRIVATE_EXTENSIONS = new Set(["pem", "p12", "pfx", "key"]);
const TEXT_EXTENSIONS = new Set(["json", "md", "markdown", "txt", "yaml", "yml", "toml", "xml", "csv"]);

function fail(message) { throw new Error(message); }
function sha256(data) { return createHash("sha256").update(data).digest("hex"); }
function safePath(name) {
  const normalized = name.replaceAll("\\", "/");
  if (!normalized || normalized.startsWith("/") || /^[A-Za-z]:/.test(normalized)
    || normalized.split("/").some((part) => part === ".." || part === "")) {
    fail(`非法包内路径：${name}`);
  }
  return normalized;
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let value = n;
  for (let i = 0; i < 8; i += 1) value = (value & 1) ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
  return value >>> 0;
});
function crc32(buffer) {
  let value = 0xffffffff;
  for (const byte of buffer) value = CRC_TABLE[(value ^ byte) & 0xff] ^ (value >>> 8);
  return (value ^ 0xffffffff) >>> 0;
}

function collectFiles(root) {
  const files = new Map();
  function walk(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const absolute = join(directory, entry.name);
      const stat = lstatSync(absolute);
      if (stat.isSymbolicLink()) fail(`不允许符号链接：${absolute}`);
      if (entry.isDirectory()) walk(absolute);
      else if (entry.isFile()) {
        const name = safePath(relative(root, absolute));
        const data = readFileSync(absolute);
        if (data.length > MAX_FILE_BYTES) fail(`单文件超过 64 MiB：${name}`);
        files.set(name, data);
      }
    }
  }
  walk(root);
  if (files.size === 0 || files.size > MAX_FILES) fail(`文件数必须在 1-${MAX_FILES} 之间`);
  const total = [...files.values()].reduce((sum, data) => sum + data.length, 0);
  if (total > MAX_TOTAL_BYTES) fail("未压缩总大小超过 512 MiB");
  return files;
}

function containsLikelySecret(text) {
  if (/-----BEGIN (?:RSA )?PRIVATE KEY-----/i.test(text) || /authorization\s*:\s*bearer\s+\S+/i.test(text)) {
    return true;
  }
  const assignment = /(?:api[_-]?key|api[_-]?secret|access[_-]?token|refresh[_-]?token|bearer[_-]?token|client[_-]?secret)\s*[=:]\s*["']?([^\s,"'}]+)/gi;
  for (const match of text.matchAll(assignment)) {
    const value = match[1] ?? "";
    if (value.length < 8 || /^(?:\$\{|your_|replace_|example|placeholder|\*\*\*|<)/i.test(value)) continue;
    return true;
  }
  return false;
}

function scanPackageFiles(files) {
  for (const [name, data] of files) {
    const lower = name.toLowerCase();
    const base = lower.split("/").at(-1) ?? lower;
    const extension = base.includes(".") ? base.split(".").at(-1) : "";
    if (base === ".env" || base.startsWith(".env.") || base === "id_rsa" || base === "id_ed25519"
      || PRIVATE_EXTENSIONS.has(extension)) {
      fail(`插件包包含禁止分发的凭据文件：${name}`);
    }
    if (EXECUTABLE_EXTENSIONS.has(extension)) {
      fail(`正式插件包不允许携带可执行脚本或二进制：${name}`);
    }
    if (TEXT_EXTENSIONS.has(extension) && data.length <= 1024 * 1024
      && containsLikelySecret(data.toString("utf8"))) {
      fail(`插件包资源疑似包含 API Key、Secret、Token 或 Authorization 明文：${name}`);
    }
  }
}

function isForbiddenSecretFieldName(value) {
  const normalized = String(value ?? "").replaceAll(/[^A-Za-z0-9]/g, "").toLowerCase();
  return new Set([
    "apikey", "apisecret", "accesstoken", "refreshtoken", "bearertoken",
    "authorization", "clientsecret", "password",
  ]).has(normalized);
}

function findForbiddenSecretField(value, path = "manifest") {
  if (Array.isArray(value)) {
    for (let index = 0; index < value.length; index += 1) {
      const found = findForbiddenSecretField(value[index], `${path}[${index}]`);
      if (found) return found;
    }
    return null;
  }
  if (!value || typeof value !== "object") return null;
  for (const [key, child] of Object.entries(value)) {
    if (isForbiddenSecretFieldName(key)) return `${path}.${key}`;
    const found = findForbiddenSecretField(child, `${path}.${key}`);
    if (found) return found;
  }
  return null;
}

function validateUiSchema(name, data) {
  let schema;
  try { schema = JSON.parse(data.toString("utf8")); }
  catch (error) { fail(`${name} 不是合法 JSON：${error.message}`); }
  const forbidden = findForbiddenSecretField(schema, name);
  if (forbidden) fail(`uiSchema 不得声明密钥字段：${forbidden}`);
  if (!Array.isArray(schema.fields)) fail(`${name}.fields 必须是数组`);
  if (schema.fields.length > 100) fail(`${name} 字段数量不能超过 100`);
  for (const [index, field] of schema.fields.entries()) {
    const key = field?.key ?? field?.id;
    if (typeof key !== "string" || !/^[A-Za-z_][A-Za-z0-9_.-]*$/.test(key)) {
      fail(`${name}.fields[${index}] 缺少合法 key`);
    }
    if (field.sensitive === true || isForbiddenSecretFieldName(key)) {
      fail(`${name}.fields[${index}] 不得收集凭据明文，请使用 credentialId`);
    }
  }
}

export function validateManifest(manifest, files) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) fail("manifest.json 必须是对象");
  if (manifest.schemaVersion !== 3) fail("打包工具只接受 Manifest v3");
  if (!files.has("README.md")) fail("正式插件包必须包含 README.md");
  const forbidden = findForbiddenSecretField(manifest);
  if (forbidden) fail(`Manifest 不得声明密钥字段：${forbidden}；请使用 credentialId`);
  for (const key of ["id", "name", "version", "authorId", "classification", "runtimeKind"]) {
    if (typeof manifest[key] !== "string" || !manifest[key].trim()) fail(`Manifest 缺少 ${key}`);
  }
  if (manifest.id.length > 128 || !/^[a-z0-9][a-z0-9._-]*$/.test(manifest.id)) {
    fail("插件 ID 必须以小写字母或数字开头，长度不超过 128，且仅允许小写字母、数字、点、横线和下划线");
  }
  if (!CAPABILITY_REGISTRY.v3Policy.classifications.includes(manifest.classification)) fail("classification 非法");
  if (!CAPABILITY_REGISTRY.v3Policy.runtimeKinds.includes(manifest.runtimeKind)) {
    fail("正式插件包只允许声明式、Prompt 或受控星辰运行时");
  }
  const permissions = manifest.permissions ?? [];
  if (new Set(permissions).size !== permissions.length) fail("Manifest 权限列表包含重复项");
  for (const permission of permissions) {
    if (!ALLOWED_PERMISSIONS.has(permission)) fail(`Manifest 申请了不存在的权限：${permission}`);
    if (!isV3PermissionRuntimeAllowed(permission, manifest.runtimeKind, CAPABILITY_REGISTRY)) {
      fail(`Manifest 权限 ${permission} 不允许用于 runtimeKind ${manifest.runtimeKind}`);
    }
  }
  const contributes = manifest.contributes ?? {};
  const hasFeatures = (contributes.features ?? []).length > 0;
  const hasEnhancements = (contributes.enhancements ?? []).length > 0;
  const contributionTypes = [
    ...(hasFeatures ? ["feature"] : []),
    ...(hasEnhancements ? ["enhancement"] : []),
  ];
  const contributionDecision = evaluateV3ClassificationContributions(
    manifest.classification,
    contributionTypes,
    CAPABILITY_REGISTRY,
  );
  if (!contributionDecision.ok) {
    if (contributionDecision.code === "missing-required-contribution") {
      if (manifest.classification === "feature") fail("feature 插件必须声明 features");
      if (manifest.classification === "enhancement") fail("enhancement 插件必须声明 enhancements");
      if (manifest.classification === "hybrid") {
        fail("hybrid 插件必须同时声明 features 和 enhancements");
      }
    }
    if (contributionDecision.code === "forbidden-contribution") {
      if (manifest.classification === "feature") {
        fail("classification=feature 不得声明 enhancement contribution");
      }
      if (manifest.classification === "enhancement") {
        fail("classification=enhancement 不得声明 feature contribution");
      }
    }
    fail(`classification ${manifest.classification} 与 contribution 组合不兼容`);
  }
  if (!isV3RuntimeClassificationAllowed(
    manifest.runtimeKind,
    manifest.classification,
    CAPABILITY_REGISTRY,
  )) {
    fail(`runtimeKind ${manifest.runtimeKind} 与 classification/contribution 组合不兼容`);
  }
  const featureCapabilities = (contributes.features ?? [])
    .flatMap((feature) => feature.capabilities ?? []);
  const permissionDecision = evaluateV3RequiredPermissions({
    runtimeKind: manifest.runtimeKind,
    contributions: contributionTypes,
    featureCapabilities,
    permissions,
  }, CAPABILITY_REGISTRY);
  if (!permissionDecision.ok) {
    if (permissionDecision.code === "missing-contribution-permission"
      && permissionDecision.contribution === "enhancement") {
      fail("包含 enhancement contribution 的 Manifest 必须声明 ai.context.augment");
    }
    if (permissionDecision.code === "missing-runtime-contribution-permission"
      && permissionDecision.contribution === "feature"
      && ["xingchen-agent", "xingchen-workflow"].includes(permissionDecision.runtimeKind)) {
      fail(`Xingchen feature 缺少必需权限 ${permissionDecision.permission}`);
    }
    if (permissionDecision.code === "missing-feature-capability-permission"
      && permissionDecision.featureCapability === "file.docx.output") {
      fail("feature capability file.docx.output 必须声明 files.writeSelected");
    }
    fail(`Manifest capability 组合缺少必需权限 ${permissionDecision.permission}`);
  }
  const resources = [];
  for (const feature of contributes.features ?? []) {
    if (!feature.uiSchema) fail(`feature 贡献点 ${feature.id ?? "<unknown>"} 必须声明 uiSchema`);
    resources.push(feature.uiSchema);
    const schemaName = safePath(String(feature.uiSchema));
    const schemaData = files.get(schemaName);
    if (!schemaData) fail(`Manifest 声明的资源不存在：${schemaName}`);
    validateUiSchema(schemaName, schemaData);
    if (feature.handler && feature.handler.kind !== "declarative") {
      fail(`贡献点 ${feature.id ?? "<unknown>"} 只允许 declarative handler`);
    }
    if (feature.handler?.resource) resources.push(feature.handler.resource);
  }
  for (const group of [contributes.agents ?? [], contributes.tools ?? [], contributes.enhancements ?? []]) {
    for (const item of group) {
      if (item.handler && item.handler.kind !== "declarative") {
        fail(`贡献点 ${item.id ?? "<unknown>"} 只允许 declarative handler`);
      }
      if (item.handler?.resource) resources.push(item.handler.resource);
    }
  }
  for (const resource of resources) {
    const name = safePath(String(resource));
    if (!files.has(name)) fail(`Manifest 声明的资源不存在：${name}`);
  }
  return manifest;
}

function dosDateTime(date = new Date(2000, 0, 1)) {
  return {
    time: ((date.getHours() & 31) << 11) | ((date.getMinutes() & 63) << 5) | ((date.getSeconds() / 2) & 31),
    date: (((date.getFullYear() - 1980) & 127) << 9) | (((date.getMonth() + 1) & 15) << 5) | (date.getDate() & 31),
  };
}

function createZip(files) {
  const localParts = [];
  const centralParts = [];
  let offset = 0;
  const stamp = dosDateTime();
  for (const [name, data] of [...files.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    const nameBuffer = Buffer.from(name, "utf8");
    const crc = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(20, 4); local.writeUInt16LE(0x800, 6);
    local.writeUInt16LE(0, 8); local.writeUInt16LE(stamp.time, 10); local.writeUInt16LE(stamp.date, 12);
    local.writeUInt32LE(crc, 14); local.writeUInt32LE(data.length, 18); local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBuffer.length, 26); local.writeUInt16LE(0, 28);
    localParts.push(local, nameBuffer, data);

    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0); central.writeUInt16LE(20, 4); central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x800, 8); central.writeUInt16LE(0, 10); central.writeUInt16LE(stamp.time, 12);
    central.writeUInt16LE(stamp.date, 14); central.writeUInt32LE(crc, 16); central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(data.length, 24); central.writeUInt16LE(nameBuffer.length, 28);
    central.writeUInt16LE(0, 30); central.writeUInt16LE(0, 32); central.writeUInt16LE(0, 34);
    central.writeUInt16LE(0, 36); central.writeUInt32LE(0, 38); central.writeUInt32LE(offset, 42);
    centralParts.push(central, nameBuffer);
    offset += local.length + nameBuffer.length + data.length;
  }
  const centralBuffer = Buffer.concat(centralParts);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(0, 4); end.writeUInt16LE(0, 6);
  end.writeUInt16LE(files.size, 8); end.writeUInt16LE(files.size, 10);
  end.writeUInt32LE(centralBuffer.length, 12); end.writeUInt32LE(offset, 16); end.writeUInt16LE(0, 20);
  return Buffer.concat([...localParts, centralBuffer, end]);
}

function readZip(path) {
  const archive = readFileSync(path);
  let eocd = -1;
  for (let index = archive.length - 22; index >= Math.max(0, archive.length - 65557); index -= 1) {
    if (archive.readUInt32LE(index) === 0x06054b50) { eocd = index; break; }
  }
  if (eocd < 0) fail("不是有效 ZIP 文件");
  const count = archive.readUInt16LE(eocd + 10);
  if (count === 0 || count > MAX_FILES) fail("ZIP 文件数量异常");
  let cursor = archive.readUInt32LE(eocd + 16);
  let total = 0;
  const files = new Map();
  const seen = new Set();
  for (let index = 0; index < count; index += 1) {
    if (archive.readUInt32LE(cursor) !== 0x02014b50) fail("ZIP 中央目录损坏");
    const method = archive.readUInt16LE(cursor + 10);
    const compressedSize = archive.readUInt32LE(cursor + 20);
    const size = archive.readUInt32LE(cursor + 24);
    const nameLength = archive.readUInt16LE(cursor + 28);
    const extraLength = archive.readUInt16LE(cursor + 30);
    const commentLength = archive.readUInt16LE(cursor + 32);
    const externalAttributes = archive.readUInt32LE(cursor + 38);
    const localOffset = archive.readUInt32LE(cursor + 42);
    const name = safePath(archive.subarray(cursor + 46, cursor + 46 + nameLength).toString("utf8"));
    if (((externalAttributes >>> 16) & 0o170000) === 0o120000) fail(`不允许符号链接：${name}`);
    if (seen.has(name.toLowerCase())) fail(`重复文件名：${name}`);
    seen.add(name.toLowerCase());
    if (size > MAX_FILE_BYTES) fail(`单文件超过限制：${name}`);
    total += size;
    if (total > MAX_TOTAL_BYTES) fail("ZIP 展开大小超过限制");
    if (archive.readUInt32LE(localOffset) !== 0x04034b50) fail("ZIP 本地文件头损坏");
    const localNameLength = archive.readUInt16LE(localOffset + 26);
    const localExtraLength = archive.readUInt16LE(localOffset + 28);
    const start = localOffset + 30 + localNameLength + localExtraLength;
    const compressed = archive.subarray(start, start + compressedSize);
    const data = method === 0 ? compressed : method === 8 ? inflateRawSync(compressed) : fail(`不支持的 ZIP 压缩算法：${method}`);
    if (data.length !== size) fail(`文件长度不匹配：${name}`);
    files.set(name, data);
    cursor += 46 + nameLength + extraLength + commentLength;
  }
  return files;
}

function pack(directory, outputArgument) {
  const root = resolve(directory);
  if (!existsSync(root) || !lstatSync(root).isDirectory()) fail("插件目录不存在");
  const files = collectFiles(root);
  scanPackageFiles(files);
  if (!files.has("manifest.json")) fail("manifest.json 必须位于插件目录根层");
  const manifest = validateManifest(JSON.parse(files.get("manifest.json").toString("utf8")), files);
  files.delete("checksums.json");
  const checksums = Object.fromEntries([...files.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([name, data]) => [name, sha256(data)]));
  files.set("checksums.json", Buffer.from(`${JSON.stringify({ algorithm: "sha256", files: checksums }, null, 2)}\n`));
  const output = resolve(outputArgument ?? join(dirname(root), `${manifest.id}-${manifest.version}.firstwork-plugin`));
  writeFileSync(output, createZip(files));
  console.log(`已打包：${output}`);
  verify(output);
}

function verify(path) {
  const absolute = resolve(path);
  if (!absolute.endsWith(".firstwork-plugin")) fail("文件扩展名必须是 .firstwork-plugin");
  const files = readZip(absolute);
  scanPackageFiles(files);
  if (!files.has("manifest.json")) fail("manifest.json 必须位于压缩包根层");
  const manifest = validateManifest(JSON.parse(files.get("manifest.json").toString("utf8")), files);
  const checksumBuffer = files.get("checksums.json");
  if (!checksumBuffer) fail("缺少 checksums.json");
  const checksums = JSON.parse(checksumBuffer.toString("utf8"));
  if (checksums.algorithm !== "sha256" || !checksums.files || typeof checksums.files !== "object") fail("checksums.json 格式错误");
  for (const [name, expected] of Object.entries(checksums.files)) {
    const data = files.get(safePath(name));
    if (!data || sha256(data) !== expected) fail(`校验和不匹配：${name}`);
  }
  console.log(`验证通过：${manifest.id} ${manifest.version}（${files.size} 个文件）`);
}

if (resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) try {
  const args = process.argv.slice(2).filter((argument) => argument !== "--");
  const [command, target, output] = args;
  if (!target || !["pack", "verify"].includes(command)) {
    fail("用法：plugin-package.mjs pack <plugin-directory> [output] | verify <plugin-file>");
  }
  if (command === "pack") pack(target, output); else verify(target);
} catch (error) {
  console.error(`插件包处理失败：${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
}
