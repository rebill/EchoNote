import type { EchoNoteError } from "../utils/errors";
import type { AsrRuntimeMode } from "../settings/settings";

export type MicrophonePermissionStatus = "unknown" | "granted" | "denied";
export type AsrServiceStatus = "not_started" | "starting" | "running" | "error";
export type ModelStatus = "not_loaded" | "loading" | "ready" | "error" | "unknown";
export type RecordingStatus = "idle" | "starting" | "recording" | "paused" | "stopping" | "error";
export type SpeakerFinalizationStatus = "idle" | "running" | "succeeded" | "failed";
export type TranscriptCorrectionStatus = "idle" | "running" | "succeeded" | "failed";
export type SummaryGenerationStatus = "idle" | "running" | "succeeded" | "failed";
export type ActiveAsrRuntime = "unknown" | "companion";
export type CompanionStatus =
  | "unknown"
  | "available"
  | "missing"
  | "invalid"
  | "not_running"
  | "stale"
  | "unavailable";

export type EchoNoteStatus = {
  microphonePermission: MicrophonePermissionStatus;
  asrRuntime: AsrRuntimeMode;
  activeAsrRuntime: ActiveAsrRuntime;
  asrService: AsrServiceStatus;
  model: ModelStatus;
  selectedModel: string;
  companionStatus: CompanionStatus;
  companionApiUrl: string | null;
  companionDiscoveryPath: string | null;
  companionMessage: string | null;
  selectedAudioInput: string;
  recording: RecordingStatus;
  currentMeetingPath: string | null;
  currentMeetingTitle: string | null;
  pendingChunkCount: number;
  lastTranscriptAt: number | null;
  speakerFinalization: SpeakerFinalizationStatus;
  speakerFinalizationMessage: string | null;
  transcriptCorrection: TranscriptCorrectionStatus;
  transcriptCorrectionMessage: string | null;
  summaryGeneration: SummaryGenerationStatus;
  summaryGenerationMessage: string | null;
  lastError: EchoNoteError | null;
};

export const DEFAULT_STATUS: EchoNoteStatus = {
  microphonePermission: "unknown",
  asrRuntime: "companion",
  activeAsrRuntime: "unknown",
  asrService: "not_started",
  model: "not_loaded",
  selectedModel: "Unknown",
  companionStatus: "unknown",
  companionApiUrl: null,
  companionDiscoveryPath: null,
  companionMessage: null,
  selectedAudioInput: "Default audio input",
  recording: "idle",
  currentMeetingPath: null,
  currentMeetingTitle: null,
  pendingChunkCount: 0,
  lastTranscriptAt: null,
  speakerFinalization: "idle",
  speakerFinalizationMessage: null,
  transcriptCorrection: "idle",
  transcriptCorrectionMessage: null,
  summaryGeneration: "idle",
  summaryGenerationMessage: null,
  lastError: null
};
