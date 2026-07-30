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

test("打包器保留同源化前的具体拒绝文案", () => {
  const featureMissing = combinationManifest("feature", "declarative-ui");
  featureMissing.contributes.features = [];
  assert.throws(
    () => validateManifest(featureMissing, combinationFiles()),
    /feature 插件必须声明 features/,
  );

  const enhancementMissing = combinationManifest("enhancement", "prompt-pack");
  enhancementMissing.contributes.enhancements = [];
  enhancementMissing.permissions = [];
  assert.throws(
    () => validateManifest(enhancementMissing, combinationFiles()),
    /enhancement 插件必须声明 enhancements/,
  );

  const hybridMissing = combinationManifest("hybrid", "xingchen-workflow");
  hybridMissing.contributes.enhancements = [];
  hybridMissing.permissions = fixedXingchenPermissions;
  assert.throws(
    () => validateManifest(hybridMissing, combinationFiles()),
    /hybrid 插件必须同时声明 features 和 enhancements/,
  );

  const featureWithEnhancement = combinationManifest("hybrid", "xingchen-workflow");
  featureWithEnhancement.classification = "feature";
  assert.throws(
    () => validateManifest(featureWithEnhancement, combinationFiles()),
    /classification=feature 不得声明 enhancement contribution/,
  );

  const enhancementWithFeature = combinationManifest("hybrid", "prompt-pack");
  enhancementWithFeature.classification = "enhancement";
  assert.throws(
    () => validateManifest(enhancementWithFeature, combinationFiles()),
    /classification=enhancement 不得声明 feature contribution/,
  );

  const enhancementPermission = combinationManifest("enhancement", "prompt-pack");
  enhancementPermission.permissions = [];
  assert.throws(
    () => validateManifest(enhancementPermission, combinationFiles()),
    /包含 enhancement contribution 的 Manifest 必须声明 ai\.context\.augment/,
  );

  const xingchenPermission = combinationManifest("feature", "xingchen-agent");
  xingchenPermission.permissions = xingchenPermission.permissions
    .filter((permission) => permission !== "credentials.use");
  assert.throws(
    () => validateManifest(xingchenPermission, combinationFiles()),
    /Xingchen feature 缺少必需权限 credentials\.use/,
  );

  const filePermission = combinationManifest("feature", "xingchen-workflow");
  filePermission.contributes.features[0].capabilities.push("file.docx.output");
  assert.throws(
    () => validateManifest(filePermission, combinationFiles()),
    /feature capability file\.docx\.output 必须声明 files\.writeSelected/,
  );
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
