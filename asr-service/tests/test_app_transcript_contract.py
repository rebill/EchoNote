from __future__ import annotations

import io
import json
import struct
import unittest
import wave

from fastapi.testclient import TestClient

from echonote_asr.app import create_app
from echonote_asr.diarization import DiarizationResult, SpeakerInterval
from echonote_asr.schemas import DiarizationStatus


class TranscriptContractTest(unittest.TestCase):
    def setUp(self) -> None:
        self.client = self.enterContext(TestClient(create_app(default_model="test-model", backend="fake")))
        response = self.client.post("/model/load", json={"model_id": "test-model"})
        self.assertEqual(response.status_code, 200)

    def test_transcribe_returns_text_and_turns(self) -> None:
        with self.assertLogs("echonote_asr.app", level="INFO") as captured:
            response = self.client.post(
                "/transcribe",
                data={
                    "chunk_id": "chunk-000001",
                    "started_at_ms": "1000",
                    "ended_at_ms": "2500",
                    "language": "zh",
                },
                files={"audio": ("chunk.wav", create_silent_wav(1500), "audio/wav")},
            )

        self.assertEqual(response.status_code, 200, response.text)
        body = response.json()
        self.assertEqual(body["chunk_id"], "chunk-000001")
        self.assertIn("fake transcript for chunk chunk-000001", body["text"])
        self.assertEqual(len(body["turns"]), 1)
        self.assertEqual(body["turns"][0]["id"], "chunk-000001-turn-001")
        self.assertIsNone(body["turns"][0]["speaker"])
        self.assertEqual(body["turns"][0]["started_at_ms"], 1000)
        self.assertEqual(body["turns"][0]["ended_at_ms"], 2500)
        completion = next(record for record in captured.records if record.getMessage() == "transcribe_completed")
        for field in (
            "_lock_wait_ms",
            "_temp_write_ms",
            "_inference_ms",
            "_cleanup_ms",
            "_response_serialize_ms",
            "_request_total_ms",
        ):
            self.assertIsInstance(getattr(completion, field), float)

    def test_finalize_without_diarization_returns_no_speaker_turns(self) -> None:
        segment = {
            "chunk_id": "chunk-000001",
            "text": "hello",
            "turns": [
                {
                    "id": "turn-1",
                    "text": "hello",
                    "speaker": None,
                    "started_at_ms": 0,
                    "ended_at_ms": 1000,
                    "confidence": None,
                }
            ],
            "started_at_ms": 0,
            "ended_at_ms": 1000,
            "language": "en",
            "model_id": "test-model",
        }

        response = self.client.post(
            "/transcript/finalize",
            data={
                "meeting_id": "meeting-1",
                "segments_json": json.dumps([segment]),
                "language": "en",
                "enable_diarization": "false",
            },
            files={"audio": ("meeting.wav", create_silent_wav(1000), "audio/wav")},
        )

        self.assertEqual(response.status_code, 200, response.text)
        body = response.json()
        self.assertEqual(body["meeting_id"], "meeting-1")
        self.assertEqual(body["diarization_status"], "disabled")
        self.assertEqual(body["turns"][0]["text"], "hello")
        self.assertIsNone(body["turns"][0]["speaker"])
        self.assertEqual(body["speakers"], [])

    def test_diarization_status_reports_unavailable_without_optional_dependency(self) -> None:
        response = self.client.get("/diarization/status")

        self.assertEqual(response.status_code, 200, response.text)
        body = response.json()
        self.assertIn(body["status"], {"available", "unavailable", "failed"})
        self.assertEqual(body["model_id"], "pyannote/speaker-diarization-community-1")

    def test_finalize_with_missing_diarization_dependency_degrades(self) -> None:
        segment = {
            "chunk_id": "chunk-000001",
            "text": "hello",
            "started_at_ms": 0,
            "ended_at_ms": 1000,
            "language": "en",
            "model_id": "test-model",
        }

        response = self.client.post(
            "/transcript/finalize",
            data={
                "meeting_id": "meeting-1",
                "segments_json": json.dumps([segment]),
                "language": "en",
                "enable_diarization": "true",
            },
            files={"audio": ("meeting.wav", create_silent_wav(1000), "audio/wav")},
        )

        self.assertEqual(response.status_code, 200, response.text)
        body = response.json()
        self.assertIn(body["diarization_status"], {"unavailable", "failed"})
        self.assertEqual(body["turns"][0]["text"], "hello")
        self.assertIsNone(body["turns"][0]["speaker"])

    def test_finalize_with_available_diarization_returns_speaker_turns(self) -> None:
        client = self.enterContext(TestClient(
            create_app(
                default_model="test-model",
                backend="fake",
                diarization_state=FakeDiarizationState(
                    [
                        SpeakerInterval("SPEAKER_A", 0, 1200),
                        SpeakerInterval("SPEAKER_B", 1500, 2600),
                    ]
                ),
            )
        ))
        response = client.post("/model/load", json={"model_id": "test-model"})
        self.assertEqual(response.status_code, 200)
        segments = [
            {
                "chunk_id": "chunk-000001",
                "text": "first",
                "turns": [
                    {
                        "id": "turn-1",
                        "text": "first",
                        "speaker": None,
                        "started_at_ms": 0,
                        "ended_at_ms": 1000,
                        "confidence": None,
                    }
                ],
                "started_at_ms": 0,
                "ended_at_ms": 1000,
                "language": "en",
                "model_id": "test-model",
            },
            {
                "chunk_id": "chunk-000002",
                "text": "second",
                "turns": [
                    {
                        "id": "turn-2",
                        "text": "second",
                        "speaker": None,
                        "started_at_ms": 1500,
                        "ended_at_ms": 2500,
                        "confidence": None,
                    }
                ],
                "started_at_ms": 1500,
                "ended_at_ms": 2500,
                "language": "en",
                "model_id": "test-model",
            },
        ]

        response = client.post(
            "/transcript/finalize",
            data={
                "meeting_id": "meeting-1",
                "segments_json": json.dumps(segments),
                "language": "en",
                "enable_diarization": "true",
            },
            files={"audio": ("meeting.wav", create_silent_wav(3000), "audio/wav")},
        )

        self.assertEqual(response.status_code, 200, response.text)
        body = response.json()
        self.assertEqual(body["diarization_status"], "available")
        self.assertEqual([turn["speaker"] for turn in body["turns"]], ["Speaker 1", "Speaker 2"])
        self.assertEqual([speaker["label"] for speaker in body["speakers"]], ["Speaker 1", "Speaker 2"])


def create_silent_wav(duration_ms: int) -> bytes:
    sample_rate = 16000
    samples = int(sample_rate * duration_ms / 1000)
    buffer = io.BytesIO()
    with wave.open(buffer, "wb") as wav_file:
        wav_file.setnchannels(1)
        wav_file.setsampwidth(2)
        wav_file.setframerate(sample_rate)
        wav_file.writeframes(struct.pack("<" + "h" * samples, *([0] * samples)))
    return buffer.getvalue()


class FakeDiarizationState:
    def __init__(self, intervals: list[SpeakerInterval]) -> None:
        self.intervals = intervals

    def status_response(self) -> dict[str, object]:
        return {
            "status": DiarizationStatus.AVAILABLE,
            "model_id": "fake-diarization",
            "error": None,
        }

    def diarize_wav(self, wav_path: str) -> DiarizationResult:
        return DiarizationResult(
            status=DiarizationStatus.AVAILABLE,
            intervals=self.intervals,
            model_id="fake-diarization",
            error=None,
        )


if __name__ == "__main__":
    unittest.main()
