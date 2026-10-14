#!/usr/bin/env python3
"""The ground under a corridor: real heights for the box round it, in game
metres, for the game (docs/vision/peninsula.md, stage 2).

  python3 tools/atlas/ground.py coast-south hwy-84-lower [--margin 1500] [--step 12] [--name <id>] [--refresh]

Reads assets/atlas/peninsula/corridors/<id>.txt for each corridor given
(written by `cargo run -p mp_atlas -- corridor <id>`: the road in game
metres at the plan's scale), takes the box round all of them with a margin
(game metres), fetches the Terrarium
terrain tiles (zoom 13, about 15 m a pixel; cached in tools/atlas/cache/)
and resamples them bilinearly onto a grid of `step` game metres, heights
scaled as the corridor's road is (game metres per real metre, vertically
too). Writes <first id>.ground.bin (or `--name <id>`.ground.bin) beside the
corridors, which `mp_levels` includes at compile time:

      0  b"MPGROUND"
      8  u32 width, u32 height (cells)
     16  f32 step, f32 x0, f32 z0 (game metres; cell (0, 0)'s centre, the
         north-west corner; x east, z south)
     28  u32 length of the zlib stream
     32  zlib: i16 heights in game decimetres, row by row from the north

The sea floor is clamped at SEA_FLOOR, as the planning grid's is.

Beside it <id>.cover.bin (stage 4a): the land cover of the same cells, the
nearest cell of the planning grid's (terrain.bin, build.py's classes):

      0  b"MPCOVER1"
      8  u32 width, u32 height, f32 step, f32 x0, f32 z0, u32 zlib length
     32  zlib: u8 classes, row by row from the north
"""

import io
import math
import os
import struct
import sys
import zlib

import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import build  # noqa: E402  (the projection, the tile fetch, the constants)

OUT = os.path.join(build.OUT, 'corridors')


def read_corridor(path):
    scale = None
    xs, zs = [], []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line.startswith('# scale '):
                scale = float(line.split()[2])
            if not line or line.startswith('#'):
                continue
            x, z = line.split()[:2]
            xs.append(float(x))
            zs.append(float(z))
    if scale is None:
        raise SystemExit(f'{path}: no scale line')
    return scale, np.array(xs), np.array(zs)


def mosaic_for(lon0, lat0, lon1, lat1, refresh):
    """The Terrarium tiles covering the box, as one array of real metres,
    and the tile origin."""
    tx0, ty0 = build.tile_xy(lon0, lat1, build.DEM_ZOOM)
    tx1, ty1 = build.tile_xy(lon1, lat0, build.DEM_ZOOM)
    tx0, ty0, tx1, ty1 = int(tx0), int(ty0), int(tx1), int(ty1)
    nx, ny = tx1 - tx0 + 1, ty1 - ty0 + 1
    mosaic = np.zeros((ny * 256, nx * 256), dtype=np.float64)
    print(f'ground: {nx * ny} tiles at zoom {build.DEM_ZOOM}', file=sys.stderr)
    for ty in range(ty0, ty1 + 1):
        for tx in range(tx0, tx1 + 1):
            url = build.DEM_URL.format(z=build.DEM_ZOOM, x=tx, y=ty)
            path = os.path.join(build.CACHE, 'terrarium', str(build.DEM_ZOOM), f'{tx}_{ty}.png')
            png = Image.open(io.BytesIO(build.fetch(url, path, refresh))).convert('RGB')
            a = np.asarray(png, dtype=np.float64)
            h = a[:, :, 0] * 256 + a[:, :, 1] + a[:, :, 2] / 256 - 32768
            oy, ox = (ty - ty0) * 256, (tx - tx0) * 256
            mosaic[oy:oy + 256, ox:ox + 256] = h
    return mosaic, tx0, ty0


def main():
    args = sys.argv[1:]
    opt = lambda k, d: float(args[args.index(k) + 1]) if k in args else d  # noqa: E731
    # The corridor ids: every bare argument that is not an option's value.
    values = {args[i + 1] for i, a in enumerate(args[:-1]) if a.startswith('--')}
    ids = [a for a in args if not a.startswith('--') and a not in values]
    if not ids:
        raise SystemExit(__doc__)
    cid = ids[0]
    margin = opt('--margin', 1500.0)
    # `--name <id>` names the output (`<first corridor>` otherwise).
    name = args[args.index('--name') + 1] if '--name' in args else None
    step = opt('--step', 12.0)
    refresh = '--refresh' in args
    scale, xs, zs = None, [], []
    for i in ids:
        sc, x, z = read_corridor(os.path.join(OUT, f'{i}.txt'))
        if scale is not None and sc != scale:
            raise SystemExit(f'{i}: scale {sc} differs from {scale}')
        scale = sc
        xs.append(x)
        zs.append(z)
    xs, zs = np.concatenate(xs), np.concatenate(zs)

    # The box, in game metres, on the grid.
    x0 = math.floor((xs.min() - margin) / step) * step
    z0 = math.floor((zs.min() - margin) / step) * step
    x1 = math.ceil((xs.max() + margin) / step) * step
    z1 = math.ceil((zs.max() + margin) / step) * step
    w = int(round((x1 - x0) / step)) + 1
    h = int(round((z1 - z0) / step)) + 1

    # Real metres for the tiles.
    lon0, lat1 = build.to_lonlat(x0 / scale, z0 / scale)
    lon1, lat0 = build.to_lonlat(x1 / scale, z1 / scale)
    mosaic, tx0, ty0 = mosaic_for(lon0, lat0, lon1, lat1, refresh)
    n = 2 ** build.DEM_ZOOM
    out = np.zeros((h, w), dtype=np.float64)
    gx = (x0 + np.arange(w) * step) / scale
    for j in range(h):
        z = (z0 + j * step) / scale
        lon, lat = build.to_lonlat(gx, z)
        px = ((lon + 180) / 360 * n - tx0) * 256 - 0.5
        py = ((1 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2 * n - ty0) * 256 - 0.5
        i0, j0 = np.floor(px).astype(int), int(math.floor(py))
        fx, fy = px - i0, py - j0
        r0, r1 = mosaic[j0], mosaic[j0 + 1]
        top = r0[i0] * (1 - fx) + r0[i0 + 1] * fx
        bot = r1[i0] * (1 - fx) + r1[i0 + 1] * fx
        out[j] = top * (1 - fy) + bot * fy
    out = np.maximum(out, build.SEA_FLOOR) * scale
    dm = np.round(out * 10).astype('<i2')
    z = zlib.compress(dm.tobytes(), 9)
    head = b'MPGROUND' + struct.pack('<IIfffI', w, h, step, x0, z0, len(z))
    path = os.path.join(OUT, f'{name or cid}.ground.bin')
    with open(path, 'wb') as f:
        f.write(head)
        f.write(z)
    print(f'{path}: {w} x {h} cells of {step:g} m from ({x0:g}, {z0:g}), heights '
          f'{out.min():.1f}..{out.max():.1f} m (game), {len(z) / 1024:.0f} kB', file=sys.stderr)

    # The land cover of the same cells, from the planning grid (stage 4a):
    # the nearest 50 m cell of terrain.bin, one byte a cell, the classes
    # build.py's (COVER_NAMES).
    cover = read_cover()
    if cover is not None:
        cw, ch, cstep, cx0, cz0, classes = cover
        gz = (z0 + np.arange(h) * step) / scale
        ci = np.clip(np.round((gx - cx0) / cstep).astype(int), 0, cw - 1)
        cj = np.clip(np.round((gz - cz0) / cstep).astype(int), 0, ch - 1)
        grid = classes[cj][:, ci]
        zc = zlib.compress(grid.astype(np.uint8).tobytes(), 9)
        head = b'MPCOVER1' + struct.pack('<IIfffI', w, h, step, x0, z0, len(zc))
        cpath = os.path.join(OUT, f'{name or cid}.cover.bin')
        with open(cpath, 'wb') as f:
            f.write(head)
            f.write(zc)
        counts = np.bincount(grid.ravel(), minlength=len(build.COVER_NAMES))
        share = ', '.join(f'{n} {c / grid.size:.0%}' for n, c in zip(build.COVER_NAMES, counts) if c)
        print(f'{cpath}: cover {share}, {len(zc) / 1024:.0f} kB', file=sys.stderr)


def read_cover():
    """terrain.bin's land cover: (w, h, step, x0, z0, classes), real metres
    in the game's frame; None when the atlas has not been built."""
    path = os.path.join(build.OUT, 'terrain.bin')
    if not os.path.exists(path):
        print(f'{path}: not built (python3 tools/atlas/build.py); no cover written', file=sys.stderr)
        return None
    with open(path, 'rb') as f:
        head = f.read(32)
        if head[:8] != b'MPATLAS1':
            raise SystemExit(f'{path}: not an MPATLAS1 file')
        w, h, step, x0, z0, _ = struct.unpack('<IIfffI', head[8:])
        f.seek(32 + w * h * 2)
        classes = np.frombuffer(f.read(w * h), dtype=np.uint8).reshape(h, w)
    return w, h, step, x0, z0, classes


if __name__ == '__main__':
    main()
