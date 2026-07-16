import test from "node:test";
import assert from "node:assert/strict";
import { MeetingAudioSpool } from "../src/audio/meeting-audio-spool";
import { encodePcm16Wav } from "../src/audio/wav-encoder";

test("MeetingAudioSpool keeps small meetings in memory", async () => {
  const spool = new MeetingAudioSpool(1024);
  const first = encodePcm16Wav(new Float32Array([0, 0.5]));
  const second = encodePcm16Wav(new Float32Array([-0.5, 1]));

  await spool.append(first);
  await spool.append(second);
  const wav = await spool.toWav();

  assert.equal(spool.storageMode, "memory");
  assert.equal(spool.pcmByteLength, 8);
  assert.deepEqual(
    [...new Uint8Array(wav ?? new ArrayBuffer(0), 44)],
    [...new Uint8Array(first, 44), ...new Uint8Array(second, 44)]
  );
  await spool.dispose();
});

test("MeetingAudioSpool spills PCM to disk after the memory threshold", async () => {
  const spool = new MeetingAudioSpool(4);
  const first = encodePcm16Wav(new Float32Array([0, 0.5]));
  const second = encodePcm16Wav(new Float32Array([-0.5, 1]));

  await Promise.all([spool.append(first), spool.append(second)]);
  const wav = await spool.toWav();

  assert.equal(spool.storageMode, "disk");
  assert.equal(spool.pcmByteLength, 8);
  assert.deepEqual(
    [...new Uint8Array(wav ?? new ArrayBuffer(0), 44)],
    [...new Uint8Array(first, 44), ...new Uint8Array(second, 44)]
  );
  await spool.dispose();
});

test("MeetingAudioSpool rejects writes after disposal", async () => {
  const spool = new MeetingAudioSpool();
  await spool.dispose();

  await assert.rejects(() => spool.append(encodePcm16Wav(new Float32Array([0]))), /disposed/);
});
