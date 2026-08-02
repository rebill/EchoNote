import type { EchoNoteStatus } from "./status-types";

export function createMeetingStartStatus(): Partial<EchoNoteStatus> {
  return {
    asrService: "starting",
    model: "unknown",
    recording: "starting",
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
}
