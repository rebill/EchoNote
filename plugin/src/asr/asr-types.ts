export type AsrModelLifecycleStatus = "not_loaded" | "loading" | "ready" | "error";

export type HealthResponse = {
  status: "ok";
  service: "echonote-asr";
  version: string;
};

export type ModelStatusResponse = {
  model_id: string;
  status: AsrModelLifecycleStatus;
  error: string | null;
};

export type ModelLoadRequest = {
  model_id: string;
};

export type ModelLoadResponse = {
  model_id: string;
  status: AsrModelLifecycleStatus;
};

export type TranscribeRequestMetadata = {
  chunk_id: string;
  started_at_ms: number;
  ended_at_ms: number;
  language?: "auto" | "zh" | "en";
};

export type TranscriptSegment = {
  chunk_id: string;
  text: string;
  started_at_ms: number;
  ended_at_ms: number;
  language: string | null;
  model_id: string;
};

export type ShutdownResponse = {
  status: "shutting_down";
};
