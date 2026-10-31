#!/usr/bin/env python3
"""A mock OpenAI-compatible server for the app smoke run (scripts/dev/app_smoke.sh).

Serves GET /v1/models and POST /v1/chat/completions (streamed SSE), answering by task: the
connection test, notes, Q&A, correction and live highlights, each with a fixed reply that
cites a time taken from the request. Model "slow" streams for a minute (cancel tests). A key
other than "smoke-key" gets 401. Logs one line per request with the task only, never text.

Usage: mock_llm.py <port-file> <log-file>; the chosen port is written to <port-file>.
"""

import json
import re
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

KEY = "smoke-key"
TIME = re.compile(r"\[(\d{1,2}:\d{2}(?::\d{2})?)\]")


def task(system: str) -> str:
    if not system:
        return "test"
    if "proofread" in system or "校对" in system:
        return "correct"
    if "running highlights" in system or "维护一份要点" in system:
        return "highlights"
    if "answer questions" in system or "回答关于一场会议的问题" in system:
        return "answer"
    return "notes"


def reply(kind: str, user: str) -> str:
    times = TIME.findall(user) or ["00:00"]
    first, second = times[0], times[min(1, len(times) - 1)]
    if kind == "test":
        return "OK"
    if kind == "notes":
        return (
            "# 冒烟测试会议\n\n## 概要\n这是冒烟测试生成的总结 [" + first + "]。\n\n"
            "## 决定\n- 无\n\n## 待办\n- [ ] 检查导出 [" + second + "]\n"
        )
    if kind == "answer":
        return "会上在 [" + second + "] 提到了这一点。"
    if kind == "highlights":
        return "- 冒烟要点 [" + first + "]"
    match = re.search(r"<transcript>\s*(\{.*\})\s*</transcript>", user, re.S)
    lines = json.loads(match.group(1))["lines"] if match else []
    fixes = [{"id": line["id"], "text": line["text"] + "SMOKEFIX"} for line in lines[:2]]
    return json.dumps({"lines": fixes}, ensure_ascii=False)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def note(self, text: str) -> None:
        with open(self.server.log_path, "a", encoding="utf-8") as log:
            log.write(f"{time.time():.3f} {text}\n")

    def authorized(self) -> bool:
        if self.headers.get("Authorization") == f"Bearer {KEY}":
            return True
        self.send_json(401, {"error": {"message": "invalid api key", "type": "invalid_request_error"}})
        self.note(f"{self.path} 401")
        return False

    def send_json(self, status: int, value) -> None:
        body = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path.rstrip("/") != "/v1/models":
            return self.send_json(404, {"error": {"message": "not found"}})
        if self.authorized():
            self.note("models")
            self.send_json(200, {"data": [{"id": "smoke-model"}, {"id": "slow"}]})

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        request = json.loads(self.rfile.read(length) or b"{}")
        if self.path.rstrip("/") != "/v1/chat/completions":
            return self.send_json(404, {"error": {"message": "not found"}})
        if not self.authorized():
            return
        messages = request.get("messages", [])
        system = next((m["content"] for m in messages if m.get("role") == "system"), "")
        user = "\n".join(m["content"] for m in messages if m.get("role") == "user")
        kind = task(system)
        self.note(f"chat {kind} model={request.get('model')}")
        if request.get("model") == "slow":
            pieces, delay = ["慢"] * 120, 0.5
        else:
            text = reply(kind, user)
            pieces, delay = [text[i : i + 8] for i in range(0, len(text), 8)], 0.02
        if not request.get("stream"):
            return self.send_json(
                200, {"choices": [{"message": {"content": "".join(pieces)}, "finish_reason": "stop"}]}
            )
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()
        try:
            for piece in pieces:
                chunk = {"choices": [{"delta": {"content": piece}, "finish_reason": None}]}
                self.wfile.write(f"data: {json.dumps(chunk, ensure_ascii=False)}\n\n".encode())
                self.wfile.flush()
                time.sleep(delay)
            done = {"choices": [{"delta": {}, "finish_reason": "stop"}]}
            self.wfile.write(f"data: {json.dumps(done)}\n\ndata: [DONE]\n\n".encode())
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            self.note(f"chat {kind} closed by client")
        self.close_connection = True


def main() -> None:
    port_file, log_path = sys.argv[1], sys.argv[2]
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.log_path = log_path
    with open(port_file, "w") as out:
        out.write(str(server.server_address[1]))
    server.serve_forever()


if __name__ == "__main__":
    main()
