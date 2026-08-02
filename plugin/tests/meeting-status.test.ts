import assert from "node:assert/strict";
import test from "node:test";
import { createMeetingStartStatus } from "../src/status/meeting-status";

test("createMeetingStartStatus clears state left by the previous meeting", () => {
  assert.deepEqual(createMeetingStartStatus(), {
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
  });
});
