import type { CompanionSettings, SettingsResponse } from "../lib/settings";
import type { SetupPrimaryAction, SetupResponse } from "../lib/setup";
import { primaryActionLabel } from "../lib/setup";
import { createRuntimeSummary } from "./RuntimeSummary";
import { createSettingsPanel } from "./SettingsPanel";
import { createSetupProgress } from "./SetupProgress";

type SetupDashboardOptions = {
  busy: boolean;
  advancedSettingsOpen: boolean;
  onRefresh: () => void | Promise<void>;
  onPrimaryAction: (action: SetupPrimaryAction) => void | Promise<void>;
  onAdvancedSettingsToggle: (open: boolean) => void;
  onSaveSettings: (settings: CompanionSettings) => Promise<void>;
  onResetSetup: () => void | Promise<void>;
  onRestartService: () => void | Promise<void>;
  onLoadModel: () => void | Promise<void>;
  onCopyDiagnostic: () => void | Promise<void>;
  onOpenLogsFolder: () => void | Promise<void>;
};

export function renderSetupDashboard(
  root: HTMLElement,
  setup: SetupResponse,
  settingsResponse: SettingsResponse,
  options: SetupDashboardOptions
): void {
  root.replaceChildren();

  const shell = createElement("main", "app-shell setup-shell");
  shell.append(createHero(setup, options));
  shell.append(createSetupProgress(setup.steps));

  const workspace = createElement("section", "workspace-grid setup-grid");
  workspace.append(createRuntimeSummary(setup));
  workspace.append(createActionsPanel(setup, options));
  workspace.append(createAdvancedSettings(settingsResponse, options));
  workspace.append(createLogPanel(setup));

  shell.append(workspace);
  root.append(shell);
}

function createHero(setup: SetupResponse, options: SetupDashboardOptions): HTMLElement {
  const header = createElement("header", `setup-hero setup-hero-${setup.status}`);
  const copy = createElement("div", "setup-hero-copy");
  copy.append(createElement("p", "eyebrow", "Local transcription"));
  copy.append(createElement("h1", undefined, "EchoNote"));
  copy.append(createElement("p", "setup-message", setup.message));

  const actions = createElement("div", "setup-hero-actions");
  const primary = createElement("button", "primary-button setup-primary", primaryActionLabel(setup.primaryAction));
  primary.type = "button";
  primary.disabled = options.busy || setup.primaryAction === "none";
  primary.addEventListener("click", () => void options.onPrimaryAction(setup.primaryAction));

  const refresh = createElement("button", "secondary-button", "Refresh");
  refresh.type = "button";
  refresh.disabled = options.busy;
  refresh.addEventListener("click", () => void options.onRefresh());
  actions.append(primary, refresh);

  header.append(copy, actions);
  return header;
}

function createActionsPanel(setup: SetupResponse, options: SetupDashboardOptions): HTMLElement {
  const panel = createElement("section", "panel command-panel");
  panel.append(createElement("h2", undefined, "Actions"));

  const actions = createElement("div", "action-grid");
  actions.append(
    createCommandButton("Restart Service", options.onRestartService, options.busy || setup.state.serviceStatus === "starting"),
    createCommandButton(
      "Load Model",
      options.onLoadModel,
      options.busy || setup.state.serviceStatus !== "running" || setup.state.modelStatus === "loading"
    ),
    createCommandButton("Copy Diagnostic", options.onCopyDiagnostic, false),
    createCommandButton("Open Logs Folder", options.onOpenLogsFolder, false)
  );
  panel.append(actions);
  return panel;
}

function createAdvancedSettings(
  response: SettingsResponse,
  options: Pick<
    SetupDashboardOptions,
    "advancedSettingsOpen" | "onAdvancedSettingsToggle" | "onSaveSettings" | "onResetSetup"
  >
): HTMLElement {
  const details = createElement("details", "panel advanced-settings");
  details.open = options.advancedSettingsOpen;
  details.addEventListener("toggle", () => {
    options.onAdvancedSettingsToggle(details.open);
  });

  const summary = createElement("summary", undefined, "Advanced Settings");
  const description = createElement(
    "p",
    "advanced-settings-description",
    "For custom Python, ports, backend, and model settings. Most users do not need this."
  );
  details.append(summary, description);
  details.append(createSettingsPanel(response, {
    onSave: options.onSaveSettings,
    onReset: options.onResetSetup
  }));
  return details;
}

function createLogPanel(setup: SetupResponse): HTMLElement {
  const panel = createElement("section", "panel log-panel");
  panel.append(createElement("h2", undefined, "Recent Logs"));

  const logList = createElement("ol", "log-list");
  for (const line of setup.state.recentLogs) {
    logList.append(createElement("li", undefined, line));
  }
  if (setup.state.recentLogs.length === 0) {
    logList.append(createElement("li", undefined, "No log lines available."));
  }

  panel.append(logList);
  return panel;
}

function createCommandButton(
  label: string,
  onClick: () => void | Promise<void>,
  disabled: boolean
): HTMLButtonElement {
  const button = createElement("button", "command-button active", label);
  button.type = "button";
  button.disabled = disabled;
  button.addEventListener("click", () => void onClick());
  return button;
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
