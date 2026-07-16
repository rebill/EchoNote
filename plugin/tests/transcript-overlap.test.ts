import test from "node:test";
import assert from "node:assert/strict";
import type { TranscriptSegment } from "../src/asr/asr-types";
import {
  reconcileOverlappingTranscriptSegment,
  trimTranscriptPrefixOverlap
} from "../src/meeting/transcript-overlap";

test("removes an exact Chinese prefix repeated by an overlapping chunk", () => {
  assert.equal(
    trimTranscriptPrefixOverlap("今天我们讨论发布计划", "发布计划，然后确认负责人。"),
    "然后确认负责人。"
  );
});

test("removes an exact English word sequence repeated by an overlapping chunk", () => {
  assert.equal(
    trimTranscriptPrefixOverlap("We should review the release plan", "the release plan, then assign owners."),
    "then assign owners."
  );
});

test("keeps unrelated text unchanged", () => {
  assert.equal(trimTranscriptPrefixOverlap("讨论发布计划", "接下来确认负责人。"), "接下来确认负责人。");
});

test("reconciles overlapping segment timestamps and its single live turn", () => {
  const previous = segment("previous", "今天我们讨论发布计划", 0, 15_000);
  const current = segment("current", "发布计划，然后确认负责人。", 14_500, 20_000);

  const reconciled = reconcileOverlappingTranscriptSegment(previous, current);

  assert.ok(reconciled);
  assert.equal(reconciled.text, "然后确认负责人。");
  assert.equal(reconciled.started_at_ms, 15_000);
  assert.equal(reconciled.turns[0].text, "然后确认负责人。");
  assert.equal(reconciled.turns[0].started_at_ms, 15_000);
});

test("drops a fully duplicated overlapping segment", () => {
  const previous = segment("previous", "确认发布计划", 0, 15_000);
  const current = segment("current", "发布计划", 14_500, 15_000);

  assert.equal(reconcileOverlappingTranscriptSegment(previous, current), null);
});

function segment(
  chunkId: string,
  text: string,
  startedAtMs: number,
  endedAtMs: number
): TranscriptSegment {
  return {
    chunk_id: chunkId,
    text,
    turns: [{
      id: `${chunkId}-turn-001`,
      text,
      speaker: null,
      started_at_ms: startedAtMs,
      ended_at_ms: endedAtMs,
      confidence: null
    }],
    started_at_ms: startedAtMs,
    ended_at_ms: endedAtMs,
    language: "zh",
    model_id: "test-model"
  };
}
