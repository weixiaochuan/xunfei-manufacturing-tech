import test from "node:test";
import assert from "node:assert/strict";
import {
  assertV3Permissions,
  isV3ClassificationContributionAllowed,
  isV3PermissionRuntimeAllowed,
  isV3RuntimeClassificationAllowed,
  loadCapabilityRegistry,
  requiredV3PolicyPermissions,
  validateCapabilityRegistry,
  validateV3Examples,
  v3RequestableCapabilities,
} from "./plugin-capabilities.mjs";

test("ai.context.augment runtime 修正与三个兼容例外保持精确", () => {
  const registry = validateCapabilityRegistry(loadCapabilityRegistry());
  for (const runtime of [
    "prompt-pack", "declarative-ui", "xingchen-agent", "xingchen-workflow",
  ]) {
    assert.equal(isV3PermissionRuntimeAllowed("ai.context.augment", runtime, registry), true);
  }
  for (const runtime of ["legacy-js", "mcp-connector", "unknown-runtime"]) {
    assert.equal(isV3PermissionRuntimeAllowed("ai.context.augment", runtime, registry), false);
  }
  assert.deepEqual(
    registry.v3Policy.runtimePermissionCompatibilityExceptions,
    ["tasks.read", "tasks.write", "mcp.connect"],
  );
  for (const permission of [
    "ai.context.read",
    "ai.session.read",
    "ui.chat.toolbar",
    "ui.chat.panel",
    "planning.files.read",
    "planning.files.write",
  ]) {
    assert.equal(isV3PermissionRuntimeAllowed(permission, "prompt-pack", registry), true);
    assert.equal(isV3PermissionRuntimeAllowed(permission, "declarative-ui", registry), true);
    assert.equal(isV3PermissionRuntimeAllowed(permission, "legacy-js", registry), false);
  }
  assert.equal(
    isV3PermissionRuntimeAllowed("credentials.use", "declarative-ui", registry),
    false,
  );
});

test("registry 完整覆盖 42 项且 v3 集合仅含 active/restricted", () => {
  const registry = validateCapabilityRegistry(loadCapabilityRegistry());
  assert.equal(registry.capabilities.length, 42);
  assert.equal(registry.capabilities.filter((item) => item.status !== "legacy").length, 25);
  assert.equal(registry.capabilities.filter((item) => item.status === "legacy").length, 17);
  const counts = Object.fromEntries(
    ["active", "restricted", "reserved", "blocked", "legacy"]
      .map((status) => [
        status,
        registry.capabilities.filter((item) => item.status === status).length,
      ]),
  );
  assert.deepEqual(counts, {
    active: 3, restricted: 17, reserved: 4, blocked: 1, legacy: 17,
  });
  assert.deepEqual(
    registry.capabilities.filter((item) => item.status === "active").map((item) => item.id),
    ["ui.editor.toolbar", "ui.chat.toolbar", "ui.chat.panel"],
  );
  assert.deepEqual(
    registry.capabilities.filter((item) => item.status === "reserved").map((item) => item.id),
    ["notes.read", "notes.write", "files.readSelected", "views.register"],
  );
  assert.deepEqual(
    v3RequestableCapabilities(registry).map((item) => item.id),
    [
      "document.read", "document.write", "tasks.read", "tasks.write", "ai.invoke",
      "ai.context.read", "ai.context.augment", "ai.session.read", "ui.editor.toolbar",
      "ui.chat.toolbar", "ui.chat.panel", "planning.files.read", "planning.files.write",
      "network.request", "files.writeSelected", "prompts.register", "mcp.connect",
      "credentials.use", "network.xingchen", "agents.invoke",
    ],
  );
});

test("v3 拒绝 unknown、legacy、reserved 和 blocked", () => {
  const registry = validateCapabilityRegistry(loadCapabilityRegistry());
  assertV3Permissions(["ai.invoke"], registry);
  for (const permission of ["unknown.permission", "ai:chat", "notes.read", "credentials.configure"]) {
    assert.throws(() => assertV3Permissions([permission], registry), permission);
  }
});

test("重复 ID 和字段缺失导致校验失败", () => {
  const registry = loadCapabilityRegistry();
  assert.throws(() => validateCapabilityRegistry({
    ...registry,
    capabilities: [...registry.capabilities, registry.capabilities[0]],
  }), /重复 capability id/);
  const malformed = structuredClone(registry);
  delete malformed.capabilities[0].owner;
  assert.throws(() => validateCapabilityRegistry(malformed), /缺少字段 owner/);
});

test("正式 registry 字段、风险、检查码和 alias 策略严格校验", () => {
  const registry = validateCapabilityRegistry(loadCapabilityRegistry());
  assert.ok(registry.capabilities.every((item) => ["L1", "L2", "L3", "L4"].includes(item.riskLevel)));
  assert.ok(registry.capabilities.every(
    (item) => item.legacyAliasPolicy.autoMapToManifestPermission === false,
  ));
  for (const field of [
    "scopeSchema", "grantModes", "grantLifetime", "trustLevels",
    "rateLimitPolicy", "dangerousCombinations",
  ]) {
    const malformed = structuredClone(registry);
    delete malformed.capabilities[0][field];
    assert.throws(() => validateCapabilityRegistry(malformed), new RegExp(`缺少字段 ${field}`));
  }
  const invalidRisk = structuredClone(registry);
  invalidRisk.capabilities[0].riskLevel = "critical";
  assert.throws(() => validateCapabilityRegistry(invalidRisk), /riskLevel/);
  const invalidCheck = structuredClone(registry);
  invalidCheck.capabilities.find((item) => item.id === "document.read")
    .requiredChecks.push("unknown-check");
  assert.throws(() => validateCapabilityRegistry(invalidCheck), /requiredChecks/);
  const missingHost = structuredClone(registry);
  missingHost.capabilities.find((item) => item.id === "ui.editor.toolbar").hostCapability = null;
  assert.throws(() => validateCapabilityRegistry(missingHost), /hostCapability/);
  const missingEnforcement = structuredClone(registry);
  missingEnforcement.capabilities.find((item) => item.id === "ui.chat.panel")
    .enforcementPoint = [];
  assert.throws(() => validateCapabilityRegistry(missingEnforcement), /enforcementPoint/);
  const autoAlias = structuredClone(registry);
  autoAlias.capabilities.find((item) => item.id === "ai:chat")
    .legacyAliasPolicy.autoMapToManifestPermission = true;
  assert.throws(() => validateCapabilityRegistry(autoAlias), /禁止自动映射/);
  const invalidTestCase = structuredClone(registry);
  invalidTestCase.capabilities[0].testCases.references[0].status = "passed";
  assert.throws(() => validateCapabilityRegistry(invalidTestCase), /testCases/);
  const unknownCapabilityField = structuredClone(registry);
  unknownCapabilityField.capabilities[0].unreviewedPolicy = true;
  assert.throws(() => validateCapabilityRegistry(unknownCapabilityField), /未知 policy 字段/);
});

test("v3 policy 拒绝未知字段、重复规则和未知引用", () => {
  const unknownField = structuredClone(loadCapabilityRegistry());
  unknownField.v3Policy.allowEverything = true;
  assert.throws(() => validateCapabilityRegistry(unknownField), /未知 policy 字段/);

  const duplicateRule = structuredClone(loadCapabilityRegistry());
  duplicateRule.v3Policy.runtimeClassificationRules.push(
    structuredClone(duplicateRule.v3Policy.runtimeClassificationRules[0]),
  );
  assert.throws(() => validateCapabilityRegistry(duplicateRule), /重复规则/);

  const unknownCapability = structuredClone(loadCapabilityRegistry());
  unknownCapability.v3Policy.contributionRequiredPermissions[0]
    .permissions.push("unknown.permission");
  assert.throws(() => validateCapabilityRegistry(unknownCapability), /未知值/);

  const unknownRuntime = structuredClone(loadCapabilityRegistry());
  unknownRuntime.v3Policy.runtimeClassificationRules[0].runtimeKind = "unknown-runtime";
  assert.throws(() => validateCapabilityRegistry(unknownRuntime), /未知值/);

  const unknownClassification = structuredClone(loadCapabilityRegistry());
  unknownClassification.v3Policy.classificationContributionRules[0]
    .classification = "unknown-classification";
  assert.throws(() => validateCapabilityRegistry(unknownClassification), /未知值/);

  const unknownContribution = structuredClone(loadCapabilityRegistry());
  unknownContribution.v3Policy.contributionRequiredPermissions[0]
    .contribution = "unknown-contribution";
  assert.throws(() => validateCapabilityRegistry(unknownContribution), /未知值/);
});

test("Node 对 canonical v3 policy 的解释保持冻结矩阵", () => {
  const registry = validateCapabilityRegistry(loadCapabilityRegistry());
  for (const [classification, contributions] of [
    ["feature", ["feature"]],
    ["enhancement", ["enhancement"]],
    ["hybrid", ["feature", "enhancement"]],
  ]) {
    assert.equal(
      isV3ClassificationContributionAllowed(classification, contributions, registry),
      true,
    );
  }
  for (const [runtimeKind, classification] of [
    ["declarative-ui", "feature"],
    ["prompt-pack", "enhancement"],
    ["xingchen-agent", "feature"],
    ["xingchen-agent", "hybrid"],
    ["xingchen-workflow", "feature"],
    ["xingchen-workflow", "hybrid"],
  ]) {
    assert.equal(
      isV3RuntimeClassificationAllowed(runtimeKind, classification, registry),
      true,
    );
  }
  assert.deepEqual(
    requiredV3PolicyPermissions({
      runtimeKind: "xingchen-workflow",
      contributions: ["feature", "enhancement"],
      featureCapabilities: ["file.docx.output"],
    }, registry).sort(),
    [
      "agents.invoke",
      "ai.context.augment",
      "ai.invoke",
      "credentials.use",
      "files.writeSelected",
      "network.xingchen",
    ],
  );
});

test("所有现有 v3 示例只使用正式可申请权限", () => {
  const registry = validateCapabilityRegistry(loadCapabilityRegistry());
  assert.doesNotThrow(() => validateV3Examples(registry));
});
