import "./styles.css";
import {
  copyDiagnosticReport,
  detectSetup,
  getCompanionAppState,
  getCompanionSettings,
  loadCompanionModel,
  openLogsFolder,
  installOrRepairRuntime,
  restartCompanionService,
  resetSetup,
  saveCompanionSettings,
  startServiceWithDefaults,
  startCompanionService,
  stopCompanionService
} from "./lib/companion-api";
import { renderStatusDashboard } from "./components/StatusDashboard";
import type { CompanionSettings } from "./lib/settings";
import type { CompanionAppState } from "./lib/state";
import type { SetupPrimaryAction, SetupResponse } from "./lib/setup";
import { renderSetupDashboard } from "./components/SetupDashboard";

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("App root not found.");
}

const appRoot = app;
let renderInFlight = false;
let modelLoadInFlight = false;
let setupActionInFlight = false;
let advancedSettingsOpen = false;
let stickySetupError: SetupResponse | null = null;

async function render(): Promise<void> {
  if (renderInFlight) {
    return;
  }

  renderInFlight = true;
  try {
    const [state, settingsResponse, setup] = await Promise.all([
      getCompanionAppState(),
      getCompanionSettings(),
      detectSetup()
    ]);

    renderWithSetup(applyPendingUiState(state), settingsResponse, stickySetupError ?? setup);
  } finally {
    renderInFlight = false;
  }
}

function renderWithSetup(
  state: CompanionAppState,
  settingsResponse: Awaited<ReturnType<typeof getCompanionSettings>>,
  setup: SetupResponse
): void {
  if (setup.status === "unknown") {
    renderStatusDashboard(appRoot, state, settingsResponse, {
      onRefresh: render,
      onSaveSettings: saveSettings,
      onStartService: () => runServiceCommand(startCompanionService),
      onStopService: () => runServiceCommand(stopCompanionService),
      onRestartService: () => runServiceCommand(restartCompanionService),
      onLoadModel: loadModel,
      onCopyDiagnostic: copyDiagnostic,
      onOpenLogsFolder: openLogs
    });
    return;
  }

  renderSetupDashboard(appRoot, setup, settingsResponse, {
    busy: setupActionInFlight || modelLoadInFlight,
    advancedSettingsOpen,
    onRefresh: refreshSetup,
    onPrimaryAction: runSetupPrimaryAction,
    onAdvancedSettingsToggle: (open) => {
      advancedSettingsOpen = open;
    },
    onSaveSettings: saveSettings,
    onResetSetup: resetSetupSettings,
    onRestartService: () => runServiceCommand(restartCompanionService),
    onLoadModel: loadModel,
    onCopyDiagnostic: copyDiagnostic,
    onOpenLogsFolder: openLogs
  });
}

async function saveSettings(settings: CompanionSettings): Promise<void> {
  try {
    stickySetupError = null;
    await saveCompanionSettings(settings);
    await detectSetup();
    await render();
  } catch (error) {
    window.alert(error instanceof Error ? error.message : String(error));
  }
}

async function resetSetupSettings(): Promise<void> {
  try {
    stickySetupError = null;
    await resetSetup();
    await render();
  } catch (error) {
    window.alert(error instanceof Error ? error.message : String(error));
  }
}

async function runSetupPrimaryAction(action: SetupPrimaryAction): Promise<void> {
  stickySetupError = null;
  setupActionInFlight = true;
  await render();

  try {
    let setupResponse: SetupResponse | null = null;
    if (action === "setup" || action === "repair") {
      setupResponse = await installOrRepairRuntime();
    } else if (action === "start") {
      setupResponse = await startServiceWithDefaults();
    } else if (action === "stop") {
      await stopCompanionService();
    } else if (action === "retry") {
      setupResponse = await installOrRepairRuntime();
    }
    stickySetupError = setupResponse?.status === "error" ? setupResponse : null;
  } catch (error) {
    window.alert(error instanceof Error ? error.message : String(error));
  } finally {
    setupActionInFlight = false;
    await render();
  }
}

async function refreshSetup(): Promise<void> {
  stickySetupError = null;
  await render();
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
