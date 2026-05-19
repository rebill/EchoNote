from __future__ import annotations

import argparse
import logging

import uvicorn

from .app import create_app
from .logging import configure_logging


LOG_LEVELS = ("critical", "error", "warning", "info", "debug")
BACKENDS = ("fake", "mlx-audio")
DEFAULT_MODEL = "mlx-community/Qwen3-ASR-0.6B-4bit"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run the EchoNote local ASR service.")
    parser.add_argument("--host", default="127.0.0.1", help="Host interface to bind.")
    parser.add_argument("--port", default=8765, type=int, help="Port to bind.")
    parser.add_argument("--model", default=DEFAULT_MODEL, help="Initial model identifier.")
    parser.add_argument("--backend", default="fake", choices=BACKENDS, help="ASR backend implementation.")
    parser.add_argument("--log-level", default="info", choices=LOG_LEVELS, help="Structured log level.")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    configure_logging(args.log_level)

    app = create_app(default_model=args.model, backend=args.backend)
    config = uvicorn.Config(app, host=args.host, port=args.port, log_config=None, log_level=args.log_level)
    server = uvicorn.Server(config)
    app.state.server = server

    logging.getLogger(__name__).info(
        "server_starting",
        extra={"_host": args.host, "_port": args.port, "_model_id": args.model, "_backend": args.backend},
    )
    server.run()


if __name__ == "__main__":
    main()
