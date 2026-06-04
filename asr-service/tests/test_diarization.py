from __future__ import annotations

import unittest
from unittest.mock import patch

from echonote_asr.diarization import (
    DiarizationState,
    SpeakerInterval,
    assign_speakers_to_turns,
    iter_pyannote_intervals,
    merge_adjacent_turns,
    move_pipeline_to_device,
    resolve_torch_device_name,
)
from echonote_asr.schemas import DiarizationStatus, TranscriptTurn


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
    def device(self, name: str) -> str:
        return name


class FakePipeline:
    def __init__(self, *, fail_devices: set[str] | None = None) -> None:
        self.devices: list[str] = []
        self.fail_devices = fail_devices or set()

    def to(self, device: str) -> None:
        self.devices.append(device)
        if device in self.fail_devices:
            raise RuntimeError(f"{device} failed")


if __name__ == "__main__":
    unittest.main()
