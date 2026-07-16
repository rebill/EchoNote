import { Notice, Plugin, TFile } from "obsidian";
import { AudioRecorder } from "./audio/audio-recorder";
import { AsrRuntimeResolutionError, resolveAsrRuntime } from "./asr/asr-runtime-resolver";
import { AsrServiceClient } from "./asr/asr-service-client";
import { resolveCompanionDiscovery } from "./asr/companion-discovery";
import type { FinalizeTranscriptResponse, TranscriptTurn } from "./asr/asr-types";
import { MeetingNoteWriter } from "./meeting/meeting-note-writer";
import { MeetingSessionController } from "./meeting/meeting-session";
import { SummaryService } from "./llm/summary-service";
import { TranscriptCorrectionService } from "./llm/transcript-correction-service";
import { EchoNoteSettingTab } from "./settings/settings-tab";
import { DEFAULT_SETTINGS, normalizeAutoStopSilenceMinutes, type EchoNoteSettings } from "./settings/settings";
import {
  createAsrRuntimeStatus,
  createCompanionResolutionStatus,
  createInitialRuntimeStatus
} from "./status/companion-status";
import { StatusStore } from "./status/status-store";
import type { EchoNoteStatus } from "./status/status-types";
import { ECHONOTE_STATUS_VIEW_TYPE, EchoNoteStatusView } from "./status/status-view";
import { createEchoNoteError } from "./utils/errors";
import {
  getMeetingArtifactBaseName,
  getMeetingArtifactPaths,
  getSegmentsPathForAudioPath,
  normalizeVaultPath,
  sanitizeMeetingId
} from "./meeting/meeting-artifacts";

type SavedMeetingArtifacts = {
  audioFile: TFile | null;
  segmentsFile: TFile | null;
  audioPath: string;
  segmentsPath: string;
};

export default class EchoNotePlugin extends Plugin {
  settings: EchoNoteSettings = DEFAULT_SETTINGS;
  statusStore = new StatusStore();
  readonly audioRecorder = new AudioRecorder();
  private noteWriter: MeetingNoteWriter | null = null;
  private meetingSession: MeetingSessionController | null = null;
  private summaryService = new SummaryService();
  private transcriptCorrectionService = new TranscriptCorrectionService();

  async onload(): Promise<void> {
    await this.loadSettings();

    this.statusStore = new StatusStore({
      selectedAudioInput: this.settings.audioInputDeviceLabel,
      ...createInitialRuntimeStatus(this.settings)
    });
    this.noteWriter = new MeetingNoteWriter(this.app);
    this.meetingSession = new MeetingSessionController({
      getSettings: () => this.settings,
      statusStore: this.statusStore,
      audioRecorder: this.audioRecorder,
      noteWriter: this.noteWriter,
      correctFinalTranscript: (file, turns, enableTimestamps) =>
        this.correctTranscriptTurns(file, turns, enableTimestamps, "automatic")
    });

    this.registerView(ECHONOTE_STATUS_VIEW_TYPE, (leaf) => new EchoNoteStatusView(leaf, this));

    this.addRibbonIcon("mic", "EchoNote", () => {
      void this.openStatusView();
    });

    this.registerCommands();
    this.addSettingTab(new EchoNoteSettingTab(this.app, this));
  }

  async onunload(): Promise<void> {
    await this.meetingSession?.stop();
    this.app.workspace.detachLeavesOfType(ECHONOTE_STATUS_VIEW_TYPE);
  }

  async loadSettings(): Promise<void> {
    const loadedSettings = ((await this.loadData()) ?? {}) as Record<string, unknown>;
    const settings = {
      ...DEFAULT_SETTINGS,
      ...loadedSettings,
      asrRuntimeMode: "companion"
    } as EchoNoteSettings & Record<string, unknown>;
    const legacyKeys = [
      "pythonPath",
      "asrServicePath",
      "asrServicePort",
      "autoStartAsrService",
      "asrModelPreset",
      "customAsrModelId"
    ];
    const shouldPersistMigration =
      (loadedSettings.asrRuntimeMode !== undefined && loadedSettings.asrRuntimeMode !== "companion")
      || legacyKeys.some((key) => key in settings);

    for (const key of legacyKeys) {
      delete settings[key];
    }

    if (typeof settings.transcriptCorrectionRules !== "string") {
      settings.transcriptCorrectionRules = DEFAULT_SETTINGS.transcriptCorrectionRules;
    }
    if (typeof settings.enableLlmTranscriptCorrection !== "boolean") {
      settings.enableLlmTranscriptCorrection = DEFAULT_SETTINGS.enableLlmTranscriptCorrection;
    }
    if (typeof settings.autoStopOnSilence !== "boolean") {
      settings.autoStopOnSilence = DEFAULT_SETTINGS.autoStopOnSilence;
    }
    settings.autoStopSilenceMinutes = normalizeAutoStopSilenceMinutes(settings.autoStopSilenceMinutes);

    this.settings = settings;

    if (shouldPersistMigration) {
      await this.saveSettings();
    }
  }

  async saveSettings(): Promise<void> {
    await this.saveData(this.settings);
  }

  syncRuntimeSettingsToStatus(): void {
    this.statusStore.setState(createInitialRuntimeStatus(this.settings));
  }

  async openStatusView(): Promise<void> {
    const existingLeaf = this.app.workspace.getLeavesOfType(ECHONOTE_STATUS_VIEW_TYPE)[0];
    if (existingLeaf) {
      this.app.workspace.revealLeaf(existingLeaf);
      return;
    }

    const leaf = this.app.workspace.getRightLeaf(false) ?? this.app.workspace.getLeaf(true);
    await leaf.setViewState({
      type: ECHONOTE_STATUS_VIEW_TYPE,
      active: true
    });
    this.app.workspace.revealLeaf(leaf);
  }

  requestMicrophonePermission(): void {
    void this.meetingSession?.requestMicrophonePermission();
  }

  prepareCompanionAsr(): void {
    void this.meetingSession?.startAsrServiceOnly();
  }

  startMeeting(): void {
    void this.meetingSession?.start();
  }

  pauseRecording(): void {
    this.meetingSession?.pause();
  }

  resumeRecording(): void {
    this.meetingSession?.resume();
  }

  stopMeeting(): void {
    void this.meetingSession?.stop();
  }

  summarizeCurrentMeeting(): void {
    void this.summarizeMeeting();
  }

  refinalizeCurrentMeeting(): void {
    void this.openStatusView();
    void this.refinalizeMeetingWithSpeakers();
  }

  correctCurrentTranscript(): void {
    void this.openStatusView();
    void this.correctCurrentTranscriptWithLlm();
  }

  refreshCompanionStatus(): void {
    void this.refreshCompanionStatusInternal();
  }

  private registerCommands(): void {
    this.addCommand({
      id: "echonote-start-meeting",
      name: "Start Meeting",
      callback: () => this.startMeeting()
    });

    this.addCommand({
      id: "echonote-pause-recording",
      name: "Pause Recording",
      callback: () => this.pauseRecording()
    });

    this.addCommand({
      id: "echonote-resume-recording",
      name: "Resume Recording",
      callback: () => this.resumeRecording()
    });

    this.addCommand({
      id: "echonote-stop-meeting",
      name: "Stop Meeting",
      callback: () => this.stopMeeting()
    });

    this.addCommand({
      id: "echonote-summarize-current-meeting",
      name: "Summarize Current Meeting",
      callback: () => this.summarizeCurrentMeeting()
    });

    this.addCommand({
      id: "echonote-refinalize-speaker-transcript",
      name: "Re-finalize Transcript with Speakers",
      callback: () => this.refinalizeCurrentMeeting()
    });

    this.addCommand({
      id: "echonote-correct-transcript-with-llm",
      name: "Correct Transcript with LLM",
      callback: () => this.correctCurrentTranscript()
    });

    this.addCommand({
      id: "echonote-open-status-panel",
      name: "Open Status Panel",
      callback: () => {
        void this.openStatusView();
      }
    });

    this.addCommand({
      id: "echonote-prepare-companion-asr",
      name: "Prepare Companion ASR",
      callback: () => this.prepareCompanionAsr()
    });

    this.addCommand({
      id: "echonote-refresh-companion-status",
      name: "Refresh Companion Status",
      callback: () => this.refreshCompanionStatus()
    });
  }

  private async refreshCompanionStatusInternal(): Promise<void> {
    const resolution = await resolveCompanionDiscovery(this.settings);
    this.statusStore.setState(createCompanionResolutionStatus(this.settings, resolution));

    if (resolution.kind === "available") {
      new Notice("EchoNote desktop runtime is available.");
      return;
    }

    new Notice(`EchoNote desktop runtime status: ${resolution.kind}`);
  }

  private async summarizeMeeting(): Promise<void> {
    if (!this.noteWriter) {
      return;
    }

    const file = this.resolveMeetingTargetFile();
    if (!file) {
      new Notice("EchoNote could not find a meeting note to summarize.");
      return;
    }

    try {
      new Notice("EchoNote: generating meeting summary...");
      const transcript = await this.noteWriter.readTranscript(file);
      const summary = await this.summaryService.summarize(transcript, this.settings);
      const summarizedFile = await this.noteWriter.writeSummary(file, summary, this.settings);
      this.meetingSession?.updateCurrentMeetingFile(file, summarizedFile);
      this.statusStore.setState({
        currentMeetingPath: summarizedFile.path,
        currentMeetingTitle: summarizedFile.basename,
        lastError: null
      });
      new Notice(`EchoNote summary written: ${summarizedFile.basename}`);
    } catch (error) {
      this.statusStore.setState({
        lastError: createEchoNoteError("LLM_REQUEST_FAILED", "Failed to summarize meeting.", {
          detail: error instanceof Error ? error.message : String(error),
          recoverable: true
        })
      });
      new Notice(`EchoNote summary failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  private async refinalizeMeetingWithSpeakers(): Promise<void> {
    if (!this.noteWriter) {
      return;
    }

    if (this.statusStore.getState().speakerFinalization === "running") {
      new Notice("EchoNote speaker finalization is already running.");
      return;
    }

    this.setSpeakerFinalizationStatus("running", "Checking the active meeting note...");
    const file = this.resolveMeetingTargetFile();
    if (!file) {
      this.setSpeakerFinalizationStatus("failed", "No active Markdown meeting note was found.");
      new Notice("EchoNote could not find a meeting note to re-finalize.");
      return;
    }

    this.statusStore.setState({
      currentMeetingPath: file.path,
      currentMeetingTitle: file.basename
    });
    this.setSpeakerFinalizationStatus("running", `Looking for saved audio and segments for ${file.basename}...`);
    const artifacts = this.resolveSavedMeetingArtifacts(file);
    if (!artifacts.audioFile) {
      this.setSpeakerFinalizationStatus("failed", `Saved audio was not found: ${artifacts.audioPath}`);
      new Notice(`EchoNote saved audio was not found: ${artifacts.audioPath}`);
      return;
    }
    if (!artifacts.segmentsFile) {
      this.setSpeakerFinalizationStatus("failed", `Saved segments were not found: ${artifacts.segmentsPath}`);
      new Notice(`EchoNote saved segments were not found: ${artifacts.segmentsPath}`);
      return;
    }

    try {
      new Notice("EchoNote: re-finalizing transcript with speaker labels...");
      this.setSpeakerFinalizationStatus("running", "Resolving EchoNote desktop ASR runtime...");
      const runtime = await resolveAsrRuntime(this.settings);
      this.statusStore.setState({
        ...createAsrRuntimeStatus(this.settings, runtime),
        asrService: "starting",
        lastError: null
      });

      const client = new AsrServiceClient(runtime.baseUrl);
      this.setSpeakerFinalizationStatus("running", "Checking ASR service health...");
      await client.health();
      this.statusStore.setState({ asrService: "running" });

      this.setSpeakerFinalizationStatus("running", `Reading saved audio: ${artifacts.audioFile.path}`);
      const wavBytes = await this.noteWriter.readMeetingAudio(artifacts.audioFile.path);
      this.setSpeakerFinalizationStatus("running", `Reading saved transcript segments: ${artifacts.segmentsFile.path}`);
      const segments = await this.noteWriter.readMeetingSegments(artifacts.segmentsFile.path);
      this.setSpeakerFinalizationStatus(
        "running",
        `Running speaker finalization on ${segments.length} segment(s). This can take several minutes.`
      );
      const finalized = await client.finalizeTranscript(
        sanitizeMeetingId(file.basename),
        wavBytes,
        segments,
        this.settings.summaryLanguage === "en" ? "en" : "zh",
        true
      );

      if (finalized.turns.length === 0) {
        this.setSpeakerFinalizationStatus(
          "failed",
          "Speaker re-finalization returned no transcript. Current transcript was kept."
        );
        new Notice("EchoNote speaker re-finalization returned no transcript. Current transcript was kept.");
        return;
      }

      this.setSpeakerFinalizationStatus("running", "Writing finalized speaker transcript to the meeting note...");
      await this.noteWriter.replaceTranscript(
        file,
        finalized.turns,
        this.settings.enableTimestamps,
        this.settings.transcriptCorrectionRules
      );
      this.setSpeakerFinalizationStatus("succeeded", this.createSpeakerFinalizationResultMessage(finalized));
      this.notifySpeakerFinalizationResult(finalized, "EchoNote re-finalized transcript");
      if (this.settings.enableLlmTranscriptCorrection) {
        void this.correctTranscriptTurns(file, finalized.turns, this.settings.enableTimestamps, "automatic");
      }
    } catch (error) {
      this.statusStore.setState({
        ...this.createRefinalizeFailureStatus(error),
        speakerFinalization: "failed",
        speakerFinalizationMessage: `Speaker re-finalization failed: ${error instanceof Error ? error.message : String(error)}`
      });
      new Notice(`EchoNote speaker re-finalization failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  private async correctCurrentTranscriptWithLlm(): Promise<void> {
    if (!this.noteWriter) {
      return;
    }

    const file = this.resolveMeetingTargetFile();
    if (!file) {
      new Notice("EchoNote could not find a meeting note to correct.");
      return;
    }

    try {
      const parsedTranscript = await this.noteWriter.readParsedTranscript(file);
      if (parsedTranscript.turns.length === 0) {
        new Notice("EchoNote transcript is empty.");
        return;
      }
      await this.correctTranscriptTurns(file, parsedTranscript.turns, parsedTranscript.hasTimestamps, "manual");
    } catch (error) {
      this.handleTranscriptCorrectionFailure(error, "Failed to correct transcript with LLM.");
    }
  }

  private async correctTranscriptTurns(
    file: TFile,
    turns: TranscriptTurn[],
    enableTimestamps: boolean,
    mode: "automatic" | "manual"
  ): Promise<void> {
    if (!this.noteWriter) {
      return;
    }

    if (mode === "automatic" && !this.settings.enableLlmTranscriptCorrection) {
      return;
    }

    try {
      this.statusStore.setState({
        transcriptCorrection: "running",
        transcriptCorrectionMessage: "Saving transcript before LLM correction.",
        lastError: null
      });
      const originalTranscript = await this.noteWriter.readTranscript(file);
      const artifactPath = await this.noteWriter.saveTranscriptBeforeLlmArtifact(file, originalTranscript);
      this.statusStore.setState({
        transcriptCorrectionMessage: "Running conservative LLM transcript correction."
      });
      const result = await this.transcriptCorrectionService.correctTurns(turns, this.settings);
      if (result.turns.length === 0) {
        throw new Error("LLM transcript correction returned no transcript.");
      }
      if (result.acceptedTurns === 0 && result.failedBatches > 0) {
        throw new Error("LLM transcript correction failed for every batch.");
      }

      if (result.changedTurns > 0) {
        await this.noteWriter.replaceTranscript(
          file,
          result.turns,
          enableTimestamps,
          this.settings.transcriptCorrectionRules
        );
      }
      await this.noteWriter.writeTranscriptCorrectionMetadata(file, new Date(), result.changedTurns, result.turns.length);
      const partialMessage =
        result.rejectedTurns > 0
          ? ` ${result.rejectedTurns} unsafe turn(s) were kept from the ASR transcript.`
          : "";
      const correctionMessage =
        result.changedTurns > 0
          ? `LLM transcript correction changed ${result.changedTurns} of ${result.turns.length} turn(s).`
          : `LLM transcript correction checked ${result.turns.length} turn(s); no changes were needed.`;
      this.statusStore.setState({
        transcriptCorrection: "succeeded",
        transcriptCorrectionMessage: `${correctionMessage} Before artifact: ${artifactPath}.${partialMessage}`
      });
      new Notice(correctionMessage);
    } catch (error) {
      this.handleTranscriptCorrectionFailure(
        error,
        mode === "automatic"
          ? "LLM transcript correction failed. Final ASR transcript was kept."
          : "LLM transcript correction failed. Current transcript was kept."
      );
    }
  }

  private resolveMeetingTargetFile(): TFile | null {
    const currentMeetingFile = this.meetingSession?.getCurrentMeetingFile();
    if (currentMeetingFile) {
      return currentMeetingFile;
    }

    const activeFile = this.app.workspace.getActiveFile();
    if (activeFile?.extension === "md") {
      return activeFile;
    }

    return null;
  }

  private resolveSavedMeetingArtifacts(meetingFile: TFile): SavedMeetingArtifacts {
    const expectedPaths = getMeetingArtifactPaths(this.settings, meetingFile.basename);
    const expectedAudioFile = this.getVaultFile(expectedPaths.audioPath);
    const expectedSegmentsFile = this.getVaultFile(expectedPaths.segmentsPath);
    if (expectedAudioFile && expectedSegmentsFile) {
      return {
        audioFile: expectedAudioFile,
        segmentsFile: expectedSegmentsFile,
        audioPath: expectedAudioFile.path,
        segmentsPath: expectedSegmentsFile.path
      };
    }

    const fallbackAudioFiles = this.findSavedMeetingAudioFiles(meetingFile.basename);
    for (const audioFile of fallbackAudioFiles) {
      const segmentsPath = getSegmentsPathForAudioPath(audioFile.path);
      const segmentsFile = this.getVaultFile(segmentsPath);
      if (segmentsFile) {
        return {
          audioFile,
          segmentsFile,
          audioPath: audioFile.path,
          segmentsPath: segmentsFile.path
        };
      }
    }

    const audioFile = expectedAudioFile ?? fallbackAudioFiles[0] ?? null;
    const segmentsPath = audioFile ? getSegmentsPathForAudioPath(audioFile.path) : expectedPaths.segmentsPath;
    return {
      audioFile,
      segmentsFile: audioFile ? this.getVaultFile(segmentsPath) : expectedSegmentsFile,
      audioPath: audioFile?.path ?? expectedPaths.audioPath,
      segmentsPath
    };
  }

  private findSavedMeetingAudioFiles(meetingTitle: string): TFile[] {
    const audioSaveFolder = normalizeVaultPath(this.settings.audioSaveFolder || "Meetings/audio");
    const meetingBaseName = getMeetingArtifactBaseName(meetingTitle);
    const folderPrefix = audioSaveFolder ? `${audioSaveFolder}/` : "";
    return this.app.vault
      .getFiles()
      .filter((file) => file.extension.toLowerCase() === "wav")
      .filter((file) => normalizeVaultPath(file.path).startsWith(folderPrefix))
      .filter((file) => file.basename === meetingBaseName);
  }

  private getVaultFile(path: string): TFile | null {
    const file = this.app.vault.getAbstractFileByPath(normalizeVaultPath(path));
    return file instanceof TFile ? file : null;
  }

  private notifySpeakerFinalizationResult(finalized: FinalizeTranscriptResponse, successPrefix: string): void {
    if (finalized.diarization_status === "available") {
      new Notice(`${successPrefix} with speaker labels.`);
    } else if (finalized.diarization_status === "failed") {
      new Notice(`${successPrefix}, but speaker diarization failed.`);
    } else if (finalized.diarization_status === "unavailable") {
      new Notice(`${successPrefix}, but speaker diarization is unavailable.`);
    } else {
      new Notice(`${successPrefix}.`);
    }
  }

  private createSpeakerFinalizationResultMessage(finalized: FinalizeTranscriptResponse): string {
    if (finalized.diarization_status === "available") {
      return `Speaker transcript written with labels. ${finalized.turns.length} turn(s) generated.`;
    }
    if (finalized.diarization_status === "failed") {
      return `Transcript written, but speaker diarization failed. ${finalized.turns.length} turn(s) generated.`;
    }
    if (finalized.diarization_status === "unavailable") {
      return `Transcript written, but speaker diarization is unavailable. ${finalized.turns.length} turn(s) generated.`;
    }
    return `Transcript written. ${finalized.turns.length} turn(s) generated.`;
  }

  private setSpeakerFinalizationStatus(
    speakerFinalization: EchoNoteStatus["speakerFinalization"],
    speakerFinalizationMessage: string
  ): void {
    this.statusStore.setState({
      speakerFinalization,
      speakerFinalizationMessage,
      lastError: speakerFinalization === "running" || speakerFinalization === "succeeded" ? null : this.statusStore.getState().lastError
    });
  }

  private handleTranscriptCorrectionFailure(error: unknown, message: string): void {
    const detail = error instanceof Error ? error.message : String(error);
    this.statusStore.setState({
      transcriptCorrection: "failed",
      transcriptCorrectionMessage: `${message} ${detail}`,
      lastError: createEchoNoteError(isLlmConfigError(error) ? "LLM_CONFIG_MISSING" : "LLM_REQUEST_FAILED", message, {
        detail,
        recoverable: true
      })
    });
    new Notice(`${message} ${detail}`);
  }

  private createRefinalizeFailureStatus(error: unknown): Partial<ReturnType<StatusStore["getState"]>> {
    if (error instanceof AsrRuntimeResolutionError) {
      return {
        ...createCompanionResolutionStatus(this.settings, error.companion),
        lastError: createEchoNoteError(error.code, error.message, {
          detail: error.detail,
          recoverable: true
        })
      };
    }

    return {
      asrService: "error",
      lastError: createEchoNoteError("ASR_FINALIZE_FAILED", "Failed to re-finalize speaker transcript.", {
        detail: error instanceof Error ? error.message : String(error),
        recoverable: true
      })
    };
  }
}

function isLlmConfigError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return /api key|base url|model/i.test(message);
}

export type { EchoNoteSettings };
