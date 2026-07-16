import test from "node:test";
import assert from "node:assert/strict";
import { buildSummarySystemPrompt, parseMeetingSummary } from "../src/llm/summary-json";

test("summary prompt requests a concise meeting title", () => {
  const prompt = buildSummarySystemPrompt("zh");

  assert.match(prompt, /meetingTitle/);
  assert.match(prompt, /without a date/i);
});

test("parseMeetingSummary reads meetingTitle from structured JSON", () => {
  const summary = parseMeetingSummary(JSON.stringify({
    meetingTitle: "版本发布计划",
    summary: "讨论了发布范围。",
    decisions: [],
    actionItems: [],
    keyPoints: [],
    openQuestions: []
  }));

  assert.equal(summary.meetingTitle, "版本发布计划");
  assert.equal(summary.summary, "讨论了发布范围。");
});
