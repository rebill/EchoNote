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
        model_path = Path(model_id).expanduser()
        if not model_path.is_dir():
            raise RuntimeError(
                f"Offline ASR model directory was not found: {model_path}. "
                "Install an EchoNote offline bundle before loading the MLX backend."
            )
        if not model_path.joinpath("config.json").is_file():
            raise RuntimeError(f"Offline ASR model is missing config.json: {model_path}")
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
                        "Install or repair EchoNote from a verified offline bundle."
                    ) from fallback_exc

        resolved_model_path = str(model_path.resolve())
        self.model = load(resolved_model_path)
        self.model_id = resolved_model_path

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


class FasterWhisperTranscriber:
    def __init__(self, *, cpu_threads: int = 0) -> None:
        if cpu_threads < 0:
            raise ValueError("cpu_threads must be zero or a positive integer")
        self.cpu_threads = cpu_threads
        self.model_id: str | None = None
        self.model: object | None = None

    def load(self, model_id: str) -> None:
        model_path = Path(model_id).expanduser()
        if not model_path.is_dir():
            raise RuntimeError(
                f"Offline faster-whisper model directory was not found: {model_path}. "
                "Install a Windows CPU model bundle before starting EchoNote ASR."
            )
        for required_file in ("config.json", "model.bin"):
            if not model_path.joinpath(required_file).is_file():
                raise RuntimeError(
                    f"Offline faster-whisper model is missing {required_file}: {model_path}"
                )

        try:
            from faster_whisper import WhisperModel
        except ImportError as exc:
            raise RuntimeError(
                "faster-whisper is not installed. Install EchoNote with the "
                "`windows-cpu` optional dependency."
            ) from exc

        resolved_model_path = str(model_path.resolve())
        model_options: dict[str, object] = {
            "device": "cpu",
            "compute_type": "int8",
            "num_workers": 1,
            "local_files_only": True,
        }
        if self.cpu_threads > 0:
            model_options["cpu_threads"] = self.cpu_threads

        self.model = WhisperModel(resolved_model_path, **model_options)
        self.model_id = resolved_model_path

    def warmup(self, *, language: str = "zh") -> None:
        if self.model is None:
            raise RuntimeError("ASR model is not loaded")

        with tempfile.TemporaryDirectory(prefix="echonote-asr-warmup-") as temp_dir:
            warmup_path = Path(temp_dir) / "warmup.wav"
            write_silence_wav(warmup_path)
            self._transcribe_wav(str(warmup_path), language=language, beam_size=1)

    def transcribe_wav(self, wav_path: str, *, language: str = "auto") -> str:
        return self._transcribe_wav(wav_path, language=language, beam_size=5)

    def _transcribe_wav(self, wav_path: str, *, language: str, beam_size: int) -> str:
        if self.model is None:
            raise RuntimeError("ASR model is not loaded")

        transcribe = getattr(self.model, "transcribe", None)
        if not callable(transcribe):
            raise RuntimeError("faster-whisper model does not expose transcribe")

        segments, _ = transcribe(
            wav_path,
            language=normalize_faster_whisper_language(language),
            beam_size=beam_size,
        )
        return "".join(str(getattr(segment, "text", "")) for segment in segments).strip()


def normalize_language_hint(language: str) -> str | None:
    normalized = language.strip().lower()
    if normalized in {"", "auto"}:
        return None
    return LANGUAGE_HINTS.get(normalized, language.strip())


def normalize_faster_whisper_language(language: str) -> str | None:
    normalized = language.strip().lower()
    if normalized in {"", "auto"}:
        return None
    return normalized


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


def create_transcriber(backend: str, *, cpu_threads: int = 0) -> Transcriber:
    if backend == "fake":
        return FakeTranscriber()
    if backend == "mlx-audio":
        return MlxAudioTranscriber()
    if backend == "faster-whisper":
        return FasterWhisperTranscriber(cpu_threads=cpu_threads)
    raise ValueError(f"unsupported ASR backend: {backend}")
