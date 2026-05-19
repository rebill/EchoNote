import type { AudioChunk } from "./audio-types";
import { AudioChunker } from "./audio-chunker";

type ChunkHandler = (chunk: AudioChunk) => void;

export class AudioRecorder {
  private audioContext: AudioContext | null = null;
  private sourceNode: MediaStreamAudioSourceNode | null = null;
  private processorNode: ScriptProcessorNode | null = null;
  private stream: MediaStream | null = null;
  private chunker: AudioChunker | null = null;
  private onChunk: ChunkHandler | null = null;
  private paused = false;

  async checkPermission(): Promise<PermissionState | "unknown"> {
    if (!navigator.permissions?.query) {
      return "unknown";
    }

    try {
      const result = await navigator.permissions.query({ name: "microphone" as PermissionName });
      return result.state;
    } catch {
      return "unknown";
    }
  }

  async requestPermission(): Promise<void> {
    if (!navigator.mediaDevices?.getUserMedia) {
      throw new Error("navigator.mediaDevices.getUserMedia is not available in this Obsidian environment.");
    }

    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    for (const track of stream.getTracks()) {
      track.stop();
    }
  }

  async listInputDevices(): Promise<MediaDeviceInfo[]> {
    if (!navigator.mediaDevices?.enumerateDevices) {
      throw new Error("navigator.mediaDevices.enumerateDevices is not available in this Obsidian environment.");
    }

    const devices = await navigator.mediaDevices.enumerateDevices();
    return devices.filter((device) => device.kind === "audioinput");
  }

  async start(chunkLengthSeconds: number, deviceId: string, onChunk: ChunkHandler): Promise<void> {
    await this.stop();

    this.onChunk = onChunk;
    this.chunker = new AudioChunker(chunkLengthSeconds);
    this.paused = false;
    this.stream = await navigator.mediaDevices.getUserMedia({
      audio: buildAudioConstraints(deviceId)
    });
    this.audioContext = new AudioContext();
    this.sourceNode = this.audioContext.createMediaStreamSource(this.stream);
    this.processorNode = this.audioContext.createScriptProcessor(4096, 1, 1);
    this.processorNode.onaudioprocess = (event) => {
      if (this.paused || !this.chunker || !this.onChunk || !this.audioContext) {
        return;
      }

      const input = event.inputBuffer.getChannelData(0);
      const chunks = this.chunker.addSamples(input, this.audioContext.sampleRate);
      for (const chunk of chunks) {
        this.onChunk(chunk);
      }
    };

    this.sourceNode.connect(this.processorNode);
    this.processorNode.connect(this.audioContext.destination);
  }

  pause(): void {
    this.paused = true;
  }

  resume(): void {
    this.paused = false;
  }

  async stop(): Promise<AudioChunk | null> {
    const finalChunk = this.chunker?.flush() ?? null;

    if (this.processorNode) {
      this.processorNode.disconnect();
      this.processorNode.onaudioprocess = null;
    }
    if (this.sourceNode) {
      this.sourceNode.disconnect();
    }
    if (this.stream) {
      for (const track of this.stream.getTracks()) {
        track.stop();
      }
    }
    if (this.audioContext && this.audioContext.state !== "closed") {
      await this.audioContext.close();
    }

    this.audioContext = null;
    this.sourceNode = null;
    this.processorNode = null;
    this.stream = null;
    this.chunker = null;
    this.onChunk = null;
    this.paused = false;

    return finalChunk;
  }
}

function buildAudioConstraints(deviceId: string): MediaTrackConstraints {
  const constraints: MediaTrackConstraints = {
    echoCancellation: false,
    noiseSuppression: false,
    autoGainControl: false
  };

  if (deviceId && deviceId !== "default") {
    constraints.deviceId = { exact: deviceId };
  }

  return constraints;
}
