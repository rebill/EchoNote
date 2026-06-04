import test from "node:test";
import assert from "node:assert/strict";
import {
  getMeetingArtifactPaths,
  getSegmentsPathForAudioPath,
  normalizeVaultPath,
  sanitizeMeetingId
} from "../src/meeting/meeting-artifacts";

test("getMeetingArtifactPaths creates matching audio and segments paths", () => {
  const paths = getMeetingArtifactPaths(
    { audioSaveFolder: "Meetings/audio" },
    "2026-06-02 16-02 Meeting"
  );

  assert.deepEqual(paths, {
    folder: "Meetings/audio/2026-06-02 16-02 Meeting",
    audioPath: "Meetings/audio/2026-06-02 16-02 Meeting/2026-06-02 16-02 Meeting.wav",
    segmentsPath: "Meetings/audio/2026-06-02 16-02 Meeting/2026-06-02 16-02 Meeting.segments.json"
  });
});

test("getMeetingArtifactPaths sanitizes meeting title path separators", () => {
  const paths = getMeetingArtifactPaths({ audioSaveFolder: "Meetings/audio/" }, "Team/Design:Sync");

  assert.equal(paths.audioPath, "Meetings/audio/Team-Design-Sync/Team-Design-Sync.wav");
  assert.equal(paths.segmentsPath, "Meetings/audio/Team-Design-Sync/Team-Design-Sync.segments.json");
});

test("getSegmentsPathForAudioPath follows the saved wav basename", () => {
  assert.equal(
    getSegmentsPathForAudioPath("Meetings/audio/Meeting/Meeting 2.wav"),
    "Meetings/audio/Meeting/Meeting 2.segments.json"
  );
});

test("normalizeVaultPath removes duplicate slashes and leading vault slash", () => {
  assert.equal(normalizeVaultPath("/Meetings//audio/Meeting/"), "Meetings/audio/Meeting");
});

test("sanitizeMeetingId keeps the ASR meeting id form-stable", () => {
  assert.equal(sanitizeMeetingId("2026-06-02 16:02 Meeting"), "2026-06-02-16-02-Meeting");
});
