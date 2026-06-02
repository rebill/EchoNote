export type CompanionBackend = "fake" | "mlx-audio";
export type CompanionModelPreset = "qwen3-0.6b-4bit" | "qwen3-1.7b-4bit" | "custom";

export type CompanionSettings = {
  pythonPath: string;
  asrServicePath: string;
  preferredPort: number;
  backend: CompanionBackend;
  modelPreset: CompanionModelPreset;
  customModelId: string;
  autoStartService: boolean;
  setupCompletedAt: string | null;
  setupVersion: string | null;
  autoRepairEnabled: boolean;
  huggingFaceToken: string;
  diarizationEnabled: boolean;
  diarizationModelId: string;
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
  backend: "fake",
  modelPreset: "qwen3-0.6b-4bit",
  customModelId: "",
  autoStartService: false,
  setupCompletedAt: null,
  setupVersion: null,
  autoRepairEnabled: false,
  huggingFaceToken: "",
  diarizationEnabled: true,
  diarizationModelId: "pyannote/speaker-diarization-community-1"
};

export const DEFAULT_SETTINGS_RESPONSE: SettingsResponse = {
  settings: DEFAULT_COMPANION_SETTINGS,
  settingsPath: "~/Library/Application Support/EchoNote/companion-settings.json",
  recovered: false
};
