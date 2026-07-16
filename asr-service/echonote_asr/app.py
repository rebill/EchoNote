from __future__ import annotations

import asyncio
import logging
import tempfile
import time
from collections.abc import Callable
from dataclasses import asdict
from typing import Annotated, Literal, TypeVar

from fastapi import Body, FastAPI, File, Form, HTTPException, UploadFile, status
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse

from .model import ModelState
from .diarization import (
    DiarizationState,
    assign_speakers_to_turns,
    diarization_state_from_environment,
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
ThreadResult = TypeVar("ThreadResult")


def create_app(
    default_model: str,
    version: str = "0.8.1",
    backend: str = "fake",
    diarization_state: DiarizationState | None = None,
    preload_model: bool | None = None,
) -> FastAPI:
    app = FastAPI(title="EchoNote ASR Service", version=version)
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_methods=["*"],
        allow_headers=["*"],
    )
    model_state = ModelState(default_model, backend=backend)
    active_diarization_state = diarization_state or diarization_state_from_environment()
    diarization_gate = asyncio.Semaphore(1)
    app.state.model_state = model_state
    app.state.diarization_state = active_diarization_state
    app.state.diarization_gate = diarization_gate
    app.state.model_preload_task = None

    should_preload_model = backend == "mlx-audio" if preload_model is None else preload_model

    async def start_model_preload() -> None:
        if should_preload_model:
            app.state.model_preload_task = asyncio.create_task(model_state.load())

    async def close_model_state() -> None:
        preload_task = app.state.model_preload_task
        if preload_task is not None:
            await preload_task
        await model_state.close()

    app.router.add_event_handler("startup", start_model_preload)
    app.router.add_event_handler("shutdown", close_model_state)

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
    ) -> JSONResponse:
        request_started_at = time.perf_counter()
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
            transcription = await model_state.transcribe_wav_bytes(wav_bytes, chunk_id=chunk_id, language=language)
        except Exception as exc:
            raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail=str(exc)) from exc

        text = sanitize_transcript_text(
            transcription.text if model_state.backend != "fake" else f"fake transcript for chunk {chunk_id}"
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
        serialization_started_at = time.perf_counter()
        response = JSONResponse(content=asdict(segment))
        response_serialize_ms = (time.perf_counter() - serialization_started_at) * 1_000
        logger.info(
            "transcribe_completed",
            extra={
                "_chunk_id": chunk_id,
                "_lock_wait_ms": transcription.lock_wait_ms,
                "_temp_write_ms": transcription.temp_write_ms,
                "_inference_ms": transcription.inference_ms,
                "_cleanup_ms": transcription.cleanup_ms,
                "_response_serialize_ms": round(response_serialize_ms, 3),
                "_request_total_ms": round((time.perf_counter() - request_started_at) * 1_000, 3),
            },
        )
        return response

    @app.post("/transcript/finalize")
    async def finalize_transcript(
        audio: Annotated[UploadFile, File()],
        meeting_id: Annotated[str, Form()],
        segments_json: Annotated[str, Form()],
        language: Annotated[Literal["auto", "zh", "en"], Form()] = "auto",
        enable_diarization: Annotated[bool, Form()] = True,
    ) -> dict[str, object]:
        finalize_started_at = time.perf_counter()
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
        diarization_queue_wait_ms = 0.0
        temp_write_ms = 0.0
        diarization_ms = 0.0
        temp_cleanup_ms = 0.0
        assignment_ms = 0.0
        merge_ms = 0.0

        if enable_diarization:
            gate_wait_started_at = time.perf_counter()
            async with diarization_gate:
                diarization_queue_wait_ms = (time.perf_counter() - gate_wait_started_at) * 1_000
                cleanup_started_at = 0.0
                with tempfile.TemporaryDirectory(prefix="echonote-finalize-") as tmp_dir:
                    write_started_at = time.perf_counter()
                    wav_path = write_temp_wav(tmp_dir, meeting_id, wav_bytes)
                    temp_write_ms = (time.perf_counter() - write_started_at) * 1_000
                    diarization_started_at = time.perf_counter()
                    diarization = await run_thread_to_completion(
                        active_diarization_state.diarize_wav,
                        wav_path,
                    )
                    diarization_ms = (time.perf_counter() - diarization_started_at) * 1_000
                    cleanup_started_at = time.perf_counter()
                temp_cleanup_ms = (time.perf_counter() - cleanup_started_at) * 1_000
            diarization_model_id = diarization.model_id
            diarization_status = diarization.status
            diarization_error = diarization.error
            if diarization.status == DiarizationStatus.AVAILABLE:
                assignment_started_at = time.perf_counter()
                assigned_turns, speakers = assign_speakers_to_turns(live_turns, diarization.intervals)
                assignment_ms = (time.perf_counter() - assignment_started_at) * 1_000
                merge_started_at = time.perf_counter()
                final_turns = merge_adjacent_turns(assigned_turns)
                merge_ms = (time.perf_counter() - merge_started_at) * 1_000
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
        payload = asdict(response)
        logger.info(
            "finalize_completed",
            extra={
                "_meeting_id": meeting_id,
                "_diarization_queue_wait_ms": round(diarization_queue_wait_ms, 3),
                "_temp_write_ms": round(temp_write_ms, 3),
                "_diarization_ms": round(diarization_ms, 3),
                "_temp_cleanup_ms": round(temp_cleanup_ms, 3),
                "_assignment_ms": round(assignment_ms, 3),
                "_merge_ms": round(merge_ms, 3),
                "_total_ms": round((time.perf_counter() - finalize_started_at) * 1_000, 3),
            },
        )
        return payload

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


async def run_thread_to_completion(
    function: Callable[..., ThreadResult],
    *args: object,
) -> ThreadResult:
    task = asyncio.create_task(asyncio.to_thread(function, *args))
    try:
        return await asyncio.shield(task)
    except asyncio.CancelledError:
        try:
            await task
        except Exception:
            pass
        raise
