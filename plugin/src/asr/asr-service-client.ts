import type { AudioChunk } from "../audio/audio-types";
import type {
  FinalizeTranscriptResponse,
  HealthResponse,
  ModelLoadResponse,
  ModelStatusResponse,
  TranscriptSegment
} from "./asr-types";

const FINALIZE_TIMEOUT_MS = 300_000;

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

    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), FINALIZE_TIMEOUT_MS);
    const response = await fetch(`${this.baseUrl}/transcript/finalize`, {
      method: "POST",
      body: formData,
      signal: controller.signal
    }).finally(() => window.clearTimeout(timeout));
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
