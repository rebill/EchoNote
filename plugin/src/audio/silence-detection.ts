import type { AudioChunk } from "./audio-types";

export const SILENCE_RMS_THRESHOLD = 0.002;
const MS_PER_MINUTE = 60_000;

export function isSilentAudioChunk(chunk: Pick<AudioChunk, "rms">): boolean {
  return chunk.rms <= SILENCE_RMS_THRESHOLD;
}

export function nextConsecutiveSilenceMs(
  currentSilenceMs: number,
  chunk: Pick<AudioChunk, "durationMs" | "rms">
): number {
  if (!isSilentAudioChunk(chunk)) {
    return 0;
  }
  return currentSilenceMs + Math.max(0, chunk.durationMs);
}

export function autoStopSilenceThresholdMs(minutes: number): number {
  return minutes * MS_PER_MINUTE;
}
