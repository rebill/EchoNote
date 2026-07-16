import test from "node:test";
import assert from "node:assert/strict";
import { concatWavFiles, encodePcm16Wav } from "../src/audio/wav-encoder";

test("concatWavFiles preserves PCM payload order", () => {
  const first = encodePcm16Wav(new Float32Array([0, 0.5, -0.5]));
  const second = encodePcm16Wav(new Float32Array([1, -1]));
  const combined = concatWavFiles([first, second]);

  assert.equal(combined.byteLength, 44 + (5 * 2));
  assert.deepEqual(
    [...new Uint8Array(combined, 44)],
    [...new Uint8Array(first, 44), ...new Uint8Array(second, 44)]
  );
  assert.equal(new DataView(combined).getUint32(40, true), 10);
});

test("concatWavFiles ignores header-only chunks", () => {
  const combined = concatWavFiles([new ArrayBuffer(44)]);

  assert.equal(combined.byteLength, 44);
  assert.equal(new DataView(combined).getUint32(40, true), 0);
});
