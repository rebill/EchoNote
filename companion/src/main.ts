import "./styles.css";
import {
  copyDiagnosticReport,
  getCompanionAppState,
  getCompanionSettings,
  loadCompanionModel,
  openLogsFolder,
  restartCompanionService,
  saveCompanionSettings,
  startCompanionService,
  stopCompanionService
} from "./lib/companion-api";
import { renderStatusDashboard } from "./components/StatusDashboard";
import type { CompanionSettings } from "./lib/settings";
import type { CompanionAppState } from "./lib/state";

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("App root not found.");
}

const appRoot = app;
let renderInFlight = false;
let modelLoadInFlight = false;

async function render(): Promise<void> {
  if (renderInFlight) {
    return;
  }

  renderInFlight = true;
  try {
    const [state, settingsResponse] = await Promise.all([
      getCompanionAppState(),
      getCompanionSettings()
    ]);

    renderStatusDashboard(appRoot, applyPendingUiState(state), settingsResponse, {
      onRefresh: render,
      onSaveSettings: saveSettings,
      onStartService: () => runServiceCommand(startCompanionService),
      onStopService: () => runServiceCommand(stopCompanionService),
      onRestartService: () => runServiceCommand(restartCompanionService),
      onLoadModel: loadModel,
      onCopyDiagnostic: copyDiagnostic,
      onOpenLogsFolder: openLogs
    });
  } finally {
    renderInFlight = false;
  }
}

async function saveSettings(settings: CompanionSettings): Promise<void> {
  try {
    await saveCompanionSettings(settings);
    await render();
  } catch (error) {
    window.alert(error instanceof Error ? error.message : String(error));
  }
}

async function loadModel(): Promise<void> {
  modelLoadInFlight = true;
  await render();

  try {
    await loadCompanionModel();
  } catch (error) {
    window.alert(error instanceof Error ? error.message : String(error));
  } finally {
    modelLoadInFlight = false;
    await render();
  }
}

async function runServiceCommand(command: () => Promise<unknown>): Promise<void> {
  try {
    await command();
    await render();
  } catch (error) {
    window.alert(error instanceof Error ? error.message : String(error));
    await render();
  }
}

async function copyDiagnostic(): Promise<void> {
  try {
    const report = await copyDiagnosticReport();
    await window.navigator.clipboard.writeText(report);
    window.alert("Diagnostic report copied.");
  } catch (error) {
    window.alert(error instanceof Error ? error.message : String(error));
  }
}

async function openLogs(): Promise<void> {
  try {
    await openLogsFolder();
  } catch (error) {
    window.alert(error instanceof Error ? error.message : String(error));
  }
}

function applyPendingUiState(state: CompanionAppState): CompanionAppState {
  if (!modelLoadInFlight) {
    return state;
  }

  if (state.serviceStatus !== "running" || state.modelStatus === "ready") {
    return state;
  }

  return {
    ...state,
    modelStatus: "loading",
    lastError: null
  };
}

window.setInterval(() => {
  if (!isEditingSettings()) {
    void render();
  }
}, 2000);

function isEditingSettings(): boolean {
  const activeElement = document.activeElement;
  return activeElement instanceof HTMLElement && Boolean(activeElement.closest(".settings-form"));
}

void render();
