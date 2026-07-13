import test from "node:test";
import assert from "node:assert/strict";
import type { TranscriptTurn } from "../src/asr/asr-types";
import type { EchoNoteSettings } from "../src/settings/settings";
import { DEFAULT_SETTINGS } from "../src/settings/settings";
import type { LlmProvider, LlmTextRequest, MeetingSummary, SummaryRequest } from "../src/llm/llm-types";
import { parseCorrectionResponse, TranscriptCorrectionService } from "../src/llm/transcript-correction-service";

test("parseCorrectionResponse extracts fenced JSON", () => {
  const turns = parseCorrectionResponse('```json\n{"turns":[{"id":"turn-1","text":"你好。"}]}\n```');

  assert.deepEqual(turns, [{ id: "turn-1", text: "你好。" }]);
});

test("TranscriptCorrectionService applies deterministic rules before and after LLM correction", async () => {
  const provider = createProvider(() => JSON.stringify({ turns: [{ id: "turn-1", text: "木溪芯片已修正。" }] }));
  const service = new TranscriptCorrectionService({ providerFactory: () => provider });

  const result = await service.correctTurns(
    [turn("turn-1", "今天木溪芯片会介绍。")],
    settings("木溪 => 沐曦\n木溪芯片 => 沐曦芯片")
  );

  assert.equal(result.acceptedTurns, 1);
  assert.equal(result.changedTurns, 1);
  assert.equal(result.rejectedTurns, 0);
  assert.equal(result.turns[0].text, "沐曦芯片已修正。");
});

test("TranscriptCorrectionService rejects unsafe length changes per turn", async () => {
  const provider = createProvider(() =>
    JSON.stringify({ turns: [{ id: "turn-1", text: "删改过多。" }] })
  );
  const service = new TranscriptCorrectionService({ providerFactory: () => provider });
  const originalText = "这是一段比较长的会议转录文本，用来验证模型不能大幅删除原始内容。";

  const result = await service.correctTurns([turn("turn-1", originalText)], settings(""));

  assert.equal(result.acceptedTurns, 0);
  assert.equal(result.changedTurns, 0);
  assert.equal(result.rejectedTurns, 1);
  assert.equal(result.turns[0].text, originalText);
});

test("TranscriptCorrectionService counts accepted unchanged turns separately from changed turns", async () => {
  const provider = createProvider(() =>
    JSON.stringify({ turns: [{ id: "turn-1", text: "今天沐曦会介绍。" }] })
  );
  const service = new TranscriptCorrectionService({ providerFactory: () => provider });

  const result = await service.correctTurns([turn("turn-1", "今天沐曦会介绍。")], settings(""));

  assert.equal(result.acceptedTurns, 1);
  assert.equal(result.changedTurns, 0);
  assert.equal(result.rejectedTurns, 0);
});

test("TranscriptCorrectionService keeps original batch when the provider fails", async () => {
  const provider = createProvider(() => {
    throw new Error("request failed");
  });
  const service = new TranscriptCorrectionService({ providerFactory: () => provider });

  const result = await service.correctTurns([turn("turn-1", "今天木溪会介绍。")], settings("木溪 => 沐曦"));

  assert.equal(result.acceptedTurns, 0);
  assert.equal(result.changedTurns, 0);
  assert.equal(result.rejectedTurns, 1);
  assert.equal(result.failedBatches, 1);
  assert.equal(result.turns[0].text, "今天沐曦会介绍。");
});

function settings(transcriptCorrectionRules: string): EchoNoteSettings {
  return {
    ...DEFAULT_SETTINGS,
    openaiApiKey: "test",
    openaiBaseUrl: "https://example.test/v1",
    openaiModel: "test-model",
    transcriptCorrectionRules
  };
}

function turn(id: string, text: string): TranscriptTurn {
  return {
    id,
    text,
    speaker: "Speaker 1",
    started_at_ms: 0,
    ended_at_ms: 1000,
    confidence: null
  };
}

function createProvider(generateText: (request: LlmTextRequest) => string): LlmProvider {
  return {
    id: "openai-compatible",
    async generateText(request: LlmTextRequest): Promise<string> {
      return generateText(request);
    },
    async generateSummary(_request: SummaryRequest): Promise<MeetingSummary> {
      throw new Error("not used");
    }
  };
}
