import { invoke } from "@tauri-apps/api/core";

export type FeatureAuthorizationStatus = "missing" | "pending" | "granted" | "denied" | "revoked" | "expired";
export interface FeatureAuthorizationView {
  capabilityId: "ai.context.augment" | "ai.invoke";
  targetKind: "enhancement" | "xingchenFeature";
  contributionId: string;
  title: string;
  hook: string;
  scenes: string[];
  features: string[];
  status: FeatureAuthorizationStatus;
  effective: boolean;
  expiresAt: string | null;
}
type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

export function createFeatureAuthorizationApi(invokeCommand: Invoke = invoke) {
  const mutation = (command: string, pluginId: string, contributionId: string, expiresAt?: string) =>
    invokeCommand<FeatureAuthorizationView>(command, {
      request: { pluginId, contributionId, ...(expiresAt ? { expiresAt } : {}) },
    });
  return {
    list: (pluginId: string) => invokeCommand<FeatureAuthorizationView[]>("list_declarative_feature_authorizations", { request: { pluginId } }),
    query: (pluginId: string, contributionId: string) => invokeCommand<FeatureAuthorizationView>("query_declarative_feature_authorization", { request: { pluginId, contributionId } }),
    request: (pluginId: string, contributionId: string, expiresAt: string) => mutation("request_declarative_feature_authorization", pluginId, contributionId, expiresAt),
    grant: (pluginId: string, contributionId: string, expiresAt: string) => mutation("grant_declarative_feature_authorization", pluginId, contributionId, expiresAt),
    deny: (pluginId: string, contributionId: string) => mutation("deny_declarative_feature_authorization", pluginId, contributionId),
    revoke: (pluginId: string, contributionId: string) => mutation("revoke_declarative_feature_authorization", pluginId, contributionId),
    expire: (pluginId: string, contributionId: string) => mutation("expire_declarative_feature_authorization", pluginId, contributionId),
  };
}
export const featureAuthorizationApi = createFeatureAuthorizationApi();
