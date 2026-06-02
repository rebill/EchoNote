from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Literal


class ModelLifecycleStatus(StrEnum):
    NOT_LOADED = "not_loaded"
    LOADING = "loading"
    READY = "ready"
    ERROR = "error"


class DiarizationStatus(StrEnum):
    DISABLED = "disabled"
    AVAILABLE = "available"
    UNAVAILABLE = "unavailable"
    FAILED = "failed"


@dataclass(frozen=True)
class HealthResponse:
    status: Literal["ok"]
    service: Literal["echonote-asr"]
    version: str


@dataclass(frozen=True)
class ModelStatusResponse:
    model_id: str
    status: ModelLifecycleStatus
    error: str | None


@dataclass(frozen=True)
class ModelLoadRequest:
    model_id: str


@dataclass(frozen=True)
class ModelLoadResponse:
    model_id: str
    status: ModelLifecycleStatus


@dataclass(frozen=True)
class TranscribeRequestMetadata:
    chunk_id: str
    started_at_ms: int
    ended_at_ms: int
    language: Literal["auto", "zh", "en"] = "auto"


@dataclass(frozen=True)
class TranscriptTurn:
    id: str
    text: str
    speaker: str | None
    started_at_ms: int
    ended_at_ms: int
    confidence: float | None = None


@dataclass(frozen=True)
class TranscriptSegment:
    chunk_id: str
    text: str
    turns: list[TranscriptTurn]
    started_at_ms: int
    ended_at_ms: int
    language: str | None
    model_id: str


@dataclass(frozen=True)
class TranscriptSpeaker:
    id: str
    label: str
    total_ms: int


@dataclass(frozen=True)
class DiarizationStatusResponse:
    status: DiarizationStatus
    model_id: str | None
    error: str | None


@dataclass(frozen=True)
class FinalizeTranscriptResponse:
    meeting_id: str
    turns: list[TranscriptTurn]
    speakers: list[TranscriptSpeaker]
    model_id: str
    diarization_model_id: str | None
    diarization_status: DiarizationStatus
    error: str | None


@dataclass(frozen=True)
class ShutdownResponse:
    status: Literal["shutting_down"]
