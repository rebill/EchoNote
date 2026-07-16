from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace
import unittest

from echonote_asr.transcriber import MlxAudioTranscriber, normalize_language_hint


class MlxAudioTranscriberTest(unittest.TestCase):
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


def loaded_transcriber(model: object) -> MlxAudioTranscriber:
    transcriber = MlxAudioTranscriber()
    transcriber.model = model
    transcriber.model_id = "test-model"
    return transcriber


if __name__ == "__main__":
    unittest.main()
