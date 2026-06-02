from __future__ import annotations

import argparse
import io
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import wave
from pathlib import Path

from fastapi.testclient import TestClient

from echonote_asr.app import create_app


SAMPLE_RATE = 16000
SILENCE_MS = 800


def main() -> int:
    parser = argparse.ArgumentParser(description="Run a local v0.4.0 pyannote diarization smoke test.")
    parser.add_argument("--voice-a", default="Daniel", help="macOS say voice for speaker A.")
    parser.add_argument("--voice-b", default="Samantha", help="macOS say voice for speaker B.")
    args = parser.parse_args()

    if shutil.which("say") is None:
        print("FAIL: macOS say command is unavailable.", file=sys.stderr)
        return 2
    if shutil.which("ffmpeg") is None:
        print("FAIL: ffmpeg is unavailable.", file=sys.stderr)
        return 2

    token = resolve_huggingface_token()
    if not token:
        print("FAIL: Hugging Face token is not configured in env or local Hugging Face cache.", file=sys.stderr)
        return 2
    os.environ["HUGGINGFACE_HUB_TOKEN"] = token

    with tempfile.TemporaryDirectory(prefix="echonote-v040-diarization-") as tmp:
        tmp_dir = Path(tmp)
        speaker_a = synthesize_voice(
            tmp_dir,
            "speaker-a",
            args.voice_a,
            "We should start by reviewing the project goals. The first milestone is improving transcription quality.",
        )
        speaker_b = synthesize_voice(
            tmp_dir,
            "speaker-b",
            args.voice_b,
            "I agree with the goal. We also need speaker labels so meetings are easier to review.",
        )
        meeting_wav, segments = build_meeting_wav(tmp_dir, speaker_a, speaker_b)
        response = finalize_with_real_diarization(meeting_wav, segments)

    speakers = [turn.get("speaker") for turn in response["turns"] if turn.get("speaker")]
    unique_speakers = sorted(set(speakers))
    if len(unique_speakers) < 2:
        print(json.dumps(response, ensure_ascii=False, indent=2), file=sys.stderr)
        print("FAIL: expected at least two speaker labels.", file=sys.stderr)
        return 1

    print(
        json.dumps(
            {
                "status": "pass",
                "diarization_status": response["diarization_status"],
                "speakers": response["speakers"],
                "turn_speakers": speakers,
            },
            ensure_ascii=False,
            indent=2,
        )
    )
    return 0


def resolve_huggingface_token() -> str:
    for key in ["HUGGINGFACE_HUB_TOKEN", "HF_TOKEN", "PYANNOTE_AUTH_TOKEN"]:
        value = os.environ.get(key, "").strip()
        if value:
            return value
    try:
        from huggingface_hub import get_token

        return (get_token() or "").strip()
    except Exception:
        return ""


def synthesize_voice(tmp_dir: Path, stem: str, voice: str, text: str) -> Path:
    aiff_path = tmp_dir / f"{stem}.aiff"
    wav_path = tmp_dir / f"{stem}.wav"
    subprocess.run(["say", "-v", voice, "-o", str(aiff_path), text], check=True)
    subprocess.run(
        [
            "ffmpeg",
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            str(aiff_path),
            "-ac",
            "1",
            "-ar",
            str(SAMPLE_RATE),
            "-c:a",
            "pcm_s16le",
            str(wav_path),
        ],
        check=True,
    )
    return wav_path


def build_meeting_wav(tmp_dir: Path, speaker_a: Path, speaker_b: Path) -> tuple[Path, list[dict[str, object]]]:
    frames_a = read_pcm_frames(speaker_a)
    frames_b = read_pcm_frames(speaker_b)
    silence = b"\x00\x00" * int(SAMPLE_RATE * SILENCE_MS / 1000)
    meeting_path = tmp_dir / "meeting.wav"

    with wave.open(str(meeting_path), "wb") as wav_file:
        wav_file.setnchannels(1)
        wav_file.setsampwidth(2)
        wav_file.setframerate(SAMPLE_RATE)
        wav_file.writeframes(frames_a + silence + frames_b)

    first_end_ms = frames_duration_ms(frames_a)
    second_start_ms = first_end_ms + SILENCE_MS
    second_end_ms = second_start_ms + frames_duration_ms(frames_b)
    segments = [
        transcript_segment("chunk-000001", "We should start by reviewing the project goals.", 0, first_end_ms),
        transcript_segment(
            "chunk-000002",
            "I agree with the goal. We also need speaker labels.",
            second_start_ms,
            second_end_ms,
        ),
    ]
    return meeting_path, segments


def read_pcm_frames(path: Path) -> bytes:
    with wave.open(str(path), "rb") as wav_file:
        if wav_file.getnchannels() != 1 or wav_file.getframerate() != SAMPLE_RATE or wav_file.getsampwidth() != 2:
            raise RuntimeError(f"unexpected WAV format for {path}")
        return wav_file.readframes(wav_file.getnframes())


def frames_duration_ms(frames: bytes) -> int:
    return round((len(frames) / 2) / SAMPLE_RATE * 1000)


def transcript_segment(chunk_id: str, text: str, start_ms: int, end_ms: int) -> dict[str, object]:
    return {
        "chunk_id": chunk_id,
        "text": text,
        "turns": [
            {
                "id": f"{chunk_id}-turn-001",
                "text": text,
                "speaker": None,
                "started_at_ms": start_ms,
                "ended_at_ms": end_ms,
                "confidence": None,
            }
        ],
        "started_at_ms": start_ms,
        "ended_at_ms": end_ms,
        "language": "en",
        "model_id": "test-model",
    }


def finalize_with_real_diarization(meeting_wav: Path, segments: list[dict[str, object]]) -> dict[str, object]:
    client = TestClient(create_app(default_model="test-model", backend="fake"))
    response = client.post("/model/load", json={"model_id": "test-model"})
    response.raise_for_status()

    with meeting_wav.open("rb") as audio:
        response = client.post(
            "/transcript/finalize",
            data={
                "meeting_id": "v0-4-0-diarization-smoke",
                "segments_json": json.dumps(segments),
                "language": "en",
                "enable_diarization": "true",
            },
            files={"audio": ("meeting.wav", audio.read(), "audio/wav")},
        )
    response.raise_for_status()
    body = response.json()
    return body


if __name__ == "__main__":
    raise SystemExit(main())
