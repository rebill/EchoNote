import { readFile } from "fs/promises";
import { homedir } from "os";
import { resolve } from "path";
import type { EchoNoteSettings } from "../settings/settings";
import { DEFAULT_COMPANION_DISCOVERY_MAX_AGE_SECONDS } from "../settings/settings";
import { AsrServiceClient } from "./asr-service-client";

export type CompanionServiceStatus = "stopped" | "starting" | "running" | "stopping" | "error";
export type CompanionBackend = "fake" | "mlx-audio";
export type CompanionModelStatus = "not_loaded" | "loading" | "ready" | "error" | "unknown";

export type CompanionDiscovery = {
  version: 1;
  app: "EchoNote ASR Companion";
  service: "echonote-asr";
  status: CompanionServiceStatus;
  baseUrl: string;
  host: "127.0.0.1";
  port: number;
  backend: CompanionBackend;
  modelId: string;
  modelStatus: CompanionModelStatus;
  pid: number | null;
  updatedAt: string;
};

export type CompanionResolution =
  | { kind: "available"; baseUrl: string; discovery: CompanionDiscovery }
  | { kind: "missing"; reason: string }
  | { kind: "invalid"; reason: string }
  | { kind: "not_running"; reason: string; status: CompanionServiceStatus }
  | { kind: "stale"; reason: string }
  | { kind: "unavailable"; reason: string };

export type ResolveCompanionDiscoveryOptions = {
  discoveryPath: string;
  maxAgeSeconds: number;
  now?: () => Date;
  healthCheck?: (baseUrl: string) => Promise<void>;
};

type ValidationResult =
  | { ok: true; discovery: CompanionDiscovery }
  | { ok: false; reason: string };

const COMPANION_DISCOVERY_KEYS = [
  "version",
  "app",
  "service",
  "status",
  "baseUrl",
  "host",
  "port",
  "backend",
  "modelId",
  "modelStatus",
  "pid",
  "updatedAt"
] as const;

const COMPANION_SERVICE_STATUSES = new Set<CompanionServiceStatus>([
  "stopped",
  "starting",
  "running",
  "stopping",
  "error"
]);
const COMPANION_BACKENDS = new Set<CompanionBackend>(["fake", "mlx-audio"]);
const COMPANION_MODEL_STATUSES = new Set<CompanionModelStatus>([
  "not_loaded",
  "loading",
  "ready",
  "error",
  "unknown"
]);

export async function resolveCompanionDiscovery(
  settings: Pick<EchoNoteSettings, "companionDiscoveryPath" | "companionDiscoveryMaxAgeSeconds">
): Promise<CompanionResolution> {
  return resolveCompanionDiscoveryFile({
    discoveryPath: settings.companionDiscoveryPath,
    maxAgeSeconds: settings.companionDiscoveryMaxAgeSeconds
  });
}

export async function resolveCompanionDiscoveryFile(
  options: ResolveCompanionDiscoveryOptions
): Promise<CompanionResolution> {
  const discoveryPath = expandHomePath(options.discoveryPath);
  const content = await readDiscoveryFile(discoveryPath);
  if (!content.ok) {
    return content.resolution;
  }

  const parsed = parseCompanionDiscovery(content.value);
  if (!parsed.ok) {
    return { kind: "invalid", reason: parsed.reason };
  }

  const { discovery } = parsed;
  if (discovery.status !== "running") {
    return {
      kind: "not_running",
      status: discovery.status,
      reason: `Companion service is ${discovery.status}.`
    };
  }

  const maxAgeSeconds = normalizeMaxAgeSeconds(options.maxAgeSeconds);
  const now = options.now?.() ?? new Date();
  if (isDiscoveryStale(discovery.updatedAt, now, maxAgeSeconds)) {
    return {
      kind: "stale",
      reason: `Companion discovery is older than ${maxAgeSeconds} seconds.`
    };
  }

  const endpointValidation = validateEndpointConsistency(discovery);
  if (!endpointValidation.ok) {
    return { kind: "invalid", reason: endpointValidation.reason };
  }

  try {
    await (options.healthCheck ?? checkCompanionHealth)(discovery.baseUrl);
  } catch (error) {
    return {
      kind: "unavailable",
      reason: error instanceof Error ? error.message : String(error)
    };
  }

  return { kind: "available", baseUrl: discovery.baseUrl, discovery };
}

export function parseCompanionDiscovery(content: string): ValidationResult {
  let value: unknown;
  try {
    value = JSON.parse(content);
  } catch (error) {
    return {
      ok: false,
      reason: `Discovery JSON is invalid: ${error instanceof Error ? error.message : String(error)}`
    };
  }

  return validateCompanionDiscovery(value);
}

export function validateCompanionDiscovery(value: unknown): ValidationResult {
  if (!isRecord(value)) {
    return { ok: false, reason: "Discovery must be a JSON object." };
  }

  const keyValidation = validateExactKeys(value);
  if (!keyValidation.ok) {
    return keyValidation;
  }

  if (value.version !== 1) {
    return { ok: false, reason: "Discovery version must be 1." };
  }
  if (value.app !== "EchoNote ASR Companion") {
    return { ok: false, reason: "Discovery app must be EchoNote ASR Companion." };
  }
  if (value.service !== "echonote-asr") {
    return { ok: false, reason: "Discovery service must be echonote-asr." };
  }
  if (!isCompanionServiceStatus(value.status)) {
    return { ok: false, reason: "Discovery status is invalid." };
  }
  if (typeof value.baseUrl !== "string" || !/^http:\/\/127\.0\.0\.1:[0-9]{1,5}$/.test(value.baseUrl)) {
    return { ok: false, reason: "Discovery baseUrl must be http://127.0.0.1:<port>." };
  }
  if (value.host !== "127.0.0.1") {
    return { ok: false, reason: "Discovery host must be 127.0.0.1." };
  }
  const port = value.port;
  if (typeof port !== "number" || !Number.isInteger(port) || port < 1 || port > 65535) {
    return { ok: false, reason: "Discovery port must be an integer from 1 to 65535." };
  }
  if (!isCompanionBackend(value.backend)) {
    return { ok: false, reason: "Discovery backend is invalid." };
  }
  if (typeof value.modelId !== "string" || value.modelId.trim().length === 0) {
    return { ok: false, reason: "Discovery modelId must be a non-empty string." };
  }
  if (!isCompanionModelStatus(value.modelStatus)) {
    return { ok: false, reason: "Discovery modelStatus is invalid." };
  }
  const pid = value.pid;
  if (pid !== null && (typeof pid !== "number" || !Number.isInteger(pid) || pid < 1)) {
    return { ok: false, reason: "Discovery pid must be null or a positive integer." };
  }
  if (typeof value.updatedAt !== "string" || Number.isNaN(Date.parse(value.updatedAt))) {
    return { ok: false, reason: "Discovery updatedAt must be a valid ISO timestamp." };
  }

  return { ok: true, discovery: value as CompanionDiscovery };
}

export function expandHomePath(filePath: string): string {
  const trimmedPath = filePath.trim();
  if (trimmedPath === "~") {
    return homedir();
  }
  if (trimmedPath.startsWith("~/")) {
    return resolve(homedir(), trimmedPath.slice(2));
  }
  return trimmedPath;
}

export function isDiscoveryStale(updatedAt: string, now: Date, maxAgeSeconds: number): boolean {
  const updatedAtMs = Date.parse(updatedAt);
  if (Number.isNaN(updatedAtMs)) {
    return true;
  }

  const ageMs = Math.abs(now.getTime() - updatedAtMs);
  return ageMs > maxAgeSeconds * 1000;
}

async function readDiscoveryFile(
  discoveryPath: string
): Promise<{ ok: true; value: string } | { ok: false; resolution: CompanionResolution }> {
  try {
    return { ok: true, value: await readFile(discoveryPath, "utf8") };
  } catch (error) {
    const nodeError = error as NodeJS.ErrnoException;
    if (nodeError.code === "ENOENT") {
      return { ok: false, resolution: { kind: "missing", reason: `Discovery file not found at ${discoveryPath}.` } };
    }
    return {
      ok: false,
      resolution: {
        kind: "invalid",
        reason: `Discovery file could not be read: ${error instanceof Error ? error.message : String(error)}`
      }
    };
  }
}

function validateEndpointConsistency(discovery: CompanionDiscovery): { ok: true } | { ok: false; reason: string } {
  const match = /^http:\/\/127\.0\.0\.1:([0-9]{1,5})$/.exec(discovery.baseUrl);
  if (!match) {
    return { ok: false, reason: "Discovery baseUrl must be http://127.0.0.1:<port>." };
  }

  const baseUrlPort = Number.parseInt(match[1] ?? "", 10);
  if (baseUrlPort !== discovery.port) {
    return { ok: false, reason: "Discovery baseUrl port does not match port." };
  }

  return { ok: true };
}

async function checkCompanionHealth(baseUrl: string): Promise<void> {
  const health = await new AsrServiceClient(baseUrl).health();
  if (health.status !== "ok") {
    throw new Error("Companion ASR health check did not return ok.");
  }
}

function normalizeMaxAgeSeconds(maxAgeSeconds: number): number {
  if (!Number.isFinite(maxAgeSeconds) || maxAgeSeconds <= 0) {
    return DEFAULT_COMPANION_DISCOVERY_MAX_AGE_SECONDS;
  }
  return maxAgeSeconds;
}

function validateExactKeys(value: Record<string, unknown>): { ok: true } | { ok: false; reason: string } {
  const expectedKeys = new Set<string>(COMPANION_DISCOVERY_KEYS);
  for (const key of COMPANION_DISCOVERY_KEYS) {
    if (!(key in value)) {
      return { ok: false, reason: `Discovery is missing required field ${key}.` };
    }
  }
  for (const key of Object.keys(value)) {
    if (!expectedKeys.has(key)) {
      return { ok: false, reason: `Discovery contains unexpected field ${key}.` };
    }
  }
  return { ok: true };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isCompanionServiceStatus(value: unknown): value is CompanionServiceStatus {
  return typeof value === "string" && COMPANION_SERVICE_STATUSES.has(value as CompanionServiceStatus);
}

function isCompanionBackend(value: unknown): value is CompanionBackend {
  return typeof value === "string" && COMPANION_BACKENDS.has(value as CompanionBackend);
}

function isCompanionModelStatus(value: unknown): value is CompanionModelStatus {
  return typeof value === "string" && COMPANION_MODEL_STATUSES.has(value as CompanionModelStatus);
}
