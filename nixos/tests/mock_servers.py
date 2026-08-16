#!/usr/bin/env python3
"""Combined local HTTP server and Telegram Bot API mock for NixOS tests."""
import json
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

PAGE = b"""<html><body><h1>Hello</h1><div class="item">Alpha</div></body></html>"""


class PageHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/page"):
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(PAGE)))
            self.end_headers()
            self.wfile.write(PAGE)
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, *args):
        pass


class TelegramHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length)
        try:
            payload = json.loads(body) if body else {}
        except Exception:
            payload = {}
        if "chat_id" not in payload:
            payload["chat_id"] = "12345"
        response = json.dumps(
            {
                "ok": True,
                "result": {
                    "message_id": 1,
                    "chat": {"id": payload.get("chat_id")},
                    "text": payload.get("text", ""),
                },
            }
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(response)))
        self.end_headers()
        self.wfile.write(response)

    def log_message(self, *args):
        pass


def main():
    page_server = HTTPServer(("127.0.0.1", 8080), PageHandler)
    tg_server = HTTPServer(("127.0.0.1", 8443), TelegramHandler)
    threading.Thread(target=page_server.serve_forever, daemon=True).start()
    tg_server.serve_forever()


if __name__ == "__main__":
    main()
