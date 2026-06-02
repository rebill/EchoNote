# EchoNote v0.4.0 技术设计文档：智能切分与说话人分离

## 1. 文档信息

- 产品名称：EchoNote
- 对应 PRD：[docs/V0_4_0_SMART_CHUNKING_SPEAKER_DIARIZATION_PRD.md](./V0_4_0_SMART_CHUNKING_SPEAKER_DIARIZATION_PRD.md)
- 技术方案版本：v0.4.0
- 目标平台：macOS
- 主要客户端：EchoNote 桌面应用 + Obsidian Desktop 插件
- ASR 方案：本地 Python FastAPI ASR service + MLX Qwen ASR
- Diarization 方案：`pyannote.audio` + `pyannote/speaker-diarization-community-1`
- 文档状态：技术设计草案

## 2. 设计目标

v0.4.0 的技术目标是把现有“固定时长 chunk -> ASR -> 追加文本”链路升级为“两阶段 transcript pipeline”：

- 录音中：插件端自适应切分，ASR service 继续提供准实时 chunk 转录。
- 停止后：ASR service 使用完整会议音频做本地 diarization，并返回 speaker-aware final turns。

具体目标：

- 降低固定时长切分导致的语义断裂。
- 在不阻塞实时转录的前提下支持匿名 speaker label。
- 保持 Obsidian 插件轻量，不在插件运行时中加载 ML/diarization 模型。
- 保持本地优先，音频不发送到云端 ASR 或云端 diarization 服务。
- 为后续 speaker rename、Keychain、重新 finalize 预留接口。

## 3. 非目标

本设计不解决：

- 真实姓名识别。
- 跨会议 speaker 记忆。
- 实时流式 diarization。
- 词级时间戳强依赖。
- 完整重叠讲话拆分。
- 云端 diarization provider。
- 替换 MLX ASR backend。

## 4. 总体架构

```text
┌──────────────────────────────────────────────────────────────┐
│                      Obsidian Desktop                        │
│                                                              │
│  EchoNote Plugin                                             │
│  - AudioRecorder                                             │
│  - AdaptiveAudioChunker                                      │
│  - MeetingSessionController                                  │
│  - ASR Service Client                                        │
│  - MeetingNoteWriter                                         │
│  - In-memory complete meeting audio buffer                   │
└───────────────────────────────┬──────────────────────────────┘
                                │
                                │ /transcribe for live chunks
                                │ /transcript/finalize for final turns
                                │
┌───────────────────────────────▼──────────────────────────────┐
│                    Python ASR Service                        │
│                                                              │
│  FastAPI                                                     │
│  - GET /health                                               │
│  - GET /model/status                                         │
│  - GET /diarization/status                                   │
│  - POST /transcribe                                          │
│  - POST /transcript/finalize                                 │
│                                                              │
│  ModelState                                                  │
│  - MLX ASR transcriber                                       │
│                                                              │
│  DiarizationState                                            │
│  - pyannote pipeline loader                                  │
│  - availability and token checks                             │
│                                                              │
│  TranscriptFinalizer                                         │
│  - assign speakers by time overlap                           │
│  - merge adjacent turns                                      │
└───────────────────────────────┬──────────────────────────────┘
                                │
                                │ managed by
                                │
┌───────────────────────────────▼──────────────────────────────┐
│                         EchoNote.app                         │
│                                                              │
│  Companion                                                    │
│  - HF token setting                                          │
│  - dependency detection                                      │
│  - diarization availability status                           │
│  - process environment injection                             │
│  - redacted diagnostics and logs                             │
│  - discovery capabilities                                    │
└──────────────────────────────────────────────────────────────┘
```

Key boundary:

- Plugin owns audio capture, adaptive chunking, live transcript UI, Markdown writes.
- ASR service owns model inference, diarization, speaker assignment, final transcript response.
- Companion owns local runtime setup, HF token, dependency checks, diagnostics, discovery.

## 5. Shared Data Contracts

### 5.1 TranscriptTurn

TypeScript:

```ts
export type TranscriptTurn = {
  id: string;
  text: string;
  speaker: string | null;
  started_at_ms: number;
  ended_at_ms: number;
  confidence?: number | null;
};
```

Python:

```py
@dataclass(frozen=True)
class TranscriptTurn:
    id: str
    text: str
    speaker: str | None
    started_at_ms: int
    ended_at_ms: int
    confidence: float | None = None
```

### 5.2 TranscriptSegment

`/transcribe` response remains compatible with existing clients by keeping `text`:

```ts
export type TranscriptSegment = {
  chunk_id: string;
  text: string;
  turns: TranscriptTurn[];
  started_at_ms: number;
  ended_at_ms: number;
  language: string | null;
  model_id: string;
};
```

For v0.4.0 live ASR, `turns` normally contains one turn:

```json
{
  "chunk_id": "chunk-000012",
  "text": "我们先看一下目标。",
  "turns": [
    {
      "id": "chunk-000012-turn-001",
      "text": "我们先看一下目标。",
      "speaker": null,
      "started_at_ms": 182000,
      "ended_at_ms": 188400,
      "confidence": null
    }
  ],
  "started_at_ms": 182000,
  "ended_at_ms": 188400,
  "language": "zh",
  "model_id": "mlx-community/Qwen3-ASR-0.6B-4bit"
}
```

### 5.3 Finalize Request

`POST /transcript/finalize` accepts complete meeting audio and live transcript turns:

```ts
export type FinalizeTranscriptRequestMetadata = {
  meeting_id: string;
  language?: "auto" | "zh" | "en";
  enable_diarization: boolean;
};
```

Multipart fields:

- `audio`: complete meeting WAV, 16kHz mono PCM16.
- `meeting_id`: stable meeting/session ID.
- `language`: optional.
- `enable_diarization`: boolean.
- `segments_json`: JSON array of live `TranscriptSegment` values.

Rationale:

- The complete WAV is needed by pyannote.
- Existing live segments preserve ASR output and timestamps.
- v0.4.0 can assign speaker by overlap without rerunning ASR over the entire meeting.

### 5.4 Finalize Response

```ts
export type TranscriptSpeaker = {
  id: string;
  label: string;
  total_ms: number;
};

export type FinalizeTranscriptResponse = {
  meeting_id: string;
  turns: TranscriptTurn[];
  speakers: TranscriptSpeaker[];
  model_id: string;
  diarization_model_id: string | null;
  diarization_status: "disabled" | "available" | "unavailable" | "failed";
  error: string | null;
};
```

`diarization_status` semantics:

- `disabled`: request disabled diarization.
- `available`: speaker labels were generated.
- `unavailable`: diarization was not configured or dependency/model unavailable.
- `failed`: diarization attempted but failed.

If status is `unavailable` or `failed`, response may still return final turns with `speaker: null`.

## 6. Plugin Design

### 6.1 AdaptiveAudioChunker

Replace fixed-sample draining with boundary-aware chunking.

Inputs:

- `Float32Array` from Web Audio.
- Input sample rate from `AudioContext`.

Outputs:

- Existing `AudioChunk` objects, still encoded as 16kHz mono PCM16 WAV.

Default config:

```ts
type AdaptiveChunkerConfig = {
  minChunkMs: 2000;
  maxChunkMs: 15000;
  silenceDurationMs: 800;
  boundaryPaddingMs: 200;
  silenceRmsThreshold: 0.002;
  analysisFrameMs: 20;
};
```

Algorithm:

1. Resample incoming PCM to 16kHz.
2. Append samples to pending buffer.
3. Analyze frames of `analysisFrameMs`.
4. Track consecutive silent frames by RMS threshold.
5. If pending duration >= `minChunkMs` and silence duration >= `silenceDurationMs`, cut at silence boundary.
6. Include `boundaryPaddingMs` after speech end when possible.
7. If pending duration >= `maxChunkMs`, force cut.
8. On stop, flush remaining pending samples.

Important constraints:

- Silence-only chunks are not sent to ASR.
- Silence samples remain in complete meeting audio for accurate timeline and diarization.
- Chunk IDs stay monotonic.
- `startedAtMs` and `endedAtMs` remain based on emitted sample count.

### 6.2 Complete Meeting Audio Buffer

Current code only stores raw chunks when `saveRawAudio` is enabled. v0.4.0 needs a separate internal buffer:

- Always keep complete meeting PCM/WAV chunks in memory while meeting is active.
- Release after stop/finalize completes.
- Persist only when `saveRawAudio` is true.

Recommended implementation:

- Store 16kHz PCM sample buffers or WAV chunks in `MeetingSessionController`.
- Use a single `concatWavFiles` output for finalize and optional save.
- Estimate memory: 16kHz mono PCM16 is roughly 115 MB/hour.

### 6.3 Live Segment Tracking

`MeetingSessionController` should keep all successful live `TranscriptSegment` values in memory:

```ts
private liveSegments: TranscriptSegment[] = [];
```

On successful `/transcribe`:

- Append segment to `liveSegments`.
- Write live segment to note.
- Update status.

On stop:

- Wait for queue drain.
- Call finalize with complete audio and `liveSegments`.
- If finalize returns useful turns, replace `## Transcript`.
- If finalize fails, keep live transcript.

### 6.4 MeetingNoteWriter

Add:

```ts
replaceTranscript(file: TFile, turns: TranscriptTurn[], enableTimestamps: boolean): Promise<void>
```

Rendering rules:

- If `speaker` exists: `[00:01:12] Speaker 1: text`
- If no speaker: `[00:01:12] text`
- Preserve existing summary sections.
- Replace only `## Transcript` section content.

## 7. ASR Service Design

### 7.1 Optional Dependencies

Update `asr-service/pyproject.toml`:

```toml
[project.optional-dependencies]
diarization = [
  "pyannote.audio>=3.3.0",
  "huggingface_hub>=0.23.0"
]
```

`mlx` remains separate. A full local install can use:

```bash
pip install -e 'asr-service[mlx,diarization]'
```

### 7.2 DiarizationState

Add `echonote_asr/diarization.py`:

Responsibilities:

- Check whether `pyannote.audio` is importable.
- Check whether token is available via explicit config or environment.
- Load `pyannote/speaker-diarization-community-1`.
- Run pipeline against complete WAV.
- Return normalized speaker intervals.

Recommended status model:

```py
class DiarizationAvailability(StrEnum):
    DISABLED = "disabled"
    AVAILABLE = "available"
    UNAVAILABLE = "unavailable"
    ERROR = "error"
```

Status endpoint:

```http
GET /diarization/status
```

Response:

```json
{
  "status": "available",
  "model_id": "pyannote/speaker-diarization-community-1",
  "error": null
}
```

### 7.3 Speaker Assignment

Input:

- Live transcript turns with start/end timestamps.
- Diarization intervals from pyannote.

Assignment:

1. For each transcript turn, compute overlap with each speaker interval.
2. Assign the speaker with largest overlap.
3. If best overlap is below minimum ratio, keep `speaker: null`.
4. Normalize speaker IDs to stable labels in first-seen order:
   - `speaker_1` -> `Speaker 1`
   - `speaker_2` -> `Speaker 2`
5. Merge adjacent same-speaker turns using conservative merge rules.

Default thresholds:

```py
MIN_SPEAKER_OVERLAP_RATIO = 0.35
MERGE_GAP_MS = 1200
MAX_MERGED_TURN_MS = 45000
MAX_MERGED_TURN_CHARS = 500
```

### 7.4 Finalize Failure Behavior

`/transcript/finalize` should prefer useful partial output over hard failure:

- If diarization unavailable: return turns with `speaker: null`, status `unavailable`, HTTP 200.
- If diarization fails after ASR/live turns are available: return turns with `speaker: null`, status `failed`, HTTP 200, `error` sanitized.
- If request is invalid or audio is invalid: return HTTP 400.
- If internal logic cannot produce any turns: return HTTP 500.

The plugin decides whether to replace transcript. It must not replace existing transcript with an empty `turns` array.

## 8. Companion Design

### 8.1 Settings

Add to Companion settings:

```ts
type CompanionSettings = {
  huggingFaceToken: string;
  diarizationEnabled: boolean;
  diarizationModelId: string;
};
```

Rust equivalent should use camelCase serde names.

Default:

- `huggingFaceToken = ""`
- `diarizationEnabled = true`
- `diarizationModelId = "pyannote/speaker-diarization-community-1"`

### 8.2 Process Environment

When starting ASR service:

- If token is present, pass it via `HUGGINGFACE_HUB_TOKEN`.
- Do not pass token through command-line args.
- Do not write token to discovery.
- Redact token from logs and diagnostic reports.

### 8.3 Setup Detection

Extend setup detector:

- Probe `pyannote.audio` import.
- Detect whether HF token is configured.
- Call `/diarization/status` when service is running.
- Show diarization as optional enhancement, not a runtime blocker.

### 8.4 Discovery Capabilities

Extend discovery schema with optional `capabilities`:

```ts
type CompanionCapabilities = {
  adaptiveChunking?: boolean;
  speakerDiarization?: "available" | "unavailable" | "disabled" | "error";
};
```

Existing plugin behavior must remain compatible if `capabilities` is absent.

## 9. Error Handling

Plugin:

- `/transcribe` failure: keep existing recoverable chunk error behavior.
- `/transcript/finalize` failure: keep live transcript, show Notice.
- Empty final turns: keep live transcript.
- Speaker labels unavailable: use no-speaker transcript, show non-blocking status.

ASR service:

- Token missing: status `unavailable`, not exception.
- Model terms not accepted or download denied: status `unavailable` or `error` with sanitized detail.
- pyannote import missing: status `unavailable`.
- diarization runtime exception: status `failed`.

Companion:

- HF token must be redacted in:
  - logs
  - diagnostic report
  - setup step details
  - error strings shown in UI

## 10. Testing Strategy

### 10.1 Unit Tests

Plugin:

- Adaptive chunker cuts on silence after min duration.
- Adaptive chunker force-cuts at max duration.
- Silence-only chunks are skipped for ASR.
- Transcript replacement only replaces `## Transcript`.
- Markdown rendering supports speaker and no-speaker turns.

ASR service:

- `TranscriptTurn` schema serialization.
- `/transcribe` returns `turns`.
- Speaker assignment by overlap.
- Speaker label normalization.
- Conservative merge rules.
- Diarization unavailable returns HTTP 200 with no-speaker turns.

Companion:

- Settings serialization with new diarization fields.
- Token redaction covers `HF_TOKEN`, `HUGGINGFACE_HUB_TOKEN`, bearer tokens, and raw configured token values.
- Discovery capability shape remains backward-compatible.

### 10.2 Integration Tests

- Fake backend `/transcribe` returns valid `turns`.
- Finalize with diarization disabled returns live turns.
- Finalize with fake diarization intervals assigns `Speaker 1`/`Speaker 2`.
- Plugin stop flow keeps live transcript when finalize fails.

### 10.3 Manual QA

- Single speaker long sentence.
- Continuous speech over 15 seconds.
- Two-speaker meeting sample.
- Missing HF token.
- Missing pyannote dependency.
- Invalid token or model access denied.
- Save raw audio disabled.
- Save raw audio enabled.
- Diagnostic report copied with token redacted.

## 11. Migration

- Existing settings should load with defaults for new fields.
- Existing notes remain unchanged.
- Existing `chunkLengthSeconds` can be kept for compatibility but should no longer drive default adaptive behavior directly.
- Existing API clients can continue reading `TranscriptSegment.text`.
- New clients should prefer `TranscriptSegment.turns`.

## 12. Implementation Order

Recommended order:

1. Freeze shared contracts.
2. Add schema/types and compatibility tests.
3. Implement adaptive chunker and plugin live segment tracking.
4. Add note transcript replacement.
5. Add ASR service finalize endpoint with diarization disabled/fake path.
6. Add diarization optional dependency and pyannote adapter.
7. Add Companion settings/status/redaction.
8. Wire plugin stop flow to finalize.
9. Update docs, API contract, privacy notes, and QA checklist.

## 13. Open Technical Risks

- pyannote performance on Apple Silicon must be measured with real meeting audio.
- Speaker assignment accuracy is bounded by ASR chunk timestamps if word-level timestamps are unavailable.
- Long meetings increase memory pressure until stop/finalize completes.
- Model access failures can be confusing unless Companion setup explains Hugging Face token and model terms clearly.
