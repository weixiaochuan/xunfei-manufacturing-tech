import assert from "node:assert/strict";
import test from "node:test";

import {
  createExactAuthorizationApi,
  expirationFromDuration,
  resourceSelectionValue,
  resourcesForCapability,
} from "./exactAuthorization.ts";

test("exact authorization API only sends minimal untrusted intent", async () => {
  const calls = [];
  const api = createExactAuthorizationApi(async (command, args) => {
    calls.push({ command, args });
    return command === "list_exact_resource_authorizations" ? [] : {};
  });

  await api.catalog("plugin-a");
  await api.grant({
    pluginId: "plugin-a",
    capabilityId: "credentials.use",
    resourceKind: "credential",
    resourceId: "credential-a",
    expiresAt: "2026-08-01T12:00:00.000Z",
  });
  await api.query({
    pluginId: "plugin-a",
    capabilityId: "credentials.use",
    resourceKind: "credential",
    resourceId: "credential-a",
  });
  await api.list("plugin-a");
  await api.revoke("plugin-a", "exact-auth-v1:2a");

  assert.deepEqual(calls, [
    {
      command: "list_exact_authorization_catalog",
      args: { request: { pluginId: "plugin-a" } },
    },
    {
      command: "grant_exact_resource_authorization",
      args: {
        request: {
          pluginId: "plugin-a",
          capabilityId: "credentials.use",
          resourceKind: "credential",
          resourceId: "credential-a",
          expiresAt: "2026-08-01T12:00:00.000Z",
        },
      },
    },
    {
      command: "query_exact_resource_authorization",
      args: {
        request: {
          pluginId: "plugin-a",
          capabilityId: "credentials.use",
          resourceKind: "credential",
          resourceId: "credential-a",
        },
      },
    },
    {
      command: "list_exact_resource_authorizations",
      args: { request: { pluginId: "plugin-a" } },
    },
    {
      command: "revoke_exact_resource_authorization",
      args: { request: { pluginId: "plugin-a", authorizationId: "exact-auth-v1:2a" } },
    },
  ]);

  for (const call of calls) {
    const serialized = JSON.stringify(call);
    for (const forbidden of [
      "subject",
      "owner",
      "installation",
      "scopeKind",
      "scopeKey",
      "canonicalHash",
      "parentAgentId",
      "createdAt",
      "updatedAt",
    ]) {
      assert.equal(serialized.includes(forbidden), false, forbidden);
    }
  }
});

test("resource matrix is driven by backend compatible capabilities", () => {
  const catalog = {
    capabilityIds: ["credentials.use", "agents.invoke", "network.xingchen"],
    maxDurationHours: 24,
    resources: [
      {
        resourceKind: "credential",
        resourceId: "shared",
        displayName: "凭据",
        compatibleCapabilities: ["credentials.use"],
      },
      {
        resourceKind: "external-agent",
        resourceId: "shared",
        displayName: "Agent",
        compatibleCapabilities: ["agents.invoke", "network.xingchen"],
      },
      {
        resourceKind: "workflow",
        resourceId: "shared",
        displayName: "Workflow",
        compatibleCapabilities: ["agents.invoke", "network.xingchen"],
      },
    ],
  };

  assert.deepEqual(
    resourcesForCapability(catalog, "credentials.use").map((item) => item.resourceKind),
    ["credential"],
  );
  assert.deepEqual(
    resourcesForCapability(catalog, "agents.invoke").map((item) => item.resourceKind),
    ["external-agent", "workflow"],
  );
  assert.deepEqual(
    resourcesForCapability(catalog, "network.xingchen").map((item) => item.resourceKind),
    ["external-agent", "workflow"],
  );
  assert.equal(catalog.resources.some((item) => item.resourceKind.includes("session")), false);
  assert.notEqual(
    resourceSelectionValue(catalog.resources[1]),
    resourceSelectionValue(catalog.resources[2]),
  );
});

test("expiration helper only produces controlled duration timestamps", () => {
  const now = new Date("2026-08-01T00:00:00.000Z");
  assert.equal(expirationFromDuration(1, now), "2026-08-01T01:00:00.000Z");
  assert.equal(expirationFromDuration(8, now), "2026-08-01T08:00:00.000Z");
  assert.equal(expirationFromDuration(24, now), "2026-08-02T00:00:00.000Z");
});
