import type { EchoNoteErrorCode } from "../utils/errors";
import type { CompanionResolution } from "./companion-discovery";
import { resolveCompanionDiscovery } from "./companion-discovery";
import type { EchoNoteSettings } from "../settings/settings";

export type CompanionAvailableResolution = CompanionResolution & { kind: "available" };
export type CompanionUnavailableResolution = Exclude<CompanionResolution, { kind: "available" }>;

export type AsrRuntime = {
  mode: "companion";
  requestedMode: "companion";
  baseUrl: string;
  companion: CompanionAvailableResolution;
};

export type ResolveAsrRuntimeOptions = {
  resolveCompanion?: (
    settings: Pick<EchoNoteSettings, "companionDiscoveryPath" | "companionDiscoveryMaxAgeSeconds">
  ) => Promise<CompanionResolution>;
};

export class AsrRuntimeResolutionError extends Error {
  constructor(
    readonly code: EchoNoteErrorCode,
    message: string,
    readonly companion: CompanionUnavailableResolution
  ) {
    super(message);
    this.name = "AsrRuntimeResolutionError";
  }

  get detail(): string {
    return this.companion.reason;
  }
}

export async function resolveAsrRuntime(
  settings: EchoNoteSettings,
  options: ResolveAsrRuntimeOptions = {}
): Promise<AsrRuntime> {
  const companion = await (options.resolveCompanion ?? resolveCompanionDiscovery)(settings);
  if (companion.kind === "available") {
    return {
      mode: "companion",
      requestedMode: "companion",
      baseUrl: companion.baseUrl,
      companion
    };
  }

  throw new AsrRuntimeResolutionError(
    getCompanionErrorCode(companion),
    "EchoNote Companion is unavailable.",
    companion
  );
}

function getCompanionErrorCode(companion: CompanionUnavailableResolution): EchoNoteErrorCode {
  if (companion.kind === "invalid") {
    return "ASR_COMPANION_DISCOVERY_INVALID";
  }
  if (companion.kind === "stale") {
    return "ASR_COMPANION_DISCOVERY_STALE";
  }
  return "ASR_COMPANION_UNAVAILABLE";
}
