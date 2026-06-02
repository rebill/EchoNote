# EchoNote v0.4.0 Smart Chunking And Speaker Diarization Task Breakdown

## 1. Goal

v0.4.0 的交付目标是解决两个质量问题：

- 当前固定时长音频切分容易切断完整句子。
- 当前转录无法区分多人会议中的说话人。

MVP 截止线：

> 用户录制一段多人会议音频；录音中可以准实时看到更自然的分段转录；停止会议后，EchoNote 在本地生成 `Speaker 1 / Speaker 2` 格式的最终 transcript。未配置或未安装 diarization 时，会议录音、实时转录和总结不受影响。

相关文档：

- [v0.4.0 PRD](./V0_4_0_SMART_CHUNKING_SPEAKER_DIARIZATION_PRD.md)
- [v0.4.0 Technical Design](./V0_4_0_SMART_CHUNKING_SPEAKER_DIARIZATION_TECH_DESIGN.md)
- [v0.4.0 QA Notes](./V0_4_0_SMART_CHUNKING_SPEAKER_DIARIZATION_QA.md)

## 2. Suggested Issues

| Order | Task | Track | Dependencies | Parallel Notes |
| --- | --- | --- | --- | --- |
| 0 | T0 Freeze transcript and finalize API contracts | Contract | None | Must finish first. |
| 1 | T1 Add shared transcript turn types in TypeScript and Python | Contract | T0 | Can run with docs/API updates. |
| 2 | T2 Add API contract fixtures and schema tests | Contract/Testing | T1 | Blocks integration confidence. |
| 3 | T3 Implement plugin adaptive audio chunker | Plugin/Audio | T1 | Independent from diarization backend. |
| 4 | T4 Track complete meeting audio and live segments in meeting session | Plugin/Meeting | T1, T3 | Needed by finalize. |
| 5 | T5 Add transcript replacement rendering in meeting note writer | Plugin/Meeting | T1 | Can run in parallel with T4. |
| 6 | T6 Add ASR `/transcribe` `turns` compatibility | ASR Service | T1 | Should preserve existing `text`. |
| 7 | T7 Add ASR `/transcript/finalize` without diarization | ASR Service | T1, T2 | First safe end-to-end finalize path. |
| 8 | T8 Implement speaker assignment and merge utilities | ASR Service | T7 | Can use fake diarization intervals. |
| 9 | T9 Add optional pyannote diarization adapter | ASR Service | T8 | Highest ML dependency risk. |
| 10 | T10 Add Companion diarization settings and HF token handling | Companion | T0 | Can run while ASR adapter lands. |
| 11 | T11 Extend Companion setup/status/discovery capabilities | Companion | T10 | Must remain backward-compatible. |
| 12 | T12 Wire plugin stop flow to finalize and transcript replacement | Plugin/Integration | T4, T5, T7 | First user-visible final transcript. |
| 13 | T13 Add degradation, timeout, and error handling | Integration | T9, T11, T12 | Release blocker. |
| 14 | T14 Update docs, privacy notes, troubleshooting, changelog | Docs/Release | T13 | Finish near release. |
| 15 | T15 Manual QA with single-speaker and two-speaker samples | QA | T13 | Release gate. |

## 3. Todo List

### Contract

- [x] T0 Freeze transcript and finalize API contracts.
  - Confirm `TranscriptTurn` fields.
  - Confirm `TranscriptSegment.turns` addition.
  - Confirm `/transcript/finalize` endpoint name.
  - Confirm finalize request uses complete WAV + `segments_json`.
  - Confirm finalize response status values.

- [x] T1 Add shared transcript turn types in TypeScript and Python.
  - Add `TranscriptTurn` to `plugin/src/asr/asr-types.ts`.
  - Add `TranscriptTurn` dataclass to `asr-service/echonote_asr/schemas.py`.
  - Add `turns` to `TranscriptSegment` in both languages.
  - Keep `text` required for backward compatibility.

- [x] T2 Add API contract fixtures and schema tests.
  - Update `docs/API_CONTRACT.md`.
  - Add fixture for `/transcribe` with `turns`.
  - Add fixture for `/transcript/finalize`.
  - Add tests that fake backend returns schema-compatible turns.

### Plugin Audio

- [x] T3 Implement plugin adaptive audio chunker.
  - Replace or wrap current fixed-size `AudioChunker`.
  - Resample to 16kHz as today.
  - Analyze RMS over short frames.
  - Cut after silence when chunk is at least 2 seconds.
  - Force cut at 15 seconds.
  - Preserve monotonic chunk IDs and accurate timestamps.
  - Add focused chunker unit tests.

### Plugin Meeting

- [x] T4 Track complete meeting audio and live segments in meeting session.
  - Maintain complete in-memory meeting audio while recording.
  - Keep `saveRawAudio` behavior as optional disk persistence only.
  - Store successful live `TranscriptSegment` values.
  - Release complete audio and segment buffers after stop/finalize.
  - Avoid sending silence-only chunks to ASR.

- [x] T5 Add transcript replacement rendering in meeting note writer.
  - Add `replaceTranscript`.
  - Render speaker turns as `[timestamp] Speaker N: text`.
  - Render no-speaker turns as `[timestamp] text`.
  - Replace only `## Transcript`.
  - Keep summary sections intact.

- [x] T12 Wire plugin stop flow to finalize and transcript replacement.
  - After queue drain, call `/transcript/finalize`.
  - Replace transcript only when final turns are non-empty.
  - Keep live transcript when finalize fails.
  - Ensure summary reads the final transcript if replacement succeeded.
  - Add recoverable user Notice for finalize failures.

### ASR Service

- [x] T6 Add ASR `/transcribe` `turns` compatibility.
  - Return one `TranscriptTurn` for each live chunk.
  - Use `speaker: null` for live transcription.
  - Keep fake backend deterministic.
  - Keep existing `text` field behavior.

- [x] T7 Add ASR `/transcript/finalize` without diarization.
  - Accept complete meeting WAV.
  - Accept `segments_json`.
  - Validate request.
  - Return live turns with `speaker: null` when diarization is disabled/unavailable.
  - Add endpoint tests with fake backend.

- [x] T8 Implement speaker assignment and merge utilities.
  - Normalize diarization intervals.
  - Assign speaker by maximum overlap.
  - Keep `speaker: null` below minimum overlap.
  - Normalize labels to `Speaker 1`, `Speaker 2`.
  - Merge adjacent same-speaker turns conservatively.
  - Add unit tests for overlap, labels, and merge limits.

- [x] T9 Add optional pyannote diarization adapter.
  - Add `diarization` optional dependency group.
  - Add `DiarizationState`.
  - Add `GET /diarization/status`.
  - Load `pyannote/speaker-diarization-community-1`.
  - Read token from `HUGGINGFACE_HUB_TOKEN`.
  - Convert pyannote output to normalized intervals.
  - Return unavailable/failed status without breaking ASR.

### Companion

- [x] T10 Add Companion diarization settings and HF token handling.
  - Add settings fields for `huggingFaceToken`, `diarizationEnabled`, `diarizationModelId`.
  - Add UI controls in advanced or ASR setup area.
  - Pass token to ASR service via environment, not command-line args.
  - Do not expose token to plugin or discovery.

- [x] T11 Extend Companion setup/status/discovery capabilities.
  - Probe optional pyannote dependency.
  - Probe token presence.
  - Call `/diarization/status` when service is running.
  - Add discovery `capabilities` without breaking existing schema.
  - Add status copy for ready/unavailable/failed.
  - Extend diagnostic report with redacted diarization state.

- [x] T13 Add degradation, timeout, and error handling.
  - Missing token should not block setup or meeting start.
  - Missing pyannote should not block setup or meeting start.
  - Diarization timeout should keep live transcript.
  - Finalize empty turns should not overwrite note.
  - Redact `HF_TOKEN`, `HUGGINGFACE_HUB_TOKEN`, bearer tokens, and raw configured token values.

### Docs And Release

- [x] T14 Update docs, privacy notes, troubleshooting, changelog.
  - Update `docs/API_CONTRACT.md`.
  - Update `docs/PRIVACY.md` for temporary in-memory complete audio.
  - Update `docs/TROUBLESHOOTING.md` for HF token and diarization failures.
  - Update README feature list and setup notes.
  - Update CHANGELOG for v0.4.0.

### QA

- [x] T15 Manual QA with single-speaker and two-speaker samples.
  - Single long sentence should cut near silence.
  - Continuous speech over 15 seconds should force cut.
  - Two-speaker sample should produce `Speaker 1` and `Speaker 2`.
  - Missing token should degrade clearly.
  - Missing pyannote should degrade clearly.
  - Finalize failure should keep live transcript.
  - Raw audio should only be saved when `Save raw audio` is enabled.
  - Diagnostic report should not leak token.

Manual QA result:

- Real pyannote two-speaker QA passed on 2026-06-01 after Hugging Face access was granted for `pyannote/speaker-diarization-community-1`. `scripts/v0_4_0_real_diarization_smoke.py` generated a synthetic two-speaker sample, called the real `/transcript/finalize` endpoint, and returned `diarization_status=available` with `Speaker 1` and `Speaker 2`. Automated coverage verifies adaptive chunking, no-speaker finalize degradation, speaker assignment/merge logic, token redaction, and fake-backend smoke behavior.

## 4. Parallel Development Plan

### Batch 0: Contract First

- T0 Freeze transcript and finalize API contracts.
- T1 Add shared transcript turn types.
- T2 Add API fixtures and schema tests.

Goal:

> TypeScript, Python, plugin integration, and docs all agree on the transcript shape before behavior changes land.

### Batch 1: Better Live Transcript

- T3 Adaptive audio chunker.
- T6 `/transcribe` `turns` compatibility.
- T5 transcript rendering helper.

Goal:

> Live transcription remains compatible, but chunk boundaries improve and transcript data is turn-aware.

### Batch 2: Finalize Without ML Risk

- T4 complete audio and live segment tracking.
- T7 finalize endpoint without diarization.
- T12 plugin stop flow to finalize.

Goal:

> Stop meeting can safely produce and replace a final transcript even before pyannote is enabled.

### Batch 3: Speaker Labels

- T8 speaker assignment and merge utilities.
- T9 pyannote adapter.
- T10 Companion token settings.
- T11 Companion status/discovery.

Goal:

> A configured local diarization environment can generate anonymous speaker-aware final transcript.

### Batch 4: Release Hardening

- T13 degradation and redaction hardening.
- T14 docs/release updates.
- T15 manual QA.

Goal:

> Speaker diarization is useful when available and harmless when unavailable.

## 5. Dependency Graph

```text
T0 contract
 └─ T1 shared types
     ├─ T2 fixtures/tests
     ├─ T3 adaptive chunker
     │   └─ T4 complete audio + live segments
     │       └─ T12 plugin finalize flow
     ├─ T5 transcript replacement
     │   └─ T12 plugin finalize flow
     └─ T6 transcribe turns
         └─ T7 finalize without diarization
             ├─ T8 speaker assignment
             │   └─ T9 pyannote adapter
             └─ T12 plugin finalize flow

T10 Companion token settings
 └─ T11 status/discovery
     └─ T13 degradation/redaction

T9, T11, T12
 └─ T13 degradation/redaction
     ├─ T14 docs/release
     └─ T15 QA
```

## 6. Suggested Ownership Split

If multiple developers or agents work in parallel:

- Contract owner: T0, T1, T2.
- Plugin audio owner: T3, T4.
- Plugin meeting owner: T5, T12.
- ASR service owner: T6, T7, T8, T9.
- Companion owner: T10, T11.
- Release owner: T13, T14, T15.

If one person works sequentially, use this order:

```text
T0 -> T1 -> T2 -> T3 -> T6 -> T5 -> T4 -> T7 -> T8 -> T10 -> T11 -> T9 -> T12 -> T13 -> T14 -> T15
```

## 7. Release Readiness Checklist

- [x] Live chunking no longer depends on fixed 15-second boundaries by default.
- [x] Continuous speech still force-cuts by 15 seconds.
- [x] `/transcribe` returns both `text` and `turns`.
- [x] `/transcript/finalize` works without diarization and returns no-speaker turns.
- [x] Configured pyannote diarization produces anonymous speaker labels.
- [x] Final transcript replaces `## Transcript` only when final turns are non-empty.
- [x] Finalize failure keeps live transcript.
- [x] Missing token and missing pyannote do not block meeting start.
- [x] `Save raw audio` remains the only path that persists complete WAV to vault.
- [x] HF token is not present in logs, discovery, diagnostic report, or plugin settings.
- [x] Summary uses the current `## Transcript` after finalize.
- [x] API contract, schemas, and fixtures are updated.
