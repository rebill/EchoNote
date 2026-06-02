import test from "node:test";
import assert from "node:assert/strict";
import { calculateFinalizeTimeoutMs } from "../src/asr/asr-service-client";

const SAMPLE_RATE = 16000;
const BYTES_PER_SAMPLE = 2;

test("calculateFinalizeTimeoutMs uses a generous minimum for short recordings", () => {
  const wav = wavHeaderForDuration(30_000);

  assert.equal(calculateFinalizeTimeoutMs(wav), 600_000);
});

test("calculateFinalizeTimeoutMs scales with recording duration", () => {
  const wav = wavHeaderForDuration(20 * 60_000);

  assert.equal(calculateFinalizeTimeoutMs(wav), 3_600_000);
});

test("calculateFinalizeTimeoutMs caps very long recordings", () => {
  const wav = wavHeaderForDuration(2 * 60 * 60_000);

  assert.equal(calculateFinalizeTimeoutMs(wav), 3_600_000);
});

function wavHeaderForDuration(durationMs: number): ArrayBuffer {
  const dataByteLength = Math.round((SAMPLE_RATE * durationMs * BYTES_PER_SAMPLE) / 1000);
  const buffer = new ArrayBuffer(44);
  const view = new DataView(buffer);

  writeAscii(view, 0, "RIFF");
  view.setUint32(4, 36 + dataByteLength, true);
  writeAscii(view, 8, "WAVE");
  writeAscii(view, 12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, SAMPLE_RATE, true);
  view.setUint32(28, SAMPLE_RATE * BYTES_PER_SAMPLE, true);
  view.setUint16(32, BYTES_PER_SAMPLE, true);
  view.setUint16(34, 16, true);
  writeAscii(view, 36, "data");
  view.setUint32(40, dataByteLength, true);
  return buffer;
}

function writeAscii(view: DataView, offset: number, value: string): void {
  for (let index = 0; index < value.length; index += 1) {
    view.setUint8(offset + index, value.charCodeAt(index));
  }
}
