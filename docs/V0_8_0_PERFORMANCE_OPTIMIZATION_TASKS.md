# EchoNote v0.8.0 Performance Optimization Tasks

## Goal

在不降低转录、总结和说话人识别正确性的前提下，降低长会议的端到端等待时间、峰值内存和主线程阻塞时间。所有优化必须先有可复现基线，再用相同输入证明收益。

## Sequential Delivery Order

| Order | Task | Scope | Dependency | Exit Gate |
| --- | --- | --- | --- | --- |
| 0 | P0 Establish performance baselines | Plugin + ASR | None | Benchmarks produce stable JSON results. |
| 1 | P1 Optimize long-meeting summaries | Plugin LLM | P0 | Turn-aware chunks, bounded concurrency, hierarchical merge. |
| 2 | P2 Optimize recording memory and note writes | Plugin audio/meeting | P0 | Faster WAV assembly and bounded write pressure. |
| 3 | P3 Optimize ASR scheduling and temporary I/O | ASR service | P0, P2 | Lower per-chunk overhead without unsafe model concurrency. |
| 4 | P4 Optimize speaker assignment/finalization | ASR diarization | P0 | Replace quadratic interval scans and preserve labels. |
| 5 | P5 Add regression gates and release closure | CI/docs/release | P1-P4 | Performance budgets, full tests, package and manual QA pass. |

## P0 Establish Performance Baselines

- [x] Add a plugin benchmark runner with machine-readable output.
- [x] Benchmark large-note section parsing and summary replacement.
- [x] Benchmark formatting 10,000 transcript turns.
- [x] Benchmark concatenating ten minutes of 16 kHz PCM WAV chunks.
- [x] Add an ASR benchmark runner.
- [x] Benchmark speaker assignment, adjacent-turn merging, and transcript sanitizing.
- [x] Record hardware, runtime versions, median, p95, and input sizes.
- [x] Freeze performance budgets before algorithm changes.

## P1 Optimize Long-Meeting Summaries

- [x] Split at transcript-turn or paragraph boundaries instead of arbitrary character offsets.
- [x] Keep every chunk within a configurable provider budget.
- [x] Run partial summaries with bounded concurrency and stable result ordering.
- [x] Retry only failed partial requests.
- [x] Merge summaries hierarchically so the final merge cannot exceed its own budget.
- [x] Expose progress in the EchoNote status panel.
- [x] Add deterministic planner tests and simulated-latency benchmarks.

Acceptance targets:

- Preserve all input text exactly once across planned chunks.
- Reduce long-summary critical-path request rounds by at least 40% for 6 or more chunks.
- Never exceed the configured chunk or merge-input budget.

## P2 Optimize Recording Memory And Note Writes

- [x] Replace per-sample WAV concatenation with bulk byte copies.
- [x] Avoid copying every chunk payload before final assembly.
- [x] Introduce bounded audio spooling for long meetings instead of retaining unlimited WAV chunks in renderer memory.
- [x] Coalesce transcript appends while preserving visible near-real-time updates.
- [x] Replace polling-based queue drain with completion signaling.
- [x] Measure stop latency and renderer memory at 10, 30, and 60 minutes.

Acceptance targets:

- Improve ten-minute WAV assembly median latency by at least 3x.
- Keep additional assembly memory close to the final output size.
- Keep live transcript persistence delay below two seconds.

## P3 Optimize ASR Scheduling And Temporary I/O

- [x] Measure model-lock wait, temporary-file write, inference, and response serialization separately.
- [x] Reuse a bounded temporary workspace where safe.
- [x] Avoid unnecessary WAV copies between request validation and inference.
- [x] Keep model inference serialized unless the backend proves concurrent execution safe.
- [x] Add structured latency fields to ASR logs.
- [x] Add fake-backend concurrency and cleanup tests.

Acceptance targets:

- Reduce non-inference per-chunk service overhead by at least 30%.
- Preserve deterministic queue ordering and temporary-file cleanup.

## P4 Optimize Speaker Assignment And Finalization

- [x] Sort and normalize turns and intervals once.
- [x] Replace per-turn full interval scans with a sweep/window algorithm.
- [x] Preserve overlap-ratio, speaker-label, and merge semantics.
- [x] Measure assignment separately from pyannote inference.
- [x] Report diarization queue wait, model load, inference, assignment, and merge durations.
- [ ] Run real diarization smoke verification when local model access is available. (Blocked locally: Hugging Face token is not configured.)

Acceptance targets:

- Improve synthetic large-meeting speaker assignment median latency by at least 10x.
- Keep all existing diarization contract and merge tests passing.

## P5 Regression Gates And Release Closure

- [x] Store representative benchmark budgets in version control.
- [x] Add stable algorithmic benchmarks to CI without machine-sensitive wall-clock assertions.
- [x] Keep hardware-dependent timing benchmarks as a release report.
- [x] Update PRD, technical design, API contract, README, troubleshooting, and changelog.
- [x] Align v0.8.0 version metadata.
- [x] Run plugin, ASR, Companion, package, and Obsidian CLI verification.
- [x] Compare final results against the P0 baseline and document deltas.

## Release Rule

性能数字不是完成条件的替代品。任何优化只要改变输出顺序、丢失转录内容、破坏文件写入安全性，或让失败恢复变差，就不能进入 v0.8.0。
