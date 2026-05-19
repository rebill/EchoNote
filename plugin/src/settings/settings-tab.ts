import { App, Notice, PluginSettingTab, Setting } from "obsidian";
import type EchoNotePlugin from "../main";
import {
  ASR_MODEL_QWEN3_0_6B_4BIT,
  ASR_MODEL_QWEN3_1_7B_4BIT,
  type AsrModelPreset,
  type ChunkLengthSeconds,
  type LlmProviderType,
  resolveAsrModelId
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
      .setName("ASR model")
      .setDesc("Changing this requires restarting the ASR service.")
      .addDropdown((dropdown) =>
        dropdown
          .addOption(ASR_MODEL_QWEN3_0_6B_4BIT, ASR_MODEL_QWEN3_0_6B_4BIT)
          .addOption(ASR_MODEL_QWEN3_1_7B_4BIT, ASR_MODEL_QWEN3_1_7B_4BIT)
          .addOption("custom", "Custom MLX model ID")
          .setValue(this.plugin.settings.asrModelPreset)
          .onChange(async (value) => {
            this.plugin.settings.asrModelPreset = value as AsrModelPreset;
            this.plugin.statusStore.setState({ selectedModel: resolveAsrModelId(this.plugin.settings) });
            await this.plugin.saveSettings();
          })
      );

    new Setting(containerEl)
      .setName("Custom MLX model ID")
      .setDesc("Used only when ASR model is set to custom.")
      .addText((text) =>
        text
          .setPlaceholder("mlx-community/...")
          .setValue(this.plugin.settings.customAsrModelId)
          .onChange(async (value) => {
            this.plugin.settings.customAsrModelId = value.trim();
            this.plugin.statusStore.setState({ selectedModel: resolveAsrModelId(this.plugin.settings) });
            await this.plugin.saveSettings();
          })
      );

    new Setting(containerEl)
      .setName("Python path")
      .setDesc("Python executable used to start the local ASR service.")
      .addText((text) =>
        text
          .setPlaceholder("python3")
          .setValue(this.plugin.settings.pythonPath)
          .onChange(async (value) => {
            this.plugin.settings.pythonPath = value.trim() || "python3";
            await this.plugin.saveSettings();
          })
      );

    new Setting(containerEl)
      .setName("ASR service path")
      .setDesc("Path to the local Python ASR service directory.")
      .addText((text) =>
        text
          .setPlaceholder("../asr-service")
          .setValue(this.plugin.settings.asrServicePath)
          .onChange(async (value) => {
            this.plugin.settings.asrServicePath = value.trim() || "../asr-service";
            await this.plugin.saveSettings();
          })
      );

    new Setting(containerEl)
      .setName("ASR service port")
      .setDesc("Localhost port for the ASR service.")
      .addText((text) =>
        text
          .setPlaceholder("8765")
          .setValue(String(this.plugin.settings.asrServicePort))
          .onChange(async (value) => {
            const port = Number.parseInt(value, 10);
            if (Number.isInteger(port) && port > 0 && port < 65536) {
              this.plugin.settings.asrServicePort = port;
              await this.plugin.saveSettings();
            }
          })
      );

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
      .setDesc("Provider used for meeting summaries.")
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
      .setDesc("Used by OpenAI-compatible summary providers.")
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
