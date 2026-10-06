//! Streets.js's buildings: `buildBlocks`, `lots`, `block`, `midrise`, the
//! signs, `rowhouse`, `towerBlock`, `tower`, `piers`, `plaza`, `farBlock`,
//! `streetTrees`, `buildTrees`, `buildAircraftLights` and `billboard`, as
//! methods of [`Bld`].

// The JS turns things by `rng() * 6.28`, not 2π.
#![allow(clippy::approx_constant)]
#![allow(clippy::too_many_arguments, clippy::needless_range_loop)]

use mp_math::{js, kernel, lerp};
use mp_scene::{NodeType, three};

use super::facades::{F_TILE, f, floor_h};
use super::props;
use super::textures::{
    AWNING_C, H_SIGNS, KONBINI, LOBBY, NEON_COLORS, ROW_GROUND, ROWHOUSE, S_TILE, SHOP_A, SHOP_B,
    SHUTTER, STUCCO, V_SIGNS,
};
use super::{
    AWNING, Billboard, Bld, Block, Front, NEON, PAINTED, Spill, Tree, ambient_patch, anim, ground,
};
use crate::city::textures::{AD_COUNT, ad_texture};
use crate::color::Color;
use crate::geom::{GeoBuilder, P2, P3, PrismOpts, StaticOpts, instanced, static_mesh, trs};
use crate::material::Material;
use crate::object::Layer;
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Matrix4, cone_geometry, cylinder_geometry,
    icosahedron_geometry,
};

const PI2: f64 = std::f64::consts::PI * 2.0;
const ADDITIVE: f64 = three::ADDITIVE_BLENDING as f64;

/// A lot round a block's edge: `[x0, z0, x1, z1, face]`, face the outward
/// direction of the street front: 'n' (−z), 's', 'w' (−x), 'e'.
pub type Lot = (f64, f64, f64, f64, char);

/// `midrise`'s options object `o`.
#[derive(Clone, Debug, Default)]
pub struct MidOpts {
    pub floors: [f64; 2],
    /// `o.shop` (`?? SHOP_A`).
    pub shop: Option<usize>,
    pub cells: Option<Vec<usize>>,
    pub neon: bool,
}

/// `lotWalls`: the street-front wall and its other three, as [a, b] xz pairs.
struct Walls {
    front: [P2; 2],
    others: Vec<[P2; 2]>,
}

fn lot_walls(x0: f64, z0: f64, x1: f64, z1: f64, face: char) -> Walls {
    let walls = [
        ('n', [[x1, z0], [x0, z0]]),
        ('s', [[x0, z1], [x1, z1]]),
        ('w', [[x0, z0], [x0, z1]]),
        ('e', [[x1, z1], [x1, z0]]),
    ];
    Walls {
        front: walls.iter().find(|w| w.0 == face).expect("a face").1,
        others: walls.iter().filter(|w| w.0 != face).map(|w| w.1).collect(),
    }
}

/// `frontFrame(fr, cx, cz)`: a lot's street wall, tangent and outward normal.
struct FrontFrame {
    fa: P2,
    fmx: f64,
    fmz: f64,
    l: f64,
    tx: f64,
    tz: f64,
    nx: f64,
    nz: f64,
}

fn front_frame(fr: &Walls, cx: f64, cz: f64) -> FrontFrame {
    let [fa, fb] = fr.front;
    let fmx = (fa[0] + fb[0]) / 2.0;
    let fmz = (fa[1] + fb[1]) / 2.0;
    let ex = fb[0] - fa[0];
    let ez = fb[1] - fa[1];
    let l = kernel::hypot(ex, ez);
    let tx = ex / l;
    let tz = ez / l;
    let mut nx = -tz;
    let mut nz = tx;
    if nx * (fmx - cx) + nz * (fmz - cz) < 0.0 {
        nx = -nx;
        nz = -nz;
    }
    FrontFrame {
        fa,
        fmx,
        fmz,
        l,
        tx,
        tz,
        nx,
        nz,
    }
}

/// `{ color }`.
pub fn col(c: P3) -> PrismOpts {
    PrismOpts {
        color: Some(c),
        ..PrismOpts::default()
    }
}

/// `{ cell, color: fd, tileW: 4, tileH: 4, roofCell, roofTile }`.
fn panel(cell: f64, fd: P3, roof_cell: f64, roof_tile: Option<f64>) -> PrismOpts {
    PrismOpts {
        cell: Some(cell),
        color: Some(fd),
        tile_w: Some(4.0),
        tile_h: Some(4.0),
        roof_cell: Some(roof_cell),
        roof_tile,
        ..PrismOpts::default()
    }
}

fn scale3(c: P3, k: f64) -> P3 {
    [c[0] * k, c[1] * k, c[2] * k]
}

/// `rect(X0, Z0, X1, Z1, ch = 0)`: a footprint, chamfered when `ch > 0`.
fn rect(x0: f64, z0: f64, x1: f64, z1: f64, ch: f64) -> Vec<P2> {
    if ch > 0.0 {
        vec![
            [x0 + ch, z0],
            [x1 - ch, z0],
            [x1, z0 + ch],
            [x1, z1 - ch],
            [x1 - ch, z1],
            [x0 + ch, z1],
            [x0, z1 - ch],
            [x0, z0 + ch],
        ]
    } else {
        vec![[x0, z0], [x1, z0], [x1, z1], [x0, z1]]
    }
}

/// One wall quad with outward normal, atlas UVs in tile units.
pub fn wall(
    b: &mut GeoBuilder,
    a: P2,
    c: P2,
    y0: f64,
    y1: f64,
    cell: f64,
    tile: [f64; 2],
    v_ref: f64,
    col: P3,
    cxz: P2,
    u_off: f64,
) {
    let mut aa = a;
    let mut cc = c;
    let ex = cc[0] - aa[0];
    let ez = cc[1] - aa[1];
    // GeoBuilder.quad's normal is (−ez, 0, ex) for a→c; flip to face out.
    if (-ez) * (aa[0] - cxz[0]) + ex * (aa[1] - cxz[1]) < 0.0 {
        aa = c;
        cc = a;
    }
    let l = kernel::hypot(ex, ez);
    let u0 = u_off;
    let u1 = u_off + l / tile[0];
    let v0 = (y0 - v_ref) / tile[1];
    let v1 = (y1 - v_ref) / tile[1];
    b.quad(
        [aa[0], y0, aa[1]],
        [cc[0], y0, cc[1]],
        [cc[0], y1, cc[1]],
        [aa[0], y1, aa[1]],
        Some([[u0, v0], [u1, v0], [u1, v1], [u0, v1]]),
        Some(col),
        cell,
    );
}

/// `roof(B, x0, z0, x1, z1, y, cell = F.ROOF, col = null)`.
fn roof(b: &mut GeoBuilder, x0: f64, z0: f64, x1: f64, z1: f64, y: f64, cell: f64, c: Option<P3>) {
    b.tri_uv(
        [x0, y, z0],
        [x1, y, z1],
        [x1, y, z0],
        [x0 / 16.0, z0 / 16.0],
        [x1 / 16.0, z1 / 16.0],
        [x1 / 16.0, z0 / 16.0],
        c,
        cell,
    );
    b.tri_uv(
        [x0, y, z0],
        [x0, y, z1],
        [x1, y, z1],
        [x0 / 16.0, z0 / 16.0],
        [x0 / 16.0, z1 / 16.0],
        [x1 / 16.0, z1 / 16.0],
        c,
        cell,
    );
}

/// Walls round a closed footprint on the façade atlas, u running on round
/// the corners so window indices (and the lights) stay continuous.
fn ring(
    b: &mut GeoBuilder,
    poly: &[P2],
    y0: f64,
    y1: f64,
    cell: f64,
    v_ref: f64,
    fd: P3,
    cxz: P2,
    u_off: f64,
) {
    let tile = F_TILE[(cell % 16.0) as usize];
    let mut u = u_off;
    for k in 0..poly.len() {
        let a = poly[k];
        let c = poly[(k + 1) % poly.len()];
        wall(b, a, c, y0, y1, cell, tile, v_ref, fd, cxz, u);
        u += kernel::hypot(c[0] - a[0], c[1] - a[1]) / tile[0];
    }
}

/// Lowest ground under a footprint (buildings on the hill stand on it).
fn lot_ground(x0: f64, z0: f64, x1: f64, z1: f64) -> f64 {
    let mut lo = f64::INFINITY;
    for [x, z] in [
        [x0, z0],
        [x1, z0],
        [x1, z1],
        [x0, z1],
        [(x0 + x1) / 2.0, (z0 + z1) / 2.0],
    ] {
        lo = js::min(lo, ground(x, z));
    }
    lo
}

/// A sign quad at p0 spanning `w` along (ax, az) and `h` up, facing the
/// horizontal normal (nx, nz). uv: [u0, v0, u1, v1]. nd: flicker data.
fn sign_quad(
    b: &mut GeoBuilder,
    p0: P3,
    ax: f64,
    az: f64,
    w: f64,
    h: f64,
    nx: f64,
    nz: f64,
    [u0, v0, u1, v1]: [f64; 4],
    nd: P3,
) {
    let [x, y, z] = p0;
    let a = [x, y, z];
    let bb = [x + ax * w, y, z + az * w];
    let c = [x + ax * w, y + h, z + az * w];
    let d = [x, y + h, z];
    let uv = Some([[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
    // quad(a, b, c, …) faces (−az, ax); flip the winding (and u) if needed.
    if -az * nx + ax * nz >= 0.0 {
        b.quad(a, bb, c, d, uv, Some(nd), 0.0);
    } else {
        b.quad(bb, a, d, c, uv, Some(nd), 0.0);
    }
}

fn glow_wall(b: &mut GeoBuilder, a: P2, c: P2, y0: f64, y1: f64, col: P3, cxz: P2) {
    let mut aa = a;
    let mut cc = c;
    let ex = cc[0] - aa[0];
    let ez = cc[1] - aa[1];
    if (-ez) * (aa[0] - cxz[0]) + ex * (aa[1] - cxz[1]) < 0.0 {
        aa = c;
        cc = a;
    }
    b.quad(
        [aa[0], y0, aa[1]],
        [cc[0], y0, cc[1]],
        [cc[0], y1, cc[1]],
        [aa[0], y1, aa[1]],
        None,
        Some(col),
        0.0,
    );
}

impl Bld<'_> {
    fn pick<T: Copy>(&mut self, a: &[T]) -> T {
        a[(self.rng() * a.len() as f64).floor() as usize]
    }

    // ── Buildings ─────────────────────────────────────────────────
    pub(super) fn build_blocks(&mut self) {
        for bi in 0..self.blocks.len() {
            let b = self.blocks[bi].clone();
            if b.tier == 2 {
                self.far_block(&b);
                continue;
            }
            self.block_build(&b);
        }
    }

    /// Lots round the edge of a block.
    fn lots(&mut self, b: &Block, frontage: [f64; 2], depth: f64) -> Vec<Lot> {
        let (px, pz, hw, walk) = (self.g.px, self.g.pz, self.g.hw, self.g.walk);
        let (fi, fj) = (b.i as f64, b.j as f64);
        let bx0 = fi * px + hw + walk;
        let bx1 = (fi + 1.0) * px - hw - walk;
        let bz0 = fj * pz + hw + walk;
        let bz1 = (fj + 1.0) * pz - hw - walk;
        let d = js::min_n(&[depth, (bz1 - bz0) / 2.0, (bx1 - bx0) / 2.0]);
        let mut out = Vec::new();
        let split = |s: &mut Self, a0: f64, a1: f64| -> Vec<f64> {
            let mut cuts = vec![a0];
            let mut u = a0;
            while a1 - u > frontage[0] * 1.5 {
                u += frontage[0] + s.rng() * (frontage[1] - frontage[0]);
                if a1 - u < frontage[0] {
                    break;
                }
                cuts.push(u);
            }
            cuts.push(a1);
            cuts
        };
        let xs = split(self, bx0, bx1);
        for k in 0..xs.len() - 1 {
            out.push((xs[k], bz0, xs[k + 1], bz0 + d, 'n'));
        }
        let xs2 = split(self, bx0, bx1);
        for k in 0..xs2.len() - 1 {
            out.push((xs2[k], bz1 - d, xs2[k + 1], bz1, 's'));
        }
        let zs = split(self, bz0 + d, bz1 - d);
        for k in 0..zs.len() - 1 {
            out.push((bx0, zs[k], bx0 + d, zs[k + 1], 'w'));
        }
        let zs2 = split(self, bz0 + d, bz1 - d);
        for k in 0..zs2.len() - 1 {
            out.push((bx1 - d, zs2[k], bx1, zs2[k + 1], 'e'));
        }
        out
    }

    /// Would a lot crowd the race route (the inside of a corner)?
    fn lot_blocked(&self, x0: f64, z0: f64, x1: f64, z1: f64) -> bool {
        let (hw, walk) = (self.g.hw, self.g.walk);
        for [x, z] in [
            [x0, z0],
            [x1, z0],
            [x1, z1],
            [x0, z1],
            [(x0 + x1) / 2.0, z0],
            [(x0 + x1) / 2.0, z1],
            [x0, (z0 + z1) / 2.0],
            [x1, (z0 + z1) / 2.0],
        ] {
            let r = self.t.distance_to_road(x, z, 40.0);
            if r.i >= 0 && r.d < hw + walk - 0.6 {
                return true;
            }
        }
        false
    }

    /// `block(b)`.
    fn block_build(&mut self, b: &Block) {
        let d = b.district;
        let near = b.tier == 0;
        if d == 2 {
            let r = kernel::hypot(b.cx - self.core[0], b.cz - self.core[1]);
            let core = kernel::exp(-kernel::pow(r / 520.0, 2.0));
            if near && self.rng() < 0.12 && core < 0.8 {
                self.plaza(b);
                return;
            }
            if self.rng() < 0.35 + core * 0.55 {
                self.tower_block(b, core);
                return;
            }
            for l in self.lots(b, [16.0, 30.0], 30.0) {
                if near && self.lot_blocked(l.0, l.1, l.2, l.3) {
                    continue;
                }
                let floors = [6.0, 14.0 + js::round(core * 10.0)];
                let shop = if self.rng() < 0.5 { LOBBY } else { SHOP_A };
                self.midrise(
                    l,
                    b,
                    &MidOpts {
                        floors,
                        shop: Some(shop),
                        ..MidOpts::default()
                    },
                );
            }
            return;
        }
        if d == 1 {
            for l in self.lots(b, [5.6, 7.6], 17.0) {
                if near && self.lot_blocked(l.0, l.1, l.2, l.3) {
                    continue;
                }
                if self.rng() < 0.12 {
                    let shop = if self.rng() < 0.5 { SHOP_B } else { SHOP_A };
                    self.midrise(
                        l,
                        b,
                        &MidOpts {
                            floors: [3.0, 6.0],
                            shop: Some(shop),
                            cells: Some(vec![f::BRICK, f::HOTEL]),
                            neon: false,
                        },
                    );
                } else {
                    self.rowhouse(l, b);
                }
            }
            if near {
                self.street_trees(b, 0.5);
            }
            return;
        }
        for l in self.lots(b, [8.0, 16.0], 24.0) {
            if near && self.lot_blocked(l.0, l.1, l.2, l.3) {
                continue;
            }
            let r = self.rng();
            let neon = near || self.rng() < 0.1;
            let shop = if r < 0.15 {
                SHUTTER
            } else if r < 0.27 {
                KONBINI
            } else if r < 0.62 {
                SHOP_A
            } else {
                SHOP_B
            };
            self.midrise(
                l,
                b,
                &MidOpts {
                    floors: [2.0, 7.0],
                    neon,
                    shop: Some(shop),
                    cells: None,
                },
            );
        }
    }

    /// Shops at street level, façade floors above, signs on the front.
    fn midrise(&mut self, l: Lot, b: &Block, o: &MidOpts) {
        let (x0, z0, x1, z1, face) = l;
        let inset = 0.25;
        let xx0 = x0 + inset;
        let xx1 = x1 - inset;
        let zz0 = z0 + inset;
        let zz1 = z1 - inset;
        let cx = (xx0 + xx1) / 2.0;
        let cz = (zz0 + zz1) / 2.0;
        let fr = lot_walls(xx0, zz0, xx1, zz1, face);
        let FrontFrame {
            fa,
            fmx,
            fmz,
            l,
            tx,
            tz,
            nx,
            nz,
        } = front_frame(&fr, cx, cz);
        let floor_y = ground(fmx, fmz) + 0.15; // street level at the front door
        let base = lot_ground(xx0, zz0, xx1, zz1) - 1.0;
        let sh = 4.6;
        let d = b.district;
        let seed = self.seed();
        let cell_u = match &o.cells {
            Some(c) => self.pick(c),
            None if d == 0 => self.pick(&[
                f::NAPT,
                f::NTILE,
                f::BRICK,
                f::HOTEL,
                f::NAPT,
                f::RIBBON,
                f::NTILE,
            ]),
            None if d == 1 => self.pick(&[f::BRICK, f::HOTEL, f::BRICK]),
            None => self.pick(&[f::STONE, f::RIBBON, f::BANDS, f::CURTAIN]),
        };
        let fh = floor_h(cell_u);
        let floors = o.floors[0] + (self.rng() * (o.floors[1] - o.floors[0] + 1.0)).floor();
        let top = floor_y + sh + floors * fh;
        let hue = if d == 0 {
            0.02 + self.rng() * 0.96
        } else {
            0.0
        };
        let fd = [floor_y, if d == 0 { 1.0 } else { 0.75 }, hue];
        // `const Bs = this.bStreet.at(cx, cz), Bf = this.bFac.at(cx, cz)`.
        self.b_street.at(cx, cz);
        self.b_fac.at(cx, cz);
        let shop = o.shop.unwrap_or(SHOP_A);
        let wall_col = [
            0.75 + self.rng() * 0.2,
            0.72 + self.rng() * 0.18,
            0.68 + self.rng() * 0.2,
        ];
        // Street floor.
        let u_off = (self.rng() * 4.0).floor() / 4.0;
        wall(
            self.b_street.at(cx, cz),
            fr.front[0],
            fr.front[1],
            base,
            floor_y + sh,
            shop as f64 + 16.0 * seed,
            S_TILE[shop],
            floor_y,
            [1.0, 1.0, 1.0],
            [cx, cz],
            u_off,
        );
        // Side walls: stucco, unless it's a corner lot whose side faces the
        // cross street, which gets shop windows too.
        let (px, pz, hw, walk) = (self.g.px, self.g.pz, self.g.hw, self.g.walk);
        let e0 = hw + walk + 1.2;
        let (fi, fj) = (b.i as f64, b.j as f64);
        let on_street = |m: P2| {
            m[0] - fi * px < e0
                || (fi + 1.0) * px - m[0] < e0
                || m[1] - fj * pz < e0
                || (fj + 1.0) * pz - m[1] < e0
        };
        for w in &fr.others {
            let m = [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0];
            let side = if b.tier == 0 && on_street(m) {
                Some(if shop == KONBINI || shop == LOBBY {
                    shop
                } else if self.rng() < 0.5 {
                    SHOP_B
                } else {
                    SHUTTER
                })
            } else {
                None
            };
            match side {
                Some(side) => {
                    let u = self.rng();
                    wall(
                        self.b_street.at(cx, cz),
                        w[0],
                        w[1],
                        base,
                        floor_y + sh,
                        side as f64 + 16.0 * seed,
                        S_TILE[side],
                        floor_y,
                        [1.0, 1.0, 1.0],
                        [cx, cz],
                        u,
                    );
                }
                None => wall(
                    self.b_street.at(cx, cz),
                    w[0],
                    w[1],
                    base,
                    floor_y + sh,
                    STUCCO as f64 + 16.0 * seed,
                    S_TILE[STUCCO],
                    floor_y,
                    wall_col,
                    [cx, cz],
                    0.0,
                ),
            }
        }
        // Floors above, a belt course over the shops and a cornice that caps
        // the roof.
        let cols = [4.0, 4.0, 4.0, 4.0, 8.0, 4.0, 6.0, 6.0]
            .get(cell_u)
            .copied()
            .unwrap_or(4.0);
        let poly = [[xx0, zz0], [xx1, zz0], [xx1, zz1], [xx0, zz1]];
        let u_off = (self.rng() * cols).floor() / cols;
        ring(
            self.b_fac.at(cx, cz),
            &poly,
            floor_y + sh,
            top,
            cell_u as f64 + 16.0 * seed,
            floor_y + sh,
            fd,
            [cx, cz],
            u_off,
        );
        let yaw = kernel::atan2(tz, tx);
        let pan = f::PANEL as f64 + 16.0 * seed;
        if b.tier == 0 {
            self.b_fac.at(cx, cz).box_(
                fmx + nx * 0.08,
                floor_y + sh - 0.25,
                fmz + nz * 0.08,
                l + 0.1,
                0.45,
                0.4,
                yaw,
                &panel(pan, fd, pan, None),
            );
        }
        self.b_fac.at(cx, cz).box_(
            cx,
            top - 0.55,
            cz,
            xx1 - xx0 + 0.5,
            1.1,
            zz1 - zz0 + 0.5,
            0.0,
            &panel(pan, fd, f::ROOF as f64 + 16.0 * seed, Some(16.0)),
        );
        // Rooftop plant and, on older blocks, a water tank on legs.
        // (`const P = this.bPlain.at(cx, cz)`: the chunk is made here even
        // when nothing goes into it.)
        self.b_plain.at(cx, cz);
        let pc = [0.3, 0.3, 0.32];
        let mut k = 0.0;
        loop {
            // The loop's bound draws anew every time it is tested.
            let bound = if b.tier == 0 {
                1.0 + (self.rng() * 3.0).floor()
            } else {
                0.0
            };
            if k >= bound {
                break;
            }
            let w = 1.5 + self.rng() * 3.0;
            let dd = 1.5 + self.rng() * 3.0;
            let px_ = lerp(xx0 + w, xx1 - w, self.rng());
            let pz_ = lerp(zz0 + dd, zz1 - dd, self.rng());
            let h = 1.0 + self.rng() * 2.0;
            self.b_plain
                .at(cx, cz)
                .box_(px_, top + 0.55, pz_, w, h, dd, 0.0, &col(pc));
            k += 1.0;
        }
        if d != 2 && self.rng() < 0.25 && b.tier == 0 {
            let px_ = lerp(xx0 + 3.0, xx1 - 3.0, self.rng());
            let pz_ = lerp(zz0 + 3.0, zz1 - 3.0, self.rng());
            let p = self.b_plain.at(cx, cz);
            for [lx, lz] in [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
                p.box_(
                    px_ + lx,
                    top + 0.55,
                    pz_ + lz,
                    0.15,
                    2.9,
                    0.15,
                    0.0,
                    &col([0.2, 0.2, 0.2]),
                );
            }
            p.box_(
                px_,
                top + 3.4,
                pz_,
                3.0,
                0.2,
                3.0,
                0.0,
                &col([0.25, 0.22, 0.2]),
            );
            let mut cyl = cylinder_geometry(1.4, 1.4, 3.0, 10.0, 1.0, false, 0.0, PI2);
            cyl.translate(0.0, 1.5, 0.0);
            props::add_geo(
                p,
                &cyl.to_non_indexed(),
                &Matrix4::IDENTITY.set_position(px_, top + 3.6, pz_),
                Some([0.42, 0.3, 0.22]),
            );
            let mut cone = cone_geometry(1.5, 0.8, 10.0, 1.0, false, 0.0, PI2);
            cone.translate(0.0, 0.4, 0.0);
            props::add_geo(
                p,
                &cone.to_non_indexed(),
                &Matrix4::IDENTITY.set_position(px_, top + 6.6, pz_),
                Some([0.3, 0.24, 0.2]),
            );
        }
        // Remember the front for pavement props and the light it spills.
        let lit = if shop == SHUTTER {
            None
        } else if shop == SHOP_B {
            Some([0.5, 0.28, 0.1])
        } else if shop == KONBINI {
            Some([0.36, 0.42, 0.5])
        } else if shop == LOBBY {
            Some([0.4, 0.32, 0.2])
        } else {
            Some([0.3, 0.3, 0.38])
        };
        self.fronts.push(Front {
            fa,
            tx,
            tz,
            nx,
            nz,
            l,
            y: floor_y,
            district: d,
            tier: b.tier,
            shop: Some(shop),
            lit,
        });
        if shop != SHUTTER && b.tier == 0 && self.rng() < (if d == 0 { 0.6 } else { 0.3 }) {
            // Striped awning over the shop front, with a valance.
            let c = scale3(
                AWNING[(self.rng() * AWNING.len() as f64).floor() as usize],
                1.6,
            );
            let w0 = 0.5;
            let w1 = l - 0.5;
            let y0 = floor_y + 3.55;
            let y1 = floor_y + 3.0;
            let out = 1.6;
            let a = [fa[0] + tx * w0 + nx * 0.05, y0, fa[1] + tz * w0 + nz * 0.05];
            let bq = [fa[0] + tx * w1 + nx * 0.05, y0, fa[1] + tz * w1 + nz * 0.05];
            let c_ = [bq[0] + nx * out, y1, bq[2] + nz * out];
            let dq = [a[0] + nx * out, y1, a[2] + nz * out];
            let ul = (w1 - w0) / S_TILE[AWNING_C][0];
            let cell = AWNING_C as f64 + 16.0 * seed;
            let top4 = [[0.0, 1.0], [ul, 1.0], [ul, 0.0], [0.0, 0.0]];
            let bs = self.b_street.at(cx, cz);
            // Up-facing and down-facing sides (the underside darker).
            bs.quad(
                a,
                dq,
                c_,
                bq,
                Some([top4[0], top4[3], top4[2], top4[1]]),
                Some(c),
                cell,
            );
            bs.quad(a, bq, c_, dq, Some(top4), Some(scale3(c, 0.55)), cell);
            let e = [c_[0], y1 - 0.35, c_[2]];
            let g = [dq[0], y1 - 0.35, dq[2]];
            let uv = Some([[0.0, 0.0], [ul, 0.0], [ul, 0.35], [0.0, 0.35]]);
            bs.quad(g, e, c_, dq, uv, Some(c), cell);
            bs.quad(e, g, dq, c_, uv, Some(scale3(c, 0.55)), cell);
        }
        let flick = |s: &mut Self| -> P3 {
            let r = s.rng();
            let a = s.rng();
            [
                a,
                if r < 0.74 {
                    0.0
                } else if r < 0.86 {
                    1.0
                } else if r < 0.94 {
                    3.0
                } else {
                    2.0
                },
                0.0,
            ]
        };
        if o.neon && floors >= 1.0 {
            // Fascia sign over the shop, blade signs sticking out, small signs in
            // upper windows: layers of neon up the street.
            if self.rng() < 0.8 {
                let k = (self.rng() * H_SIGNS as f64).floor() as usize;
                let w = js::min(l - 1.0, 3.0 + self.rng() * 2.5);
                let h = w / 2.0;
                let u = (l - w) / 2.0 + (self.rng() - 0.5) * (l - w - 0.4);
                let y0 = floor_y + sh + 0.3;
                let nd = flick(self);
                self.h_sign(fa, tx, tz, nx, nz, u, w, h, y0, 0.12, k, nd);
            }
            if floors >= 3.0 && self.rng() < 0.35 {
                let k = (self.rng() * H_SIGNS as f64).floor() as usize;
                let w = 1.4 + self.rng() * 0.8;
                let u = 0.8 + self.rng() * (l - w - 1.6);
                let y0 = floor_y + sh + fh * (1.3 + (self.rng() * 2.0).floor());
                let nd = flick(self);
                self.h_sign(fa, tx, tz, nx, nz, u, w, w / 2.0, y0, 0.06, k, nd);
            }
            let blades = if floors >= 2.0 {
                (if self.rng() < 0.6 { 1 } else { 0 })
                    + (if floors >= 4.0 && self.rng() < 0.45 {
                        1
                    } else {
                        0
                    })
            } else {
                0
            };
            for q in 0..blades {
                let k = (self.rng() * V_SIGNS as f64).floor() as usize;
                let room = top - floor_y - sh - 1.2;
                let h = js::min(room, 3.5 + self.rng() * 4.0) * (if q != 0 { 0.7 } else { 1.0 });
                let w = h / 4.0;
                if h < 2.0 {
                    continue;
                }
                let u = if q != 0 {
                    if self.rng() < 0.5 { l * 0.35 } else { l * 0.65 }
                } else if self.rng() < 0.5 {
                    0.6
                } else {
                    l - 0.6
                };
                let y0 = floor_y + sh + 1.0 + (if q != 0 { js::max(0.0, room - h) } else { 0.0 });
                let nd = flick(self);
                self.blade_sign(fa, tx, tz, nx, nz, u, w, h, y0, k, nd);
            }
        }
        // Overhead wires strung across Neon District streets.
        if d == 0 && b.tier == 0 && self.rng() < 0.4 {
            let street = 2.0 * (self.g.hw + self.g.walk) + 0.4;
            let mut q = 0.0;
            loop {
                let bound = 1.0 + (self.rng() * 3.0).floor();
                if q >= bound {
                    break;
                }
                let u = self.rng() * l;
                let y = floor_y + sh + 1.5 + self.rng() * 5.0;
                let a = [fa[0] + tx * u, y, fa[1] + tz * u];
                let dd = (self.rng() - 0.5) * 8.0;
                let bb = [
                    a[0] + nx * street + tx * dd,
                    y + (self.rng() - 0.5) * 2.0,
                    a[2] + nz * street + tz * dd,
                ];
                let sag = 0.6 + self.rng() * 1.2;
                props::wire(self, a, bb, sag, 8);
                q += 1.0;
            }
        }
        // A rooftop billboard now and then, facing the street.
        if o.neon && b.tier == 0 && self.rng() < 0.06 && l > 9.0 {
            let w = js::min(l - 1.0, 12.0);
            let h = w * 0.375;
            let ox = fmx - tx * w / 2.0 - nx * 2.0;
            let oz = fmz - tz * w / 2.0 - nz * 2.0;
            let y0 = top + 1.5;
            let ad = (self.rng() * AD_COUNT as f64).floor() as usize;
            self.billboards.push(Billboard {
                ox,
                oz,
                y0,
                tx,
                tz,
                w,
                h,
                ad,
            });
        }
    }

    /// Horizontal neon sign flat on a front, u metres along it.
    fn h_sign(
        &mut self,
        fa: P2,
        tx: f64,
        tz: f64,
        nx: f64,
        nz: f64,
        u: f64,
        w: f64,
        h: f64,
        y0: f64,
        off: f64,
        k: usize,
        nd: P3,
    ) {
        let ox = fa[0] + tx * u + nx * off;
        let oz = fa[1] + tz * u + nz * off;
        let cu = (k % 4) as f64 / 4.0;
        let cv = 1.0 - (k / 4) as f64 / 4.0;
        // Read left to right from the street: start from whichever end is on
        // the viewer's left.
        let flip = -tz * nx + tx * nz < 0.0;
        let sx = if flip { ox + tx * w } else { ox };
        let sz = if flip { oz + tz * w } else { oz };
        sign_quad(
            self.b_neon_h.at(ox, oz),
            [sx, y0, sz],
            if flip { -tx } else { tx },
            if flip { -tz } else { tz },
            w,
            h,
            nx,
            nz,
            [cu, cv - 0.25, cu + 0.25, cv],
            nd,
        );
        // A dark backing box so the sign has depth.
        self.b_plain.at(ox, oz).box_(
            ox + tx * w / 2.0 - nx * (off / 2.0),
            y0 - 0.05,
            oz + tz * w / 2.0 - nz * (off / 2.0),
            w + 0.1,
            h + 0.1,
            js::max(0.04, off - 0.02),
            kernel::atan2(tz, tx),
            &col([0.08, 0.08, 0.09]),
        );
        self.sign_lights.push(super::SignLight {
            x: ox + tx * w / 2.0,
            y: y0 + h / 2.0,
            z: oz + tz * w / 2.0,
            col: NEON_COLORS[k % NEON_COLORS.len()],
            lamp: false,
            big: false,
        });
    }

    /// Vertical blade sign on a bracket: a lit face each way along the street
    /// on a dark box, sticking out from the front.
    fn blade_sign(
        &mut self,
        fa: P2,
        tx: f64,
        tz: f64,
        nx: f64,
        nz: f64,
        u: f64,
        w: f64,
        h: f64,
        y0: f64,
        k: usize,
        nd: P3,
    ) {
        let bx = fa[0] + tx * u + nx * 0.35;
        let bz = fa[1] + tz * u + nz * 0.35;
        let cu = k as f64 / 8.0;
        for d in [1.0, -1.0] {
            let along = d * (-nz * tx + nx * tz) >= 0.0; // does +n run left-to-right seen from this side?
            let ox = bx + tx * d * 0.09;
            let oz = bz + tz * d * 0.09;
            let p0 = if along {
                [ox, y0, oz]
            } else {
                [ox + nx * w, y0, oz + nz * w]
            };
            sign_quad(
                self.b_neon_v.at(bx, bz),
                p0,
                if along { nx } else { -nx },
                if along { nz } else { -nz },
                w,
                h,
                tx * d,
                tz * d,
                [cu, 0.0, cu + 0.125, 1.0],
                nd,
            );
        }
        let p = self.b_plain.at(bx, bz);
        let yaw = kernel::atan2(nz, nx);
        p.box_(
            bx + nx * w / 2.0,
            y0 - 0.06,
            bz + nz * w / 2.0,
            w + 0.1,
            h + 0.12,
            0.16,
            yaw,
            &col([0.07, 0.07, 0.08]),
        );
        for yy in [y0 + 0.3, y0 + h - 0.4] {
            p.box_(
                bx - nx * 0.18,
                yy,
                bz - nz * 0.18,
                0.4,
                0.06,
                0.06,
                yaw,
                &col([0.2, 0.2, 0.22]),
            );
        }
        self.sign_lights.push(super::SignLight {
            x: bx + nx * w / 2.0,
            y: y0 + h / 2.0,
            z: bz + nz * w / 2.0,
            col: NEON_COLORS[(k * 3 + 1) % NEON_COLORS.len()],
            lamp: false,
            big: false,
        });
    }

    /// Painted Victorian rowhouse: a garage storey with the entry up a stoop,
    /// two or three floors with a canted bay window, floor bands, a bracketed
    /// cornice and sometimes a gable.
    fn rowhouse(&mut self, l: Lot, b: &Block) {
        let (x0, z0, x1, z1, face) = l;
        let xx0 = x0 + 0.1;
        let xx1 = x1 - 0.1;
        let zz0 = z0 + 0.1;
        let zz1 = z1 - 0.1;
        let cx = (xx0 + xx1) / 2.0;
        let cz = (zz0 + zz1) / 2.0;
        let fr = lot_walls(xx0, zz0, xx1, zz1, face);
        let FrontFrame {
            fa,
            fmx,
            fmz,
            l,
            tx,
            tz,
            nx,
            nz,
        } = front_frame(&fr, cx, cz);
        let street_y = ground(fmx, fmz) + 0.15;
        let g1 = street_y + 3.5; // top of the garage storey
        let base = lot_ground(xx0, zz0, xx1, zz1) - 1.0;
        let floors = 2.0 + (if self.rng() < 0.35 { 1.0 } else { 0.0 });
        let top = g1 + floors * 3.4;
        let colr = PAINTED[(self.rng() * PAINTED.len() as f64).floor() as usize];
        let r = self.rng();
        let trim = if r < 0.6 {
            [0.97, 0.96, 0.93]
        } else if r < 0.72 {
            [0.25, 0.34, 0.28]
        } else if r < 0.84 {
            [0.5, 0.16, 0.16]
        } else {
            [0.95, 0.85, 0.5]
        };
        let seed = self.seed();
        let full = b.tier == 0 && self.route_dist(fmx, fmz) < 26.0; // small details only where you pass them
        // `const Bs = this.bStreet.at(cx, cz), P = this.bPlain.at(cx, cz)`.
        self.b_street.at(cx, cz);
        self.b_plain.at(cx, cz);
        let mirror = self.rng() < 0.5; // garage on the right, door on the left
        let u_off = if mirror { 1.0 } else { 0.0 };
        let tile_sign = if mirror { -1.0 } else { 1.0 };
        {
            let bs = self.b_street.at(cx, cz);
            wall(
                bs,
                fa,
                fr.front[1],
                base,
                g1,
                ROW_GROUND as f64 + 16.0 * seed,
                [tile_sign * l, 3.5],
                street_y,
                colr,
                [cx, cz],
                u_off,
            );
            wall(
                bs,
                fa,
                fr.front[1],
                g1,
                top,
                ROWHOUSE as f64 + 16.0 * seed,
                [tile_sign * l, 3.4],
                g1,
                colr,
                [cx, cz],
                u_off,
            );
            for w in &fr.others {
                wall(
                    bs,
                    w[0],
                    w[1],
                    base,
                    top,
                    STUCCO as f64 + 16.0 * seed,
                    S_TILE[STUCCO],
                    street_y,
                    scale3(colr, 0.85),
                    [cx, cz],
                    0.0,
                );
            }
        }
        roof(
            self.b_fac.at(cx, cz),
            xx0,
            zz0,
            xx1,
            zz1,
            top + 0.01,
            f::ROOF as f64 + 16.0 * seed,
            Some([street_y, 0.0, 0.0]),
        );
        // wall() maps u along the front from its (possibly swapped) start; find
        // where along the front texture u = 0.25 (the window over the garage) is.
        let flipped = (-(fr.front[1][1] - fa[1])) * (fa[0] - cx)
            + (fr.front[1][0] - fa[0]) * (fa[1] - cz)
            < 0.0;
        let u_at = |uu: f64| {
            let m = if mirror { 1.0 - uu } else { uu };
            if flipped { l * (1.0 - m) } else { l * m }
        };
        let yaw = kernel::atan2(tz, tx);
        // Canted bay window over the garage.
        let bu = u_at(0.25);
        let bw = js::min(l * 0.46, 3.2);
        let dep = 0.7;
        let fw = bw - 2.0 * dep;
        let bcx = fa[0] + tx * bu;
        let bcz = fa[1] + tz * bu;
        let pt = |a: f64, d: f64| [bcx + tx * a + nx * d, bcz + tz * a + nz * d];
        let bay = [
            pt(-bw / 2.0, 0.02),
            pt(-fw / 2.0, dep),
            pt(fw / 2.0, dep),
            pt(bw / 2.0, 0.02),
        ];
        let yb0 = g1 - 0.1;
        let yb1 = top - 0.55;
        let tile_b = [tile_sign * l, 3.4];
        for k in 0..3 {
            let a = bay[k];
            let c = bay[k + 1];
            let seg_l = kernel::hypot(c[0] - a[0], c[1] - a[1]);
            // Centre the texture's window on each face.
            let uc = 0.25;
            let half = seg_l / (2.0 * l);
            wall(
                self.b_street.at(cx, cz),
                a,
                c,
                yb0,
                yb1,
                ROWHOUSE as f64 + 16.0 * seed,
                tile_b,
                g1,
                colr,
                [bcx - nx * 2.0, bcz - nz * 2.0],
                uc - tile_sign * half,
            );
        }
        // Bay soffit and cap, and the floor bands across the front.
        let bay_poly = bay;
        let p = self.b_plain.at(cx, cz);
        let out_poly: Vec<P2> = bay_poly
            .iter()
            .map(|q| [q[0] + nx * 0.06, q[1] + nz * 0.06])
            .collect();
        if b.tier == 0 {
            p.prism(&out_poly, yb1, yb1 + 0.28, &col(trim));
        }
        if full {
            p.prism(
                &out_poly,
                yb0 - 0.3,
                yb0,
                &PrismOpts {
                    color: Some(trim),
                    roof: Some(false),
                    ..PrismOpts::default()
                },
            );
        }
        let soff: Vec<P3> = bay_poly.iter().map(|q| [q[0], yb0 - 0.3, q[1]]).collect();
        let ny = (soff[1][2] - soff[0][2]) * (soff[2][0] - soff[0][0])
            - (soff[1][0] - soff[0][0]) * (soff[2][2] - soff[0][2]);
        if full && ny < 0.0 {
            p.quad(
                soff[0],
                soff[1],
                soff[2],
                soff[3],
                None,
                Some(scale3(trim, 0.7)),
                0.0,
            );
        } else if full {
            p.quad(
                soff[3],
                soff[2],
                soff[1],
                soff[0],
                None,
                Some(scale3(trim, 0.7)),
                0.0,
            );
        }
        let mut q = 1.0;
        while q < floors {
            p.box_(
                fmx + nx * 0.05,
                g1 + q * 3.4 - 0.1,
                fmz + nz * 0.05,
                l,
                0.2,
                0.12,
                yaw,
                &col(trim),
            );
            q += 1.0;
        }
        if b.tier == 0 {
            p.box_(
                fmx + nx * 0.05,
                g1 - 0.2,
                fmz + nz * 0.05,
                l,
                0.25,
                0.14,
                yaw,
                &col(trim),
            );
        }
        // Cornice with brackets, and a gable on some.
        p.box_(
            fmx + nx * 0.35,
            top - 0.15,
            fmz + nz * 0.35,
            l + 0.1,
            0.55,
            0.8,
            yaw,
            &col(trim),
        );
        if full {
            let mut u = 0.35;
            while u < l {
                let px_ = fa[0] + tx * u + nx * 0.2;
                let pz_ = fa[1] + tz * u + nz * 0.2;
                p.box_(px_, top - 0.55, pz_, 0.14, 0.4, 0.4, yaw, &col(trim));
                u += 0.9;
            }
        }
        // A gable or a parapet over the cornice (only near the route).
        if b.tier == 0 && self.rng() < 0.35 {
            let gh = 1.4 + self.rng() * 0.8;
            let gyb = top + 0.4;
            let a = [fa[0] + nx * 0.1, gyb, fa[1] + nz * 0.1];
            let bq = [fr.front[1][0] + nx * 0.1, gyb, fr.front[1][1] + nz * 0.1];
            let cq = [fmx + nx * 0.1, gyb + gh, fmz + nz * 0.1];
            // Outward-facing triangle (either winding works for one of the two).
            let n1 = (bq[1] - a[1]) * (cq[2] - a[2]) - (bq[2] - a[2]) * (cq[1] - a[1]);
            let nxz = (bq[0] - a[0]) * (cq[1] - a[1]) - (bq[1] - a[1]) * (cq[0] - a[0]);
            let out = n1 * nx + nxz * nz >= 0.0;
            let p = self.b_plain.at(cx, cz);
            if out {
                p.tri(a, bq, cq, Some(colr), 0.0);
            } else {
                p.tri(bq, a, cq, Some(colr), 0.0);
            }
            p.box_(
                fmx + nx * 0.2,
                gyb - 0.12,
                fmz + nz * 0.2,
                l,
                0.15,
                0.3,
                yaw,
                &col(trim),
            );
        } else if b.tier == 0 {
            self.b_plain.at(cx, cz).box_(
                fmx - nx * 0.1,
                top + 0.1,
                fmz - nz * 0.1,
                l,
                0.6,
                0.3,
                yaw,
                &col(scale3(colr, 0.9)),
            );
        }
        // Stoop: steps up to the door (texture door at u 0.78, sill 0.8 m up).
        let du = u_at(0.78);
        let sx = fa[0] + tx * du;
        let sz = fa[1] + tz * du;
        let steps = if full {
            5
        } else if b.tier == 0 {
            1
        } else {
            0
        };
        let p = self.b_plain.at(cx, cz);
        for k in 0..steps {
            let d = f64::from(steps - k) * 0.32;
            let px_ = sx + nx * d / 2.0;
            let pz_ = sz + nz * d / 2.0;
            p.box_(
                px_,
                street_y - 0.2,
                pz_,
                1.3,
                0.2 + f64::from(k + 1) * 0.16,
                d,
                yaw,
                &col([0.6, 0.58, 0.55]),
            );
        }
        if full {
            for s in [-0.7, 0.7] {
                p.box_(
                    sx + tx * s + nx * 0.8,
                    street_y + 0.1,
                    sz + tz * s + nz * 0.8,
                    0.07,
                    1.4,
                    1.6,
                    yaw,
                    &col(trim),
                );
            }
        }
        // Porch light by the door.
        if self.rng() < 0.7 {
            self.b_glow.at(sx, sz).box_(
                sx + tx * 0.9 + nx * 0.1,
                street_y + 2.9,
                sz + tz * 0.9 + nz * 0.1,
                0.16,
                0.24,
                0.16,
                yaw,
                &col([3.0, 2.1, 1.1]),
            );
        }
        self.fronts.push(Front {
            fa,
            tx,
            tz,
            nx,
            nz,
            l,
            y: street_y,
            district: 1,
            tier: b.tier,
            shop: None,
            lit: None,
        });
    }

    /// Office towers: a lit lobby podium, a shaft with setbacks, a crown.
    fn tower_block(&mut self, b: &Block, core: f64) {
        let (px, pz, hw, walk) = (self.g.px, self.g.pz, self.g.hw, self.g.walk);
        let (fi, fj) = (b.i as f64, b.j as f64);
        let bx0 = fi * px + hw + walk;
        let bx1 = (fi + 1.0) * px - hw - walk;
        let bz0 = fj * pz + hw + walk;
        let bz1 = (fj + 1.0) * pz - hw - walk;
        let lots_xz = if self.rng() < 0.5 {
            vec![[bx0, bz0, bx1, bz1]]
        } else {
            vec![
                [bx0, bz0, (bx0 + bx1) / 2.0 - 2.0, bz1],
                [(bx0 + bx1) / 2.0 + 2.0, bz0, bx1, bz1],
            ]
        };
        for [x0, z0, x1, z1] in lots_xz {
            if b.tier == 0 && self.lot_blocked(x0, z0, x1, z1) {
                // Shrink away from the route's corner.
                let s = 0.72;
                let cx = (x0 + x1) / 2.0;
                let cz = (z0 + z1) / 2.0;
                let l2 = [
                    cx - (cx - x0) * s,
                    cz - (cz - z0) * s,
                    cx + (x1 - cx) * s,
                    cz + (z1 - cz) * s,
                ];
                if self.lot_blocked(l2[0], l2[1], l2[2], l2[3]) {
                    continue;
                }
                self.tower(l2, core, b);
            } else {
                self.tower([x0, z0, x1, z1], core, b);
            }
        }
    }

    fn tower(&mut self, [x0, z0, x1, z1]: [f64; 4], core: f64, b: &Block) {
        let cx = (x0 + x1) / 2.0;
        let cz = (z0 + z1) / 2.0;
        let floor_y = ground(cx, cz) + 0.15;
        let base = lot_ground(x0, z0, x1, z1) - 1.0;
        // `const Bs = ..., Bf = ..., P = this.bPlain.at(cx, cz)`.
        self.b_street.at(cx, cz);
        self.b_fac.at(cx, cz);
        self.b_plain.at(cx, cz);
        let h = 60.0 + core * (90.0 + self.rng() * 170.0) + self.rng() * 40.0;
        let seed = self.seed();
        let fd = [floor_y, 0.8, 0.0];
        // Lobby: double-height glass all round.
        let pr = rect(x0 + 0.3, z0 + 0.3, x1 - 0.3, z1 - 0.3, 0.0);
        for k in 0..4 {
            wall(
                self.b_street.at(cx, cz),
                pr[k],
                pr[(k + 1) % 4],
                base,
                floor_y + 6.0,
                LOBBY as f64 + 16.0 * seed,
                S_TILE[LOBBY],
                floor_y,
                [1.0, 1.0, 1.0],
                [cx, cz],
                k as f64 * 0.37,
            );
        }
        // Podium: a few floors of stone or strip windows, capped.
        let pod_cell = self.pick(&[f::STONE, f::RIBBON, f::STONE, f::BANDS]);
        let pod_top = floor_y + 6.0 + floor_h(pod_cell) * (2.0 + (self.rng() * 4.0).floor());
        let u_off = (self.rng() * 4.0).floor() / 4.0;
        ring(
            self.b_fac.at(cx, cz),
            &pr,
            floor_y + 6.0,
            pod_top,
            pod_cell as f64 + 16.0 * seed,
            floor_y + 6.0,
            fd,
            [cx, cz],
            u_off,
        );
        let pan = f::PANEL as f64 + 16.0 * seed;
        let roof_cell = f::ROOF as f64 + 16.0 * seed;
        self.b_fac.at(cx, cz).box_(
            cx,
            pod_top - 0.4,
            cz,
            x1 - x0 - 0.2,
            0.9,
            z1 - z0 - 0.2,
            0.0,
            &panel(pan, fd, roof_cell, Some(16.0)),
        );
        // Entrance canopy on the side nearest the route, glowing underneath.
        {
            let sides = [
                [cx, z0 + 0.3, 0.0, -1.0],
                [cx, z1 - 0.3, 0.0, 1.0],
                [x0 + 0.3, cz, -1.0, 0.0],
                [x1 - 0.3, cz, 1.0, 0.0],
            ];
            let mut best = sides[0];
            let mut bd = f64::INFINITY;
            for s in sides {
                let d = self.route_dist(s[0], s[1]);
                if d < bd {
                    bd = d;
                    best = s;
                }
            }
            let [ex, ez, nx, nz] = best;
            let yaw = kernel::atan2(nx, -nz); // box length along the wall
            let w = js::min(
                10.0,
                (if nx != 0.0 { z1 - z0 } else { x1 - x0 }).abs() * 0.4,
            );
            self.b_plain.at(cx, cz).box_(
                ex + nx * 1.6,
                floor_y + 4.3,
                ez + nz * 1.6,
                w,
                0.35,
                3.2,
                yaw,
                &col([0.2, 0.2, 0.22]),
            );
            self.b_glow.at(ex, ez).box_(
                ex + nx * 1.6,
                floor_y + 4.26,
                ez + nz * 1.6,
                w - 0.6,
                0.04,
                2.6,
                yaw,
                &col([2.6, 2.3, 1.8]),
            );
            self.spill.push(Spill {
                x: ex + nx * 3.5,
                z: ez + nz * 3.5,
                r: w * 0.5,
                col: [0.45, 0.38, 0.26],
                tx: -nz,
                tz: nx,
                long: true,
            });
            self.fronts.push(Front {
                fa: [ex + nz * w / 2.0, ez - nx * w / 2.0],
                tx: -nz,
                tz: nx,
                nx,
                nz,
                l: w,
                y: floor_y,
                district: 2,
                tier: b.tier,
                shop: Some(LOBBY),
                lit: None,
            });
        }
        // Shaft: maybe chamfered corners, maybe piers; then setbacks.
        let style = self.pick(&[
            f::CURTAIN,
            f::STONE,
            f::BANDS,
            f::FINS,
            f::CURTAIN,
            f::DARK,
            f::RIBBON,
        ]);
        let ins = 4.0 + self.rng() * 6.0;
        let mut xx0 = x0 + ins;
        let mut xx1 = x1 - ins;
        let mut zz0 = z0 + ins;
        let mut zz1 = z1 - ins;
        let ch = if self.rng() < 0.35 {
            2.5 + self.rng() * 2.0
        } else {
            0.0
        };
        let shaft_top = pod_top + (h - (pod_top - floor_y)) * (0.62 + self.rng() * 0.2);
        let u_off = (self.rng() * 8.0).floor() / 8.0;
        let poly = rect(xx0, zz0, xx1, zz1, ch);
        ring(
            self.b_fac.at(cx, cz),
            &poly,
            pod_top - 0.3,
            shaft_top,
            style as f64 + 16.0 * seed,
            pod_top,
            fd,
            [cx, cz],
            u_off,
        );
        roof(
            self.b_fac.at(cx, cz),
            xx0,
            zz0,
            xx1,
            zz1,
            shaft_top,
            roof_cell,
            Some(fd),
        );
        if (style == f::STONE || style == f::CURTAIN || style == f::RIBBON) && self.rng() < 0.65 {
            self.piers(&poly, pod_top, shaft_top, pan, fd, [cx, cz], 3.0);
        }
        let mut top = shaft_top;
        let tiers = if self.rng() < 0.8 {
            1 + (if self.rng() < 0.4 { 1 } else { 0 })
        } else {
            0
        };
        let mut tier_style = style;
        for q in 0..tiers {
            let s = 3.0 + self.rng() * 5.0;
            if xx1 - xx0 - 2.0 * s < 10.0 || zz1 - zz0 - 2.0 * s < 10.0 {
                break;
            }
            xx0 += s;
            xx1 -= s;
            zz0 += s;
            zz1 -= s;
            let upper = if q == tiers - 1 {
                floor_y + h
            } else {
                top + (floor_y + h - top) * 0.55
            };
            // Setback terrace edge.
            self.b_fac.at(cx, cz).box_(
                (xx0 + xx1) / 2.0,
                top - 0.3,
                (zz0 + zz1) / 2.0,
                xx1 - xx0 + 2.0 * s + 0.4,
                0.8,
                zz1 - zz0 + 2.0 * s + 0.4,
                0.0,
                &panel(pan, fd, roof_cell, None),
            );
            if self.rng() < 0.3 {
                tier_style = f::DARK;
            }
            ring(
                self.b_fac.at(cx, cz),
                &rect(xx0, zz0, xx1, zz1, ch * 0.6),
                top - 0.3,
                upper,
                tier_style as f64 + 16.0 * seed,
                pod_top,
                fd,
                [cx, cz],
                u_off,
            );
            roof(
                self.b_fac.at(cx, cz),
                xx0,
                zz0,
                xx1,
                zz1,
                upper,
                roof_cell,
                Some(fd),
            );
            top = upper;
        }
        // Crown.
        let cr = self.rng();
        let cp = rect(xx0 - 0.2, zz0 - 0.2, xx1 + 0.2, zz1 + 0.2, ch * 0.6);
        if cr < 0.35 {
            // Lit louvred crown screen above the roof.
            ring(
                self.b_fac.at(cx, cz),
                &cp,
                top,
                top + 6.0,
                f::CROWN as f64 + 16.0 * seed,
                top,
                [0.0, 0.0, 0.0],
                [cx, cz],
                0.0,
            );
            top += 6.0;
        } else if cr < 0.6 {
            // Glass pyramid.
            let apex = [
                (xx0 + xx1) / 2.0,
                top + js::min(xx1 - xx0, zz1 - zz0) * 0.45,
                (zz0 + zz1) / 2.0,
            ];
            let r4 = rect(xx0, zz0, xx1, zz1, 0.0);
            let bf = self.b_fac.at(cx, cz);
            for k in 0..4 {
                let a = r4[k];
                let c = r4[(k + 1) % 4];
                let aa = [a[0], top, a[1]];
                let cc = [c[0], top, c[1]];
                // Face outward: test against the centre.
                let ex = cc[0] - aa[0];
                let ez = cc[2] - aa[2];
                let outward = (-ez) * (aa[0] - apex[0]) + ex * (aa[2] - apex[2]) >= 0.0;
                let lk = kernel::hypot(ex, ez) / 12.0;
                let dark = f::DARK as f64 + 16.0 * seed;
                if outward {
                    bf.tri_uv(
                        aa,
                        cc,
                        apex,
                        [0.0, 0.0],
                        [lk, 0.0],
                        [lk / 2.0, 1.2],
                        Some(fd),
                        dark,
                    );
                } else {
                    bf.tri_uv(
                        cc,
                        aa,
                        apex,
                        [0.0, 0.0],
                        [lk, 0.0],
                        [lk / 2.0, 1.2],
                        Some(fd),
                        dark,
                    );
                }
            }
            // Lit edges up the ridges.
            let g = self.b_glow.at(cx, cz);
            for a in &r4 {
                for q in 0..6 {
                    let u = f64::from(q) / 6.0;
                    g.box_(
                        lerp(a[0], apex[0], u),
                        lerp(top, apex[1], u),
                        lerp(a[1], apex[2], u),
                        0.35,
                        0.35,
                        0.35,
                        0.0,
                        &col([3.0, 2.6, 2.0]),
                    );
                }
            }
            top = apex[1];
        } else if cr < 0.85 {
            // LED band.
            let c = NEON[(self.rng() * NEON.len() as f64).floor() as usize];
            let g = self.b_glow.at(cx, cz);
            for k in 0..cp.len() {
                glow_wall(
                    g,
                    cp[k],
                    cp[(k + 1) % cp.len()],
                    top - 3.5,
                    top - 2.1,
                    c,
                    [cx, cz],
                );
            }
            ring(
                self.b_fac.at(cx, cz),
                &rect(
                    xx0 + (xx1 - xx0) * 0.25,
                    zz0 + (zz1 - zz0) * 0.25,
                    xx1 - (xx1 - xx0) * 0.25,
                    zz1 - (zz1 - zz0) * 0.25,
                    0.0,
                ),
                top,
                top + 4.5,
                f::MECH as f64 + 16.0 * seed,
                top,
                fd,
                [cx, cz],
                0.0,
            );
        } else {
            // Mechanical penthouse and a mast.
            let mx0 = xx0 + (xx1 - xx0) * 0.2;
            let mx1 = xx1 - (xx1 - xx0) * 0.2;
            let mz0 = zz0 + (zz1 - zz0) * 0.2;
            let mz1 = zz1 - (zz1 - zz0) * 0.2;
            ring(
                self.b_fac.at(cx, cz),
                &rect(mx0, mz0, mx1, mz1, 0.0),
                top,
                top + 5.0,
                f::MECH as f64 + 16.0 * seed,
                top,
                fd,
                [cx, cz],
                0.0,
            );
            roof(
                self.b_fac.at(cx, cz),
                mx0,
                mz0,
                mx1,
                mz1,
                top + 5.0,
                roof_cell,
                Some(fd),
            );
            let mh = 18.0 + self.rng() * 25.0;
            self.b_plain.at(cx, cz).box_(
                cx,
                top + 5.0,
                cz,
                0.5,
                mh,
                0.5,
                0.0,
                &col([0.5, 0.5, 0.52]),
            );
            top += 5.0 + mh;
        }
        if h > 120.0 || cr >= 0.85 {
            self.aircraft.push([cx, top + 1.0, cz]);
        }
    }

    /// Vertical piers proud of a façade every `pitch` metres: relief that
    /// catches the light and breaks up the silhouette at grazing angles.
    fn piers(&mut self, poly: &[P2], y0: f64, y1: f64, cell: f64, fd: P3, cxz: P2, pitch: f64) {
        let b = self.b_fac.at(cxz[0], cxz[1]);
        for k in 0..poly.len() {
            let a = poly[k];
            let c = poly[(k + 1) % poly.len()];
            let ex = c[0] - a[0];
            let ez = c[1] - a[1];
            let l = kernel::hypot(ex, ez);
            if l < pitch * 2.0 {
                continue;
            }
            let tx = ex / l;
            let tz = ez / l;
            let mut nx = -tz;
            let mut nz = tx;
            let mx = (a[0] + c[0]) / 2.0;
            let mz = (a[1] + c[1]) / 2.0;
            if nx * (mx - cxz[0]) + nz * (mz - cxz[1]) < 0.0 {
                nx = -nx;
                nz = -nz;
            }
            let n = js::round(l / pitch);
            let mut q = 1.0;
            while q < n {
                let u = (q / n) * l;
                b.box_(
                    a[0] + tx * u + nx * 0.18,
                    y0,
                    a[1] + tz * u + nz * 0.18,
                    0.45,
                    y1 - y0,
                    0.36,
                    kernel::atan2(tz, tx),
                    &panel(cell, fd, cell, None),
                );
                q += 1.0;
            }
        }
    }

    /// A small plaza in place of a block: paving, trees in planters, benches,
    /// lit bollards and a fountain.
    fn plaza(&mut self, b: &Block) {
        // `const P = this.bPlain.at(b.cx, b.cz)`.
        self.b_plain.at(b.cx, b.cz);
        let y = ground(b.cx, b.cz) + 0.15;
        for _ in 0..12 {
            let x = lerp(b.cx - 40.0, b.cx + 40.0, self.rng());
            let z = lerp(b.cz - 28.0, b.cz + 28.0, self.rng());
            if kernel::hypot(x - b.cx, z - b.cz) < 11.0 {
                continue;
            }
            let yy = ground(x, z) + 0.15;
            let s = 0.9 + self.rng() * 0.4;
            self.trees.push(Tree {
                x,
                y: yy + 0.6,
                z,
                s,
            });
            self.b_plain
                .at(b.cx, b.cz)
                .box_(x, yy, z, 2.4, 0.6, 2.4, 0.0, &col([0.45, 0.43, 0.4]));
            if self.rng() < 0.6 {
                let m = Matrix4::make_rotation_y(self.rng() * 6.28).set_position(x + 2.6, yy, z);
                self.put_kit_plain(b.cx, b.cz, "bench", &m, None);
            }
        }
        self.b_plain.at(b.cx, b.cz).box_(
            b.cx,
            y,
            b.cz,
            12.0,
            0.7,
            12.0,
            0.0,
            &col([0.45, 0.44, 0.42]),
        );
        self.b_glow.at(b.cx, b.cz).box_(
            b.cx,
            y + 0.7,
            b.cz,
            1.2,
            3.5,
            1.2,
            0.0,
            &col([0.6, 1.4, 2.2]),
        );
        self.b_glow.at(b.cx, b.cz).box_(
            b.cx,
            y + 0.72,
            b.cz,
            10.5,
            0.05,
            10.5,
            0.0,
            &col([0.12, 0.35, 0.55]),
        );
        let mut a = 0.0;
        while a < 6.28 {
            let x = b.cx + kernel::cos(a) * 16.0;
            let z = b.cz + kernel::sin(a) * 16.0;
            let yy = ground(x, z) + 0.15;
            self.put_kit_plain(
                b.cx,
                b.cz,
                "bollard",
                &Matrix4::IDENTITY.set_position(x, yy, z),
                None,
            );
            self.b_glow
                .at(x, z)
                .box_(x, yy + 0.72, z, 0.2, 0.12, 0.2, 0.0, &col([2.6, 2.3, 1.8]));
            a += 0.52;
        }
        self.spill.push(Spill {
            x: b.cx,
            z: b.cz,
            r: 14.0,
            col: [0.12, 0.14, 0.16],
            tx: 1.0,
            tz: 0.0,
            long: false,
        });
    }

    /// Far blocks: a few plain boxes, taller downtown.
    fn far_block(&mut self, b: &Block) {
        let (px, pz, hw, walk) = (self.g.px, self.g.pz, self.g.hw, self.g.walk);
        let (fi, fj) = (b.i as f64, b.j as f64);
        let bx0 = fi * px + hw + walk;
        let bx1 = (fi + 1.0) * px - hw - walk;
        let bz0 = fj * pz + hw + walk;
        let bz1 = (fj + 1.0) * pz - hw - walk;
        let r = kernel::hypot(b.cx - self.core[0], b.cz - self.core[1]);
        let core = if b.district == 2 {
            kernel::exp(-kernel::pow(r / 700.0, 2.0))
        } else {
            0.0
        };
        let halves = if self.rng() < 0.5 {
            [
                [bx0, bz0, bx1, (bz0 + bz1) / 2.0 - 1.0],
                [bx0, (bz0 + bz1) / 2.0 + 1.0, bx1, bz1],
            ]
        } else {
            [
                [bx0, bz0, (bx0 + bx1) / 2.0 - 1.0, bz1],
                [(bx0 + bx1) / 2.0 + 1.0, bz0, bx1, bz1],
            ]
        };
        for [x0, z0, x1, z1] in halves {
            if self.rng() < 0.1 {
                continue;
            }
            let cx = (x0 + x1) / 2.0;
            let cz = (z0 + z1) / 2.0;
            let tall = core > 0.2 && self.rng() < 0.3 + core * 0.5;
            let cell = if tall || (b.district == 2 && self.rng() < 0.5) {
                f::FAR_OFF
            } else {
                f::FAR_RES
            };
            let fh = floor_h(cell);
            let h = if tall {
                60.0 + core * 160.0 * self.rng() + 40.0 * self.rng()
            } else {
                fh * (if b.district == 1 {
                    3.0 + (self.rng() * 3.0).floor()
                } else {
                    3.0 + (self.rng() * 8.0).floor()
                })
            };
            let base = lot_ground(x0, z0, x1, z1) - 1.0;
            let gy0 = ground(cx, cz);
            let seed = self.seed();
            let fd = [gy0, 0.35, 0.0];
            let u_off = (self.rng() * 8.0).floor() / 8.0;
            let bb = self.b_fac.at(cx, cz);
            ring(
                bb,
                &[[x0, z0], [x1, z0], [x1, z1], [x0, z1]],
                base,
                gy0 + h,
                cell as f64 + 16.0 * seed,
                gy0,
                fd,
                [cx, cz],
                u_off,
            );
            roof(
                bb,
                x0,
                z0,
                x1,
                z1,
                gy0 + h,
                f::ROOF as f64 + 16.0 * seed,
                Some(fd),
            );
            if tall && self.rng() < 0.4 {
                let c = NEON[(self.rng() * NEON.len() as f64).floor() as usize];
                let r4 = [
                    [x0 - 0.2, z0 - 0.2],
                    [x1 + 0.2, z0 - 0.2],
                    [x1 + 0.2, z1 + 0.2],
                    [x0 - 0.2, z1 + 0.2],
                ];
                for k in 0..4 {
                    glow_wall(
                        self.b_glow.at(cx, cz),
                        r4[k],
                        r4[(k + 1) % 4],
                        gy0 + h - 3.0,
                        gy0 + h - 1.8,
                        c,
                        [cx, cz],
                    );
                }
            }
            if tall && h > 150.0 {
                self.aircraft.push([cx, gy0 + h + 1.0, cz]);
            }
        }
    }

    fn street_trees(&mut self, b: &Block, p: f64) {
        let (px, pz, hw) = (self.g.px, self.g.pz, self.g.hw);
        let (fi, fj) = (b.i as f64, b.j as f64);
        let edge = hw + 1.4;
        let mut x = fi * px + hw + 10.0;
        while x < (fi + 1.0) * px - hw - 8.0 {
            for z in [fj * pz + edge, (fj + 1.0) * pz - edge] {
                if self.rng() > p || !self.on_kerb(x, z) {
                    continue;
                }
                if self.t.distance_to_road(x, z, 20.0).d < hw + 1.0 {
                    continue;
                }
                let s = 0.7 + self.rng() * 0.3;
                self.trees.push(Tree {
                    x,
                    y: ground(x, z) + 0.15,
                    z,
                    s,
                });
            }
            x += 11.0;
        }
    }

    pub(super) fn build_trees(&mut self) {
        if self.trees.is_empty() {
            return;
        }
        let mut trunk = cylinder_geometry(0.15, 0.22, 3.2, 5.0, 1.0, false, 0.0, PI2);
        trunk.translate(0.0, 1.6, 0.0);
        let mut crown = icosahedron_geometry(2.2, 0.0);
        crown.scale(1.0, 1.2, 1.0);
        crown.translate(0.0, 4.5, 0.0);
        let paint = |g0: BufferGeometry, c: P3| -> BufferGeometry {
            let mut g = if g0.index.is_some() {
                g0.to_non_indexed()
            } else {
                g0
            };
            let n = g.position().count();
            let mut a = vec![0.0f64; n * 3];
            for i in 0..n {
                a[i * 3..i * 3 + 3].copy_from_slice(&c);
            }
            g.set_attribute("color", BufferAttribute::from_f64(&a, 3));
            g
        };
        let a = paint(trunk, [0.22, 0.15, 0.1]);
        let c = paint(crown, [0.14, 0.25, 0.1]);
        let mut geo = BufferGeometry::new();
        for n in ["position", "normal", "color"] {
            let aa = a
                .get_attribute(n)
                .expect("attribute")
                .as_f32()
                .expect("f32");
            let cc = c
                .get_attribute(n)
                .expect("attribute")
                .as_f32()
                .expect("f32");
            let mut arr = Vec::with_capacity(aa.len() + cc.len());
            arr.extend_from_slice(aa);
            arr.extend_from_slice(cc);
            geo.set_attribute(n, BufferAttribute::from_f32(arr, 3));
        }
        geo.compute_vertex_normals();
        let mat = self.graph.add_material(ambient_patch(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.95)
                .set("flatShading", true),
            [0.06, 0.07, 0.05],
            "tree",
        ));
        let trees = self.trees.clone();
        let mut ms = Vec::with_capacity(trees.len());
        for t in &trees {
            let yaw = self.rng() * 6.28;
            let sy = t.s * (0.85 + self.rng() * 0.3);
            ms.push(trs(t.x, t.y, t.z, yaw, t.s, sy, t.s, 0.0, 0.0));
        }
        let geo = self.graph.add_geometry(geo);
        let im = instanced(self.graph, geo, mat, &ms, true, true);
        for i in 0..trees.len() {
            let h = 0.24 + self.rng() * 0.08;
            let l = 0.4 + self.rng() * 0.2;
            let mut c = Color::default();
            c.set_hsl(h, 0.45, l);
            self.graph
                .get_mut(im)
                .instances
                .as_mut()
                .expect("instanced")
                .set_color_at(i, c);
        }
        self.graph.add(self.group, im);
    }

    pub(super) fn build_aircraft_lights(&mut self) {
        if self.aircraft.is_empty() {
            return;
        }
        let pos: Vec<f64> = self.aircraft.iter().flatten().copied().collect();
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.compute_bounding_sphere();
        let glow = self.textures.glow_texture();
        let glow = self.graph.cached_texture(&glow, Layer::Main, "");
        let m = self.graph.add_material(
            Material::points()
                .set("size", 3.5)
                .set("sizeAttenuation", false)
                .set("map", glow)
                .set("color", Color::new(6.0, 0.35, 0.25))
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", ADDITIVE),
        );
        let geo = self.graph.add_geometry(g);
        let pts = self.graph.drawable(NodeType::Points, geo, m);
        self.graph.get_mut(pts).frustum_culled = false;
        self.graph.add(self.group, pts);
        self.anims
            .push(Box::new(anim::Aircraft { time: 0.0, mat: m }));
        // Billboards collected while placing buildings.
        for bb in self.billboards.clone() {
            self.billboard(bb);
        }
    }

    fn billboard(&mut self, bb: Billboard) {
        let Billboard {
            ox,
            oz,
            y0,
            tx,
            tz,
            w,
            h,
            ad,
        } = bb;
        let mat = match self.ad_mats.iter().find(|(a, _)| *a == ad) {
            Some(&(_, m)) => m,
            None => {
                let t = ad_texture(self.textures, ad as i64);
                let t = self.graph.cached_texture(&t, Layer::Main, "");
                let m = self.graph.add_material(
                    Material::basic()
                        .set("map", t)
                        .set("color", Color::new(1.5, 1.5, 1.5))
                        .set("side", three::DOUBLE_SIDE as f64),
                );
                self.ad_mats.push((ad, m));
                m
            }
        };
        let p = [
            [ox, y0, oz],
            [ox + tx * w, y0, oz + tz * w],
            [ox + tx * w, y0 + h, oz + tz * w],
            [ox, y0 + h, oz],
        ];
        let mut pos = Vec::with_capacity(18);
        for k in [0, 1, 2, 0, 2, 3] {
            pos.extend_from_slice(&p[k]);
        }
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.set_attribute(
            "uv",
            BufferAttribute::from_f64(
                &[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0],
                2,
            ),
        );
        let geo = self.graph.add_geometry(g);
        let mesh = static_mesh(
            self.graph,
            geo,
            mat,
            &StaticOpts {
                receive: false,
                ..StaticOpts::default()
            },
        );
        self.graph.add(self.group, mesh);
        // The posts go into a builder Streets has already emitted, so
        // nothing draws them (as in the JS).
        let p2 = self.b_plain.at(ox, oz);
        for u in [0.2, 0.8] {
            p2.box_(
                ox + tx * w * u,
                y0 - 1.6,
                oz + tz * w * u,
                0.25,
                1.6,
                0.25,
                0.0,
                &col([0.2, 0.2, 0.22]),
            );
        }
        self.sign_lights.push(super::SignLight {
            x: ox + tx * w / 2.0,
            y: y0 + h / 2.0,
            z: oz + tz * w / 2.0,
            col: "#ffffff",
            lamp: false,
            big: true,
        });
    }
}
