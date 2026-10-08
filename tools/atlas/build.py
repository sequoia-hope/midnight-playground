#!/usr/bin/env python3
"""The atlas: real geography for planning the open world (docs/vision/atlas.md).

Downloads the San Mateo peninsula's elevation, land cover, water, parks,
roads, railways and town names, caches them in tools/atlas/cache/, and
writes what the planner and mp_atlas read:

  assets/atlas/peninsula/terrain.bin  a grid over the whole box: height
                                      (decimetres) and land cover per cell
  assets/atlas/peninsula/geo.json     the projection, the sources, the road
                                      graph (edges between junctions, with
                                      class, name and route number), the
                                      railways, parks and towns

The plan itself (regions, routes, places, events) is written by hand in
assets/atlas/peninsula/plan.json; this script never touches it.

Sources (all free to use; geo.json carries the attribution):
  Mapzen/Tilezen terrain tiles on AWS Open Data (Terrarium encoding): a
    blend of USGS 3DEP (10 m in the US), SRTM and bathymetry. Public domain
    and open licences; see https://github.com/tilezen/joerd.
  Overture Maps Foundation, release OVERTURE_RELEASE, on AWS Open Data:
    transportation (from OpenStreetMap, ODbL), base land cover (ESA
    WorldCover, CC BY 4.0), land use and water (OpenStreetMap, ODbL),
    divisions (OpenStreetMap and others).

Local frame, as the levels: x east, z south (y up), metres from ORIGIN, Half
Moon Bay's main intersection (Highway 1 at Highway 92). The projection is
equirectangular about the origin's latitude; across the box (70 km north to
south) east-west distances are off by at most 0.6 % at the top and bottom
edges, which planning does not notice. mp_atlas::Projection is the same.

Needs python3 with numpy, Pillow, shapely and pyarrow, and network access on
the first run.
  python3 tools/atlas/build.py            # use the cache where it can
  python3 tools/atlas/build.py --refresh  # download everything again
"""

import argparse
import io
import json
import math
import os
import struct
import sys
import time
import urllib.error
import urllib.request

import numpy as np
from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CACHE = os.path.join(ROOT, 'tools', 'atlas', 'cache')
OUT = os.path.join(ROOT, 'assets', 'atlas', 'peninsula')

# The box: Pescadero and Big Basin's edge in the south to the Golden Gate in
# the north, the ocean in the west to the bay shore in the east.
LON0, LON1 = -122.56, -122.08
LAT0, LAT1 = 37.18, 37.84
# Half Moon Bay: Highway 1 at Highway 92.
ORIGIN = (37.46802, -122.43350)  # lat, lon
# The terrain grid's spacing (m). Planning needs the shape of the land, not
# its detail; corridor generation will read the cache's full resolution.
STEP = 50.0
# Terrarium tiles at zoom 13: about 15 m a pixel at this latitude.
DEM_ZOOM = 13
DEM_URL = 'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png'
OVERTURE_RELEASE = '2026-09-23.1'
OVERTURE = f'overturemaps-us-west-2/release/{OVERTURE_RELEASE}/'

# Land cover classes in terrain.bin (mp_atlas::Cover has the same numbers).
SEA, WATER, URBAN, CROP, GRASS, SHRUB, FOREST, BARREN, WETLAND, SAND = range(10)
COVER_NAMES = ['sea', 'water', 'urban', 'crop', 'grass', 'shrub', 'forest',
               'barren', 'wetland', 'sand']
OVERTURE_COVER = {'urban': URBAN, 'crop': CROP, 'grass': GRASS, 'shrub': SHRUB,
                  'forest': FOREST, 'barren': BARREN, 'wetland': WETLAND,
                  'mangrove': WETLAND, 'moss': GRASS, 'snow': BARREN}

# Roads the planner keeps: everything from tertiary up. Residential streets
# and tracks wait for corridor generation.
ROAD_CLASSES = ['motorway', 'trunk', 'primary', 'secondary', 'tertiary']
# Simplification tolerance for road and rail lines (m).
LINE_TOL = 4.0
# Parks and protected land smaller than this are left out (m^2).
AREA_MIN = 400_000.0
# The deepest sea floor kept (m).
SEA_FLOOR = -120.0


# ── Projection ──────────────────────────────────────────────────────

def metres_per_degree(lat):
    p = math.radians(lat)
    k_lat = 111132.954 - 559.822 * math.cos(2 * p) + 1.175 * math.cos(4 * p)
    k_lon = 111412.84 * math.cos(p) - 93.5 * math.cos(3 * p)
    return k_lat, k_lon


K_LAT, K_LON = metres_per_degree(ORIGIN[0])


def to_local(lon, lat):
    """(lon, lat) to (x east, z south) metres from the origin."""
    return (lon - ORIGIN[1]) * K_LON, -(lat - ORIGIN[0]) * K_LAT


def to_lonlat(x, z):
    return ORIGIN[1] + x / K_LON, ORIGIN[0] - z / K_LAT


# The grid: cell (i, j) centres at (X0 + i STEP, Z0 + j STEP), j = 0 north.
X0, Z0 = to_local(LON0, LAT1)
X0, Z0 = math.ceil(X0 / STEP) * STEP, math.ceil(Z0 / STEP) * STEP
XE, ZE = to_local(LON1, LAT0)
W = int((XE - X0) // STEP) + 1
H = int((ZE - Z0) // STEP) + 1


# ── Downloads ───────────────────────────────────────────────────────

def fetch(url, path, refresh):
    if os.path.exists(path) and not refresh:
        with open(path, 'rb') as f:
            return f.read()
    req = urllib.request.Request(url, headers={'User-Agent': 'midnight-playground-atlas/1.0'})
    # The tile bucket answers a spurious 404 now and then for tiles that
    # exist: try a few times before giving up.
    for attempt in range(6):
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                data = r.read()
            break
        except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError) as e:
            if attempt == 5:
                raise
            print(f'  retry {url}: {e}', file=sys.stderr)
            time.sleep(1 + attempt * 2)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path + '.part', 'wb') as f:
        f.write(data)
    os.replace(path + '.part', path)
    return data


def tile_xy(lon, lat, z):
    n = 2 ** z
    x = (lon + 180) / 360 * n
    y = (1 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2 * n
    return x, y


def heights(refresh):
    """The grid's heights (m), bilinear from the Terrarium mosaic."""
    tx0, ty0 = tile_xy(LON0, LAT1, DEM_ZOOM)
    tx1, ty1 = tile_xy(LON1, LAT0, DEM_ZOOM)
    tx0, ty0, tx1, ty1 = int(tx0), int(ty0), int(tx1), int(ty1)
    nx, ny = tx1 - tx0 + 1, ty1 - ty0 + 1
    mosaic = np.zeros((ny * 256, nx * 256), dtype=np.float64)
    print(f'terrain: {nx * ny} tiles at zoom {DEM_ZOOM}', file=sys.stderr)
    for ty in range(ty0, ty1 + 1):
        for tx in range(tx0, tx1 + 1):
            url = DEM_URL.format(z=DEM_ZOOM, x=tx, y=ty)
            path = os.path.join(CACHE, 'terrarium', str(DEM_ZOOM), f'{tx}_{ty}.png')
            png = Image.open(io.BytesIO(fetch(url, path, refresh))).convert('RGB')
            a = np.asarray(png, dtype=np.float64)
            h = a[:, :, 0] * 256 + a[:, :, 1] + a[:, :, 2] / 256 - 32768
            oy, ox = (ty - ty0) * 256, (tx - tx0) * 256
            mosaic[oy:oy + 256, ox:ox + 256] = h
    out = np.zeros((H, W), dtype=np.float64)
    xs = X0 + np.arange(W) * STEP
    for j in range(H):
        z = Z0 + j * STEP
        # A row of the grid is one latitude.
        lon, lat = to_lonlat(xs, z)
        n = 2 ** DEM_ZOOM
        px = ((lon + 180) / 360 * n - tx0) * 256 - 0.5
        py = ((1 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2 * n - ty0) * 256 - 0.5
        i0, j0 = np.floor(px).astype(int), int(math.floor(py))
        fx, fy = px - i0, py - j0
        r0, r1 = mosaic[j0], mosaic[j0 + 1]
        top = r0[i0] * (1 - fx) + r0[i0 + 1] * fx
        bot = r1[i0] * (1 - fx) + r1[i0 + 1] * fx
        out[j] = top * (1 - fy) + bot * fy
    return out


def overture(theme_type, refresh, columns):
    """Rows of one Overture type inside the box, cached as parquet."""
    import pyarrow.compute as pc
    import pyarrow.dataset as ds
    import pyarrow.fs as pafs
    import pyarrow.parquet as pq
    name = theme_type.replace('theme=', '').replace('/type=', '-').strip('/')
    path = os.path.join(CACHE, 'overture', OVERTURE_RELEASE, name + '.parquet')
    if not os.path.exists(path) or refresh:
        print(f'overture: {name}', file=sys.stderr)
        s3 = pafs.S3FileSystem(anonymous=True, region='us-west-2')
        d = ds.dataset(OVERTURE + theme_type, filesystem=s3, format='parquet')
        f = ((pc.field('bbox', 'xmin') < LON1) & (pc.field('bbox', 'xmax') > LON0)
             & (pc.field('bbox', 'ymin') < LAT1) & (pc.field('bbox', 'ymax') > LAT0))
        cols = [c for c in columns + ['geometry'] if c in d.schema.names]
        t = d.to_table(filter=f, columns=cols)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        pq.write_table(t, path + '.part')
        os.replace(path + '.part', path)
    return pq.read_table(path).to_pylist()


# ── Geometry ────────────────────────────────────────────────────────

def local_geom(g):
    """A shapely geometry in lon/lat, clipped to the box, in local metres."""
    from shapely import affinity, box
    g = g.intersection(box(LON0, LAT0, LON1, LAT1))
    if g.is_empty:
        return g
    # x = (lon - lon0) K_LON, z = -(lat - lat0) K_LAT
    return affinity.affine_transform(
        g, [K_LON, 0, 0, -K_LAT, -ORIGIN[1] * K_LON, ORIGIN[0] * K_LAT])


def polygons(g):
    if g.is_empty:
        return []
    if g.geom_type == 'Polygon':
        return [g]
    if g.geom_type in ('MultiPolygon', 'GeometryCollection'):
        return [p for q in g.geoms for p in polygons(q)]
    return []


def lines(g):
    if g.is_empty:
        return []
    if g.geom_type == 'LineString':
        return [g]
    if g.geom_type in ('MultiLineString', 'GeometryCollection'):
        return [p for q in g.geoms for p in lines(q)]
    return []


def grid_xy(coords):
    return [((x - X0) / STEP + 0.5, (z - Z0) / STEP + 0.5) for x, z in coords]


def paint(cover, poly, cls):
    """Set the cells whose centres fall inside poly (holes respected)."""
    mask = Image.new('1', (W, H), 0)
    d = ImageDraw.Draw(mask)
    d.polygon(grid_xy(poly.exterior.coords), fill=1)
    for hole in poly.interiors:
        d.polygon(grid_xy(hole.coords), fill=0)
    cover[np.asarray(mask, dtype=bool)] = cls


def r1(v):
    return round(v, 1)


# ── Build ───────────────────────────────────────────────────────────

def build_cover(height, refresh):
    from shapely import wkb
    cover = np.full((H, W), GRASS, dtype=np.uint8)
    # Land cover, in Overture's drawing order; only the detailed versions.
    rows = overture('theme=base/type=land_cover/', refresh, ['subtype', 'cartography'])
    rows = [r for r in rows if (r['cartography'] or {}).get('min_zoom', 0) >= 8]
    rows.sort(key=lambda r: (r['cartography'] or {}).get('sort_key') or 0)
    for r in rows:
        cls = OVERTURE_COVER.get(r['subtype'])
        if cls is None:
            continue
        for p in polygons(local_geom(wkb.loads(r['geometry']))):
            paint(cover, p, cls)
    # Farmland and orchards from the map, over the satellite's classes.
    use = overture('theme=base/type=land_use/', refresh, ['subtype', 'class', 'names'])
    for r in use:
        if r['subtype'] == 'agriculture' and r['class'] in (
                'farmland', 'orchard', 'vineyard', 'meadow', 'plant_nursery'):
            for p in polygons(local_geom(wkb.loads(r['geometry']))):
                paint(cover, p, CROP)
        elif r['class'] == 'beach':
            for p in polygons(local_geom(wkb.loads(r['geometry']))):
                paint(cover, p, SAND)
    # Water: lakes and reservoirs, then the sea and the bay.
    water = overture('theme=base/type=water/', refresh, ['subtype', 'class', 'names'])
    for r in water:
        if r['subtype'] in ('lake', 'reservoir', 'pond', 'water') and r['class'] not in (
                'swimming_pool', 'fountain'):
            for p in polygons(local_geom(wkb.loads(r['geometry']))):
                if p.area > 20_000:
                    paint(cover, p, WATER)
    for r in water:
        if r['subtype'] == 'ocean' or r['class'] in ('bay', 'sea', 'ocean', 'strait'):
            for p in polygons(local_geom(wkb.loads(r['geometry']))):
                paint(cover, p, SEA)
    return cover, use, water


def build_roads(refresh):
    """The road graph: edges between connectors, simplified, in local metres."""
    from shapely import wkb
    from shapely.geometry import LineString
    from shapely.ops import substring
    rows = overture('theme=transportation/type=segment/', refresh,
                    ['id', 'subtype', 'class', 'names', 'routes', 'connectors', 'road_flags'])
    nodes = {}

    def node(cid, pt):
        if cid not in nodes:
            nodes[cid] = (len(nodes), pt)
        return nodes[cid][0]

    edges, rail = [], []
    for r in rows:
        if r['subtype'] == 'rail':
            if r['class'] in ('standard_gauge', 'light_rail', 'subway', 'tram', 'narrow_gauge'):
                for ln in lines(local_geom(wkb.loads(r['geometry']))):
                    s = ln.simplify(LINE_TOL)
                    name = (r['names'] or {}).get('primary')
                    rail.append({'class': r['class'], 'name': name,
                                 'pts': [[r1(x), r1(z)] for x, z in s.coords]})
            continue
        if r['subtype'] != 'road' or r['class'] not in ROAD_CLASSES:
            continue
        g = wkb.loads(r['geometry'])
        name = (r['names'] or {}).get('primary')
        refs = sorted({rt['ref'] for rt in (r['routes'] or []) if rt.get('ref')})
        flags = set()
        for f in r['road_flags'] or []:
            flags.update(f.get('values') or [])
        line = local_geom(g)
        if line.is_empty or line.geom_type != 'LineString':
            # Clipped into pieces by the box's edge: keep each piece, no junctions.
            for ln in lines(line):
                s = ln.simplify(LINE_TOL)
                a = node(f'edge:{r["id"]}:a:{len(edges)}', s.coords[0])
                b = node(f'edge:{r["id"]}:b:{len(edges)}', s.coords[-1])
                edges.append((a, b, r['class'], name, refs, flags, list(s.coords)))
            continue
        # Split at the connectors (junctions) along the segment.
        full = LineString(local_geom(g).coords)
        cons = sorted((c['at'], c['connector_id']) for c in (r['connectors'] or []))
        if not cons or cons[0][0] > 1e-9:
            cons.insert(0, (0.0, f'end:{r["id"]}:0'))
        if cons[-1][0] < 1 - 1e-9:
            cons.append((1.0, f'end:{r["id"]}:1'))
        for (a0, ca), (a1, cb) in zip(cons, cons[1:]):
            if a1 - a0 < 1e-12:
                continue
            piece = substring(full, a0, a1, normalized=True)
            if piece.geom_type != 'LineString' or len(piece.coords) < 2:
                continue
            s = piece.simplify(LINE_TOL)
            a = node(ca, s.coords[0])
            b = node(cb, s.coords[-1])
            edges.append((a, b, r['class'], name, refs, flags, list(s.coords)))
    node_pts = [None] * len(nodes)
    for i, pt in nodes.values():
        node_pts[i] = pt
    edges = merge_chains(edges)
    # Keep only the nodes the roads use, numbered in first use.
    renum = {}
    for e in edges:
        for k in (0, 1):
            renum.setdefault(e[k], len(renum))
    used = [None] * len(renum)
    for old_id, new_id in renum.items():
        used[new_id] = [round(node_pts[old_id][0]), round(node_pts[old_id][1])]
    out = []
    for a, b, cls, name, refs, flags, pts in edges:
        e = {'a': renum[a], 'b': renum[b], 'class': cls,
             'pts': [[round(x), round(z)] for x, z in pts]}
        if name:
            e['name'] = name
        if refs:
            e['refs'] = refs
        for f in ('is_bridge', 'is_tunnel'):
            if f in flags:
                e[f[3:]] = True
        out.append(e)
    return used, out, merge_rail(rail)


def merge_chains(edges):
    """Joins roads that meet only each other (a junction with a residential
    street the planner leaves out, a change of surface) into one road, when
    they are the same road: class, name, route numbers, bridge and tunnel."""
    from shapely.geometry import LineString
    key = lambda e: (e[2], e[3], tuple(e[4]), tuple(sorted(e[5] & {'is_bridge', 'is_tunnel'})))
    at = {}
    for i, e in enumerate(edges):
        at.setdefault(e[0], []).append(i)
        at.setdefault(e[1], []).append(i)
    alive = [True] * len(edges)
    edges = [list(e) for e in edges]
    for n, ids in at.items():
        ids = [i for i in ids if alive[i]]
        if len(ids) != 2 or ids[0] == ids[1]:
            continue
        i, j = ids
        if key(edges[i]) != key(edges[j]):
            continue
        ei, ej = edges[i], edges[j]
        pi = ei[6] if ei[1] == n else ei[6][::-1]
        pj = ej[6] if ej[0] == n else ej[6][::-1]
        start = ei[0] if ei[1] == n else ei[1]
        end = ej[1] if ej[0] == n else ej[0]
        if start == end:
            continue  # a loop on its own: leave it in two
        ei[0], ei[1], ei[6] = start, end, pi + pj[1:]
        alive[j] = False
        for k in (start, end):
            at[k] = [i if x == j else x for x in at[k]]
    out = []
    for e, a in zip(edges, alive):
        if a:
            e[6] = list(LineString(e[6]).simplify(LINE_TOL).coords)
            out.append(tuple(e))
    return out


def merge_rail(rail):
    """Railway pieces joined end to end where they meet and match."""
    from shapely.geometry import MultiLineString
    from shapely.ops import linemerge
    groups = {}
    for r in rail:
        groups.setdefault((r['class'], r['name']), []).append(r['pts'])
    out = []
    for (cls, name), parts in groups.items():
        merged = linemerge(MultiLineString(parts))
        for ln in lines(merged):
            if ln.length < 150:
                continue  # sidings and yard stubs
            out.append({'class': cls, 'name': name,
                        'pts': [[round(x), round(z)] for x, z in ln.simplify(LINE_TOL).coords]})
    return out


def build_areas(use):
    """Parks and protected land, named, simplified."""
    from shapely import wkb
    areas = []
    for r in use:
        if r['subtype'] not in ('protected', 'park') and r['class'] not in (
                'national_park', 'state_park', 'nature_reserve', 'protected_area'):
            continue
        name = (r['names'] or {}).get('primary')
        if not name:
            continue
        for p in polygons(local_geom(wkb.loads(r['geometry']))):
            if p.area < AREA_MIN:
                continue
            s = p.simplify(25.0)
            areas.append({'kind': r['class'], 'name': name, 'area_km2': round(p.area / 1e6, 2),
                          'ring': [[round(x), round(z)] for x, z in s.exterior.coords]})
    areas.sort(key=lambda a: -a['area_km2'])
    return areas


def build_towns(refresh):
    from shapely import wkb
    rows = overture('theme=divisions/type=division/', refresh,
                    ['subtype', 'class', 'names', 'population'])
    towns = []
    for r in rows:
        if r['subtype'] not in ('locality', 'neighborhood', 'macrohood'):
            continue
        g = wkb.loads(r['geometry'])
        if g.geom_type != 'Point' or not (LON0 < g.x < LON1 and LAT0 < g.y < LAT1):
            continue
        x, z = to_local(g.x, g.y)
        towns.append({'name': (r['names'] or {}).get('primary'), 'kind': r['class'] or r['subtype'],
                      'population': r['population'], 'x': r1(x), 'z': r1(z),
                      'lat': round(g.y, 5), 'lon': round(g.x, 5)})
    towns.sort(key=lambda t: -(t['population'] or 0))
    return towns


def write_terrain(height, cover):
    """terrain.bin: little-endian.

      0  b"MPATLAS1"
      8  u32 width, u32 height
     16  f32 step (m), f32 x0, f32 z0 (centre of cell 0, 0)
     28  u32 reserved (0)
     32  i16 heights in decimetres, row by row from the north (width x height)
     ..  u8 land cover classes, the same order
    """
    # The sea floor matters only as somewhere to stop: no deeper than
    # SEA_FLOOR (the bathymetry offshore has seams and spikes).
    dm = np.round(np.maximum(height, SEA_FLOOR) * 10).astype('<i2')
    head = b'MPATLAS1' + struct.pack('<IIfffI', W, H, STEP, X0, Z0, 0)
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, 'terrain.bin'), 'wb') as f:
        f.write(head)
        f.write(dm.tobytes())
        f.write(cover.astype(np.uint8).tobytes())


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--refresh', action='store_true', help='download everything again')
    args = ap.parse_args()
    print(f'grid {W} x {H} at {STEP:g} m, origin {ORIGIN}', file=sys.stderr)
    height = heights(args.refresh)
    cover, use, _water = build_cover(height, args.refresh)
    # The sea's edge where no polygon reached: below sea level and open.
    write_terrain(height, cover)
    nodes, roads, rail = build_roads(args.refresh)
    areas = build_areas(use)
    towns = build_towns(args.refresh)
    geo = {
        'name': 'peninsula',
        'origin': {'lat': ORIGIN[0], 'lon': ORIGIN[1],
                   'what': "Half Moon Bay: Highway 1 at Highway 92"},
        'projection': {'kind': 'equirectangular', 'm_per_deg_lat': K_LAT,
                       'm_per_deg_lon': K_LON,
                       'axes': 'x east, z south, metres from the origin'},
        'box': {'lon0': LON0, 'lat0': LAT0, 'lon1': LON1, 'lat1': LAT1},
        'grid': {'width': W, 'height': H, 'step': STEP, 'x0': X0, 'z0': Z0},
        'cover_classes': COVER_NAMES,
        'sources': [
            'Terrain: Mapzen/Tilezen terrain tiles on AWS Open Data (USGS 3DEP, SRTM, '
            'bathymetry); public domain and open licences.',
            f'Roads, railways, land use, water and towns: Overture Maps Foundation release '
            f'{OVERTURE_RELEASE}, from OpenStreetMap, (c) OpenStreetMap contributors, ODbL.',
            'Land cover: ESA WorldCover via Overture Maps, CC BY 4.0.',
        ],
        'nodes': nodes,
        'roads': roads,
        'rail': rail,
        'areas': areas,
        'towns': towns,
    }
    with open(os.path.join(OUT, 'geo.json'), 'w') as f:
        json.dump(geo, f, separators=(',', ':'), ensure_ascii=False)
        f.write('\n')
    counts = np.bincount(cover.ravel(), minlength=len(COVER_NAMES))
    print(f'terrain.bin: {W} x {H}, heights {height.min():.0f}..{height.max():.0f} m; cover '
          + ', '.join(f'{n} {c / cover.size:.0%}' for n, c in zip(COVER_NAMES, counts)),
          file=sys.stderr)
    print(f'geo.json: {len(nodes)} nodes, {len(roads)} road edges, {len(rail)} rail lines, '
          f'{len(areas)} parks, {len(towns)} towns', file=sys.stderr)


if __name__ == '__main__':
    main()
