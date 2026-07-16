from __future__ import annotations

import asyncio
import logging
from dataclasses import asdict, dataclass
import tempfile
from pathlib import Path
import time

from .schemas import ModelLifecycleStatus, ModelLoadResponse, ModelStatusResponse
from .transcriber import Transcriber, create_transcriber

logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class TranscriptionResult:
    text: str
    lock_wait_ms: float
    temp_write_ms: float
    inference_ms: float
    cleanup_ms: float
    total_ms: float


class ModelState:
    def __init__(
        self,
        model_id: str,
        backend: str = "fake",
        *,
        transcriber: Transcriber | None = None,
        temp_root: str | Path | None = None,
    ) -> None:
        self._model_id = model_id
        self._backend = backend
        self._status = ModelLifecycleStatus.NOT_LOADED
        self._error: str | None = None
        self._lock = asyncio.Lock()
        self._inference_lock = asyncio.Lock()
        self._transcriber: Transcriber = transcriber or create_transcriber(backend)
        self._workspace = tempfile.TemporaryDirectory(prefix="echonote-asr-", dir=temp_root)
        self._wav_path = Path(self._workspace.name) / "chunk.wav"
        self._closed = False

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

        load_started_at = time.perf_counter()
        load_ms = 0.0
        warmup_ms = 0.0
        warmed_up = False
        try:
            async with self._inference_lock:
                await run_thread_to_completion(self._transcriber.load, active_model_id)
                load_ms = elapsed_ms(load_started_at)
                warmup = getattr(self._transcriber, "warmup", None)
                if callable(warmup) and self._backend != "fake":
                    warmup_started_at = time.perf_counter()
                    try:
                        await run_thread_to_completion(warmup, language="zh")
                        warmed_up = True
                    except Exception:
                        logger.warning(
                            "model_warmup_failed",
                            exc_info=True,
                            extra={"_model_id": active_model_id, "_backend": self._backend},
                        )
                    finally:
                        warmup_ms = elapsed_ms(warmup_started_at)
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
                extra={
                    "_model_id": self._model_id,
                    "_status": self._status.value,
                    "_load_ms": round(load_ms, 3),
                    "_warmup_ms": round(warmup_ms, 3),
                    "_warmed_up": warmed_up,
                },
            )
            return asdict(ModelLoadResponse(model_id=self._model_id, status=self._status))

    async def transcribe_wav_bytes(
        self,
        wav_bytes: bytes,
        *,
        chunk_id: str,
        language: str = "auto",
    ) -> TranscriptionResult:
        total_started_at = time.perf_counter()
        lock_started_at = time.perf_counter()
        async with self._inference_lock:
            lock_wait_ms = elapsed_ms(lock_started_at)
            async with self._lock:
                if self._status != ModelLifecycleStatus.READY:
                    raise RuntimeError(f"model is {self._status}")
                if self._closed:
                    raise RuntimeError("model state is closed")

            text = ""
            temp_write_ms = 0.0
            inference_ms = 0.0
            cleanup_ms = 0.0
            failure: Exception | None = None
            try:
                write_started_at = time.perf_counter()
                await asyncio.to_thread(self._wav_path.write_bytes, wav_bytes)
                temp_write_ms = elapsed_ms(write_started_at)
                inference_started_at = time.perf_counter()
                text = await run_thread_to_completion(
                    self._transcriber.transcribe_wav,
                    str(self._wav_path),
                    language=language,
                )
                inference_ms = elapsed_ms(inference_started_at)
            except Exception as exc:
                failure = exc
                if temp_write_ms > 0:
                    inference_ms = inference_ms or elapsed_ms(inference_started_at)
            finally:
                cleanup_started_at = time.perf_counter()
                await asyncio.to_thread(self._wav_path.unlink, missing_ok=True)
                cleanup_ms = elapsed_ms(cleanup_started_at)

            if failure is not None:
                logger.error(
                    "transcribe_inference_failed",
                    extra={
                        "_chunk_id": chunk_id,
                        "_lock_wait_ms": round(lock_wait_ms, 3),
                        "_temp_write_ms": round(temp_write_ms, 3),
                        "_inference_ms": round(inference_ms, 3),
                        "_cleanup_ms": round(cleanup_ms, 3),
                        "_total_ms": round(elapsed_ms(total_started_at), 3),
                        "_error": str(failure),
                    },
                )
                raise failure

            result = TranscriptionResult(
                text=text,
                lock_wait_ms=round(lock_wait_ms, 3),
                temp_write_ms=round(temp_write_ms, 3),
                inference_ms=round(inference_ms, 3),
                cleanup_ms=round(cleanup_ms, 3),
                total_ms=round(elapsed_ms(total_started_at), 3),
            )
            logger.info(
                "transcribe_inference_completed",
                extra={
                    "_chunk_id": chunk_id,
                    "_lock_wait_ms": result.lock_wait_ms,
                    "_temp_write_ms": result.temp_write_ms,
                    "_inference_ms": result.inference_ms,
                    "_cleanup_ms": result.cleanup_ms,
                    "_total_ms": result.total_ms,
                },
            )
            return result

    async def close(self) -> None:
        async with self._inference_lock:
            if self._closed:
                return
            self._closed = True
            await asyncio.to_thread(self._workspace.cleanup)


async def run_thread_to_completion(function: object, *args: object, **kwargs: object) -> object:
    assert callable(function)
    task = asyncio.create_task(asyncio.to_thread(function, *args, **kwargs))
    try:
        return await asyncio.shield(task)
    except asyncio.CancelledError:
        try:
            await task
        except Exception:
            pass
        raise


def elapsed_ms(started_at: float) -> float:
    return (time.perf_counter() - started_at) * 1_000
