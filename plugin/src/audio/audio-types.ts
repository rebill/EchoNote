export type AudioChunk = {
  id: string;
  startedAtMs: number;
  endedAtMs: number;
  overlapSamples: number;
  wavBytes: ArrayBuffer;
  createdAt: number;
  durationMs: number;
  rms: number;
};

export type AudioChunkMetadata = {
  id: string;
  startedAtMs: number;
  endedAtMs: number;
  sampleRate: 16000;
  channels: 1;
  encoding: "pcm_s16le";
};

export type RawAudioSaveMode = "disabled" | "enabled";
