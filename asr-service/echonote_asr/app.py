from __future__ import annotations

import asyncio
import tempfile
import logging
import os
from dataclasses import asdict
from typing import Annotated, Literal

from fastapi import Body, FastAPI, File, Form, HTTPException, UploadFile, status
from fastapi.middleware.cors import CORSMiddleware

from .model import ModelState
from .diarization import (
    DiarizationState,
    assign_speakers_to_turns,
    load_segments_json,
    merge_adjacent_turns,
    write_temp_wav,
)
from .schemas import (
    DiarizationStatus,
    FinalizeTranscriptResponse,
    HealthResponse,
    ModelLifecycleStatus,
    ModelLoadRequest,
    ShutdownResponse,
    TranscriptSegment,
    TranscriptTurn,
)
from .text_sanitizer import sanitize_transcript_text
from .wav import read_valid_wav

logger = logging.getLogger(__name__)


def create_app(
    default_model: str,
    version: str = "0.4.0",
    backend: str = "fake",
    diarization_state: DiarizationState | None = None,
) -> FastAPI:
    app = FastAPI(title="EchoNote ASR Service", version=version)
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_methods=["*"],
        allow_headers=["*"],
    )
    model_state = ModelState(default_model, backend=backend)
    active_diarization_state = diarization_state or DiarizationState(
        enabled=os.environ.get("ECHONOTE_DIARIZATION_ENABLED", "1") != "0",
        model_id=os.environ.get("ECHONOTE_DIARIZATION_MODEL_ID", "").strip()
        or "pyannote/speaker-diarization-community-1",
    )
    app.state.model_state = model_state
    app.state.diarization_state = active_diarization_state

    @app.get("/health")
    async def health() -> dict[str, object]:
        return asdict(HealthResponse(status="ok", service="echonote-asr", version=version))

    @app.get("/model/status")
    async def model_status() -> dict[str, object]:
        return await model_state.status_response()

    @app.post("/model/load")
    async def model_load(request: Annotated[ModelLoadRequest, Body()]) -> dict[str, object]:
        return await model_state.load(request.model_id)

    @app.get("/diarization/status")
    async def diarization_status() -> dict[str, object]:
        return active_diarization_state.status_response()

    @app.post("/transcribe")
    async def transcribe(
        audio: Annotated[UploadFile, File()],
        chunk_id: Annotated[str, Form()],
        started_at_ms: Annotated[int, Form()],
        ended_at_ms: Annotated[int, Form()],
        language: Annotated[Literal["auto", "zh", "en"], Form()] = "auto",
    ) -> dict[str, object]:
        current_status = await model_state.status_response()
        if current_status["status"] != ModelLifecycleStatus.READY:
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail=f"model is {current_status['status']}",
            )
        if not chunk_id.strip():
            raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="chunk_id is required")
        if started_at_ms < 0 or ended_at_ms <= started_at_ms:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="started_at_ms and ended_at_ms must define a positive range",
            )

        wav_bytes = await read_valid_wav(audio)
        logger.info(
            "transcribe_requested",
            extra={
                "_chunk_id": chunk_id,
                "_started_at_ms": started_at_ms,
                "_ended_at_ms": ended_at_ms,
                "_language": language,
                "_bytes": len(wav_bytes),
                "_model_id": model_state.model_id,
            },
        )
        try:
            transcript_text = await model_state.transcribe_wav_bytes(wav_bytes, chunk_id=chunk_id, language=language)
        except Exception as exc:
            raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail=str(exc)) from exc

        text = sanitize_transcript_text(
            transcript_text if model_state.backend != "fake" else f"fake transcript for chunk {chunk_id}"
        )
        turns = [
            TranscriptTurn(
                id=f"{chunk_id}-turn-001",
                text=text,
                speaker=None,
                started_at_ms=started_at_ms,
                ended_at_ms=ended_at_ms,
                confidence=None,
            )
        ] if text else []
        segment = TranscriptSegment(
            chunk_id=chunk_id,
            text=text,
            turns=turns,
            started_at_ms=started_at_ms,
            ended_at_ms=ended_at_ms,
            language=None if language == "auto" else language,
            model_id=model_state.model_id,
        )
        return asdict(segment)

    @app.post("/transcript/finalize")
    async def finalize_transcript(
        audio: Annotated[UploadFile, File()],
        meeting_id: Annotated[str, Form()],
        segments_json: Annotated[str, Form()],
        language: Annotated[Literal["auto", "zh", "en"], Form()] = "auto",
        enable_diarization: Annotated[bool, Form()] = True,
    ) -> dict[str, object]:
        if not meeting_id.strip():
            raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="meeting_id is required")

        try:
            live_turns = load_segments_json(segments_json)
        except ValueError as exc:
            raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail=str(exc)) from exc

        wav_bytes = await read_valid_wav(audio)
        logger.info(
            "finalize_requested",
            extra={
                "_meeting_id": meeting_id,
                "_language": language,
                "_enable_diarization": enable_diarization,
                "_turn_count": len(live_turns),
                "_bytes": len(wav_bytes),
                "_model_id": model_state.model_id,
            },
        )

        diarization_model_id: str | None = None
        diarization_status = DiarizationStatus.DISABLED
        diarization_error: str | None = None
        speakers = []
        final_turns = live_turns

        if enable_diarization:
            with tempfile.TemporaryDirectory(prefix="echonote-finalize-") as tmp_dir:
                wav_path = write_temp_wav(tmp_dir, meeting_id, wav_bytes)
                diarization = await asyncio.to_thread(active_diarization_state.diarize_wav, wav_path)
            diarization_model_id = diarization.model_id
            diarization_status = diarization.status
            diarization_error = diarization.error
            if diarization.status == DiarizationStatus.AVAILABLE:
                assigned_turns, speakers = assign_speakers_to_turns(live_turns, diarization.intervals)
                final_turns = merge_adjacent_turns(assigned_turns)
            else:
                final_turns = live_turns

        response = FinalizeTranscriptResponse(
            meeting_id=meeting_id,
            turns=final_turns,
            speakers=speakers,
            model_id=model_state.model_id,
            diarization_model_id=diarization_model_id,
            diarization_status=diarization_status,
            error=diarization_error,
        )
        return asdict(response)

    @app.post("/shutdown")
    async def shutdown() -> dict[str, object]:
        logger.info("shutdown_requested")

        async def stop_server() -> None:
            await asyncio.sleep(0.1)
            server = getattr(app.state, "server", None)
            if server is not None:
                server.should_exit = True

        asyncio.create_task(stop_server())
        return asdict(ShutdownResponse(status="shutting_down"))

    return app
