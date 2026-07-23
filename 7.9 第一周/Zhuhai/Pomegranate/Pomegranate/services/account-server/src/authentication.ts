import type { SessionService, SessionUser } from "./sessions.js";

export class InvalidSessionError extends Error {
  constructor() {
    super("invalid_session");
    this.name = "InvalidSessionError";
  }
}

export function readBearerToken(authorization: unknown): string | null {
  if (typeof authorization !== "string") {
    return null;
  }
  return /^Bearer ([A-Za-z0-9_-]{43,512})$/.exec(authorization)?.[1] ?? null;
}

export async function requirePlatformUser(
  authorization: unknown,
  sessionService: SessionService,
): Promise<SessionUser> {
  const token = readBearerToken(authorization);
  if (!token) {
    throw new InvalidSessionError();
  }

  const user = await sessionService.findActive(token);
  if (!user) {
    throw new InvalidSessionError();
  }
  return user;
}
