#!/usr/bin/env python3
"""Serve the Rusty7800 web shell locally (fixes `file://` CORS failures).

Why this exists: opening `site/index.html` straight from disk (`file://`)
cannot work — browsers block ES-module scripts and the `fetch()` of the
`.wasm` bundle on the null origin (CORS). Serve this directory over HTTP
instead::

    python3 site/serve.py            # serves site/ on http://127.0.0.1:8901
    python3 site/serve.py --port 8080

Stdlib only. Forces the `application/wasm` MIME type (some servers guess
wrong, which breaks `instantiateStreaming`) and disables caching so a
fresh `wasm-pack` rebuild shows up on reload. For a zero-server option
(double-clickable from disk), see `bundle.py`.
"""

import argparse
import functools
import http.server
import os

HERE = os.path.dirname(os.path.abspath(__file__))


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".wasm": "application/wasm",
        ".js": "text/javascript",
    }

    def end_headers(self):
        # Dev convenience: never cache the app shell or its bundles.
        if self.path.split("?")[0].endswith((".html", ".js", ".wasm")):
            self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, *args):
        pass  # quiet; the URL below is the whole UI


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--port", type=int, default=8901)
    args = ap.parse_args()
    handler = functools.partial(Handler, directory=HERE)
    try:
        srv = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), handler)
    except OSError as e:
        raise SystemExit(
            f"cannot bind 127.0.0.1:{args.port} ({e}); "
            "another server may hold it — retry with --port PORT"
        )
    with srv:
        print(f"Rusty7800 web shell: http://127.0.0.1:{args.port}/index.html")
        print("Standalone (file://-safe) build: rusty7800-standalone.html")
        print("Press Ctrl-C to stop.")
        try:
            srv.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
