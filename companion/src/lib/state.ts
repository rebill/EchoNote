import type { CompanionBackend } from "./settings";

export type ServiceStatus = "stopped" | "starting" | "running" | "stopping" | "error";
export type ModelStatus = "not_loaded" | "loading" | "ready" | "error" | "unknown";
export type DiarizationStatus = "disabled" | "available" | "unavailable" | "failed";

export type CompanionAppState = {
  serviceStatus: ServiceStatus;
  modelStatus: ModelStatus;
  baseUrl: string | null;
  pid: number | null;
  resolvedModelId: string;
  backend: CompanionBackend;
  diarizationStatus: DiarizationStatus;
  diarizationModelId: string;
  lastError: string | null;
  lastExitCode: number | null;
  recentLogs: string[];
  discoveryPath: string;
  settingsPath: string;
  logsPath: string;
};

export const DEFAULT_COMPANION_STATE: CompanionAppState = {
  serviceStatus: "stopped",
  modelStatus: "unknown",
  baseUrl: "http://127.0.0.1:8765",
  pid: null,
  resolvedModelId: "offline-asr-model-not-installed",
  backend: "fake",
  diarizationStatus: "unavailable",
  diarizationModelId: "offline-diarization-model-not-installed",
  lastError: null,
  lastExitCode: null,
  recentLogs: [
    "EchoNote setup dashboard loaded.",
    "Local ASR service is ready to be configured."
  ],
  discoveryPath: "~/Library/Application Support/EchoNote/companion.json",
  settingsPath: "~/Library/Application Support/EchoNote/companion-settings.json",
  logsPath: "~/Library/Logs/EchoNote"
};
