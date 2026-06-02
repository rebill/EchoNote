import { Notice, Platform, TFile } from "obsidian";
import { AudioRecorder } from "../audio/audio-recorder";
import type { AudioChunk } from "../audio/audio-types";
import { concatWavFiles } from "../audio/wav-encoder";
import { AsrRuntimeResolutionError, resolveAsrRuntime } from "../asr/asr-runtime-resolver";
import { AsrServiceClient } from "../asr/asr-service-client";
import type { EchoNoteSettings } from "../settings/settings";
import { createAsrRuntimeStatus, createCompanionResolutionStatus } from "../status/companion-status";
import type { StatusStore } from "../status/status-store";
import { createEchoNoteError } from "../utils/errors";
import { MeetingNoteWriter } from "./meeting-note-writer";

const MIN_TRANSCRIBE_CHUNK_MS = 1000;
const SILENCE_RMS_THRESHOLD = 0.002;

type MeetingSessionControllerOptions = {
  getSettings: () => EchoNoteSettings;
  statusStore: StatusStore;
  audioRecorder: AudioRecorder;
  noteWriter: MeetingNoteWriter;
};

export class MeetingSessionController {
  private meetingFile: TFile | null = null;
  private meetingTitle: string | null = null;
  private meetingAudioFolder: string | null = null;
  private meetingAudioChunks: ArrayBuffer[] = [];
  private liveSegments: Awaited<ReturnType<AsrServiceClient["transcribe"]>>[] = [];
  private queue: AudioChunk[] = [];
  private processing = false;
  private starting = false;
  private started = false;
  private stopping = false;
  private asrClient: AsrServiceClient | null = null;

  constructor(private readonly options: MeetingSessionControllerOptions) {}

  async requestMicrophonePermission(): Promise<void> {
    try {
      await this.options.audioRecorder.requestPermission();
      this.options.statusStore.setState({ microphonePermission: "granted" });
    } catch (error) {
      this.options.statusStore.setState({
        microphonePermission: "denied",
        lastError: createEchoNoteError("MIC_PERMISSION_DENIED", "Microphone permission was denied.", {
          detail: error instanceof Error ? error.message : String(error),
          recoverable: true
        })
      });
    }
  }

  async start(): Promise<void> {
    if (this.starting || this.started) {
      new Notice("EchoNote is already recording.");
      return;
    }

    if (this.stopping) {
      new Notice("EchoNote is stopping the current meeting.");
      return;
    }

    if (!Platform.isMacOS) {
      this.options.statusStore.setState({
        lastError: createEchoNoteError("UNSUPPORTED_PLATFORM", "EchoNote MVP only supports macOS.", {
          recoverable: false
        })
      });
      return;
    }

    const settings = this.options.getSettings();
    const startedAt = new Date();
    this.starting = true;

    try {
      this.options.statusStore.setState({
        asrService: "starting",
        model: "unknown",
        recording: "starting",
        lastError: null
      });
      new Notice("EchoNote: checking ASR service...");

      const runtime = await resolveAsrRuntime(settings);
      this.options.statusStore.setState(createAsrRuntimeStatus(settings, runtime));
      const client = this.createClient(runtime.baseUrl);
      this.asrClient = client;
      await this.ensureAsrService(client);
      const modelId = await this.ensureModelReady(client, runtime.companion.discovery.modelId);

      new Notice("EchoNote: creating meeting note...");
      const meetingInfo = await this.options.noteWriter.createMeetingNote({
        settings,
        startTime: startedAt,
        asrModel: modelId
      });
      this.meetingFile = meetingInfo.file;
      this.meetingTitle = meetingInfo.title;
      this.meetingAudioFolder = meetingInfo.audioFolder;
      this.meetingAudioChunks = [];
      this.liveSegments = [];

      this.options.statusStore.setState({
        currentMeetingPath: this.meetingFile.path,
        currentMeetingTitle: this.meetingFile.basename,
        pendingChunkCount: 0
      });

      new Notice("EchoNote: requesting microphone...");
      await this.options.audioRecorder.start(settings.chunkLengthSeconds, settings.audioInputDeviceId, (chunk) => {
        void this.enqueueChunk(chunk);
      });

      this.started = true;
      this.options.statusStore.setState({ recording: "recording", microphonePermission: "granted" });
      new Notice("EchoNote meeting started.");
    } catch (error) {
      await this.options.audioRecorder.stop();
      this.started = false;
      this.asrClient = null;
      this.options.statusStore.setState({
        ...this.createRuntimeFailureStatus(error),
        asrService: "error",
        model: "error",
        recording: "error",
        lastError: this.createStartFailureError(error, "Failed to start EchoNote meeting.")
      });
      new Notice(`EchoNote failed to start: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      this.starting = false;
    }
  }

  async startAsrServiceOnly(): Promise<void> {
    const settings = this.options.getSettings();

    try {
      this.options.statusStore.setState({
        asrService: "starting",
        model: "unknown",
        lastError: null
      });
      const runtime = await resolveAsrRuntime(settings);
      this.options.statusStore.setState(createAsrRuntimeStatus(settings, runtime));
      const client = this.createClient(runtime.baseUrl);
      this.asrClient = client;
      await this.ensureAsrService(client);
      await this.ensureModelReady(client, runtime.companion.discovery.modelId);
      new Notice("EchoNote desktop ASR is ready.");
    } catch (error) {
      this.asrClient = null;
      this.options.statusStore.setState({
        ...this.createRuntimeFailureStatus(error),
        asrService: "error",
        model: "error",
        lastError: this.createStartFailureError(error, "Failed to start ASR service.")
      });
    }
  }

  pause(): void {
    if (!this.started) {
      return;
    }
    this.options.audioRecorder.pause();
    this.options.statusStore.setState({ recording: "paused" });
  }

  resume(): void {
    if (!this.started) {
      return;
    }
    this.options.audioRecorder.resume();
    this.options.statusStore.setState({ recording: "recording" });
  }

  async stop(): Promise<void> {
    if (this.stopping) {
      new Notice("EchoNote is already stopping.");
      return;
    }

    if (this.starting) {
      new Notice("EchoNote is still starting.");
      return;
    }

    if (!this.started) {
      this.options.statusStore.setState({ recording: "idle" });
      return;
    }

    this.stopping = true;
    this.options.statusStore.setState({ recording: "stopping" });
    try {
      const finalChunk = await this.options.audioRecorder.stop();
      const stoppedAt = new Date();
      if (finalChunk) {
        await this.enqueueChunk(finalChunk);
      }

      await this.waitForQueueToDrain();
      await this.finalizeTranscriptIfPossible();
      if (this.meetingFile) {
        await this.options.noteWriter.updateMeetingEndTime(this.meetingFile, stoppedAt);
      }
      await this.saveCompleteAudioIfEnabled();
      this.started = false;
      this.meetingFile = null;
      this.meetingTitle = null;
      this.meetingAudioFolder = null;
      this.asrClient = null;
      this.meetingAudioChunks = [];
      this.liveSegments = [];
      this.queue = [];
      this.options.statusStore.setState({
        recording: "idle",
        pendingChunkCount: 0
      });
      new Notice("EchoNote meeting stopped.");
    } catch (error) {
      this.options.statusStore.setState({
        recording: "error",
        lastError: createEchoNoteError("MEETING_STOP_FAILED", "Failed to stop EchoNote meeting.", {
          detail: error instanceof Error ? error.message : String(error),
          recoverable: true
        })
      });
      new Notice(`EchoNote failed to stop: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      this.stopping = false;
    }
  }

  getCurrentMeetingFile(): TFile | null {
    return this.meetingFile;
  }

  private async ensureAsrService(client: AsrServiceClient): Promise<void> {
    try {
      await client.health();
      this.options.statusStore.setState({ asrService: "running" });
      return;
    } catch {
      throw new Error("Companion ASR runtime is unavailable. Open the EchoNote desktop app and click Start Service.");
    }
  }

  private async ensureModelReady(client: AsrServiceClient, fallbackModelId: string): Promise<string> {
    const deadline = Date.now() + 60000;
    let status = await client.modelStatus();
    let modelId = status.model_id || fallbackModelId;

    if (status.status === "not_loaded") {
      this.options.statusStore.setState({ model: "loading", selectedModel: modelId });
      await client.loadModel(modelId);
      status = await client.modelStatus();
      modelId = status.model_id || modelId;
    }

    while (status.status === "loading" && Date.now() < deadline) {
      this.options.statusStore.setState({ model: "loading", selectedModel: modelId });
      await sleep(500);
      status = await client.modelStatus();
      modelId = status.model_id || modelId;
    }

    if (status.status !== "ready") {
      throw new Error(status.error ?? `model is ${status.status}`);
    }
    this.options.statusStore.setState({ model: "ready", selectedModel: modelId });
    return modelId;
  }

  private async enqueueChunk(chunk: AudioChunk): Promise<void> {
    this.meetingAudioChunks.push(chunk.wavBytes);

    if (this.shouldSkipTranscription(chunk)) {
      this.options.statusStore.setState({ pendingChunkCount: this.queue.length });
      return;
    }

    this.queue.push(chunk);
    this.options.statusStore.setState({ pendingChunkCount: this.queue.length });
    void this.processQueue();
  }

  private shouldSkipTranscription(chunk: AudioChunk): boolean {
    return chunk.durationMs < MIN_TRANSCRIBE_CHUNK_MS || chunk.rms < SILENCE_RMS_THRESHOLD;
  }

  private async processQueue(): Promise<void> {
    if (this.processing) {
      return;
    }

    this.processing = true;
    try {
      while (this.queue.length > 0) {
        const chunk = this.queue.shift();
        this.options.statusStore.setState({ pendingChunkCount: this.queue.length });
        if (!chunk || !this.meetingFile) {
          continue;
        }

        await this.processChunk(chunk, this.meetingFile);
      }
    } finally {
      this.processing = false;
    }
  }

  private async processChunk(chunk: AudioChunk, meetingFile: TFile): Promise<void> {
    const client = this.asrClient;
    if (!client) {
      this.options.statusStore.setState({
        lastError: createEchoNoteError("ASR_COMPANION_UNAVAILABLE", "Companion ASR client is not available.", {
          detail: "Start the meeting again after the EchoNote desktop app is running.",
          recoverable: true
        })
      });
      return;
    }

    const settings = this.options.getSettings();
    try {
      const segment = await client.transcribe(chunk, settings.summaryLanguage === "en" ? "en" : "zh");
      this.liveSegments.push(segment);
      await this.options.noteWriter.appendTranscript(meetingFile, segment, settings.enableTimestamps);
      this.options.statusStore.setState({ lastTranscriptAt: Date.now(), lastError: null });
    } catch (error) {
      this.options.statusStore.setState({
        lastError: createEchoNoteError("ASR_TRANSCRIBE_FAILED", `Failed to transcribe ${chunk.id}.`, {
          detail: error instanceof Error ? error.message : String(error),
          recoverable: true
        })
      });
    }
  }

  private async waitForQueueToDrain(): Promise<void> {
    while (this.processing || this.queue.length > 0) {
      await sleep(200);
    }
  }

  private async finalizeTranscriptIfPossible(): Promise<void> {
    const client = this.asrClient;
    const meetingFile = this.meetingFile;
    if (!client || !meetingFile || this.liveSegments.length === 0 || this.meetingAudioChunks.length === 0) {
      return;
    }

    const settings = this.options.getSettings();
    const meetingId = sanitizeMeetingId(this.meetingTitle ?? meetingFile.basename);
    const wavBytes = concatWavFiles(this.meetingAudioChunks);
    try {
      const finalized = await client.finalizeTranscript(
        meetingId,
        wavBytes,
        this.liveSegments,
        settings.summaryLanguage === "en" ? "en" : "zh",
        true
      );
      if (finalized.turns.length === 0) {
        new Notice("EchoNote speaker finalization returned no transcript. Live transcript was kept.");
        return;
      }

      await this.options.noteWriter.replaceTranscript(meetingFile, finalized.turns, settings.enableTimestamps);
      if (finalized.diarization_status === "available") {
        new Notice("EchoNote final transcript generated with speaker labels.");
      } else if (finalized.diarization_status === "failed") {
        new Notice("EchoNote speaker diarization failed. Final transcript was generated without speaker labels.");
      } else if (finalized.diarization_status === "unavailable") {
        new Notice("EchoNote speaker diarization is unavailable. Final transcript was generated without speaker labels.");
      }
    } catch (error) {
      this.options.statusStore.setState({
        lastError: createEchoNoteError("ASR_FINALIZE_FAILED", "Failed to finalize speaker transcript.", {
          detail: error instanceof Error ? error.message : String(error),
          recoverable: true
        })
      });
      new Notice("EchoNote speaker finalization failed. Live transcript was kept.");
    }
  }

  private async saveCompleteAudioIfEnabled(): Promise<void> {
    const settings = this.options.getSettings();
    if (!settings.saveRawAudio || this.meetingAudioChunks.length === 0) {
      return;
    }

    const meetingTitle = this.meetingTitle ?? "meeting";
    const audioFolder = this.meetingAudioFolder ?? `${settings.audioSaveFolder}/${meetingTitle}`;
    const wavBytes = concatWavFiles(this.meetingAudioChunks);
    await this.options.noteWriter.saveMeetingAudio(audioFolder, meetingTitle, wavBytes);
  }

  private createClient(baseUrl: string): AsrServiceClient {
    return new AsrServiceClient(baseUrl);
  }

  private createStartFailureError(error: unknown, message: string) {
    if (error instanceof AsrRuntimeResolutionError) {
      return createEchoNoteError(error.code, error.message, {
        detail: error.detail,
        recoverable: true
      });
    }

    return createEchoNoteError("ASR_SERVICE_START_FAILED", message, {
      detail: error instanceof Error ? error.message : String(error),
      recoverable: true
    });
  }

  private createRuntimeFailureStatus(error: unknown): Partial<ReturnType<StatusStore["getState"]>> {
    if (error instanceof AsrRuntimeResolutionError) {
      return createCompanionResolutionStatus(this.options.getSettings(), error.companion);
    }

    return {};
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function sanitizeMeetingId(value: string): string {
  return value.replace(/[^a-zA-Z0-9_-]+/g, "-").replace(/^-+|-+$/g, "") || "meeting";
}
