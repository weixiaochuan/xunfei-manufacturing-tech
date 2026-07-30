import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const REGISTRY_PATH = join(ROOT, "config", "plugin-capabilities.v1.json");
const REQUIRED_FIELDS = [
  "id", "semanticVersion", "status", "legacyAliases", "titleKey", "descriptionKey",
  "riskLevel", "dataClasses", "operations", "scopeType", "runtimeKinds",
  "pluginSources", "hostCapability", "enforcementPoint", "requiredChecks",
  "auditPolicy", "disableBehavior", "updateBehavior", "rollbackBehavior", "owner",
  "testCases",
];
const STATUSES = new Set(["active", "restricted", "reserved", "blocked", "legacy", "deprecated"]);
const REQUESTABLE_STATUSES = new Set(["active", "restricted"]);

export function loadCapabilityRegistry(path = REGISTRY_PATH) {
  return JSON.parse(readFileSync(path, "utf8"));
}

export function validateCapabilityRegistry(registry) {
  if (registry.schemaVersion !== 1 || !Array.isArray(registry.capabilities)) {
    throw new Error("capability registry schemaVersion/capabilities 无效");
  }
  const ids = new Set();
  for (const capability of registry.capabilities) {
    for (const field of REQUIRED_FIELDS) {
      if (!(field in capability)) throw new Error(`${capability.id ?? "<unknown>"} 缺少字段 ${field}`);
    }
    if (typeof capability.id !== "string" || !capability.id) throw new Error("capability id 不能为空");
    if (ids.has(capability.id)) throw new Error(`重复 capability id：${capability.id}`);
    ids.add(capability.id);
    if (!STATUSES.has(capability.status)) throw new Error(`${capability.id} status 无效`);
    if (!/^\d+\.\d+\.\d+$/.test(capability.semanticVersion)) {
      throw new Error(`${capability.id} semanticVersion 无效`);
    }
    if (REQUESTABLE_STATUSES.has(capability.status)) {
      if (!capability.title || !capability.description || !capability.riskLevel) {
        throw new Error(`${capability.id} 缺少前端 title/description/risk 文案`);
      }
      if (!Array.isArray(capability.enforcementPoint) || capability.enforcementPoint.length === 0) {
        throw new Error(`${capability.id} 缺少 enforcementPoint`);
      }
      if (!Array.isArray(capability.testCases) || capability.testCases.length === 0) {
        throw new Error(`${capability.id} 缺少 testCases`);
      }
    }
    if (capability.status === "legacy" && !capability.runtimeKinds.includes("legacy-js")) {
      throw new Error(`${capability.id} legacy 项必须限定 legacy-js`);
    }
  }
  if (registry.capabilities.length !== 42) throw new Error(`registry 必须为 42 项，当前 ${registry.capabilities.length}`);
  const formal = registry.capabilities.filter((item) => item.status !== "legacy");
  const legacy = registry.capabilities.filter((item) => item.status === "legacy");
  if (formal.length !== 25 || legacy.length !== 17) {
    throw new Error(`registry 必须为 25 项正式点式权限和 17 项 legacy，当前 ${formal.length}/${legacy.length}`);
  }
  if (formal.some((item) => item.id.includes(":"))) throw new Error("正式权限不得使用冒号式名称");
  if (registry.capabilities.find((item) => item.id === "credentials.configure")?.status !== "blocked") {
    throw new Error("credentials.configure 必须为 blocked");
  }
  return registry;
}

export function v3RequestableCapabilities(registry) {
  return registry.capabilities.filter((item) => REQUESTABLE_STATUSES.has(item.status));
}

export function assertV3Permissions(permissions, registry) {
  const allowed = new Set(v3RequestableCapabilities(registry).map((item) => item.id));
  for (const permission of permissions) {
    if (!allowed.has(permission)) throw new Error(`Manifest 申请了非正式可用权限：${permission}`);
  }
}

function manifestPaths(root) {
  const paths = [];
  for (const entry of readdirSync(root)) {
    const path = join(root, entry);
    if (statSync(path).isDirectory()) paths.push(...manifestPaths(path));
    else if (entry === "manifest.json") paths.push(path);
  }
  return paths;
}

export function validateV3Examples(registry) {
  for (const path of manifestPaths(join(ROOT, "dev-plugins"))) {
    const manifest = JSON.parse(readFileSync(path, "utf8"));
    if (manifest.schemaVersion === 3) assertV3Permissions(manifest.permissions ?? [], registry);
  }
}

function renderTypeScript(registry) {
  const all = registry.capabilities;
  const requestable = v3RequestableCapabilities(registry);
  const json = (value) => JSON.stringify(value, null, 2);
  const presentation = Object.fromEntries(all.map((item) => [item.id, {
    title: item.title,
    description: item.description,
    riskLevel: item.riskLevel,
    status: item.status,
  }]));
  return `// 此文件由 scripts/plugin-capabilities.mjs 生成，请勿手工修改。\n`
    + `export const PLUGIN_CAPABILITY_IDS = ${json(all.map((item) => item.id))} as const;\n`
    + `export type PluginCapabilityId = (typeof PLUGIN_CAPABILITY_IDS)[number];\n`
    + `export const PLUGIN_V3_CAPABILITY_IDS = ${json(requestable.map((item) => item.id))} as const;\n`
    + `export type PluginV3CapabilityId = (typeof PLUGIN_V3_CAPABILITY_IDS)[number];\n`
    + `export const PLUGIN_CAPABILITY_PRESENTATION = ${json(presentation)} as const;\n`;
}

function renderRust(registry) {
  const rustArray = (name, items) => `pub(crate) const ${name}: &[&str] = &[\n${items.map((item) => `    ${JSON.stringify(item.id)},`).join("\n")}\n];\n`;
  return `// 此文件由 scripts/plugin-capabilities.mjs 生成，请勿手工修改。\n`
    + rustArray("VALID_PERMISSIONS", registry.capabilities)
    + "\n"
    + rustArray("V3_MANIFEST_PERMISSIONS", v3RequestableCapabilities(registry));
}

export function generatedFiles(registry) {
  return new Map([
    [join(ROOT, "src", "generated", "pluginCapabilities.ts"), renderTypeScript(registry)],
    [join(ROOT, "src-tauri", "src", "services", "plugin_capabilities_generated.rs"), renderRust(registry)],
  ]);
}

export function generate(registry) {
  for (const [path, content] of generatedFiles(registry)) {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content, "utf8");
  }
}

export function checkGenerated(registry) {
  for (const [path, expected] of generatedFiles(registry)) {
    if (readFileSync(path, "utf8") !== expected) {
      throw new Error(`派生文件未同步：${relative(ROOT, path)}；请运行 npm run plugin:capabilities:generate`);
    }
  }
}

if (resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  try {
    const registry = validateCapabilityRegistry(loadCapabilityRegistry());
    const command = process.argv[2] ?? "check";
    if (command === "generate") generate(registry);
    else if (command === "check") {
      checkGenerated(registry);
      validateV3Examples(registry);
    } else throw new Error("用法：plugin-capabilities.mjs generate|check");
    console.log(`capability registry ${command} 通过：42 项（正式 ${registry.capabilities.filter((x) => x.status !== "legacy").length} / legacy ${registry.capabilities.filter((x) => x.status === "legacy").length} / v3 可申请 ${v3RequestableCapabilities(registry).length}）`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
