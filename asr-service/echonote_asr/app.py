from __future__ import annotations

import asyncio
import logging
from dataclasses import asdict
from typing import Annotated, Literal

from fastapi import Body, FastAPI, File, Form, HTTPException, UploadFile, status
from fastapi.middleware.cors import CORSMiddleware

from .model import ModelState
from .schemas import (
    HealthResponse,
    ModelLifecycleStatus,
    ModelLoadRequest,
    ShutdownResponse,
    TranscriptSegment,
)
from .text_sanitizer import sanitize_transcript_text
from .wav import read_valid_wav

logger = logging.getLogger(__name__)


def create_app(default_model: str, version: str = "0.1.0", backend: str = "fake") -> FastAPI:
    app = FastAPI(title="EchoNote ASR Service", version=version)
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_methods=["*"],
        allow_headers=["*"],
    )
    model_state = ModelState(default_model, backend=backend)
    app.state.model_state = model_state

    @app.get("/health")
    async def health() -> dict[str, object]:
        return asdict(HealthResponse(status="ok", service="echonote-asr", version=version))

    @app.get("/model/status")
    async def model_status() -> dict[str, object]:
        return await model_state.status_response()

    @app.post("/model/load")
    async def model_load(request: Annotated[ModelLoadRequest, Body()]) -> dict[str, object]:
        return await model_state.load(request.model_id)

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

        segment = TranscriptSegment(
            chunk_id=chunk_id,
            text=sanitize_transcript_text(
                transcript_text if model_state.backend != "fake" else f"fake transcript for chunk {chunk_id}"
            ),
            started_at_ms=started_at_ms,
            ended_at_ms=ended_at_ms,
            language=None if language == "auto" else language,
            model_id=model_state.model_id,
        )
        return asdict(segment)

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
