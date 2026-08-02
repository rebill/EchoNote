from __future__ import annotations

import argparse
from collections.abc import Sequence
import logging
import os

import uvicorn

from .app import create_app
from .logging import configure_logging


LOG_LEVELS = ("critical", "error", "warning", "info", "debug")
BACKENDS = ("fake", "mlx-audio", "faster-whisper")
DEFAULT_MODEL = "offline-asr-model-not-installed"
LOCAL_HOST = "127.0.0.1"


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run the EchoNote local ASR service.")
    parser.add_argument(
        "--host",
        default=LOCAL_HOST,
        choices=(LOCAL_HOST,),
        help="Host interface to bind. Only 127.0.0.1 is supported.",
    )
    parser.add_argument("--port", default=8765, type=int, help="Port to bind.")
    parser.add_argument("--model", default=DEFAULT_MODEL, help="Absolute local ASR model directory.")
    parser.add_argument("--backend", default="fake", choices=BACKENDS, help="ASR backend implementation.")
    parser.add_argument(
        "--cpu-threads",
        default=0,
        type=non_negative_int,
        help="CPU threads for faster-whisper. Use 0 for the runtime default.",
    )
    parser.add_argument("--log-level", default="info", choices=LOG_LEVELS, help="Structured log level.")
    return parser.parse_args(argv)


def non_negative_int(raw: str) -> int:
    value = int(raw)
    if value < 0:
        raise argparse.ArgumentTypeError("must be zero or a positive integer")
    return value


def main() -> None:
    args = parse_args()
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    os.environ["HF_DATASETS_OFFLINE"] = "1"
    os.environ.pop("HUGGINGFACE_HUB_TOKEN", None)
    os.environ.pop("HF_TOKEN", None)
    configure_logging(args.log_level)

    app = create_app(
        default_model=args.model,
        backend=args.backend,
        cpu_threads=args.cpu_threads,
    )
    config = uvicorn.Config(
        app,
        host=LOCAL_HOST,
        port=args.port,
        workers=1,
        log_config=None,
        log_level=args.log_level,
    )
    server = uvicorn.Server(config)
    app.state.server = server

    logging.getLogger(__name__).info(
        "server_starting",
        extra={
            "_host": LOCAL_HOST,
            "_port": args.port,
            "_model_id": args.model,
            "_backend": args.backend,
            "_cpu_threads": args.cpu_threads,
        },
    )
    server.run()


if __name__ == "__main__":
    main()
