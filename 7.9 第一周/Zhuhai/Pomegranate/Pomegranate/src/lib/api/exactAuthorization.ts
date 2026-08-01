import { invoke } from "@tauri-apps/api/core";

export type ExactAuthorizableResourceKind =
  | "credential"
  | "external-agent"
  | "workflow";

export type ExactAuthorizationStatus =
  | "missing"
  | "pending"
  | "granted"
  | "denied"
  | "revoked"
  | "expired";

export interface ExactAuthorizationResourceOption {
  resourceKind: ExactAuthorizableResourceKind;
  resourceId: string;
  displayName: string;
  compatibleCapabilities: string[];
}

export interface ExactAuthorizationCatalog {
  capabilityIds: string[];
  resources: ExactAuthorizationResourceOption[];
  maxDurationHours: number;
}

export interface ExactAuthorizationView {
  authorizationId: string | null;
  pluginId: string;
  capabilityId: string;
  resourceKind: ExactAuthorizableResourceKind | "agent-or-workflow";
  status: ExactAuthorizationStatus;
  effective: boolean;
  available: boolean | null;
  expiresAt: string | null;
}

export interface ExactAuthorizationGrantIntent {
  pluginId: string;
  capabilityId: string;
  resourceKind: ExactAuthorizableResourceKind;
  resourceId: string;
  expiresAt: string;
}

export interface ExactAuthorizationQueryIntent {
  pluginId: string;
  capabilityId: string;
  resourceKind: ExactAuthorizableResourceKind;
  resourceId: string;
}

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/**
 * exact scope 永远不进入 WebView；这个封装只发送最小资源意图和后端撤权句柄。
 */
export function createExactAuthorizationApi(invokeCommand: Invoke = invoke) {
  return {
    catalog: (pluginId: string) =>
      invokeCommand<ExactAuthorizationCatalog>("list_exact_authorization_catalog", {
        request: { pluginId },
      }),
    grant: (request: ExactAuthorizationGrantIntent) =>
      invokeCommand<ExactAuthorizationView>("grant_exact_resource_authorization", {
        request,
      }),
    query: (request: ExactAuthorizationQueryIntent) =>
      invokeCommand<ExactAuthorizationView>("query_exact_resource_authorization", {
        request,
      }),
    list: (pluginId: string) =>
      invokeCommand<ExactAuthorizationView[]>("list_exact_resource_authorizations", {
        request: { pluginId },
      }),
    revoke: (pluginId: string, authorizationId: string) =>
      invokeCommand<ExactAuthorizationView>("revoke_exact_resource_authorization", {
        request: { pluginId, authorizationId },
      }),
  };
}

export const exactAuthorizationApi = createExactAuthorizationApi();

export function resourcesForCapability(
  catalog: ExactAuthorizationCatalog,
  capabilityId: string,
): ExactAuthorizationResourceOption[] {
  return catalog.resources.filter((resource) =>
    resource.compatibleCapabilities.includes(capabilityId),
  );
}

export function resourceSelectionValue(resource: ExactAuthorizationResourceOption): string {
  return `${resource.resourceKind}\u0000${resource.resourceId}`;
}

export function expirationFromDuration(hours: 1 | 8 | 24, now = new Date()): string {
  return new Date(now.getTime() + hours * 60 * 60 * 1000).toISOString();
}
