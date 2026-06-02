import {
  formatDiarizationStatus,
  formatExitCode,
  formatModelStatus,
  formatPid,
  formatServiceStatus
} from "../lib/formatting";
import type { CompanionSettings, SettingsResponse } from "../lib/settings";
import type { CompanionAppState, ModelStatus, ServiceStatus } from "../lib/state";
import { createSettingsPanel } from "./SettingsPanel";

type DashboardOptions = {
  onRefresh: () => void | Promise<void>;
  onSaveSettings: (settings: CompanionSettings) => Promise<void>;
  onStartService: () => void | Promise<void>;
  onStopService: () => void | Promise<void>;
  onRestartService: () => void | Promise<void>;
  onLoadModel: () => void | Promise<void>;
  onCopyDiagnostic: () => void | Promise<void>;
  onOpenLogsFolder: () => void | Promise<void>;
};

export function renderStatusDashboard(
  root: HTMLElement,
  state: CompanionAppState,
  settingsResponse: SettingsResponse,
  options: DashboardOptions
): void {
  root.replaceChildren();

  const shell = createElement("main", "app-shell");
  shell.append(createHeader(state, options));
  shell.append(createStatusBand(state));

  const workspace = createElement("section", "workspace-grid");
  workspace.append(createServicePanel(state));
  workspace.append(createRuntimePanel(state));
  workspace.append(createSettingsPanel(settingsResponse, { onSave: options.onSaveSettings }));
  workspace.append(createActionsPanel(state, options));
  workspace.append(createLogPanel(state));

  shell.append(workspace);
  root.append(shell);
}

function createHeader(state: CompanionAppState, options: DashboardOptions): HTMLElement {
  const header = createElement("header", "top-bar");
  const titleGroup = createElement("div", "title-group");
  titleGroup.append(createElement("p", "eyebrow", "ASR runtime"));
  titleGroup.append(createElement("h1", undefined, "EchoNote"));

  const meta = createElement("div", "header-meta");
  meta.append(createElement("span", "runtime-chip", state.backend));
  const refreshButton = createElement("button", "secondary-button", "Refresh");
  refreshButton.type = "button";
  refreshButton.addEventListener("click", () => void options.onRefresh());
  meta.append(refreshButton);

  header.append(titleGroup, meta);
  return header;
}

function createStatusBand(state: CompanionAppState): HTMLElement {
  const band = createElement("section", "status-band");
  band.append(
    createMetric("Service", formatServiceStatus(state.serviceStatus), state.serviceStatus),
    createMetric("Model", formatModelStatus(state.modelStatus), state.modelStatus),
    createMetric("Speakers", formatDiarizationStatus(state.diarizationStatus), state.diarizationStatus),
    createMetric("API", state.baseUrl ?? "Unavailable", state.baseUrl ? "running" : "stopped"),
    createMetric("PID", formatPid(state.pid), state.pid ? "running" : "stopped")
  );
  return band;
}

function createServicePanel(state: CompanionAppState): HTMLElement {
  const panel = createElement("section", "panel");
  panel.append(createElement("h2", undefined, "Service"));
  panel.append(
    createKeyValue("Status", formatServiceStatus(state.serviceStatus)),
    createKeyValue("Backend", state.backend),
    createKeyValue("Base URL", state.baseUrl ?? "Unavailable"),
    createKeyValue("Last exit code", formatExitCode(state.lastExitCode))
  );

  if (state.lastError) {
    panel.append(createElement("p", "error-text", state.lastError));
  }

  return panel;
}

function createRuntimePanel(state: CompanionAppState): HTMLElement {
  const panel = createElement("section", "panel");
  panel.append(createElement("h2", undefined, "Runtime"));
  panel.append(
    createKeyValue("Model", state.resolvedModelId),
    createKeyValue("Model status", formatModelStatus(state.modelStatus)),
    createKeyValue("Speaker diarization", formatDiarizationStatus(state.diarizationStatus)),
    createKeyValue("Diarization model", state.diarizationModelId),
    createKeyValue("Discovery", state.discoveryPath),
    createKeyValue("Settings", state.settingsPath),
    createKeyValue("Logs", state.logsPath)
  );

  if (state.modelStatus === "loading") {
    panel.append(createModelLoadProgress());
  }

  return panel;
}

function createActionsPanel(state: CompanionAppState, options: DashboardOptions): HTMLElement {
  const panel = createElement("section", "panel command-panel");
  panel.append(createElement("h2", undefined, "Controls"));

  const actions = createElement("div", "action-grid");
  actions.append(
    createCommandButton(
      "Start Service",
      options.onStartService,
      state.serviceStatus === "running" || state.serviceStatus === "starting"
    ),
    createCommandButton(
      "Stop Service",
      options.onStopService,
      state.serviceStatus === "stopped" || state.serviceStatus === "stopping"
    ),
    createCommandButton(
      "Restart Service",
      options.onRestartService,
      state.serviceStatus === "starting" || state.serviceStatus === "stopping"
    ),
    createCommandButton(
      "Load Model",
      options.onLoadModel,
      state.serviceStatus !== "running"
        || state.modelStatus === "loading"
        || state.modelStatus === "ready"
    ),
    createCommandButton("Copy Diagnostic", options.onCopyDiagnostic, false),
    createCommandButton("Open Logs Folder", options.onOpenLogsFolder, false)
  );

  const refreshButton = createElement("button", "command-button active", "Refresh State");
  refreshButton.type = "button";
  refreshButton.addEventListener("click", () => void options.onRefresh());
  actions.append(refreshButton);

  panel.append(actions);
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

function createModelLoadProgress(): HTMLElement {
  const progress = createElement("div", "model-progress");
  const label = createElement("div", "model-progress-label");
  label.append(
    createElement("span", undefined, "Loading model"),
    createElement("strong", undefined, "In progress")
  );

  const track = createElement("div", "model-progress-track");
  track.setAttribute("role", "progressbar");
  track.setAttribute("aria-label", "ASR model loading progress");
  track.setAttribute("aria-valuetext", "Loading");
  track.append(createElement("span", "model-progress-bar"));

  progress.append(label, track);
  return progress;
}

function createLogPanel(state: CompanionAppState): HTMLElement {
  const panel = createElement("section", "panel log-panel");
  panel.append(createElement("h2", undefined, "Recent Logs"));

  const logList = createElement("ol", "log-list");
  for (const line of state.recentLogs) {
    logList.append(createElement("li", undefined, line));
  }

  panel.append(logList);
  return panel;
}

function createMetric(
  label: string,
  value: string,
  tone: ServiceStatus | ModelStatus | CompanionAppState["diarizationStatus"]
): HTMLElement {
  const metric = createElement("article", `metric metric-${tone}`);
  metric.append(createElement("span", "metric-label", label));
  metric.append(createElement("strong", "metric-value", value));
  return metric;
}

function createKeyValue(label: string, value: string): HTMLElement {
  const row = createElement("div", "key-value");
  row.append(createElement("span", undefined, label));
  row.append(createElement("strong", undefined, value));
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
