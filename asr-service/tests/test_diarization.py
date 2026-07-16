from __future__ import annotations

import os
import threading
import time
import unittest
from concurrent.futures import ThreadPoolExecutor
from unittest.mock import patch

from echonote_asr import diarization as diarization_module
from echonote_asr.diarization import (
    DEFAULT_ACCELERATOR_SEGMENTATION_STEP,
    DEFAULT_CPU_SEGMENTATION_STEP,
    DiarizationState,
    MIN_SPEAKER_OVERLAP_RATIO,
    NATIVE_THREAD_ENVIRONMENT_VARIABLES,
    SpeakerInterval,
    assign_speakers_to_turns,
    best_speaker_for_turn,
    configure_native_thread_environment,
    configure_pipeline_segmentation_step,
    configure_torch_cpu_threads,
    diarization_state_from_environment,
    iter_pyannote_intervals,
    merge_adjacent_turns,
    move_pipeline_to_device,
    resolve_segmentation_step,
    resolve_torch_device_name,
)
from echonote_asr.schemas import DiarizationStatus, TranscriptSpeaker, TranscriptTurn


class DiarizationAssignmentTest(unittest.TestCase):
    def test_status_is_disabled_when_feature_is_disabled(self) -> None:
        status = DiarizationState(enabled=False).status_response()

        self.assertEqual(status["status"], DiarizationStatus.DISABLED)

    def test_status_is_unavailable_when_token_is_missing(self) -> None:
        with patch("echonote_asr.diarization.pyannote_available", return_value=True):
            with patch("echonote_asr.diarization.huggingface_token", return_value=""):
                status = DiarizationState(enabled=True).status_response()

        self.assertEqual(status["status"], DiarizationStatus.UNAVAILABLE)
        self.assertEqual(status["error"], "Hugging Face token is not configured")

    def test_assigns_speaker_by_largest_overlap(self) -> None:
        turns = [
            TranscriptTurn("turn-1", "hello", None, 0, 1000),
            TranscriptTurn("turn-2", "world", None, 1000, 2000),
        ]
        intervals = [
            SpeakerInterval("SPEAKER_A", 0, 900),
            SpeakerInterval("SPEAKER_B", 1100, 2000),
        ]

        assigned, speakers = assign_speakers_to_turns(turns, intervals)

        self.assertEqual([turn.speaker for turn in assigned], ["Speaker 1", "Speaker 2"])
        self.assertEqual([speaker.label for speaker in speakers], ["Speaker 1", "Speaker 2"])
        self.assertEqual([speaker.id for speaker in speakers], ["speaker_1", "speaker_2"])

    def test_keeps_unknown_speaker_when_overlap_is_too_small(self) -> None:
        turns = [TranscriptTurn("turn-1", "hello", None, 0, 1000)]
        intervals = [SpeakerInterval("SPEAKER_A", 0, 100)]

        assigned, speakers = assign_speakers_to_turns(turns, intervals)

        self.assertIsNone(assigned[0].speaker)
        self.assertEqual(speakers, [])

    def test_assignment_preserves_original_turn_and_interval_encounter_order(self) -> None:
        turns = [
            TranscriptTurn("later", "later", None, 1000, 2000),
            TranscriptTurn("earlier", "earlier", None, 0, 1000),
            TranscriptTurn("tie", "tie", None, 2000, 3000),
        ]
        intervals = [
            SpeakerInterval("SPEAKER_B", 2500, 3000),
            SpeakerInterval("SPEAKER_A", 0, 2000),
            SpeakerInterval("SPEAKER_A", 2000, 2500),
        ]

        assigned, speakers = assign_speakers_to_turns(turns, intervals)

        self.assertEqual([turn.id for turn in assigned], ["later", "earlier", "tie"])
        self.assertEqual([turn.speaker for turn in assigned], ["Speaker 1", "Speaker 1", "Speaker 2"])
        self.assertEqual([speaker.label for speaker in speakers], ["Speaker 1", "Speaker 2"])

    def test_sweep_assignment_matches_naive_full_scan(self) -> None:
        turns = [
            TranscriptTurn(
                f"turn-{index}",
                f"text-{index}",
                None,
                ((index * 1703) % 20_000),
                ((index * 1703) % 20_000) + 300 + (index % 7) * 100,
            )
            for index in range(120)
        ]
        intervals = [
            SpeakerInterval(
                f"raw-{index % 5}",
                ((index * 911) % 20_000),
                ((index * 911) % 20_000) + 200 + (index % 11) * 80,
            )
            for index in range(240)
        ]

        actual, actual_speakers = assign_speakers_to_turns(turns, intervals)
        expected, expected_speakers = naive_assign_speakers(turns, intervals)

        self.assertEqual(actual, expected)
        self.assertEqual(actual_speakers, expected_speakers)

    def test_large_assignment_keeps_overlap_checks_near_linear(self) -> None:
        turns = [
            TranscriptTurn(f"turn-{index}", "text", None, index * 3000, (index + 1) * 3000)
            for index in range(2000)
        ]
        intervals = [
            SpeakerInterval(f"raw-{index % 4}", index * 1500, (index + 1) * 1500)
            for index in range(4000)
        ]

        with patch(
            "echonote_asr.diarization.interval_overlap_ms",
            wraps=diarization_module.interval_overlap_ms,
        ) as overlap:
            assign_speakers_to_turns(turns, intervals)

        self.assertLessEqual(overlap.call_count, 8000)

    def test_merges_adjacent_same_speaker_conservatively(self) -> None:
        turns = [
            TranscriptTurn("turn-1", "我们先看目标。", "Speaker 1", 0, 1000, 0.9),
            TranscriptTurn("turn-2", "然后看风险。", "Speaker 1", 1600, 2500, 0.8),
            TranscriptTurn("turn-3", "我补充一点。", "Speaker 2", 2600, 3200, 0.9),
        ]

        merged = merge_adjacent_turns(turns)

        self.assertEqual(len(merged), 2)
        self.assertEqual(merged[0].text, "我们先看目标。然后看风险。")
        self.assertEqual(merged[0].speaker, "Speaker 1")
        self.assertEqual(merged[0].ended_at_ms, 2500)
        self.assertEqual(merged[1].speaker, "Speaker 2")

    def test_reads_community_pipeline_diarize_output(self) -> None:
        output = FakeDiarizeOutput(
            FakeAnnotation(
                [
                    (FakeSegment(0.031, 3.727), "SPEAKER_00"),
                    (FakeSegment(4.638, 7.861), "SPEAKER_01"),
                ]
            )
        )

        intervals = list(iter_pyannote_intervals(output))

        self.assertEqual(
            intervals,
            [
                SpeakerInterval("SPEAKER_00", 31, 3727),
                SpeakerInterval("SPEAKER_01", 4638, 7861),
            ],
        )

    def test_auto_device_prefers_mps_when_cuda_is_unavailable(self) -> None:
        torch = FakeTorch(cuda_available=False, mps_available=True)

        self.assertEqual(resolve_torch_device_name(torch, "auto"), "mps")

    def test_auto_device_prefers_cuda_when_available(self) -> None:
        torch = FakeTorch(cuda_available=True, mps_available=True)

        self.assertEqual(resolve_torch_device_name(torch, "auto"), "cuda")

    def test_move_pipeline_to_device_falls_back_to_cpu_when_accelerator_fails(self) -> None:
        pipeline = FakePipeline(fail_devices={"mps"})

        with patch("echonote_asr.diarization.resolve_torch_device_name", return_value="mps"):
            with patch.dict("sys.modules", {"torch": FakeTorchModule()}):
                moved = move_pipeline_to_device(pipeline, "auto")

        self.assertIs(moved, pipeline)
        self.assertEqual(pipeline.devices, ["mps", "cpu"])

    def test_environment_configures_device_thread_budget_and_segmentation_step(self) -> None:
        with patch.dict(
            os.environ,
            {
                "ECHONOTE_DIARIZATION_DEVICE": "cpu",
                "ECHONOTE_DIARIZATION_CPU_THREADS": "1",
                "ECHONOTE_DIARIZATION_SEGMENTATION_STEP": "0.25",
            },
            clear=False,
        ):
            state = diarization_state_from_environment()

        self.assertEqual(state.device, "cpu")
        self.assertEqual(state.cpu_threads, 1)
        self.assertEqual(state.segmentation_step, 0.25)

    def test_invalid_environment_performance_values_fall_back_safely(self) -> None:
        with patch.dict(
            os.environ,
            {
                "ECHONOTE_DIARIZATION_CPU_THREADS": "0",
                "ECHONOTE_DIARIZATION_SEGMENTATION_STEP": "not-a-number",
            },
            clear=False,
        ):
            state = diarization_state_from_environment()

        self.assertGreaterEqual(state.cpu_threads, 1)
        self.assertIsNone(state.segmentation_step)

    def test_cpu_uses_balanced_segmentation_step_by_default(self) -> None:
        self.assertEqual(resolve_segmentation_step(None, "cpu"), DEFAULT_CPU_SEGMENTATION_STEP)
        self.assertEqual(resolve_segmentation_step(None, "mps"), DEFAULT_ACCELERATOR_SEGMENTATION_STEP)

    def test_configures_pipeline_segmentation_stride(self) -> None:
        pipeline = FakePerformancePipeline(duration=10.0)

        configured = configure_pipeline_segmentation_step(pipeline, 0.2)

        self.assertTrue(configured)
        self.assertEqual(pipeline.segmentation_step, 0.2)
        self.assertEqual(pipeline._segmentation.step, 2.0)

    def test_configures_torch_cpu_thread_budget(self) -> None:
        torch = FakeTorchModule()

        configured = configure_torch_cpu_threads(torch, 2)

        self.assertTrue(configured)
        self.assertEqual(torch.num_threads, 2)

    def test_configures_native_thread_budget(self) -> None:
        with patch.dict(os.environ, {"OMP_NUM_THREADS": "8"}, clear=False):
            configure_native_thread_environment(2)

            self.assertTrue(
                all(os.environ[name] == "2" for name in NATIVE_THREAD_ENVIRONMENT_VARIABLES)
            )

    def test_serializes_concurrent_diarization_calls(self) -> None:
        state = DiarizationState(device="cpu")
        pipeline = ConcurrentTrackingPipeline()
        state._pipeline = pipeline

        with patch.object(state, "_availability_error", return_value=None):
            with ThreadPoolExecutor(max_workers=2) as executor:
                results = list(executor.map(state.diarize_wav, ["first.wav", "second.wav"]))

        self.assertEqual([result.status for result in results], [DiarizationStatus.AVAILABLE] * 2)
        self.assertEqual(pipeline.max_active_calls, 1)


def naive_assign_speakers(
    turns: list[TranscriptTurn],
    intervals: list[SpeakerInterval],
) -> tuple[list[TranscriptTurn], list[TranscriptSpeaker]]:
    labels: dict[str, str] = {}
    totals: dict[str, int] = {}
    assigned: list[TranscriptTurn] = []
    for turn in turns:
        raw_speaker, ratio = best_speaker_for_turn(turn, intervals)
        speaker = None
        if raw_speaker is not None and ratio >= MIN_SPEAKER_OVERLAP_RATIO:
            speaker = labels.setdefault(raw_speaker, f"Speaker {len(labels) + 1}")
            totals[speaker] = totals.get(speaker, 0) + max(0, turn.ended_at_ms - turn.started_at_ms)
        assigned.append(TranscriptTurn(
            id=turn.id,
            text=turn.text,
            speaker=speaker,
            started_at_ms=turn.started_at_ms,
            ended_at_ms=turn.ended_at_ms,
            confidence=ratio if speaker is not None else turn.confidence,
        ))
    speakers = [
        TranscriptSpeaker(id=f"speaker_{index}", label=label, total_ms=totals.get(label, 0))
        for index, label in enumerate(labels.values(), start=1)
    ]
    return assigned, speakers


class FakeDiarizeOutput:
    def __init__(self, annotation: FakeAnnotation) -> None:
        self.exclusive_speaker_diarization = annotation


class FakeAnnotation:
    def __init__(self, tracks: list[tuple["FakeSegment", str]]) -> None:
        self._tracks = tracks

    def itertracks(self, *, yield_label: bool = False) -> list[tuple["FakeSegment", None, str]]:
        if not yield_label:
            raise ValueError("test fake only supports labels")
        return [(segment, None, speaker) for segment, speaker in self._tracks]


class FakeSegment:
    def __init__(self, start: float, end: float) -> None:
        self.start = start
        self.end = end


class FakeTorch:
    def __init__(self, *, cuda_available: bool, mps_available: bool) -> None:
        self.cuda = FakeDeviceBackend(cuda_available)
        self.backends = FakeBackends(mps_available)


class FakeBackends:
    def __init__(self, mps_available: bool) -> None:
        self.mps = FakeDeviceBackend(mps_available)


class FakeDeviceBackend:
    def __init__(self, available: bool) -> None:
        self._available = available

    def is_available(self) -> bool:
        return self._available


class FakeTorchModule:
    def __init__(self) -> None:
        self.num_threads: int | None = None

    def device(self, name: str) -> str:
        return name

    def set_num_threads(self, value: int) -> None:
        self.num_threads = value


class FakePipeline:
    def __init__(self, *, fail_devices: set[str] | None = None) -> None:
        self.devices: list[str] = []
        self.fail_devices = fail_devices or set()

    def to(self, device: str) -> None:
        self.devices.append(device)
        if device in self.fail_devices:
            raise RuntimeError(f"{device} failed")


class FakeSegmentationInference:
    def __init__(self, duration: float) -> None:
        self.duration = duration
        self.step = duration * 0.1


class FakePerformancePipeline:
    def __init__(self, *, duration: float) -> None:
        self.segmentation_step = 0.1
        self._segmentation = FakeSegmentationInference(duration)


class ConcurrentTrackingPipeline:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self.active_calls = 0
        self.max_active_calls = 0

    def __call__(self, wav_path: str) -> FakeAnnotation:
        del wav_path
        with self._lock:
            self.active_calls += 1
            self.max_active_calls = max(self.max_active_calls, self.active_calls)
        time.sleep(0.02)
        with self._lock:
            self.active_calls -= 1
        return FakeAnnotation([])


if __name__ == "__main__":
    unittest.main()
