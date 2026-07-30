import test from "node:test";
import assert from "node:assert/strict";
import { isV3PermissionAllowed, validateManifest } from "./plugin-package.mjs";

test("打包器权限集合来自 registry", () => {
  for (const permission of ["ai.invoke", "ai.context.augment", "credentials.use", "agents.invoke"]) {
    assert.equal(isV3PermissionAllowed(permission), true, permission);
  }
});

const fixedXingchenPermissions = [
  "credentials.use", "agents.invoke", "network.xingchen", "ai.invoke",
];

function combinationManifest(classification, runtimeKind) {
  const features = ["feature", "hybrid"].includes(classification)
    ? [{ id: "feature", title: "Feature", uiSchema: "ui.json", capabilities: [] }]
    : [];
  const enhancements = ["enhancement", "hybrid"].includes(classification)
    ? [{ id: "enhancement", title: "Enhancement", hook: "promptEnhancer",
      handler: { kind: "declarative", resource: "prompt.md" } }]
    : [];
  return {
    schemaVersion: 3, id: "com.firstwork.a42-test", name: "A4.2 Test",
    version: "1.0.0", authorId: "tests", classification, runtimeKind,
    permissions: [
      ...(enhancements.length ? ["ai.context.augment"] : []),
      ...(["xingchen-agent", "xingchen-workflow"].includes(runtimeKind) && features.length
        ? fixedXingchenPermissions : []),
    ],
    contributes: { features, enhancements },
  };
}

function combinationFiles() {
  return new Map([
    ["README.md", Buffer.from("test")],
    ["ui.json", Buffer.from('{"fields":[]}')],
    ["prompt.md", Buffer.from("prompt")],
  ]);
}

test("打包器执行冻结的 runtime/classification/contribution 矩阵", () => {
  for (const [classification, runtimeKind] of [
    ["feature", "declarative-ui"], ["enhancement", "prompt-pack"],
    ["feature", "xingchen-agent"], ["feature", "xingchen-workflow"],
    ["hybrid", "xingchen-agent"], ["hybrid", "xingchen-workflow"],
  ]) {
    assert.doesNotThrow(() => validateManifest(
      combinationManifest(classification, runtimeKind), combinationFiles(),
    ));
  }
  for (const [classification, runtimeKind] of [
    ["enhancement", "declarative-ui"], ["hybrid", "declarative-ui"],
    ["feature", "prompt-pack"], ["hybrid", "prompt-pack"],
    ["enhancement", "xingchen-agent"], ["enhancement", "xingchen-workflow"],
  ]) {
    assert.throws(() => validateManifest(
      combinationManifest(classification, runtimeKind), combinationFiles(),
    ));
  }
  const featureWithEnhancement = combinationManifest("hybrid", "xingchen-workflow");
  featureWithEnhancement.classification = "feature";
  assert.throws(() => validateManifest(featureWithEnhancement, combinationFiles()));
  const enhancementWithFeature = combinationManifest("hybrid", "prompt-pack");
  enhancementWithFeature.classification = "enhancement";
  assert.throws(() => validateManifest(enhancementWithFeature, combinationFiles()));
  for (const runtimeKind of ["legacy-js", "mcp-connector", "unknown-runtime"]) {
    const value = combinationManifest("feature", "declarative-ui");
    value.runtimeKind = runtimeKind;
    assert.throws(() => validateManifest(value, combinationFiles()));
  }
});

test("打包器拒绝缺失的组合权限和重复权限", () => {
  const hybrid = combinationManifest("hybrid", "xingchen-workflow");
  hybrid.permissions = hybrid.permissions.filter((permission) => permission !== "ai.context.augment");
  assert.throws(() => validateManifest(hybrid, combinationFiles()), /ai\.context\.augment/);
  for (const runtimeKind of ["xingchen-agent", "xingchen-workflow"]) {
    for (const missing of fixedXingchenPermissions) {
      const value = combinationManifest("feature", runtimeKind);
      value.permissions = value.permissions.filter((permission) => permission !== missing);
      assert.throws(() => validateManifest(value, combinationFiles()));
    }
  }
  const duplicate = combinationManifest("feature", "xingchen-workflow");
  duplicate.permissions.push("ai.invoke");
  assert.throws(() => validateManifest(duplicate, combinationFiles()), /重复/);
});

test("file.docx.output 必须配套 files.writeSelected", () => {
  const value = combinationManifest("feature", "xingchen-workflow");
  value.contributes.features[0].capabilities.push("file.docx.output");
  assert.throws(() => validateManifest(value, combinationFiles()), /files\.writeSelected/);
  value.permissions.push("files.writeSelected");
  assert.doesNotThrow(() => validateManifest(value, combinationFiles()));
});

test("permission/runtime 仅保留三个集中兼容例外", () => {
  const value = combinationManifest("feature", "declarative-ui");
  value.permissions = ["planning.files.write"];
  assert.throws(() => validateManifest(value, combinationFiles()), /runtimeKind/);
  for (const permission of ["tasks.read", "tasks.write", "mcp.connect"]) {
    value.permissions = [permission];
    assert.doesNotThrow(() => validateManifest(value, combinationFiles()));
  }
});

test("打包器拒绝 unknown、legacy、reserved 和 blocked 权限", () => {
  for (const permission of ["unknown.permission", "ai:chat", "notes.read", "credentials.configure"]) {
    assert.equal(isV3PermissionAllowed(permission), false, permission);
  }
});
