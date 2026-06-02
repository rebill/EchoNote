import type { AudioChunk } from "./audio-types";
import { encodePcm16Wav } from "./wav-encoder";

const TARGET_SAMPLE_RATE = 16000;
const DEFAULT_MIN_CHUNK_MS = 2000;
const DEFAULT_MAX_CHUNK_MS = 15000;
const DEFAULT_SILENCE_DURATION_MS = 800;
const DEFAULT_SILENCE_RMS_THRESHOLD = 0.002;
const DEFAULT_ANALYSIS_FRAME_MS = 20;

export type AudioChunkerOptions = {
  minChunkMs?: number;
  maxChunkMs?: number;
  silenceDurationMs?: number;
  silenceRmsThreshold?: number;
  analysisFrameMs?: number;
};

export class AudioChunker {
  private readonly minChunkSamples: number;
  private readonly maxChunkSamples: number;
  private readonly silenceFrameCount: number;
  private readonly silenceRmsThreshold: number;
  private readonly analysisFrameSamples: number;
  private pendingSamples: number[] = [];
  private emittedSamples = 0;
  private nextChunkIndex = 1;

  constructor(chunkLengthSeconds: number, options: AudioChunkerOptions = {}) {
    const maxChunkMs = Math.min(
      DEFAULT_MAX_CHUNK_MS,
      Math.max(1000, Math.round(chunkLengthSeconds * 1000))
    );
    this.minChunkSamples = msToSamples(options.minChunkMs ?? DEFAULT_MIN_CHUNK_MS);
    this.maxChunkSamples = msToSamples(options.maxChunkMs ?? maxChunkMs);
    this.analysisFrameSamples = msToSamples(options.analysisFrameMs ?? DEFAULT_ANALYSIS_FRAME_MS);
    this.silenceFrameCount = Math.max(
      1,
      Math.ceil((options.silenceDurationMs ?? DEFAULT_SILENCE_DURATION_MS) / (options.analysisFrameMs ?? DEFAULT_ANALYSIS_FRAME_MS))
    );
    this.silenceRmsThreshold = options.silenceRmsThreshold ?? DEFAULT_SILENCE_RMS_THRESHOLD;
  }

  addSamples(input: Float32Array, inputSampleRate: number): AudioChunk[] {
    const resampled = resampleTo16k(input, inputSampleRate);
    for (const sample of resampled) {
      this.pendingSamples.push(sample);
    }

    return this.drainFullChunks();
  }

  flush(): AudioChunk | null {
    if (this.pendingSamples.length === 0) {
      return null;
    }

    const samples = this.pendingSamples.splice(0);
    return this.createChunk(samples);
  }

  private drainFullChunks(): AudioChunk[] {
    const chunks: AudioChunk[] = [];
    while (true) {
      const cutSampleCount = this.findNextCutSampleCount();
      if (cutSampleCount === null) {
        break;
      }
      const samples = this.pendingSamples.splice(0, cutSampleCount);
      chunks.push(this.createChunk(samples));
    }
    return chunks;
  }

  private findNextCutSampleCount(): number | null {
    if (this.pendingSamples.length < this.minChunkSamples) {
      return null;
    }

    const searchLimit = Math.min(this.pendingSamples.length, this.maxChunkSamples);
    let silentFrames = 0;

    for (let start = 0; start + this.analysisFrameSamples <= searchLimit; start += this.analysisFrameSamples) {
      const frameEnd = start + this.analysisFrameSamples;
      const rms = calculateRms(this.pendingSamples.slice(start, frameEnd));
      if (rms <= this.silenceRmsThreshold) {
        silentFrames += 1;
      } else {
        silentFrames = 0;
      }

      if (frameEnd >= this.minChunkSamples && silentFrames >= this.silenceFrameCount) {
        return frameEnd;
      }
    }

    if (this.pendingSamples.length >= this.maxChunkSamples) {
      return this.maxChunkSamples;
    }

    return null;
  }

  private createChunk(samples: number[]): AudioChunk {
    const startedAtMs = Math.round((this.emittedSamples / TARGET_SAMPLE_RATE) * 1000);
    this.emittedSamples += samples.length;
    const endedAtMs = Math.round((this.emittedSamples / TARGET_SAMPLE_RATE) * 1000);
    const chunkIndex = this.nextChunkIndex;
    this.nextChunkIndex += 1;

    return {
      id: `chunk-${String(chunkIndex).padStart(6, "0")}`,
      startedAtMs,
      endedAtMs,
      wavBytes: encodePcm16Wav(Float32Array.from(samples), TARGET_SAMPLE_RATE),
      createdAt: Date.now(),
      durationMs: endedAtMs - startedAtMs,
      rms: calculateRms(samples)
    };
  }
}

function msToSamples(ms: number): number {
  return Math.max(1, Math.round((TARGET_SAMPLE_RATE * ms) / 1000));
}

function calculateRms(samples: number[]): number {
  if (samples.length === 0) {
    return 0;
  }

  let sum = 0;
  for (const sample of samples) {
    sum += sample * sample;
  }
  return Math.sqrt(sum / samples.length);
}

function resampleTo16k(input: Float32Array, inputSampleRate: number): Float32Array {
  if (inputSampleRate === TARGET_SAMPLE_RATE) {
    return new Float32Array(input);
  }

  const ratio = inputSampleRate / TARGET_SAMPLE_RATE;
  const outputLength = Math.max(1, Math.floor(input.length / ratio));
  const output = new Float32Array(outputLength);

  for (let i = 0; i < outputLength; i += 1) {
    const sourceIndex = i * ratio;
    const leftIndex = Math.floor(sourceIndex);
    const rightIndex = Math.min(leftIndex + 1, input.length - 1);
    const weight = sourceIndex - leftIndex;
    output[i] = input[leftIndex] * (1 - weight) + input[rightIndex] * weight;
  }

  return output;
}
