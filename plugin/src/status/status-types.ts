import type { EchoNoteError } from "../utils/errors";

export type MicrophonePermissionStatus = "unknown" | "granted" | "denied";
export type AsrServiceStatus = "not_started" | "starting" | "running" | "error";
export type ModelStatus = "not_loaded" | "loading" | "ready" | "error";
export type RecordingStatus = "idle" | "recording" | "paused" | "stopping" | "error";

export type EchoNoteStatus = {
  microphonePermission: MicrophonePermissionStatus;
  asrService: AsrServiceStatus;
  model: ModelStatus;
  selectedModel: string;
  selectedAudioInput: string;
  recording: RecordingStatus;
  currentMeetingPath: string | null;
  currentMeetingTitle: string | null;
  pendingChunkCount: number;
  lastTranscriptAt: number | null;
  lastError: EchoNoteError | null;
};

export const DEFAULT_STATUS: EchoNoteStatus = {
  microphonePermission: "unknown",
  asrService: "not_started",
  model: "not_loaded",
  selectedModel: "mlx-community/Qwen3-ASR-0.6B-4bit",
  selectedAudioInput: "Default audio input",
  recording: "idle",
  currentMeetingPath: null,
  currentMeetingTitle: null,
  pendingChunkCount: 0,
  lastTranscriptAt: null,
  lastError: null
};
