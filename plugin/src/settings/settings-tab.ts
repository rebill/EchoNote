import { App, Notice, PluginSettingTab, Setting } from "obsidian";
import type EchoNotePlugin from "../main";
import {
  DEFAULT_AUTO_STOP_SILENCE_MINUTES,
  DEFAULT_COMPANION_DISCOVERY_MAX_AGE_SECONDS,
  DEFAULT_COMPANION_DISCOVERY_PATH,
  MAX_AUTO_STOP_SILENCE_MINUTES,
  MIN_AUTO_STOP_SILENCE_MINUTES,
  type ChunkLengthSeconds,
  type LlmProviderType,
  normalizeAutoStopSilenceMinutes
} from "./settings";

export class EchoNoteSettingTab extends PluginSettingTab {
  private audioInputDevices: MediaDeviceInfo[] = [];

  constructor(
    app: App,
    private readonly plugin: EchoNotePlugin
  ) {
    super(app, plugin);
  }

  display(): void {
    const { containerEl } = this;
    containerEl.empty();
    containerEl.createEl("h2", { text: "EchoNote Settings" });

    this.renderMeetingSettings(containerEl);
    this.renderAsrSettings(containerEl);
    this.renderAudioSettings(containerEl);
    this.renderLlmSettings(containerEl);
  }

  private renderMeetingSettings(containerEl: HTMLElement): void {
    containerEl.createEl("h3", { text: "Meeting" });

    new Setting(containerEl)
      .setName("Default meeting folder")
      .setDesc("Folder where new meeting notes are created.")
      .addText((text) =>
        text
          .setPlaceholder("Meetings")
          .setValue(this.plugin.settings.meetingFolder)
          .onChange(async (value) => {
            this.plugin.settings.meetingFolder = value.trim() || "Meetings";
            await this.plugin.saveSettings();
          })
      );

    new Setting(containerEl)
      .setName("Default meeting title format")
      .setDesc("Used when EchoNote creates a new meeting note.")
      .addText((text) =>
        text
          .setPlaceholder("YYYY-MM-DD HH-mm Meeting")
          .setValue(this.plugin.settings.meetingTitleFormat)
          .onChange(async (value) => {
            this.plugin.settings.meetingTitleFormat = value.trim() || "YYYY-MM-DD HH-mm Meeting";
            await this.plugin.saveSettings();
          })
      );
  }

  private renderAsrSettings(containerEl: HTMLElement): void {
    containerEl.createEl("h3", { text: "ASR" });

    new Setting(containerEl)
      .setName("ASR backend")
      .setDesc("EchoNote now uses the EchoNote desktop app only. Start, stop, restart, and configure the ASR service in the desktop app.");

    new Setting(containerEl)
      .setName("Companion discovery path")
      .setDesc("Used to find the Companion-managed ASR endpoint.")
      .addText((text) =>
        text
          .setPlaceholder(DEFAULT_COMPANION_DISCOVERY_PATH)
          .setValue(this.plugin.settings.companionDiscoveryPath)
          .onChange(async (value) => {
            this.plugin.settings.companionDiscoveryPath = value.trim() || DEFAULT_COMPANION_DISCOVERY_PATH;
            await this.plugin.saveSettings();
            this.plugin.syncRuntimeSettingsToStatus();
          })
      );

    new Setting(containerEl)
      .setName("Companion discovery max age")
      .setDesc("Seconds before a Companion discovery file is treated as stale.")
      .addText((text) => {
        text.inputEl.type = "number";
        text.inputEl.min = "1";
        text.inputEl.step = "1";
        text
          .setPlaceholder(String(DEFAULT_COMPANION_DISCOVERY_MAX_AGE_SECONDS))
          .setValue(String(this.plugin.settings.companionDiscoveryMaxAgeSeconds))
          .onChange(async (value) => {
            const maxAgeSeconds = Number.parseInt(value, 10);
            if (Number.isInteger(maxAgeSeconds) && maxAgeSeconds > 0) {
              this.plugin.settings.companionDiscoveryMaxAgeSeconds = maxAgeSeconds;
              await this.plugin.saveSettings();
              this.plugin.syncRuntimeSettingsToStatus();
            }
          });
      });

    new Setting(containerEl)
      .setName("Audio chunk length")
      .setDesc("Length of each quasi-real-time transcription chunk.")
      .addDropdown((dropdown) =>
        dropdown
          .addOption("10", "10 seconds")
          .addOption("15", "15 seconds")
          .addOption("30", "30 seconds")
          .setValue(String(this.plugin.settings.chunkLengthSeconds))
          .onChange(async (value) => {
            this.plugin.settings.chunkLengthSeconds = Number.parseInt(value, 10) as ChunkLengthSeconds;
            await this.plugin.saveSettings();
          })
      );

    new Setting(containerEl)
      .setName("Transcript corrections")
      .setDesc("One correction per line, using wrong text => correct text.")
      .addTextArea((text) => {
        text.inputEl.rows = 6;
        text
          .setPlaceholder("木溪 => 沐曦\nOpen AI => OpenAI")
          .setValue(this.plugin.settings.transcriptCorrectionRules)
          .onChange(async (value) => {
            this.plugin.settings.transcriptCorrectionRules = value;
            await this.plugin.saveSettings();
          });
      });

    new Setting(containerEl)
      .setName("Enable LLM transcript correction")
      .setDesc("Experimental and disabled by default. Sends final transcripts to the current LLM provider for conservative typo checks; terminology rules are more reliable.")
      .addToggle((toggle) =>
        toggle.setValue(this.plugin.settings.enableLlmTranscriptCorrection).onChange(async (value) => {
          this.plugin.settings.enableLlmTranscriptCorrection = value;
          await this.plugin.saveSettings();
        })
      );
  }

  private renderAudioSettings(containerEl: HTMLElement): void {
    containerEl.createEl("h3", { text: "Audio" });

    const audioDeviceContainer = containerEl.createDiv({ cls: "echonote-audio-device-settings" });
    audioDeviceContainer.createEl("p", {
      text: "Choose a microphone or virtual mix device such as BlackHole or Loopback."
    });
    const refreshButton = audioDeviceContainer.createEl("button", { text: "Refresh audio input devices" });
    refreshButton.addEventListener("click", async () => {
      await this.refreshAudioInputDevices();
      this.display();
    });

    new Setting(containerEl)
      .setName("Audio input device")
      .setDesc("Select a microphone or virtual mix device such as BlackHole or Loopback.")
      .addDropdown((dropdown) => {
        dropdown.addOption("default", "Default audio input");
        for (const device of this.audioInputDevices) {
          dropdown.addOption(device.deviceId, device.label || `Audio input ${dropdown.selectEl.length}`);
        }
        dropdown.setValue(this.plugin.settings.audioInputDeviceId || "default");
        dropdown.onChange(async (value) => {
          const selectedDevice = this.audioInputDevices.find((device) => device.deviceId === value);
          this.plugin.settings.audioInputDeviceId = value;
          this.plugin.settings.audioInputDeviceLabel =
            value === "default" ? "Default audio input" : selectedDevice?.label || value;
          this.plugin.statusStore.setState({ selectedAudioInput: this.plugin.settings.audioInputDeviceLabel });
          await this.plugin.saveSettings();
        });
      });

    new Setting(containerEl)
      .setName("Selected audio input")
      .setDesc(this.plugin.settings.audioInputDeviceLabel || "Default audio input");

    new Setting(containerEl)
      .setName("Auto-stop on silence")
      .setDesc("Automatically stop the meeting when EchoNote records a long silent period.")
      .addToggle((toggle) =>
        toggle.setValue(this.plugin.settings.autoStopOnSilence).onChange(async (value) => {
          this.plugin.settings.autoStopOnSilence = value;
          await this.plugin.saveSettings();
        })
      );

    new Setting(containerEl)
      .setName("Auto-stop silence duration")
      .setDesc("Minutes of continuous silence before EchoNote stops recording.")
      .addText((text) => {
        text.inputEl.type = "number";
        text.inputEl.min = String(MIN_AUTO_STOP_SILENCE_MINUTES);
        text.inputEl.max = String(MAX_AUTO_STOP_SILENCE_MINUTES);
        text.inputEl.step = "1";
        text
          .setPlaceholder(String(DEFAULT_AUTO_STOP_SILENCE_MINUTES))
          .setValue(String(this.plugin.settings.autoStopSilenceMinutes))
          .onChange(async (value) => {
            this.plugin.settings.autoStopSilenceMinutes = normalizeAutoStopSilenceMinutes(value);
            await this.plugin.saveSettings();
          });
      });

    new Setting(containerEl)
      .setName("Save raw audio")
      .setDesc("Disabled by default. When enabled, meeting audio is saved in the vault.")
      .addToggle((toggle) =>
        toggle.setValue(this.plugin.settings.saveRawAudio).onChange(async (value) => {
          this.plugin.settings.saveRawAudio = value;
          await this.plugin.saveSettings();
        })
      );

    new Setting(containerEl)
      .setName("Audio save folder")
      .setDesc("Used when raw audio saving is enabled.")
      .addText((text) =>
        text
          .setPlaceholder("Meetings/audio")
          .setValue(this.plugin.settings.audioSaveFolder)
          .onChange(async (value) => {
            this.plugin.settings.audioSaveFolder = value.trim() || "Meetings/audio";
            await this.plugin.saveSettings();
          })
      );
  }

  private async refreshAudioInputDevices(): Promise<void> {
    try {
      await this.plugin.audioRecorder.requestPermission();
      this.audioInputDevices = await this.plugin.audioRecorder.listInputDevices();
      if (
        this.plugin.settings.audioInputDeviceId !== "default" &&
        !this.audioInputDevices.some((device) => device.deviceId === this.plugin.settings.audioInputDeviceId)
      ) {
        this.plugin.settings.audioInputDeviceId = "default";
        this.plugin.settings.audioInputDeviceLabel = "Default audio input";
        this.plugin.statusStore.setState({ selectedAudioInput: this.plugin.settings.audioInputDeviceLabel });
        await this.plugin.saveSettings();
      }
      new Notice(`EchoNote found ${this.audioInputDevices.length} audio input device(s).`);
    } catch (error) {
      new Notice(`EchoNote failed to refresh audio devices: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  private renderLlmSettings(containerEl: HTMLElement): void {
    containerEl.createEl("h3", { text: "LLM" });

    new Setting(containerEl)
      .setName("LLM provider")
      .setDesc("Provider used for meeting summaries and optional transcript correction.")
      .addDropdown((dropdown) =>
        dropdown
          .addOption("openai-compatible", "OpenAI-compatible")
          .addOption("anthropic", "Anthropic")
          .setValue(this.plugin.settings.llmProvider)
          .onChange(async (value) => {
            this.plugin.settings.llmProvider = value as LlmProviderType;
            await this.plugin.saveSettings();
          })
      );

    new Setting(containerEl)
      .setName("OpenAI-compatible base URL")
      .setDesc("Used by OpenAI-compatible LLM features.")
      .addText((text) =>
        text
          .setPlaceholder("https://api.openai.com/v1")
          .setValue(this.plugin.settings.openaiBaseUrl)
          .onChange(async (value) => {
            this.plugin.settings.openaiBaseUrl = value.trim() || "https://api.openai.com/v1";
            await this.plugin.saveSettings();
          })
      );

    new Setting(containerEl)
      .setName("OpenAI-compatible API key")
      .addText((text) =>
        text
          .setPlaceholder("sk-...")
          .setValue(this.plugin.settings.openaiApiKey)
          .onChange(async (value) => {
            this.plugin.settings.openaiApiKey = value.trim();
            await this.plugin.saveSettings();
          })
      );

    new Setting(containerEl)
      .setName("OpenAI-compatible model")
      .addText((text) =>
        text.setValue(this.plugin.settings.openaiModel).onChange(async (value) => {
          this.plugin.settings.openaiModel = value.trim();
          await this.plugin.saveSettings();
        })
      );

    new Setting(containerEl)
      .setName("Anthropic API key")
      .addText((text) =>
        text
          .setPlaceholder("sk-ant-...")
          .setValue(this.plugin.settings.anthropicApiKey)
          .onChange(async (value) => {
            this.plugin.settings.anthropicApiKey = value.trim();
            await this.plugin.saveSettings();
          })
      );

    new Setting(containerEl)
      .setName("Anthropic model")
      .addText((text) =>
        text.setValue(this.plugin.settings.anthropicModel).onChange(async (value) => {
          this.plugin.settings.anthropicModel = value.trim();
          await this.plugin.saveSettings();
        })
      );

    new Setting(containerEl)
      .setName("Summary language")
      .addDropdown((dropdown) =>
        dropdown
          .addOption("zh", "Chinese")
          .addOption("en", "English")
          .addOption("auto", "Auto")
          .setValue(this.plugin.settings.summaryLanguage)
          .onChange(async (value) => {
            this.plugin.settings.summaryLanguage = value as "zh" | "en" | "auto";
            await this.plugin.saveSettings();
          })
      );

    new Setting(containerEl)
      .setName("Summary prompt")
      .setDesc("Custom instruction used when generating meeting summaries.")
      .addTextArea((text) =>
        text.setValue(this.plugin.settings.summaryPrompt).onChange(async (value) => {
          this.plugin.settings.summaryPrompt = value;
          await this.plugin.saveSettings();
        })
      );
  }
}
