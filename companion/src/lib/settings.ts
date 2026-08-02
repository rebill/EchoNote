export type CompanionBackend = "fake" | "mlx-audio";
export type CompanionModelPreset = "qwen3-0.6b-4bit" | "qwen3-1.7b-4bit" | "custom";

export type CompanionSettings = {
  pythonPath: string;
  asrServicePath: string;
  preferredPort: number;
  backend: CompanionBackend;
  modelPreset: CompanionModelPreset;
  customModelPath: string;
  offlineMode: boolean;
  offlineBundlePath: string;
  runtimePath: string;
  modelsPath: string;
  asrModelPath: string;
  autoStartService: boolean;
  setupCompletedAt: string | null;
  setupVersion: string | null;
  autoRepairEnabled: boolean;
  diarizationEnabled: boolean;
  diarizationModelPath: string;
};

export type SettingsResponse = {
  settings: CompanionSettings;
  settingsPath: string;
  recovered: boolean;
};

export const DEFAULT_COMPANION_SETTINGS: CompanionSettings = {
  pythonPath: "python3",
  asrServicePath: "../asr-service",
  preferredPort: 8765,
  backend: "mlx-audio",
  modelPreset: "qwen3-0.6b-4bit",
  customModelPath: "",
  offlineMode: true,
  offlineBundlePath: "~/Library/Application Support/EchoNote/offline-bundle",
  runtimePath: "~/Library/Application Support/EchoNote/runtime",
  modelsPath: "~/Library/Application Support/EchoNote/models",
  asrModelPath: "",
  autoStartService: false,
  setupCompletedAt: null,
  setupVersion: null,
  autoRepairEnabled: false,
  diarizationEnabled: true,
  diarizationModelPath: ""
};

export const DEFAULT_SETTINGS_RESPONSE: SettingsResponse = {
  settings: DEFAULT_COMPANION_SETTINGS,
  settingsPath: "~/Library/Application Support/EchoNote/companion-settings.json",
  recovered: false
};
