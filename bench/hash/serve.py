#!/usr/bin/env python3
"""Serves bench.html to a browser on this computer, prints what the page sends back and ends."""
import http.server
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
TYPES = {".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm"}


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        path = HERE / self.path.lstrip("/").split("?")[0]
        allowed = path.parent == HERE and path.suffix in TYPES and path.exists()
        body = path.read_bytes() if allowed else b""
        self.send_response(200 if allowed else 404)
        self.send_header("Content-Type", TYPES.get(path.suffix, "text/plain"))
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        text = self.rfile.read(int(self.headers["Content-Length"])).decode()
        print(text, flush=True)
        self.send_response(204)
        self.end_headers()
        self.server.done = True

    def log_message(self, *_):
        pass


server = http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), Handler)
server.done = False
while not server.done:
    server.handle_request()
