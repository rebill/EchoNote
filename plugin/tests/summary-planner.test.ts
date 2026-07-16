import test from "node:test";
import assert from "node:assert/strict";
import { groupByRenderedBudget, mapWithConcurrency, splitTextAtBoundaries } from "../src/llm/summary-planner";

test("splitTextAtBoundaries preserves every input character exactly once", () => {
  const text = "第一段完整内容。\n第二段包含 emoji 🚀 和更多内容。\nThird paragraph is here.";
  const chunks = splitTextAtBoundaries(text, 20);

  assert.equal(chunks.join(""), text);
  assert.equal(chunks.every((chunk) => chunk.length <= 20), true);
  assert.equal(chunks.some((chunk) => chunk.endsWith("\n") || /[。.!?]$/.test(chunk)), true);
});

test("splitTextAtBoundaries does not split a surrogate pair", () => {
  const chunks = splitTextAtBoundaries("12345🚀67890", 6);

  assert.equal(chunks.join(""), "12345🚀67890");
  assert.equal(chunks.some((chunk) => chunk.includes("�")), false);
});

test("mapWithConcurrency preserves order and enforces the worker limit", async () => {
  let active = 0;
  let maxActive = 0;
  const result = await mapWithConcurrency([1, 2, 3, 4, 5], 2, async (value) => {
    active += 1;
    maxActive = Math.max(maxActive, active);
    await new Promise((resolve) => setTimeout(resolve, 2));
    active -= 1;
    return value * 2;
  });

  assert.deepEqual(result, [2, 4, 6, 8, 10]);
  assert.equal(maxActive, 2);
});

test("groupByRenderedBudget keeps every group within budget", () => {
  const groups = groupByRenderedBudget(["aaaa", "bbbb", "cc", "dddd"], 10, (value) => value);

  assert.deepEqual(groups, [["aaaa", "bbbb"], ["cc", "dddd"]]);
  assert.equal(groups.flat().join(""), "aaaabbbbccdddd");
});
