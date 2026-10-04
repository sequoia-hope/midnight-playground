//! `src/world/Harbor.js` (roadmap WP 7.1): Port Meridian, zone 2 of Level 2:
//! the harbour bridge and the container docks. `harbor/build.js` is
//! [`build`], `harbor/textures.js` [`textures`].
//!
//! Lateral layout (m from our centreline, + = right, sea on the left):
//!   our lanes ±9.4, barriers to ±10.52; median −10.52…−12.02;
//!   westbound lanes −12.02…−31.82, barrier back −32.46;
//!   bridge girder edges +13.4 / −35.4, tower legs +15.3 / −37.3;
//!   container yard −45…−140, quay apron −150…−190 (sea beyond);
//!   sheds and depot on the right from +40.
//!
//! The port keeps the JS structure: [`Harbor::plan`] sets the runout, lays
//! out the westbound connector to the terminal gate and flattens the ground
//! under it; [`Harbor::build`] runs the JS `build*` methods in the JS order
//! into the group `harbor` (added to the world first, as the JS does),
//! batching static geometry by material and chunk ([`build::Batch`]), and
//! sets the westbound carriageway for traffic by the rule the simulation
//! reads (`mr_levels::world::Harbor`, DECISIONS D471). The updaters (the
//! towers' warning lamps, the lighting's lens colour, the finish gantry's
//! chase bulbs, the port's glow points, the boats and the breakwater lamps)
//! are [`Animator`]s in the JS order.

// Index loops stay index loops, and the JS signatures stay (D52, D130).
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod build;
pub mod textures;

use mr_levels::world::Harbor as WorldHarbor;
use mr_math::{Mulberry32, Noise2D, clamp, js, kernel, lerp, smoothstep};
use mr_scene::{NodeType, three};
use mr_track::{Frame, Tag, Track};
use serde_json::Value;

use crate::city::freeway::{
    FwPath, MED_C, OPP_BACK, OPP_C, OPP_FACE_IN, OPP_FACE_OUT, PFrame, Prof, SweepOpts, SweepUv,
    chunked, opp_y, our_y, prof, sweep,
};
use crate::city::textures::{BannerOpts, banner_texture};
use crate::coast::kit::{SignAtlas, panel};
use crate::color::Color;
use crate::geom::{GeoBuilder, P2, P3, PrismOpts, instanced, trs, yaw_of};
use crate::material::Material;
use crate::object::{Layer, MaterialId, NodeId, SceneGraph, TextureId};
use crate::terrain::Terrain;
use crate::textures::{Cached, SignOpts, TextureCache};
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, EulerOrder, Matrix4, Vector3, box_geometry,
    cylinder_geometry, dodecahedron_geometry, merge_geometries, plane_geometry, sphere_geometry,
};
use crate::world::{Animator, Change, Edit, Handle, Scenery, SceneryInfo, UpdateCtx, World};

use build::{
    AddOpts, Batch, RFrame, RProf, beam, col, frustum, quad_out, ribbon, rprof, span_matrix, tint,
};

const PI: f64 = core::f64::consts::PI;
const DOUBLE_SIDE: f64 = three::DOUBLE_SIDE as f64;
const ADDITIVE: f64 = three::ADDITIVE_BLENDING as f64;

const FWD_EXT: f64 = WorldHarbor::FWD_EXT;
const JERSEY: [[f64; 2]; 6] = [
    [-0.02, 0.0],
    [0.02, 0.25],
    [0.2, 0.36],
    [0.28, 1.0],
    [0.42, 1.0],
    [0.62, 0.25],
];
const GIRDER_R: f64 = 13.4;
const GIRDER_L: f64 = -35.4;
const GIRDER_D: f64 = 3.4;
const LEG_R: f64 = 15.3;
const LEG_L: f64 = -37.3;
const TOWER_TOP: f64 = 160.0;
/// The quay face.
const QUAY: f64 = -190.0;
/// The start of the quay apron.
const APRON_IN: f64 = -148.0;
const CONT: [f64; 3] = [12.19, 2.59, 2.44];

// Line liveries (weighted by repetition): navy, sky blue, rust red, orange,
// green, greys, reefer white, yellow, teal, brown, magenta, beige.
const CONTAINER_COLS: [u32; 22] = [
    0x1d4f8a, 0x2a6fb0, 0x5d9bc7, 0x5d9bc7, 0xb3342b, 0x9c3a2a, 0xd8612a, 0x2f7d3a, 0x2f6d4a,
    0x8a8f96, 0x6f757c, 0xd9d4c7, 0xe6e6e0, 0xe0b52b, 0x1f7f86, 0x6b3b2a, 0x7a4a32, 0xa8306a,
    0xc9b98f, 0x36414d, 0x1d4f8a, 0xb3342b,
];
/// The rail spur on the right, between the road and the sheds.
const RAIL_LATS: [f64; 2] = [22.0, 26.5];
const SHED_NAMES: [(&str, &str); 5] = [
    ("MERIDIAN LOGISTICS", "#1d4f7a"),
    ("PIER 7 COLD STORE", "#2f6d4a"),
    ("NORDSTAR SHIPPING", "#8a2a24"),
    ("BAYSIDE CARGO", "#36414d"),
    ("WAREHOUSE 12", "#6b3b2a"),
];

const WHITE_LINE: P3 = [0.92, 0.92, 0.9];
const YELLOW_LINE: P3 = [0.95, 0.72, 0.12];

/// The westbound connector: frames from the carriageway's west end, curving
/// toward the quay and levelling out at the gate plaza.
#[derive(Clone, Debug)]
pub struct Connector {
    pub frames: Vec<RFrame>,
    pub p3: P2,
    pub t3: P2,
    pub gate: RFrame,
}

/// The Port Meridian scenery module.
pub struct Harbor {
    pub zone: usize,
    pub label: &'static str,
    pub z0: f64,
    pub span: Option<Tag>,
    pub up: Option<Tag>,
    pub down: Option<Tag>,
    /// `sWS`: where the westbound carriageway starts.
    pub s_ws: f64,
    pub connector: Option<Connector>,
    /// The group `harbor`, once built.
    pub group: Option<NodeId>,
}

impl Harbor {
    /// `new Harbor({ zone = 2 })`.
    pub fn new(info: &SceneryInfo) -> Harbor {
        Harbor {
            zone: info.zone,
            label: "Building the harbour",
            z0: 0.0,
            span: None,
            up: None,
            down: None,
            s_ws: 0.0,
            connector: None,
            group: None,
        }
    }

    /// Bézier from the westbound carriageway's west end, curving (to its
    /// traffic's right) toward the quay and levelling out at the gate plaza.
    fn make_connector(&self, t: &Track, terrain: &Terrain) -> Connector {
        let f0 = t.frame(self.s_ws);
        let fe = t.frame(self.s_ws - 150.0);
        let p0 = [f0.x + f0.rx * OPP_C, f0.z + f0.rz * OPP_C];
        let t0 = [-f0.fx, -f0.fz];
        let p3 = [fe.x + fe.rx * -84.0, fe.z + fe.rz * -84.0];
        let mut t3 = [-fe.fx * 0.5 - fe.rx * 0.87, -fe.fz * 0.5 - fe.rz * 0.87];
        let l3 = kernel::hypot(t3[0], t3[1]);
        t3 = [t3[0] / l3, t3[1] / l3];
        let p1 = [p0[0] + t0[0] * 70.0, p0[1] + t0[1] * 70.0];
        let p2 = [p3[0] - t3[0] * 50.0, p3[1] - t3[1] * 50.0];
        let y0 = opp_y(&f0);
        let y1 = terrain
            .flat_y
            .get(self.zone)
            .copied()
            .flatten()
            .unwrap_or(f0.y)
            + 0.12;
        let bez = |u: f64| -> P2 {
            let v = 1.0 - u;
            [
                v * v * v * p0[0]
                    + 3.0 * v * v * u * p1[0]
                    + 3.0 * v * u * u * p2[0]
                    + u * u * u * p3[0],
                v * v * v * p0[1]
                    + 3.0 * v * v * u * p1[1]
                    + 3.0 * v * u * u * p2[1]
                    + u * u * u * p3[1],
            ]
        };
        let mut pts: Vec<P2> = (0..=60).map(|i| bez(f64::from(i) / 60.0)).collect();
        // Extend straight past P3 to the plaza.
        for k in 1..=8 {
            let k = f64::from(k);
            pts.push([p3[0] + t3[0] * k * 6.0, p3[1] + t3[1] * k * 6.0]);
        }
        let mut dist = 0.0;
        let mut cum = vec![0.0];
        for i in 1..pts.len() {
            dist += kernel::hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]);
            cum.push(dist);
        }
        let bend_len = cum[60];
        let n = pts.len();
        let frames: Vec<RFrame> = pts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let a = pts[i.saturating_sub(1)];
                let b = pts[(i + 1).min(n - 1)];
                let mut fx = b[0] - a[0];
                let mut fz = b[1] - a[1];
                let l = js::or(kernel::hypot(fx, fz), 1.0);
                fx /= l;
                fz /= l;
                let y = lerp(
                    y0,
                    y1,
                    smoothstep(0.08, 0.95, js::min(1.0, cum[i] / bend_len)),
                );
                RFrame {
                    x: p[0],
                    z: p[1],
                    fx,
                    fz,
                    y,
                }
            })
            .collect();
        Connector {
            frames,
            p3,
            t3,
            gate: RFrame {
                x: p3[0] + t3[0] * 26.0,
                z: p3[1] + t3[1] * 26.0,
                y: y1,
                fx: t3[0],
                fz: t3[1],
            },
        }
    }
}

impl Scenery for Harbor {
    fn name(&self) -> &str {
        "Harbor"
    }

    fn label(&self) -> Option<&str> {
        Some(self.label)
    }

    fn plan(&mut self, w: &mut World) -> Result<(), String> {
        let World {
            track,
            terrain,
            sim_data,
            ..
        } = w;
        let t = track.as_mut().ok_or("the route is surveyed first")?;
        let tr = terrain.as_mut().ok_or("no terrain")?;
        self.z0 = t.zone_start[self.zone] as f64;
        self.span = t.tag("bridge").first().map(|g| (*g).clone());
        self.up = t.tag("bridge-up").first().map(|g| (*g).clone());
        self.down = t.tag("bridge-down").first().map(|g| (*g).clone());
        // The westbound carriageway starts just before the climb; west of
        // that it peels away to the right (for its traffic) into the
        // terminal gate.
        self.s_ws = WorldHarbor::s_ws(t, self.zone);
        t.runout = WorldHarbor::plan_runout(t.runout); // buildForward draws it; drivable after the finish
        sim_data.runout = t.runout;
        let connector = self.make_connector(t, tr);
        // Embankment under the connector, kept clear of our viaduct.
        let mut i = 0;
        while i < connector.frames.len() {
            let fr = connector.frames[i];
            let p = t.project_window(fr.x, fr.z, self.s_ws - 60.0, 60);
            let clear = p.lat.abs() - 10.6;
            let r = clamp(clear - 10.0, 2.0, 11.0);
            let fall = clamp(clear - r - 1.0, 1.5, 16.0);
            tr.add_flatten(fr.x, fr.z, r, fall, Some(fr.y - 0.35));
            i += 3;
        }
        let g = connector.gate;
        tr.add_flatten(g.x, g.z, 26.0, 18.0, Some(g.y - 0.3));
        self.connector = Some(connector);
        Ok(())
    }

    fn build(&mut self, w: &mut World) -> Result<(), String> {
        let World {
            track,
            terrain,
            graph,
            textures,
            root,
            animators,
            sim_data,
            ..
        } = w;
        let t = track.as_ref().ok_or("the route is surveyed first")?;
        let tr = terrain.as_ref().ok_or("no terrain")?;
        let connector = self.connector.clone().ok_or("Harbor.plan has not run")?;
        let group = graph.group("harbor");
        graph.add(*root, group);
        let path = FwPath::new(t, self.s_ws, t.length, 0.0, FWD_EXT);
        let towers = match &self.span {
            Some(s) => vec![s.s0 + 225.0, s.s1 - 225.0],
            None => Vec::new(),
        };
        let z0 = self.z0;
        let down_s1 = self.down.as_ref().map(|d| d.s1);
        let gantry_sites = vec![
            GantrySite {
                s: z0 + 95.0,
                signs: vec![
                    GSign {
                        lines: vec!["I-9 EAST", "Harbor Bridge"],
                        arrow: Some("up"),
                        lane: -3.2,
                    },
                    GSign {
                        lines: vec!["PORT MERIDIAN", "Terminal 3", "2 MILES"],
                        arrow: None,
                        lane: 4.6,
                    },
                ],
            },
            GantrySite {
                s: down_s1.unwrap_or(z0 + 1940.0) + 110.0,
                signs: vec![
                    GSign {
                        lines: vec!["PORT MERIDIAN", "Terminal 3"],
                        arrow: Some("right"),
                        lane: 4.6,
                    },
                    GSign {
                        lines: vec!["DOWNTOWN", "MERIDIAN 4"],
                        arrow: Some("up"),
                        lane: -3.2,
                    },
                ],
            },
            GantrySite {
                s: t.finish_s - 320.0,
                signs: vec![
                    GSign {
                        lines: vec!["EXIT 7", "Terminal Rd"],
                        arrow: Some("right"),
                        lane: 4.6,
                    },
                    GSign {
                        lines: vec!["I-9 EAST", "Downtown Meridian"],
                        arrow: Some("up"),
                        lane: -3.2,
                    },
                ],
            },
        ];
        let mut reserved: Vec<[f64; 2]> = gantry_sites
            .iter()
            .map(|g| [g.s - 10.0, g.s + 10.0])
            .collect();
        reserved.push([t.finish_s - 10.0, t.finish_s + 10.0]);
        for &s in &towers {
            reserved.push([s - 14.0, s + 14.0]);
        }
        let mut b = Build {
            t,
            terrain: tr,
            graph,
            textures,
            group,
            batch: Batch::new(group),
            rng: Mulberry32::new(8080),
            path,
            s_ws: self.s_ws,
            span: self.span.clone(),
            up: self.up.clone(),
            down: self.down.clone(),
            z0,
            ground_y: tr.flat_y.get(self.zone).copied().flatten().unwrap_or(6.0) - 0.3,
            towers,
            gantry_sites,
            reserved,
            mats: Vec::new(),
            connector,
            animators: Vec::new(),
            marks: GeoBuilder::new(true, false),
            containers: Vec::new(),
            yard_blocks: Vec::new(),
            straddle_u: Vec::new(),
            amber_beacons: Vec::new(),
            red_beacons: Vec::new(),
            crane_floods: Vec::new(),
            ship_lamps: Vec::new(),
            bollards: Vec::new(),
            dock_doors: Vec::new(),
            ship: None,
            cranes: None,
        };

        b.build_opposite();
        b.build_connector();
        b.build_forward();
        b.build_girder();
        b.build_opp_piers();
        b.build_towers();
        b.build_cables();
        b.build_lighting();
        b.build_gantries();
        b.build_finish();
        b.build_paving();
        b.build_quay();
        b.build_yard();
        b.build_rtgs();
        b.build_cranes();
        b.build_ship();
        b.build_port_lights();
        b.build_rail();
        b.build_sheds();
        b.build_trucks();
        if !b.marks.is_empty() {
            let g = b.marks.build();
            let m = b.marking_mat();
            b.batch.add(
                b.graph,
                g,
                m,
                AddOpts {
                    receive: true,
                    ..AddOpts::default()
                },
            );
        }
        b.build_boats();
        b.build_breakwaters();
        b.flush_containers();
        b.batch.flush(b.graph);
        // Static from here on: one matrix update each.
        let children = b.graph.get(group).children.clone();
        for c in children {
            let o = b.graph.get_mut(c);
            let drawn = matches!(
                o.ty,
                NodeType::Mesh | NodeType::InstancedMesh | NodeType::Points
            );
            if drawn && o.user_data.get("animated") != Some(&Value::Bool(true)) {
                o.matrix_auto_update = false;
                o.update_matrix();
            }
        }
        let new_animators = std::mem::take(&mut b.animators);
        animators.extend(new_animators);
        // For traffic: westbound lanes alongside our track (lat relative to it).
        sim_data.opposite_carriageway = Some(WorldHarbor::opposite_carriageway(t, self.zone));
        self.group = Some(group);
        Ok(())
    }
}

/// A sign on a gantry: `{ lines, arrow, lane }`.
struct GSign {
    lines: Vec<&'static str>,
    arrow: Option<&'static str>,
    lane: f64,
}

/// A gantry site: `{ s, signs }`.
struct GantrySite {
    s: f64,
    signs: Vec<GSign>,
}

/// A container: `{ x, y, z, yaw, c, v }`.
#[derive(Clone, Copy, Debug)]
struct Container {
    x: f64,
    y: f64,
    z: f64,
    yaw: f64,
    c: u32,
    v: f64,
}

/// A line of paint: `{ lat, w, c, dash }`.
#[derive(Clone, Copy, Debug)]
struct Line {
    lat: f64,
    w: f64,
    c: P3,
    dash: Option<[f64; 2]>,
}

fn line(lat: f64, w: f64, c: P3) -> Line {
    Line {
        lat,
        w,
        c,
        dash: None,
    }
}

fn dashed(lat: f64, w: f64, c: P3, dash: [f64; 2]) -> Line {
    Line {
        lat,
        w,
        c,
        dash: Some(dash),
    }
}

/// A yard block's extent along the path.
#[derive(Clone, Copy, Debug)]
struct Block {
    u0: f64,
    u1: f64,
}

/// `this.ship`: the hull's frame.
#[derive(Clone, Copy, Debug)]
struct Ship {
    cx: f64,
    cz: f64,
    dx: f64,
    dz: f64,
    nx: f64,
    nz: f64,
    l: f64,
}

/// `{ texture, aspect }` of a cached sign.
fn sign_of(
    graph: &mut SceneGraph,
    textures: &mut TextureCache,
    lines: &[&str],
    o: &SignOpts,
) -> (TextureId, f64) {
    let c = textures.sign_texture(lines, o);
    let aspect = match &*c {
        Cached::Sign { aspect, .. } => *aspect,
        _ => unreachable!("signTexture returns a sign"),
    };
    (graph.cached_texture(&c, Layer::Main, ""), aspect)
}

/// `(f) => oppY(f)` for a sweep's profile.
fn opp(f: &PFrame, _: f64) -> f64 {
    opp_y(f)
}

/// What `build()` holds as `this` while it runs.
struct Build<'a> {
    t: &'a Track,
    terrain: &'a Terrain,
    graph: &'a mut SceneGraph,
    textures: &'a mut TextureCache,
    group: NodeId,
    batch: Batch,
    rng: Mulberry32,
    path: FwPath,
    s_ws: f64,
    span: Option<Tag>,
    up: Option<Tag>,
    down: Option<Tag>,
    z0: f64,
    ground_y: f64,
    towers: Vec<f64>,
    gantry_sites: Vec<GantrySite>,
    reserved: Vec<[f64; 2]>,
    /// `this.mats`, by key in the order made.
    mats: Vec<(&'static str, MaterialId)>,
    connector: Connector,
    animators: Vec<Box<dyn Animator>>,
    /// Painted yard markings, flushed with the markings material.
    marks: GeoBuilder,
    containers: Vec<Container>,
    yard_blocks: Vec<Block>,
    straddle_u: Vec<f64>,
    amber_beacons: Vec<P3>,
    red_beacons: Vec<P3>,
    crane_floods: Vec<P3>,
    ship_lamps: Vec<P3>,
    bollards: Vec<Vector3>,
    dock_doors: Vec<(f64, f64)>,
    ship: Option<Ship>,
    cranes: Option<Vec<f64>>,
}

impl Build<'_> {
    fn add(&mut self, n: NodeId) {
        self.graph.add(self.group, n);
    }

    fn badd(&mut self, g: BufferGeometry, mat: MaterialId, cast: bool, chunk: f64) {
        self.batch.add(
            self.graph,
            g,
            mat,
            AddOpts {
                cast,
                receive: true,
                chunk,
            },
        );
    }

    fn mat(&mut self, key: &'static str, make: impl FnOnce(&mut Self) -> MaterialId) -> MaterialId {
        if let Some(&(_, m)) = self.mats.iter().find(|(k, _)| *k == key) {
            return m;
        }
        let m = make(self);
        self.mats.push((key, m));
        m
    }

    fn concrete(&mut self) -> MaterialId {
        self.mat("concrete", |b| {
            let map = b
                .graph
                .cached_texture(&b.textures.concrete_texture(), Layer::Main, "");
            b.graph.add_material(
                Material::standard()
                    .set("map", map)
                    .set("roughness", 0.9)
                    .set("color", 0xdcd8d0),
            )
        })
    }

    /// Vertex-coloured painted steel.
    fn paint(&mut self) -> MaterialId {
        self.mat("paint", |b| {
            b.graph.add_material(
                Material::standard()
                    .set("vertexColors", true)
                    .set("metalness", 0.35)
                    .set("roughness", 0.5),
            )
        })
    }

    /// Vertex-coloured matte surfaces.
    fn matte(&mut self) -> MaterialId {
        self.mat("matte", |b| {
            b.graph.add_material(
                Material::standard()
                    .set("vertexColors", true)
                    .set("roughness", 0.85),
            )
        })
    }

    fn asphalt(&mut self) -> MaterialId {
        self.mat("asphalt", |b| {
            let map = b
                .graph
                .cached_texture(&b.textures.asphalt_texture(2), Layer::Main, "");
            b.graph
                .add_material(Material::standard().set("map", map).set("roughness", 0.88))
        })
    }

    fn pole_mat(&mut self) -> MaterialId {
        self.mat("pole", |b| {
            b.graph.add_material(
                Material::standard()
                    .set("color", 0x80868c)
                    .set("metalness", 0.6)
                    .set("roughness", 0.5),
            )
        })
    }

    fn marking_mat(&mut self) -> MaterialId {
        self.mat("markings", |b| {
            let m = b.graph.add_material(
                Material::standard()
                    .set("vertexColors", true)
                    .set("roughness", 0.55)
                    .set("emissive", 0xffffff)
                    .set("emissiveIntensity", 0.0)
                    .set("polygonOffset", true)
                    .set("polygonOffsetFactor", -2.0)
                    .set("polygonOffsetUnits", -2.0),
            );
            b.graph.add_night(Some(m), "emissiveIntensity", 0.0, 0.06);
            m
        })
    }

    fn elev(&self, u: f64) -> bool {
        u >= self.path.s_a && u <= self.path.s_b && self.terrain.is_elevated(self.t, self.t.idx(u))
    }

    fn in_span(&self, u: f64) -> bool {
        self.span
            .as_ref()
            .is_some_and(|s| u > s.s0 - 12.0 && u < s.s1 + 12.0)
    }

    fn pframe(&self, u: f64) -> PFrame {
        self.path.frame(self.t, u)
    }

    /// `ranges(test, a, b, step = 2)`.
    fn ranges(&self, test: impl Fn(&Self, f64) -> bool, a: f64, b: f64) -> Vec<[f64; 2]> {
        let mut out = Vec::new();
        let mut st: Option<f64> = None;
        let mut u = a;
        while u <= b {
            let ok = test(self, u);
            if ok && st.is_none() {
                st = Some(u);
            }
            if !ok && let Some(s) = st {
                out.push([s, u]);
                st = None;
            }
            u += 2.0;
        }
        if let Some(s) = st {
            out.push([s, b]);
        }
        out
    }

    fn sweep(&self, ranges: &[[f64; 2]], profile: &[Prof], o: &SweepOpts) -> BufferGeometry {
        sweep(self.t, &self.path, ranges, profile, o)
    }

    // ── Westbound carriageway ───────────────────────────────────────────

    fn build_opposite(&mut self) {
        let path = self.path.clone();
        let conc = self.concrete();
        let asphalt = self.asphalt();
        let all = [[path.u0, path.u1]];
        let elev = self.ranges(|b, u| b.elev(u), path.u0, path.u1);
        let ground = self.ranges(|b, u| !b.elev(u), path.u0, path.u1);
        let o48 = SweepOpts {
            step: 4.0,
            u_s: 4.8,
            v_s: 10.0,
            ..SweepOpts::default()
        };
        for r in chunked(&all, 300.0) {
            let g = self.sweep(
                &[r],
                &[
                    prof(OPP_FACE_OUT, opp),
                    prof(OPP_C, opp),
                    prof(OPP_FACE_IN, opp),
                ],
                &o48,
            );
            self.badd(g, asphalt, false, 700.0);
        }
        // Median: westbound barrier and the strip back to ours.
        let med_profile = || {
            let mut p: Vec<Prof> = JERSEY
                .iter()
                .map(|&[o, dy]| prof(OPP_FACE_IN + o, move |f, _| opp_y(f) + dy))
                .collect();
            p.push(prof(-11.38, |f, _| opp_y(f) + 0.25));
            p.push(prof(-10.52, |f, _| our_y(f, -10.52) + 0.25));
            p
        };
        let s4 = SweepOpts {
            step: 4.0,
            ..SweepOpts::default()
        };
        for r in chunked(&all, 300.0) {
            let g = self.sweep(&[r], &med_profile(), &s4);
            self.badd(g, conc, true, 700.0);
        }
        // Outer barrier, with a skirt on the ground and a fascia when
        // elevated.
        let outer = |skirt: bool| {
            let mut p: Vec<Prof> = if skirt {
                vec![
                    prof(OPP_BACK - 3.2, |f, _| opp_y(f) - 2.8),
                    prof(OPP_BACK, |f, _| opp_y(f) - 0.02),
                ]
            } else {
                vec![prof(OPP_BACK, |f, _| opp_y(f) - 1.8)]
            };
            for &[o, dy] in JERSEY.iter().rev() {
                p.push(prof(OPP_FACE_OUT - o, move |f, _| opp_y(f) + dy));
            }
            p
        };
        for r in chunked(&ground, 300.0) {
            let g = self.sweep(&[r], &outer(true), &s4);
            self.badd(g, conc, true, 700.0);
        }
        for r in chunked(&elev, 300.0) {
            let g = self.sweep(&[r], &outer(false), &s4);
            self.badd(g, conc, true, 700.0);
        }
        // Deck underside where elevated (the deep girder hides it on the
        // bridge).
        for r in chunked(&elev, 300.0) {
            let g = self.sweep(
                &[r],
                &[
                    prof(-10.52, |f, _| our_y(f, -10.52) - 1.8),
                    prof(OPP_BACK, |f, _| opp_y(f) - 1.8),
                ],
                &s4,
            );
            self.badd(g, conc, false, 700.0);
        }
        // Markings.
        let l = OPP_C + 9.4 - 1.2;
        let r = OPP_C - 9.4 + 2.0;
        let lw = (l - r) / 4.0;
        let mut lines = vec![line(l, 0.15, YELLOW_LINE), line(r, 0.18, WHITE_LINE)];
        for q in 1..4 {
            lines.push(dashed(l - lw * f64::from(q), 0.14, WHITE_LINE, [3.0, 12.0]));
        }
        self.add_markings(&lines, &all, &|f, _| opp_y(f), None);
    }

    /// `addMarkings(lines, ranges, yOf, frameAt = path.frame)`.
    fn add_markings(
        &mut self,
        lines: &[Line],
        ranges: &[[f64; 2]],
        y_of: &dyn Fn(&Frame, f64) -> f64,
        frame_at: Option<&dyn Fn(f64) -> Frame>,
    ) {
        let mut pos: Vec<f64> = Vec::new();
        let mut colr: Vec<f64> = Vec::new();
        for &[a, b] in ranges {
            let mut s = a;
            while s < b - 1.0 {
                let (f0, f1) = match frame_at {
                    Some(fa) => (fa(s), fa(s + 1.0)),
                    None => (self.pframe(s).f, self.pframe(s + 1.0).f),
                };
                for ln in lines {
                    if let Some(d) = ln.dash
                        && ((s + 0.5) % d[1] + d[1]) % d[1] > d[0]
                    {
                        continue;
                    }
                    let mut q: Vec<P3> = Vec::with_capacity(4);
                    for fr in [&f0, &f1] {
                        for side in [-1.0, 1.0] {
                            let lat = ln.lat + side * ln.w * 0.5;
                            q.push([fr.x + fr.rx * lat, y_of(fr, lat) + 0.02, fr.z + fr.rz * lat]);
                        }
                    }
                    for v in [q[0], q[1], q[2], q[1], q[3], q[2]] {
                        pos.extend_from_slice(&v);
                        colr.extend_from_slice(&ln.c);
                    }
                }
                s += 1.0;
            }
        }
        if pos.is_empty() {
            return;
        }
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.set_attribute("color", BufferAttribute::from_f64(&colr, 3));
        g.compute_vertex_normals();
        let m = self.marking_mat();
        self.batch.add(
            self.graph,
            g,
            m,
            AddOpts {
                receive: true,
                ..AddOpts::default()
            },
        );
    }

    // ── Westbound connector to the terminal gate ────────────────────────

    fn build_connector(&mut self) {
        let frames = self.connector.frames.clone();
        let conc = self.concrete();
        let asphalt = self.asphalt();
        let w = 9.9;
        let fy = |fr: &RFrame, _: f64| fr.y;
        let g = ribbon(
            &frames,
            &[rprof(-w, fy), rprof(0.0, fy), rprof(w, fy)],
            4.8,
            10.0,
        );
        self.badd(g, asphalt, false, 700.0);
        // Barriers both sides, with retaining walls down into the ground.
        let mut right: Vec<RProf> = JERSEY
            .iter()
            .map(|&[o, dy]| rprof(w + o, move |fr, _| fr.y + dy))
            .collect();
        right.push(rprof(w + 0.62, |fr, _| fr.y - 4.0));
        let mut left: Vec<RProf> = vec![rprof(-w - 0.62, |fr, _| fr.y - 4.0)];
        for &[o, dy] in JERSEY.iter().rev() {
            left.push(rprof(-w - o, move |fr, _| fr.y + dy));
        }
        let g = ribbon(&frames, &right, 4.0, 8.0);
        self.badd(g, conc, true, 700.0);
        let g = ribbon(&frames, &left, 4.0, 8.0);
        self.badd(g, conc, true, 700.0);
        // Lane lines.
        let l = -w + 1.2;
        let r = w - 2.0;
        let lw = (r - l) / 4.0;
        let mut lines = vec![line(l, 0.15, YELLOW_LINE), line(r, 0.18, WHITE_LINE)];
        for q in 1..4 {
            lines.push(dashed(l + lw * f64::from(q), 0.14, WHITE_LINE, [3.0, 12.0]));
        }
        let mut cum = vec![0.0];
        for i in 1..frames.len() {
            cum.push(
                cum[i - 1]
                    + kernel::hypot(frames[i].x - frames[i - 1].x, frames[i].z - frames[i - 1].z),
            );
        }
        let frame_at = |d: f64| -> Frame {
            let mut i = 0;
            while i < cum.len() - 2 && cum[i + 1] < d {
                i += 1;
            }
            let a = frames[i];
            let b = frames[i + 1];
            let k = clamp((d - cum[i]) / js::or(cum[i + 1] - cum[i], 1.0), 0.0, 1.0);
            let fx = lerp(a.fx, b.fx, k);
            let fz = lerp(a.fz, b.fz, k);
            let l = js::or(kernel::hypot(fx, fz), 1.0);
            let ofx = fx / l;
            let ofz = fz / l;
            Frame {
                x: lerp(a.x, b.x, k),
                z: lerp(a.z, b.z, k),
                y: lerp(a.y, b.y, k),
                fx: ofx,
                fz: ofz,
                rx: -ofz,
                rz: ofx,
                ..Frame::default()
            }
        };
        let last = cum[cum.len() - 1];
        self.add_markings(
            &lines,
            &[[0.0, last - 30.0]],
            &|fr, _| fr.y,
            Some(&frame_at),
        );
        self.build_gate_plaza();
    }

    fn build_gate_plaza(&mut self) {
        let g = self.connector.gate;
        let mut geo = GeoBuilder::new(true, false);
        let (fx, fz) = (g.fx, g.fz);
        let (rx, rz) = (-fz, fx);
        let p = |o: f64, a: f64| -> P2 { [g.x + rx * o + fx * a, g.z + rz * o + fz * a] };
        let y = g.y;
        let along = kernel::atan2(fz, fx);
        // Canopy on five posts, booths between the lanes.
        let white = col(0xe8e8e4);
        let blue = col(0x1d4f7a);
        let grey = col(0x8a8f96);
        for o in [-10.4, -5.2, 0.0, 5.2, 10.4] {
            let q = p(o, 0.0);
            geo.box_(q[0], y, q[1], 0.7, 7.2, 0.7, along, &copts(grey));
            if o.abs() < 10.0 {
                let bb = p(o, -2.0);
                geo.box_(bb[0], y, bb[1], 3.6, 2.8, 1.6, along, &roofed(white, blue));
            }
        }
        let c = p(0.0, 0.0);
        geo.box_(
            c[0],
            y + 7.2,
            c[1],
            12.0,
            1.4,
            23.0,
            along,
            &roofed(white, grey),
        );
        geo.box_(c[0], y + 8.6, c[1], 13.0, 0.25, 24.0, along, &copts(blue));
        // Plaza slab beyond.
        let s = p(0.0, 16.0);
        geo.box_(
            s[0],
            y - 0.3,
            s[1],
            30.0,
            0.32,
            28.0,
            along,
            &copts(col(0x55575b)),
        );
        let matte = self.matte();
        self.badd(geo.build(), matte, true, 700.0);
        // Name board on the canopy facing arriving traffic.
        let (texture, aspect) = sign_of(
            self.graph,
            self.textures,
            &["PORT MERIDIAN", "TERMINAL GATE"],
            &SignOpts {
                bg: "#123a52",
                fg: "#ffffff",
                border: Some("#ffffff"),
                w: 512,
                h: 160,
                font: "bold 54px \"Arial Narrow\", Arial, sans-serif",
                arrow: None,
            },
        );
        let m = self.graph.add_material(
            Material::standard()
                .set("map", texture)
                .set("roughness", 0.6),
        );
        let h = 1.3;
        let w = h * aspect;
        let pg = self.graph.add_geometry(plane_geometry(w, h, 1.0, 1.0));
        let board = self.graph.mesh(pg, m);
        let bp = p(0.0, -6.2);
        {
            let o = self.graph.get_mut(board);
            o.position = Vector3::new(bp[0], y + 7.9, bp[1]);
            o.set_rotation(&Euler::new(0.0, kernel::atan2(-fx, -fz), 0.0));
        }
        self.add(board);
    }

    /// Our lanes and barriers carrying on past the end of the track.
    fn build_forward(&mut self) {
        let path = self.path.clone();
        if path.u1 <= path.s_b {
            return;
        }
        let conc = self.concrete();
        let r = [[path.s_b - 0.5, path.u1]];
        let asphalt = self.asphalt();
        let fy = |f: &PFrame, _: f64| f.y;
        let g = self.sweep(
            &r,
            &[
                prof(-10.25, |f, _| f.y - 1.6),
                prof(-10.25, fy),
                prof(0.0, fy),
                prof(10.25, fy),
                prof(10.25, |f, _| f.y - 1.6),
            ],
            &SweepOpts {
                step: 6.0,
                u_s: 4.8,
                v_s: 10.0,
                ..SweepOpts::default()
            },
        );
        self.badd(g, asphalt, false, 700.0);
        let mut right: Vec<Prof> = JERSEY
            .iter()
            .map(|&[o, dy]| prof(9.9 + o, move |f, _| f.y + dy))
            .collect();
        right.push(prof(10.52, |f, _| f.y - 1.8));
        let g = self.sweep(
            &r,
            &right,
            &SweepOpts {
                step: 6.0,
                ..SweepOpts::default()
            },
        );
        self.badd(g, conc, true, 700.0);
        let l = -9.4 + 1.2;
        let rr = 9.4 - 2.0;
        let lw = (rr - l) / 4.0;
        let mut lines = vec![line(l, 0.15, YELLOW_LINE), line(rr, 0.18, WHITE_LINE)];
        for q in 1..4 {
            lines.push(dashed(l + lw * f64::from(q), 0.14, WHITE_LINE, [3.0, 12.0]));
        }
        self.add_markings(&lines, &r, &|f, _| f.y, None);
    }

    // ── Bridge ──────────────────────────────────────────────────────────

    /// A painted steel box girder under both carriageways wherever the
    /// deck is high: cantilevered edge beams carry the stay-cable anchors.
    fn build_girder(&mut self) {
        let (Some(up), Some(down)) = (self.up.clone(), self.down.clone()) else {
            return;
        };
        let t = self.t;
        let a = up.s0 + 60.0;
        let b = down.s1 - 60.0;
        let top = |f: &PFrame, _: f64| our_y(f, 10.52) - 0.06;
        let top_o = |f: &PFrame, _: f64| opp_y(f) - 0.06;
        // The sweep wants increasing lat for upward faces; this profile
        // walks right → down → left → up, which gives outward normals
        // throughout.
        let steel = self.mat("girder", |b| {
            b.graph.add_material(
                Material::standard()
                    .set("color", 0x356f8c)
                    .set("metalness", 0.45)
                    .set("roughness", 0.45),
            )
        });
        for r in chunked(&[[a, b]], 280.0) {
            let prof_v = [
                prof(10.52, top),
                prof(GIRDER_R, top),
                prof(GIRDER_R, |f, _| f.y - 2.2),
                prof(GIRDER_R - 1.6, |f, _| f.y - GIRDER_D),
                prof(GIRDER_L + 1.6, |f, _| f.y - GIRDER_D),
                prof(GIRDER_L, |f, _| f.y - 2.2),
                prof(GIRDER_L, top_o),
                prof(OPP_BACK, top_o),
            ];
            let g = self.sweep(
                &[r],
                &prof_v,
                &SweepOpts {
                    step: 4.0,
                    uv: SweepUv::Wall,
                    u_s: 6.0,
                    v_s: 4.0,
                    color: None,
                },
            );
            self.badd(g, steel, true, 700.0);
        }
        // End faces.
        let mut geo = GeoBuilder::new(false, false);
        for (s, dir) in [(a, -1.0), (b, 1.0)] {
            let f = t.frame(s);
            let q = |lat: f64, y: f64| -> P3 { [f.x + f.rx * lat, y, f.z + f.rz * lat] };
            quad_out(
                &mut geo,
                q(GIRDER_R, f.y - 0.06),
                q(GIRDER_L, f.y - 0.06),
                q(GIRDER_L, f.y - GIRDER_D),
                q(GIRDER_R, f.y - GIRDER_D),
                [f.fx * dir, 0.0, f.fz * dir],
                None,
                None,
            );
        }
        self.badd(geo.build(), steel, false, 700.0);
    }

    fn build_opp_piers(&mut self) {
        let t = self.t;
        let runs = self.ranges(|b, u| b.elev(u), self.s_ws, t.length);
        let mut cols: Vec<Matrix4> = Vec::new();
        let mut caps: Vec<Matrix4> = Vec::new();
        for [s0, s1] in runs {
            let mut s = s0 + 15.0;
            while s < s1 - 5.0 {
                let cur = s;
                s += 32.0;
                if self.in_span(cur) {
                    continue;
                }
                let f = t.frame(cur);
                let top = opp_y(&f) - 2.8;
                let yaw = yaw_of(f.fx, f.fz);
                for lat in [OPP_C - 9.4 * 0.55, OPP_C + 9.4 * 0.55] {
                    let x = f.x + f.rx * lat;
                    let z = f.z + f.rz * lat;
                    let gy = self.terrain.height_at(x, z) - 0.5;
                    cols.push(trs(
                        x,
                        gy,
                        z,
                        yaw,
                        1.0,
                        js::max(0.5, top - gy),
                        1.0,
                        0.0,
                        0.0,
                    ));
                }
                let cx = f.x + f.rx * OPP_C;
                let cz = f.z + f.rz * OPP_C;
                caps.push(trs(
                    cx,
                    top + 0.4,
                    cz,
                    yaw,
                    1.0,
                    1.0,
                    9.4 * 2.0 + 2.0,
                    0.0,
                    0.0,
                ));
            }
        }
        if cols.is_empty() {
            return;
        }
        let mut col_geo = cylinder_geometry(1.1, 1.3, 1.0, 10.0, 1.0, false, 0.0, PI * 2.0);
        col_geo.translate(0.0, 0.5, 0.0);
        let cap_geo = box_geometry(2.2, 1.2, 1.0, 1.0, 1.0, 1.0);
        let conc = self.concrete();
        let cg = self.graph.add_geometry(col_geo);
        let n = instanced(self.graph, cg, conc, &cols, true, true);
        self.add(n);
        let pg = self.graph.add_geometry(cap_geo);
        let n = instanced(self.graph, pg, conc, &caps, true, true);
        self.add(n);
    }

    /// Two H-frame towers: legs outside both carriageways on caissons, a
    /// crossbeam carrying the deck and a portal at the top.
    fn build_towers(&mut self) {
        let t = self.t;
        let mut geo = GeoBuilder::new(false, false);
        let mut warn: Vec<P3> = Vec::new();
        for st in self.towers.clone() {
            let f = t.frame(st);
            let deck = f.y;
            let p = |lat: f64, a: f64| -> P2 {
                [f.x + f.rx * lat + f.fx * a, f.z + f.rz * lat + f.fz * a]
            };
            for lat in [LEG_R, LEG_L] {
                let [x, z] = p(lat, 0.0);
                // Caisson (pile cap) at the waterline.
                let mut cais = cylinder_geometry(7.5, 8.5, 20.0, 20.0, 1.0, false, 0.0, PI * 2.0);
                cais.translate(x, -16.0 + 10.0, z);
                let conc = self.concrete();
                self.badd(cais, conc, true, 700.0);
                // Leg: tapered, slightly chamfered look from two stacked
                // frustums.
                let lo = deck - GIRDER_D - 4.0;
                frustum(
                    &mut geo, x, z, 4.0, lo, f.fx, f.fz, 11.0, 6.0, 9.0, 5.0, None, 8.0,
                );
                frustum(
                    &mut geo,
                    x,
                    z,
                    lo,
                    TOWER_TOP - 6.0,
                    f.fx,
                    f.fz,
                    9.0,
                    5.0,
                    6.2,
                    3.8,
                    None,
                    8.0,
                );
                frustum(
                    &mut geo,
                    x,
                    z,
                    TOWER_TOP - 6.0,
                    TOWER_TOP,
                    f.fx,
                    f.fz,
                    6.2,
                    3.8,
                    5.2,
                    3.2,
                    None,
                    8.0,
                );
                // Pilasters proud of the faces: the cross section becomes a
                // cross, so each face shows a pair of crisp vertical shadow
                // lines.
                frustum(
                    &mut geo, x, z, 6.0, lo, f.fx, f.fz, 6.6, 6.9, 5.6, 5.8, None, 8.0,
                );
                frustum(
                    &mut geo,
                    x,
                    z,
                    lo,
                    TOWER_TOP - 8.0,
                    f.fx,
                    f.fz,
                    5.6,
                    5.8,
                    3.4,
                    4.6,
                    None,
                    8.0,
                );
                frustum(
                    &mut geo,
                    x,
                    z,
                    TOWER_TOP,
                    TOWER_TOP + 1.2,
                    f.fx,
                    f.fz,
                    5.8,
                    3.8,
                    5.8,
                    3.8,
                    None,
                    8.0,
                );
                warn.push([x, TOWER_TOP + 0.6, z]);
            }
            // Crossbeam under the deck and portal beams above it.
            let mut beam_at = |y: f64, h: f64, d: f64| {
                let a = p(LEG_L, 0.0);
                let b = p(LEG_R, 0.0);
                beam(&mut geo, [a[0], y, a[1]], [b[0], y, b[1]], d, h, None, true);
            };
            beam_at(deck - GIRDER_D - 2.2, 4.2, 7.0);
            beam_at(TOWER_TOP - 10.0, 5.0, 4.4);
            beam_at(deck + 50.0, 3.6, 3.6);
            let mid = p((LEG_L + LEG_R) / 2.0, 0.0);
            warn.push([mid[0], TOWER_TOP - 7.2, mid[1]]);
        }
        let conc = self.concrete();
        self.badd(geo.build(), conc, true, 2000.0);
        // Aircraft warning lights.
        let lamp = self
            .graph
            .add_material(Material::basic().set("color", Color::new(6.0, 0.4, 0.3)));
        let sg = self
            .graph
            .add_geometry(sphere_geometry(0.7, 8.0, 6.0, 0.0, PI * 2.0, 0.0, PI));
        let mats: Vec<Matrix4> = warn
            .iter()
            .map(|p| trs(p[0], p[1], p[2], 0.0, 1.0, 1.0, 1.0, 0.0, 0.0))
            .collect();
        let lamp_mesh = instanced(self.graph, sg, lamp, &mats, false, false);
        self.add(lamp_mesh);
        let mut time = 0.0;
        self.animators
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                time += u.dt;
                let on = (time % 1.6) < 0.5;
                out.push(Edit {
                    target: Handle::Material(lamp),
                    change: Change::Color {
                        prop: "color",
                        rgb: if on {
                            [6.0, 0.4, 0.3]
                        } else {
                            [0.5, 0.05, 0.04]
                        },
                    },
                });
            }));
    }

    /// Semi-fan stay cables in two vertical planes per tower.
    fn build_cables(&mut self) {
        let t = self.t;
        let mut mats: Vec<Matrix4> = Vec::new();
        let mut anchors = GeoBuilder::new(true, false);
        for st in self.towers.clone() {
            let ft = t.frame(st);
            for (leg_lat, anchor_lat, opp) in [(LEG_R, 13.0, false), (LEG_L, -35.0, true)] {
                for dir in [-1.0, 1.0] {
                    for k in 0..11 {
                        let k = f64::from(k);
                        let sa = st + dir * (28.0 + 18.4 * k);
                        let fa = t.frame(sa);
                        let deck_y = if opp {
                            opp_y(&fa)
                        } else {
                            our_y(&fa, anchor_lat)
                        } + 0.2;
                        let a = [fa.x + fa.rx * anchor_lat, deck_y, fa.z + fa.rz * anchor_lat];
                        let hy = 108.0 + 4.4 * k;
                        // Each stay is a pair of strands side by side, with a
                        // damper sleeve where it meets the deck and an anchor
                        // block.
                        for off in [-0.32, 0.32] {
                            let a2 = [a[0] + fa.rx * off, a[1], a[2] + fa.rz * off];
                            let b2 = [
                                ft.x + ft.rx * (leg_lat + off) + ft.fx * dir * 2.4,
                                hy,
                                ft.z + ft.rz * (leg_lat + off) + ft.fz * dir * 2.4,
                            ];
                            mats.push(span_matrix(a2, b2, 0.14));
                            let d = [b2[0] - a2[0], b2[1] - a2[1], b2[2] - a2[2]];
                            let l = kernel::hypot3(d[0], d[1], d[2]);
                            let e = [
                                a2[0] + d[0] / l * 5.0,
                                a2[1] + d[1] / l * 5.0,
                                a2[2] + d[2] / l * 5.0,
                            ];
                            mats.push(span_matrix(a2, e, 0.3));
                        }
                        anchors.box_(
                            a[0],
                            a[1] - 0.55,
                            a[2],
                            1.6,
                            0.7,
                            1.4,
                            kernel::atan2(fa.fz, fa.fx),
                            &copts(col(0x356f8c)),
                        );
                    }
                }
            }
        }
        if mats.is_empty() {
            return;
        }
        let m = self.graph.add_material(
            Material::standard()
                .set("color", 0xf0f0ec)
                .set("metalness", 0.3)
                .set("roughness", 0.35),
        );
        let g = self.graph.add_geometry(cylinder_geometry(
            1.0,
            1.0,
            1.0,
            6.0,
            1.0,
            true,
            0.0,
            PI * 2.0,
        ));
        let n = instanced(self.graph, g, m, &mats, false, false);
        self.add(n);
        let paint = self.paint();
        self.badd(anchors.build(), paint, true, 2000.0);
    }

    // ── Road furniture ──────────────────────────────────────────────────

    fn reserved_at(&self, u: f64) -> bool {
        self.reserved.iter().any(|&[a, b]| u > a && u < b)
    }

    fn build_lighting(&mut self) {
        let path = self.path.clone();
        let mut geo = GeoBuilder::new(false, false);
        let mut heads: Vec<Matrix4> = Vec::new();
        let mut u = path.u0 + 20.0;
        while u < path.u1 - 10.0 {
            let cur = u;
            u += 48.0;
            if self.reserved_at(cur) {
                continue;
            }
            let f = self.pframe(cur);
            let x = f.x + f.rx * MED_C;
            let z = f.z + f.rz * MED_C;
            let base = js::min(our_y(&f, -10.5), opp_y(&f)) - 0.2;
            let top = base + 12.2;
            let arm_yaw = kernel::atan2(f.rz, f.rx);
            let none = PrismOpts::default();
            geo.box_(x, base, z, 0.36, top - base, 0.36, arm_yaw, &none);
            geo.box_(x, top - 0.35, z, 13.4, 0.18, 0.18, arm_yaw, &none);
            for lat in [-4.6 + 0.8, OPP_C + 5.4 - 0.8] {
                let hx = f.x + f.rx * lat;
                let hz = f.z + f.rz * lat;
                geo.box_(
                    hx,
                    top - 0.35,
                    hz,
                    1.3,
                    0.22,
                    0.45,
                    kernel::atan2(f.fz, f.fx),
                    &none,
                );
                heads.push(trs(hx, top - 0.39, hz, 0.0, 1.1, 0.06, 0.4, 0.0, 0.0));
            }
        }
        let pole = self.pole_mat();
        self.badd(geo.build(), pole, false, 700.0);
        let lens = self
            .graph
            .add_material(Material::basic().set("color", Color::new(1.2, 1.1, 0.9)));
        let bg = self
            .graph
            .add_geometry(box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0));
        let n = instanced(self.graph, bg, lens, &heads, false, false);
        self.add(n);
        self.animators
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                let k = 0.3 + 0.7 * smoothstep(0.2, 0.7, u.night);
                out.push(Edit {
                    target: Handle::Material(lens),
                    change: Change::Color {
                        prop: "color",
                        rgb: [4.5 * k, 3.2 * k, 1.8 * k],
                    },
                });
            }));
    }

    fn build_gantries(&mut self) {
        let t = self.t;
        let steel = self.mat("gantrySteel", |b| {
            b.graph.add_material(
                Material::standard()
                    .set("color", 0x9aa0a6)
                    .set("metalness", 0.6)
                    .set("roughness", 0.45),
            )
        });
        let mut geo = GeoBuilder::new(false, false);
        let sites = std::mem::take(&mut self.gantry_sites);
        for gs in &sites {
            let f = t.frame(gs.s);
            let yaw = kernel::atan2(f.fz, f.fx);
            let across_yaw = kernel::atan2(f.rz, f.rx);
            let p = |lat: f64, along: f64| -> P2 {
                [
                    f.x + f.rx * lat + f.fx * along,
                    f.z + f.rz * lat + f.fz * along,
                ]
            };
            let road = f.y + f.bank.abs() * 10.5;
            for lat in [11.15, MED_C] {
                let q = p(lat, 0.0);
                let base = js::min(
                    our_y(&f, js::max(-10.5, js::min(10.5, lat))),
                    self.terrain.height_at(q[0], q[1]),
                ) - 0.2;
                geo.box_(
                    q[0],
                    base,
                    q[1],
                    0.5,
                    road + 9.9 - base,
                    0.5,
                    yaw,
                    &PrismOpts {
                        roof: Some(true),
                        ..PrismOpts::default()
                    },
                );
            }
            let span = 11.15 - MED_C + 0.6;
            for (dy, along) in [(7.6, -0.5), (7.6, 0.5), (9.5, -0.5), (9.5, 0.5)] {
                let q = p((11.15 + MED_C) / 2.0, along);
                geo.box_(
                    q[0],
                    road + dy,
                    q[1],
                    span,
                    0.16,
                    0.16,
                    across_yaw,
                    &PrismOpts::default(),
                );
            }
            let mut lat = MED_C + 1.0;
            while lat < 11.0 {
                for along in [-0.5, 0.5] {
                    let q = p(lat, along);
                    geo.box_(
                        q[0],
                        road + 7.6,
                        q[1],
                        0.1,
                        1.9,
                        0.1,
                        across_yaw,
                        &PrismOpts::default(),
                    );
                }
                lat += 1.6;
            }
            for sg in &gs.signs {
                let font = format!(
                    "bold {}px \"Arial Narrow\", Arial, sans-serif",
                    if sg.lines.len() > 2 { 54 } else { 64 }
                );
                let (texture, aspect) = sign_of(
                    self.graph,
                    self.textures,
                    &sg.lines,
                    &SignOpts {
                        bg: "#0b6b3a",
                        fg: "#fff",
                        border: Some("#fff"),
                        w: 512,
                        h: 256,
                        font: &font,
                        arrow: sg.arrow,
                    },
                );
                let h = 3.0;
                let w = h * aspect;
                let m = self.graph.add_material(
                    Material::standard()
                        .set("map", texture)
                        .set("emissive", 0xffffff)
                        .set("emissiveMap", texture)
                        .set("emissiveIntensity", 0.1)
                        .set("roughness", 0.5),
                );
                self.graph
                    .add_night(Some(m), "emissiveIntensity", 0.05, 0.42);
                let pg = self.graph.add_geometry(plane_geometry(w, h, 1.0, 1.0));
                let mesh = self.graph.mesh(pg, m);
                let q = p(sg.lane, -0.75);
                {
                    let o = self.graph.get_mut(mesh);
                    o.position = Vector3::new(q[0], road + 6.8 + h / 2.0, q[1]);
                    o.set_rotation(&Euler::new(0.0, kernel::atan2(-f.fx, -f.fz), 0.0));
                }
                self.add(mesh);
                let bq = p(sg.lane, -0.65);
                geo.box_(
                    bq[0],
                    road + 6.75,
                    bq[1],
                    w + 0.1,
                    h + 0.1,
                    0.12,
                    across_yaw,
                    &PrismOpts::default(),
                );
            }
        }
        self.gantry_sites = sites;
        self.badd(geo.build(), steel, true, 700.0);
    }

    fn build_finish(&mut self) {
        let t = self.t;
        let f = t.frame(t.finish_s);
        let across_yaw = kernel::atan2(f.rz, f.rx);
        let yaw = kernel::atan2(f.fz, f.fx);
        let p = |lat: f64, along: f64| -> P2 {
            [
                f.x + f.rx * lat + f.fx * along,
                f.z + f.rz * lat + f.fz * along,
            ]
        };
        let mut geo = GeoBuilder::new(false, false);
        let road = f.y + f.bank.abs() * 10.5;
        let none = PrismOpts::default();
        for lat in [11.4, MED_C] {
            let q = p(lat, 0.0);
            geo.box_(q[0], road - 0.3, q[1], 0.8, 11.2, 0.8, yaw, &none);
        }
        let mid = p((11.4 + MED_C) / 2.0, 0.0);
        let span = 11.4 - MED_C + 0.8;
        geo.box_(
            mid[0],
            road + 9.8,
            mid[1],
            span,
            0.5,
            1.2,
            across_yaw,
            &none,
        );
        geo.box_(
            mid[0],
            road + 6.9,
            mid[1],
            span,
            0.35,
            1.0,
            across_yaw,
            &none,
        );
        let mat = self.graph.add_material(
            Material::standard()
                .set("color", 0x202226)
                .set("metalness", 0.6)
                .set("roughness", 0.4),
        );
        self.badd(geo.build(), mat, true, 700.0);
        let tex = banner_texture(
            "FINISH",
            &BannerOpts {
                w: 1024,
                h: 256,
                checker: true,
                font: "italic 900 150px \"Arial Narrow\", Arial, sans-serif",
                ..BannerOpts::default()
            },
        );
        let tex = crate::city::freeway::own_texture(self.graph, tex);
        let bm = self.graph.add_material(
            Material::standard()
                .set("map", tex)
                .set("emissive", 0xffffff)
                .set("emissiveMap", tex)
                .set("emissiveIntensity", 0.3)
                .set("roughness", 0.6)
                .set("side", DOUBLE_SIDE),
        );
        self.graph
            .add_night(Some(bm), "emissiveIntensity", 0.15, 0.6);
        let w = span - 0.9;
        let pg = self.graph.add_geometry(plane_geometry(w, 2.6, 1.0, 1.0));
        let banner = self.graph.mesh(pg, bm);
        {
            let o = self.graph.get_mut(banner);
            o.position = Vector3::new(mid[0] - f.fx * 0.62, road + 8.4, mid[1] - f.fz * 0.62);
            o.set_rotation(&Euler::new(0.0, kernel::atan2(-f.fx, -f.fz), 0.0));
        }
        self.add(banner);
        let mut bulbs: Vec<Matrix4> = Vec::new();
        let mut lat = MED_C + 0.4;
        while lat <= 11.0 {
            for dy in [7.0, 9.8] {
                let q = p(lat, -0.65);
                bulbs.push(trs(q[0], road + dy, q[1], 0.0, 0.18, 0.18, 0.18, 0.0, 0.0));
            }
            lat += 0.7;
        }
        let bulb_mat = self
            .graph
            .add_material(Material::basic().set("color", Color::new(5.0, 4.0, 2.4)));
        let sg = self
            .graph
            .add_geometry(sphere_geometry(1.0, 8.0, 6.0, 0.0, PI * 2.0, 0.0, PI));
        let bulb_mesh = instanced(self.graph, sg, bulb_mat, &bulbs, false, false);
        {
            let inst = self
                .graph
                .get_mut(bulb_mesh)
                .instances
                .as_mut()
                .expect("instanced");
            for i in 0..bulbs.len() {
                inst.set_color_at(i, Color::new(1.0, 1.0, 1.0));
            }
        }
        self.add(bulb_mesh);
        let n = bulbs.len();
        let mut time = 0.0;
        self.animators
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                time += u.dt;
                let ph = (time * 8.0).floor();
                for i in 0..n {
                    let v = if ((((i >> 1) as f64) + ph) % 3.0) == 0.0 {
                        1.0
                    } else {
                        0.12
                    };
                    let c = Color::new(v, v, v);
                    out.push(Edit {
                        target: Handle::Node(bulb_mesh),
                        change: Change::InstanceColor {
                            index: i as u32,
                            rgb: [c.r as f32, c.g as f32, c.b as f32],
                        },
                    });
                }
            }));
    }

    // ── Container terminal ──────────────────────────────────────────────

    fn port_start(&self) -> f64 {
        self.down.as_ref().map_or(self.z0 + 1940.0, |d| d.s1) + 40.0
    }

    /// Painted mark on the yard: a quad across the path at u, from lat0 to
    /// lat1, du0..du1 along it (`y = groundY + 0.07`).
    fn mark(&mut self, u: f64, lat0: f64, lat1: f64, du0: f64, du1: f64, c: P3) {
        let y = self.ground_y + 0.07;
        let f = self.pframe(u);
        let p = |lat: f64, d: f64| -> P3 {
            [f.x + f.rx * lat + f.fx * d, y, f.z + f.rz * lat + f.fz * d]
        };
        quad_out(
            &mut self.marks,
            p(lat0, du0),
            p(lat1, du0),
            p(lat1, du1),
            p(lat0, du1),
            [0.0, 1.0, 0.0],
            Some(c),
            None,
        );
    }

    /// The yard is paved: concrete slabs from the westbound barrier out to
    /// the quay apron, and on the right out past the sheds. Vertex colours
    /// carry a broad wash of wear so the 16 m tile doesn't read as a repeat.
    fn build_paving(&mut self) {
        let path = self.path.clone();
        let y = self.ground_y + 0.04;
        let a = self.port_start() - 40.0;
        let b = path.u1 - 5.0;
        let mat = self.mat("paving", |bb| {
            let map =
                bb.graph
                    .cached_texture(&textures::paving_texture(bb.textures), Layer::Main, "");
            bb.graph.add_material(
                Material::standard()
                    .set("map", map)
                    .set("vertexColors", true)
                    .set("roughness", 0.92)
                    .set("polygonOffset", true)
                    .set("polygonOffsetFactor", -1.0)
                    .set("polygonOffsetUnits", -1.0),
            )
        });
        let n = Noise2D::new(606);
        let bands = [[APRON_IN + 0.3, -35.8], [11.2, 236.0]];
        // Next to our road the ground rises and falls with its embankment:
        // the paving follows the terrain there so neither shows through the
        // other.
        let terrain = self.terrain;
        for [l0, l1] in bands {
            let mut lats: Vec<f64> = if l0 > 0.0 {
                vec![l0, 12.5, 14.0, 16.0, 18.0]
            } else {
                Vec::new()
            };
            let mut l = if l0 > 0.0 { 20.0 } else { l0 };
            while l < l1 {
                lats.push(l);
                l += 10.0;
            }
            lats.push(l1);
            for r in chunked(&[[a, b]], 300.0) {
                let prof_v: Vec<Prof> = lats
                    .iter()
                    .map(|&lat| {
                        prof(lat, move |fr: &PFrame, lat: f64| {
                            if lat > 0.0 && lat < 20.0 {
                                js::max(
                                    y,
                                    terrain.height_at(fr.x + fr.rx * lat, fr.z + fr.rz * lat)
                                        + 0.05,
                                )
                            } else {
                                y
                            }
                        })
                    })
                    .collect();
                let mut g = self.sweep(
                    &[r],
                    &prof_v,
                    &SweepOpts {
                        step: 8.0,
                        u_s: 16.0,
                        v_s: 16.0,
                        ..SweepOpts::default()
                    },
                );
                let p = g.position();
                let cnt = p.count();
                let mut c = vec![0.0f32; cnt * 3];
                for i in 0..cnt {
                    let x = p.get_x(i);
                    let z = p.get_z(i);
                    let v = 0.82
                        + 0.14 * n.noise(x / 60.0, z / 60.0)
                        + 0.06 * n.noise(x / 13.0 + 7.0, z / 13.0);
                    c[i * 3] = v as f32;
                    c[i * 3 + 1] = (v * 0.99) as f32;
                    c[i * 3 + 2] = (v * 0.97) as f32;
                }
                g.set_attribute("color", BufferAttribute::from_f32(c, 3));
                self.batch.add(
                    self.graph,
                    g,
                    mat,
                    AddOpts {
                        receive: true,
                        chunk: 1500.0,
                        cast: false,
                    },
                );
            }
        }
    }

    /// Concrete quay apron with a crisp face to the sea, bollards and
    /// fenders, and the crane rails the ship-to-shore cranes run on.
    fn build_quay(&mut self) {
        let path = self.path.clone();
        let g0 = self.ground_y + 0.15;
        let a = self.port_start();
        let b = path.u1 - 20.0;
        let conc = self.concrete();
        let matte = self.matte();
        for r in chunked(&[[a, b]], 300.0) {
            let g = self.sweep(
                &[r],
                &[
                    prof(QUAY, |_, _| -2.5),
                    prof(QUAY, move |_, _| g0),
                    prof(APRON_IN, move |_, _| g0),
                    prof(APRON_IN + 0.5, move |_, _| g0 - 0.6),
                ],
                &SweepOpts {
                    step: 6.0,
                    uv: SweepUv::Road,
                    u_s: 8.0,
                    v_s: 8.0,
                    color: None,
                },
            );
            self.badd(g, conc, false, 700.0);
            // Coping: a dark capping strip along the quay edge.
            let g = self.sweep(
                &[r],
                &[
                    prof(QUAY - 0.05, move |_, _| g0 - 0.6),
                    prof(QUAY - 0.05, move |_, _| g0 + 0.18),
                    prof(QUAY + 0.6, move |_, _| g0 + 0.18),
                    prof(QUAY + 0.6, move |_, _| g0),
                ],
                &SweepOpts {
                    step: 6.0,
                    color: Some(col(0x4a4c50)),
                    ..SweepOpts::default()
                },
            );
            self.badd(g, matte, false, 700.0);
        }
        let mut geo = GeoBuilder::new(true, false);
        let mut bollards: Vec<Matrix4> = Vec::new();
        let mut u = a + 4.0;
        while u < b {
            let f = self.pframe(u);
            bollards.push(trs(
                f.x + f.rx * (QUAY + 1.3),
                g0 + 0.18,
                f.z + f.rz * (QUAY + 1.3),
                yaw_of(f.fx, f.fz),
                1.0,
                1.0,
                1.0,
                0.0,
                0.0,
            ));
            // Cylindrical rubber fender hanging on the face, with its chains.
            let fe = [f.x + f.rx * (QUAY - 0.7), f.z + f.rz * (QUAY - 0.7)];
            geo.box_(
                fe[0],
                g0 - 3.6,
                fe[1],
                3.2,
                1.3,
                1.3,
                kernel::atan2(f.fz, f.fx),
                &copts(col(0x1a1a1a)),
            );
            geo.box_(
                fe[0],
                g0 - 2.3,
                fe[1],
                0.12,
                2.4,
                0.12,
                0.0,
                &copts(col(0x3a3a3a)),
            );
            u += 14.0;
        }
        self.badd(geo.build(), matte, false, 700.0);
        // Bollards: a waisted post with a mushroom head.
        let mut post = cylinder_geometry(0.3, 0.36, 0.62, 10.0, 1.0, false, 0.0, PI * 2.0);
        post.translate(0.0, 0.31, 0.0);
        let mut head = cylinder_geometry(0.46, 0.34, 0.2, 10.0, 1.0, false, 0.0, PI * 2.0);
        head.translate(0.0, 0.66, 0.0);
        let pn = post.to_non_indexed();
        let hn = head.to_non_indexed();
        let bg = tint(
            merge_geometries(&[&pn, &hn], false).expect("the bollard merges"),
            col(0x2c2d30),
        );
        let placed: Vec<BufferGeometry> = bollards
            .iter()
            .map(|m| {
                let mut g = bg.clone();
                g.apply_matrix4(m);
                g
            })
            .collect();
        let refs: Vec<&BufferGeometry> = placed.iter().collect();
        let merged = merge_geometries(&refs, false).expect("the bollards merge");
        let paint = self.paint();
        self.badd(merged, paint, true, 700.0);
        self.bollards = bollards
            .iter()
            .map(Vector3::set_from_matrix_position)
            .collect();
        // Crane rails under the crane legs, with yellow safety lines either
        // side, and a hatched strip along the quay edge.
        let yellow = col(0xe0b52b);
        let white = col(0xe8e8e2);
        let steel = col(0x55585c);
        let mut lines = Vec::new();
        for lat in [QUAY + 6.0, QUAY + 30.0] {
            lines.push(line(lat, 0.18, steel));
            lines.push(line(lat - 1.4, 0.15, yellow));
            lines.push(line(lat + 1.4, 0.15, yellow));
        }
        lines.push(line(QUAY + 2.6, 0.2, yellow));
        lines.push(line(APRON_IN - 1.5, 0.2, white));
        lines.push(dashed(QUAY + 18.0, 0.15, white, [4.0, 10.0]));
        self.add_markings(&lines, &[[a, b]], &move |_, _| g0, None);
    }

    /// Stacks of 40 ft boxes in the yard (left) and a depot behind the sheds
    /// (right). All containers — yard, ship, trucks and trains — are one
    /// InstancedMesh.
    fn add_container(&mut self, x: f64, y: f64, z: f64, yaw: f64, rng: Option<&mut Mulberry32>) {
        let rng = match rng {
            Some(r) => r,
            None => &mut self.rng,
        };
        let c = CONTAINER_COLS[(rng.next_f64() * CONTAINER_COLS.len() as f64).floor() as usize];
        let v = (rng.next_f64() * 4.0).floor();
        self.containers.push(Container { x, y, z, yaw, c, v });
    }

    fn build_yard(&mut self) {
        let path = self.path.clone();
        let g0 = self.ground_y;
        self.containers = Vec::new();
        let a = self.port_start() + 30.0;
        let b = path.u1 - 60.0;
        let groups = [-48.0, -73.0, -98.0, -123.0];
        let yellow = col(0xe0b52b);
        let white = col(0xe8e8e2);
        self.yard_blocks = Vec::new();
        let mut u = a;
        while u < b {
            let bays = js::min(10.0, ((b - u) / 12.6).floor() + 1.0);
            self.yard_blocks.push(Block {
                u0: u - 6.3,
                u1: u + (bays - 1.0) * 12.6 + 6.3,
            });
            let mut bay = 0.0;
            while bay < bays {
                let uc = u + bay * 12.6;
                let f = self.pframe(uc);
                let yaw = yaw_of(f.fx, f.fz);
                // Stacks step down toward the block ends and are uneven
                // within it.
                let end_bias = if js::min(bay, bays - 1.0 - bay) < 1.0 {
                    -1.0
                } else {
                    0.0
                };
                for g in groups {
                    let block_bias = (self.rng.next_f64() * 3.0).floor() - 1.0;
                    for row in 0..6 {
                        let lat = g - f64::from(row) * 2.6;
                        let r = self.rng.next_f64();
                        let mut tiers = if r < 0.08 {
                            0.0
                        } else if r < 0.2 {
                            1.0
                        } else if r < 0.38 {
                            2.0
                        } else if r < 0.68 {
                            3.0
                        } else if r < 0.9 {
                            4.0
                        } else {
                            5.0
                        };
                        let bias = if self.rng.next_f64() < 0.5 {
                            block_bias
                        } else {
                            0.0
                        };
                        tiers = clamp(tiers + end_bias + bias, 0.0, 5.0);
                        let mut k = 0.0;
                        while k < tiers {
                            // Boxes never sit perfectly square on each other.
                            let jl = (self.rng.next_f64() - 0.5) * 0.12;
                            let ja = (self.rng.next_f64() - 0.5) * 0.3;
                            let jy = (self.rng.next_f64() - 0.5) * 0.008;
                            self.add_container(
                                f.x + f.rx * (lat + jl) + f.fx * ja,
                                g0 + k * CONT[1],
                                f.z + f.rz * (lat + jl) + f.fz * ja,
                                yaw + jy,
                                None,
                            );
                            k += 1.0;
                        }
                    }
                    // Painted slot ends between bays.
                    if bay > 0.0 {
                        self.mark(uc - 6.3, g - 14.4, g + 1.4, -0.08, 0.08, white);
                    }
                }
                bay += 1.0;
            }
            // Block outlines and aisle centre lines.
            for g in groups {
                let lines = [line(g + 1.5, 0.16, yellow), line(g - 14.5, 0.16, yellow)];
                self.add_markings(
                    &lines,
                    &[[u - 6.3, u + (bays - 1.0) * 12.6 + 6.3]],
                    &move |_, _| g0 + 0.05,
                    None,
                );
            }
            u += 10.0 * 12.6 + 22.0;
        }
        let aisle: Vec<Line> = [-40.5, -67.0, -92.0, -117.0, -141.0]
            .iter()
            .map(|&lat| dashed(lat, 0.14, white, [3.0, 6.0]))
            .collect();
        self.add_markings(
            &aisle,
            &[[a - 20.0, b + 20.0]],
            &move |_, _| g0 + 0.05,
            None,
        );
        // Depot on the right, behind the sheds.
        let mut u = self.port_start() + 200.0;
        while u < b {
            for bay in 0..8 {
                let uc = u + f64::from(bay) * 12.6;
                if uc > b {
                    break;
                }
                let f = self.pframe(uc);
                let yaw = yaw_of(f.fx, f.fz);
                for row in 0..8 {
                    let lat = 132.0 + f64::from(row) * 2.6;
                    let tiers = (self.rng.next_f64() * 4.6).floor();
                    let mut k = 0.0;
                    while k < tiers {
                        self.add_container(
                            f.x + f.rx * lat,
                            g0 + k * CONT[1],
                            f.z + f.rz * lat,
                            yaw,
                            None,
                        );
                        k += 1.0;
                    }
                }
            }
            u += 8.0 * 12.6 + 30.0;
        }
        // Straddle carriers parked in the aisles, and high-mast lights.
        let mut geo = GeoBuilder::new(true, false);
        let yel = col(0xe8b020);
        let dk = col(0x2b2d31);
        let glass = col(0x9ab8c8);
        let mut masts: Vec<Matrix4> = Vec::new();
        let mut crowns: Vec<Matrix4> = Vec::new();
        self.straddle_u = Vec::new();
        let mut i = 0usize;
        let mut u = a + 60.0;
        while u < b {
            let f = self.pframe(u);
            let along = [f.fx, f.fz];
            let ac = [f.rx, f.rz];
            let lat = [-67.0, -92.0, -117.0][i % 3]; // aisle centres between stack groups
            self.straddle_u.push(u);
            let c = [f.x + ac[0] * lat, f.z + ac[1] * lat];
            let ya = kernel::atan2(along[1], along[0]);
            for (p, q) in [(-4.0, -2.6), (4.0, -2.6), (-4.0, 2.6), (4.0, 2.6)] {
                let x = c[0] + along[0] * p + ac[0] * q;
                let z = c[1] + along[1] * p + ac[1] * q;
                geo.box_(x, g0 + 1.1, z, 0.7, 11.9, 0.7, 0.0, &copts(yel));
                geo.box_(x, g0, z, 1.3, 1.1, 0.7, ya, &copts(dk)); // wheel
            }
            for q in [-2.6, 2.6] {
                let x = c[0] + ac[0] * q;
                let z = c[1] + ac[1] * q;
                geo.box_(x, g0 + 12.0, z, 9.0, 1.2, 0.8, ya, &copts(yel));
                geo.box_(x, g0 + 1.4, z, 8.6, 0.5, 0.5, ya, &copts(yel));
            }
            let cx = c[0] + ac[0] * 2.6 + along[0] * 4.3;
            let cz = c[1] + ac[1] * 2.6 + along[1] * 4.3;
            geo.box_(cx, g0 + 9.5, cz, 1.6, 2.2, 1.6, 0.0, &copts(dk));
            geo.box_(cx, g0 + 10.2, cz, 1.66, 0.8, 1.66, 0.0, &unroofed(glass));
            geo.box_(c[0], g0 + 8.2, c[1], 7.2, 0.35, 2.6, ya, &copts(dk)); // spreader
            if i.is_multiple_of(2) {
                // High mast further along the same aisle.
                let f = self.pframe(u + 47.0);
                let mx = f.x + f.rx * lat;
                let mz = f.z + f.rz * lat;
                masts.push(trs(mx, g0, mz, 0.0, 0.5, 34.0, 0.5, 0.0, 0.0));
                crowns.push(trs(
                    mx,
                    g0 + 34.0,
                    mz,
                    yaw_of(f.fx, f.fz),
                    1.0,
                    1.0,
                    1.0,
                    0.0,
                    0.0,
                ));
            }
            u += 95.0;
            i += 1;
        }
        let paint = self.paint();
        self.badd(geo.build(), paint, true, 700.0);
        let pole = self.pole_mat();
        let mut mast_geo = cylinder_geometry(0.6, 1.0, 1.0, 8.0, 1.0, false, 0.0, PI * 2.0);
        mast_geo.translate(0.0, 0.5, 0.0);
        let mg = self.graph.add_geometry(mast_geo);
        let n = instanced(self.graph, mg, pole, &masts, true, false);
        self.add(n);
        // Crown: a ring frame carrying six floodlights, lamp faces tilted
        // down.
        let mut cg = GeoBuilder::new(false, false);
        cg.box_(0.0, -0.2, 0.0, 3.6, 0.3, 3.6, 0.0, &PrismOpts::default());
        for k in 0..6 {
            let an = (f64::from(k) / 6.0) * PI * 2.0;
            cg.box_(
                kernel::cos(an) * 1.7,
                -0.9,
                kernel::sin(an) * 1.7,
                1.0,
                0.7,
                0.8,
                an,
                &PrismOpts::default(),
            );
        }
        let crown_mat = self.graph.add_material(
            Material::standard()
                .set("color", 0x9aa0a6)
                .set("emissive", 0xfff0d0)
                .set("emissiveIntensity", 0.1)
                .set("metalness", 0.5)
                .set("roughness", 0.4),
        );
        self.graph
            .add_night(Some(crown_mat), "emissiveIntensity", 0.1, 2.5);
        let cgg = self.graph.add_geometry(cg.build());
        let n = instanced(self.graph, cgg, crown_mat, &crowns, false, false);
        self.add(n);
    }

    fn flush_containers(&mut self) {
        let list = std::mem::take(&mut self.containers);
        if list.is_empty() {
            return;
        }
        let geo = self.graph.add_geometry(textures::container_geometry());
        let m = textures::container_material(self.graph, self.textures);
        let mat = self.graph.add_material(m);
        let mats: Vec<Matrix4> = list
            .iter()
            .map(|c| {
                trs(
                    c.x,
                    c.y,
                    c.z,
                    c.yaw,
                    CONT[0],
                    CONT[1] - 0.04,
                    CONT[2],
                    0.0,
                    0.0,
                )
            })
            .collect();
        let im = instanced(self.graph, geo, mat, &mats, true, true);
        let mut rng = Mulberry32::new(515);
        {
            let inst = self
                .graph
                .get_mut(im)
                .instances
                .as_mut()
                .expect("instanced");
            // A little fade and dirt per box so a row of one colour isn't
            // uniform.
            for (i, k) in list.iter().enumerate() {
                let mut c = Color::hex(k.c);
                c.multiply_scalar(0.85 + rng.next_f64() * 0.22);
                inst.set_color_at(i, c);
            }
        }
        let vars: Vec<f64> = list.iter().map(|k| k.v).collect();
        self.graph
            .geometry_mut(geo)
            .set_instanced_attribute("aVar", BufferAttribute::from_f64(&vars, 1));
        self.add(im);
    }

    /// Rubber-tyred gantry cranes straddling the yard blocks: six rows and
    /// a truck lane under each.
    fn build_rtgs(&mut self) {
        let g0 = self.ground_y;
        let mut geo = GeoBuilder::new(true, false);
        let white = col(0xecebe6);
        let yel = col(0xe8b020);
        let dk = col(0x2b2d31);
        let blue = col(0x1d4f7a);
        let glass = col(0x9ab8c8);
        let groups = [-48.0, -73.0, -98.0, -123.0];
        let mut lamps: Vec<P3> = Vec::new();
        let mut k = 0i64;
        for blk in self.yard_blocks.clone() {
            for g in groups {
                k += 1;
                if k % 3 == 0 {
                    continue;
                }
                let kf = k as f64;
                let u = lerp(blk.u0 + 12.0, blk.u1 - 12.0, (kf * 0.618) % 1.0);
                if self.straddle_u.iter().any(|s| (s - u).abs() < 14.0) {
                    continue;
                }
                let f = self.pframe(u);
                let ya = kernel::atan2(f.fz, f.fx);
                let lat_a = g + 3.2;
                let lat_b = g - 20.2;
                let hh = 19.0;
                let p = |lat: f64, d: f64, y: f64| -> P3 {
                    [f.x + f.rx * lat + f.fx * d, y, f.z + f.rz * lat + f.fz * d]
                };
                for lat in [lat_a, lat_b] {
                    for d in [-3.6, 3.6] {
                        beam(
                            &mut geo,
                            p(lat, d, g0 + 1.2),
                            p(lat, d * 0.8, g0 + hh),
                            0.8,
                            0.8,
                            Some(white),
                            true,
                        );
                        let w = p(lat, d, 0.0);
                        geo.box_(w[0], g0, w[2], 1.6, 1.3, 0.9, ya, &copts(dk)); // tyres
                    }
                    beam(
                        &mut geo,
                        p(lat, -4.4, g0 + 1.6),
                        p(lat, 4.4, g0 + 1.6),
                        0.9,
                        1.0,
                        Some(white),
                        true,
                    ); // sill beam
                    beam(
                        &mut geo,
                        p(lat, -3.2, g0 + hh),
                        p(lat, 3.2, g0 + hh),
                        0.8,
                        0.8,
                        Some(white),
                        true,
                    );
                    let eh = p(lat, -4.6, 0.0);
                    geo.box_(eh[0], g0 + 1.2, eh[2], 1.6, 2.2, 2.0, ya, &copts(yel)); // engine house
                }
                for d in [-2.2, 2.2] {
                    beam(
                        &mut geo,
                        p(lat_a + 1.0, d, g0 + hh + 0.8),
                        p(lat_b - 1.0, d, g0 + hh + 0.8),
                        1.1,
                        1.6,
                        Some(white),
                        true,
                    );
                }
                // Trolley with its cab, hoist ropes and spreader over the
                // stacks.
                let tl = lerp(lat_a, lat_b, 0.3 + ((kf * 0.37) % 0.5));
                let tr = p(tl, 0.0, 0.0);
                geo.box_(tr[0], g0 + hh + 1.6, tr[2], 5.2, 1.6, 4.4, ya, &copts(yel));
                let cab = p(tl, 2.6, 0.0);
                geo.box_(
                    cab[0],
                    g0 + hh - 1.6,
                    cab[2],
                    1.8,
                    2.0,
                    1.8,
                    ya,
                    &roofed(glass, white),
                );
                let sy = g0 + 5.0 * CONT[1] + 2.0 + ((kf * 3.0) % 5.0);
                for (d, o) in [(-1.5, -0.8), (1.5, -0.8), (-1.5, 0.8), (1.5, 0.8)] {
                    beam(
                        &mut geo,
                        p(tl + o, d, sy + 0.3),
                        p(tl + o, d, g0 + hh + 1.6),
                        0.07,
                        0.07,
                        Some(dk),
                        false,
                    );
                }
                let sp = p(tl, 0.0, 0.0);
                geo.box_(sp[0], sy, sp[2], 12.2, 0.4, 2.5, ya, &copts(yel));
                // Name band on the girder.
                for d in [-2.2, 2.2] {
                    beam(
                        &mut geo,
                        p(lat_a + 0.5, d * 1.26, g0 + hh + 0.9),
                        p(lat_a + 8.0, d * 1.26, g0 + hh + 0.9),
                        0.05,
                        1.0,
                        Some(blue),
                        false,
                    );
                }
                lamps.push(p(lat_a, 0.0, g0 + hh + 2.7));
                lamps.push(p(lat_b, 0.0, g0 + hh + 2.7));
            }
        }
        if geo.is_empty() {
            return;
        }
        let paint = self.paint();
        self.badd(geo.build(), paint, true, 1500.0);
        self.amber_beacons.extend(lamps);
    }

    fn crane_sites(&mut self) -> Vec<f64> {
        if self.cranes.is_none() {
            let mid = self.ship_mid_u();
            self.cranes = Some(
                [-110.0, -55.0, 0.0, 55.0, 110.0]
                    .iter()
                    .map(|d| mid + d)
                    .collect(),
            );
        }
        self.cranes.clone().expect("cranes")
    }

    fn ship_mid_u(&self) -> f64 {
        js::min(self.t.finish_s - 60.0, self.port_start() + 480.0)
    }

    /// Ship-to-shore cranes along the quay, booms reaching over the water.
    fn build_cranes(&mut self) {
        let g0 = self.ground_y + 0.15;
        let mut geo = GeoBuilder::new(true, false);
        let red = col(0xc23a2e);
        let white = col(0xe9e9e4);
        let dk = col(0x2b2d31);
        let blue = col(0x1d4f7a);
        let glass = col(0x8fb0c4);
        let grey = col(0x7a7e84);
        let sites = self.crane_sites();
        let mut beacons: Vec<P3> = Vec::new();
        let mut floods: Vec<P3> = Vec::new();
        for (ci, &u) in sites.iter().enumerate() {
            let f = self.pframe(u);
            let al = [f.fx, 0.0, f.fz];
            let out = [-f.rx, 0.0, -f.rz]; // "out" = toward the sea
            let o0 = [f.x + f.rx * (QUAY + 18.0), g0, f.z + f.rz * (QUAY + 18.0)];
            let at = |o: f64, a: f64, y: f64| -> P3 {
                [
                    o0[0] + out[0] * o + al[0] * a,
                    y,
                    o0[2] + out[2] * o + al[2] * a,
                ]
            };
            let yaw_out = kernel::atan2(out[2], out[0]);
            let yaw_al = kernel::atan2(al[2], al[0]);
            let w = Some(white);
            let r = Some(red);
            // Portal legs (water side o=+12, land side o=-12), 18 m apart
            // along the quay.
            for o in [-12.0, 12.0] {
                for a in [-9.0, 9.0] {
                    beam(
                        &mut geo,
                        at(o, a, g0 + 2.4),
                        at(o, a, g0 + 46.0),
                        1.6,
                        1.6,
                        w,
                        true,
                    );
                    // Bogies on the rail: an equaliser beam over two wheel
                    // trucks.
                    let p = at(o, a, 0.0);
                    geo.box_(p[0], g0 + 1.4, p[2], 5.2, 1.0, 1.4, yaw_al, &copts(red));
                    for d in [-1.6, 1.6] {
                        let q = at(o, a + d, 0.0);
                        geo.box_(q[0], g0, q[2], 1.8, 1.4, 1.1, yaw_al, &copts(dk));
                    }
                }
            }
            for a in [-9.0, 9.0] {
                beam(
                    &mut geo,
                    at(-12.0, a, g0 + 45.0),
                    at(12.0, a, g0 + 45.0),
                    1.8,
                    2.2,
                    w,
                    true,
                );
            }
            for o in [-12.0, 12.0] {
                beam(
                    &mut geo,
                    at(o, -9.0, g0 + 2.4),
                    at(o, 9.0, g0 + 2.4),
                    1.4,
                    1.4,
                    w,
                    true,
                );
            }
            for o in [-12.0, 12.0] {
                beam(
                    &mut geo,
                    at(o, -9.0, g0 + 44.0),
                    at(o, 9.0, g0 + 44.0),
                    1.4,
                    1.4,
                    w,
                    true,
                );
            }
            // Portal ties at mid height and diagonal bracing on the ends.
            for a in [-9.0, 9.0] {
                beam(
                    &mut geo,
                    at(-12.0, a, g0 + 4.0),
                    at(12.0, a, g0 + 40.0),
                    0.8,
                    0.8,
                    w,
                    true,
                );
                beam(
                    &mut geo,
                    at(-12.0, a, g0 + 24.0),
                    at(12.0, a, g0 + 24.0),
                    0.9,
                    0.9,
                    w,
                    true,
                );
            }
            for o in [-12.0, 12.0] {
                beam(
                    &mut geo,
                    at(o, -9.0, g0 + 30.0),
                    at(o, 0.0, g0 + 44.0),
                    0.6,
                    0.6,
                    w,
                    true,
                );
                beam(
                    &mut geo,
                    at(o, 9.0, g0 + 30.0),
                    at(o, 0.0, g0 + 44.0),
                    0.6,
                    0.6,
                    w,
                    true,
                );
            }
            // Stair tower up the land-side leg.
            let mut y = g0 + 3.0;
            while y < g0 + 44.0 {
                let s0 = at(-13.4, -9.0, 0.0);
                let s1 = at(-13.4, -6.6, 0.0);
                beam(
                    &mut geo,
                    [s0[0], y, s0[2]],
                    [s1[0], y + 1.6, s1[2]],
                    0.9,
                    0.12,
                    Some(grey),
                    false,
                );
                beam(
                    &mut geo,
                    [s1[0], y + 1.6, s1[2]],
                    [s0[0], y + 3.2, s0[2]],
                    0.9,
                    0.12,
                    Some(grey),
                    false,
                );
                y += 3.2;
            }
            // A-frame over the land-side legs.
            for a in [-7.0, 7.0] {
                beam(
                    &mut geo,
                    at(-12.0, a, g0 + 46.0),
                    at(-6.0, a, g0 + 82.0),
                    1.2,
                    1.2,
                    r,
                    true,
                );
                beam(
                    &mut geo,
                    at(8.0, a, g0 + 46.0),
                    at(-6.0, a, g0 + 82.0),
                    1.2,
                    1.2,
                    r,
                    true,
                );
                beam(
                    &mut geo,
                    at(-9.0, a, g0 + 64.0),
                    at(1.0, a, g0 + 64.0),
                    0.7,
                    0.7,
                    r,
                    true,
                );
            }
            beam(
                &mut geo,
                at(-6.0, -7.0, g0 + 82.0),
                at(-6.0, 7.0, g0 + 82.0),
                1.2,
                1.2,
                r,
                true,
            );
            beam(
                &mut geo,
                at(-6.0, -7.0, g0 + 64.0),
                at(-6.0, 7.0, g0 + 64.0),
                0.7,
                0.7,
                r,
                true,
            );
            // Boom (two box girders) from back-reach to far over the water,
            // with a lacing of diagonals between the cross members and a
            // walkway.
            for a in [-3.2, 3.2] {
                beam(
                    &mut geo,
                    at(-34.0, a, g0 + 52.0),
                    at(66.0, a, g0 + 52.0),
                    1.4,
                    2.6,
                    r,
                    true,
                );
            }
            let mut o = -30.0;
            while o <= 62.0 {
                beam(
                    &mut geo,
                    at(o, -3.2, g0 + 51.2),
                    at(o, 3.2, g0 + 51.2),
                    0.5,
                    0.5,
                    r,
                    false,
                );
                if o < 62.0 {
                    beam(
                        &mut geo,
                        at(o, -3.2, g0 + 51.1),
                        at(o + 8.0, 3.2, g0 + 51.1),
                        0.35,
                        0.35,
                        r,
                        false,
                    );
                }
                o += 8.0;
            }
            for a in [-4.4, 4.4] {
                beam(
                    &mut geo,
                    at(-34.0, a, g0 + 51.0),
                    at(64.0, a, g0 + 51.0),
                    0.9,
                    0.1,
                    Some(grey),
                    false,
                );
                beam(
                    &mut geo,
                    at(-34.0, a * 1.1, g0 + 52.1),
                    at(64.0, a * 1.1, g0 + 52.1),
                    0.06,
                    0.06,
                    Some(grey),
                    false,
                );
            }
            // Forestays and backstays.
            for a in [-3.2, 3.2] {
                beam(
                    &mut geo,
                    at(-6.0, a * 1.8, g0 + 82.0),
                    at(64.0, a, g0 + 53.4),
                    0.35,
                    0.35,
                    Some(dk),
                    false,
                );
                beam(
                    &mut geo,
                    at(-6.0, a * 1.8, g0 + 82.0),
                    at(28.0, a, g0 + 53.4),
                    0.35,
                    0.35,
                    Some(dk),
                    false,
                );
                beam(
                    &mut geo,
                    at(-6.0, a * 1.8, g0 + 82.0),
                    at(-32.0, a, g0 + 53.4),
                    0.35,
                    0.35,
                    Some(dk),
                    false,
                );
            }
            // Machinery house with a louvred band, trolley, cab, and the
            // spreader hanging on its ropes.
            let mh = at(-26.0, 0.0, 0.0);
            geo.box_(
                mh[0],
                g0 + 53.4,
                mh[2],
                11.0,
                6.5,
                8.0,
                yaw_out,
                &roofed(white, blue),
            );
            geo.box_(
                mh[0],
                g0 + 57.2,
                mh[2],
                11.1,
                1.0,
                8.1,
                yaw_out,
                &unroofed(blue),
            );
            let to = 14.0 + ((u * 0.37) % 34.0);
            let tr = at(to, 0.0, 0.0);
            geo.box_(
                tr[0],
                g0 + 49.2,
                tr[2],
                5.0,
                3.4,
                6.5,
                yaw_out,
                &copts(blue),
            );
            geo.box_(
                tr[0],
                g0 + 46.2,
                tr[2],
                2.2,
                2.6,
                2.2,
                yaw_out,
                &copts(white),
            );
            let cw = at(to + 1.15, 0.0, 0.0);
            geo.box_(
                cw[0],
                g0 + 46.6,
                cw[2],
                0.1,
                1.8,
                2.0,
                yaw_out,
                &unroofed(glass),
            );
            let sy = g0 + 14.0 + ((ci as f64 * 11.0) % 22.0);
            for (d, a) in [(-1.2, -1.1), (1.2, -1.1), (-1.2, 1.1), (1.2, 1.1)] {
                beam(
                    &mut geo,
                    at(to + d, a, sy + 0.4),
                    at(to + d, a, g0 + 49.2),
                    0.08,
                    0.08,
                    Some(dk),
                    false,
                );
            }
            let sp = at(to, 0.0, 0.0);
            geo.box_(
                sp[0],
                sy,
                sp[2],
                2.6,
                0.5,
                12.2,
                yaw_out,
                &copts(col(0xe8b020)),
            );
            beacons.push(at(66.0, 0.0, g0 + 54.2));
            beacons.push(at(-6.0, 0.0, g0 + 83.4));
            let mut o = 2.0;
            while o <= 58.0 {
                floods.push(at(o, 0.0, g0 + 50.4));
                o += 14.0;
            }
        }
        let paint = self.paint();
        self.badd(geo.build(), paint, true, 2000.0);
        self.red_beacons = beacons;
        self.crane_floods = floods;
    }

    /// Lamps on the cranes and gantries: red aircraft beacons that blink,
    /// amber gantry beacons, and floodlights under the booms — fixed-size
    /// glow points so they read from a kilometre away.
    fn build_port_lights(&mut self) {
        let mut all: Vec<(P3, [f32; 3])> = Vec::new();
        for p in &self.red_beacons {
            all.push((*p, [2.2, 0.15, 0.1]));
        }
        for p in &self.amber_beacons {
            all.push((*p, [1.9, 1.0, 0.2]));
        }
        for p in &self.crane_floods {
            all.push((*p, [1.6, 1.45, 1.2]));
        }
        for p in &self.ship_lamps {
            all.push((*p, [1.7, 1.6, 1.4]));
        }
        if all.is_empty() {
            return;
        }
        let mut pos = vec![0.0f32; all.len() * 3];
        let mut base = vec![0.0f32; all.len() * 3];
        for (i, (p, c)) in all.iter().enumerate() {
            for k in 0..3 {
                pos[i * 3 + k] = p[k] as f32;
                base[i * 3 + k] = c[k];
            }
        }
        let colr = base.clone();
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f32(pos, 3));
        g.set_attribute("color", BufferAttribute::from_f32(colr, 3));
        let glow = self
            .graph
            .cached_texture(&self.textures.glow_texture(), Layer::Main, "");
        let m = self.graph.add_material(
            Material::points()
                .set("size", 7.0)
                .set("sizeAttenuation", false)
                .set("map", glow)
                .set("vertexColors", true)
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", ADDITIVE),
        );
        let gg = self.graph.add_geometry(g);
        let pts = self.graph.drawable(NodeType::Points, gg, m);
        self.graph.get_mut(pts).frustum_culled = false;
        self.add(pts);
        let n_r = self.red_beacons.len();
        let n_a = self.amber_beacons.len();
        let n = all.len();
        let mut time = 0.0;
        self.animators
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                time += u.dt;
                let blink = if (time % 1.6) < 0.5 { 1.0 } else { 0.12 };
                let lit = 0.25 + 0.75 * smoothstep(0.1, 0.6, u.night);
                let mut colr = vec![0.0f32; n * 3];
                for i in 0..n {
                    let k = if i < n_r {
                        blink
                    } else if i < n_r + n_a {
                        (if ((time + i as f64 * 0.37) % 1.2) < 0.6 {
                            1.0
                        } else {
                            0.15
                        }) * lit
                    } else {
                        lit * 0.9
                    };
                    for c in 0..3 {
                        colr[i * 3 + c] = (f64::from(base[i * 3 + c]) * k) as f32;
                    }
                }
                out.push(Edit {
                    target: Handle::Geometry(gg),
                    change: Change::Attribute {
                        name: "color",
                        offset: 0,
                        values: colr,
                    },
                });
            }));
    }

    /// A container ship moored along the quay: hull, deck stacks,
    /// accommodation block and funnel. Placed along the chord of the
    /// (slightly curved) quay.
    fn build_ship(&mut self) {
        let mid = self.ship_mid_u();
        let lship = 270.0;
        let bm = 38.0;
        let qa = self.pframe(mid - lship / 2.0);
        let qb = self.pframe(mid + lship / 2.0);
        let a = [qa.x + qa.rx * QUAY, qa.z + qa.rz * QUAY];
        let bq = [qb.x + qb.rx * QUAY, qb.z + qb.rz * QUAY];
        let mut dx = bq[0] - a[0];
        let mut dz = bq[1] - a[1];
        let len = kernel::hypot(dx, dz);
        dx /= len;
        dz /= len;
        // Seaward normal: the side of the chord away from the road.
        let mut nx = -dz;
        let mut nz = dx;
        let f = self.pframe(mid);
        if nx * -f.rx + nz * -f.rz < 0.0 {
            nx = -nx;
            nz = -nz;
        }
        // How far the quay bulges past the chord.
        let mut bulge = 0.0;
        let mut u = mid - lship / 2.0;
        while u <= mid + lship / 2.0 {
            let f = self.pframe(u);
            let qx = f.x + f.rx * QUAY;
            let qz = f.z + f.rz * QUAY;
            bulge = js::max(bulge, (qx - a[0]) * nx + (qz - a[1]) * nz);
            u += 10.0;
        }
        let cx = (a[0] + bq[0]) / 2.0 + nx * (bulge + 3.0 + bm / 2.0);
        let cz = (a[1] + bq[1]) / 2.0 + nz * (bulge + 3.0 + bm / 2.0);
        // x along ship, z across (toward sea).
        let p = |x: f64, y: f64, z: f64| -> P3 { [cx + dx * x + nx * z, y, cz + dz * x + nz * z] };
        let hull_c = col(0x1f3550);
        let boot = col(0x9c2b24);
        let deck_c = col(0x6b6e70);
        let white_c = col(0xeeeeea);
        let dk = col(0x23262b);
        let mut geo = GeoBuilder::new(true, false);
        let half = |x: f64| -> f64 {
            // Beam along the hull: full amidships, fine bow, slightly
            // narrower stern.
            let t = (x + lship / 2.0) / lship;
            if t > 0.82 {
                return bm / 2.0
                    * js::max(0.0004, 1.0 - kernel::pow((t - 0.82) / 0.18, 2.0)).sqrt();
            }
            if t < 0.06 {
                return bm / 2.0 * (0.86 + 0.14 * t / 0.06);
            }
            bm / 2.0
        };
        let nn = 40;
        let xs: Vec<f64> = (0..=nn)
            .map(|i| -lship / 2.0 + (lship * f64::from(i)) / f64::from(nn))
            .collect();
        let bands = [(-11.0, -1.8, boot), (-1.8, 0.9, boot), (0.9, 13.0, hull_c)];
        for i in 0..nn as usize {
            let (x0, x1) = (xs[i], xs[i + 1]);
            let (h0, h1) = (half(x0), half(x1));
            for side in [-1.0, 1.0] {
                for (y0, y1, c) in bands {
                    quad_out(
                        &mut geo,
                        p(x0, y0, side * h0),
                        p(x1, y0, side * h1),
                        p(x1, y1, side * h1),
                        p(x0, y1, side * h0),
                        [nx * side, 0.0, nz * side],
                        Some(c),
                        None,
                    );
                }
            }
            // Deck.
            quad_out(
                &mut geo,
                p(x0, 13.0, -h0),
                p(x1, 13.0, -h1),
                p(x1, 13.0, h1),
                p(x0, 13.0, h0),
                [0.0, 1.0, 0.0],
                Some(deck_c),
                None,
            );
        }
        // Transom.
        let hs = half(-lship / 2.0);
        quad_out(
            &mut geo,
            p(-lship / 2.0, -11.0, -hs),
            p(-lship / 2.0, -11.0, hs),
            p(-lship / 2.0, 13.0, hs),
            p(-lship / 2.0, 13.0, -hs),
            [-dx, 0.0, -dz],
            Some(hull_c),
            None,
        );
        // Accommodation block near the stern, bridge wings, funnel.
        let yaw_ship = kernel::atan2(dz, dx);
        let sb = p(-lship / 2.0 + 30.0, 13.0, 0.0);
        geo.box_(
            sb[0],
            13.0,
            sb[2],
            16.0,
            24.0,
            bm * 0.82,
            yaw_ship,
            &roofed(white_c, col(0xb0b2b4)),
        );
        geo.box_(
            sb[0],
            34.0,
            sb[2],
            8.0,
            3.0,
            bm + 3.0,
            yaw_ship,
            &copts(white_c),
        );
        for k in 0..7 {
            // Window bands on the forward face.
            let q = p(-lship / 2.0 + 38.05, 0.0, 0.0);
            geo.box_(
                q[0],
                15.0 + f64::from(k) * 3.2,
                q[2],
                0.1,
                1.2,
                bm * 0.78,
                yaw_ship,
                &unroofed(dk),
            );
        }
        let fnl = p(-lship / 2.0 + 14.0, 0.0, 0.0);
        geo.box_(
            fnl[0],
            13.0,
            fnl[2],
            8.0,
            30.0,
            7.0,
            yaw_ship,
            &copts(col(0xb3342b)),
        );
        geo.box_(fnl[0], 43.0, fnl[2], 8.2, 2.5, 7.2, yaw_ship, &copts(dk));
        // Sheer stripe under the deck edge, draft marks at bow and stern.
        for i in 0..nn as usize {
            let (x0, x1) = (xs[i], xs[i + 1]);
            let (h0, h1) = (half(x0), half(x1));
            for side in [-1.0, 1.0] {
                let o = side * 0.04;
                quad_out(
                    &mut geo,
                    p(x0, 11.6, side * h0 + o),
                    p(x1, 11.6, side * h1 + o),
                    p(x1, 12.1, side * h1 + o),
                    p(x0, 12.1, side * h0 + o),
                    [nx * side, 0.0, nz * side],
                    Some(white_c),
                    None,
                );
                // Deck railing: posts every segment and a top rail.
                let pp = p(x0, 13.0, side * (h0 - 0.3));
                geo.box_(pp[0], 13.0, pp[2], 0.08, 1.1, 0.08, 0.0, &unroofed(white_c));
                beam(
                    &mut geo,
                    p(x0, 14.1, side * (h0 - 0.3)),
                    p(x1, 14.1, side * (h1 - 0.3)),
                    0.07,
                    0.07,
                    Some(white_c),
                    false,
                );
            }
        }
        for xm in [lship / 2.0 - 14.0, -lship / 2.0 + 4.0] {
            for k in 0..8 {
                for side in [-1.0, 1.0] {
                    let q = p(xm, 0.0, side * (half(xm) + 0.06));
                    geo.box_(
                        q[0],
                        -1.5 + f64::from(k) * 1.1,
                        q[2],
                        0.6,
                        0.12,
                        0.12,
                        yaw_ship,
                        &unroofed(white_c),
                    );
                }
            }
        }
        // Forecastle with a breakwater, and a foremast carrying a lamp.
        let fc = p(lship / 2.0 - 16.0, 0.0, 0.0);
        geo.box_(
            fc[0],
            13.0,
            fc[2],
            22.0,
            2.6,
            bm * 0.7,
            yaw_ship,
            &roofed(hull_c, deck_c),
        );
        let bw = p(lship / 2.0 - 28.0, 0.0, 0.0);
        geo.box_(
            bw[0],
            13.0,
            bw[2],
            0.6,
            4.2,
            bm * 0.8,
            yaw_ship,
            &copts(white_c),
        );
        let fm = p(lship / 2.0 - 12.0, 0.0, 0.0);
        geo.box_(
            fm[0],
            15.6,
            fm[2],
            0.6,
            11.0,
            0.6,
            yaw_ship,
            &copts(col(0xd8a020)),
        );
        // Wheelhouse windows wrapping the top of the accommodation, a radar
        // mast above, and orange lifeboats in davits on both sides.
        let acc = |x: f64, y: f64, z: f64| p(-lship / 2.0 + 30.0 + x, y, z);
        for side in [-1.0, 1.0] {
            let wv = acc(0.0, 0.0, side * (bm * 0.41 + 0.05));
            geo.box_(wv[0], 31.6, wv[2], 15.0, 1.6, 0.1, yaw_ship, &unroofed(dk));
            let lb = acc(-3.0, 0.0, side * (bm * 0.41 + 2.4));
            geo.box_(
                lb[0],
                21.5,
                lb[2],
                8.0,
                2.2,
                2.6,
                yaw_ship,
                &roofed(col(0xf07a1a), col(0xf29a40)),
            );
            for d in [-3.0, 3.0] {
                let dv = acc(-3.0 + d, 0.0, side * (bm * 0.41 + 0.6));
                let dv2 = acc(-3.0 + d, 0.0, side * (bm * 0.41 + 2.6));
                beam(
                    &mut geo,
                    [dv[0], 21.0, dv[2]],
                    [dv[0], 25.5, dv[2]],
                    0.3,
                    0.3,
                    Some(white_c),
                    true,
                );
                beam(
                    &mut geo,
                    [dv[0], 25.5, dv[2]],
                    [dv2[0], 24.2, dv2[2]],
                    0.25,
                    0.25,
                    Some(white_c),
                    true,
                );
            }
        }
        let fw = acc(8.05, 0.0, 0.0);
        geo.box_(
            fw[0],
            31.6,
            fw[2],
            0.1,
            1.6,
            bm * 0.8,
            yaw_ship,
            &unroofed(dk),
        );
        let rm = acc(0.0, 0.0, 0.0);
        beam(
            &mut geo,
            [rm[0], 37.0, rm[2]],
            [rm[0], 45.0, rm[2]],
            0.5,
            0.5,
            Some(white_c),
            true,
        );
        let rb = acc(0.0, 0.0, 0.0);
        geo.box_(rb[0], 42.0, rb[2], 0.5, 0.3, 7.0, yaw_ship, &copts(white_c));
        geo.box_(
            rb[0],
            44.2,
            rb[2],
            4.4,
            0.25,
            0.4,
            yaw_ship + 0.6,
            &copts(dk),
        );
        // Funnel: company band.
        geo.box_(
            fnl[0],
            33.0,
            fnl[2],
            8.15,
            4.0,
            7.15,
            yaw_ship,
            &unroofed(white_c),
        );
        self.ship_lamps = vec![
            acc(0.0, 45.4, 0.0),
            p(lship / 2.0 - 12.0, 26.8, 0.0),
            p(-lship / 2.0 + 2.0, 15.0, 0.0),
        ];
        // Deck containers: bays forward of the accommodation, each on a
        // hatch cover, with lashing bridges between bays.
        let mut rng = Mulberry32::new(99);
        let yaw_c = -yaw_ship;
        let lash = col(0x9a9ea3);
        let mut x = -lship / 2.0 + 46.0;
        while x < lship / 2.0 - 42.0 {
            let rows = ((half(x + 6.0) * 2.0 - 2.0) / 2.5).floor();
            let tiers = 3.0 + (rng.next_f64() * 4.0).floor();
            let hc = p(x + 6.1, 0.0, 0.0);
            geo.box_(
                hc[0],
                13.0,
                hc[2],
                12.8,
                0.9,
                rows * 2.5 + 0.6,
                yaw_ship,
                &copts(col(0x55616b)),
            );
            if x > -lship / 2.0 + 46.0 {
                let lz = (rows * 2.5) / 2.0 + 0.3;
                let mut q = -1.0;
                while q <= 1.0 {
                    let lp = p(x - 0.4, 0.0, q * lz);
                    geo.box_(lp[0], 13.9, lp[2], 0.4, 5.4, 0.4, yaw_ship, &copts(lash));
                    q += 2.0 / 3.0;
                }
                beam(
                    &mut geo,
                    p(x - 0.4, 19.2, -lz),
                    p(x - 0.4, 19.2, lz),
                    0.5,
                    0.35,
                    Some(lash),
                    true,
                );
                beam(
                    &mut geo,
                    p(x - 0.4, 16.6, -lz),
                    p(x - 0.4, 16.6, lz),
                    0.3,
                    0.25,
                    Some(lash),
                    false,
                );
            }
            let mut r = 0.0;
            while r < rows {
                let z = -((rows - 1.0) * 2.5) / 2.0 + r * 2.5;
                let edge = if z.abs() > bm * 0.4 { 1.0 } else { 0.0 };
                let gap = if rng.next_f64() < 0.12 { 1.0 } else { 0.0 };
                let tt = js::max(1.0, tiers - edge - gap);
                let mut k = 0.0;
                while k < tt {
                    let q = p(x + 6.1, 13.9 + k * CONT[1], z);
                    self.add_container(q[0], q[1], q[2], yaw_c, Some(&mut rng));
                    k += 1.0;
                }
                r += 1.0;
            }
            x += 13.2;
        }
        let paint = self.paint();
        self.badd(geo.build(), paint, true, 3000.0);
        // Name on the bow.
        let (texture, aspect) = sign_of(
            self.graph,
            self.textures,
            &["MERIDIAN STAR"],
            &SignOpts {
                bg: "#1f3550",
                fg: "#ffffff",
                border: None,
                w: 512,
                h: 96,
                font: "bold 64px Arial, sans-serif",
                arrow: None,
            },
        );
        let nm = self.graph.add_material(
            Material::standard()
                .set("map", texture)
                .set("roughness", 0.6),
        );
        for side in [-1.0, 1.0] {
            let h = 3.2;
            let w = h * aspect;
            let q = p(
                lship / 2.0 - 58.0,
                0.0,
                side * (half(lship / 2.0 - 58.0) + 0.05),
            );
            let pg = self.graph.add_geometry(plane_geometry(w, h, 1.0, 1.0));
            let m = self.graph.mesh(pg, nm);
            {
                let o = self.graph.get_mut(m);
                o.position = Vector3::new(q[0], 9.5, q[2]);
                o.set_rotation(&Euler::new(0.0, kernel::atan2(nx * side, nz * side), 0.0));
                if side < 0.0 {
                    o.scale.x = -1.0;
                }
            }
            self.graph
                .material_mut(nm)
                .set_value("side", crate::material::Param::Num(DOUBLE_SIDE));
            self.add(m);
        }
        self.ship = Some(Ship {
            cx,
            cz,
            dx,
            dz,
            nx,
            nz,
            l: lship,
        });
        // Name and port of registry across the stern.
        {
            let (texture, aspect) = sign_of(
                self.graph,
                self.textures,
                &["MERIDIAN STAR", "PORT MERIDIAN"],
                &SignOpts {
                    bg: "#1f3550",
                    fg: "#ffffff",
                    border: None,
                    w: 512,
                    h: 160,
                    font: "bold 56px Arial, sans-serif",
                    arrow: None,
                },
            );
            let m = self.graph.add_material(
                Material::standard()
                    .set("map", texture)
                    .set("roughness", 0.6),
            );
            let h = 4.2;
            let w = h * aspect;
            let q = p(-lship / 2.0 - 0.08, 0.0, 0.0);
            let pg = self.graph.add_geometry(plane_geometry(w, h, 1.0, 1.0));
            let sm = self.graph.mesh(pg, m);
            {
                let o = self.graph.get_mut(sm);
                o.position = Vector3::new(q[0], 8.5, q[2]);
                o.set_rotation(&Euler::new(0.0, kernel::atan2(-dx, -dz), 0.0));
            }
            self.add(sm);
        }
        // Mooring lines: head, breast and stern lines down to the quay
        // bollards.
        if !self.bollards.is_empty() {
            let mut lines = GeoBuilder::new(true, false);
            let rope = col(0xd8cfb8);
            for xs0 in [
                lship / 2.0 - 8.0,
                lship / 2.0 - 30.0,
                -lship / 2.0 + 6.0,
                -lship / 2.0 + 26.0,
            ] {
                let aa = p(xs0, 14.0, -half(xs0) + 0.5);
                let mut best: Option<Vector3> = None;
                let mut bd = f64::INFINITY;
                for bp in &self.bollards {
                    let d = kernel::hypot(bp.x - aa[0], bp.z - aa[2]);
                    // Head and stern lines run out further.
                    let want = if xs0.abs() > lship / 2.0 - 12.0 {
                        40.0
                    } else {
                        18.0
                    };
                    let sc = (d - want).abs();
                    if sc < bd {
                        bd = sc;
                        best = Some(*bp);
                    }
                }
                let Some(best) = best else { continue };
                // Sagging rope in a few straight pieces.
                let bp = [best.x, best.y + 0.6, best.z];
                let mut prev = aa;
                for k in 1..=4 {
                    let t2 = f64::from(k) / 4.0;
                    let q = [
                        lerp(aa[0], bp[0], t2),
                        lerp(aa[1], bp[1], t2) - kernel::sin(t2 * PI) * 1.2,
                        lerp(aa[2], bp[2], t2),
                    ];
                    beam(&mut lines, prev, q, 0.1, 0.1, Some(rope), false);
                    prev = q;
                }
            }
            self.badd(lines.build(), paint, true, 3000.0);
        }
    }

    // ── Rail spur on the right: two tracks on ballast, with a container
    // train standing on one of them ─────────────────────────────────────

    fn build_rail(&mut self) {
        let path = self.path.clone();
        let g0 = self.ground_y + 0.04;
        let mut rng = Mulberry32::new(2323);
        let a = self.port_start() - 10.0;
        let b = path.u1 - 40.0;
        let gravel = {
            let cached =
                self.graph
                    .cached_texture(&self.textures.gravel_texture(), Layer::Main, "");
            let t = self.graph.clone_texture(cached);
            crate::mountain::canvas::set_wrap(
                self.graph,
                t,
                three::REPEAT_WRAPPING,
                three::REPEAT_WRAPPING,
            );
            t
        };
        let ballast = self.mat("ballast", |bb| {
            bb.graph.add_material(
                Material::standard()
                    .set("map", gravel)
                    .set("color", 0x8f877e)
                    .set("roughness", 1.0),
            )
        });
        let mut ties = GeoBuilder::new(true, false);
        let tie_c = col(0x6a655e);
        let paint = self.paint();
        for c in RAIL_LATS {
            for r in chunked(&[[a, b]], 300.0) {
                let g = self.sweep(
                    &[r],
                    &[
                        prof(c - 2.0, move |_, _| g0),
                        prof(c - 1.5, move |_, _| g0 + 0.28),
                        prof(c + 1.5, move |_, _| g0 + 0.28),
                        prof(c + 2.0, move |_, _| g0),
                    ],
                    &SweepOpts {
                        step: 6.0,
                        u_s: 3.0,
                        v_s: 3.0,
                        ..SweepOpts::default()
                    },
                );
                self.batch.add(
                    self.graph,
                    g,
                    ballast,
                    AddOpts {
                        receive: true,
                        chunk: 1500.0,
                        cast: false,
                    },
                );
                for rl in [c - 0.75, c + 0.75] {
                    let g = self.sweep(
                        &[r],
                        &[
                            prof(rl - 0.04, move |_, _| g0 + 0.36),
                            prof(rl - 0.04, move |_, _| g0 + 0.5),
                            prof(rl + 0.04, move |_, _| g0 + 0.5),
                            prof(rl + 0.04, move |_, _| g0 + 0.36),
                        ],
                        &SweepOpts {
                            step: 6.0,
                            color: Some(col(0x6d6a66)),
                            ..SweepOpts::default()
                        },
                    );
                    self.badd(g, paint, true, 700.0);
                }
            }
            let mut u = a + 0.4;
            while u < b {
                let f = self.pframe(u);
                ties.box_(
                    f.x + f.rx * c,
                    g0 + 0.28,
                    f.z + f.rz * c,
                    2.6,
                    0.16,
                    0.26,
                    kernel::atan2(f.rz, f.rx),
                    &copts(tie_c),
                );
                u += 0.72;
            }
        }
        let matte = self.matte();
        self.badd(ties.build(), matte, false, 700.0);
        // A string of well wagons carrying boxes, and a locomotive at its
        // head.
        let mut geo = GeoBuilder::new(true, false);
        let wag = col(0x5a3a2c);
        let dk = col(0x26282b);
        let loco = col(0x1d4f7a);
        let yel = col(0xe0b52b);
        let c = RAIL_LATS[1];
        let deck_y = g0 + 0.5 + 1.1;
        let u0 = a + 120.0;
        let mut u = u0;
        for _ in 0..14 {
            let f = self.pframe(u + 7.6);
            let yaw = yaw_of(f.fx, f.fz);
            let ya = kernel::atan2(f.fz, f.fx);
            let x = f.x + f.rx * c;
            let z = f.z + f.rz * c;
            geo.box_(x, deck_y - 0.5, z, 15.0, 0.5, 2.7, ya, &copts(wag));
            for d in [-5.8, 5.8] {
                let bx = x + f.fx * d;
                let bz = z + f.fz * d;
                geo.box_(bx, g0 + 0.5, bz, 2.6, 0.7, 2.3, ya, &copts(dk));
            }
            if rng.next_f64() < 0.85 {
                self.add_container(x, deck_y, z, yaw, Some(&mut rng));
                if rng.next_f64() < 0.5 {
                    self.add_container(x, deck_y + CONT[1], z, yaw, Some(&mut rng));
                }
            }
            u += 15.6;
        }
        let f = self.pframe(u + 10.0);
        let ya = kernel::atan2(f.fz, f.fx);
        let x = f.x + f.rx * c;
        let z = f.z + f.rz * c;
        geo.box_(
            x,
            g0 + 1.2,
            z,
            20.0,
            3.4,
            3.0,
            ya,
            &roofed(loco, col(0x8a9096)),
        );
        let cab = [x + f.fx * 8.2, z + f.fz * 8.2];
        geo.box_(
            cab[0],
            g0 + 4.6,
            cab[1],
            3.6,
            0.9,
            3.02,
            ya,
            &unroofed(col(0x2a3440)),
        );
        geo.box_(x, g0 + 1.2, z, 20.1, 0.3, 3.02, ya, &unroofed(yel));
        for d in [-6.5, 6.5] {
            geo.box_(
                x + f.fx * d,
                g0 + 0.5,
                z + f.fz * d,
                3.4,
                0.7,
                2.4,
                ya,
                &copts(dk),
            );
        }
        self.badd(geo.build(), paint, true, 700.0);
    }

    /// Container trucks: under the quay cranes waiting to be loaded, in the
    /// yard lanes, and backed up to the shed doors.
    fn truck(
        &mut self,
        geo: &mut GeoBuilder,
        x: f64,
        z: f64,
        fx: f64,
        fz: f64,
        rng: &mut Mulberry32,
        loaded: bool,
    ) {
        let g0 = self.ground_y + 0.04;
        let ya = kernel::atan2(fz, fx);
        let yaw = yaw_of(fx, fz);
        const CABS: [u32; 6] = [0xb3342b, 0xe9e9e4, 0x1d4f8a, 0x2f7d3a, 0xe0b52b, 0x2b2d31];
        let cc = col(CABS[(rng.next_f64() * CABS.len() as f64).floor() as usize]);
        let dk = col(0x1c1d20);
        let steel = col(0x55585c);
        let glass = col(0x3a4a58);
        let at = |d: f64| -> P2 { [x + fx * d, z + fz * d] };
        // Chassis (trailer) and wheels.
        let p = at(-1.2);
        geo.box_(p[0], g0 + 1.05, p[1], 12.4, 0.3, 2.4, ya, &copts(steel));
        for d in [-6.0, -4.8, 3.4] {
            let p = at(d);
            geo.box_(p[0], g0, p[1], 1.0, 1.0, 2.5, ya, &copts(dk));
        }
        // Tractor: cab over the front axle, sleeper, fuel tank.
        let p = at(6.9);
        geo.box_(p[0], g0 + 1.0, p[1], 2.3, 2.6, 2.5, ya, &copts(cc));
        let p = at(7.95);
        geo.box_(p[0], g0 + 2.2, p[1], 0.1, 1.1, 2.2, ya, &unroofed(glass));
        let p = at(5.5);
        geo.box_(p[0], g0 + 0.5, p[1], 3.0, 0.6, 2.2, ya, &copts(dk));
        for d in [5.2, 7.2] {
            let p = at(d);
            geo.box_(p[0], g0, p[1], 1.0, 1.0, 2.5, ya, &copts(dk));
        }
        if loaded {
            let p = at(-1.2);
            self.add_container(p[0], g0 + 1.35, p[1], yaw, Some(rng));
        }
    }

    fn build_trucks(&mut self) {
        let mut rng = Mulberry32::new(777);
        let mut geo = GeoBuilder::new(true, false);
        // Under the cranes, facing along the quay.
        for (i, u) in self.crane_sites().into_iter().enumerate() {
            if i % 2 == 1 {
                continue;
            }
            let f = self.pframe(u + 2.0);
            let lat = QUAY + 18.0 + if i % 4 != 0 { 3.5 } else { -3.5 };
            let loaded = rng.next_f64() < 0.5;
            self.truck(
                &mut geo,
                f.x + f.rx * lat,
                f.z + f.rz * lat,
                f.fx,
                f.fz,
                &mut rng,
                loaded,
            );
        }
        // In the yard truck lanes.
        for blk in self.yard_blocks.clone() {
            for g in [-48.0, -98.0] {
                if rng.next_f64() < 0.4 {
                    continue;
                }
                let u = lerp(blk.u0 + 15.0, blk.u1 - 15.0, rng.next_f64());
                let f = self.pframe(u);
                let lat = g - 17.2;
                let loaded = rng.next_f64() < 0.7;
                self.truck(
                    &mut geo,
                    f.x + f.rx * lat,
                    f.z + f.rz * lat,
                    f.fx,
                    f.fz,
                    &mut rng,
                    loaded,
                );
            }
        }
        // Backed up to the shed doors.
        for (du, dlat) in self.dock_doors.clone() {
            if rng.next_f64() < 0.55 {
                continue;
            }
            let f = self.pframe(du);
            let lat = dlat - 9.4;
            let loaded = rng.next_f64() < 0.6;
            self.truck(
                &mut geo,
                f.x + f.rx * lat,
                f.z + f.rz * lat,
                -f.rx,
                -f.rz,
                &mut rng,
                loaded,
            );
        }
        if !geo.is_empty() {
            let paint = self.paint();
            self.badd(geo.build(), paint, true, 700.0);
        }
    }

    // ── Sheds and offices on the right ──────────────────────────────────

    fn build_sheds(&mut self) {
        let path = self.path.clone();
        let g0 = self.ground_y;
        let mut walls = GeoBuilder::new(true, false);
        let mut roofs = GeoBuilder::new(true, false);
        const TINTS: [u32; 6] = [0xb9c3cc, 0xd6cfbd, 0x9fb3a0, 0xb07d62, 0xc9ccd1, 0x8fa6b8];
        let doors = col(0x2f3236);
        let stripe = col(0x1d4f7a);
        let roller_c = [col(0x9aa3ab), col(0x7d8f9e), col(0xb9b3a4)];
        self.dock_doors = Vec::new();
        let mut shed_count = 0usize;
        struct ShedSign {
            rect: crate::coast::kit::Rect,
            at: P3,
            yaw: f64,
            w: f64,
        }
        let mut shed_signs: Vec<ShedSign> = Vec::new();
        let mut shed_atlas = SignAtlas::new();
        let mut u = self.port_start() - 30.0;
        let end = path.u1 - 40.0;
        let mut first = true;
        while u < end {
            let l = 90.0 + self.rng.next_f64() * 60.0;
            let d = 34.0 + self.rng.next_f64() * 22.0;
            let h = 10.0 + self.rng.next_f64() * 5.0;
            let uc = u + l / 2.0;
            let f = self.pframe(js::min(uc, path.u1 - 1.0));
            let front = 46.0 + self.rng.next_f64() * 10.0;
            let lat_c = front + d / 2.0;
            let cx = f.x + f.rx * lat_c;
            let cz = f.z + f.rz * lat_c;
            let yaw = kernel::atan2(f.fz, f.fx);
            let c = col(TINTS[(self.rng.next_f64() * TINTS.len() as f64).floor() as usize]);
            walls.box_(
                cx,
                g0,
                cz,
                l,
                h,
                d,
                yaw,
                &PrismOpts {
                    tile_w: Some(12.0),
                    tile_h: Some(12.0),
                    color: Some(c),
                    roof: Some(false),
                    ..PrismOpts::default()
                },
            );
            // Low gable roof.
            let ax = kernel::cos(yaw);
            let az = kernel::sin(yaw);
            let rx = -az;
            let rz = ax;
            let pt =
                |a: f64, o: f64, y: f64| -> P3 { [cx + ax * a + rx * o, y, cz + az * a + rz * o] };
            let rc = col(0x8d9296);
            for side in [-1.0, 1.0] {
                quad_out(
                    &mut roofs,
                    pt(-l / 2.0 - 0.5, side * (d / 2.0 + 0.5), h + g0),
                    pt(l / 2.0 + 0.5, side * (d / 2.0 + 0.5), h + g0),
                    pt(l / 2.0 + 0.5, 0.0, h + g0 + 2.4),
                    pt(-l / 2.0 - 0.5, 0.0, h + g0 + 2.4),
                    [rx * side, 1.0, rz * side],
                    Some(rc),
                    None,
                );
            }
            for e in [-1.0, 1.0] {
                roofs.tri(
                    pt(e * l / 2.0, -d / 2.0, h + g0),
                    pt(e * l / 2.0, d / 2.0, h + g0),
                    pt(e * l / 2.0, 0.0, h + g0 + 2.4),
                    Some(c),
                    0.0,
                );
                roofs.tri(
                    pt(e * l / 2.0, d / 2.0, h + g0),
                    pt(e * l / 2.0, -d / 2.0, h + g0),
                    pt(e * l / 2.0, 0.0, h + g0 + 2.4),
                    Some(c),
                    0.0,
                );
            }
            // Roller doors (ribs run across: the wall texture turned 90°) in
            // dark frames, rubber dock bumpers, and a canopy over the dock.
            let rot = [[0.0, 0.0], [0.0, 1.2], [3.0, 1.2], [3.0, 0.0]];
            let mut a = -l / 2.0 + 8.0;
            while a < l / 2.0 - 6.0 {
                quad_out(
                    &mut walls,
                    pt(a - 0.3, -d / 2.0 - 0.04, g0),
                    pt(a + 5.3, -d / 2.0 - 0.04, g0),
                    pt(a + 5.3, -d / 2.0 - 0.04, g0 + 5.8),
                    pt(a - 0.3, -d / 2.0 - 0.04, g0 + 5.8),
                    [-rx, 0.0, -rz],
                    Some(doors),
                    None,
                );
                let ri = (js::round(a / 9.0 + u).abs() % 3.0) as usize;
                quad_out(
                    &mut walls,
                    pt(a, -d / 2.0 - 0.08, g0 + 1.2),
                    pt(a + 5.0, -d / 2.0 - 0.08, g0 + 1.2),
                    pt(a + 5.0, -d / 2.0 - 0.08, g0 + 5.5),
                    pt(a, -d / 2.0 - 0.08, g0 + 5.5),
                    [-rx, 0.0, -rz],
                    Some(roller_c[ri]),
                    Some(rot),
                );
                for e in [0.3, 4.7] {
                    let q = pt(a + e, -d / 2.0 - 0.3, 0.0);
                    walls.box_(q[0], g0 + 0.5, q[2], 0.4, 0.7, 0.5, yaw, &copts(doors));
                }
                self.dock_doors.push((uc + a + 2.5, front));
                a += 9.0;
            }
            let dock_f = pt(0.0, -d / 2.0 - 0.8, 0.0);
            walls.box_(
                dock_f[0],
                g0,
                dock_f[2],
                l - 8.0,
                1.2,
                1.6,
                yaw,
                &copts(col(0x9a978f)),
            );
            let can = pt(0.0, -d / 2.0 - 1.5, 0.0);
            roofs.box_(
                can[0],
                g0 + 6.3,
                can[2],
                l - 6.0,
                0.3,
                3.0,
                yaw,
                &copts(col(0x6d7378)),
            );
            // Skylight strips and vents along the roof.
            for side in [-1.0, 1.0] {
                let mut a = -l / 2.0 + 6.0;
                while a < l / 2.0 - 6.0 {
                    let o0 = side * d * 0.14;
                    let o1 = side * d * 0.3;
                    let y0 = h + g0 + 2.4 * (1.0 - o0.abs() / (d / 2.0)) + 0.05;
                    let y1 = h + g0 + 2.4 * (1.0 - o1.abs() / (d / 2.0)) + 0.05;
                    quad_out(
                        &mut roofs,
                        pt(a, o0, y0),
                        pt(a + 6.0, o0, y0),
                        pt(a + 6.0, o1, y1),
                        pt(a, o1, y1),
                        [0.0, 1.0, 0.0],
                        Some(col(0xc9dde6)),
                        None,
                    );
                    a += 12.0;
                }
            }
            let mut a = -l / 2.0 + 12.0;
            while a < l / 2.0 - 8.0 {
                let q = pt(a, 0.0, 0.0);
                roofs.box_(
                    q[0],
                    g0 + h + 2.3,
                    q[2],
                    1.4,
                    1.2,
                    1.4,
                    yaw,
                    &copts(col(0x9a9ea3)),
                );
                a += 24.0;
            }
            // Company name on the wall facing the road.
            let nm = SHED_NAMES[shed_count % SHED_NAMES.len()];
            shed_count += 1;
            let rect = shed_atlas.add(640.0, 96.0, |g, w, hh| {
                panel(g, w, hh, nm.1, "#ffffff", nm.1, &[nm.0], &[50.0])
            });
            shed_signs.push(ShedSign {
                rect,
                at: pt(0.0, -d / 2.0 - 0.12, g0 + h - 2.2),
                yaw: kernel::atan2(-rx, -rz),
                w: js::min(l * 0.4, 20.0),
            });
            quad_out(
                &mut walls,
                pt(-l / 2.0, -d / 2.0 - 0.06, g0 + h - 2.2),
                pt(l / 2.0, -d / 2.0 - 0.06, g0 + h - 2.2),
                pt(l / 2.0, -d / 2.0 - 0.06, g0 + h - 1.2),
                pt(-l / 2.0, -d / 2.0 - 0.06, g0 + h - 1.2),
                [-rx, 0.0, -rz],
                Some(stripe),
                None,
            );
            if first {
                // Rooftop sign facing arriving traffic.
                let (texture, aspect) = sign_of(
                    self.graph,
                    self.textures,
                    &["PORT MERIDIAN"],
                    &SignOpts {
                        bg: "#123a52",
                        fg: "#ffffff",
                        border: Some("#ffffff"),
                        w: 1024,
                        h: 180,
                        font: "bold 110px \"Arial Narrow\", Arial, sans-serif",
                        arrow: None,
                    },
                );
                let m = self.graph.add_material(
                    Material::standard()
                        .set("map", texture)
                        .set("emissive", 0xffffff)
                        .set("emissiveMap", texture)
                        .set("emissiveIntensity", 0.1)
                        .set("roughness", 0.5)
                        .set("side", DOUBLE_SIDE),
                );
                self.graph.add_night(Some(m), "emissiveIntensity", 0.1, 0.8);
                let sh = 6.0;
                let sw = sh * aspect;
                let pg = self.graph.add_geometry(plane_geometry(sw, sh, 1.0, 1.0));
                let sign = self.graph.mesh(pg, m);
                let p = pt(-l / 2.0 + sw / 2.0 + 4.0, -d / 4.0, 0.0);
                {
                    let o = self.graph.get_mut(sign);
                    o.position = Vector3::new(p[0], g0 + h + 2.4 + sh / 2.0 + 1.0, p[2]);
                    o.set_rotation(&Euler::new(
                        0.0,
                        kernel::atan2(-ax * 0.6 - rx * 0.8, -az * 0.6 - rz * 0.8),
                        0.0,
                    ));
                }
                self.add(sign);
                for k in [-0.35, 0.35] {
                    let q = pt(-l / 2.0 + sw / 2.0 + 4.0 + k * sw, -d / 4.0 + 0.6, 0.0);
                    walls.box_(
                        q[0],
                        g0 + h,
                        q[2],
                        0.4,
                        3.8,
                        0.4,
                        0.0,
                        &copts(col(0x55585c)),
                    );
                }
                first = false;
            }
            u += l + 20.0 + self.rng.next_f64() * 25.0;
        }
        // Wall names: one atlas, one mesh.
        let mut sg: Vec<BufferGeometry> = Vec::new();
        for n in &shed_signs {
            let h = n.w * 96.0 / 640.0;
            let mut g = plane_geometry(n.w, h, 1.0, 1.0);
            let uv = g.get_attribute_mut("uv").expect("uv");
            for k in 0..uv.count() {
                let (x, y) = (uv.get_x(k), uv.get_y(k));
                uv.set_xy(
                    k,
                    lerp(n.rect.u0, n.rect.u1, x),
                    lerp(n.rect.v0, n.rect.v1, y),
                );
            }
            g.rotate_y(n.yaw);
            g.translate(n.at[0], n.at[1], n.at[2]);
            sg.push(g);
        }
        if !sg.is_empty() {
            let tex = crate::coast::kit::canvas_tex(self.graph, &shed_atlas.canvas);
            let m = self.graph.add_material(
                Material::standard()
                    .set("map", tex)
                    .set("roughness", 0.6)
                    .set("polygonOffset", true)
                    .set("polygonOffsetFactor", -2.0)
                    .set("polygonOffsetUnits", -2.0),
            );
            let refs: Vec<&BufferGeometry> = sg.iter().collect();
            let merged = merge_geometries(&refs, false).expect("the names merge");
            self.badd(merged, m, false, 700.0);
        }
        let wall_mat = self.mat("shedWall", |bb| {
            let map = bb.graph.cached_texture(
                &textures::corrugated_texture(bb.textures),
                Layer::Main,
                "",
            );
            bb.graph.add_material(
                Material::standard()
                    .set("map", map)
                    .set("vertexColors", true)
                    .set("roughness", 0.75)
                    .set("metalness", 0.2),
            )
        });
        self.badd(walls.build(), wall_mat, true, 700.0);
        let paint = self.paint();
        self.badd(roofs.build(), paint, true, 700.0);
        // Terminal office: a glassy block by the gate end of the port.
        let mut og = GeoBuilder::new(true, false);
        let f = self.pframe(self.port_start() + 20.0);
        let ox = f.x + f.rx * 40.0;
        let oz = f.z + f.rz * 40.0;
        og.box_(
            ox,
            g0,
            oz,
            26.0,
            22.0,
            18.0,
            kernel::atan2(f.fz, f.fx),
            &roofed(col(0x6f8fa6), col(0x9aa0a6)),
        );
        for k in 0..6 {
            og.box_(
                ox,
                g0 + 2.5 + f64::from(k) * 3.5,
                oz,
                26.2,
                0.5,
                18.2,
                kernel::atan2(f.fz, f.fx),
                &unroofed(col(0xdedede)),
            );
        }
        self.badd(og.build(), paint, true, 700.0);
    }

    // ── On the water ────────────────────────────────────────────────────

    fn build_boats(&mut self) {
        let t = self.t;
        let paint = self.paint();
        let mut boats: Vec<Boat> = Vec::new();
        // Tugs by the ship's bow.
        if let Some(s) = self.ship {
            for (k, off) in [(0.0, 1.0), (1.0, 0.75)] {
                let mut g = GeoBuilder::new(true, false);
                hull(&mut g, 28.0, 10.0, 3.0, col(0x1d1f24), col(0x8a3b2a));
                g.box_(
                    -2.0,
                    3.0,
                    0.0,
                    8.0,
                    3.2,
                    6.5,
                    0.0,
                    &roofed(col(0xe9e9e4), col(0xb3342b)),
                );
                g.box_(
                    -1.0,
                    6.2,
                    0.0,
                    5.0,
                    2.4,
                    5.5,
                    0.0,
                    &roofed(col(0xe9e9e4), col(0x2b2d31)),
                );
                g.box_(-7.0, 3.0, 0.0, 1.4, 7.0, 1.4, 0.0, &copts(col(0xb3342b)));
                let m = self.boat_mesh(g, paint);
                let x = s.cx + s.dx * (s.l / 2.0 + 30.0 + k * 40.0) + s.nx * (off * 30.0);
                let z = s.cz + s.dz * (s.l / 2.0 + 30.0 + k * 40.0) + s.nz * (off * 30.0);
                boats.push(Boat {
                    m,
                    x,
                    z,
                    yaw: -kernel::atan2(s.dz, s.dx) + k * 0.6,
                    ph: k * 1.7,
                    drift: None,
                });
            }
        }
        // A sailboat and a fishing boat in the shipping channel.
        if let Some(span) = self.span.clone() {
            let sm = (span.s0 + span.s1) / 2.0;
            let f = t.frame(sm);
            let mut g = GeoBuilder::new(true, false);
            hull(&mut g, 11.0, 3.6, 1.2, col(0xf2f2ee), col(0xb89a72));
            g.box_(0.0, 1.2, 0.0, 0.25, 14.0, 0.25, 0.0, &copts(col(0xd0d0d0)));
            g.tri(
                [0.3, 2.5, 0.0],
                [0.3, 15.0, 0.0],
                [5.0, 2.5, 0.0],
                Some(col(0xfafaf6)),
                0.0,
            );
            g.tri(
                [0.3, 2.5, 0.0],
                [5.0, 2.5, 0.0],
                [0.3, 15.0, 0.0],
                Some(col(0xfafaf6)),
                0.0,
            );
            g.tri(
                [-0.3, 3.0, 0.0],
                [-4.5, 3.0, 0.0],
                [-0.3, 12.0, 0.0],
                Some(col(0xe8e2d0)),
                0.0,
            );
            g.tri(
                [-0.3, 3.0, 0.0],
                [-0.3, 12.0, 0.0],
                [-4.5, 3.0, 0.0],
                Some(col(0xe8e2d0)),
                0.0,
            );
            let sail = self.boat_mesh(g, paint);
            boats.push(Boat {
                m: sail,
                x: f.x - f.rx * 180.0,
                z: f.z - f.rz * 180.0,
                yaw: 0.0,
                ph: 0.4,
                drift: Some(Drift {
                    ax: -f.rx,
                    az: -f.rz,
                    range: 520.0,
                    speed: 3.2,
                    base: [f.x - f.rx * 60.0, f.z - f.rz * 60.0],
                }),
            });
            let mut g = GeoBuilder::new(true, false);
            hull(&mut g, 16.0, 5.0, 2.0, col(0x2e6d8e), col(0x9a8f7c));
            g.box_(
                -3.0,
                2.0,
                0.0,
                5.0,
                3.0,
                4.0,
                0.0,
                &roofed(col(0xe9e9e4), col(0x2e6d8e)),
            );
            g.box_(3.0, 2.0, 0.0, 0.3, 8.0, 0.3, 0.0, &copts(col(0x55585c)));
            let fish = self.boat_mesh(g, paint);
            boats.push(Boat {
                m: fish,
                x: f.x + f.rx * 260.0 + f.fx * 120.0,
                z: f.z + f.rz * 260.0 + f.fz * 120.0,
                yaw: 1.2,
                ph: 2.1,
                drift: None,
            });
        }
        let mut time = 0.0;
        self.animators
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                time += u.dt;
                for b in &boats {
                    let (mut x, mut z, mut yaw) = (b.x, b.z, b.yaw);
                    if let Some(d) = &b.drift {
                        let dd = (time * d.speed) % (d.range * 2.0);
                        let k = if dd < d.range { dd } else { d.range * 2.0 - dd };
                        x = d.base[0] + d.ax * (k - d.range * 0.5);
                        z = d.base[1] + d.az * (k - d.range * 0.5);
                        let sgn = if dd < d.range { 1.0 } else { -1.0 };
                        yaw = -kernel::atan2(d.az * sgn, d.ax * sgn);
                    }
                    let q = crate::three_geom::Quaternion::from_euler(&Euler::with_order(
                        kernel::sin(time * 0.9 + b.ph) * 0.03,
                        yaw,
                        kernel::sin(time * 1.1 + b.ph * 2.0) * 0.05,
                        EulerOrder::YXZ,
                    ));
                    out.push(Edit {
                        target: Handle::Node(b.m),
                        change: Change::Transform {
                            position: [x, 0.15 + kernel::sin(time * 1.3 + b.ph) * 0.25, z],
                            quaternion: [q.x, q.y, q.z, q.w],
                            scale: [1.0, 1.0, 1.0],
                        },
                    });
                }
            }));
        // Channel buoys.
        if let Some(span) = self.span.clone() {
            let mut mats: Vec<Matrix4> = Vec::new();
            let mut colors: Vec<u32> = Vec::new();
            for (s, c) in [(span.s0 + 160.0, 0x2f9a45), (span.s1 - 160.0, 0xc0302a)] {
                let f = t.frame(s);
                for lat in [-260.0, -400.0, -540.0, 150.0, 300.0] {
                    mats.push(trs(
                        f.x + f.rx * lat,
                        -0.5,
                        f.z + f.rz * lat,
                        0.0,
                        1.2,
                        3.2,
                        1.2,
                        0.0,
                        0.0,
                    ));
                    colors.push(c);
                }
            }
            let mut g = cylinder_geometry(0.6, 1.0, 1.0, 10.0, 1.0, false, 0.0, PI * 2.0);
            g.translate(0.0, 0.5, 0.0);
            let gid = self.graph.add_geometry(g);
            let m = self
                .graph
                .add_material(Material::standard().set("roughness", 0.6));
            let im = instanced(self.graph, gid, m, &mats, false, false);
            {
                let inst = self
                    .graph
                    .get_mut(im)
                    .instances
                    .as_mut()
                    .expect("instanced");
                for (i, c) in colors.iter().enumerate() {
                    inst.set_color_at(i, Color::hex(*c));
                }
            }
            self.add(im);
        }
    }

    /// A boat's mesh: `mk(build)`.
    fn boat_mesh(&mut self, g: GeoBuilder, paint: MaterialId) -> NodeId {
        let gid = self.graph.add_geometry(g.build());
        let m = self.graph.mesh(gid, paint);
        {
            let o = self.graph.get_mut(m);
            o.cast_shadow = true;
            o.user_data.insert("animated".into(), Value::Bool(true));
        }
        self.add(m);
        m
    }

    /// Rubble breakwaters either side of the harbour mouth, each with a
    /// small lighthouse at its tip.
    fn build_breakwaters(&mut self) {
        let t = self.t;
        let Some(span) = self.span.clone() else {
            return;
        };
        let mut rng = Mulberry32::new(31);
        let mut rocks: Vec<Matrix4> = Vec::new();
        let arms = [
            (
                span.s0 + 40.0,
                span.s0 + 170.0,
                -196.0,
                -470.0,
                0x2fd05a_u32,
            ),
            (
                span.s1 - 40.0,
                span.s1 - 170.0,
                -196.0,
                -470.0,
                0xff3a2a_u32,
            ),
        ];
        let mut geo = GeoBuilder::new(true, false);
        let mut lamps: Vec<(P3, u32)> = Vec::new();
        for (s0, s1, lat0, lat1, light) in arms {
            let f0 = t.frame(s0);
            let f1 = t.frame(s1);
            let a = [f0.x + f0.rx * lat0, f0.z + f0.rz * lat0];
            let bp = [f1.x + f1.rx * lat1, f1.z + f1.rz * lat1];
            let l = kernel::hypot(bp[0] - a[0], bp[1] - a[1]);
            let dx = (bp[0] - a[0]) / l;
            let dz = (bp[1] - a[1]) / l;
            let nx = -dz;
            let nz = dx;
            let mut d = 0.0;
            while d < l {
                for o in [-7.0, -3.5, 0.0, 3.5, 7.0] {
                    let x = a[0] + dx * d + nx * (o + (rng.next_f64() - 0.5) * 2.0);
                    let z = a[1] + dz * d + nz * (o + (rng.next_f64() - 0.5) * 2.0);
                    let top = if f64::abs(o) < 4.0 { 3.2 } else { 0.8 };
                    let s = 2.2 + rng.next_f64() * 2.4;
                    let yaw = rng.next_f64() * 6.0;
                    let sx = s * (0.9 + rng.next_f64() * 0.4);
                    let sy = s * (0.7 + rng.next_f64() * 0.4);
                    let sz = s * (0.9 + rng.next_f64() * 0.4);
                    let pitch = rng.next_f64() * 0.6;
                    let roll = rng.next_f64() * 0.6;
                    rocks.push(trs(x, top - s * 0.5, z, yaw, sx, sy, sz, pitch, roll));
                }
                d += 3.2;
            }
            // Lighthouse: tapered white tower with a coloured band and
            // lantern.
            let (lx, lz) = (bp[0], bp[1]);
            frustum(
                &mut geo,
                lx,
                lz,
                3.0,
                16.0,
                dx,
                dz,
                3.4,
                3.4,
                2.4,
                2.4,
                Some(col(0xf0efe8)),
                4.0,
            );
            frustum(
                &mut geo,
                lx,
                lz,
                9.0,
                11.5,
                dx,
                dz,
                2.95,
                2.95,
                2.8,
                2.8,
                Some(col(if light == 0xff3a2a {
                    0xc0302a
                } else {
                    0x2f9a45
                })),
                4.0,
            );
            frustum(
                &mut geo,
                lx,
                lz,
                16.0,
                16.4,
                dx,
                dz,
                3.4,
                3.4,
                3.4,
                3.4,
                Some(col(0x2b2d31)),
                4.0,
            );
            frustum(
                &mut geo,
                lx,
                lz,
                18.6,
                19.4,
                dx,
                dz,
                2.2,
                2.2,
                0.6,
                0.6,
                Some(col(0x2b2d31)),
                4.0,
            );
            lamps.push(([lx, 17.5, lz], light));
        }
        let rock_geo = self.graph.add_geometry(dodecahedron_geometry(0.62, 0.0));
        let rock_mat = self.graph.add_material(
            Material::standard()
                .set("color", 0x8a8580)
                .set("roughness", 0.95)
                .set("flatShading", true),
        );
        let n = instanced(self.graph, rock_geo, rock_mat, &rocks, false, true);
        self.add(n);
        let paint = self.paint();
        self.badd(geo.build(), paint, true, 3000.0);
        let lamp_mat = self
            .graph
            .add_material(Material::basic().set("color", 0xffffff));
        let sg = self
            .graph
            .add_geometry(sphere_geometry(1.0, 10.0, 8.0, 0.0, PI * 2.0, 0.0, PI));
        let mats: Vec<Matrix4> = lamps
            .iter()
            .map(|(p, _)| trs(p[0], p[1], p[2], 0.0, 1.0, 1.0, 1.0, 0.0, 0.0))
            .collect();
        let im = instanced(self.graph, sg, lamp_mat, &mats, false, false);
        {
            let inst = self
                .graph
                .get_mut(im)
                .instances
                .as_mut()
                .expect("instanced");
            for (i, (_, c)) in lamps.iter().enumerate() {
                let mut cc = Color::hex(*c);
                cc.multiply_scalar(3.0);
                inst.set_color_at(i, cc);
            }
        }
        self.add(im);
        let lights: Vec<u32> = lamps.iter().map(|(_, c)| *c).collect();
        let mut time = 0.0;
        self.animators
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                time += u.dt;
                for (i, &c) in lights.iter().enumerate() {
                    let mut cc = Color::hex(c);
                    cc.multiply_scalar(if ((time + i as f64 * 1.3) % 4.0) < 1.0 {
                        4.0
                    } else {
                        0.35
                    });
                    out.push(Edit {
                        target: Handle::Node(im),
                        change: Change::InstanceColor {
                            index: i as u32,
                            rgb: [cc.r as f32, cc.g as f32, cc.b as f32],
                        },
                    });
                }
            }));
    }
}

/// A drifting boat's course.
#[derive(Clone, Copy, Debug)]
struct Drift {
    ax: f64,
    az: f64,
    range: f64,
    speed: f64,
    base: P2,
}

/// A boat on the water: `{ m, x, z, yaw, ph, drift, ... }`.
#[derive(Clone, Copy, Debug)]
struct Boat {
    m: NodeId,
    x: f64,
    z: f64,
    yaw: f64,
    ph: f64,
    drift: Option<Drift>,
}

/// Boat hull along +x with a pointed bow, bottom at y=-1.
fn hull(geo: &mut GeoBuilder, l: f64, b: f64, h: f64, c: P3, deck_c: P3) {
    let pts: [P2; 5] = [
        [-l / 2.0, -b / 2.0 * 0.85],
        [l * 0.2, -b / 2.0],
        [l / 2.0, 0.0],
        [l * 0.2, b / 2.0],
        [-l / 2.0, b / 2.0 * 0.85],
    ];
    for i in 0..pts.len() {
        let a = pts[i];
        let bb = pts[(i + 1) % pts.len()];
        let mx = (a[0] + bb[0]) / 2.0;
        let mz = (a[1] + bb[1]) / 2.0;
        quad_out(
            geo,
            [a[0], -1.0, a[1]],
            [bb[0], -1.0, bb[1]],
            [bb[0], h, bb[1]],
            [a[0], h, a[1]],
            [mx, 0.0, mz],
            Some(c),
            None,
        );
    }
    // Deck (wound to face up).
    geo.tri(
        [pts[0][0], h, pts[0][1]],
        [pts[2][0], h, pts[2][1]],
        [pts[1][0], h, pts[1][1]],
        Some(deck_c),
        0.0,
    );
    geo.tri(
        [pts[0][0], h, pts[0][1]],
        [pts[3][0], h, pts[3][1]],
        [pts[2][0], h, pts[2][1]],
        Some(deck_c),
        0.0,
    );
    geo.tri(
        [pts[0][0], h, pts[0][1]],
        [pts[4][0], h, pts[4][1]],
        [pts[3][0], h, pts[3][1]],
        Some(deck_c),
        0.0,
    );
}

/// `{ color }`.
fn copts(c: P3) -> PrismOpts {
    PrismOpts {
        color: Some(c),
        ..PrismOpts::default()
    }
}

/// `{ color, roofColor }`.
fn roofed(c: P3, roof: P3) -> PrismOpts {
    PrismOpts {
        color: Some(c),
        roof_color: Some(roof),
        ..PrismOpts::default()
    }
}

/// `{ color, roof: false }`.
fn unroofed(c: P3) -> PrismOpts {
    PrismOpts {
        color: Some(c),
        roof: Some(false),
        ..PrismOpts::default()
    }
}
