"""Static file server for the game (run by serve.sh, which finds the port).

python3 -m http.server sends Last-Modified but no Cache-Control, so browsers
guess how long each file stays fresh. A phone then reloads with some modules
new and some hours old: new touch controls lit up while an old Input.js
ignored them. Everything under src/ and the pages are sent no-store, so every
load is one consistent version. vendor/ (pinned three.js) and audio/ (the
recorded radio voice) are no-cache: always revalidated, but a 304 saves
re-downloading them.
"""
import argparse
import functools
import os
import http.server
from pathlib import Path


class Handler(http.server.SimpleHTTPRequestHandler):
    # The Rust build's wasm must be application/wasm for streaming compilation.
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                      '.wasm': 'application/wasm', '.mrscene': 'application/octet-stream'}

    def pinned(self):
        # No path yet when the request line itself is bad (e.g. a browser
        # trying https on this port), and send_error still sends headers.
        return getattr(self, 'path', '').startswith(('/vendor/', '/audio/'))

    def send_head(self):
        # Never answer 304 for a file the browser was told not to keep: an
        # old copy cached before this server existed must be replaced.
        if not self.pinned():
            del self.headers['If-Modified-Since']
        gz = self.precompressed()
        if gz:
            return gz
        return super().send_head()

    def precompressed(self):
        """The Rust build under dist/: when `cargo xtask web --release` left a
        `<file>.gz` beside the file and the browser takes gzip, send that with
        Content-Encoding, so load times on phones are realistic (SPEC 6.6)."""
        path = getattr(self, 'path', '').split('?', 1)[0].split('#', 1)[0]
        if not path.startswith('/dist/') or 'gzip' not in self.headers.get('Accept-Encoding', ''):
            return None
        file = self.translate_path(path)
        if not os.path.isfile(file) or not os.path.isfile(file + '.gz'):
            return None
        try:
            f = open(file + '.gz', 'rb')
        except OSError:
            return None
        self.send_response(200)
        self.send_header('Content-Type', self.guess_type(file))
        self.send_header('Content-Encoding', 'gzip')
        self.send_header('Content-Length', str(os.fstat(f.fileno()).st_size))
        self.send_header('Vary', 'Accept-Encoding')
        self.end_headers()
        return f

    def end_headers(self):
        self.send_header('Cache-Control', 'no-cache' if self.pinned() else 'no-store')
        super().end_headers()


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('--port', type=int, required=True)
    ap.add_argument('--bind', default='0.0.0.0')
    args = ap.parse_args()
    root = Path(__file__).resolve().parent.parent
    handler = functools.partial(Handler, directory=str(root))
    with http.server.ThreadingHTTPServer((args.bind, args.port), handler) as httpd:
        print(f'Serving {root} on http://{args.bind}:{args.port}/', flush=True)
        httpd.serve_forever()


if __name__ == '__main__':
    main()
