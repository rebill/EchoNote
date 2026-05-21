import { Notice, Plugin, TFile } from "obsidian";
import { AudioRecorder } from "./audio/audio-recorder";
import { resolveCompanionDiscovery } from "./asr/companion-discovery";
import { MeetingNoteWriter } from "./meeting/meeting-note-writer";
import { MeetingSessionController } from "./meeting/meeting-session";
import { SummaryService } from "./llm/summary-service";
import { EchoNoteSettingTab } from "./settings/settings-tab";
import { DEFAULT_SETTINGS, type EchoNoteSettings } from "./settings/settings";
import { createCompanionResolutionStatus, createInitialRuntimeStatus } from "./status/companion-status";
import { StatusStore } from "./status/status-store";
import { ECHONOTE_STATUS_VIEW_TYPE, EchoNoteStatusView } from "./status/status-view";
import { createEchoNoteError } from "./utils/errors";

export default class EchoNotePlugin extends Plugin {
  settings: EchoNoteSettings = DEFAULT_SETTINGS;
  statusStore = new StatusStore();
  readonly audioRecorder = new AudioRecorder();
  private noteWriter: MeetingNoteWriter | null = null;
  private meetingSession: MeetingSessionController | null = null;
  private summaryService = new SummaryService();

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
      noteWriter: this.noteWriter
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
      new Notice("EchoNote Companion is available.");
      return;
    }

    new Notice(`EchoNote Companion status: ${resolution.kind}`);
  }

  private async summarizeMeeting(): Promise<void> {
    if (!this.noteWriter) {
      return;
    }

    const file = this.resolveSummaryTargetFile();
    if (!file) {
      new Notice("EchoNote could not find a meeting note to summarize.");
      return;
    }

    try {
      new Notice("EchoNote: generating meeting summary...");
      const transcript = await this.noteWriter.readTranscript(file);
      const summary = await this.summaryService.summarize(transcript, this.settings);
      await this.noteWriter.writeSummary(file, summary);
      new Notice("EchoNote summary written.");
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

  private resolveSummaryTargetFile(): TFile | null {
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
}

export type { EchoNoteSettings };
