import {
  createRemoteJWKSet,
  jwtVerify,
  type JWTPayload,
  type JWTVerifyGetKey,
} from "jose";
import type { OidcConfig } from "./config.js";

const REQUEST_TIMEOUT_MS = 5_000;
const LOOPBACK_HOSTS = new Set(["127.0.0.1", "localhost"]);

export interface OidcDiscovery {
  issuer: string;
  authorizationEndpoint: string;
  tokenEndpoint: string;
  userinfoEndpoint: string;
  jwksUri: string;
}

export interface OidcTokenResult {
  accessToken: string;
  idToken: string;
}

export interface VerifiedIdToken {
  subject: string;
  organization: string;
  organizationClaim: "owner" | "organization" | "organizations";
  username: string;
  displayName: string | null;
  email: string | null;
  claimTypes: Record<string, string>;
}

export interface OidcUserInfo {
  sub?: unknown;
  owner?: unknown;
  organization?: unknown;
  organizations?: unknown;
  preferred_username?: unknown;
  username?: unknown;
  name?: unknown;
  email?: unknown;
  [claim: string]: unknown;
}

export interface OidcClient {
  discover(): Promise<OidcDiscovery>;
  getAuthorizationUrl(state: string): Promise<URL>;
  exchangeCode(code: string): Promise<OidcTokenResult>;
  verifyIdToken(idToken: string): Promise<VerifiedIdToken>;
  getUserInfo(accessToken: string): Promise<OidcUserInfo>;
}

type FetchImplementation = typeof fetch;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readString(record: Record<string, unknown>, name: string): string {
  const value = record[name];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error("OIDC 响应缺少必需字段");
  }
  return value;
}

function readOptionalClaimString(record: Record<string, unknown>, name: string): string | null {
  const value = record[name];
  if (value === undefined || value === null || value === "") {
    return null;
  }
  if (typeof value !== "string") {
    throw new Error("OIDC ID Token 用户资料字段类型无效");
  }
  return value;
}

function effectivePort(url: URL): string {
  if (url.port) {
    return url.port;
  }
  return url.protocol === "https:" ? "443" : "80";
}

function isTrustedCasdoorUrl(baseUrl: URL, candidate: URL): boolean {
  const sameHost = baseUrl.hostname === candidate.hostname;
  const loopbackAlias =
    LOOPBACK_HOSTS.has(baseUrl.hostname) && LOOPBACK_HOSTS.has(candidate.hostname);

  return (
    candidate.protocol === baseUrl.protocol &&
    effectivePort(candidate) === effectivePort(baseUrl) &&
    (sameHost || loopbackAlias) &&
    !candidate.username &&
    !candidate.password &&
    !candidate.hash
  );
}

function validateEndpoint(baseUrl: URL, value: string): string {
  let endpoint: URL;
  try {
    endpoint = new URL(value);
  } catch {
    throw new Error("OIDC Discovery 返回了无效端点");
  }

  if (!isTrustedCasdoorUrl(baseUrl, endpoint)) {
    throw new Error("OIDC Discovery 返回了不受信任的端点");
  }
  return endpoint.toString();
}

async function readJson(response: Response): Promise<Record<string, unknown>> {
  const value: unknown = await response.json();
  if (!isRecord(value)) {
    throw new Error("OIDC 响应不是 JSON 对象");
  }
  return value;
}

function describeClaimType(value: unknown): string {
  if (value === null) {
    return "null";
  }
  if (Array.isArray(value)) {
    const elementTypes = [...new Set(value.map((item) => describeClaimType(item)))];
    return `array<${elementTypes.join("|") || "empty"}>`;
  }
  return typeof value;
}

function describeClaimTypes(payload: JWTPayload): Record<string, string> {
  const types: Record<string, string> = {};
  for (const [name, value] of Object.entries(payload)) {
    types[name] = describeClaimType(value);
  }
  return types;
}

function findOrganizationClaim(
  payload: JWTPayload,
  expectedOrganization: string,
): VerifiedIdToken["organizationClaim"] | null {
  if (payload.owner === expectedOrganization) {
    return "owner";
  }
  if (payload.organization === expectedOrganization) {
    return "organization";
  }
  if (
    Array.isArray(payload.organizations) &&
    payload.organizations.some((value) => value === expectedOrganization)
  ) {
    return "organizations";
  }
  return null;
}

export class OidcOrganizationError extends Error {
  constructor(readonly claimTypes: Record<string, string>) {
    super("OIDC ID Token 未明确证明用户属于所需组织");
    this.name = "OidcOrganizationError";
  }
}

export async function verifyIdTokenWithKey(
  idToken: string,
  key: JWTVerifyGetKey,
  expected: { issuer: string; audience: string; organization: string },
): Promise<VerifiedIdToken> {
  const { payload, protectedHeader } = await jwtVerify(idToken, key, {
    algorithms: ["RS256"],
    issuer: expected.issuer,
    audience: expected.audience,
    requiredClaims: ["iss", "sub", "aud", "exp", "iat"],
  });

  if (protectedHeader.typ !== undefined && protectedHeader.typ !== "JWT") {
    throw new Error("OIDC ID Token typ 无效");
  }
  if (typeof payload.sub !== "string" || payload.sub.length === 0) {
    throw new Error("OIDC ID Token sub 无效");
  }

  const claimTypes = describeClaimTypes(payload);
  const organizationClaim = findOrganizationClaim(payload, expected.organization);
  if (!organizationClaim) {
    throw new OidcOrganizationError(claimTypes);
  }

  return {
    subject: payload.sub,
    organization: expected.organization,
    organizationClaim,
    username: readString(payload, "name"),
    displayName: readOptionalClaimString(payload, "displayName"),
    email: readOptionalClaimString(payload, "email"),
    claimTypes,
  };
}

export function createOidcClient(
  config: OidcConfig,
  fetchImplementation: FetchImplementation = fetch,
): OidcClient {
  const baseUrl = new URL(config.baseUrl);
  let discoveryPromise: Promise<OidcDiscovery> | undefined;
  let remoteJwks: ReturnType<typeof createRemoteJWKSet> | undefined;
  let remoteJwksUri: string | undefined;

  const discover = async (): Promise<OidcDiscovery> => {
    if (!discoveryPromise) {
      discoveryPromise = (async () => {
        const discoveryUrl = new URL("/.well-known/openid-configuration", baseUrl);
        const response = await fetchImplementation(discoveryUrl, {
          headers: { accept: "application/json" },
          redirect: "error",
          signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
        });
        if (!response.ok) {
          throw new Error("OIDC Discovery 请求失败");
        }

        const document = await readJson(response);
        const issuer = readString(document, "issuer");
        validateEndpoint(baseUrl, issuer);
        const authorizationEndpoint = validateEndpoint(
          baseUrl,
          readString(document, "authorization_endpoint"),
        );
        const tokenEndpoint = validateEndpoint(
          baseUrl,
          readString(document, "token_endpoint"),
        );
        const userinfoEndpoint = validateEndpoint(
          baseUrl,
          readString(document, "userinfo_endpoint"),
        );
        const jwksUri = validateEndpoint(
          baseUrl,
          readString(document, "jwks_uri"),
        );

        return {
          issuer,
          authorizationEndpoint,
          tokenEndpoint,
          userinfoEndpoint,
          jwksUri,
        };
      })().catch((error: unknown) => {
        discoveryPromise = undefined;
        throw error;
      });
    }
    return discoveryPromise;
  };

  return {
    discover,

    async getAuthorizationUrl(state: string): Promise<URL> {
      const discovery = await discover();
      const url = new URL(discovery.authorizationEndpoint);
      url.searchParams.set("client_id", config.clientId);
      url.searchParams.set("response_type", "code");
      url.searchParams.set("redirect_uri", config.redirectUri);
      url.searchParams.set("scope", "openid profile email");
      url.searchParams.set("state", state);
      return url;
    },

    async exchangeCode(code: string): Promise<OidcTokenResult> {
      const discovery = await discover();
      const body = new URLSearchParams({
        grant_type: "authorization_code",
        client_id: config.clientId,
        client_secret: config.clientSecret,
        code,
        redirect_uri: config.redirectUri,
      });
      const response = await fetchImplementation(discovery.tokenEndpoint, {
        method: "POST",
        headers: {
          accept: "application/json",
          "content-type": "application/x-www-form-urlencoded",
        },
        body,
        redirect: "error",
        signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
      });
      if (!response.ok) {
        throw new Error("OIDC 令牌交换失败");
      }

      const token = await readJson(response);
      const tokenType = readString(token, "token_type");
      if (tokenType.toLowerCase() !== "bearer") {
        throw new Error("OIDC 令牌类型无效");
      }
      return {
        accessToken: readString(token, "access_token"),
        idToken: readString(token, "id_token"),
      };
    },

    async verifyIdToken(idToken: string): Promise<VerifiedIdToken> {
      const discovery = await discover();
      if (!remoteJwks || remoteJwksUri !== discovery.jwksUri) {
        remoteJwks = createRemoteJWKSet(new URL(discovery.jwksUri), {
          timeoutDuration: REQUEST_TIMEOUT_MS,
          cooldownDuration: 30_000,
        });
        remoteJwksUri = discovery.jwksUri;
      }
      return verifyIdTokenWithKey(idToken, remoteJwks, {
        issuer: discovery.issuer,
        audience: config.clientId,
        organization: config.organization,
      });
    },

    async getUserInfo(accessToken: string): Promise<OidcUserInfo> {
      const discovery = await discover();
      const response = await fetchImplementation(discovery.userinfoEndpoint, {
        headers: {
          accept: "application/json",
          authorization: `Bearer ${accessToken}`,
        },
        redirect: "error",
        signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
      });
      if (!response.ok) {
        throw new Error("OIDC UserInfo 请求失败");
      }
      return (await readJson(response)) as OidcUserInfo;
    },
  };
}
