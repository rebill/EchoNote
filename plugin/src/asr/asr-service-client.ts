import type { AudioChunk } from "../audio/audio-types";
import type {
  FinalizeTranscriptResponse,
  HealthResponse,
  ModelLoadResponse,
  ModelStatusResponse,
  TranscriptSegment
} from "./asr-types";

const MIN_FINALIZE_TIMEOUT_MS = 600_000;
const MAX_FINALIZE_TIMEOUT_MS = 3_600_000;
const FINALIZE_TIMEOUT_AUDIO_MULTIPLIER = 6;

export class AsrServiceClient {
  constructor(private readonly baseUrl: string) {}

  async health(): Promise<HealthResponse> {
    return this.getJson<HealthResponse>("/health");
  }

  async modelStatus(): Promise<ModelStatusResponse> {
    return this.getJson<ModelStatusResponse>("/model/status");
  }

  async loadModel(modelId: string): Promise<ModelLoadResponse> {
    const response = await fetch(`${this.baseUrl}/model/load`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model_id: modelId })
    });
    return this.parseJsonResponse<ModelLoadResponse>(response);
  }

  async transcribe(chunk: AudioChunk, language: "auto" | "zh" | "en" = "zh"): Promise<TranscriptSegment> {
    const formData = new FormData();
    formData.append("audio", new Blob([chunk.wavBytes], { type: "application/octet-stream" }), `${chunk.id}.wav`);
    formData.append("chunk_id", chunk.id);
    formData.append("started_at_ms", String(chunk.startedAtMs));
    formData.append("ended_at_ms", String(chunk.endedAtMs));
    formData.append("language", language);

    const response = await fetch(`${this.baseUrl}/transcribe`, {
      method: "POST",
      body: formData
    });
    return this.parseJsonResponse<TranscriptSegment>(response);
  }

  async finalizeTranscript(
    meetingId: string,
    wavBytes: ArrayBuffer,
    segments: TranscriptSegment[],
    language: "auto" | "zh" | "en" = "zh",
    enableDiarization = true
  ): Promise<FinalizeTranscriptResponse> {
    const formData = new FormData();
    formData.append("audio", new Blob([wavBytes], { type: "application/octet-stream" }), `${meetingId}.wav`);
    formData.append("meeting_id", meetingId);
    formData.append("segments_json", JSON.stringify(segments));
    formData.append("language", language);
    formData.append("enable_diarization", String(enableDiarization));

    const timeoutMs = calculateFinalizeTimeoutMs(wavBytes);
    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), timeoutMs);
    const response = await fetch(`${this.baseUrl}/transcript/finalize`, {
      method: "POST",
      body: formData,
      signal: controller.signal
    })
      .catch((error) => {
        if (isAbortError(error)) {
          throw new Error(
            `Speaker finalization timed out after ${formatDuration(timeoutMs)}. ` +
              "The live transcript was kept. Try a shorter recording or wait for the ASR service to finish before retrying."
          );
        }
        throw error;
      })
      .finally(() => window.clearTimeout(timeout));
    return this.parseJsonResponse<FinalizeTranscriptResponse>(response);
  }

  async shutdown(): Promise<void> {
    try {
      await fetch(`${this.baseUrl}/shutdown`, { method: "POST" });
    } catch {
      // The service may exit before the HTTP response is fully observed.
    }
  }

  private async getJson<T>(path: string): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`);
    return this.parseJsonResponse<T>(response);
  }

  private async parseJsonResponse<T>(response: Response): Promise<T> {
    const body = await response.text();
    if (!response.ok) {
      throw new Error(body || `ASR service request failed with ${response.status}`);
    }
    return JSON.parse(body) as T;
  }
}

export function calculateFinalizeTimeoutMs(wavBytes: ArrayBuffer): number {
  const durationMs = readWavDurationMs(wavBytes);
  const dynamicTimeoutMs = Math.ceil(durationMs * FINALIZE_TIMEOUT_AUDIO_MULTIPLIER);
  return Math.min(MAX_FINALIZE_TIMEOUT_MS, Math.max(MIN_FINALIZE_TIMEOUT_MS, dynamicTimeoutMs));
}

function readWavDurationMs(wavBytes: ArrayBuffer): number {
  if (wavBytes.byteLength < 44) {
    return 0;
  }

  const view = new DataView(wavBytes);
  const sampleRate = view.getUint32(24, true);
  const byteRate = view.getUint32(28, true);
  const dataBytes = findWavDataByteLength(view);
  if (sampleRate <= 0 || byteRate <= 0 || dataBytes <= 0) {
    return 0;
  }

  return Math.round((dataBytes / byteRate) * 1000);
}

function findWavDataByteLength(view: DataView): number {
  for (let offset = 12; offset + 8 <= view.byteLength; ) {
    const chunkId = readAscii(view, offset, 4);
    const chunkSize = view.getUint32(offset + 4, true);
    if (chunkId === "data") {
      return chunkSize;
    }
    offset += 8 + chunkSize + (chunkSize % 2);
  }
  return 0;
}

function readAscii(view: DataView, offset: number, length: number): string {
  let value = "";
  for (let index = 0; index < length; index += 1) {
    value += String.fromCharCode(view.getUint8(offset + index));
  }
  return value;
}

function isAbortError(error: unknown): boolean {
  if (error instanceof DOMException && error.name === "AbortError") {
    return true;
  }
  return error instanceof Error && (error.name === "AbortError" || /aborted/i.test(error.message));
}

function formatDuration(ms: number): string {
  const minutes = Math.max(1, Math.round(ms / 60_000));
  return `${minutes} minute${minutes === 1 ? "" : "s"}`;
}
