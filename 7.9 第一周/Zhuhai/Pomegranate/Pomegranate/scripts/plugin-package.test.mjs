import test from "node:test";
import assert from "node:assert/strict";
import { isV3PermissionAllowed } from "./plugin-package.mjs";

test("打包器权限集合来自 registry", () => {
  for (const permission of ["ai.invoke", "ai.context.augment", "credentials.use", "agents.invoke"]) {
    assert.equal(isV3PermissionAllowed(permission), true, permission);
  }
});

test("打包器拒绝 unknown、legacy、reserved 和 blocked 权限", () => {
  for (const permission of ["unknown.permission", "ai:chat", "notes.read", "credentials.configure"]) {
    assert.equal(isV3PermissionAllowed(permission), false, permission);
  }
});
