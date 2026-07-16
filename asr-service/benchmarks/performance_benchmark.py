from __future__ import annotations

import json
import platform
import statistics
import sys
import tempfile
import time
from pathlib import Path
from typing import Callable

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from echonote_asr.diarization import (  # noqa: E402
    SpeakerInterval,
    assign_speakers_to_turns,
    merge_adjacent_turns,
)
from echonote_asr.schemas import TranscriptTurn  # noqa: E402
from echonote_asr.text_sanitizer import sanitize_transcript_text  # noqa: E402


def main() -> None:
    turns = [
        TranscriptTurn(
            id=f"turn-{index + 1}",
            text=f"Turn {index + 1} discusses performance.",
            speaker=None,
            started_at_ms=index * 3_000,
            ended_at_ms=(index + 1) * 3_000,
            confidence=0.9,
        )
        for index in range(2_000)
    ]
    intervals = [
        SpeakerInterval(
            speaker=f"speaker-{index % 4}",
            started_at_ms=index * 1_500,
            ended_at_ms=(index + 1) * 1_500,
        )
        for index in range(4_000)
    ]
    assigned_turns, _ = assign_speakers_to_turns(turns, intervals)
    pathological_text = "EchoNote performance regression sentence. " * 500
    wav_payload = bytes(44 + (16_000 * 15 * 2))

    def legacy_temp_write() -> None:
        with tempfile.TemporaryDirectory(prefix="echonote-asr-legacy-") as tmp_dir:
            Path(tmp_dir, "chunk.wav").write_bytes(wav_payload)

    reusable_workspace = tempfile.TemporaryDirectory(prefix="echonote-asr-reused-")
    reusable_wav_path = Path(reusable_workspace.name, "chunk.wav")

    def reused_temp_write() -> None:
        reusable_wav_path.write_bytes(wav_payload)
        reusable_wav_path.unlink()

    results = [
        benchmark(
            "diarization.assign_speakers.2k_turns_4k_intervals",
            5,
            lambda: assign_speakers_to_turns(turns, intervals),
        ),
        benchmark(
            "diarization.merge_adjacent.2k_turns",
            20,
            lambda: merge_adjacent_turns(assigned_turns),
        ),
        benchmark(
            "transcript.sanitize.pathological",
            50,
            lambda: sanitize_transcript_text(pathological_text),
        ),
        benchmark("asr.temp_io.legacy_workspace", 50, legacy_temp_write),
        benchmark("asr.temp_io.reused_workspace", 50, reused_temp_write),
    ]
    reusable_workspace.cleanup()

    print(
        json.dumps(
            {
                "runtime": {
                    "python": platform.python_version(),
                    "platform": platform.platform(),
                    "machine": platform.machine(),
                },
                "inputs": {
                    "turns": len(turns),
                    "intervals": len(intervals),
                },
                "results": results,
            },
            indent=2,
        )
    )


def benchmark(name: str, iterations: int, operation: Callable[[], object]) -> dict[str, object]:
    for _ in range(2):
        operation()

    durations_ms: list[float] = []
    for _ in range(iterations):
        started_at = time.perf_counter()
        operation()
        durations_ms.append((time.perf_counter() - started_at) * 1_000)

    ordered = sorted(durations_ms)
    p95_index = min(len(ordered) - 1, max(0, int(len(ordered) * 0.95)))
    return {
        "name": name,
        "iterations": iterations,
        "medianMs": round(statistics.median(ordered), 3),
        "p95Ms": round(ordered[p95_index], 3),
        "minMs": round(ordered[0], 3),
        "maxMs": round(ordered[-1], 3),
    }


if __name__ == "__main__":
    main()
