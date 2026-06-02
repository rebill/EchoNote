import test from "node:test";
import assert from "node:assert/strict";
import { validateCompanionDiscovery } from "../src/asr/companion-discovery";

const baseDiscovery = {
  version: 1,
  app: "EchoNote",
  service: "echonote-asr",
  status: "running",
  baseUrl: "http://127.0.0.1:8765",
  host: "127.0.0.1",
  port: 8765,
  backend: "fake",
  modelId: "mlx-community/Qwen3-ASR-0.6B-4bit",
  modelStatus: "ready",
  pid: 12345,
  updatedAt: "2026-05-21T06:34:00.000Z"
} as const;

test("validateCompanionDiscovery accepts discovery without capabilities", () => {
  const result = validateCompanionDiscovery(baseDiscovery);

  assert.equal(result.ok, true);
});

test("validateCompanionDiscovery accepts optional v0.4 capabilities", () => {
  const result = validateCompanionDiscovery({
    ...baseDiscovery,
    capabilities: {
      adaptiveChunking: true,
      speakerDiarization: "unavailable"
    }
  });

  assert.equal(result.ok, true);
});

test("validateCompanionDiscovery rejects invalid capabilities", () => {
  const result = validateCompanionDiscovery({
    ...baseDiscovery,
    capabilities: {
      adaptiveChunking: "yes"
    }
  });

  assert.equal(result.ok, false);
});
