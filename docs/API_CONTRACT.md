# EchoNote MVP API 与类型契约

## 1. 目录边界

EchoNote 使用以下顶层代码目录：

- `plugin/`：Obsidian TypeScript 插件工程。
- `asr-service/`：Python 本地 ASR 服务工程。
- `companion/`：v0.2.0 新增的 Tauri Companion 工程，负责本地 ASR runtime 管理。

仓库根目录不直接放业务源码。

## 2. 默认设置

TypeScript 契约文件：[plugin/src/settings/settings.ts](../plugin/src/settings/settings.ts)

关键默认值：

| 字段 | 默认值 |
| --- | --- |
| `meetingFolder` | `Meetings` |
| `meetingTitleFormat` | `YYYY-MM-DD HH-mm Meeting` |
| `enableTimestamps` | `true` |
| `asrRuntimeMode` | `companion` |
| `companionDiscoveryPath` | `~/Library/Application Support/EchoNote/companion.json` |
| `companionDiscoveryMaxAgeSeconds` | `30` |
| `chunkLengthSeconds` | `15` |
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
- `asrRuntime`
- `activeAsrRuntime`
- `asrService`
- `model`
- `selectedModel`
- `companionStatus`
- `companionApiUrl`
- `companionDiscoveryPath`
- `companionMessage`
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
  "version": "0.3.0"
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
  "turns": [
    {
      "id": "chunk_000012-turn-001",
      "text": "我们今天主要讨论 EchoNote 第一版的范围。",
      "speaker": null,
      "started_at_ms": 180000,
      "ended_at_ms": 195000,
      "confidence": null
    }
  ],
  "started_at_ms": 180000,
  "ended_at_ms": 195000,
  "language": "zh",
  "model_id": "mlx-community/Qwen3-ASR-0.6B-4bit"
}
```

`TranscriptSegment` 必须包含：

- `chunk_id`
- `text`
- `turns`
- `started_at_ms`
- `ended_at_ms`
- `model_id`

`TranscriptTurn` 必须包含：

- `id`
- `text`
- `speaker`
- `started_at_ms`
- `ended_at_ms`

`speaker` 在实时转录阶段通常为 `null`。

### `GET /diarization/status`

响应：

```json
{
  "status": "unavailable",
  "model_id": "pyannote/speaker-diarization-community-1",
  "error": "Hugging Face token is not configured"
}
```

`status` 可选值：

- `disabled`
- `available`
- `unavailable`
- `failed`

### `POST /transcript/finalize`

请求类型：`multipart/form-data`

字段：

- `audio`：完整会议 WAV，16kHz mono PCM 16-bit。
- `meeting_id`：会议 ID。
- `segments_json`：实时转录返回的 `TranscriptSegment[]` JSON 字符串。
- `language`：可选，`auto`、`zh` 或 `en`。
- `enable_diarization`：可选，`true` 或 `false`。

响应：

```json
{
  "meeting_id": "2026-06-01-Meeting",
  "turns": [
    {
      "id": "chunk_000001-turn-001",
      "text": "我们先看今天的目标。",
      "speaker": "Speaker 1",
      "started_at_ms": 3000,
      "ended_at_ms": 9000,
      "confidence": 0.92
    }
  ],
  "speakers": [
    {
      "id": "speaker_1",
      "label": "Speaker 1",
      "total_ms": 6000
    }
  ],
  "model_id": "mlx-community/Qwen3-ASR-0.6B-4bit",
  "diarization_model_id": "pyannote/speaker-diarization-community-1",
  "diarization_status": "available",
  "error": null
}
```

降级规则：

- `enable_diarization=false` 时返回 `diarization_status=disabled`，turns 的 `speaker` 可以为 `null`。
- 未配置 Hugging Face token 或未安装 `pyannote.audio` 时返回 HTTP 200 和 `diarization_status=unavailable`。
- diarization 运行失败时返回 HTTP 200 和 `diarization_status=failed`，不得返回空 transcript 覆盖实时稿。

### `POST /shutdown`

响应：

```json
{
  "status": "shutting_down"
}
```

## 5. Companion Discovery 契约

v0.2.0 使用本地 JSON 文件作为 Companion 和 Obsidian 插件之间的最小共享契约。

JSON Schema 文件：[docs/contracts/companion-discovery.schema.json](./contracts/companion-discovery.schema.json)

示例 fixtures：

- [running](./contracts/fixtures/companion.discovery.running.json)
- [stopped](./contracts/fixtures/companion.discovery.stopped.json)
- [error](./contracts/fixtures/companion.discovery.error.json)

默认 discovery path：

```text
~/Library/Application Support/EchoNote/companion.json
```

插件允许用户覆盖 discovery path，便于本地测试和 fixtures 驱动测试。插件读取时必须展开 `~` 为当前用户 home。

### 5.1 Schema v1

`companion.json` v1 必须只包含以下字段：

```ts
type CompanionServiceStatus =
  | "stopped"
  | "starting"
  | "running"
  | "stopping"
  | "error";

type CompanionBackend = "fake" | "mlx-audio";

type CompanionModelStatus =
  | "not_loaded"
  | "loading"
  | "ready"
  | "error"
  | "unknown";

type CompanionDiscovery = {
  version: 1;
  app: "EchoNote";
  service: "echonote-asr";
  status: CompanionServiceStatus;
  baseUrl: string;
  host: "127.0.0.1";
  port: number;
  backend: CompanionBackend;
  modelId: string;
  modelStatus: CompanionModelStatus;
  pid: number | null;
  updatedAt: string;
  capabilities?: {
    adaptiveChunking?: boolean;
    speakerDiarization?: "available" | "unavailable" | "disabled" | "failed" | "error";
  };
};
```

字段规则：

| 字段 | 规则 |
| --- | --- |
| `version` | 固定为 `1`。不兼容变更必须升级版本号。 |
| `app` | 固定为 `EchoNote`。 |
| `service` | 固定为 `echonote-asr`。 |
| `status` | Companion 管理的 ASR service 状态。插件只有在 `running` 时才可使用 `baseUrl`。 |
| `baseUrl` | 固定格式为 `http://127.0.0.1:<port>`，不得包含 path、query 或 trailing slash。 |
| `host` | v0.2.0 固定为 `127.0.0.1`。 |
| `port` | 整数，范围 `1..65535`，必须与 `baseUrl` 中的端口一致。 |
| `backend` | `fake` 或 `mlx-audio`。 |
| `modelId` | Companion 解析后的实际模型 ID，不能为空。 |
| `modelStatus` | 当前模型状态；无法确认时写 `unknown`。 |
| `pid` | Companion 当前管理的 ASR 子进程 PID；没有活跃子进程时为 `null`。 |
| `updatedAt` | UTC ISO 8601 时间戳，例如 `2026-05-21T06:34:00.000Z`。 |
| `capabilities` | v0.4.0 新增可选能力字段；旧客户端必须兼容缺失值。 |

安全约束：

- Discovery 文件不得包含 API key、LLM provider token、transcript、音频路径或日志正文。
- v0.2.0 只允许 localhost ASR endpoint；插件必须拒绝非 `127.0.0.1` 的 `host` 或 `baseUrl`。
- Companion 写入必须使用原子替换：先写临时文件，再 rename 到正式路径。

写入策略：

- 服务状态变化时立即写入。
- 模型状态变化时立即写入。
- `running` 状态下至少每 10 秒刷新一次 `updatedAt`，即使其他字段没有变化。
- `stopped`、`stopping`、`starting`、`error` 状态不要求心跳刷新，但状态变化必须写入最后一次已知状态。

### 5.2 Discovery Resolution

插件读取 discovery 时必须按以下顺序解析：

1. 文件不存在：返回 `missing`。
2. JSON 解析失败或 schema 校验失败：返回 `invalid`。
3. `status` 不是 `running`：返回 `not_running`，并保留原始 `status` 供状态面板展示。
4. `updatedAt` 与当前时间相差超过 `companionDiscoveryMaxAgeSeconds`：返回 `stale`。默认最大年龄为 `30` 秒。
5. `baseUrl` 不匹配 `host`/`port` 或不是 `http://127.0.0.1:<port>`：返回 `invalid`。
6. 对 `baseUrl` 执行 `GET /health` 二次确认；失败时返回 `unavailable`。
7. `/health` 返回可用后，返回 `available`，插件可使用 `baseUrl` 调用现有 ASR HTTP API。

`updatedAt` 超过最大年龄时直接视为 stale，不再因为 `/health` 成功而继续使用该 discovery。Companion 在 `running` 状态下有 10 秒心跳刷新义务，因此 stale 表示 Companion 对该 endpoint 的管理状态已经不可信。

建议 TypeScript 返回类型：

```ts
type CompanionResolution =
  | { kind: "available"; baseUrl: string; discovery: CompanionDiscovery }
  | { kind: "missing"; reason: string }
  | { kind: "invalid"; reason: string }
  | { kind: "not_running"; reason: string; status: CompanionServiceStatus }
  | { kind: "stale"; reason: string }
  | { kind: "unavailable"; reason: string };
```

## 6. ASR Runtime 契约

v0.2.0 插件只保留 Companion ASR runtime。ASR service 的启动、停止、重启和日志由 EchoNote 桌面应用管理，Obsidian 插件不再启动 Python ASR 进程。

```ts
type AsrRuntimeMode = "companion";
```

默认设置：

| 字段 | 默认值 |
| --- | --- |
| `asrRuntimeMode` | `companion` |
| `companionDiscoveryPath` | `~/Library/Application Support/EchoNote/companion.json` |
| `companionDiscoveryMaxAgeSeconds` | `30` |

运行语义：

| Discovery 行为 | ASR endpoint | 插件是否启动 Python ASR |
| --- | --- | --- |
| 必须解析到 `available`。任何 `missing`、`invalid`、`not_running`、`stale` 或 `unavailable` 都是用户可见错误。 | 仅 Companion `baseUrl`。 | 从不启动 Python ASR。 |

Runtime resolver 返回契约：

```ts
type AsrRuntime =
  {
    mode: "companion";
    requestedMode: "companion";
    baseUrl: string;
    companion: CompanionResolution & { kind: "available" };
  };
```

错误码映射：

| Discovery result | 错误码 |
| --- | --- |
| `missing` | `ASR_COMPANION_UNAVAILABLE` |
| `invalid` | `ASR_COMPANION_DISCOVERY_INVALID` |
| `not_running` | `ASR_COMPANION_UNAVAILABLE` |
| `stale` | `ASR_COMPANION_DISCOVERY_STALE` |
| `unavailable` | `ASR_COMPANION_UNAVAILABLE` |

插件不得调用 `AsrProcessManager.start()` 或其他插件侧 Python ASR 启动逻辑。旧配置中的 `auto`/`manual` 值在加载时会迁移为 `companion`。

## 7. 音频契约

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

## 8. LLM 契约

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

## 9. 错误契约

TypeScript 契约文件：[plugin/src/utils/errors.ts](../plugin/src/utils/errors.ts)

`EchoNoteError` 字段：

- `code`
- `message`
- `detail`
- `recoverable`
- `createdAt`

`EchoNoteErrorCode` 当前集合：

- `UNSUPPORTED_PLATFORM`
- `MIC_PERMISSION_DENIED`
- `ASR_SERVICE_START_FAILED`
- `ASR_SERVICE_UNAVAILABLE`
- `ASR_MODEL_LOAD_FAILED`
- `ASR_TRANSCRIBE_FAILED`
- `ASR_COMPANION_UNAVAILABLE`
- `ASR_COMPANION_DISCOVERY_INVALID`
- `ASR_COMPANION_DISCOVERY_STALE`
- `NOTE_CREATE_FAILED`
- `NOTE_WRITE_FAILED`
- `LLM_CONFIG_MISSING`
- `LLM_REQUEST_FAILED`
- `LLM_RESPONSE_PARSE_FAILED`

错误码集合在 `EchoNoteErrorCode` 中冻结。新增错误码必须同步更新本文档和交付计划。
