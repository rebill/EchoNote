import { invoke } from "@tauri-apps/api/core";
import {
  DEFAULT_COMPANION_SETTINGS,
  DEFAULT_SETTINGS_RESPONSE,
  type CompanionSettings,
  type SettingsResponse
} from "./settings";
import { DEFAULT_COMPANION_STATE, type CompanionAppState } from "./state";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export async function getCompanionAppState(): Promise<CompanionAppState> {
  if (!canInvokeTauri()) {
    const settings = getFallbackSettings();
    return {
      ...DEFAULT_COMPANION_STATE,
      baseUrl: `http://127.0.0.1:${settings.preferredPort}`,
      resolvedModelId: resolveModelId(settings),
      backend: settings.backend
    };
  }

  try {
    return await invoke<CompanionAppState>("get_app_state");
  } catch (error) {
    return {
      ...DEFAULT_COMPANION_STATE,
      serviceStatus: "error",
      lastError: error instanceof Error ? error.message : String(error),
      recentLogs: ["Failed to read Companion app state from Tauri."]
    };
  }
}

export async function getCompanionSettings(): Promise<SettingsResponse> {
  if (!canInvokeTauri()) {
    return {
      ...DEFAULT_SETTINGS_RESPONSE,
      settings: getFallbackSettings()
    };
  }

  return invoke<SettingsResponse>("get_settings");
}

export async function saveCompanionSettings(settings: CompanionSettings): Promise<SettingsResponse> {
  const normalized = normalizeSettings(settings);

  if (!canInvokeTauri()) {
    window.localStorage.setItem("echonote-companion-settings", JSON.stringify(normalized));
    return {
      ...DEFAULT_SETTINGS_RESPONSE,
      settings: normalized
    };
  }

  return invoke<SettingsResponse>("save_settings", { settings: normalized });
}

export async function startCompanionService(): Promise<CompanionAppState> {
  if (!canInvokeTauri()) {
    return serviceControlFallback("Start Service");
  }

  return invoke<CompanionAppState>("start_service");
}

export async function stopCompanionService(): Promise<CompanionAppState> {
  if (!canInvokeTauri()) {
    return serviceControlFallback("Stop Service");
  }

  return invoke<CompanionAppState>("stop_service");
}

export async function restartCompanionService(): Promise<CompanionAppState> {
  if (!canInvokeTauri()) {
    return serviceControlFallback("Restart Service");
  }

  return invoke<CompanionAppState>("restart_service");
}

export async function loadCompanionModel(): Promise<CompanionAppState> {
  if (!canInvokeTauri()) {
    return serviceControlFallback("Load Model");
  }

  return invoke<CompanionAppState>("load_model");
}

export async function copyDiagnosticReport(): Promise<string> {
  if (!canInvokeTauri()) {
    return "# EchoNote Diagnostic Report\n\nBrowser preview mode does not have access to Companion diagnostics.";
  }

  return invoke<string>("copy_diagnostic_report");
}

export async function openLogsFolder(): Promise<void> {
  if (!canInvokeTauri()) {
    return;
  }

  return invoke<void>("open_logs_folder");
}

function canInvokeTauri(): boolean {
  return Boolean(window.__TAURI_INTERNALS__);
}

function getFallbackSettings(): CompanionSettings {
  try {
    const raw = window.localStorage.getItem("echonote-companion-settings");
    if (!raw) {
      return DEFAULT_COMPANION_SETTINGS;
    }
    return normalizeSettings(JSON.parse(raw) as CompanionSettings);
  } catch {
    return DEFAULT_COMPANION_SETTINGS;
  }
}

function normalizeSettings(settings: CompanionSettings): CompanionSettings {
  const preferredPort = Number.isInteger(settings.preferredPort)
    && settings.preferredPort > 0
    && settings.preferredPort <= 65535
    ? settings.preferredPort
    : DEFAULT_COMPANION_SETTINGS.preferredPort;

  const modelPreset = settings.modelPreset === "custom" && !settings.customModelId.trim()
    ? DEFAULT_COMPANION_SETTINGS.modelPreset
    : settings.modelPreset;

  return {
    pythonPath: settings.pythonPath.trim() || DEFAULT_COMPANION_SETTINGS.pythonPath,
    asrServicePath: settings.asrServicePath.trim() || DEFAULT_COMPANION_SETTINGS.asrServicePath,
    preferredPort,
    backend: settings.backend,
    modelPreset,
    customModelId: settings.customModelId.trim(),
    autoStartService: settings.autoStartService
  };
}

function resolveModelId(settings: CompanionSettings): string {
  if (settings.modelPreset === "qwen3-1.7b-4bit") {
    return "mlx-community/Qwen3-ASR-1.7B-4bit";
  }
  if (settings.modelPreset === "custom" && settings.customModelId.trim()) {
    return settings.customModelId.trim();
  }
  return "mlx-community/Qwen3-ASR-0.6B-4bit";
}

function serviceControlFallback(action: string): CompanionAppState {
  const settings = getFallbackSettings();
  return {
    ...DEFAULT_COMPANION_STATE,
    serviceStatus: "error",
    baseUrl: `http://127.0.0.1:${settings.preferredPort}`,
    resolvedModelId: resolveModelId(settings),
    backend: settings.backend,
    lastError: `${action} requires the Tauri desktop runtime.`,
    recentLogs: [`${action} is unavailable in browser preview mode.`]
  };
}
