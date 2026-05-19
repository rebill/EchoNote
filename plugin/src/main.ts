import { Notice, Plugin, TFile } from "obsidian";
import { AudioRecorder } from "./audio/audio-recorder";
import { AsrProcessManager } from "./asr/asr-process-manager";
import { MeetingNoteWriter } from "./meeting/meeting-note-writer";
import { MeetingSessionController } from "./meeting/meeting-session";
import { SummaryService } from "./llm/summary-service";
import { EchoNoteSettingTab } from "./settings/settings-tab";
import { DEFAULT_SETTINGS, resolveAsrModelId, type EchoNoteSettings } from "./settings/settings";
import { StatusStore } from "./status/status-store";
import { ECHONOTE_STATUS_VIEW_TYPE, EchoNoteStatusView } from "./status/status-view";
import { createEchoNoteError } from "./utils/errors";

export default class EchoNotePlugin extends Plugin {
  settings: EchoNoteSettings = DEFAULT_SETTINGS;
  statusStore = new StatusStore();
  readonly audioRecorder = new AudioRecorder();
  private noteWriter: MeetingNoteWriter | null = null;
  private asrProcessManager: AsrProcessManager | null = null;
  private meetingSession: MeetingSessionController | null = null;
  private summaryService = new SummaryService();

  async onload(): Promise<void> {
    await this.loadSettings();

    this.statusStore = new StatusStore({
      selectedModel: resolveAsrModelId(this.settings),
      selectedAudioInput: this.settings.audioInputDeviceLabel
    });
    this.noteWriter = new MeetingNoteWriter(this.app);
    this.asrProcessManager = new AsrProcessManager(
      this,
      () => {
        this.statusStore.setState({ asrService: "running" });
      },
      (code, signal) => {
        this.statusStore.setState({
          asrService: "not_started",
          model: "not_loaded"
        });
        if (code !== 0 && signal !== "SIGTERM") {
          new Notice(`EchoNote ASR service exited: ${code ?? signal ?? "unknown"}`);
        }
      }
    );
    this.meetingSession = new MeetingSessionController({
      getSettings: () => this.settings,
      statusStore: this.statusStore,
      audioRecorder: this.audioRecorder,
      noteWriter: this.noteWriter,
      asrProcessManager: this.asrProcessManager
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
    await this.asrProcessManager?.stop();
    this.app.workspace.detachLeavesOfType(ECHONOTE_STATUS_VIEW_TYPE);
  }

  async loadSettings(): Promise<void> {
    this.settings = {
      ...DEFAULT_SETTINGS,
      ...(await this.loadData())
    };
  }

  async saveSettings(): Promise<void> {
    await this.saveData(this.settings);
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

  startAsrService(): void {
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

  restartAsrService(): void {
    void this.meetingSession?.restartAsrService();
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
      id: "echonote-restart-asr-service",
      name: "Restart ASR Service",
      callback: () => this.restartAsrService()
    });
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
