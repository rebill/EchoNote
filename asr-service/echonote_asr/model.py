from __future__ import annotations

import asyncio
import logging
from dataclasses import asdict
import tempfile
from pathlib import Path

from .schemas import ModelLifecycleStatus, ModelLoadResponse, ModelStatusResponse
from .transcriber import Transcriber, create_transcriber

logger = logging.getLogger(__name__)


class ModelState:
    def __init__(self, model_id: str, backend: str = "fake") -> None:
        self._model_id = model_id
        self._backend = backend
        self._status = ModelLifecycleStatus.NOT_LOADED
        self._error: str | None = None
        self._lock = asyncio.Lock()
        self._transcriber: Transcriber = create_transcriber(backend)

    @property
    def model_id(self) -> str:
        return self._model_id

    @property
    def backend(self) -> str:
        return self._backend

    async def status_response(self) -> dict[str, object]:
        async with self._lock:
            return asdict(
                ModelStatusResponse(
                    model_id=self._model_id,
                    status=self._status,
                    error=self._error,
                )
            )

    async def load(self, model_id: str | None = None) -> dict[str, object]:
        async with self._lock:
            requested_model_id = model_id or self._model_id
            if self._status == ModelLifecycleStatus.LOADING:
                return asdict(ModelLoadResponse(model_id=self._model_id, status=self._status))
            if self._status == ModelLifecycleStatus.READY and requested_model_id == self._model_id:
                return asdict(ModelLoadResponse(model_id=self._model_id, status=self._status))

            self._model_id = requested_model_id
            self._status = ModelLifecycleStatus.LOADING
            self._error = None
            active_model_id = self._model_id
            logger.info(
                "model_load_started",
                extra={"_model_id": active_model_id, "_status": self._status.value},
            )

        try:
            await asyncio.to_thread(self._transcriber.load, active_model_id)
            if self._backend == "fake":
                await asyncio.sleep(0.5)
        except Exception as exc:
            async with self._lock:
                self._status = ModelLifecycleStatus.ERROR
                self._error = str(exc)
                logger.exception(
                    "model_load_failed",
                    extra={"_model_id": self._model_id, "_backend": self._backend},
                )
                return asdict(ModelLoadResponse(model_id=self._model_id, status=self._status))

        async with self._lock:
            self._status = ModelLifecycleStatus.READY
            logger.info(
                "model_load_completed",
                extra={"_model_id": self._model_id, "_status": self._status.value},
            )
            return asdict(ModelLoadResponse(model_id=self._model_id, status=self._status))

    async def transcribe_wav_bytes(self, wav_bytes: bytes, *, chunk_id: str, language: str = "auto") -> str:
        async with self._lock:
            if self._status != ModelLifecycleStatus.READY:
                raise RuntimeError(f"model is {self._status}")

        with tempfile.TemporaryDirectory(prefix="echonote-asr-chunk-") as tmp_dir:
            wav_path = Path(tmp_dir) / f"{chunk_id}.wav"
            wav_path.write_bytes(wav_bytes)
            return await asyncio.to_thread(
                self._transcriber.transcribe_wav,
                str(wav_path),
                language=language,
            )
