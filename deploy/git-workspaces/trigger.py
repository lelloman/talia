#!/usr/bin/env python3
"""Internal fixed-action queue for the host's cron-owned Git probe."""
import argparse
import fcntl
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path
import time

from probe import atomic


def read(path):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, ValueError):
        return {}


def enqueue(directory, result_path):
    directory = Path(directory)
    with (directory / 'queue.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        request = read(directory / 'request.json')
        result = read(result_path)
        requested = request.get('requested', 0)
        # Repeated clicks share a pending request; expire abandoned requests.
        if requested <= result.get('checked', 0) or time.time() - requested > 180:
            requested = time.time()
            atomic(directory / 'request.json', json.dumps({'requested': requested}))
        return {'requested': requested}


def handler(directory, result_path):
    class Handler(BaseHTTPRequestHandler):
        def reply(self, status, value):
            body = json.dumps(value).encode()
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            if self.path != '/check':
                return self.reply(404, {'error': 'not_found'})
            # No commands, paths or configuration are accepted from callers.
            if self.headers.get('Transfer-Encoding') or self.headers.get('Content-Length', '0') != '0':
                return self.reply(400, {'error': 'body_not_allowed'})
            try:
                self.reply(202, enqueue(directory, result_path))
            except OSError:
                self.reply(503, {'error': 'queue_unavailable'})

        def do_GET(self):
            if self.path != '/status':
                return self.reply(404, {'error': 'not_found'})
            result = read(result_path)
            self.reply(200, {'checked': result.get('checked', 0), 'success': result.get('success', False)})

        def log_message(self, *args):
            pass
    return Handler


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--requests', required=True)
    p.add_argument('--result', required=True)
    p.add_argument('--port', type=int, default=8080)
    a = p.parse_args()
    HTTPServer(('0.0.0.0', a.port), handler(a.requests, a.result)).serve_forever()
