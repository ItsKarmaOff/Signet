#!/usr/bin/env python3
"""Local stand-in for the GitHub releases feed, for the update end-to-end test (scripts/update-e2e.sh).

  update-e2e-server.py <folder> <port> <tag>

GET /releases?... answers the shape of the GitHub API (one release, tag <tag>, every file of <folder> as an asset whose
browser_download_url points back at this server); any other path is a file of <folder> (latest*.yml, packages).
"""
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import unquote, urlparse

FOLDER, PORT, TAG = os.path.abspath(sys.argv[1]), int(sys.argv[2]), sys.argv[3]


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _send(self, code, body, ctype="application/octet-stream", head=False):
        self.send_response(code)
        self.send_header("content-type", ctype)
        self.send_header("content-length", str(len(body)))
        self.send_header("connection", "close")
        self.end_headers()
        if not head:
            self.wfile.write(body)

    def _serve(self, head):
        path = unquote(urlparse(self.path).path)
        sys.stderr.write("%s %s\n" % (self.command, self.path))
        if path == "/releases":
            base = "http://127.0.0.1:%d" % PORT
            assets = [
                {"name": n, "browser_download_url": "%s/%s" % (base, n)}
                for n in sorted(os.listdir(FOLDER))
                if os.path.isfile(os.path.join(FOLDER, n))
            ]
            body = json.dumps([{"tag_name": TAG, "draft": False, "prerelease": False, "assets": assets}]).encode()
            return self._send(200, body, "application/json", head)
        target = os.path.join(FOLDER, os.path.basename(path))
        if os.path.isfile(target):
            with open(target, "rb") as f:
                return self._send(200, f.read(), head=head)
        self._send(404, b"", head=head)

    def do_GET(self):
        self._serve(False)

    def do_HEAD(self):
        self._serve(True)

    def log_message(self, *a):
        pass


ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
