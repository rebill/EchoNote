from __future__ import annotations

import os
import importlib.util
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Iterable

from .schemas import DiarizationStatus, TranscriptSpeaker, TranscriptTurn
from .text_sanitizer import sanitize_transcript_text

DEFAULT_DIARIZATION_MODEL = "pyannote/speaker-diarization-community-1"
MIN_SPEAKER_OVERLAP_RATIO = 0.35
MERGE_GAP_MS = 1200
MAX_MERGED_TURN_MS = 45_000
MAX_MERGED_TURN_CHARS = 500


@dataclass(frozen=True)
class SpeakerInterval:
    speaker: str
    started_at_ms: int
    ended_at_ms: int
    confidence: float | None = None


@dataclass(frozen=True)
class DiarizationResult:
    status: DiarizationStatus
    intervals: list[SpeakerInterval]
    model_id: str | None
    error: str | None = None


class DiarizationState:
    def __init__(self, *, enabled: bool = True, model_id: str = DEFAULT_DIARIZATION_MODEL) -> None:
        self.enabled = enabled
        self.model_id = model_id
        self._pipeline: Any | None = None
        self._error: str | None = None

    def status_response(self) -> dict[str, object]:
        if not self.enabled:
            return {"status": DiarizationStatus.DISABLED, "model_id": self.model_id, "error": None}
        if self._pipeline is not None:
            return {"status": DiarizationStatus.AVAILABLE, "model_id": self.model_id, "error": None}

        availability_error = self._availability_error()
        if availability_error is not None:
            return {"status": DiarizationStatus.UNAVAILABLE, "model_id": self.model_id, "error": availability_error}

        if self._error is not None:
            return {"status": DiarizationStatus.FAILED, "model_id": self.model_id, "error": self._error}

        return {"status": DiarizationStatus.AVAILABLE, "model_id": self.model_id, "error": None}

    def diarize_wav(self, wav_path: str) -> DiarizationResult:
        if not self.enabled:
            return DiarizationResult(DiarizationStatus.DISABLED, [], self.model_id)

        availability_error = self._availability_error()
        if availability_error is not None:
            return DiarizationResult(DiarizationStatus.UNAVAILABLE, [], self.model_id, availability_error)

        try:
            pipeline = self._load_pipeline()
            output = pipeline(wav_path)
            intervals = list(iter_pyannote_intervals(output))
            return DiarizationResult(DiarizationStatus.AVAILABLE, intervals, self.model_id)
        except Exception as exc:
            self._error = str(exc)
            return DiarizationResult(DiarizationStatus.FAILED, [], self.model_id, str(exc))

    def _availability_error(self) -> str | None:
        if not pyannote_available():
            return "pyannote.audio is not installed"
        if not huggingface_token():
            return "Hugging Face token is not configured"
        return None

    def _load_pipeline(self) -> Any:
        if self._pipeline is not None:
            return self._pipeline

        from pyannote.audio import Pipeline

        self._pipeline = Pipeline.from_pretrained(self.model_id, token=huggingface_token())
        self._error = None
        return self._pipeline


def pyannote_available() -> bool:
    try:
        return importlib.util.find_spec("pyannote.audio") is not None
    except ModuleNotFoundError:
        return False


def huggingface_token() -> str:
    return (
        os.environ.get("HUGGINGFACE_HUB_TOKEN")
        or os.environ.get("HF_TOKEN")
        or os.environ.get("PYANNOTE_AUTH_TOKEN")
        or ""
    ).strip()


def iter_pyannote_intervals(output: Any) -> Iterable[SpeakerInterval]:
    annotation = pyannote_annotation(output)
    itertracks = getattr(annotation, "itertracks", None)
    if not callable(itertracks):
        return []

    intervals: list[SpeakerInterval] = []
    for item in itertracks(yield_label=True):
        if len(item) != 3:
            continue
        segment, _, speaker = item
        start = getattr(segment, "start", None)
        end = getattr(segment, "end", None)
        if not isinstance(start, (int, float)) or not isinstance(end, (int, float)) or end <= start:
            continue
        intervals.append(
            SpeakerInterval(
                speaker=str(speaker),
                started_at_ms=max(0, round(start * 1000)),
                ended_at_ms=max(0, round(end * 1000)),
            )
        )
    return intervals


def pyannote_annotation(output: Any) -> Any:
    if callable(getattr(output, "itertracks", None)):
        return output
    for attribute in ("exclusive_speaker_diarization", "speaker_diarization"):
        annotation = getattr(output, attribute, None)
        if callable(getattr(annotation, "itertracks", None)):
            return annotation
    return output


def load_segments_json(raw_segments: str) -> list[TranscriptTurn]:
    import json

    try:
        value = json.loads(raw_segments)
    except json.JSONDecodeError as exc:
        raise ValueError(f"segments_json is invalid JSON: {exc}") from exc

    if not isinstance(value, list):
        raise ValueError("segments_json must be a JSON array")

    turns: list[TranscriptTurn] = []
    for segment_index, segment in enumerate(value):
        if not isinstance(segment, dict):
            raise ValueError("segments_json entries must be objects")
        segment_turns = segment.get("turns")
        if isinstance(segment_turns, list) and segment_turns:
            for turn_index, turn in enumerate(segment_turns):
                turns.append(parse_turn(turn, f"segment-{segment_index}-turn-{turn_index}"))
            continue

        turns.append(
            TranscriptTurn(
                id=str(segment.get("chunk_id") or f"segment-{segment_index}"),
                text=sanitize_transcript_text(str(segment.get("text") or "")),
                speaker=None,
                started_at_ms=parse_int(segment.get("started_at_ms"), "started_at_ms"),
                ended_at_ms=parse_int(segment.get("ended_at_ms"), "ended_at_ms"),
                confidence=None,
            )
        )

    return [turn for turn in turns if turn.text and turn.ended_at_ms > turn.started_at_ms]


def parse_turn(value: object, fallback_id: str) -> TranscriptTurn:
    if not isinstance(value, dict):
        raise ValueError("turn entries must be objects")

    speaker = value.get("speaker")
    confidence = value.get("confidence")
    return TranscriptTurn(
        id=str(value.get("id") or fallback_id),
        text=sanitize_transcript_text(str(value.get("text") or "")),
        speaker=str(speaker) if speaker is not None and str(speaker).strip() else None,
        started_at_ms=parse_int(value.get("started_at_ms"), "started_at_ms"),
        ended_at_ms=parse_int(value.get("ended_at_ms"), "ended_at_ms"),
        confidence=float(confidence) if isinstance(confidence, (int, float)) else None,
    )


def parse_int(value: object, field: str) -> int:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{field} must be a number")
    return int(value)


def assign_speakers_to_turns(
    turns: list[TranscriptTurn],
    intervals: list[SpeakerInterval],
    *,
    min_overlap_ratio: float = MIN_SPEAKER_OVERLAP_RATIO,
) -> tuple[list[TranscriptTurn], list[TranscriptSpeaker]]:
    if not turns:
        return [], []
    if not intervals:
        return turns, []

    labeler = SpeakerLabeler()
    assigned: list[TranscriptTurn] = []
    totals: dict[str, int] = {}

    for turn in turns:
        raw_speaker, ratio = best_speaker_for_turn(turn, intervals)
        speaker = labeler.label(raw_speaker) if raw_speaker is not None and ratio >= min_overlap_ratio else None
        if speaker is not None:
            totals[speaker] = totals.get(speaker, 0) + max(0, turn.ended_at_ms - turn.started_at_ms)
        assigned.append(
            TranscriptTurn(
                id=turn.id,
                text=turn.text,
                speaker=speaker,
                started_at_ms=turn.started_at_ms,
                ended_at_ms=turn.ended_at_ms,
                confidence=ratio if speaker is not None else turn.confidence,
            )
        )

    speakers = [
        TranscriptSpeaker(id=labeler.raw_id(label), label=label, total_ms=totals.get(label, 0))
        for label in labeler.labels()
    ]
    return assigned, speakers


def best_speaker_for_turn(turn: TranscriptTurn, intervals: list[SpeakerInterval]) -> tuple[str | None, float]:
    duration = max(1, turn.ended_at_ms - turn.started_at_ms)
    overlaps: dict[str, int] = {}

    for interval in intervals:
        overlap = interval_overlap_ms(turn.started_at_ms, turn.ended_at_ms, interval.started_at_ms, interval.ended_at_ms)
        if overlap <= 0:
            continue
        overlaps[interval.speaker] = overlaps.get(interval.speaker, 0) + overlap

    if not overlaps:
        return None, 0.0

    speaker, overlap = max(overlaps.items(), key=lambda item: item[1])
    return speaker, min(1.0, overlap / duration)


def interval_overlap_ms(start_a: int, end_a: int, start_b: int, end_b: int) -> int:
    return max(0, min(end_a, end_b) - max(start_a, start_b))


class SpeakerLabeler:
    def __init__(self) -> None:
        self._raw_to_label: dict[str, str] = {}
        self._label_to_raw: dict[str, str] = {}

    def label(self, raw_speaker: str) -> str:
        if raw_speaker not in self._raw_to_label:
            label = f"Speaker {len(self._raw_to_label) + 1}"
            self._raw_to_label[raw_speaker] = label
            self._label_to_raw[label] = f"speaker_{len(self._raw_to_label)}"
        return self._raw_to_label[raw_speaker]

    def labels(self) -> list[str]:
        return list(self._label_to_raw.keys())

    def raw_id(self, label: str) -> str:
        return self._label_to_raw[label]


def merge_adjacent_turns(
    turns: list[TranscriptTurn],
    *,
    merge_gap_ms: int = MERGE_GAP_MS,
    max_merged_turn_ms: int = MAX_MERGED_TURN_MS,
    max_merged_turn_chars: int = MAX_MERGED_TURN_CHARS,
) -> list[TranscriptTurn]:
    merged: list[TranscriptTurn] = []

    for turn in turns:
        if not turn.text:
            continue
        if not merged or not can_merge_turns(
            merged[-1],
            turn,
            merge_gap_ms=merge_gap_ms,
            max_merged_turn_ms=max_merged_turn_ms,
            max_merged_turn_chars=max_merged_turn_chars,
        ):
            merged.append(turn)
            continue

        previous = merged[-1]
        merged[-1] = TranscriptTurn(
            id=previous.id,
            text=join_turn_text(previous.text, turn.text),
            speaker=previous.speaker,
            started_at_ms=previous.started_at_ms,
            ended_at_ms=turn.ended_at_ms,
            confidence=average_confidence(previous.confidence, turn.confidence),
        )

    return merged


def can_merge_turns(
    left: TranscriptTurn,
    right: TranscriptTurn,
    *,
    merge_gap_ms: int,
    max_merged_turn_ms: int,
    max_merged_turn_chars: int,
) -> bool:
    if left.speaker != right.speaker:
        return False
    if left.speaker is None:
        return False
    if right.started_at_ms - left.ended_at_ms > merge_gap_ms:
        return False
    if right.ended_at_ms - left.started_at_ms > max_merged_turn_ms:
        return False
    if len(join_turn_text(left.text, right.text)) > max_merged_turn_chars:
        return False
    return True


def join_turn_text(left: str, right: str) -> str:
    if not left:
        return right
    if not right:
        return left
    if left[-1].isascii() and right[0].isascii():
        return f"{left} {right}"
    return f"{left}{right}"


def average_confidence(left: float | None, right: float | None) -> float | None:
    values = [value for value in [left, right] if value is not None]
    if not values:
        return None
    return sum(values) / len(values)


def asdict_list(turns: list[TranscriptTurn]) -> list[dict[str, object]]:
    return [asdict(turn) for turn in turns]


def write_temp_wav(tmp_dir: str, meeting_id: str, wav_bytes: bytes) -> str:
    safe_meeting_id = "".join(ch if ch.isalnum() or ch in {"-", "_"} else "-" for ch in meeting_id).strip("-")
    path = Path(tmp_dir) / f"{safe_meeting_id or 'meeting'}.wav"
    path.write_bytes(wav_bytes)
    return str(path)
