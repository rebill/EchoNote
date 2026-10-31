# EchoNote v1.0.0 Desktop App Tasks

## Goal

用一个 Tauri 2 应用替代 Obsidian 插件、Companion、Python ASR 服务和 VS Code 扩展：macOS 和 Windows 上 CPU 实时转写，
双轨采集，应用内资料库，用户自选 LLM。每个里程碑先有可复现基线和门禁，再进入下一个里程碑。

- 需求：[PRD](V1_0_0_DESKTOP_APP_PRD.md)
- 方案：[技术方案](V1_0_0_DESKTOP_APP_TECH_DESIGN.md)
- 预算：[v1-performance-budgets.json](v1-performance-budgets.json)
- 分支：`feat/standalone-app`（基于 `release/v0.9.0`）

## Sequential Delivery Order

| Order | Milestone | Scope | Dependency | Exit Gate |
| --- | --- | --- | --- | --- |
| 0 | M0 Design and technical validation | Docs, spikes, bench | None | Gate decisions recorded; budgets and eval set frozen; user sign-off |
| 1 | M1 Headless core engine | `echonote-core`, `echonote-bench`, CI | M0 | Fixture WAVs through the full pipeline pass invariants and budgets in CI on macOS and Windows |
| 2 | M2 App shell and recording | Tauri app, UI | M1 | Real dual-track recording on both platforms meets live budgets in the packaged app |
| 3 | M3 Playback, editing, export | Core, UI | M2 | Playback seek, edits with revert, Markdown/TXT export, delete removes all files |
| 4 | M4 AI features | Core LLM, UI | M2 | Summary, Q&A, correction, live highlights against two providers plus mock-server contract tests |
| 5 | M5 High-accuracy model and tuning | Core, model manager | M3 | Verified model download, re-transcription, 3-hour soak, low-end Windows run |
| 6 | M6 Packaging and closure | Bundles, CI, docs | M1-M5 | DMG and NSIS install on clean machines; legacy removal approved separately |

## M0 Design And Technical Validation

- [x] Create `feat/standalone-app` worktree without touching the `codex/vscode-extension` working tree.
- [x] Write PRD, technical design, and this task list.
- [x] Add `scripts/dev/fetch_assets.py` with pinned sizes and SHA-256 for models and evaluation data.
- [x] Add `scripts/dev/build_eval_sets.py` for deterministic AISHELL-1, LibriSpeech, and synthetic dual-track meeting sets.
- [x] Add `echonote-bench` with `smoke`, `eval`, and `live` (virtual-time pipeline simulation) commands.
- [ ] Spike A: evaluate SenseVoice, Paraformer, Qwen3-ASR, Fun-ASR-Nano, streaming Zipformer on error rate, RTF, memory, load time.
- [ ] Spike A gate: choose the live model, the high-accuracy model, and S1 or S2.
- [x] Spike B: macOS Process Tap capture prototype; permission flow verified in an ad-hoc signed bundle on macOS 15.7.7 (`scripts/dev/macos_capture_probe.sh`).
- [ ] Spike B: Windows WASAPI loopback run with `scripts/dev/windows_m0.ps1 -Step capture`; verify silence gaps and device switching (needs Windows machine).
- [ ] Spike B: macOS 14.2–14.3 first-run permission flow (needs a 14.x machine or VM).
- [x] Spike C: real speaker bleed measured on MacBook Air (`scripts/dev/analyze_capture_probe.py`); text similarity separates bleed, energy correlation does not.
- [x] Spike C follow-up: hard segment cap in the simulator (`--hard-cap`), `continuous-zh` eval set and matrix jobs.
- [ ] Spike C: clean speaker-only recording for an ERLE measurement (moved to M1 week 1 with AEC3).
- [x] Spike D: static sherpa-onnx link on macOS arm64 (19.3 MB) and x64 (22.6 MB, Rosetta smoke matches arm64); Windows target type-checks.
- [ ] Spike D: Windows MSVC link with `+crt-static`; record binary size and DLL dependencies (`windows_m0.ps1 -Step build`).
- [x] Spike E: macOS 15.7.7 WKWebView plays, seeks and decodes Ogg/Opus, WebM/Opus and CAF/Opus (`scripts/dev/webview_audio_probe.sh`).
- [ ] Spike E: macOS 14.2 WebKit and WebView2.
- [x] License review (tech design section 15); TEN VAD excluded by default.
- [ ] Confirm the streaming Zipformer license before any S2 decision; write NOTICE in M2 with the final dependency list.
- [x] Architecture review of the technical design; findings folded into the design (section 18).
- [ ] Idle-time benchmark matrix on macOS (`scripts/dev/run_m0_bench.py`) and Windows (`windows_m0.ps1 -Step bench`).
- [ ] User sign-off on the memory budget split (core steady 450 MB, peak 550 MB, WebView 150 MB).
- [ ] Freeze `v1-performance-budgets.json`, eval-set manifests, and segment output semantics.
- [ ] Report M0 results to the user and get sign-off before M1.

Acceptance targets:

- Every gate decision cites a JSON report under `ECHONOTE_DEV_DATA/results/`.
- Eval-set manifests are reproducible: rebuilding them yields the recorded SHA-256.
- Windows numbers come from a 4-core, 8 GB machine; until then, Windows-dependent gates stay open.
## M1 Headless Core Engine

- [x] Capture abstraction (`AudioSource` trait) with fake, WAV-file, cpal microphone, macOS tap, and WASAPI loopback sources (`echonote-core::capture`).
- [x] Lock-free SPSC ring buffer per source; audio callback does no allocation and takes no lock (rtrb; full buffers dropped whole, the frame counter keeps the timeline right).
- [x] Timeline: per-track sample clock anchored to a shared monotonic start; zero-fill gaps; drift correction within ±20 ms (steady error = drift × 10 s; tested at ±500 and ±1500 ppm).
- [x] Resample to 16 kHz mono with rubato; downmix multichannel input.
- [x] Segmenter owned by EchoNote: VAD reports speech per window only; finals are ranges on one timeline buffer; hard cap with 0.4 s overlap stitched by token timestamps. Sherpa's relaxed mode is kept for pauses inside long utterances.
- [x] Segmenter and scheduler as one state machine in `echonote-core::pipeline` with an injected clock; `echonote-bench live` drives the same code.
- [x] CI gates use a fixed decode-cost model (0.03 s + 0.1 × audio s, in `gates.json`).
- [x] ASR scheduler: single worker, finals before previews, one preview slot shared by both tracks, adaptive preview interval.
- [x] Skip mic previews during suspected bleed: a sys preview that judges the mic preview a duplicate mutes the mic utterance until it ends.
- [x] Port `live_demo` onto `echonote_core::pipeline`.
- [x] ASR backlog spill: past about 60 s, drop in-memory PCM and transcribe the range later from the Ogg file.
- [x] Keep token timestamps from `OfflineAsr::decode` (`decode_transcript`).
- [ ] AEC3 spike in week 1: ERLE on a clean speaker-only recording; reserve a bounded look-behind stage for mic processing. Blocked: needs a clean speaker-only recording from the user's machine (no speech, other sounds off).
- [x] Native WASAPI source (windows-rs) with a silent render stream so loopback stays on the device clock; process loopback on Windows 11 (type-checked with clippy for the Windows target; not yet run on Windows).
- [x] macOS tap: enable only the tap stream (`kAudioDevicePropertyIOProcStreamUsage`), handle tap format changes, timestamp from `AudioTimeStamp`.
- [x] Overload policy: drop previews first; never drop audio or finals; drain on stop.
- [x] Port `text_sanitizer` (repeated-run collapse) and its tests from `asr-service` (`echonote-core::text`; matches the Python output on 3,005 generated cases).
- [x] Mic/sys duplicate suppression: time-aligned token matching, token-level trimming, provisional decision from sys previews (`echonote-core::crosstalk`); sys finals decode first.
- [ ] Recorded double-talk eval set (real speakers and room) with false-deletion and miss-rate gates; the synthetic `meeting-zh-dual` bleed run and the `meeting-dual` gate fixture are the stand-ins. Blocked: needs recordings with two real speakers in a room.
- [x] Session state machine: idle, starting, recording, paused, stopping, finalized, recovered (`echonote-core::engine`).
- [x] SQLite store (WAL, migrations with backup, FTS5 trigram) for meetings, tracks, segments, revisions, settings (`echonote-core::store`).
- [x] Store thread: one writer connection owned by the engine, read-only connections for UI queries.
- [x] Ogg/Opus writer per track with page-level writes, background fsync every 10 s (`F_FULLFSYNC` on macOS), per-track transcribed-until marker; probe, repair and decode-from-position for recovery (`echonote-core::recording`, vendored libopus in `crates/echonote-opus`).
- [x] Crash recovery on startup: repair the files of meetings left in `recording`, transcribe each track from its transcribed-until marker, mark the meeting recovered (with the session state machine).
- [x] Long-file seek accuracy on a 3-hour Ogg/Opus file in WKWebView (`scripts/dev/webview_seek_probe.sh`): Ogg/Opus fails (duration 6560 s of 10800 s, seeks off by up to 56 min); WAV is within ±27 ms. Playback serves WAV decoded on the fly (tech design section 12).
- [x] Model lifecycle: lazy load on record, buffered audio while loading, unload after 10 minutes idle.
- [x] `echonote-bench` regression gates: invariants, decode-work ratio, queue bound, error rate on frozen fixtures (`echonote-bench gate`; words lost and crosstalk on synthetic fixtures in CI, real-model error rate stays in `echonote-bench eval` because eval audio cannot be redistributed).
- [x] CI on macOS and Windows runners: fmt, clippy, tests, bench gates (`.github/workflows/desktop.yml`, sherpa archive checked against pinned SHA-256).
- [ ] First green run of the desktop workflow on GitHub; confirm the test counts match a local run. Needs a push, which waits for the user.
- [ ] Real-device run of the M1 capture code: macOS tap and microphone in `scripts/dev/live_demo.sh` (TCC prompt), WASAPI process and endpoint loopback on Windows 10 and 11 (needs a Windows machine).

Acceptance targets:

- Fixture pipeline output satisfies all invariants on both platforms.
- Decoded-audio ratio and decodes per audio minute stay within frozen budgets.
- Killing the process mid-recording loses at most the in-flight segment.

## M2 App Shell And Recording

- [x] Tauri 2 app (`app/`) with Svelte 5 + TypeScript + Vite; strict CSP and least-privilege capabilities (no inline scripts or styles; one capability: the app's commands by name plus event listen).
- [x] Meeting list with search; meeting detail with Summary, Transcript, Q&A tabs (Summary and Q&A are placeholders until M4).
- [x] Recording view: timer, per-track meters, live transcript (grey previews, final text), pause/resume/stop.
- [x] Virtualized transcript list; IPC deltas batched to at most 10 Hz (smoke run: 8.8 deltas/s, p5 gap 101 ms).
- [x] Tray/menu bar and global shortcut; single instance.
- [x] macOS permission flow for microphone and system audio; Windows microphone privacy guidance (system audio status through TCC preflight; Windows reads the privacy switches).
- [x] Auto-stop on silence; device hot switch; prevent sleep while recording.
- [x] Hide speaker labels when the sys track has no speech.
- [x] zh-CN and en UI strings.
- [x] Debug smoke run with fake input through the real UI (`scripts/dev/app_smoke.sh`).
- [ ] Packaged-app run on real devices: permission prompts, tray, shortcut, device switching, live budgets with a meeting client running (PRD A1, A2, A4, A5, A8, A9). macOS needs the user at the machine for the prompts; Windows needs a Windows machine.

Acceptance targets:

- PRD A1, A2, A4, A5, A8, A9 pass on macOS and Windows with the packaged app.
- Live budgets met in the packaged app while a meeting client is running.

## M3 Playback, Editing, Export

- [x] Playback via the M0 Spike E decision; click a segment to seek (custom protocol decodes the Ogg/Opus tracks to a mixed WAV per range; smoke run lands within 0.01 s of the segment start).
- [x] Segment editing with revisions and revert (revert restores the recognized text as a new revision; crosstalk lines can be shown or put back).
- [x] Glossary: rule replacements applied to display text; stored for hotwords and LLM prompts (schema V2; search also matches the misrecognized spellings).
- [x] Markdown and TXT export; remember the last folder (save dialog opened from Rust; the WebView never passes a path).
- [x] Delete meeting removes database rows and audio files; show storage usage (rename-then-delete, leftovers cleared at startup).

Acceptance targets:

- PRD A7 and A12 pass.
- Exported Markdown opens cleanly in Obsidian and keeps timestamps and speaker labels.

## M4 AI Features

- [ ] Provider abstraction: OpenAI-compatible (with presets) and Anthropic Messages; streaming SSE; cancel.
- [ ] Keys in the OS keychain; connection test; optional proxy and extra headers.
- [ ] Summary templates (built-in and custom), streaming Markdown, automatic title.
- [ ] Long transcripts: port the summary planner (boundary split, bounded concurrency, retries, hierarchical merge).
- [ ] Q&A with timestamp citations; FTS retrieval when the transcript exceeds the context budget.
- [ ] LLM correction: changed segments only, strict validation, diff view, per-segment accept/revert.
- [ ] Live highlights: opt-in, incremental every 3 minutes, never blocks ASR.
- [ ] Contract tests against a mock OpenAI/Anthropic server, including errors, rate limits, and partial streams.

Acceptance targets:

- PRD A10 and A11 pass.
- No API key or transcript text appears in logs.

## M5 High-Accuracy Model And Tuning

- [ ] Model manager: manifest with URLs, sizes, SHA-256, mirrors; resumable download; atomic install.
- [ ] Verify Qwen3-ASR files against the upstream GitHub release digest before publishing the manifest.
- [ ] Re-transcribe a saved meeting with the high-accuracy model; keep the previous revision.
- [ ] Traditional/Simplified output normalization for models that emit Traditional characters.
- [ ] Tune threads, preview interval, and VAD parameters against the frozen budgets.
- [ ] 3-hour soak on macOS and on the low-end Windows machine.

Acceptance targets:

- PRD A6 passes on both platforms.
- All budgets in `v1-performance-budgets.json` pass on the low-end baseline.

## M6 Packaging And Closure

- [ ] macOS arm64 and x64 DMG; Windows x64 NSIS; default model and VAD bundled.
- [ ] Self-signing instructions and Gatekeeper/SmartScreen guidance for unsigned builds.
- [ ] CI release matrix with build, tests, and bench gates.
- [ ] NOTICE with model and dependency licenses.
- [ ] README, CHANGELOG, privacy and troubleshooting docs for v1.
- [ ] Propose removal of `plugin/`, `companion/`, `asr-service/`, `vscode-extension/`; remove only after approval.

Acceptance targets:

- Installers work on clean macOS 14.2+ and Windows 10/11 machines without network access.
- Installer size within budget.
