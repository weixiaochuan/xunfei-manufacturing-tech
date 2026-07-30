import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const REGISTRY_PATH = join(ROOT, "config", "plugin-capabilities.v1.json");
const REQUIRED_FIELDS = [
  "id", "semanticVersion", "status", "legacyAliases", "titleKey", "descriptionKey",
  "title", "description", "riskLevel", "dataClasses", "operations", "scopeType",
  "scopeSchema", "grantModes", "grantLifetime", "runtimeKinds", "trustLevels",
  "pluginSources", "hostCapability", "enforcementPoint", "requiredChecks",
  "auditPolicy", "rateLimitPolicy", "dangerousCombinations", "disableBehavior",
  "updateBehavior", "rollbackBehavior", "owner", "legacyAliasPolicy", "testCases",
];
const STATUSES = new Set(["active", "restricted", "reserved", "blocked", "legacy", "deprecated"]);
const REQUESTABLE_STATUSES = new Set(["active", "restricted"]);
const RISK_LEVELS = new Set(["L1", "L2", "L3", "L4"]);
const REQUIRED_CHECKS = new Set(["P", "E", "S", "V", "I", "M", "A", "T", "R"]);
const IMPLEMENTATION_STATUSES = new Set([
  "partial", "planned-not-yet-enforced", "not-applicable",
  "compatibility-required-only", "legacy-compatibility", "source-policy-only",
]);
const GRANT_MODES = new Set(["required", "optional"]);
const GRANT_LIFETIMES = new Set(["persistent", "session", "one-shot", "policy"]);
const TRUST_LEVELS = new Set(["bundled", "internal", "reviewed-external", "development"]);
const TEST_CATEGORIES = new Set([
  "positive", "rejection", "scope", "revocation", "disable", "lifecycle",
]);
const TEST_STATUSES = new Set(["enforced", "planned-not-yet-enforced"]);
const RATE_LIMIT_MODES = new Set([
  "not-applicable", "existing-host-limit", "planned-not-yet-enforced",
]);
const REGISTRY_FIELDS = [
  "schemaVersion", "registryVersion", "allowedStatuses", "v3Policy",
  "testProfiles", "capabilities",
];
const V3_CLASSIFICATIONS = new Set(["feature", "enhancement", "hybrid"]);
const V3_RUNTIME_KINDS = new Set([
  "declarative-ui", "prompt-pack", "xingchen-agent", "xingchen-workflow",
]);
const V3_CONTRIBUTION_TYPES = new Set(["feature", "enhancement"]);
const V3_FEATURE_CAPABILITIES = new Set(["file.docx.output"]);
const V3_POLICY_FIELDS = [
  "classifications", "runtimeKinds", "contributionTypes", "featureCapabilities",
  "runtimePermissionCompatibilityExceptions", "classificationContributionRules",
  "runtimeClassificationRules", "contributionRequiredPermissions",
  "runtimeContributionRequiredPermissions", "featureCapabilityRequiredPermissions",
];

function assertExactFields(value, fields, path) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${path} 必须为对象`);
  }
  const allowed = new Set(fields);
  for (const field of Object.keys(value)) {
    if (!allowed.has(field)) throw new Error(`${path}.${field} 是未知 policy 字段`);
  }
  for (const field of fields) {
    if (!(field in value)) throw new Error(`${path} 缺少字段 ${field}`);
  }
}

function assertUniqueStrings(values, allowed, path) {
  if (!Array.isArray(values) || values.some((value) => typeof value !== "string")) {
    throw new Error(`${path} 必须为字符串数组`);
  }
  if (new Set(values).size !== values.length) throw new Error(`${path} 包含重复项`);
  for (const value of values) {
    if (allowed && !allowed.has(value)) throw new Error(`${path} 引用了未知值 ${value}`);
  }
}

function assertUniqueRules(rules, keyOf, path) {
  if (!Array.isArray(rules)) throw new Error(`${path} 必须为数组`);
  const keys = new Set();
  for (const rule of rules) {
    const key = keyOf(rule);
    if (keys.has(key)) throw new Error(`${path} 包含重复规则 ${key}`);
    keys.add(key);
  }
}

function assertStructuredPolicy(value, allowedValues, path, allowEmpty = false) {
  assertExactFields(value, ["allowed", "implementationStatus"], path);
  assertUniqueStrings(value.allowed, allowedValues, `${path}.allowed`);
  if (!allowEmpty && value.allowed.length === 0) throw new Error(`${path}.allowed 不能为空`);
  if (!IMPLEMENTATION_STATUSES.has(value.implementationStatus)) {
    throw new Error(`${path}.implementationStatus 无效`);
  }
}

function validateTestProfiles(testProfiles) {
  if (!testProfiles || typeof testProfiles !== "object" || Array.isArray(testProfiles)) {
    throw new Error("$.testProfiles 必须为对象");
  }
  for (const [name, profile] of Object.entries(testProfiles)) {
    assertExactFields(profile, ["categories", "defaultStatus"], `$.testProfiles.${name}`);
    assertUniqueStrings(profile.categories, TEST_CATEGORIES, `$.testProfiles.${name}.categories`);
    if (!TEST_STATUSES.has(profile.defaultStatus)) {
      throw new Error(`$.testProfiles.${name}.defaultStatus 无效`);
    }
  }
  for (const required of ["standard-v1-matrix", "non-requestable-v1", "legacy-compatibility-v1"]) {
    if (!(required in testProfiles)) throw new Error(`$.testProfiles 缺少 ${required}`);
  }
  const fullMatrix = ["positive", "rejection", "scope", "revocation", "disable", "lifecycle"];
  for (const name of ["standard-v1-matrix", "legacy-compatibility-v1"]) {
    if (!fullMatrix.every((category) => testProfiles[name].categories.includes(category))) {
      throw new Error(`$.testProfiles.${name} 未覆盖完整测试类别`);
    }
  }
}

function validateCapabilityPolicyFields(capability, capabilityIds, testProfiles) {
  const path = `$.capabilities.${capability.id}`;
  assertExactFields(capability.legacyAliasPolicy, ["mode", "autoMapToManifestPermission"], `${path}.legacyAliasPolicy`);
  if (capability.legacyAliasPolicy.mode !== "explicit-compatibility-only"
    || capability.legacyAliasPolicy.autoMapToManifestPermission !== false) {
    throw new Error(`${path}.legacyAliasPolicy 必须禁止自动映射`);
  }
  assertExactFields(
    capability.scopeSchema,
    ["type", "version", "implementationStatus"],
    `${path}.scopeSchema`,
  );
  const scopeTypeMatches = capability.scopeSchema.type === capability.scopeType
    || (capability.scopeType === "none" && capability.scopeSchema.type === "not-applicable");
  if (!scopeTypeMatches || capability.scopeSchema.version !== 1) {
    throw new Error(`${path}.scopeSchema 必须绑定当前 scopeType 和版本 1`);
  }
  if (!IMPLEMENTATION_STATUSES.has(capability.scopeSchema.implementationStatus)) {
    throw new Error(`${path}.scopeSchema.implementationStatus 无效`);
  }
  const nonRequestable = ["reserved", "blocked"].includes(capability.status);
  assertStructuredPolicy(capability.grantModes, GRANT_MODES, `${path}.grantModes`, nonRequestable);
  assertStructuredPolicy(
    capability.grantLifetime,
    GRANT_LIFETIMES,
    `${path}.grantLifetime`,
    nonRequestable,
  );
  assertStructuredPolicy(capability.trustLevels, TRUST_LEVELS, `${path}.trustLevels`, nonRequestable);
  assertUniqueStrings(capability.requiredChecks, REQUIRED_CHECKS, `${path}.requiredChecks`);
  if (!nonRequestable && capability.requiredChecks.length !== REQUIRED_CHECKS.size) {
    throw new Error(`${path}.requiredChecks 必须覆盖 P/E/S/V/I/M/A/T/R`);
  }
  if (!capability.rateLimitPolicy || typeof capability.rateLimitPolicy !== "object"
    || Array.isArray(capability.rateLimitPolicy)) {
    throw new Error(`${path}.rateLimitPolicy 必须为对象`);
  }
  const rateFields = Object.keys(capability.rateLimitPolicy);
  if (rateFields.some((field) => !["mode", "implementationStatus"].includes(field))
    || !("mode" in capability.rateLimitPolicy)) {
    throw new Error(`${path}.rateLimitPolicy 字段无效`);
  }
  if (!RATE_LIMIT_MODES.has(capability.rateLimitPolicy.mode)) {
    throw new Error(`${path}.rateLimitPolicy.mode 无效`);
  }
  if ("implementationStatus" in capability.rateLimitPolicy
    && !IMPLEMENTATION_STATUSES.has(capability.rateLimitPolicy.implementationStatus)) {
    throw new Error(`${path}.rateLimitPolicy.implementationStatus 无效`);
  }
  if (!Array.isArray(capability.dangerousCombinations)) {
    throw new Error(`${path}.dangerousCombinations 必须为数组`);
  }
  for (const [index, combination] of capability.dangerousCombinations.entries()) {
    const combinationPath = `${path}.dangerousCombinations[${index}]`;
    assertExactFields(
      combination,
      ["condition", "withCapabilities", "elevatesTo", "policy"],
      combinationPath,
    );
    if (typeof combination.condition !== "string" || !combination.condition
      || typeof combination.policy !== "string" || !combination.policy) {
      throw new Error(`${combinationPath} condition/policy 不能为空`);
    }
    assertUniqueStrings(
      combination.withCapabilities,
      capabilityIds,
      `${combinationPath}.withCapabilities`,
    );
    if (!RISK_LEVELS.has(combination.elevatesTo)) {
      throw new Error(`${combinationPath}.elevatesTo 无效`);
    }
  }
  assertExactFields(capability.testCases, ["profile", "references"], `${path}.testCases`);
  const profile = testProfiles[capability.testCases.profile];
  if (!profile) throw new Error(`${path}.testCases.profile 不存在`);
  if (!Array.isArray(capability.testCases.references)) {
    throw new Error(`${path}.testCases.references 必须为数组`);
  }
  for (const [index, reference] of capability.testCases.references.entries()) {
    const referencePath = `${path}.testCases.references[${index}]`;
    assertExactFields(reference, ["id", "category", "status"], referencePath);
    if (typeof reference.id !== "string" || !reference.id.trim()) {
      throw new Error(`${referencePath}.id 不能为空`);
    }
    if (!TEST_CATEGORIES.has(reference.category)
      || !profile.categories.includes(reference.category)) {
      throw new Error(`${referencePath}.category 无效`);
    }
    if (!TEST_STATUSES.has(reference.status)) throw new Error(`${referencePath}.status 无效`);
  }
}

function validateV3Policy(policy, capabilityIds) {
  assertExactFields(policy, V3_POLICY_FIELDS, "$.v3Policy");
  assertUniqueStrings(policy.classifications, V3_CLASSIFICATIONS, "$.v3Policy.classifications");
  assertUniqueStrings(policy.runtimeKinds, V3_RUNTIME_KINDS, "$.v3Policy.runtimeKinds");
  assertUniqueStrings(
    policy.contributionTypes, V3_CONTRIBUTION_TYPES, "$.v3Policy.contributionTypes",
  );
  assertUniqueStrings(
    policy.featureCapabilities, V3_FEATURE_CAPABILITIES, "$.v3Policy.featureCapabilities",
  );
  if (policy.classifications.length !== V3_CLASSIFICATIONS.size
    || policy.runtimeKinds.length !== V3_RUNTIME_KINDS.size
    || policy.contributionTypes.length !== V3_CONTRIBUTION_TYPES.size
    || policy.featureCapabilities.length !== V3_FEATURE_CAPABILITIES.size) {
    throw new Error("$.v3Policy vocabulary 必须完整覆盖当前宿主支持值");
  }
  assertUniqueStrings(
    policy.runtimePermissionCompatibilityExceptions,
    capabilityIds,
    "$.v3Policy.runtimePermissionCompatibilityExceptions",
  );

  assertUniqueRules(
    policy.classificationContributionRules,
    (rule) => rule.classification,
    "$.v3Policy.classificationContributionRules",
  );
  for (const [index, rule] of policy.classificationContributionRules.entries()) {
    const path = `$.v3Policy.classificationContributionRules[${index}]`;
    assertExactFields(
      rule,
      ["classification", "requiredContributions", "forbiddenContributions"],
      path,
    );
    if (!V3_CLASSIFICATIONS.has(rule.classification)) {
      throw new Error(`${path}.classification 引用了未知值 ${rule.classification}`);
    }
    assertUniqueStrings(
      rule.requiredContributions, V3_CONTRIBUTION_TYPES, `${path}.requiredContributions`,
    );
    assertUniqueStrings(
      rule.forbiddenContributions, V3_CONTRIBUTION_TYPES, `${path}.forbiddenContributions`,
    );
  }
  if (policy.classificationContributionRules.length !== V3_CLASSIFICATIONS.size) {
    throw new Error("$.v3Policy.classificationContributionRules 必须覆盖全部 classification");
  }

  assertUniqueRules(
    policy.runtimeClassificationRules,
    (rule) => rule.runtimeKind,
    "$.v3Policy.runtimeClassificationRules",
  );
  for (const [index, rule] of policy.runtimeClassificationRules.entries()) {
    const path = `$.v3Policy.runtimeClassificationRules[${index}]`;
    assertExactFields(rule, ["runtimeKind", "classifications"], path);
    if (!V3_RUNTIME_KINDS.has(rule.runtimeKind)) {
      throw new Error(`${path}.runtimeKind 引用了未知值 ${rule.runtimeKind}`);
    }
    assertUniqueStrings(rule.classifications, V3_CLASSIFICATIONS, `${path}.classifications`);
  }
  if (policy.runtimeClassificationRules.length !== V3_RUNTIME_KINDS.size) {
    throw new Error("$.v3Policy.runtimeClassificationRules 必须覆盖全部 runtimeKind");
  }

  assertUniqueRules(
    policy.contributionRequiredPermissions,
    (rule) => rule.contribution,
    "$.v3Policy.contributionRequiredPermissions",
  );
  for (const [index, rule] of policy.contributionRequiredPermissions.entries()) {
    const path = `$.v3Policy.contributionRequiredPermissions[${index}]`;
    assertExactFields(rule, ["contribution", "permissions"], path);
    if (!V3_CONTRIBUTION_TYPES.has(rule.contribution)) {
      throw new Error(`${path}.contribution 引用了未知值 ${rule.contribution}`);
    }
    assertUniqueStrings(rule.permissions, capabilityIds, `${path}.permissions`);
  }

  assertUniqueRules(
    policy.runtimeContributionRequiredPermissions,
    (rule) => `${[...(rule.runtimeKinds ?? [])].sort().join(",")}|${rule.contribution}`,
    "$.v3Policy.runtimeContributionRequiredPermissions",
  );
  for (const [index, rule] of policy.runtimeContributionRequiredPermissions.entries()) {
    const path = `$.v3Policy.runtimeContributionRequiredPermissions[${index}]`;
    assertExactFields(rule, ["runtimeKinds", "contribution", "permissions"], path);
    assertUniqueStrings(rule.runtimeKinds, V3_RUNTIME_KINDS, `${path}.runtimeKinds`);
    if (!V3_CONTRIBUTION_TYPES.has(rule.contribution)) {
      throw new Error(`${path}.contribution 引用了未知值 ${rule.contribution}`);
    }
    assertUniqueStrings(rule.permissions, capabilityIds, `${path}.permissions`);
  }

  assertUniqueRules(
    policy.featureCapabilityRequiredPermissions,
    (rule) => rule.featureCapability,
    "$.v3Policy.featureCapabilityRequiredPermissions",
  );
  for (const [index, rule] of policy.featureCapabilityRequiredPermissions.entries()) {
    const path = `$.v3Policy.featureCapabilityRequiredPermissions[${index}]`;
    assertExactFields(rule, ["featureCapability", "permissions"], path);
    if (!V3_FEATURE_CAPABILITIES.has(rule.featureCapability)) {
      throw new Error(`${path}.featureCapability 引用了未知值 ${rule.featureCapability}`);
    }
    assertUniqueStrings(rule.permissions, capabilityIds, `${path}.permissions`);
  }
  return policy;
}

export function loadCapabilityRegistry(path = REGISTRY_PATH) {
  return JSON.parse(readFileSync(path, "utf8"));
}

export function validateCapabilityRegistry(registry) {
  assertExactFields(registry, REGISTRY_FIELDS, "$");
  if (registry.schemaVersion !== 1 || !Array.isArray(registry.capabilities)) {
    throw new Error("capability registry schemaVersion/capabilities 无效");
  }
  assertUniqueStrings(registry.allowedStatuses, STATUSES, "$.allowedStatuses");
  if (registry.allowedStatuses.length !== STATUSES.size) {
    throw new Error("$.allowedStatuses 必须覆盖全部正式状态");
  }
  validateTestProfiles(registry.testProfiles);
  const ids = new Set();
  for (const capability of registry.capabilities) {
    assertExactFields(capability, REQUIRED_FIELDS, `$.capabilities.${capability.id ?? "<unknown>"}`);
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
    if (!RISK_LEVELS.has(capability.riskLevel)) {
      throw new Error(`${capability.id} riskLevel 必须为 L1/L2/L3/L4`);
    }
    assertUniqueStrings(capability.legacyAliases, null, `${capability.id}.legacyAliases`);
    assertUniqueStrings(capability.runtimeKinds, null, `${capability.id}.runtimeKinds`);
    assertUniqueStrings(capability.pluginSources, null, `${capability.id}.pluginSources`);
    if (!Array.isArray(capability.enforcementPoint)
      || capability.enforcementPoint.some((item) => typeof item !== "string" || !item.trim())) {
      throw new Error(`${capability.id} enforcementPoint 必须为非空字符串数组`);
    }
    if (REQUESTABLE_STATUSES.has(capability.status)) {
      if (!capability.title || !capability.description || !capability.riskLevel) {
        throw new Error(`${capability.id} 缺少前端 title/description/risk 文案`);
      }
      if (typeof capability.hostCapability !== "string" || !capability.hostCapability.trim()) {
        throw new Error(`${capability.id} 缺少 hostCapability`);
      }
      if (capability.enforcementPoint.length === 0) {
        throw new Error(`${capability.id} 缺少 enforcementPoint`);
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
  const counts = Object.fromEntries(
    [...STATUSES].map((status) => [
      status,
      registry.capabilities.filter((item) => item.status === status).length,
    ]),
  );
  const expectedCounts = {
    active: 3,
    restricted: 17,
    reserved: 4,
    blocked: 1,
    legacy: 17,
    deprecated: 0,
  };
  for (const [status, expected] of Object.entries(expectedCounts)) {
    if (counts[status] !== expected) {
      throw new Error(`registry status ${status} 必须为 ${expected} 项，当前 ${counts[status]}`);
    }
  }
  const capabilityIds = new Set(registry.capabilities.map((item) => item.id));
  for (const capability of registry.capabilities) {
    validateCapabilityPolicyFields(capability, capabilityIds, registry.testProfiles);
  }
  const requestable = v3RequestableCapabilities(registry);
  if (requestable.length !== 20) {
    throw new Error(`v3 requestable capability 必须为 20 项，当前 ${requestable.length}`);
  }
  if (requestable.some((item) => item.status === "reserved"
    || item.status === "blocked" || item.status === "legacy")) {
    throw new Error("reserved/blocked/legacy 不得进入 v3 requestable 集合");
  }
  validateV3Policy(
    registry.v3Policy,
    new Set(requestable.map((item) => item.id)),
  );
  return registry;
}

export function v3RequestableCapabilities(registry) {
  return registry.capabilities.filter((item) => REQUESTABLE_STATUSES.has(item.status));
}

export function isV3PermissionRuntimeAllowed(permission, runtimeKind, registry) {
  if (registry.v3Policy.runtimePermissionCompatibilityExceptions.includes(permission)) return true;
  const capability = v3RequestableCapabilities(registry).find((item) => item.id === permission);
  return Boolean(capability?.runtimeKinds.includes(runtimeKind));
}

export function isV3ClassificationContributionAllowed(
  classification,
  contributions,
  registry,
) {
  return evaluateV3ClassificationContributions(classification, contributions, registry).ok;
}

export function evaluateV3ClassificationContributions(
  classification,
  contributions,
  registry,
) {
  const rule = registry.v3Policy.classificationContributionRules
    .find((item) => item.classification === classification);
  if (!rule) return { ok: false, code: "unknown-classification", classification };
  const missing = rule.requiredContributions
    .find((item) => !contributions.includes(item));
  if (missing) {
    return {
      ok: false,
      code: "missing-required-contribution",
      classification,
      contribution: missing,
    };
  }
  const forbidden = rule.forbiddenContributions
    .find((item) => contributions.includes(item));
  if (forbidden) {
    return {
      ok: false,
      code: "forbidden-contribution",
      classification,
      contribution: forbidden,
    };
  }
  return { ok: true };
}

export function isV3RuntimeClassificationAllowed(runtimeKind, classification, registry) {
  const rule = registry.v3Policy.runtimeClassificationRules
    .find((item) => item.runtimeKind === runtimeKind);
  return Boolean(rule?.classifications.includes(classification));
}

export function requiredV3PolicyPermissions(
  { runtimeKind, contributions, featureCapabilities },
  registry,
) {
  const required = [];
  for (const rule of registry.v3Policy.contributionRequiredPermissions) {
    if (contributions.includes(rule.contribution)) required.push(...rule.permissions);
  }
  for (const rule of registry.v3Policy.runtimeContributionRequiredPermissions) {
    if (rule.runtimeKinds.includes(runtimeKind) && contributions.includes(rule.contribution)) {
      required.push(...rule.permissions);
    }
  }
  for (const rule of registry.v3Policy.featureCapabilityRequiredPermissions) {
    if (featureCapabilities.includes(rule.featureCapability)) required.push(...rule.permissions);
  }
  return [...new Set(required)];
}

export function evaluateV3RequiredPermissions(
  { runtimeKind, contributions, featureCapabilities, permissions },
  registry,
) {
  for (const rule of registry.v3Policy.contributionRequiredPermissions) {
    if (!contributions.includes(rule.contribution)) continue;
    const missing = rule.permissions.find((permission) => !permissions.includes(permission));
    if (missing) {
      return {
        ok: false,
        code: "missing-contribution-permission",
        contribution: rule.contribution,
        permission: missing,
      };
    }
  }
  for (const rule of registry.v3Policy.runtimeContributionRequiredPermissions) {
    if (!rule.runtimeKinds.includes(runtimeKind)
      || !contributions.includes(rule.contribution)) continue;
    const missing = rule.permissions.find((permission) => !permissions.includes(permission));
    if (missing) {
      return {
        ok: false,
        code: "missing-runtime-contribution-permission",
        runtimeKind,
        contribution: rule.contribution,
        permission: missing,
      };
    }
  }
  for (const rule of registry.v3Policy.featureCapabilityRequiredPermissions) {
    if (!featureCapabilities.includes(rule.featureCapability)) continue;
    const missing = rule.permissions.find((permission) => !permissions.includes(permission));
    if (missing) {
      return {
        ok: false,
        code: "missing-feature-capability-permission",
        featureCapability: rule.featureCapability,
        permission: missing,
      };
    }
  }
  return { ok: true };
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
  const stringArray = (items) => `&[${items.map((item) => JSON.stringify(item)).join(", ")}]`;
  const runtimeMap = v3RequestableCapabilities(registry)
    .map((item) => `    (${JSON.stringify(item.id)}, &[${item.runtimeKinds.map((kind) => JSON.stringify(kind)).join(", ")}]),`)
    .join("\n");
  const policy = registry.v3Policy;
  const classificationRules = policy.classificationContributionRules
    .map((rule) => `    (${JSON.stringify(rule.classification)}, ${stringArray(rule.requiredContributions)}, ${stringArray(rule.forbiddenContributions)}),`)
    .join("\n");
  const runtimeClassificationRules = policy.runtimeClassificationRules
    .map((rule) => `    (${JSON.stringify(rule.runtimeKind)}, ${stringArray(rule.classifications)}),`)
    .join("\n");
  const contributionPermissions = policy.contributionRequiredPermissions
    .map((rule) => `    (${JSON.stringify(rule.contribution)}, ${stringArray(rule.permissions)}),`)
    .join("\n");
  const runtimeContributionPermissions = policy.runtimeContributionRequiredPermissions
    .map((rule) => `    (${stringArray(rule.runtimeKinds)}, ${JSON.stringify(rule.contribution)}, ${stringArray(rule.permissions)}),`)
    .join("\n");
  const featureCapabilityPermissions = policy.featureCapabilityRequiredPermissions
    .map((rule) => `    (${JSON.stringify(rule.featureCapability)}, ${stringArray(rule.permissions)}),`)
    .join("\n");
  return `// 此文件由 scripts/plugin-capabilities.mjs 生成，请勿手工修改。\n`
    + rustArray("VALID_PERMISSIONS", registry.capabilities)
    + "\n"
    + rustArray("V3_MANIFEST_PERMISSIONS", v3RequestableCapabilities(registry))
    + "\n"
    + `pub(crate) const V3_PERMISSION_RUNTIME_KINDS: &[(&str, &[&str])] = &[\n${runtimeMap}\n];\n\n`
    + `pub(crate) const V3_PERMISSION_RUNTIME_COMPATIBILITY_EXCEPTIONS: &[&str] = ${stringArray(policy.runtimePermissionCompatibilityExceptions)};\n\n`
    + `pub(crate) const V3_CLASSIFICATION_CONTRIBUTION_RULES: &[(&str, &[&str], &[&str])] = &[\n${classificationRules}\n];\n\n`
    + `pub(crate) const V3_RUNTIME_CLASSIFICATION_RULES: &[(&str, &[&str])] = &[\n${runtimeClassificationRules}\n];\n\n`
    + `pub(crate) const V3_CONTRIBUTION_REQUIRED_PERMISSIONS: &[(&str, &[&str])] = &[\n${contributionPermissions}\n];\n\n`
    + `pub(crate) const V3_RUNTIME_CONTRIBUTION_REQUIRED_PERMISSIONS: &[(&[&str], &str, &[&str])] = &[\n${runtimeContributionPermissions}\n];\n\n`
    + `pub(crate) const V3_FEATURE_CAPABILITY_REQUIRED_PERMISSIONS: &[(&str, &[&str])] = &[\n${featureCapabilityPermissions}\n];\n`;
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
