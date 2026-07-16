import { mkdtemp, open, rm, type FileHandle } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { concatWavFiles, createPcm16WavBuffer } from "./wav-encoder";

const DEFAULT_MEMORY_THRESHOLD_BYTES = 32 * 1024 * 1024;

export class MeetingAudioSpool {
  private readonly memoryThresholdBytes: number;
  private memoryChunks: ArrayBuffer[] = [];
  private totalPcmBytes = 0;
  private tempFolder: string | null = null;
  private tempPath: string | null = null;
  private fileHandle: FileHandle | null = null;
  private filePosition = 0;
  private writeChain: Promise<void> = Promise.resolve();
  private writeError: unknown = null;
  private disposed = false;

  constructor(memoryThresholdBytes: number = DEFAULT_MEMORY_THRESHOLD_BYTES) {
    if (!Number.isInteger(memoryThresholdBytes) || memoryThresholdBytes < 0) {
      throw new Error("memoryThresholdBytes must be a non-negative integer.");
    }
    this.memoryThresholdBytes = memoryThresholdBytes;
  }

  get pcmByteLength(): number {
    return this.totalPcmBytes;
  }

  get storageMode(): "memory" | "disk" {
    return this.fileHandle ? "disk" : "memory";
  }

  append(wavBytes: ArrayBuffer): Promise<void> {
    if (this.disposed) {
      return Promise.reject(new Error("Meeting audio spool is disposed."));
    }
    const operation = this.writeChain.then(() => {
      if (this.writeError) {
        throw this.writeError;
      }
      return this.appendInternal(wavBytes);
    });
    this.writeChain = operation.catch((error) => {
      this.writeError ??= error;
    });
    return operation;
  }

  async toWav(sampleRate: number = 16000): Promise<ArrayBuffer | null> {
    await this.writeChain;
    if (this.writeError) {
      throw this.writeError;
    }
    if (this.totalPcmBytes === 0) {
      return null;
    }
    if (!this.fileHandle) {
      return concatWavFiles(this.memoryChunks, sampleRate);
    }

    await this.fileHandle.sync();
    const wav = createPcm16WavBuffer(this.totalPcmBytes, sampleRate);
    const destination = Buffer.from(wav, 44, this.totalPcmBytes);
    let offset = 0;
    while (offset < destination.byteLength) {
      const { bytesRead } = await this.fileHandle.read(
        destination,
        offset,
        destination.byteLength - offset,
        offset
      );
      if (bytesRead === 0) {
        throw new Error("Meeting audio spool ended before all PCM bytes were read.");
      }
      offset += bytesRead;
    }
    return wav;
  }

  async dispose(): Promise<void> {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    await this.writeChain;
    await this.fileHandle?.close().catch(() => undefined);
    this.fileHandle = null;
    this.memoryChunks = [];
    if (this.tempFolder) {
      await rm(this.tempFolder, { recursive: true, force: true });
    }
    this.tempFolder = null;
    this.tempPath = null;
    this.totalPcmBytes = 0;
    this.filePosition = 0;
    this.writeError = null;
  }

  private async appendInternal(wavBytes: ArrayBuffer): Promise<void> {
    if (wavBytes.byteLength <= 44) {
      return;
    }
    const payloadLength = wavBytes.byteLength - 44;
    if (!this.fileHandle && this.totalPcmBytes + payloadLength <= this.memoryThresholdBytes) {
      this.memoryChunks.push(wavBytes);
      this.totalPcmBytes += payloadLength;
      return;
    }

    await this.ensureDiskSpool();
    await this.writePayload(new Uint8Array(wavBytes, 44));
    this.totalPcmBytes += payloadLength;
  }

  private async ensureDiskSpool(): Promise<void> {
    if (this.fileHandle) {
      return;
    }
    this.tempFolder = await mkdtemp(join(tmpdir(), "echonote-audio-"));
    this.tempPath = join(this.tempFolder, "meeting.pcm");
    this.fileHandle = await open(this.tempPath, "w+", 0o600);
    for (const wavBytes of this.memoryChunks) {
      await this.writePayload(new Uint8Array(wavBytes, 44));
    }
    this.memoryChunks = [];
  }

  private async writePayload(payload: Uint8Array): Promise<void> {
    if (!this.fileHandle) {
      throw new Error("Meeting audio spool file is not open.");
    }
    const source = Buffer.from(payload.buffer, payload.byteOffset, payload.byteLength);
    let offset = 0;
    while (offset < source.byteLength) {
      const { bytesWritten } = await this.fileHandle.write(
        source,
        offset,
        source.byteLength - offset,
        this.filePosition
      );
      if (bytesWritten === 0) {
        throw new Error("Meeting audio spool could not write PCM bytes.");
      }
      offset += bytesWritten;
      this.filePosition += bytesWritten;
    }
  }
}
