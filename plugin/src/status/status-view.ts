import { ItemView, WorkspaceLeaf } from "obsidian";
import type EchoNotePlugin from "../main";
import type { EchoNoteStatus } from "./status-types";

export const ECHONOTE_STATUS_VIEW_TYPE = "echonote-status-view";

type StatusRow = {
  label: string;
  value: string;
};

export class EchoNoteStatusView extends ItemView {
  private unsubscribe: (() => void) | null = null;

  constructor(
    leaf: WorkspaceLeaf,
    private readonly plugin: EchoNotePlugin
  ) {
    super(leaf);
  }

  getViewType(): string {
    return ECHONOTE_STATUS_VIEW_TYPE;
  }

  getDisplayText(): string {
    return "EchoNote Status";
  }

  getIcon(): string {
    return "mic";
  }

  async onOpen(): Promise<void> {
    this.unsubscribe = this.plugin.statusStore.subscribe((status) => {
      this.render(status);
    });
  }

  async onClose(): Promise<void> {
    this.unsubscribe?.();
    this.unsubscribe = null;
  }

  private render(status: EchoNoteStatus): void {
    const container = this.contentEl;
    container.empty();
    container.addClass("echonote-status-view");

    container.createEl("h2", { text: "EchoNote Status" });

    const rows: StatusRow[] = [
      { label: "Microphone Permission", value: status.microphonePermission },
      { label: "ASR Service", value: status.asrService },
      { label: "Model", value: status.model },
      { label: "Selected Model", value: status.selectedModel },
      { label: "Audio Input", value: status.selectedAudioInput },
      { label: "Recording", value: status.recording },
      { label: "Current Meeting", value: status.currentMeetingTitle ?? "None" },
      { label: "Chunk Queue", value: `${status.pendingChunkCount} pending` },
      { label: "Last Transcript", value: this.formatLastTranscript(status.lastTranscriptAt) }
    ];

    const list = container.createDiv({ cls: "echonote-status-list" });
    for (const row of rows) {
      const item = list.createDiv({ cls: "echonote-status-row" });
      item.createSpan({ cls: "echonote-status-label", text: row.label });
      item.createSpan({ cls: "echonote-status-value", text: row.value });
    }

    if (status.lastError) {
      const error = container.createDiv({ cls: "echonote-status-error" });
      error.createEl("h3", { text: status.lastError.code });
      error.createEl("p", { text: status.lastError.message });
      if (status.lastError.detail) {
        error.createEl("pre", { text: status.lastError.detail });
      }
    }

    const actions = container.createDiv({ cls: "echonote-status-actions" });
    this.addPanelAction(actions, "Request Microphone Permission", () => this.plugin.requestMicrophonePermission());
    this.addPanelAction(actions, "Start ASR Service", () => this.plugin.startAsrService());
    this.addPanelAction(actions, "Restart ASR Service", () => this.plugin.restartAsrService());
    this.addPanelAction(actions, "Start Meeting", () => this.plugin.startMeeting());
    this.addPanelAction(actions, "Pause Recording", () => this.plugin.pauseRecording());
    this.addPanelAction(actions, "Resume Recording", () => this.plugin.resumeRecording());
    this.addPanelAction(actions, "Stop Meeting", () => this.plugin.stopMeeting());
    this.addPanelAction(actions, "Summarize Meeting", () => this.plugin.summarizeCurrentMeeting());
  }

  private addPanelAction(parent: HTMLElement, text: string, onClick: () => void): void {
    const button = parent.createEl("button", { text });
    button.addEventListener("click", onClick);
  }

  private formatLastTranscript(lastTranscriptAt: number | null): string {
    if (!lastTranscriptAt) {
      return "Never";
    }

    const secondsAgo = Math.max(0, Math.round((Date.now() - lastTranscriptAt) / 1000));
    return `${secondsAgo} seconds ago`;
  }
}
