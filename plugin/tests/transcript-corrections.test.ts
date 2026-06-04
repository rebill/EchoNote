import test from "node:test";
import assert from "node:assert/strict";
import {
  applyTranscriptCorrections,
  parseTranscriptCorrectionRules
} from "../src/meeting/transcript-corrections";

test("parseTranscriptCorrectionRules parses line based correction table", () => {
  const rules = parseTranscriptCorrectionRules(`
    # names
    木溪 => 沐曦
    Open AI => OpenAI
    invalid
  `);

  assert.deepEqual(rules, [
    { from: "Open AI", to: "OpenAI" },
    { from: "木溪", to: "沐曦" }
  ]);
});

test("applyTranscriptCorrections replaces all matching ASR mistakes", () => {
  const corrected = applyTranscriptCorrections(
    "今天木溪会介绍木溪芯片和 Open AI 接口。",
    "木溪 => 沐曦\nOpen AI => OpenAI"
  );

  assert.equal(corrected, "今天沐曦会介绍沐曦芯片和 OpenAI 接口。");
});

test("applyTranscriptCorrections prefers longer source phrases", () => {
  const corrected = applyTranscriptCorrections("木溪芯片来自木溪。", "木溪 => 沐曦\n木溪芯片 => 沐曦芯片");

  assert.equal(corrected, "沐曦芯片来自沐曦。");
});
