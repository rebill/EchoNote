import test from "node:test";
import assert from "node:assert/strict";
import { resolveMeetingTarget } from "../src/meeting/meeting-target";

test("resolveMeetingTarget always prefers the active recording meeting", async () => {
  const result = await resolveMeetingTarget({
    currentMeeting: "recording",
    activeMeetingCandidate: "active",
    storedMeetingCandidate: "stored",
    isVerifiedMeeting: async () => false
  });

  assert.equal(result, "recording");
});

test("resolveMeetingTarget uses a verified active meeting note", async () => {
  const result = await resolveMeetingTarget({
    currentMeeting: null,
    activeMeetingCandidate: "active",
    storedMeetingCandidate: "stored",
    isVerifiedMeeting: async (candidate) => candidate === "active"
  });

  assert.equal(result, "active");
});

test("resolveMeetingTarget falls back to the stored meeting when the active note is unrelated", async () => {
  const result = await resolveMeetingTarget({
    currentMeeting: null,
    activeMeetingCandidate: "unrelated",
    storedMeetingCandidate: "stored",
    isVerifiedMeeting: async (candidate) => candidate === "stored"
  });

  assert.equal(result, "stored");
});

test("resolveMeetingTarget rejects unverified candidates", async () => {
  const result = await resolveMeetingTarget({
    currentMeeting: null,
    activeMeetingCandidate: "unrelated",
    storedMeetingCandidate: "stale",
    isVerifiedMeeting: async () => false
  });

  assert.equal(result, null);
});
