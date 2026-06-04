import test from "node:test";
import assert from "node:assert/strict";
import {
  autoStopSilenceThresholdMs,
  isSilentAudioChunk,
  nextConsecutiveSilenceMs
} from "../src/audio/silence-detection";

test("isSilentAudioChunk follows the shared RMS silence threshold", () => {
  assert.equal(isSilentAudioChunk({ rms: 0.001 }), true);
  assert.equal(isSilentAudioChunk({ rms: 0.1 }), false);
});

test("nextConsecutiveSilenceMs accumulates silence and resets on audible chunks", () => {
  let silenceMs = nextConsecutiveSilenceMs(0, { durationMs: 2800, rms: 0 });
  silenceMs = nextConsecutiveSilenceMs(silenceMs, { durationMs: 2800, rms: 0.001 });
  assert.equal(silenceMs, 5600);

  silenceMs = nextConsecutiveSilenceMs(silenceMs, { durationMs: 2000, rms: 0.1 });
  assert.equal(silenceMs, 0);
});

test("autoStopSilenceThresholdMs converts minutes to milliseconds", () => {
  assert.equal(autoStopSilenceThresholdMs(10), 600_000);
});
