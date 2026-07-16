from __future__ import annotations

import inspect
from pathlib import Path
import tempfile
from typing import Protocol
import wave


LANGUAGE_HINTS = {
    "zh": "Chinese",
    "en": "English",
}


class Transcriber(Protocol):
    def load(self, model_id: str) -> None:
        ...

    def warmup(self, *, language: str = "auto") -> None:
        ...

    def transcribe_wav(self, wav_path: str, *, language: str = "auto") -> str:
        ...


class FakeTranscriber:
    def __init__(self) -> None:
        self.model_id: str | None = None

    def load(self, model_id: str) -> None:
        self.model_id = model_id

    def warmup(self, *, language: str = "auto") -> None:
        return None

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

    def warmup(self, *, language: str = "zh") -> None:
        if self.model is None:
            raise RuntimeError("ASR model is not loaded")

        with tempfile.TemporaryDirectory(prefix="echonote-asr-warmup-") as temp_dir:
            warmup_path = Path(temp_dir) / "warmup.wav"
            write_silence_wav(warmup_path)
            generate = getattr(self.model, "generate", None)
            if callable(generate):
                self._call_model_generate(generate, str(warmup_path), language, max_tokens=8)
                return
            self.transcribe_wav(str(warmup_path), language=language)

    def transcribe_wav(self, wav_path: str, *, language: str = "auto") -> str:
        if self.model is None:
            raise RuntimeError("ASR model is not loaded")

        generated_text = self._transcribe_with_model_generate(wav_path, language)
        if generated_text:
            return generated_text

        return self._transcribe_with_generate_transcription(wav_path, language)

    def _transcribe_with_model_generate(self, wav_path: str, language: str) -> str:
        generate = getattr(self.model, "generate", None)
        if not callable(generate):
            return ""

        result = self._call_model_generate(generate, wav_path, language)
        text = getattr(result, "text", None)
        if isinstance(text, str):
            return text.strip()
        if isinstance(result, str):
            return result.strip()
        return ""

    def _transcribe_with_generate_transcription(self, wav_path: str, language: str) -> str:
        try:
            from mlx_audio.stt.generate import generate_transcription
        except ImportError as exc:
            raise RuntimeError("mlx-audio transcription module is unavailable") from exc

        output_file = Path(wav_path).with_suffix(".txt")
        output_path = str(output_file)
        try:
            transcription = self._call_generate_transcription(
                generate_transcription,
                wav_path,
                output_path,
                language,
            )

            text = getattr(transcription, "text", None)
            if isinstance(text, str):
                return text.strip()

            if output_file.exists():
                return output_file.read_text(encoding="utf-8").strip()
            return ""
        finally:
            output_file.unlink(missing_ok=True)

    def _call_model_generate(
        self,
        generate: object,
        wav_path: str,
        language: str,
        *,
        max_tokens: int | None = None,
    ) -> object:
        assert callable(generate)
        kwargs: dict[str, object] = {}
        language_hint = normalize_language_hint(language)
        if language_hint is not None and accepts_keyword(generate, "language"):
            kwargs["language"] = language_hint
        if max_tokens is not None and accepts_keyword(generate, "max_tokens"):
            kwargs["max_tokens"] = max_tokens
        return generate(wav_path, **kwargs)

    def _call_generate_transcription(
        self,
        generate_transcription: object,
        wav_path: str,
        output_path: str,
        language: str,
    ) -> object:
        assert callable(generate_transcription)
        kwargs: dict[str, object] = {}
        language_hint = normalize_language_hint(language)
        if language_hint is not None and accepts_keyword(generate_transcription, "language"):
            kwargs["language"] = language_hint
        try:
            return generate_transcription(
                model=self.model,
                audio=wav_path,
                output_path=output_path,
                format="txt",
                verbose=False,
                **kwargs,
            )
        except TypeError:
            return generate_transcription(
                model=self.model,
                audio_path=wav_path,
                output_path=output_path,
                format="txt",
                verbose=False,
                **kwargs,
            )


def normalize_language_hint(language: str) -> str | None:
    normalized = language.strip().lower()
    if normalized in {"", "auto"}:
        return None
    return LANGUAGE_HINTS.get(normalized, language.strip())


def accepts_keyword(function: object, keyword: str) -> bool:
    assert callable(function)
    try:
        parameters = inspect.signature(function).parameters.values()
    except (TypeError, ValueError):
        return False
    return any(
        parameter.name == keyword or parameter.kind == inspect.Parameter.VAR_KEYWORD
        for parameter in parameters
    )


def write_silence_wav(path: Path, *, duration_ms: int = 1000, sample_rate: int = 16000) -> None:
    frame_count = max(1, round(sample_rate * duration_ms / 1000))
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(sample_rate)
        output.writeframes(b"\x00\x00" * frame_count)


def create_transcriber(backend: str) -> Transcriber:
    if backend == "fake":
        return FakeTranscriber()
    if backend == "mlx-audio":
        return MlxAudioTranscriber()
    raise ValueError(f"unsupported ASR backend: {backend}")
