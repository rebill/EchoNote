import { DEFAULT_COMPANION_SETTINGS } from "./settings";
import { DEFAULT_COMPANION_STATE } from "./state";
import type { SetupResponse, SetupStatus } from "./setup";

export const SETUP_RESPONSE_FIXTURES: Record<SetupStatus, SetupResponse> = {
  unknown: fixture("unknown", "Setup status has not been checked yet.", "retry"),
  checking: fixture("checking", "Checking EchoNote setup.", "none"),
  not_configured: fixture("not_configured", "Set up EchoNote to use local transcription.", "setup"),
  ready: fixture("ready", "EchoNote is ready.", "start"),
  running: fixture("running", "Local transcription is running.", "stop"),
  repair_required: fixture("repair_required", "EchoNote found an issue it can repair.", "repair"),
  installing: fixture("installing", "Setting up EchoNote.", "none"),
  unsupported: fixture("unsupported", "This Mac is not supported.", "none"),
  error: fixture("error", "EchoNote setup needs attention.", "retry")
};

function fixture(
  status: SetupStatus,
  message: string,
  primaryAction: SetupResponse["primaryAction"]
): SetupResponse {
  return {
    status,
    message,
    primaryAction,
    settings: DEFAULT_COMPANION_SETTINGS,
    state: DEFAULT_COMPANION_STATE,
    steps: [
      {
        id: "system",
        label: "Check System",
        status: status === "unsupported" ? "failed" : "passed",
        summary: status === "unsupported" ? "Unsupported platform." : "This Mac can run EchoNote setup.",
        detail: null,
        recoverable: false
      },
      {
        id: "python",
        label: "Find Python",
        status: status === "ready" || status === "running" ? "passed" : "pending",
        summary: "Python 3.11 or newer is available.",
        detail: null,
        recoverable: true
      },
      {
        id: "runtime",
        label: "Prepare ASR Runtime",
        status: status === "repair_required" ? "failed" : "pending",
        summary: "ASR service source is available.",
        detail: null,
        recoverable: true
      }
    ]
  };
}
