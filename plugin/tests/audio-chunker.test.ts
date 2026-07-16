import test from "node:test";
import assert from "node:assert/strict";
import { AudioChunker } from "../src/audio/audio-chunker";

const SAMPLE_RATE = 16000;

test("cuts on silence after the minimum chunk duration", () => {
  const chunker = new AudioChunker(15);
  const input = concatSamples(tone(2100), silence(900));

  const chunks = chunker.addSamples(input, SAMPLE_RATE);

  assert.equal(chunks.length, 1);
  assert.equal(chunks[0].startedAtMs, 0);
  assert.equal(chunks[0].durationMs, 2900);
  assert.equal(chunks[0].overlapSamples, 0);
});

test("force cuts continuous speech at the max chunk duration", () => {
  const chunker = new AudioChunker(15);

  const chunks = chunker.addSamples(tone(16_000), SAMPLE_RATE);

  assert.equal(chunks.length, 1);
  assert.equal(chunks[0].startedAtMs, 0);
  assert.equal(chunks[0].durationMs, 15_000);
  assert.equal(chunks[0].overlapSamples, 0);
});

test("retains 500 ms of audio after a forced cut", () => {
  const chunker = new AudioChunker(15);

  const chunks = chunker.addSamples(tone(16_000), SAMPLE_RATE);
  const finalChunk = chunker.flush();

  assert.equal(chunks.length, 1);
  assert.ok(finalChunk);
  assert.equal(finalChunk.startedAtMs, 14_500);
  assert.equal(finalChunk.endedAtMs, 16_000);
  assert.equal(finalChunk.durationMs, 1500);
  assert.equal(finalChunk.overlapSamples, SAMPLE_RATE / 2);
});

test("can disable forced-cut overlap", () => {
  const chunker = new AudioChunker(15, { forcedChunkOverlapMs: 0 });

  chunker.addSamples(tone(16_000), SAMPLE_RATE);
  const finalChunk = chunker.flush();

  assert.ok(finalChunk);
  assert.equal(finalChunk.startedAtMs, 15_000);
  assert.equal(finalChunk.durationMs, 1000);
  assert.equal(finalChunk.overlapSamples, 0);
});

function tone(durationMs: number): Float32Array {
  return new Float32Array(Math.round((SAMPLE_RATE * durationMs) / 1000)).fill(0.1);
}

function silence(durationMs: number): Float32Array {
  return new Float32Array(Math.round((SAMPLE_RATE * durationMs) / 1000));
}

function concatSamples(...parts: Float32Array[]): Float32Array {
  const output = new Float32Array(parts.reduce((total, part) => total + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    output.set(part, offset);
    offset += part.length;
  }
  return output;
}
