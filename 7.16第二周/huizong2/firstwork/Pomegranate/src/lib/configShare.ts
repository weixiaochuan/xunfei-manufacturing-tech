import { aiModelApi, configApi, syncV1Api } from "@/lib/api";
import { useAppStore, type ActiveView, type MobileDashboardItem, type MobileTabKey } from "@/store";
import type { AiModel, AiModelInput, SyncBackend, SyncBackendInput, WebDavConfig } from "@/types";

type EnvelopeKind = "ai-model" | "webdav-backend" | "feature-toggles";

interface PlainEnvelopeBase<K extends EnvelopeKind, D> {
  kbConfig: "v1";
  kind: K;
  version: 1;
  data: D;
}

export interface AiModelShareData extends AiModelInput {}

export interface WebDavBackendShareData {
  name: string;
  kind: "webdav";
  config: WebDavConfig;
}

export interface FeatureTogglesShareData {
  enabledViews?: ActiveView[];
  mobileDashboardItems?: MobileDashboardItem[];
  mobileTabKeys?: MobileTabKey[];
}

export type Envelope =
  | PlainEnvelopeBase<"ai-model", AiModelShareData>
  | PlainEnvelopeBase<"webdav-backend", WebDavBackendShareData>
  | PlainEnvelopeBase<"feature-toggles", FeatureTogglesShareData>;

interface EncryptedEnvelope {
  kbConfig: "v1-enc";
  version: 1;
  algo: "AES-GCM-256";
  kdf: "PBKDF2";
  iterations: number;
  salt: string;
  iv: string;
  cipher: string;
}

export interface ParseEnvelopeSuccess {
  ok: true;
  envelope: Envelope;
}

export interface ParseEnvelopeFailure {
  ok: false;
  reason: string;
  encrypted?: boolean;
}

export type ParseEnvelopeResult = ParseEnvelopeSuccess | ParseEnvelopeFailure;

export interface ApplyEnvelopeSummary {
  aiModels: number;
  webdavBackends: number;
  featureToggles: boolean;
  errors: string[];
}

const PBKDF2_ITERATIONS = 100_000;
const ENVELOPE_VERSION = 1;
const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

export const KIND_LABELS: Record<EnvelopeKind, string> = {
  "ai-model": "AI 模型",
  "webdav-backend": "WebDAV 同步",
  "feature-toggles": "功能开关",
};

function randomBytes(length: number) {
  const bytes = new Uint8Array(length);
  crypto.getRandomValues(bytes);
  return bytes;
}

function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

function fromBase64(value: string): Uint8Array {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

async function deriveKey(pin: string, salt: Uint8Array) {
  const baseKey = await crypto.subtle.importKey(
    "raw",
    textEncoder.encode(pin),
    { name: "PBKDF2" },
    false,
    ["deriveKey"],
  );

  return crypto.subtle.deriveKey(
    {
      name: "PBKDF2",
      salt,
      iterations: PBKDF2_ITERATIONS,
      hash: "SHA-256",
    },
    baseKey,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"],
  );
}

function normalizeAiModelPayload(payload: AiModel | AiModelInput): AiModelShareData {
  return {
    name: payload.name,
    provider: payload.provider,
    protocol: "protocol" in payload ? payload.protocol : undefined,
    api_url: payload.api_url,
    api_key: payload.api_key ?? null,
    model_id: payload.model_id,
    max_context: payload.max_context,
    supports_tools: payload.supports_tools,
    supports_vision: payload.supports_vision,
    max_output_tokens: payload.max_output_tokens,
  };
}

function normalizeWebDavBackendPayload(payload: SyncBackend | WebDavBackendShareData): WebDavBackendShareData {
  if ("config" in payload) return payload;

  let config: WebDavConfig = { url: "", username: "" };
  try {
    config = JSON.parse(payload.configJson) as WebDavConfig;
  } catch {
    config = { url: "", username: "" };
  }

  return {
    name: payload.name,
    kind: "webdav",
    config,
  };
}

function ensureEnvelope(value: unknown): Envelope {
  if (!value || typeof value !== "object") {
    throw new Error("配置内容不是有效对象");
  }

  const record = value as Record<string, unknown>;
  const kind = record.kind;
  const version = record.version;
  const rawData = (record.data ?? record.payload) as unknown;

  if (kind !== "ai-model" && kind !== "webdav-backend" && kind !== "feature-toggles") {
    throw new Error("不支持的配置类型");
  }
  if (version !== ENVELOPE_VERSION) {
    throw new Error(`不支持的配置版本: ${String(version)}`);
  }

  return {
    kbConfig: "v1",
    kind,
    version: ENVELOPE_VERSION,
    data: rawData as Envelope["data"],
  } as Envelope;
}

export function exportAiModel(payload: AiModel | AiModelInput): Envelope {
  return {
    kbConfig: "v1",
    kind: "ai-model",
    version: ENVELOPE_VERSION,
    data: normalizeAiModelPayload(payload),
  };
}

export function exportWebDavBackend(payload: SyncBackend | WebDavBackendShareData): Envelope {
  return {
    kbConfig: "v1",
    kind: "webdav-backend",
    version: ENVELOPE_VERSION,
    data: normalizeWebDavBackendPayload(payload),
  };
}

export function stringifyEnvelope(envelope: Envelope, pretty = false): string {
  return JSON.stringify(envelope, null, pretty ? 2 : 0);
}

export async function stringifyEncrypted(
  envelope: Envelope,
  pin: string,
  pretty = false,
): Promise<string> {
  const normalizedPin = pin.trim();
  if (!normalizedPin) {
    throw new Error("加密 PIN 不能为空");
  }

  const salt = randomBytes(16);
  const iv = randomBytes(12);
  const key = await deriveKey(normalizedPin, salt);
  const plaintext = textEncoder.encode(stringifyEnvelope(envelope, false));
  const encrypted = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, plaintext);

  const payload: EncryptedEnvelope = {
    kbConfig: "v1-enc",
    version: ENVELOPE_VERSION,
    algo: "AES-GCM-256",
    kdf: "PBKDF2",
    iterations: PBKDF2_ITERATIONS,
    salt: toBase64(salt),
    iv: toBase64(iv),
    cipher: toBase64(new Uint8Array(encrypted)),
  };

  return JSON.stringify(payload, null, pretty ? 2 : 0);
}

export async function parseEnvelope(value: string, pin?: string): Promise<ParseEnvelopeResult> {
  const trimmed = value.trim();
  if (!trimmed) {
    return { ok: false, reason: "配置内容为空" };
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch (error) {
    return { ok: false, reason: `JSON 解析失败: ${(error as Error).message}` };
  }

  const record = parsed as Record<string, unknown>;
  if (record.kbConfig === "v1-enc") {
    if (!pin?.trim()) {
      return { ok: false, encrypted: true, reason: "需要 PIN 才能解密" };
    }

    try {
      const salt = fromBase64(String(record.salt ?? ""));
      const iv = fromBase64(String(record.iv ?? ""));
      const cipher = fromBase64(String(record.cipher ?? ""));
      const key = await deriveKey(pin.trim(), salt);
      const decrypted = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, key, cipher);
      const plaintext = textDecoder.decode(new Uint8Array(decrypted));
      return { ok: true, envelope: ensureEnvelope(JSON.parse(plaintext)) };
    } catch {
      return { ok: false, encrypted: true, reason: "PIN 错误或数据已损坏" };
    }
  }

  try {
    return { ok: true, envelope: ensureEnvelope(parsed) };
  } catch (error) {
    return { ok: false, reason: (error as Error).message };
  }
}

export const encodeEnvelope = stringifyEnvelope;
export const decodeEnvelope = parseEnvelope;

async function applyAiModelEnvelope(data: AiModelShareData): Promise<number> {
  const payload = normalizeAiModelPayload(data);
  const existing = await aiModelApi.list();
  const matched = existing.find(
    (item) =>
      item.provider === payload.provider &&
      item.api_url === payload.api_url &&
      item.model_id === payload.model_id,
  );

  if (matched) {
    await aiModelApi.update(matched.id, payload);
  } else {
    await aiModelApi.create(payload);
  }

  return 1;
}

async function applyWebDavEnvelope(data: WebDavBackendShareData): Promise<number> {
  const backends = await syncV1Api.listBackends();
  const input: SyncBackendInput = {
    kind: "webdav",
    name: data.name,
    configJson: JSON.stringify(data.config),
  };

  const matched = backends.find((item) => {
    if (item.kind !== "webdav") return false;
    try {
      const config = JSON.parse(item.configJson) as WebDavConfig;
      return config.url === data.config.url && config.username === data.config.username;
    } catch {
      return false;
    }
  });

  if (matched) {
    await syncV1Api.updateBackend(matched.id, input);
  } else {
    await syncV1Api.createBackend(input);
  }

  return 1;
}

async function applyFeatureTogglesEnvelope(data: FeatureTogglesShareData): Promise<boolean> {
  if (data.enabledViews) {
    await configApi.set("enabled_views", JSON.stringify(data.enabledViews));
  }
  if (data.mobileDashboardItems) {
    await configApi.set("mobile_dashboard_items", JSON.stringify(data.mobileDashboardItems));
  }
  if (data.mobileTabKeys) {
    await configApi.set("mobile_tab_keys", JSON.stringify(data.mobileTabKeys));
  }

  const store = useAppStore.getState();
  await Promise.all([
    store.loadEnabledViews(),
    store.loadMobileDashboardItems(),
    store.loadMobileTabKeys(),
  ]);

  return Boolean(data.enabledViews || data.mobileDashboardItems || data.mobileTabKeys);
}

export async function applyEnvelope(envelope: Envelope): Promise<ApplyEnvelopeSummary> {
  const summary: ApplyEnvelopeSummary = {
    aiModels: 0,
    webdavBackends: 0,
    featureToggles: false,
    errors: [],
  };

  try {
    if (envelope.kind === "ai-model") {
      summary.aiModels = await applyAiModelEnvelope(envelope.data);
    } else if (envelope.kind === "webdav-backend") {
      summary.webdavBackends = await applyWebDavEnvelope(envelope.data);
    } else if (envelope.kind === "feature-toggles") {
      summary.featureToggles = await applyFeatureTogglesEnvelope(envelope.data);
    }
  } catch (error) {
    summary.errors.push((error as Error).message ?? String(error));
  }

  return summary;
}
