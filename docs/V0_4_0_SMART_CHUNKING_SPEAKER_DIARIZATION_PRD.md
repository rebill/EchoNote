# EchoNote v0.4.0 需求文档：智能切分与说话人分离

## 1. 文档信息

- 产品名称：EchoNote
- 目标版本：v0.4.0
- 版本主题：智能音频切分 + 匿名说话人分离
- 目标平台：macOS
- 主要客户端：EchoNote 桌面应用 + Obsidian Desktop 插件
- 技术路线：插件端自适应切分 + Python ASR service 会后 diarization 后处理
- 文档状态：需求草案

## 2. 背景

EchoNote 当前版本采用固定时长音频切分。插件端 `AudioChunker` 按配置的 `chunkLengthSeconds` 生成 10、15 或 30 秒 WAV 分段，然后发送到本地 ASR service 转录。

这带来两个明显问题：

- 固定时长切分容易在一句话中间截断音频，导致转录文本语义断裂。
- 当前转录结果只有文本和时间戳，没有说话人信息，多人会议中无法定位谁说了什么。

v0.4.0 需要在不牺牲现有本地优先和准实时体验的前提下，改善切分质量，并在会议结束后生成带匿名 speaker label 的最终转录稿。

## 3. 一句话描述

EchoNote v0.4.0 将固定时长切分升级为静音感知切分，并在会议停止后生成 `Speaker 1 / Speaker 2` 格式的最终会议转录。

## 4. 目标

### 4.1 产品目标

- 减少实时转录中完整句子被硬切断的情况。
- 在多人会议最终稿中标注匿名说话人。
- 保持录音中准实时转录可用，不让说话人分离阻塞会议记录。
- 保持默认本地处理，不引入云端 ASR 或云端 diarization 作为默认路径。
- 说话人分离失败时保留实时转录，不丢失用户数据。
- 默认不保存原始音频到 Obsidian vault。

### 4.2 工程目标

- 将插件端固定时长 `AudioChunker` 升级为自适应切分器。
- 在 transcript 数据模型中新增 `TranscriptTurn` 和 speaker 字段。
- 在 ASR service 中新增可选本地 diarization 能力。
- 使用 `pyannote.audio` 和 `pyannote/speaker-diarization-community-1` 作为 v0.4.0 默认 diarization 路线。
- 通过 Companion 管理 Hugging Face token，插件侧不可见。
- 扩展 API contract、文档和测试，确保 TypeScript/Python schema 一致。

## 5. 非目标

v0.4.0 不包含：

- 自动识别真实姓名。
- 跨会议声纹记忆。
- 用户手动重命名 speaker。
- Keychain 存储 Hugging Face token。
- 云端 diarization provider。
- 流式实时 diarization。
- 复杂重叠讲话拆分。
- 替换当前 MLX Qwen ASR 为 Whisper 或 WhisperX。
- Windows、Linux 或 Obsidian Mobile 支持。

这些能力可以在 v0.4.x 或后续版本中继续规划。

## 6. 目标用户

- 使用 EchoNote 记录多人会议、访谈、讨论或播客素材的用户。
- 对本地隐私和 Obsidian Markdown 工作流有要求的用户。
- 需要会后快速判断“谁提出了什么观点/行动项”的用户。
- 能接受首次配置 Hugging Face token 以下载本地 diarization 模型的用户。

## 7. 核心用户流程

### 7.1 实时录音与智能切分

1. 用户点击 `Start Meeting`。
2. EchoNote 创建会议笔记并开始录音。
3. 插件端持续接收 Web Audio PCM 数据。
4. 自适应切分器检测音量和静音窗口。
5. 遇到自然静音边界时生成 chunk。
6. 如果用户连续讲话超过最长阈值，强制生成 chunk。
7. ASR service 返回实时转录。
8. 插件将实时转录追加到 `## Transcript`。

体验目标：

- 正常情况下 3-8 秒出一段实时文本。
- 最长 15 秒必须出一段，避免 UI 长时间无反馈。
- chunk 优先落在静音边界，而不是固定秒数边界。

### 7.2 停止会议与最终稿生成

1. 用户点击 `Stop Meeting`。
2. 插件停止录音并等待实时转录队列完成。
3. 插件将会议期间临时保存在内存中的完整音频发送给 ASR service。
4. ASR service 运行 diarization 后处理。
5. ASR service 将 speaker 时间区间与已转录文本对齐。
6. 插件用 speaker-aware 最终稿覆盖 `## Transcript` 内容。
7. 插件释放内存中的完整音频。

最终稿格式：

```markdown
[00:00:03] Speaker 1: 我们先看今天的目标。
[00:00:11] Speaker 2: 好，我这边先补充背景。
```

### 7.3 降级流程

如果未配置 Hugging Face token、pyannote 依赖不可用、模型下载失败或 diarization 超时：

1. 录音和实时转录不受影响。
2. 停止会议后跳过或中止 diarization。
3. 如果能生成无 speaker 最终稿，则覆盖为无 speaker turns。
4. 如果 finalize 全部失败，则保留实时转录稿。
5. 用户看到清晰提示，说明说话人分离不可用或失败。

## 8. MVP 范围

### 8.1 自适应音频切分

必须支持：

- `minChunkSeconds = 2`。
- `targetLatencySeconds = 3-8`。
- `maxChunkSeconds = 15`。
- `silenceDurationMs = 600-900`。
- `boundaryPaddingMs = 200`。
- 连续讲话超过 `maxChunkSeconds` 时强制切分。
- 静音或过短 chunk 不发送 ASR，但仍保留在完整会议音频中。

`chunkLengthSeconds` 设置在 v0.4.0 中应改造为高级参数或被新设置替代。默认用户不需要理解固定 chunk length。

### 8.2 Transcript 数据模型

新增 `TranscriptTurn`：

```ts
type TranscriptTurn = {
  id: string;
  text: string;
  speaker: string | null;
  started_at_ms: number;
  ended_at_ms: number;
  confidence?: number | null;
};
```

`TranscriptSegment` 保留 `text` 字段以兼容现有实时转录逻辑，同时新增 `turns`：

```ts
type TranscriptSegment = {
  chunk_id: string;
  text: string;
  turns: TranscriptTurn[];
  started_at_ms: number;
  ended_at_ms: number;
  language: string | null;
  model_id: string;
};
```

### 8.3 匿名说话人分离

必须支持：

- `Speaker 1`、`Speaker 2`、`Speaker 3` 格式的匿名标签。
- 说话人 label 仅在单次会议内有效。
- 不承诺自动识别真实姓名。
- 连续同一 speaker 的相邻短片段保守合并。
- 无法判断 speaker 时允许 `speaker: null`。

建议合并规则：

- 同一 speaker。
- 相邻间隔小于 `mergeGapMs = 1200`。
- 合并后不超过 `maxTurnChars = 500`。
- 合并后不超过 `maxTurnMs = 45000`。
- 不同 speaker 一律不合并。

### 8.4 Companion 设置与状态

Companion 需要新增：

- Hugging Face token 设置项。
- Diarization 开关或 availability 状态。
- Diarization 模型 ID，默认 `pyannote/speaker-diarization-community-1`。
- 依赖检测和降级提示。
- 诊断报告中的 diarization 状态。

Hugging Face token 由 Companion 管理，Obsidian 插件不可见。

Discovery 文件不得包含 token。只允许暴露能力状态，例如：

```json
{
  "capabilities": {
    "adaptiveChunking": true,
    "speakerDiarization": "available"
  }
}
```

### 8.5 隐私要求

- 会议音频继续只发送到本机 `127.0.0.1` ASR service。
- 默认不保存完整音频到 Obsidian vault。
- 为了会后 finalize，插件可以在会议期间临时在内存中保留完整音频。
- 停止会议并完成 finalize 后必须释放内存音频。
- 只有用户开启 `Save raw audio` 时才保存完整 WAV。
- 日志、错误信息、diagnostic report、discovery 文件不得泄漏 Hugging Face token。

## 9. 用户可见文案

建议状态文案：

- `Speaker diarization ready`
- `Speaker diarization unavailable`
- `Hugging Face token required for speaker labels`
- `Speaker diarization failed. Live transcript was kept.`
- `Final transcript generated with speaker labels.`

避免使用需要用户理解内部实现的文案，例如 `pyannote pipeline crashed`。详细错误写入日志。

## 10. 验收标准

v0.4.0 发布前必须满足：

- 单人长句场景：不再每固定 15 秒硬切，chunk 优先落在静音边界。
- 连续讲话超过 15 秒：仍能强制切分并继续实时转录。
- 双人会议音频：最终稿包含 `Speaker 1:` / `Speaker 2:`，大部分话轮归属正确。
- 未配置 Hugging Face token：实时转录正常，最终稿无 speaker 或 speaker 为 null，用户看到清晰降级提示。
- diarization 依赖缺失：Companion 状态显示 unavailable，不影响会议开始和总结。
- diarization 失败或超时：保留实时 transcript，不覆盖为空稿。
- 默认不保存原始音频到 vault；开启 `Save raw audio` 才保存完整 WAV。
- 诊断日志、discovery、错误信息不泄漏 Hugging Face token。
- Summary 使用最终 `Transcript` 内容；如果 finalize 失败，则使用实时 transcript。
- API contract、TypeScript schema 和 Python schema 一致。

## 11. 风险

- pyannote 模型需要 Hugging Face token 和模型条款确认，首次使用门槛高于 ASR-only 路径。
- Apple Silicon 本地 diarization 性能需要实测，长会议可能 finalize 较慢。
- 当前 ASR backend 不一定提供词级时间戳，speaker 对齐只能先按 chunk/turn 时间区间做近似。
- 重叠讲话和多人抢话场景不会在 v0.4.0 中完全解决。
- 临时内存保存完整音频会增加长会议内存占用。

## 12. 参考

- [pyannote.audio](https://github.com/pyannote/pyannote-audio)
- [pyannote/speaker-diarization-community-1](https://huggingface.co/pyannote/speaker-diarization-community-1)
- [EchoNote API contract](./API_CONTRACT.md)
- [EchoNote Privacy Notes](./PRIVACY.md)
