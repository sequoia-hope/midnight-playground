"""Static file server for the game (run by serve.sh, which finds the port).

Playground Kit's template/tools/serve.py (D1233) plus this game's own: the
URL space is GitHub Pages' (D1112, D1197), the Rust build (dist/next/)
over the repo root so / is Midnight Playground, and the JS game under
/legacy/ (see Handler.translate_path); the .mrscene type; and the Radio
page's song ratings (radio.md 8): a POST to FAVOURITES (the file the page
also reads) upserts one song into it on disk, so the owner's keeps and
rejects land in the working tree and travel with the repo; GitHub Pages
has no such endpoint, so the page shows the rating controls only where the
GET carries X-Favourites: writable.

From the template: python3 -m http.server sends Last-Modified but no
Cache-Control, so browsers guess how long each file stays fresh, and a
phone then reloads with some modules new and some hours old. Everything is
sent no-store, so every load is one consistent version, except the PINNED
directories (vendor/, the pinned three.js; audio/, the recorded voice),
which are no-cache: always revalidated, but a 304 saves re-downloading
them. A precompressed `<file>.br` or `<file>.gz` beside a file under dist/
is sent with Content-Encoding when the browser takes it (brotli only on
https, so phones on the tailnet front get it and plain http gets gzip,
D677). Before the first build, / is a page saying so.

The port is `--port`, required, with no default: serve.sh passes $PORT or
`proj port`.
"""
import argparse
import functools
import json
import os
import http.server
import threading
from pathlib import Path

# The web build, from the repository root.
DIST = os.path.join('dist', 'next')
# URL prefixes sent no-cache instead of no-store (also under /legacy/).
PINNED = ('/vendor/', '/audio/')
TITLE = 'Midnight Playground'

NOT_BUILT = (f"""<!doctype html><meta charset=utf-8><title>{TITLE}</title>
<body style="font:16px system-ui;background:#05060c;color:#e8ecf4;padding:2rem">
<h1>{TITLE}</h1><p>No web build yet. Run <code>cargo xtask web --release</code>, then reload.
The JavaScript game is at <a href="legacy/" style="color:#b9c2d8">legacy/</a>.</p>""").encode()

# The song ratings the Radio page writes, relative to the repo root; the
# same path the page fetches. One object per line under "songs", sorted, so
# a diff shows the songs that changed.
FAVOURITES = 'crates/mp_music/favourites.json'
FAVOURITES_ABOUT = ('Songs heard on the Radio page and kept or rejected (docs/vision/radio.md 8). A song is '
                    'its (genre, seed) pair: "keep" puts it in the station\'s rotation, "reject" keeps discovery '
                    'from drawing it again; the note is why. Written by tools/serve.py; edit by hand freely.')
_lock = threading.Lock()


def upsert_favourite(root, song):
    """Adds `song` to FAVOURITES, replacing an earlier verdict on the same
    (genre, seed); returns the file's new contents."""
    for k in ('genre', 'seed', 'verdict'):
        if k not in song:
            raise ValueError(f'a song needs {k}')
    if song['verdict'] not in ('keep', 'reject'):
        raise ValueError('verdict is keep or reject')
    path = root / FAVOURITES
    with _lock:
        try:
            data = json.loads(path.read_text())
        except FileNotFoundError:
            data = {}
        songs = [s for s in data.get('songs', []) if (s['genre'], s['seed']) != (song['genre'], song['seed'])]
        songs.append(song)
        songs.sort(key=lambda s: (s.get('station', ''), s['genre'], s['seed']))
        data = {'about': FAVOURITES_ABOUT, 'songs': songs}
        lines = ',\n'.join('  ' + json.dumps(s, ensure_ascii=False, sort_keys=True) for s in songs)
        text = '{\n "about": ' + json.dumps(FAVOURITES_ABOUT) + ',\n "songs": [' + (f'\n{lines}\n ' if lines else '') + ']\n}\n'
        tmp = path.with_suffix('.json.tmp')
        tmp.write_text(text)
        os.replace(tmp, path)
    return data


class Handler(http.server.SimpleHTTPRequestHandler):
    # The wasm must be application/wasm for streaming compilation.
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                      '.wasm': 'application/wasm', '.js': 'text/javascript', '.mjs': 'text/javascript',
                      '.mrscene': 'application/octet-stream'}

    def pinned(self):
        # No path yet when the request line itself is bad (e.g. a browser
        # trying https on this port), and send_error still sends headers.
        path = getattr(self, 'path', '')
        if path.startswith('/legacy/'):
            path = path[len('/legacy'):]
        return path.startswith(PINNED)

    def translate_path(self, path):
        """The working tree laid out as GitHub Pages lays out the site
        (DECISIONS D1197): the Rust build, `dist/next/`, over the root, so
        `/` is Midnight Playground; the JS game under `/legacy/`, as its
        tracked files at the root; everything else, `dist/next/` itself,
        the tools and the parity reports, where it is in the tree."""
        file = super().translate_path(path)
        root = os.path.abspath(self.directory)
        rel = os.path.relpath(file, root)
        if rel == '.':
            rel = ''
        if rel == 'legacy' or rel.startswith('legacy' + os.sep):
            rel = rel[len('legacy'):].lstrip(os.sep)
            if rel == 'dist' or rel.startswith('dist' + os.sep):
                return file  # Pages drops legacy/dist/: nothing there.
            return os.path.join(root, rel)
        # Over the root file by file, as `cp -r dist/next/. _site/` is: a
        # directory only counts when the build has its page.
        over = os.path.join(root, DIST, rel)
        if os.path.isfile(over) or os.path.isfile(os.path.join(over, 'index.html')):
            return over
        return file

    def is_favourites(self):
        return getattr(self, 'path', '').split('?', 1)[0] == '/' + FAVOURITES

    def send_head(self):
        # Never answer 304 for a file the browser was told not to keep: an
        # old copy cached before this server existed must be replaced.
        if not self.pinned():
            del self.headers['If-Modified-Since']
        path = getattr(self, 'path', '').split('?', 1)[0].split('#', 1)[0]
        if path == '/' and not os.path.isfile(self.translate_path('/index.html')):
            self.send_response(200)
            self.send_header('Content-Type', 'text/html; charset=utf-8')
            self.send_header('Content-Length', str(len(NOT_BUILT)))
            self.end_headers()
            self.wfile.write(NOT_BUILT)
            return None
        return self.precompressed(path) or super().send_head()

    def do_POST(self):
        if not self.is_favourites():
            return self.send_error(405, 'nothing here takes a POST')
        try:
            n = int(self.headers.get('Content-Length') or 0)
            song = json.loads(self.rfile.read(n))
            data = upsert_favourite(Path(self.directory), song)
        except (ValueError, TypeError, KeyError) as e:
            return self.send_error(400, f'bad song: {e}')
        body = json.dumps(data).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def precompressed(self, path):
        file = self.translate_path(path)
        if not file.startswith(os.path.join(os.path.abspath(self.directory), 'dist') + os.sep):
            return None
        if not os.path.isfile(file):
            return None
        accepted = {t.split(';', 1)[0].strip().lower()
                    for t in self.headers.get('Accept-Encoding', '').split(',')}
        for encoding, ext in (('br', '.br'), ('gzip', '.gz')):
            if encoding not in accepted or not os.path.isfile(file + ext):
                continue
            try:
                f = open(file + ext, 'rb')
            except OSError:
                continue
            self.send_response(200)
            self.send_header('Content-Type', self.guess_type(file))
            self.send_header('Content-Encoding', encoding)
            self.send_header('Content-Length', str(os.fstat(f.fileno()).st_size))
            self.send_header('Vary', 'Accept-Encoding')
            self.end_headers()
            return f
        return None

    def end_headers(self):
        self.send_header('Cache-Control', 'no-cache' if self.pinned() else 'no-store')
        if self.is_favourites():
            self.send_header('X-Favourites', 'writable')
        super().end_headers()


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('--port', type=int, required=True, help='from serve.sh: $PORT or `proj port`; no default')
    ap.add_argument('--bind', default='0.0.0.0')
    args = ap.parse_args()
    root = Path(__file__).resolve().parent.parent
    handler = functools.partial(Handler, directory=str(root))
    with http.server.ThreadingHTTPServer((args.bind, args.port), handler) as httpd:
        print(f'Serving {root} on http://{args.bind}:{args.port}/', flush=True)
        httpd.serve_forever()


if __name__ == '__main__':
    main()
