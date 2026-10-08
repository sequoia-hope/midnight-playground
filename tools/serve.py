"""Static file server for the game (run by serve.sh, which finds the port).

python3 -m http.server sends Last-Modified but no Cache-Control, so browsers
guess how long each file stays fresh. A phone then reloads with some modules
new and some hours old: new touch controls lit up while an old Input.js
ignored them. Everything under src/ and the pages are sent no-store, so every
load is one consistent version. vendor/ (pinned three.js) and audio/ (the
recorded radio voice) are no-cache: always revalidated, but a 304 saves
re-downloading them.

The one thing it writes: the Radio page's song ratings (radio.md 8). A POST
to FAVOURITES (the file the page also reads) upserts one song into it on
disk, so the owner's keeps and rejects land in the working tree and travel
with the repo; GitHub Pages has no such endpoint, so the page shows the
rating controls only where the GET carries X-Favourites: writable.
"""
import argparse
import functools
import json
import os
import http.server
import threading
from pathlib import Path

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
    # The Rust build's wasm must be application/wasm for streaming compilation.
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                      '.wasm': 'application/wasm', '.mrscene': 'application/octet-stream'}

    def pinned(self):
        # No path yet when the request line itself is bad (e.g. a browser
        # trying https on this port), and send_error still sends headers.
        return getattr(self, 'path', '').startswith(('/vendor/', '/audio/'))

    def is_favourites(self):
        return getattr(self, 'path', '').split('?', 1)[0] == '/' + FAVOURITES

    def send_head(self):
        # Never answer 304 for a file the browser was told not to keep: an
        # old copy cached before this server existed must be replaced.
        if not self.pinned():
            del self.headers['If-Modified-Since']
        gz = self.precompressed()
        if gz:
            return gz
        return super().send_head()

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

    def precompressed(self):
        """The Rust build under dist/: when `cargo xtask web --release` left a
        `<file>.br` or `<file>.gz` beside the file, send the best one the
        browser takes with Content-Encoding, so load times on phones are
        realistic (SPEC 6.6). Browsers offer brotli only on https, so phones
        on the tailnet front get it and plain http gets gzip (D677)."""
        path = getattr(self, 'path', '').split('?', 1)[0].split('#', 1)[0]
        if not path.startswith('/dist/'):
            return None
        accepted = {t.split(';', 1)[0].strip().lower()
                    for t in self.headers.get('Accept-Encoding', '').split(',')}
        file = self.translate_path(path)
        if not os.path.isfile(file):
            return None
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
