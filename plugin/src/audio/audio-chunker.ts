import type { AudioChunk } from "./audio-types";
import { encodePcm16Wav } from "./wav-encoder";

const TARGET_SAMPLE_RATE = 16000;

export class AudioChunker {
  private readonly chunkSampleCount: number;
  private pendingSamples: number[] = [];
  private emittedSamples = 0;
  private nextChunkIndex = 1;

  constructor(chunkLengthSeconds: number) {
    this.chunkSampleCount = TARGET_SAMPLE_RATE * chunkLengthSeconds;
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
    while (this.pendingSamples.length >= this.chunkSampleCount) {
      const samples = this.pendingSamples.splice(0, this.chunkSampleCount);
      chunks.push(this.createChunk(samples));
    }
    return chunks;
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
