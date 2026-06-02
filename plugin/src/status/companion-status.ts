import type { CompanionResolution } from "../asr/companion-discovery";
import type { AsrRuntime } from "../asr/asr-runtime-resolver";
import type { EchoNoteSettings } from "../settings/settings";
import type { EchoNoteStatus, ModelStatus } from "./status-types";

export function createInitialRuntimeStatus(settings: EchoNoteSettings): Partial<EchoNoteStatus> {
  return {
    asrRuntime: settings.asrRuntimeMode,
    activeAsrRuntime: "unknown",
    companionStatus: "unknown",
    companionApiUrl: null,
    companionDiscoveryPath: settings.companionDiscoveryPath,
    companionMessage: "Companion status not checked yet.",
    model: "unknown",
    selectedModel: "Unknown"
  };
}

export function createAsrRuntimeStatus(settings: EchoNoteSettings, runtime: AsrRuntime): Partial<EchoNoteStatus> {
  const baseStatus: Partial<EchoNoteStatus> = {
    asrRuntime: settings.asrRuntimeMode,
    activeAsrRuntime: runtime.mode,
    companionDiscoveryPath: settings.companionDiscoveryPath
  };

  return {
    ...baseStatus,
    ...createCompanionResolutionStatus(settings, runtime.companion)
  };
}

export function createCompanionResolutionStatus(
  settings: Pick<EchoNoteSettings, "asrRuntimeMode" | "companionDiscoveryPath">,
  resolution: CompanionResolution
): Partial<EchoNoteStatus> {
  const baseStatus: Partial<EchoNoteStatus> = {
    asrRuntime: settings.asrRuntimeMode,
    companionDiscoveryPath: settings.companionDiscoveryPath
  };

  if (resolution.kind === "available") {
    return {
      ...baseStatus,
      companionStatus: "available",
      companionApiUrl: resolution.baseUrl,
      companionMessage: "EchoNote desktop ASR endpoint is available.",
      model: toPluginModelStatus(resolution.discovery.modelStatus),
      selectedModel: resolution.discovery.modelId
    };
  }

  return {
    ...baseStatus,
    companionStatus: resolution.kind,
    companionApiUrl: null,
    companionMessage: buildCompanionMessage(resolution)
  };
}

function buildCompanionMessage(resolution: Exclude<CompanionResolution, { kind: "available" }>): string {
  if (resolution.kind === "missing") {
    return `${resolution.reason} Open EchoNote and click Set Up EchoNote or Start Service.`;
  }
  if (resolution.kind === "stale") {
    return `${resolution.reason} Open EchoNote and click Restart Service.`;
  }
  if (resolution.kind === "invalid") {
    return `${resolution.reason} Open EchoNote and click Set Up EchoNote, or delete the stale discovery file after closing EchoNote.`;
  }
  if (resolution.kind === "not_running") {
    return `${resolution.reason} Open EchoNote and click Start Service.`;
  }
  return `${resolution.reason} Check that EchoNote is running and the ASR service passes /health.`;
}

function toPluginModelStatus(status: string): ModelStatus {
  if (status === "not_loaded" || status === "loading" || status === "ready" || status === "error") {
    return status;
  }
  return "unknown";
}
