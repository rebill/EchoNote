import {
  DEFAULT_COMPANION_SETTINGS,
  type CompanionBackend,
  type CompanionModelPreset,
  type CompanionSettings,
  type SettingsResponse
} from "../lib/settings";

type SettingsPanelOptions = {
  onSave: (settings: CompanionSettings) => Promise<void>;
  onReset?: () => void | Promise<void>;
};

export function createSettingsPanel(
  response: SettingsResponse,
  options: SettingsPanelOptions
): HTMLElement {
  const panel = createElement("section", "panel settings-panel");
  panel.append(createElement("h2", undefined, "Settings"));

  const form = createElement("form", "settings-form");
  form.append(
    createTextField("Python path", "pythonPath", response.settings.pythonPath),
    createTextField("ASR service path", "asrServicePath", response.settings.asrServicePath),
    createNumberField("Port", "preferredPort", response.settings.preferredPort),
    createSelectField<CompanionBackend>("Backend", "backend", response.settings.backend, [
      ["fake", "fake"],
      ["mlx-audio", "mlx-audio"]
    ]),
    createSelectField<CompanionModelPreset>("Model preset", "modelPreset", response.settings.modelPreset, [
      ["qwen3-0.6b-4bit", "Qwen3 ASR 0.6B 4-bit"],
      ["qwen3-1.7b-4bit", "Qwen3 ASR 1.7B 4-bit"],
      ["custom", "Custom model"]
    ]),
    createCheckboxField("Offline mode (required)", "offlineMode", response.settings.offlineMode, true),
    createTextField("Offline bundle path", "offlineBundlePath", response.settings.offlineBundlePath),
    createTextField("Managed runtime path", "runtimePath", response.settings.runtimePath),
    createTextField("Installed models path", "modelsPath", response.settings.modelsPath),
    createTextField("Installed ASR model path", "asrModelPath", response.settings.asrModelPath),
    createTextField("Custom local model path", "customModelPath", response.settings.customModelPath),
    createCheckboxField("Speaker diarization", "diarizationEnabled", response.settings.diarizationEnabled),
    createTextField(
      "Installed diarization model path",
      "diarizationModelPath",
      response.settings.diarizationModelPath
    )
  );

  const footer = createElement("div", "settings-footer");
  footer.append(createElement("span", "settings-path", response.settingsPath));
  if (options.onReset) {
    const resetButton = createElement("button", "secondary-button", "Reset to Defaults");
    resetButton.type = "button";
    resetButton.addEventListener("click", () => void options.onReset?.());
    footer.append(resetButton);
  }
  const saveButton = createElement("button", "primary-button", "Save Settings");
  saveButton.type = "submit";
  footer.append(saveButton);
  form.append(footer);

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const formData = new FormData(form);
    const settings = readSettings(formData, response.settings);
    void options.onSave(settings);
  });

  if (response.recovered) {
    panel.append(createElement("p", "notice-text", "Settings recovered with safe defaults."));
  }

  panel.append(form);
  return panel;
}

function readSettings(formData: FormData, current: CompanionSettings): CompanionSettings {
  const preferredPort = Number.parseInt(String(formData.get("preferredPort") ?? ""), 10);

  return {
    pythonPath: readString(formData, "pythonPath", DEFAULT_COMPANION_SETTINGS.pythonPath),
    asrServicePath: readString(formData, "asrServicePath", DEFAULT_COMPANION_SETTINGS.asrServicePath),
    preferredPort: Number.isInteger(preferredPort) ? preferredPort : DEFAULT_COMPANION_SETTINGS.preferredPort,
    backend: readString(formData, "backend", DEFAULT_COMPANION_SETTINGS.backend) as CompanionBackend,
    modelPreset: readString(
      formData,
      "modelPreset",
      DEFAULT_COMPANION_SETTINGS.modelPreset
    ) as CompanionModelPreset,
    customModelPath: readString(formData, "customModelPath", ""),
    offlineMode: true,
    offlineBundlePath: readString(
      formData,
      "offlineBundlePath",
      DEFAULT_COMPANION_SETTINGS.offlineBundlePath
    ),
    runtimePath: readString(formData, "runtimePath", DEFAULT_COMPANION_SETTINGS.runtimePath),
    modelsPath: readString(formData, "modelsPath", DEFAULT_COMPANION_SETTINGS.modelsPath),
    asrModelPath: readString(formData, "asrModelPath", ""),
    autoStartService: current.autoStartService,
    setupCompletedAt: current.setupCompletedAt,
    setupVersion: current.setupVersion,
    autoRepairEnabled: current.autoRepairEnabled,
    diarizationEnabled: formData.get("diarizationEnabled") === "on",
    diarizationModelPath: readString(
      formData,
      "diarizationModelPath",
      DEFAULT_COMPANION_SETTINGS.diarizationModelPath
    )
  };
}

function readString(formData: FormData, key: string, fallback: string): string {
  const value = formData.get(key);
  return typeof value === "string" ? value : fallback;
}

function createTextField(label: string, name: keyof CompanionSettings, value: string): HTMLElement {
  const row = createField(label);
  const input = createElement("input", "text-input");
  input.name = name;
  input.value = value;
  input.autocomplete = "off";
  row.append(input);
  return row;
}

function createCheckboxField(
  label: string,
  name: keyof CompanionSettings,
  value: boolean,
  disabled = false
): HTMLElement {
  const row = createField(label);
  const input = createElement("input", "checkbox-input");
  input.name = name;
  input.type = "checkbox";
  input.checked = value;
  input.disabled = disabled;
  row.append(input);
  return row;
}

function createNumberField(label: string, name: keyof CompanionSettings, value: number): HTMLElement {
  const row = createField(label);
  const input = createElement("input", "text-input");
  input.name = name;
  input.type = "number";
  input.min = "1";
  input.max = "65535";
  input.value = String(value);
  row.append(input);
  return row;
}

function createSelectField<T extends string>(
  label: string,
  name: keyof CompanionSettings,
  value: T,
  options: Array<[T, string]>
): HTMLElement {
  const row = createField(label);
  const select = createElement("select", "text-input");
  select.name = name;
  for (const [optionValue, optionLabel] of options) {
    const option = createElement("option", undefined, optionLabel);
    option.value = optionValue;
    option.selected = optionValue === value;
    select.append(option);
  }
  row.append(select);
  return row;
}

function createField(label: string): HTMLElement {
  const row = createElement("label", "form-field");
  row.append(createElement("span", undefined, label));
  return row;
}

function createElement<K extends keyof HTMLElementTagNameMap>(
  tagName: K,
  className?: string,
  text?: string
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tagName);
  if (className) {
    element.className = className;
  }
  if (text !== undefined) {
    element.textContent = text;
  }
  return element;
}
