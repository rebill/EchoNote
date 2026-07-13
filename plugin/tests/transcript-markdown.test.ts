import test from "node:test";
import assert from "node:assert/strict";
import { formatTranscriptTurns, parseTranscriptMarkdown } from "../src/meeting/transcript-markdown";

test("parseTranscriptMarkdown preserves generated speaker labels and timestamps", () => {
  const parsed = parseTranscriptMarkdown("[00:01] Speaker 1: 今天木溪会介绍。\n[01:02:03] Speaker 2: Open AI 接口。");

  assert.equal(parsed.hasTimestamps, true);
  assert.equal(parsed.turns.length, 2);
  assert.deepEqual(parsed.turns.map((turn) => turn.speaker), ["Speaker 1", "Speaker 2"]);
  assert.deepEqual(parsed.turns.map((turn) => turn.started_at_ms), [1000, 3723000]);
  assert.equal(parsed.turns[0].text, "今天木溪会介绍。");
});

test("parseTranscriptMarkdown keeps ordinary colon text as transcript text", () => {
  const parsed = parseTranscriptMarkdown("注意: 这里不是 speaker label");

  assert.equal(parsed.hasTimestamps, false);
  assert.equal(parsed.turns.length, 1);
  assert.equal(parsed.turns[0].speaker, null);
  assert.equal(parsed.turns[0].text, "注意: 这里不是 speaker label");
});

test("formatTranscriptTurns applies deterministic correction rules", () => {
  const rendered = formatTranscriptTurns(
    [
      {
        id: "turn-1",
        text: "今天木溪会介绍 Open AI。",
        speaker: "Speaker 1",
        started_at_ms: 61000,
        ended_at_ms: 62000,
        confidence: null
      }
    ],
    true,
    "木溪 => 沐曦\nOpen AI => OpenAI"
  );

  assert.equal(rendered, "[01:01] Speaker 1: 今天沐曦会介绍 OpenAI。");
});
