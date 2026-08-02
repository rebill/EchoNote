from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

from echonote_asr.transcriber import (
    FasterWhisperTranscriber,
    MlxAudioTranscriber,
    create_transcriber,
    normalize_faster_whisper_language,
    normalize_language_hint,
)


class MlxAudioTranscriberTest(unittest.TestCase):
    def test_load_requires_an_existing_local_model_directory(self) -> None:
        transcriber = MlxAudioTranscriber()

        with self.assertRaisesRegex(RuntimeError, "Offline ASR model directory was not found"):
            transcriber.load("mlx-community/Qwen3-ASR-0.6B-4bit")

    def test_load_passes_an_absolute_local_path_to_mlx_audio(self) -> None:
        loaded_paths: list[str] = []
        with tempfile.TemporaryDirectory() as model_dir:
            Path(model_dir, "config.json").write_text("{}", encoding="utf-8")
            with patch.dict("sys.modules", {
                "mlx_audio": SimpleNamespace(),
                "mlx_audio.stt": SimpleNamespace(load=lambda path: loaded_paths.append(path) or object()),
            }):
                transcriber = MlxAudioTranscriber()
                transcriber.load(model_dir)
            expected_path = str(Path(model_dir).resolve())

        self.assertEqual(loaded_paths, [expected_path])

    def test_load_rejects_an_incomplete_local_model_directory(self) -> None:
        with tempfile.TemporaryDirectory() as model_dir:
            transcriber = MlxAudioTranscriber()

            with self.assertRaisesRegex(RuntimeError, "missing config.json"):
                transcriber.load(model_dir)

    def test_maps_api_language_codes_to_qwen_language_names(self) -> None:
        model = RecordingModel()
        transcriber = loaded_transcriber(model)

        text = transcriber.transcribe_wav("meeting.wav", language="zh")

        self.assertEqual(text, "transcript")
        self.assertEqual(model.calls, [("meeting.wav", "Chinese", 8192)])

    def test_auto_language_keeps_model_language_detection_enabled(self) -> None:
        model = RecordingModel()
        transcriber = loaded_transcriber(model)

        transcriber.transcribe_wav("meeting.wav", language="auto")

        self.assertEqual(model.calls, [("meeting.wav", None, 8192)])

    def test_generate_without_language_keyword_remains_compatible(self) -> None:
        model = LanguageAgnosticModel()
        transcriber = loaded_transcriber(model)

        self.assertEqual(transcriber.transcribe_wav("meeting.wav", language="en"), "transcript")
        self.assertEqual(model.calls, ["meeting.wav"])

    def test_warmup_runs_a_bounded_inference_on_a_temporary_wav(self) -> None:
        model = RecordingModel()
        transcriber = loaded_transcriber(model)

        transcriber.warmup(language="zh")

        warmup_path, language, max_tokens = model.calls[0]
        self.assertEqual(language, "Chinese")
        self.assertEqual(max_tokens, 8)
        self.assertFalse(Path(warmup_path).exists())

    def test_normalizes_supported_language_hints(self) -> None:
        self.assertEqual(normalize_language_hint("zh"), "Chinese")
        self.assertEqual(normalize_language_hint("EN"), "English")
        self.assertIsNone(normalize_language_hint("auto"))
        self.assertEqual(normalize_language_hint("Cantonese"), "Cantonese")


class FasterWhisperTranscriberTest(unittest.TestCase):
    def test_load_uses_a_local_cpu_int8_model(self) -> None:
        loaded_models: list[tuple[str, dict[str, object]]] = []

        def whisper_model(model_path: str, **options: object) -> object:
            loaded_models.append((model_path, options))
            return RecordingFasterWhisperModel()

        with faster_whisper_model_dir() as model_dir:
            with patch.dict(
                "sys.modules",
                {"faster_whisper": SimpleNamespace(WhisperModel=whisper_model)},
            ):
                transcriber = FasterWhisperTranscriber(cpu_threads=6)
                transcriber.load(model_dir)
            expected_path = str(Path(model_dir).resolve())

        self.assertEqual(
            loaded_models,
            [
                (
                    expected_path,
                    {
                        "device": "cpu",
                        "compute_type": "int8",
                        "num_workers": 1,
                        "local_files_only": True,
                        "cpu_threads": 6,
                    },
                )
            ],
        )

    def test_load_rejects_an_incomplete_local_model_directory(self) -> None:
        with tempfile.TemporaryDirectory() as model_dir:
            Path(model_dir, "config.json").write_text("{}", encoding="utf-8")
            transcriber = FasterWhisperTranscriber()

            with self.assertRaisesRegex(RuntimeError, "missing model.bin"):
                transcriber.load(model_dir)

    def test_transcribe_maps_language_and_joins_lazy_segments(self) -> None:
        model = RecordingFasterWhisperModel()
        transcriber = loaded_faster_whisper_transcriber(model)

        text = transcriber.transcribe_wav("meeting.wav", language="zh")

        self.assertEqual(text, "hello world")
        self.assertEqual(model.calls, [("meeting.wav", "zh", 5)])

    def test_auto_language_keeps_detection_enabled(self) -> None:
        model = RecordingFasterWhisperModel()
        transcriber = loaded_faster_whisper_transcriber(model)

        transcriber.transcribe_wav("meeting.wav", language="auto")

        self.assertEqual(model.calls, [("meeting.wav", None, 5)])

    def test_warmup_uses_one_beam_and_removes_its_wav(self) -> None:
        model = RecordingFasterWhisperModel()
        transcriber = loaded_faster_whisper_transcriber(model)

        transcriber.warmup(language="en")

        warmup_path, language, beam_size = model.calls[0]
        self.assertEqual(language, "en")
        self.assertEqual(beam_size, 1)
        self.assertFalse(Path(warmup_path).exists())

    def test_factory_creates_the_cpu_backend_with_its_thread_budget(self) -> None:
        transcriber = create_transcriber("faster-whisper", cpu_threads=4)

        self.assertIsInstance(transcriber, FasterWhisperTranscriber)
        self.assertEqual(transcriber.cpu_threads, 4)

    def test_normalizes_faster_whisper_language_hints(self) -> None:
        self.assertEqual(normalize_faster_whisper_language("ZH"), "zh")
        self.assertEqual(normalize_faster_whisper_language("en"), "en")
        self.assertIsNone(normalize_faster_whisper_language("auto"))


class RecordingModel:
    def __init__(self) -> None:
        self.calls: list[tuple[str, str | None, int]] = []

    def generate(
        self,
        audio: str,
        *,
        language: str | None = None,
        max_tokens: int = 8192,
    ) -> object:
        self.calls.append((audio, language, max_tokens))
        return SimpleNamespace(text="transcript")


class LanguageAgnosticModel:
    def __init__(self) -> None:
        self.calls: list[str] = []

    def generate(self, audio: str) -> object:
        self.calls.append(audio)
        return SimpleNamespace(text="transcript")


class RecordingFasterWhisperModel:
    def __init__(self) -> None:
        self.calls: list[tuple[str, str | None, int]] = []

    def transcribe(
        self,
        audio: str,
        *,
        language: str | None,
        beam_size: int,
    ) -> tuple[object, object]:
        self.calls.append((audio, language, beam_size))
        segments = iter((SimpleNamespace(text=" hello"), SimpleNamespace(text=" world")))
        return segments, SimpleNamespace(language=language)


def loaded_transcriber(model: object) -> MlxAudioTranscriber:
    transcriber = MlxAudioTranscriber()
    transcriber.model = model
    transcriber.model_id = "test-model"
    return transcriber


def loaded_faster_whisper_transcriber(model: object) -> FasterWhisperTranscriber:
    transcriber = FasterWhisperTranscriber()
    transcriber.model = model
    transcriber.model_id = "test-model"
    return transcriber


def faster_whisper_model_dir() -> tempfile.TemporaryDirectory[str]:
    model_dir = tempfile.TemporaryDirectory()
    Path(model_dir.name, "config.json").write_text("{}", encoding="utf-8")
    Path(model_dir.name, "model.bin").write_bytes(b"test-model")
    return model_dir


if __name__ == "__main__":
    unittest.main()
