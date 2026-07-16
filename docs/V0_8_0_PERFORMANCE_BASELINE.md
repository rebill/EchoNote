# EchoNote v0.8.0 Performance Baseline

Measured on 2026-07-16 before v0.8.0 algorithm changes.

## Environment

- MacBook Air `Mac14,2`
- Apple M2, 8 cores
- 16 GB memory
- macOS 15.7.7
- Plugin benchmark runtime: Node.js v23.11.0, arm64
- ASR benchmark runtime: Python 3.14.3, arm64

## Commands

```bash
cd plugin
npm run benchmark:plugin

cd ../asr-service
.venv/bin/python benchmarks/performance_benchmark.py
```

## Plugin Baseline

| Benchmark | Input | Median | p95 | v0.8.0 budget |
| --- | ---: | ---: | ---: | ---: |
| Extract transcript | 20,000 lines / 1,029,096 chars | 0.284 ms | 0.444 ms | p95 ≤ 0.60 ms |
| Verify meeting note | 20,000 lines / 1,029,096 chars | 0.300 ms | 0.435 ms | p95 ≤ 0.60 ms |
| Replace summary sections | 20,000 lines / 1,029,096 chars | 2.758 ms | 3.604 ms | p95 ≤ 4.50 ms |
| Format transcript | 10,000 turns | 52.140 ms | 63.245 ms | p95 ≤ 45 ms |
| Sanitize pathological transcript | 500 repeated sentences | 0.246 ms | 0.277 ms | p95 ≤ 0.40 ms |
| Concatenate WAV | 40 × 15-second chunks / 10 minutes | 55.555 ms | 56.767 ms | p95 ≤ 19 ms |

## ASR Baseline

| Benchmark | Input | Median | p95 | v0.8.0 budget |
| --- | ---: | ---: | ---: | ---: |
| Assign speakers | 2,000 turns / 4,000 intervals | 810.371 ms | 811.408 ms | p95 ≤ 80 ms |
| Merge adjacent turns | 2,000 turns | 0.139 ms | 0.142 ms | p95 ≤ 0.25 ms |
| Sanitize pathological transcript | 500 repeated sentences | 0.566 ms | 0.570 ms | p95 ≤ 1.00 ms |

## Interpretation

1. Speaker assignment is the strongest CPU optimization candidate because the current implementation scans every interval for every turn.
2. WAV assembly performs per-sample reads and writes and should move to bulk byte copies.
3. Transcript formatting is noticeable at 10,000 turns but is below the two primary hotspots.
4. Markdown section operations are already fast; they should be protected from regressions rather than aggressively rewritten.
5. LLM summary performance is network-bound, so its baseline is measured in request rounds rather than local milliseconds. The current implementation performs one sequential request per chunk plus one final merge request.

## P2 Results

Measured with the same plugin benchmark runner after the audio-pipeline changes:

| Benchmark | Storage | Assembly | Output | Retained ArrayBuffer delta |
| --- | --- | ---: | ---: | ---: |
| 10-minute stop | Memory | 2.230 ms | 18.311 MiB | 18.311 MiB |
| 30-minute stop | Disk | 35.577 ms | 54.932 MiB | 54.474 MiB |
| 60-minute stop | Disk | 31.478 ms | 109.863 MiB | 109.864 MiB |

The ten-minute bulk WAV benchmark improved from a 55.555 ms median to 1.852 ms (30.0x). Meetings larger than the
32 MiB PCM threshold spill to disk, and retained ArrayBuffer growth during final assembly stays approximately equal
to the final WAV output rather than the output plus a second full set of chunk payloads.

## P4 Results

| Benchmark | Baseline median | v0.8.0 median | Improvement | v0.8.0 p95 | Budget |
| --- | ---: | ---: | ---: | ---: | ---: |
| Assign 2,000 turns / 4,000 intervals | 810.371 ms | 5.485 ms | 147.7x | 8.910 ms | p95 <= 80 ms |

The sweep/window implementation preserves original turn order, original interval tie-breaking, overlap confidence,
and speaker-label encounter order. A deterministic equivalence test compares it with the previous full-scan behavior.

## Additional Results

| Benchmark | Baseline median | v0.8.0 median | Improvement | v0.8.0 p95 |
| --- | ---: | ---: | ---: | ---: |
| Format 10,000 transcript turns | 52.140 ms | 31.202 ms | 1.67x | 43.522 ms |
| ASR temporary I/O | 0.546 ms | 0.254 ms | 2.15x | 2.727 ms |

The transcript formatter reuses parsed correction rules for a whole batch and short-circuits most repeated-run
comparisons. The ASR service reuses one bounded workspace, reducing median non-inference temporary-I/O overhead by
53.5% while keeping inference serialized.
