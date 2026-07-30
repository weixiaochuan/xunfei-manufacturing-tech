import test from "node:test";
import assert from "node:assert/strict";
import {
  assertV3Permissions,
  isV3PermissionRuntimeAllowed,
  loadCapabilityRegistry,
  V3_PERMISSION_RUNTIME_COMPATIBILITY_EXCEPTIONS,
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
    V3_PERMISSION_RUNTIME_COMPATIBILITY_EXCEPTIONS,
    ["tasks.read", "tasks.write", "mcp.connect"],
  );
  assert.equal(
    isV3PermissionRuntimeAllowed("planning.files.write", "declarative-ui", registry),
    false,
  );
});

test("registry 完整覆盖 42 项且 v3 集合仅含 active/restricted", () => {
  const registry = validateCapabilityRegistry(loadCapabilityRegistry());
  assert.equal(registry.capabilities.length, 42);
  assert.equal(registry.capabilities.filter((item) => item.status !== "legacy").length, 25);
  assert.equal(registry.capabilities.filter((item) => item.status === "legacy").length, 17);
  assert.equal(v3RequestableCapabilities(registry).length, 20);
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

test("所有现有 v3 示例只使用正式可申请权限", () => {
  const registry = validateCapabilityRegistry(loadCapabilityRegistry());
  assert.doesNotThrow(() => validateV3Examples(registry));
});
