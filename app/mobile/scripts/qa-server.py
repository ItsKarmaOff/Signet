#!/usr/bin/env python3
"""QA server (development): the iOS simulator asks "Open in Signet?" again on every deep link and simctl cannot
tap the screen. The app (in __DEV__, `src/dev/useQaCommands.ts`) reads one command per second here and navigates.

    python3 scripts/qa-server.py &
    curl -G --data-urlencode 'p=/dev/seed' localhost:8099/set       # same paths as the deep links, without the scheme
    curl -G --data-urlencode 'p=/?dev=linkmenu' localhost:8099/set
"""
import http.server, json, time
from urllib.parse import parse_qs, urlparse

cur = {'id': '0', 'path': ''}

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        if self.path.startswith('/set'):
            cur['id'], cur['path'] = str(time.time()), parse_qs(urlparse(self.path).query)['p'][0]
            self.send_response(200); self.end_headers(); self.wfile.write(b'ok')
            return
        self.send_response(200); self.send_header('Content-Type', 'application/json'); self.end_headers()
        self.wfile.write(json.dumps(cur).encode())

http.server.ThreadingHTTPServer(('127.0.0.1', 8099), Handler).serve_forever()
