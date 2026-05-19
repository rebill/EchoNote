from __future__ import annotations

import tempfile
from pathlib import Path
from typing import Protocol


class Transcriber(Protocol):
    def load(self, model_id: str) -> None:
        ...

    def transcribe_wav(self, wav_path: str, *, language: str = "auto") -> str:
        ...


class FakeTranscriber:
    def __init__(self) -> None:
        self.model_id: str | None = None

    def load(self, model_id: str) -> None:
        self.model_id = model_id

    def transcribe_wav(self, wav_path: str, *, language: str = "auto") -> str:
        return f"fake transcript for {Path(wav_path).name}"


class MlxAudioTranscriber:
    def __init__(self) -> None:
        self.model_id: str | None = None
        self.model: object | None = None

    def load(self, model_id: str) -> None:
        try:
            from mlx_audio.stt import load
        except ImportError as exc:
            try:
                from mlx_audio.stt.utils import load
            except ImportError:
                try:
                    from mlx_audio.stt.utils import load_model as load
                except ImportError as fallback_exc:
                    raise RuntimeError(
                        "mlx-audio is not installed or does not expose an STT loader. "
                        "Install it with `pip install -U mlx-audio` or `pip install -e 'asr-service[mlx]'`."
                    ) from fallback_exc

        self.model = load(model_id)
        self.model_id = model_id

    def transcribe_wav(self, wav_path: str, *, language: str = "auto") -> str:
        if self.model is None:
            raise RuntimeError("ASR model is not loaded")

        generated_text = self._transcribe_with_model_generate(wav_path)
        if generated_text:
            return generated_text

        return self._transcribe_with_generate_transcription(wav_path)

    def _transcribe_with_model_generate(self, wav_path: str) -> str:
        generate = getattr(self.model, "generate", None)
        if not callable(generate):
            return ""

        result = generate(wav_path)
        text = getattr(result, "text", None)
        if isinstance(text, str):
            return text.strip()
        if isinstance(result, str):
            return result.strip()
        return ""

    def _transcribe_with_generate_transcription(self, wav_path: str) -> str:
        try:
            from mlx_audio.stt.generate import generate_transcription
        except ImportError as exc:
            raise RuntimeError("mlx-audio transcription module is unavailable") from exc

        with tempfile.TemporaryDirectory(prefix="echonote-asr-") as tmp_dir:
            output_path = str(Path(tmp_dir) / "transcript.txt")
            transcription = self._call_generate_transcription(generate_transcription, wav_path, output_path)

            text = getattr(transcription, "text", None)
            if isinstance(text, str):
                return text.strip()

            output_file = Path(output_path)
            if output_file.exists():
                return output_file.read_text(encoding="utf-8").strip()

        return ""

    def _call_generate_transcription(self, generate_transcription: object, wav_path: str, output_path: str) -> object:
        assert callable(generate_transcription)
        try:
            return generate_transcription(
                model=self.model,
                audio=wav_path,
                output_path=output_path,
                format="txt",
                verbose=False,
            )
        except TypeError:
            return generate_transcription(
                model=self.model,
                audio_path=wav_path,
                output_path=output_path,
                format="txt",
                verbose=False,
            )


def create_transcriber(backend: str) -> Transcriber:
    if backend == "fake":
        return FakeTranscriber()
    if backend == "mlx-audio":
        return MlxAudioTranscriber()
    raise ValueError(f"unsupported ASR backend: {backend}")
