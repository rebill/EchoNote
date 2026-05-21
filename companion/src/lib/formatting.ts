import type { ModelStatus, ServiceStatus } from "./state";

const SERVICE_LABELS: Record<ServiceStatus, string> = {
  stopped: "Stopped",
  starting: "Starting",
  running: "Running",
  stopping: "Stopping",
  error: "Error"
};

const MODEL_LABELS: Record<ModelStatus, string> = {
  not_loaded: "Not Loaded",
  loading: "Loading",
  ready: "Ready",
  error: "Error",
  unknown: "Unknown"
};

export function formatServiceStatus(status: ServiceStatus): string {
  return SERVICE_LABELS[status];
}

export function formatModelStatus(status: ModelStatus): string {
  return MODEL_LABELS[status];
}

export function formatPid(pid: number | null): string {
  return pid === null ? "None" : String(pid);
}

export function formatExitCode(exitCode: number | null): string {
  return exitCode === null ? "None" : String(exitCode);
}
