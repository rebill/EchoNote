import type { CompanionSettings } from "./settings";
import type { CompanionAppState } from "./state";

export type SetupStatus =
  | "unknown"
  | "checking"
  | "not_configured"
  | "ready"
  | "running"
  | "repair_required"
  | "installing"
  | "unsupported"
  | "error";

export type SetupStepId =
  | "system"
  | "python"
  | "runtime"
  | "dependencies"
  | "port"
  | "service"
  | "model"
  | "obsidian";

export type SetupStepStatus = "pending" | "running" | "passed" | "warning" | "failed" | "skipped";

export type SetupPrimaryAction = "setup" | "repair" | "start" | "stop" | "retry" | "none";

export type SetupStep = {
  id: SetupStepId;
  label: string;
  status: SetupStepStatus;
  summary: string;
  detail: string | null;
  recoverable: boolean;
};

export type SetupResponse = {
  status: SetupStatus;
  steps: SetupStep[];
  settings: CompanionSettings;
  state: CompanionAppState;
  primaryAction: SetupPrimaryAction;
  message: string;
};

export function primaryActionLabel(action: SetupPrimaryAction): string {
  switch (action) {
    case "setup":
      return "Set Up EchoNote";
    case "repair":
      return "Repair EchoNote";
    case "start":
      return "Start Service";
    case "stop":
      return "Stop Service";
    case "retry":
      return "Retry";
    case "none":
      return "Ready";
  }
}
