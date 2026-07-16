from __future__ import annotations

import asyncio
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

from echonote_asr.model import ModelState


class TrackingTranscriber:
    def __init__(self) -> None:
        self.active = 0
        self.max_active = 0
        self.calls: list[str] = []
        self._lock = threading.Lock()

    def load(self, model_id: str) -> None:
        return None

    def transcribe_wav(self, wav_path: str, *, language: str = "auto") -> str:
        with self._lock:
            self.active += 1
            self.max_active = max(self.max_active, self.active)
        try:
            payload = Path(wav_path).read_bytes().decode("utf-8")
            self.calls.append(payload)
            time.sleep(0.02)
            return payload
        finally:
            with self._lock:
                self.active -= 1


class ModelStateTest(unittest.IsolatedAsyncioTestCase):
    async def test_transcription_is_serialized_and_reuses_a_clean_workspace(self) -> None:
        transcriber = TrackingTranscriber()
        with tempfile.TemporaryDirectory() as temp_root:
            state = ModelState(
                "test-model",
                backend="injected",
                transcriber=transcriber,
                temp_root=temp_root,
            )
            await state.load()

            results = await asyncio.gather(*(
                state.transcribe_wav_bytes(value.encode(), chunk_id=value)
                for value in ("one", "two", "three")
            ))

            self.assertEqual([result.text for result in results], ["one", "two", "three"])
            self.assertEqual(transcriber.calls, ["one", "two", "three"])
            self.assertEqual(transcriber.max_active, 1)
            workspaces = list(Path(temp_root).iterdir())
            self.assertEqual(len(workspaces), 1)
            self.assertEqual(list(workspaces[0].iterdir()), [])
            self.assertGreater(results[1].lock_wait_ms, 0)

            workspace = workspaces[0]
            await state.close()
            self.assertFalse(workspace.exists())

    async def test_failed_inference_cleans_input_and_does_not_block_next_request(self) -> None:
        transcriber = FailingOnceTranscriber()
        with tempfile.TemporaryDirectory() as temp_root:
            state = ModelState(
                "test-model",
                backend="injected",
                transcriber=transcriber,
                temp_root=temp_root,
            )
            await state.load()

            with self.assertRaisesRegex(RuntimeError, "inference failed"):
                await state.transcribe_wav_bytes(b"first", chunk_id="first")
            result = await state.transcribe_wav_bytes(b"second", chunk_id="second")

            self.assertEqual(result.text, "second")
            workspace = next(Path(temp_root).iterdir())
            self.assertEqual(list(workspace.iterdir()), [])
            await state.close()

    async def test_failed_temp_write_still_leaves_workspace_clean(self) -> None:
        with tempfile.TemporaryDirectory() as temp_root:
            state = ModelState(
                "test-model",
                backend="injected",
                transcriber=TrackingTranscriber(),
                temp_root=temp_root,
            )
            await state.load()

            with self.assertLogs("echonote_asr.model", level="ERROR"):
                with patch.object(Path, "write_bytes", side_effect=OSError("disk full")):
                    with self.assertRaisesRegex(OSError, "disk full"):
                        await state.transcribe_wav_bytes(b"input", chunk_id="write-failure")

            workspace = next(Path(temp_root).iterdir())
            self.assertEqual(list(workspace.iterdir()), [])
            await state.close()


class FailingOnceTranscriber(TrackingTranscriber):
    def __init__(self) -> None:
        super().__init__()
        self.should_fail = True

    def transcribe_wav(self, wav_path: str, *, language: str = "auto") -> str:
        if self.should_fail:
            self.should_fail = False
            raise RuntimeError("inference failed")
        return super().transcribe_wav(wav_path, language=language)


if __name__ == "__main__":
    unittest.main()
