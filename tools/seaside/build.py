#!/usr/bin/env python3
"""Seaside Raceway: build the level's data from real survey data.

Seaside Raceway is a copy of Laguna Seca (Monterey County, California). This
script downloads what it needs, caches it in tools/seaside/cache/, and writes
two generated modules:

  src/levels/seaside/circuit.js  the racing line's shape, height and camber,
                                 plus walls, buildings, grandstands, bridges,
                                 the pit lane and the infield lake
  src/levels/seaside/ground.js   the ground: heights, colour, tree cover,
                                 and which ground is paved and which loose
  src/levels/seaside/photo.jpg   the aerial photo round the circuit, graded
                                 for the game, draped over the ground
  assets/seaside/survey.bin      circuit.js and ground.js as one binary file
                                 for the Rust port (mp_levels::seaside)

Sources (all free to use):
  OpenStreetMap (ODbL, (c) OpenStreetMap contributors): the circuit's route
    relation 21195763 gives the centreline in racing order; walls, buildings,
    grandstands, bridges and water come from the same download. It sits
    about 1.3 m west of the survey, so everything from it is moved onto the
    photo (which lines up with the lidar), and the centreline and the
    tarmac's width are then taken from the photo's own edges.
  USGS 3DEP 1 m DEM (public domain): bare-earth lidar from the 2018-19
    CA_AZ_FEMA_R9_Lidar_2017_D18 survey. The track surface, its camber and
    the hills around it. The same service's coarser data fills in out to
    the horizon.
  USGS NAIP aerial imagery (public domain): the tarmac's edges, the ground's
    colour, paved and loose run-off, and where the oaks stand.

Needs python3 with numpy and Pillow, and network access on the first run.
  python3 tools/seaside/build.py            # use the cache where it can
  python3 tools/seaside/build.py --refresh  # download everything again
"""

import argparse
import base64
import json
import math
import os
import sys
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
import zlib

import numpy as np
from PIL import Image

Image.MAX_IMAGE_PIXELS = None

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CACHE = os.path.join(ROOT, 'tools', 'seaside', 'cache')
OUT = os.path.join(ROOT, 'src', 'levels', 'seaside')
SURVEY = os.path.join(ROOT, 'assets', 'seaside', 'survey.bin')

ROUTE_RELATION = '21195763'
PIT_LANE_WAY = '109859349'
OSM_BBOX = (-121.768, 36.576, -121.742, 36.593)  # lon0, lat0, lon1, lat1
DEM_SERVICE = 'https://elevation.nationalmap.gov/arcgis/rest/services/3DEPElevation/ImageServer/exportImage'
NAIP_SERVICE = 'https://imagery.nationalmap.gov/arcgis/rest/services/USGSNAIPImagery/ImageServer/exportImage'
UA = 'midnight-racer-seaside-build/1.0'

# The start/finish line, metres along the OSM route from its first node:
# on the pit straight, level with the main grandstands.
START_LINE_S = 2440
# The lap starts at the line.
START_S = 0

# Local frame: x east, z south (three.js: y up), metres from the line.
# Grids (UTM zone 10N metres). DEM_FINE covers the circuit and its
# surroundings at 4 m; DEM_WIDE reaches the far hills at 16 m.
FINE_STEP, WIDE_STEP = 4, 16
# Furthest the barrier stands from the centreline (open run-off).
WALL_MAX = 34.0
# The draped photo: metres per pixel, and how far past the lap it reaches.
PHOTO_STEP, PHOTO_PAD = 0.6, 160
# The tarmac's measured width is held between these (m).
WIDTH_MIN, WIDTH_MAX = 10.5, 15.5


# ── Geodesy ───────────────────────────────────────────────────────────
def ll2utm(lat, lon, zone=10):
    """Latitude/longitude (NAD83 ~ WGS84 here) to UTM easting/northing."""
    a = 6378137.0
    f = 1 / 298.257222101
    k0 = 0.9996
    e2 = f * (2 - f)
    ep2 = e2 / (1 - e2)
    lon0 = math.radians(-183 + 6 * zone)
    phi, lam = math.radians(lat), math.radians(lon)
    N = a / math.sqrt(1 - e2 * math.sin(phi) ** 2)
    T = math.tan(phi) ** 2
    C = ep2 * math.cos(phi) ** 2
    A = math.cos(phi) * (lam - lon0)
    e4, e6 = e2 * e2, e2 ** 3
    M = a * ((1 - e2 / 4 - 3 * e4 / 64 - 5 * e6 / 256) * phi
             - (3 * e2 / 8 + 3 * e4 / 32 + 45 * e6 / 1024) * math.sin(2 * phi)
             + (15 * e4 / 256 + 45 * e6 / 1024) * math.sin(4 * phi)
             - (35 * e6 / 3072) * math.sin(6 * phi))
    x = k0 * N * (A + (1 - T + C) * A ** 3 / 6 + (5 - 18 * T + T * T + 72 * C - 58 * ep2) * A ** 5 / 120) + 500000
    y = k0 * (M + N * math.tan(phi) * (A * A / 2 + (5 - T + 9 * C + 4 * C * C) * A ** 4 / 24
                                       + (61 - 58 * T + T * T + 600 * C - 330 * ep2) * A ** 6 / 720))
    return x, y


# ── Downloads ─────────────────────────────────────────────────────────
def fetch(name, url, refresh, data=None, timeout=300):
    path = os.path.join(CACHE, name)
    if os.path.exists(path) and not refresh:
        return path
    os.makedirs(CACHE, exist_ok=True)
    print(f'  downloading {name}', file=sys.stderr)
    req = urllib.request.Request(url, data=data, headers={'User-Agent': UA})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        body = r.read()
    with open(path + '.part', 'wb') as f:
        f.write(body)
    os.replace(path + '.part', path)
    return path


def export_image(service, bbox, size, fmt, extra=''):
    q = {
        'bbox': ','.join(str(v) for v in bbox), 'bboxSR': 26910, 'imageSR': 26910,
        'size': f'{size[0]},{size[1]}', 'format': fmt, 'f': 'image',
    }
    return service + '?' + urllib.parse.urlencode(q) + extra


def dem_raster(name, bbox, step, refresh):
    """A float32 DEM on a UTM grid, pixel centres at bbox edges + step/2."""
    w, h = round((bbox[2] - bbox[0]) / step), round((bbox[3] - bbox[1]) / step)
    url = export_image(DEM_SERVICE, bbox, (w, h), 'tiff',
                       '&pixelType=F32&interpolation=RSP_BilinearInterpolation&compression=None&noDataInterpretation=esriNoDataMatchAny')
    a = np.array(Image.open(fetch(name, url, refresh))).astype(np.float64)
    if not np.all(np.isfinite(a)) or a.min() < -100:
        raise SystemExit(f'{name}: DEM has holes')
    return a


# ── Small helpers ─────────────────────────────────────────────────────
def gauss_wrap(a, sigma):
    """Periodic gaussian smoothing along axis 0."""
    r = int(math.ceil(sigma * 3))
    k = np.exp(-np.arange(-r, r + 1) ** 2 / (2 * sigma * sigma))
    k /= k.sum()
    out = np.zeros_like(a, dtype=np.float64)
    for i, w in enumerate(k):
        out += w * np.roll(a, r - i, axis=0)
    return out


def resample_closed(pts, step):
    """Closed polyline → points every `step` metres (linear)."""
    p = np.vstack([pts, pts[:1]])
    d = np.hypot(*np.diff(p, axis=0).T)
    cum = np.concatenate([[0], np.cumsum(d)])
    n = int(round(cum[-1] / step))
    s = np.arange(n) * cum[-1] / n
    return np.stack([np.interp(s, cum, p[:, 0]), np.interp(s, cum, p[:, 1])], 1), cum[-1]


class Grid:
    """Bilinear sampling of a north-up raster in UTM metres."""

    def __init__(self, a, x0, y1, step):
        self.a, self.x0, self.y1, self.step = a, x0, y1, step

    def __call__(self, e, n):
        c = (np.asarray(e) - self.x0) / self.step - 0.5
        r = (self.y1 - np.asarray(n)) / self.step - 0.5
        H, W = self.a.shape[:2]
        c = np.clip(c, 0, W - 1.001)
        r = np.clip(r, 0, H - 1.001)
        i, j = np.floor(r).astype(int), np.floor(c).astype(int)
        fr, fc = r - i, c - j
        if self.a.ndim == 3:
            fr, fc = fr[..., None], fc[..., None]
        A = self.a
        return (A[i, j] * (1 - fr) * (1 - fc) + A[i, j + 1] * (1 - fr) * fc
                + A[i + 1, j] * fr * (1 - fc) + A[i + 1, j + 1] * fr * fc)


def pack(arr, dtype):
    """numpy array → zlib-deflated little-endian bytes."""
    raw = np.ascontiguousarray(arr.astype(dtype)).tobytes()
    return zlib.compress(raw, 9)


def ints(a):
    return [int(v) for v in a]


def js_ints(a):
    return '[' + ','.join(str(v) for v in ints(a)) + ']'


class Survey:
    """assets/seaside/survey.bin: every value circuit.js and ground.js hold,
    for the Rust port (mp_levels::seaside), so both games read the same
    numbers. Little-endian:

      magic "MRSURVEY", version u32 (1), section count u32, then per
      section: name (u8 length, UTF-8), type u8, count u32, data:
        1 f64 × count     2 i32 × count     3 bytes × count (a zlib stream)
        4 strings: count × (u32 length, UTF-8)
        5 i32 lists: count × (u32 length, i32 × length)

    A float is the value the JS module's decimal text parses to: the same
    Python float that text was printed from.
    """

    def __init__(self):
        self.parts = []

    def _add(self, name, kind, count, data):
        nb = name.encode()
        self.parts.append(bytes([len(nb)]) + nb + bytes([kind]) + int(count).to_bytes(4, 'little') + data)

    def f64(self, name, vals):
        self._add(name, 1, len(vals), np.array(vals, '<f8').tobytes())

    def i32(self, name, vals):
        self._add(name, 2, len(vals), np.array(vals, '<i4').tobytes())

    def zlib(self, name, data):
        self._add(name, 3, len(data), data)

    def strs(self, name, vals):
        self._add(name, 4, len(vals), b''.join(len(v.encode()).to_bytes(4, 'little') + v.encode() for v in vals))

    def lists(self, name, vals):
        self._add(name, 5, len(vals), b''.join(len(v).to_bytes(4, 'little') + np.array(v, '<i4').tobytes() for v in vals))

    def write(self, path):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'wb') as f:
            f.write(b'MRSURVEY' + (1).to_bytes(4, 'little') + len(self.parts).to_bytes(4, 'little'))
            for p in self.parts:
                f.write(p)


def median_wrap(a, r):
    """Periodic running median over ±r samples, ignoring NaNs."""
    st = np.stack([np.roll(a, k) for k in range(-r, r + 1)])
    with np.errstate(all='ignore'):
        import warnings
        with warnings.catch_warnings():
            warnings.simplefilter('ignore')
            return np.nanmedian(st, axis=0)


def fill_wrap(a):
    """Periodic linear interpolation over NaN gaps."""
    ok = np.isfinite(a)
    i = np.arange(len(a))
    return np.interp(i, np.concatenate([i[ok] - len(a), i[ok], i[ok] + len(a)]), np.tile(a[ok], 3))


def frames(c):
    """Unit tangents and driver's-right normals of a closed 1 m polyline (map frame)."""
    tan = np.roll(c, -2, 0) - np.roll(c, 2, 0)
    tan /= np.linalg.norm(tan, axis=1)[:, None]
    return tan, np.stack([tan[:, 1], -tan[:, 0]], 1)


def photo_edges(photo, c, right):
    """Where the tarmac ends on each side of every metre, from the photo:
    the first spot, 4 m or more out, that's coloured (dirt, grass, kerb
    paint) or much brighter than the middle of the road (the white edge
    line). NaN where neither shows within 17 m (paved run-off)."""
    lats = np.arange(-17, 17.01, 0.5)
    prof = np.stack([photo(c[:, 0] + right[:, 0] * l, c[:, 1] + right[:, 1] * l) for l in lats], 1)
    lum, chroma = prof.mean(2), prof.max(2) - prof.min(2)
    c0 = int(np.argmin(np.abs(lats)))
    base = np.median(lum[:, c0 - 6:c0 + 7], axis=1)
    hit = (chroma > 15) | (lum > base[:, None] + 30)
    out = []
    for d in (-1, 1):
        e = np.full(len(c), np.nan)
        ks = range(8, c0 + 1)
        for k in reversed(ks):  # nearest hit wins
            j = c0 + d * k
            e = np.where(hit[:, j], lats[j], e)
        out.append(e)
    return out


def classify_loose(rgb):
    """0 for paved ground (asphalt, concrete, paint), 1 for loose (dirt,
    gravel, grass, scrub), from photo colour. Asphalt and concrete are grey;
    the run-off's dirt is tan and the grass gold or green."""
    r, g, b = rgb[..., 0], rgb[..., 1], rgb[..., 2]
    lum = (r + g + b) / 3
    chroma = rgb.max(-1) - rgb.min(-1)
    grey = (chroma < 14) & (lum > 70)
    white = lum > 200
    teal = (g > r + 12) & (b > r + 8)  # the painted run-off strips
    return np.where(grey | white | teal, 0.0, 1.0)


# ── OSM ───────────────────────────────────────────────────────────────
def load_osm(refresh):
    lon0, lat0, lon1, lat1 = OSM_BBOX
    url = f'https://api.openstreetmap.org/api/0.6/map?bbox={lon0},{lat0},{lon1},{lat1}'
    root = ET.parse(fetch('map.osm', url, refresh)).getroot()
    nodes = {n.get('id'): ll2utm(float(n.get('lat')), float(n.get('lon'))) for n in root.iter('node')}
    ways = {}
    for w in root.iter('way'):
        tags = {t.get('k'): t.get('v') for t in w.iter('tag')}
        ways[w.get('id')] = {'id': w.get('id'), 'tags': tags, 'nodes': [nd.get('ref') for nd in w.iter('nd')]}
    rels = {}
    for r in root.iter('relation'):
        rels[r.get('id')] = [(m.get('type'), m.get('ref')) for m in r.iter('member')]
    return nodes, ways, rels


def centreline_utm(nodes, ways, rels):
    pts = []
    for typ, ref in rels[ROUTE_RELATION]:
        seq = [nodes[i] for i in ways[ref]['nodes']]
        if pts and pts[-1] == seq[0]:
            seq = seq[1:]
        pts += seq
    if pts[0] == pts[-1]:
        pts = pts[:-1]
    return np.array(pts)


# ── Main build ────────────────────────────────────────────────────────
def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--refresh', action='store_true', help='download everything again')
    args = ap.parse_args()
    R = args.refresh

    print('OpenStreetMap', file=sys.stderr)
    nodes, ways, rels = load_osm(R)

    def lap_from(nodes):
        # 1 m samples, smoothed so the OSM polyline's vertices don't kink
        # the curvature (σ 4 m barely changes a 15 m hairpin radius), and
        # rolled so the lap starts START_S metres before the line.
        c, _ = resample_closed(centreline_utm(nodes, ways, rels), 1.0)
        c, _ = resample_closed(gauss_wrap(c, 4.0), 1.0)
        return np.roll(c, -((START_LINE_S - START_S) % len(c)), axis=0)

    # The download areas come from OpenStreetMap's own lap, so they (and
    # the cache) don't move when the lap is moved onto the survey below.
    c1 = lap_from(nodes)
    minE, maxE = c1[:, 0].min(), c1[:, 0].max()
    minN, maxN = c1[:, 1].min(), c1[:, 1].max()
    # Fine grids: the circuit plus 400 m, for the road and the ground near it.
    fine_bbox = (math.floor((minE - 400) / 4) * 4, math.floor((minN - 400) / 4) * 4,
                 math.ceil((maxE + 400) / 4) * 4, math.ceil((maxN + 400) / 4) * 4)
    lidar_bbox = (math.floor(minE - 60), math.floor(minN - 60), math.ceil(maxE + 60), math.ceil(maxN + 60))
    cx, cy = (minE + maxE) / 2, (minN + maxN) / 2
    half = 3600
    wide_bbox = (math.floor((cx - half) / 16) * 16, math.floor((cy - half) / 16) * 16,
                 math.floor((cx - half) / 16) * 16 + 2 * half, math.floor((cy - half) / 16) * 16 + 2 * half)
    photo_bbox = (math.floor(minE - PHOTO_PAD), math.floor(minN - PHOTO_PAD), math.ceil(maxE + PHOTO_PAD), math.ceil(maxN + PHOTO_PAD))

    print('USGS NAIP', file=sys.stderr)
    nb = fine_bbox
    nw, nh = round((nb[2] - nb[0]) / 0.75), round((nb[3] - nb[1]) / 0.75)
    naip_fine = np.array(Image.open(fetch('naip_fine_075m.png', export_image(NAIP_SERVICE, nb, (nw, nh), 'png'), R)).convert('RGB')).astype(np.float64)
    pb = photo_bbox
    pw, phh = round((pb[2] - pb[0]) / PHOTO_STEP), round((pb[3] - pb[1]) / PHOTO_STEP)
    naip_photo = np.array(Image.open(fetch('naip_photo_06m.png', export_image(NAIP_SERVICE, pb, (pw, phh), 'png'), R)).convert('RGB')).astype(np.float64)
    PH = Grid(naip_photo, pb[0], pb[3], PHOTO_STEP)

    # OpenStreetMap was traced off other imagery and sits a little to one
    # side of the survey (the NAIP photo and the lidar agree to a few tens
    # of centimetres). Find the shift that centres the lap between the
    # photo's tarmac edges, and move every OSM node by it.
    tan, right = frames(c1)
    eL, eR = photo_edges(PH, c1, right)
    ok = np.isfinite(eL) & np.isfinite(eR) & (eR - eL > 9) & (eR - eL < 16)
    shift, *_ = np.linalg.lstsq(right[ok], ((eL + eR) / 2)[ok], rcond=None)
    nodes = {k: (e + shift[0], nn + shift[1]) for k, (e, nn) in nodes.items()}
    c1 = lap_from(nodes)
    print(f'  OpenStreetMap moved {shift[0]:+.2f} m east, {shift[1]:+.2f} m north onto the survey', file=sys.stderr)

    # Then the centreline itself: what's left of the offset from the middle
    # of the tarmac, metre by metre, smoothed over tens of metres (it's a
    # tracing error, not a wiggle in the road) and never more than 1.5 m.
    tan, right = frames(c1)
    eL, eR = photo_edges(PH, c1, right)
    ok = np.isfinite(eL) & np.isfinite(eR) & (eR - eL > 9) & (eR - eL < 16)
    off = np.where(ok, (eL + eR) / 2, np.nan)
    off = gauss_wrap(np.clip(fill_wrap(median_wrap(off, 15)), -1.5, 1.5), 10.0)
    c1 = c1 + right * off[:, None]
    c1, total = resample_closed(gauss_wrap(c1, 2.0), 1.0)
    n = len(c1)
    tan, right = frames(c1)
    print(f'  centreline {total:.1f} m, moved onto the photo\'s tarmac by up to {np.abs(off).max():.1f} m ({ok.mean() * 100:.0f} % of the lap measured)', file=sys.stderr)

    # The tarmac's width. Each edge counts where it's plausibly close
    # (paved run-off runs on with no edge to see, or shows one far out);
    # with only one, the road is taken as centred, which it now is.
    eL, eR = photo_edges(PH, c1, right)
    good = lambda e: np.where((np.abs(e) >= WIDTH_MIN / 2 - 0.5) & (np.abs(e) <= WIDTH_MAX / 2 + 0.25), np.abs(e), np.nan)
    hL, hR = good(eL), good(eR)
    wid = np.where(np.isfinite(hL) & np.isfinite(hR), hL + hR, 2 * np.where(np.isfinite(hL), hL, hR))
    wid = gauss_wrap(np.clip(fill_wrap(median_wrap(wid, 20)), WIDTH_MIN, WIDTH_MAX), 10.0)
    print(f'  tarmac {wid.min():.1f}..{wid.max():.1f} m wide (median {np.median(wid):.1f})', file=sys.stderr)
    E0, N0 = (round(v) for v in c1[START_S])

    print('USGS 3DEP', file=sys.stderr)
    lidar = dem_raster('dem_lidar_1m.tif', lidar_bbox, 1, R)
    L1 = Grid(lidar, lidar_bbox[0], lidar_bbox[3], 1)
    fine = dem_raster('dem_fine_4m.tif', fine_bbox, FINE_STEP, R)
    wide = dem_raster('dem_wide_16m.tif', wide_bbox, WIDE_STEP, R)

    # Road height and camber: the lidar across the tarmac at each metre.
    lats = np.arange(-4.5, 4.51, 0.75)
    prof = np.stack([L1(c1[:, 0] + right[:, 0] * l, c1[:, 1] + right[:, 1] * l) for l in lats], 1)
    y = np.median(prof, axis=1)
    # Least-squares cross slope (rise per metre toward the driver's right).
    lc = lats - lats.mean()
    slope = (prof - prof.mean(1, keepdims=True)) @ lc / (lc @ lc)
    y = gauss_wrap(y, 2.5)
    # Game bank: surface y = centre y - lat * bank, lat positive to the right.
    bank = gauss_wrap(-slope, 6.0)
    print(f'  road height {y.min():.1f}..{y.max():.1f} m (range {y.max() - y.min():.2f} m); camber {np.abs(bank).max():.3f} max', file=sys.stderr)

    # To the local frame.
    X = c1[:, 0] - E0
    Z = -(c1[:, 1] - N0)
    Y0 = float(round(y.min()) - 1)  # heights stored above this

    # ── circuit.js ───────────────────────────────────────────────────
    def local(pts):
        return [(round((e - E0) * 10), round(-(nn - N0) * 10)) for e, nn in pts]

    def flat(pts):
        return js_ints([v for p in local(pts) for v in p])

    def way_pts(w):
        return [nodes[i] for i in w['nodes'] if i in nodes]

    def s_of(e, nn):
        d = np.hypot(c1[:, 0] - e, c1[:, 1] - nn)
        k = int(d.argmin())
        return k, float(d[k])

    near = lambda pts, r: any(s_of(*p)[1] < r for p in pts)
    feats = {'walls': [], 'buildings': [], 'stands': [], 'bridges': [], 'water': [], 'parking': [], 'roads': [], 'pit': None}
    for w in ways.values():
        t, pts = w['tags'], way_pts(w)
        if len(pts) < 2:
            continue
        if w['id'] == PIT_LANE_WAY:
            feats['pit'] = pts
        elif t.get('barrier') == 'wall' and near(pts, 160):
            feats['walls'].append(pts)
        elif t.get('building') == 'grandstand':
            feats['stands'].append(pts)
        elif 'building' in t and near(pts, 400):
            feats['buildings'].append((t.get('name', ''), pts))
        elif t.get('bridge') == 'yes' and near(pts, 60):
            feats['bridges'].append((t.get('highway', ''), pts))
        elif t.get('natural') == 'water' and near(pts, 400):
            feats['water'].append(pts)
        elif t.get('amenity') == 'parking' and near(pts, 400):
            feats['parking'].append(pts)
        elif t.get('highway') in ('service', 'unclassified', 'track', 'residential', 'footway', 'path') and near(pts, 700):
            feats['roads'].append((t['highway'], pts))

    # Barriers: how far the nearest OSM wall is on each side of every metre,
    # cast along the normal. Where there's none (open run-off) or it's very
    # far, the game builds a tyre wall at WALL_MAX. On the inside of a tight
    # corner the distance can't pass the centre of curvature.
    segs = []
    for pts in feats['walls']:
        segs += [(pts[i], pts[i + 1]) for i in range(len(pts) - 1)]
    A = np.array([a for a, b in segs])
    B = np.array([b for a, b in segs])
    hd = np.arctan2(tan[:, 1], tan[:, 0])
    kap = gauss_wrap(np.angle(np.exp(1j * (np.roll(hd, -1) - np.roll(hd, 1)))) / 2, 6.0)
    walls = np.full((n, 2), WALL_MAX)
    for side, sgn in ((0, -1), (1, 1)):
        o = c1
        dvec = right * sgn
        # Ray o + t d against segments A + u (B - A).
        e = B - A
        for i in range(n):
            d = dvec[i]
            den = d[0] * e[:, 1] - d[1] * e[:, 0]
            ok = np.abs(den) > 1e-9
            w = A - o[i]
            t = np.where(ok, (w[:, 0] * e[:, 1] - w[:, 1] * e[:, 0]) / np.where(ok, den, 1), np.inf)
            u = np.where(ok, (w[:, 0] * d[1] - w[:, 1] * d[0]) / np.where(ok, den, 1), -1)
            hit = ok & (t > 2) & (u >= 0) & (u <= 1)
            if hit.any():
                walls[i, side] = min(WALL_MAX, t[hit].min())
    # Driver's left is +curvature's inside for a left-hander (map frame is
    # counter-clockwise positive): inside = left when kap > 0.
    rad = 1 / np.maximum(np.abs(kap), 1e-6)
    inside = np.where(kap > 0, 0, 1)
    for i in range(n):
        walls[i, inside[i]] = min(walls[i, inside[i]], rad[i] * 0.8)
    # A wall that comes and goes for a metre or two (a gap, a gate) would
    # jolt the corridor: take the local minimum over ±6 m, then smooth.
    for side in (0, 1):
        m = np.min(np.stack([np.roll(walls[:, side], k) for k in range(-6, 7)]), axis=0)
        walls[:, side] = gauss_wrap(m, 3.0)
    print(f'  walls: left {walls[:, 0].min():.1f}..{walls[:, 0].max():.1f} m, right {walls[:, 1].min():.1f}..{walls[:, 1].max():.1f} m', file=sys.stderr)

    # Run-off: the ground's grade outward from the tarmac edge on each side
    # (rise per metre away from the track), fitted to the lidar between the
    # edge and the barrier.
    runoff = np.zeros((n, 2))
    for side, sgn in ((0, -1), (1, 1)):
        for i in range(n):
            far = min(walls[i, side] - 1.0, 26.0)
            if far < 9:
                continue
            ls = np.arange(7.0, far + 0.01, 1.5)
            p = c1[i][None, :] + right[i][None, :] * (sgn * ls)[:, None]
            h = L1(p[:, 0], p[:, 1])
            lc = ls - ls.mean()
            runoff[i, side] = float(((h - h.mean()) @ lc) / (lc @ lc))
        runoff[:, side] = gauss_wrap(np.clip(runoff[:, side], -0.25, 0.25), 5.0)
    print(f'  run-off grade: left {runoff[:, 0].min():.2f}..{runoff[:, 0].max():.2f}, right {runoff[:, 1].min():.2f}..{runoff[:, 1].max():.2f}', file=sys.stderr)

    step = 2
    idx = np.arange(0, n, step)
    line = {
        'x': ints(np.round(X[idx] * 100)),
        'z': ints(np.round(Z[idx] * 100)),
        'y': ints(np.round((y[idx] - Y0) * 100)),
        'bank': ints(np.round(bank[idx] * 10000)),
        'hw': ints(np.round(wid[idx] * 50)),
        'wallL': ints(np.round(walls[idx, 0] * 10)),
        'wallR': ints(np.round(walls[idx, 1] * 10)),
        'runL': ints(np.round(runoff[idx, 0] * 1000)),
        'runR': ints(np.round(runoff[idx, 1] * 1000)),
    }
    flat_ints = lambda pts: [v for p in local(pts) for v in p]
    photo = (pb[0] - E0, -(pb[3] - N0), pb[2] - E0, -(pb[1] - N0), pw, phh)
    sv = Survey()
    sv.f64('origin', [E0, N0, 10])
    sv.f64('lap', [float(f'{total:.2f}')])
    sv.f64('startS', [START_S])
    sv.f64('line.step', [step])
    sv.f64('line.y0', [float(f'{Y0:.0f}')])
    for k, v in line.items():
        sv.i32('line.' + k, v)
    sv.i32('pitLane', flat_ints(feats['pit']))
    sv.lists('walls', [flat_ints(p) for p in feats['walls']])
    sv.lists('grandstands', [flat_ints(p) for p in feats['stands']])
    sv.strs('buildings.name', [nm for nm, p in feats['buildings']])
    sv.lists('buildings.pts', [flat_ints(p) for nm, p in feats['buildings']])
    sv.strs('bridges.kind', [k for k, p in feats['bridges']])
    sv.lists('bridges.pts', [flat_ints(p) for k, p in feats['bridges']])
    sv.lists('water', [flat_ints(p) for p in feats['water']])
    sv.lists('parking', [flat_ints(p) for p in feats['parking']])
    sv.strs('paths.kind', [k for k, p in feats['roads']])
    sv.lists('paths.pts', [flat_ints(p) for k, p in feats['roads']])
    sv.strs('photo.file', ['photo.jpg'])
    sv.f64('photo', photo)
    lines = [
        '// Generated by tools/seaside/build.py. Do not edit: run the script.',
        '//',
        '// Seaside Raceway is Laguna Seca. Centreline: OpenStreetMap route relation',
        f'// {ROUTE_RELATION} ((c) OpenStreetMap contributors, ODbL). Heights and camber:',
        '// USGS 3DEP 1 m bare-earth lidar DEM (CA_AZ_FEMA_R9_Lidar_2017_D18, public domain).',
        '//',
        '// Local frame: metres, x east, z south, from the start/finish line.',
        f'// UTM zone 10N of the origin: E {E0}, N {N0}.',
        '',
        f'export const ORIGIN = {{ utmE: {E0}, utmN: {N0}, zone: 10 }};',
        f'export const LAP = {total:.2f}; // metres round the centreline',
        f'export const START_S = {START_S};',
        '',
        '// Centreline every 2 m from the start of the lap: x, z, height and',
        '// bank (surface y = y - lat * bank), all in centimetres (bank in 1/10000),',
        '// half the tarmac\'s width, how far the barriers are on each side, and the',
        '// run-off grade out to them.',
        f'export const LINE = {{ step: {step}, y0: {Y0:.0f},',
        f'  x: {js_ints(line["x"])},',
        f'  z: {js_ints(line["z"])},',
        f'  y: {js_ints(line["y"])},',
        f'  bank: {js_ints(line["bank"])},',
        f'  hw: {js_ints(line["hw"])}, // half the tarmac\'s width, measured off the photo',
        f'  wallL: {js_ints(line["wallL"])}, // decimetres, left of the centreline',
        f'  wallR: {js_ints(line["wallR"])},',
        f'  runL: {js_ints(line["runL"])}, // run-off grade outward, 1/1000',
        f'  runR: {js_ints(line["runR"])},',
        '};',
        '',
        '// Everything below is polylines/polygons as flat [x, z, x, z, ...] lists in',
        '// decimetres, from OpenStreetMap.',
        f'export const PIT_LANE = {flat(feats["pit"])};',
        'export const WALLS = [',
        *[f'  {flat(p)},' for p in feats['walls']],
        '];',
        'export const GRANDSTANDS = [',
        *[f'  {flat(p)},' for p in feats['stands']],
        '];',
        'export const BUILDINGS = [',
        *[f'  {{ name: {json.dumps(nm)}, pts: {flat(p)} }},' for nm, p in feats['buildings']],
        '];',
        'export const BRIDGES = [',
        *[f'  {{ kind: {json.dumps(k)}, pts: {flat(p)} }},' for k, p in feats['bridges']],
        '];',
        'export const WATER = [',
        *[f'  {flat(p)},' for p in feats['water']],
        '];',
        'export const PARKING = [',
        *[f'  {flat(p)},' for p in feats['parking']],
        '];',
        'export const PATHS = [',
        *[f'  {{ kind: {json.dumps(k)}, pts: {flat(p)} }},' for k, p in feats['roads']],
        '];',
        '',
        '// The aerial photo draped over the ground (photo.jpg, USGS NAIP), graded',
        '// for the game: its edges in the local frame (x0, z0 the north-west corner).',
        f"export const PHOTO = {{ file: 'photo.jpg', x0: {photo[0]}, z0: {photo[1]}, x1: {photo[2]}, z1: {photo[3]}, w: {photo[4]}, h: {photo[5]} }};",
        '',
    ]
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, 'circuit.js'), 'w') as f:
        f.write('\n'.join(lines))

    # ── ground.js ────────────────────────────────────────────────────
    # Photos: 3 m over the wide area, 0.75 m near the circuit (above).
    img_bbox = (wide_bbox[0], wide_bbox[1], wide_bbox[2], wide_bbox[3])
    iw = round((img_bbox[2] - img_bbox[0]) / 3)
    naip_wide = np.array(Image.open(fetch('naip_wide_3m.png', export_image(NAIP_SERVICE, img_bbox, (iw, iw), 'png'), R)).convert('RGB')).astype(np.float64)

    def block_mean(a, k):
        H, W = a.shape[0] // k * k, a.shape[1] // k * k
        a = a[:H, :W]
        return a.reshape(H // k, k, W // k, k, -1).mean((1, 3))

    # Oak canopy: dark, greenish pixels (summer grass is pale gold, paving
    # grey, bare ground pale tan; the chaparral is mid olive).
    def canopy(a):
        r, g, b = a[..., 0], a[..., 1], a[..., 2]
        lum = (r + g + b) / 3
        return ((lum < 92) & (g >= r * 0.92) & (g > b * 1.02)).astype(np.float64)

    # Colour (8 m near the circuit, 32 m beyond) and oak canopy cover (4 m /
    # 32 m). The photo is resized to whole metres first so cells line up
    # exactly with the DEM grids.
    fw, fh = round(nb[2] - nb[0]), round(nb[3] - nb[1])
    photo1 = np.array(Image.fromarray(naip_fine.astype(np.uint8)).resize((fw, fh), Image.BOX)).astype(np.float64)
    W4 = round((img_bbox[2] - img_bbox[0]) / 4)
    photo4 = np.array(Image.fromarray(naip_wide.astype(np.uint8)).resize((W4, W4), Image.BOX)).astype(np.float64)
    fine_col = block_mean(photo1, 8)
    fine_cov = block_mean(canopy(photo1)[..., None], FINE_STEP)[..., 0]
    wide_col = block_mean(photo4, 32 // 4)
    wide_cov = block_mean(canopy(photo4)[..., None], 32 // 4)[..., 0]

    base = math.floor(min(fine.min(), wide.min())) - 1

    def grid(name, bbox, step, a, kind, **kw):
        """One grid: x0/z0 are the local coords of the north-west sample."""
        h, w = a.shape[:2]
        x0, z0 = bbox[0] + step / 2 - E0, -(bbox[3] - step / 2 - N0)
        meta = f'x0: {x0}, z0: {z0}, step: {step}, w: {w}, h: {h}'
        if kind == 'height':
            # Quantised heights, then 2D deltas (row-wise, then down the
            # columns) as int16: smooth ground deflates far better.
            qh = np.round((a - base) / kw['q']).astype(np.int32)
            dx = np.diff(qh, axis=1, prepend=0)
            dd = np.diff(dx, axis=0, prepend=0)
            data = pack(dd, '<i2')
            meta += f", kind: 'height', q: {kw['q']}"
            gkind = 'height'
        elif kind == 'rgb':
            # 5 bits a channel, one plane per channel.
            data = pack(np.clip(np.round(a / 8.226), 0, 31).transpose(2, 0, 1), 'u1')
            meta += ", kind: 'rgb5'"
            gkind = 'rgb5'
        else:
            data = pack(np.clip(np.round(a * 255), 0, 255), 'u1')
            meta += ", kind: 'cover'"
            gkind = 'cover'
        sv.strs(name + '.kind', [gkind])
        sv.f64(name, [x0, z0, step, w, h, kw.get('q', 0)])
        sv.zlib(name + '.data', data)
        data = base64.b64encode(data).decode('ascii')
        return f"export const {name} = {{ {meta},\n  data: '{data}' }};"

    # The draped photo, graded as the terrain's vertex colours are (see
    # TerrainColorizer.raceway): the summer haze warmed and the colour
    # richer and a little darker for the game's sun. Stored as sRGB.
    lin = (naip_photo / 255) ** 2.2
    lin[..., 0] *= 1.04
    lin[..., 2] *= 0.82
    lum = lin.mean(-1, keepdims=True)
    lin = np.maximum(0, lum + (lin - lum) * 1.55) * 0.74
    srgb = np.where(lin <= 0.0031308, lin * 12.92, 1.055 * np.power(np.clip(lin, 0.0031308, None), 1 / 2.4) - 0.055)
    Image.fromarray(np.clip(np.round(srgb * 255), 0, 255).astype(np.uint8)).save(os.path.join(OUT, 'photo.jpg'), quality=82, optimize=True, progressive=True)

    # Paved or loose: 0 on asphalt and concrete, 1 on dirt and grass, at 1 m,
    # out to 50 m from the lap (no car gets further). Off the tarmac the
    # car only slows on the loose stuff.
    lb_ = photo_bbox
    lw, lh = lb_[2] - lb_[0], lb_[3] - lb_[1]
    cls = Image.fromarray((classify_loose(naip_photo) * 255).astype(np.uint8)).resize((lw, lh), Image.BOX)
    loose = np.array(cls).astype(np.float64) / 255
    # Speckle out: a 3x3 box, then back to clean paved/loose with a soft edge.
    k = np.ones(3) / 3
    for ax in (0, 1):
        loose = np.apply_along_axis(lambda r: np.convolve(np.pad(r, 1, mode='edge'), k, 'valid'), ax, loose)
    loose = np.clip((loose - 0.5) * 3 + 0.5, 0, 1)
    near_lap = np.zeros((lh, lw), bool)
    for e, nn in c1[::3]:
        j, i = int(e - lb_[0]), int(lb_[3] - nn)
        near_lap[max(0, i - 50):i + 51, max(0, j - 50):j + 51] = True
    loose[~near_lap] = 1
    print(f'  run-off: {(1 - loose[near_lap]).mean() * 100:.0f} % of the ground within 50 m of the lap is paved', file=sys.stderr)

    sv.f64('base', [base])
    g = [
        '// Generated by tools/seaside/build.py. Do not edit: run the script.',
        '//',
        '// The ground round Seaside Raceway (Laguna Seca): USGS 3DEP bare-earth',
        '// heights, and colour and oak canopy from USGS NAIP aerial photos (both',
        '// public domain). Each grid is row-major from its north-west sample',
        '// (x0, z0 in the local frame of circuit.js), zlib-deflated, base64:',
        '//   height: int16 2D deltas of heights in steps of q metres above BASE',
        '//   rgb5:   photo colour, 0..31 a channel, R plane then G then B',
        '//   cover:  oak canopy cover, or loose ground (dirt, grass) as against',
        '//           paved, 0..255',
        '',
        f'export const BASE = {base};',
        grid('HEIGHT_FINE', fine_bbox, FINE_STEP, fine, 'height', q=0.05),
        grid('HEIGHT_WIDE', wide_bbox, WIDE_STEP, wide, 'height', q=0.2),
        grid('COLOR_FINE', fine_bbox, 8, fine_col, 'rgb'),
        grid('COLOR_WIDE', wide_bbox, 32, wide_col, 'rgb'),
        grid('TREES_FINE', fine_bbox, FINE_STEP, fine_cov, 'cover'),
        grid('TREES_WIDE', wide_bbox, 32, wide_cov, 'cover'),
        grid('LOOSE', photo_bbox, 1, loose, 'cover'),
        '',
    ]
    with open(os.path.join(OUT, 'ground.js'), 'w') as f:
        f.write('\n'.join(g))
    for fn in ('circuit.js', 'ground.js', 'photo.jpg'):
        print(f'  wrote src/levels/seaside/{fn} ({os.path.getsize(os.path.join(OUT, fn)) / 1024:.0f} KB)', file=sys.stderr)
    sv.write(SURVEY)
    print(f'  wrote {os.path.relpath(SURVEY, ROOT)} ({os.path.getsize(SURVEY) / 1024:.0f} KB)', file=sys.stderr)


if __name__ == '__main__':
    main()
