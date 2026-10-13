#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["laya-mlx"]
# ///
"""Local System One judge for stupid-comments, backed by Laya on Apple silicon.

Serves POST /v1/systemone ({model, state, questions} -> {answers}) on loopback.

Run:  uv run scripts/laya-judge.py            (or: ./scripts/laya-judge.py)
Env:  LAYA_JUDGE_PORT (8766), LAYA_JUDGE_MODEL (aac6fef/laya-mlx)
"""

import json
import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import laya_mlx

HOST = "127.0.0.1"
PORT = int(os.environ.get("LAYA_JUDGE_PORT", "8766"))
CHECKPOINT = os.environ.get("LAYA_JUDGE_MODEL", "aac6fef/laya-mlx")
QTYPES = ("noul", "choice", "score")
MAX_BODY = 1 << 20

AGENT = laya_mlx.load(CHECKPOINT)
GPU = threading.Lock()


def judge(body):
    state, questions = body.get("state"), body.get("questions")
    if not state or not isinstance(questions, dict) or not questions:
        raise ValueError("state and questions are required")
    for qid, q in questions.items():
        if not isinstance(q, dict) or q.get("type") not in QTYPES:
            raise ValueError(f"questions[{qid!r}].type must be one of {', '.join(QTYPES)}")
    plain = {qid: {k: q[k] for k in ("type", "instructions", "criteria") if k in q} for qid, q in questions.items()}
    with GPU:
        result = AGENT.predict(state, plain)
    return {
        "model": body.get("model") or CHECKPOINT,
        "answers": result["answers"],
        "usage": {"input_tokens": result["usage"]["input_tokens"], "output_tokens": 0, "cost": 0},
    }


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        pass

    def send(self, status, payload):
        data = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def allowed(self):
        # Loopback names only, and never a browser: blocks DNS rebinding.
        return self.headers.get("Host") in (f"127.0.0.1:{PORT}", f"localhost:{PORT}") and "Origin" not in self.headers

    def do_GET(self):
        if self.path == "/health" and self.allowed():
            self.send(200, {"status": "ok", "model": CHECKPOINT})
        else:
            self.send(404, {"error": "not found"})

    def do_POST(self):
        if self.path != "/v1/systemone" or not self.allowed():
            return self.send(404, {"error": "not found"})
        length = int(self.headers.get("Content-Length") or 0)
        if length > MAX_BODY:
            return self.send(413, {"error": "body too large"})
        try:
            self.send(200, judge(json.loads(self.rfile.read(length))))
        except (ValueError, AttributeError) as error:
            self.send(422, {"error": str(error)})


if __name__ == "__main__":
    print(f"laya-judge: {CHECKPOINT} on http://{HOST}:{PORT}/v1/systemone", flush=True)
    ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()
