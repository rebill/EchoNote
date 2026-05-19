from __future__ import annotations

import argparse
import json
import platform
import sys
import time
from pathlib import Path

from .transcriber import MlxAudioTranscriber
from .wav import validate_wav_bytes


DEFAULT_MODEL = "mlx-community/Qwen3-ASR-0.6B-4bit"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run a real MLX Qwen3 ASR spike against one WAV file.")
    parser.add_argument("--audio", required=True, help="Path to a 16kHz mono PCM16 WAV file.")
    parser.add_argument("--model", default=DEFAULT_MODEL, help="MLX model ID.")
    parser.add_argument("--language", default="auto", choices=("auto", "zh", "en"), help="Input language hint.")
    parser.add_argument("--json", action="store_true", help="Print JSON result only.")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    audio_path = Path(args.audio).expanduser().resolve()
    if not audio_path.exists():
        raise SystemExit(f"audio file does not exist: {audio_path}")

    wav_bytes = audio_path.read_bytes()
    validate_wav_bytes(wav_bytes)

    transcriber = MlxAudioTranscriber()

    try:
        load_started = time.perf_counter()
        transcriber.load(args.model)
        load_seconds = time.perf_counter() - load_started

        transcribe_started = time.perf_counter()
        text = transcriber.transcribe_wav(str(audio_path), language=args.language)
        transcribe_seconds = time.perf_counter() - transcribe_started
    except Exception as exc:
        error_result = {
            "ok": False,
            "model_id": args.model,
            "audio_path": str(audio_path),
            "error": str(exc),
        }
        print(json.dumps(error_result, ensure_ascii=False, indent=None if args.json else 2), file=sys.stderr)
        raise SystemExit(1) from None

    result = {
        "ok": True,
        "python": sys.version.split()[0],
        "platform": platform.platform(),
        "model_id": args.model,
        "audio_path": str(audio_path),
        "audio_bytes": len(wav_bytes),
        "language": args.language,
        "load_seconds": round(load_seconds, 3),
        "transcribe_seconds": round(transcribe_seconds, 3),
        "text": text,
    }

    if args.json:
        print(json.dumps(result, ensure_ascii=False))
        return

    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
