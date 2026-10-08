//! `geo.json`: the projection, the road graph, railways, parks and towns.

use serde_json::Value;

/// A point in the local frame: x east, z south, metres from the origin.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V2 {
    pub x: f64,
    pub z: f64,
}

impl V2 {
    pub const fn new(x: f64, z: f64) -> V2 {
        V2 { x, z }
    }

    pub fn dist(self, o: V2) -> f64 {
        let (dx, dz) = (self.x - o.x, self.z - o.z);
        (dx * dx + dz * dz).sqrt()
    }

    pub fn lerp(self, o: V2, t: f64) -> V2 {
        V2::new(self.x + (o.x - self.x) * t, self.z + (o.z - self.z) * t)
    }
}

/// Latitude and longitude in degrees (WGS84), in that order, as Google
/// Maps copies them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

/// Equirectangular about the origin, with the metres per degree the build
/// script computed at the origin's latitude (so no trigonometry here).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projection {
    pub origin: LatLon,
    pub m_per_deg_lat: f64,
    pub m_per_deg_lon: f64,
}

impl Projection {
    pub fn to_local(&self, p: LatLon) -> V2 {
        V2::new(
            (p.lon - self.origin.lon) * self.m_per_deg_lon,
            -(p.lat - self.origin.lat) * self.m_per_deg_lat,
        )
    }

    pub fn to_latlon(&self, v: V2) -> LatLon {
        LatLon {
            lat: self.origin.lat - v.z / self.m_per_deg_lat,
            lon: self.origin.lon + v.x / self.m_per_deg_lon,
        }
    }
}

/// A road between two junctions (or a junction and a dead end).
#[derive(Clone, Debug)]
pub struct Road {
    pub a: usize,
    pub b: usize,
    /// motorway, trunk, primary, secondary or tertiary.
    pub class: String,
    pub name: Option<String>,
    /// Route numbers: "1", "92", "35", "84", "280", "101".
    pub refs: Vec<String>,
    pub bridge: bool,
    pub tunnel: bool,
    pub pts: Vec<V2>,
    /// Its length (m).
    pub length: f64,
}

impl Road {
    pub fn has_ref(&self, r: &str) -> bool {
        self.refs.iter().any(|x| x == r)
    }
}

/// A named park or protected area.
#[derive(Clone, Debug)]
pub struct Area {
    pub kind: String,
    pub name: String,
    pub ring: Vec<V2>,
}

/// A town or neighbourhood's label.
#[derive(Clone, Debug)]
pub struct Town {
    pub name: String,
    pub kind: String,
    pub at: V2,
}

/// A railway line.
#[derive(Clone, Debug)]
pub struct Rail {
    pub class: String,
    pub name: Option<String>,
    pub pts: Vec<V2>,
}

pub struct Geo {
    pub name: String,
    pub projection: Projection,
    pub nodes: Vec<V2>,
    pub roads: Vec<Road>,
    pub rail: Vec<Rail>,
    pub areas: Vec<Area>,
    pub towns: Vec<Town>,
}

pub(crate) fn num(v: &Value, k: &str) -> Result<f64, String> {
    v[k].as_f64()
        .ok_or_else(|| format!("`{k}`: expected a number"))
}

pub(crate) fn text(v: &Value, k: &str) -> Option<String> {
    v[k].as_str().map(str::to_owned)
}

fn pts(v: &Value) -> Vec<V2> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| Some(V2::new(p[0].as_f64()?, p[1].as_f64()?)))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn polyline_length(p: &[V2]) -> f64 {
    p.windows(2).map(|w| w[0].dist(w[1])).sum()
}

impl Geo {
    pub fn from_json(bytes: &[u8]) -> Result<Geo, String> {
        let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("geo.json: {e}"))?;
        let o = &v["origin"];
        let pr = &v["projection"];
        let projection = Projection {
            origin: LatLon {
                lat: num(o, "lat")?,
                lon: num(o, "lon")?,
            },
            m_per_deg_lat: num(pr, "m_per_deg_lat")?,
            m_per_deg_lon: num(pr, "m_per_deg_lon")?,
        };
        let nodes = pts(&v["nodes"]);
        let arr = |k: &str| v[k].as_array().cloned().unwrap_or_default();
        let roads = arr("roads")
            .iter()
            .map(|r| {
                let pts = pts(&r["pts"]);
                Road {
                    a: r["a"].as_u64().unwrap_or(0) as usize,
                    b: r["b"].as_u64().unwrap_or(0) as usize,
                    class: text(r, "class").unwrap_or_default(),
                    name: text(r, "name"),
                    refs: r["refs"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|s| s.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                    bridge: r["bridge"].as_bool().unwrap_or(false),
                    tunnel: r["tunnel"].as_bool().unwrap_or(false),
                    length: polyline_length(&pts),
                    pts,
                }
            })
            .collect::<Vec<_>>();
        if let Some(r) = roads
            .iter()
            .find(|r| r.a >= nodes.len() || r.b >= nodes.len())
        {
            return Err(format!(
                "geo.json: a road joins node {} or {}, past the last",
                r.a, r.b
            ));
        }
        Ok(Geo {
            name: text(&v, "name").unwrap_or_default(),
            projection,
            nodes,
            roads,
            rail: arr("rail")
                .iter()
                .map(|r| Rail {
                    class: text(r, "class").unwrap_or_default(),
                    name: text(r, "name"),
                    pts: pts(&r["pts"]),
                })
                .collect(),
            areas: arr("areas")
                .iter()
                .map(|a| Area {
                    kind: text(a, "kind").unwrap_or_default(),
                    name: text(a, "name").unwrap_or_default(),
                    ring: pts(&a["ring"]),
                })
                .collect(),
            towns: arr("towns")
                .iter()
                .filter_map(|t| {
                    Some(Town {
                        name: text(t, "name")?,
                        kind: text(t, "kind").unwrap_or_default(),
                        at: V2::new(t["x"].as_f64()?, t["z"].as_f64()?),
                    })
                })
                .collect(),
        })
    }
}

/// Is `p` inside the polygon `ring` (even-odd rule)?
pub fn inside(p: V2, ring: &[V2]) -> bool {
    let mut c = false;
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (ring[i], ring[j]);
        if (a.z > p.z) != (b.z > p.z) && p.x < (b.x - a.x) * (p.z - a.z) / (b.z - a.z) + a.x {
            c = !c;
        }
        j = i;
    }
    c
}

/// The nearest point to `p` on a polyline: (distance from p, distance along).
pub fn nearest_on(p: V2, line: &[V2]) -> (f64, f64) {
    let mut best = (f64::INFINITY, 0.0);
    let mut along = 0.0;
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let l2 = dx * dx + dz * dz;
        let l = l2.sqrt();
        let t = if l2 > 0.0 {
            (((p.x - a.x) * dx + (p.z - a.z) * dz) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let d = p.dist(a.lerp(b, t));
        if d < best.0 {
            best = (d, along + t * l);
        }
        along += l;
    }
    best
}

/// The point `d` metres along a polyline.
pub fn point_along(line: &[V2], d: f64) -> V2 {
    let mut left = d.max(0.0);
    for w in line.windows(2) {
        let l = w[0].dist(w[1]);
        if left <= l && l > 0.0 {
            return w[0].lerp(w[1], left / l);
        }
        left -= l;
    }
    *line.last().unwrap_or(&V2::default())
}

/// The part of a polyline between two distances along it, in the order
/// asked (reversed when `d1 < d0`).
pub fn sub_line(line: &[V2], d0: f64, d1: f64) -> Vec<V2> {
    let (lo, hi) = if d0 <= d1 { (d0, d1) } else { (d1, d0) };
    let mut out = vec![point_along(line, lo)];
    let mut along = 0.0;
    for w in line.windows(2) {
        along += w[0].dist(w[1]);
        if along > lo && along < hi {
            out.push(w[1]);
        }
    }
    out.push(point_along(line, hi));
    if d1 < d0 {
        out.reverse();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_round_trips() {
        let p = Projection {
            origin: LatLon {
                lat: 37.5,
                lon: -122.4,
            },
            m_per_deg_lat: 111_000.0,
            m_per_deg_lon: 88_000.0,
        };
        let a = LatLon {
            lat: 37.6,
            lon: -122.3,
        };
        let v = p.to_local(a);
        assert!((v.x - 8_800.0).abs() < 1e-6 && (v.z + 11_100.0).abs() < 1e-6);
        let b = p.to_latlon(v);
        assert!((b.lat - a.lat).abs() < 1e-12 && (b.lon - a.lon).abs() < 1e-12);
    }

    #[test]
    fn lines_are_cut_and_measured() {
        let l = [V2::new(0.0, 0.0), V2::new(10.0, 0.0), V2::new(10.0, 10.0)];
        assert_eq!(polyline_length(&l), 20.0);
        assert_eq!(point_along(&l, 15.0), V2::new(10.0, 5.0));
        let (d, s) = nearest_on(V2::new(12.0, 4.0), &l);
        assert!((d - 2.0).abs() < 1e-12 && (s - 14.0).abs() < 1e-12);
        let part = sub_line(&l, 15.0, 5.0);
        assert_eq!(part.first(), Some(&V2::new(10.0, 5.0)));
        assert_eq!(part.last(), Some(&V2::new(5.0, 0.0)));
        assert!(inside(
            V2::new(1.0, 1.0),
            &[V2::new(0.0, 0.0), V2::new(4.0, 0.0), V2::new(0.0, 4.0)]
        ));
    }
}
