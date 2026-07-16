import test from "node:test";
import assert from "node:assert/strict";
import type { LlmProvider, LlmTextRequest, MeetingSummary, SummaryRequest } from "../src/llm/llm-types";
import { SummaryService } from "../src/llm/summary-service";
import { DEFAULT_SETTINGS } from "../src/settings/settings";

test("SummaryService bounds partial-summary concurrency and preserves merge order", async () => {
  let active = 0;
  let maxActive = 0;
  const requests: SummaryRequest[] = [];
  const provider = createProvider(async (request) => {
    requests.push(request);
    active += 1;
    maxActive = Math.max(maxActive, active);
    await new Promise((resolve) => setTimeout(resolve, 2));
    active -= 1;
    return summaryFor(request.transcript);
  });
  const service = new SummaryService({
    chunkChars: 12,
    mergeInputChars: 10_000,
    concurrency: 2,
    attempts: 1,
    providerFactory: () => provider
  });

  await service.summarize("chunk one.\nchunk two.\nchunk three.\nchunk four.", DEFAULT_SETTINGS);

  assert.equal(maxActive, 2);
  const mergeRequest = requests.find((request) => request.prompt.startsWith("Merge these partial"));
  assert.ok(mergeRequest);
  assert.ok(mergeRequest.transcript.indexOf("chunk one") < mergeRequest.transcript.indexOf("chunk four"));
});

test("SummaryService retries only the failed partial request", async () => {
  const attempts = new Map<string, number>();
  const provider = createProvider(async (request) => {
    const count = (attempts.get(request.transcript) ?? 0) + 1;
    attempts.set(request.transcript, count);
    if (request.transcript.includes("retry") && count === 1) {
      throw new Error("temporary failure");
    }
    return summaryFor(request.transcript);
  });
  const service = new SummaryService({
    chunkChars: 8,
    mergeInputChars: 10_000,
    concurrency: 2,
    attempts: 2,
    providerFactory: () => provider
  });

  await service.summarize("retry.\nstable.", DEFAULT_SETTINGS);

  const partialAttempts = [...attempts.entries()].filter(([text]) => !text.includes("Partial Summary"));
  assert.equal(partialAttempts.find(([text]) => text.includes("retry"))?.[1], 2);
  assert.equal(partialAttempts.find(([text]) => text.includes("stable"))?.[1], 1);
});

test("SummaryService merges hierarchically without exceeding the merge budget", async () => {
  const mergeInputLengths: number[] = [];
  let mergeCount = 0;
  const provider = createProvider(async (request) => {
    if (request.prompt.startsWith("Merge these partial")) {
      mergeInputLengths.push(request.transcript.length);
      mergeCount += 1;
      return summaryFor(`merged-${mergeCount}`);
    }
    return summaryFor(`partial-${request.transcript.slice(0, 6)}`);
  });
  const service = new SummaryService({
    chunkChars: 10,
    mergeInputChars: 450,
    concurrency: 2,
    attempts: 1,
    providerFactory: () => provider
  });

  const progress: string[] = [];
  const result = await service.summarize(
    "one line.\ntwo line.\nthree line.\nfour line.\nfive line.\nsix line.\nseven line.\neight line.",
    DEFAULT_SETTINGS,
    (value) => progress.push(`${value.stage}:${value.completed}/${value.total}`)
  );

  assert.match(result.summary, /^merged-/);
  assert.equal(mergeCount > 1, true);
  assert.equal(mergeInputLengths.every((length) => length <= 450), true);
  assert.equal(progress.some((value) => value.startsWith("partial:")), true);
  assert.equal(progress.some((value) => value.startsWith("merge:")), true);
});

function createProvider(generateSummary: (request: SummaryRequest) => Promise<MeetingSummary>): LlmProvider {
  return {
    id: "openai-compatible",
    generateSummary,
    async generateText(_request: LlmTextRequest): Promise<string> {
      throw new Error("not used");
    }
  };
}

function summaryFor(value: string): MeetingSummary {
  return {
    meetingTitle: value.trim().slice(0, 24) || "Meeting",
    summary: value.trim(),
    decisions: [],
    actionItems: [],
    keyPoints: [],
    openQuestions: []
  };
}
