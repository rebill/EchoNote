# EchoNote MVP API 与类型契约

## 1. 目录边界

EchoNote MVP 使用两个独立顶层代码目录：

- `plugin/`：Obsidian TypeScript 插件工程。
- `asr-service/`：Python 本地 ASR 服务工程。

仓库根目录不直接放业务源码。

## 2. 默认设置

TypeScript 契约文件：[plugin/src/settings/settings.ts](../plugin/src/settings/settings.ts)

关键默认值：

| 字段 | 默认值 |
| --- | --- |
| `meetingFolder` | `Meetings` |
| `meetingTitleFormat` | `YYYY-MM-DD HH-mm Meeting` |
| `enableTimestamps` | `true` |
| `asrModelPreset` | `mlx-community/Qwen3-ASR-0.6B-4bit` |
| `pythonPath` | `python3` |
| `asrServicePath` | `../asr-service` |
| `asrServicePort` | `8765` |
| `chunkLengthSeconds` | `15` |
| `autoStartAsrService` | `true` |
| `audioInputDeviceId` | `default` |
| `audioInputDeviceLabel` | `Default audio input` |
| `saveRawAudio` | `false` |
| `audioSaveFolder` | `Meetings/audio` |
| `llmProvider` | `openai-compatible` |
| `openaiBaseUrl` | `https://api.openai.com/v1` |
| `summaryLanguage` | `zh` |

## 3. 状态契约

TypeScript 契约文件：[plugin/src/status/status-types.ts](../plugin/src/status/status-types.ts)

`EchoNoteStatus` 字段：

- `microphonePermission`
- `asrService`
- `model`
- `selectedModel`
- `selectedAudioInput`
- `recording`
- `currentMeetingPath`
- `currentMeetingTitle`
- `pendingChunkCount`
- `lastTranscriptAt`
- `lastError`

## 4. ASR HTTP API

TypeScript 契约文件：[plugin/src/asr/asr-types.ts](../plugin/src/asr/asr-types.ts)

Python 契约文件：[asr-service/echonote_asr/schemas.py](../asr-service/echonote_asr/schemas.py)

### `GET /health`

响应：

```json
{
  "status": "ok",
  "service": "echonote-asr",
  "version": "0.1.0"
}
```

### `GET /model/status`

响应：

```json
{
  "model_id": "mlx-community/Qwen3-ASR-0.6B-4bit",
  "status": "ready",
  "error": null
}
```

`status` 可选值：

- `not_loaded`
- `loading`
- `ready`
- `error`

### `POST /model/load`

请求：

```json
{
  "model_id": "mlx-community/Qwen3-ASR-0.6B-4bit"
}
```

响应：

```json
{
  "model_id": "mlx-community/Qwen3-ASR-0.6B-4bit",
  "status": "loading"
}
```

### `POST /transcribe`

请求类型：`multipart/form-data`

字段：

- `audio`：16kHz mono PCM 16-bit WAV。
- `chunk_id`：音频分段 ID。
- `started_at_ms`：会议内起始偏移，毫秒。
- `ended_at_ms`：会议内结束偏移，毫秒。
- `language`：可选，`auto`、`zh` 或 `en`。

响应：

```json
{
  "chunk_id": "chunk_000012",
  "text": "我们今天主要讨论 EchoNote 第一版的范围。",
  "started_at_ms": 180000,
  "ended_at_ms": 195000,
  "language": "zh",
  "model_id": "mlx-community/Qwen3-ASR-0.6B-4bit"
}
```

`TranscriptSegment` 必须包含：

- `chunk_id`
- `text`
- `started_at_ms`
- `ended_at_ms`
- `model_id`

### `POST /shutdown`

响应：

```json
{
  "status": "shutting_down"
}
```

## 5. 音频契约

TypeScript 契约文件：[plugin/src/audio/audio-types.ts](../plugin/src/audio/audio-types.ts)

`AudioChunk` 字段：

- `id`
- `startedAtMs`
- `endedAtMs`
- `wavBytes`
- `createdAt`
- `durationMs`
- `rms`

插件发送给 ASR 服务的音频格式固定为：

- WAV
- 16kHz
- mono
- PCM 16-bit

转录队列过滤规则：

- 小于 1 秒的音频分段不发送给 ASR。
- RMS 低于静音阈值的音频分段不发送给 ASR。
- 被过滤分段仍可参与完整会议 WAV 合并保存。

EchoNote 录音输入设备契约：

- 默认 `audioInputDeviceId` 为 `default`。
- 插件需要通过 `navigator.mediaDevices.enumerateDevices()` 获取 `audioinput` 设备。
- 用户可以选择 BlackHole、Loopback 等虚拟混音输入设备。
- 录音时应关闭浏览器音频处理：
  - `echoCancellation: false`
  - `noiseSuppression: false`
  - `autoGainControl: false`

## 6. LLM 契约

TypeScript 契约文件：[plugin/src/llm/llm-types.ts](../plugin/src/llm/llm-types.ts)

`MeetingSummary` 必须包含：

- `summary`
- `decisions`
- `actionItems`
- `keyPoints`
- `openQuestions`

`LlmProvider` 必须实现：

```ts
generateSummary(request: SummaryRequest): Promise<MeetingSummary>
```

LLM 输出必须解析为 JSON：

```json
{
  "summary": "...",
  "decisions": ["..."],
  "actionItems": ["..."],
  "keyPoints": ["..."],
  "openQuestions": ["..."]
}
```

总结写回规则：

- 只更新 `Summary`、`Decisions`、`Action Items`、`Key Points`、`Open Questions`。
- 不覆盖 `Transcript`。
- LLM 返回无法解析的 JSON 时，不写入会议笔记。

## 7. 错误契约

TypeScript 契约文件：[plugin/src/utils/errors.ts](../plugin/src/utils/errors.ts)

`EchoNoteError` 字段：

- `code`
- `message`
- `detail`
- `recoverable`
- `createdAt`

错误码集合在 `EchoNoteErrorCode` 中冻结。新增错误码必须同步更新本文档和交付计划。
