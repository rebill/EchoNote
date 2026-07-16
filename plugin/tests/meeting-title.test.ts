import test from "node:test";
import assert from "node:assert/strict";
import { createSummarizedMeetingTitle, normalizeMeetingTopic } from "../src/meeting/meeting-title";

test("createSummarizedMeetingTitle uses the note date and generated meeting topic", () => {
  const title = createSummarizedMeetingTitle(
    "# Old\n\n- Date: 2026-07-13\n",
    "2026-07-13 09-00 Meeting",
    "EchoNote 0.7 发布计划",
    "A longer summary.",
    new Date(0)
  );

  assert.equal(title, "2026-07-13_EchoNote 0.7 发布计划");
});

test("createSummarizedMeetingTitle falls back to the summary and creation date", () => {
  const title = createSummarizedMeetingTitle(
    "# Old\n",
    "Meeting",
    "",
    "讨论移动端发布范围。后续内容不应进入标题。",
    new Date(2026, 6, 12, 9, 30)
  );

  assert.equal(title, "2026-07-12_讨论移动端发布范围");
});

test("normalizeMeetingTopic removes duplicate dates and unsafe filename punctuation", () => {
  assert.equal(normalizeMeetingTopic("2026-07-13_季度复盘 / 下一步"), "季度复盘 - 下一步");
});
