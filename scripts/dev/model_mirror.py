"""A slow local model mirror for the app smoke run (ECHONOTE_MODEL_MIRROR).

Serves ROOT/<model dir>/<file> on 127.0.0.1 at about 12 MB/s, so a download can be paused
part way, and honours `Range: bytes=N-` so it can resume. Writes the port to PORT_FILE.

Usage: model_mirror.py ROOT PORT_FILE
"""

from __future__ import annotations

import http.server
import sys
import time
from pathlib import Path

CHUNK = 64 << 10
PAUSE = 0.005


class Handler(http.server.BaseHTTPRequestHandler):
    root: Path

    def do_GET(self) -> None:  # noqa: N802 (http.server naming)
        # The root holds symlinks to model directories, so check the request, not the target.
        parts = self.path.split("?")[0].lstrip("/").split("/")
        path = self.root.joinpath(*parts)
        if ".." in parts or "" in parts or not path.is_file():
            self.send_error(404)
            return
        size = path.stat().st_size
        start = 0
        header = self.headers.get("Range", "")
        if header.startswith("bytes=") and header.endswith("-"):
            start = int(header[6:-1])
        if start >= size and size > 0:
            self.send_error(416)
            return
        self.send_response(206 if start else 200)
        self.send_header("Content-Length", str(size - start))
        if start:
            self.send_header("Content-Range", f"bytes {start}-{size - 1}/{size}")
        self.end_headers()
        with path.open("rb") as file:
            file.seek(start)
            while chunk := file.read(CHUNK):
                try:
                    self.wfile.write(chunk)
                except (BrokenPipeError, ConnectionResetError):
                    return
                time.sleep(PAUSE)

    def log_message(self, format: str, *args: object) -> None:  # noqa: A002
        pass


def main() -> None:
    root, port_file = Path(sys.argv[1]).resolve(), Path(sys.argv[2])
    Handler.root = root
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    port_file.write_text(str(server.server_address[1]))
    server.serve_forever()


if __name__ == "__main__":
    main()
