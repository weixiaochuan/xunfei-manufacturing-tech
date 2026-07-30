import assert from "node:assert/strict";
import test from "node:test";
import {
  SignJWT,
  createLocalJWKSet,
  exportJWK,
  generateKeyPair,
  type JWTPayload,
} from "jose";
import {
  OidcOrganizationError,
  verifyIdTokenWithKey,
} from "../src/oidc.js";

const ISSUER = "http://localhost:8000";
const AUDIENCE = "test-client-id";
const ORGANIZATION = "pomegranate";
const NOW = Math.floor(Date.now() / 1_000);

const primaryKeys = await generateKeyPair("RS256", { extractable: true });
const primaryJwk = await exportJWK(primaryKeys.publicKey);
primaryJwk.kid = "primary";
primaryJwk.alg = "RS256";
primaryJwk.use = "sig";
const primaryJwks = createLocalJWKSet({ keys: [primaryJwk] });

interface TokenOptions {
  claims?: JWTPayload;
  issuer?: string;
  audience?: string;
  expirationTime?: number;
  notBefore?: number;
  signingKey?: CryptoKey;
  typ?: string;
}

async function signIdToken(options: TokenOptions = {}): Promise<string> {
  const token = new SignJWT(options.claims ?? { owner: ORGANIZATION })
    .setProtectedHeader({ alg: "RS256", kid: "primary", typ: options.typ ?? "JWT" })
    .setIssuer(options.issuer ?? ISSUER)
    .setSubject("stable-user-id")
    .setAudience(options.audience ?? AUDIENCE)
    .setIssuedAt(NOW)
    .setExpirationTime(options.expirationTime ?? NOW + 300);
  if (options.notBefore !== undefined) {
    token.setNotBefore(options.notBefore);
  }
  return token.sign(options.signingKey ?? primaryKeys.privateKey);
}

async function verify(token: string) {
  return verifyIdTokenWithKey(token, primaryJwks, {
    issuer: ISSUER,
    audience: AUDIENCE,
    organization: ORGANIZATION,
  });
}

test("accepts a valid RS256 ID Token for pomegranate", async () => {
  const verified = await verify(
    await signIdToken({
      claims: {
        owner: ORGANIZATION,
        name: "alice",
        displayName: "Alice",
        email: "alice@example.test",
      },
    }),
  );
  assert.deepEqual(
    {
      subject: verified.subject,
      organization: verified.organization,
      organizationClaim: verified.organizationClaim,
      username: verified.username,
      displayName: verified.displayName,
      email: verified.email,
    },
    {
      subject: "stable-user-id",
      organization: ORGANIZATION,
      organizationClaim: "owner",
      username: "alice",
      displayName: "Alice",
      email: "alice@example.test",
    },
  );
});

test("rejects a signed token for another organization", async () => {
  const token = await signIdToken({
    claims: { owner: "another-organization", name: "alice" },
  });
  await assert.rejects(() => verify(token), OidcOrganizationError);
});

test("rejects a signed token without an organization claim", async () => {
  const token = await signIdToken({ claims: { name: "alice" } });
  await assert.rejects(
    () => verify(token),
    (error: unknown) => {
      assert.ok(error instanceof OidcOrganizationError);
      assert.equal(error.claimTypes.owner, undefined);
      assert.equal(error.claimTypes.organization, undefined);
      assert.equal(error.claimTypes.organizations, undefined);
      return true;
    },
  );
});

test("rejects an ID Token signed by an untrusted key", async () => {
  const otherKeys = await generateKeyPair("RS256");
  const token = await signIdToken({
    signingKey: otherKeys.privateKey,
    claims: { owner: ORGANIZATION, name: "alice" },
  });
  await assert.rejects(() => verify(token));
});

test("rejects an ID Token with the wrong issuer", async () => {
  const token = await signIdToken({
    issuer: "http://untrusted.example",
    claims: { owner: ORGANIZATION, name: "alice" },
  });
  await assert.rejects(() => verify(token));
});

test("rejects an ID Token with the wrong audience", async () => {
  const token = await signIdToken({
    audience: "another-client",
    claims: { owner: ORGANIZATION, name: "alice" },
  });
  await assert.rejects(() => verify(token));
});

test("rejects an expired ID Token", async () => {
  const token = await signIdToken({
    expirationTime: NOW - 60,
    claims: { owner: ORGANIZATION, name: "alice" },
  });
  await assert.rejects(() => verify(token));
});

test("allows a future nbf only within an explicit tolerance while keeping exp strict", async () => {
  const token = await signIdToken({
    notBefore: NOW + 28_800,
    expirationTime: NOW + 29_100,
    claims: { owner: ORGANIZATION, name: "alice" },
  });
  await verifyIdTokenWithKey(token, primaryJwks, {
    issuer: ISSUER,
    audience: AUDIENCE,
    organization: ORGANIZATION,
    nbfClockToleranceSeconds: 28_860,
  });

  await assert.rejects(() =>
    verifyIdTokenWithKey(token, primaryJwks, {
      issuer: ISSUER,
      audience: AUDIENCE,
      organization: ORGANIZATION,
      nbfClockToleranceSeconds: 60,
    }),
  );

  const expiredToken = await signIdToken({
    notBefore: NOW - 60,
    expirationTime: NOW - 1,
    claims: { owner: ORGANIZATION, name: "alice" },
  });
  await assert.rejects(() =>
    verifyIdTokenWithKey(expiredToken, primaryJwks, {
      issuer: ISSUER,
      audience: AUDIENCE,
      organization: ORGANIZATION,
      nbfClockToleranceSeconds: 28_860,
    }),
  );
});

test("rejects an ID Token with an invalid explicit typ", async () => {
  const token = await signIdToken({
    typ: "at+jwt",
    claims: { owner: ORGANIZATION, name: "alice" },
  });
  await assert.rejects(() => verify(token));
});
