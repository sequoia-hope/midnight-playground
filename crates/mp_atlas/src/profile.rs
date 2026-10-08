//! A route measured over the real ground: its height every few metres, the
//! climb, the steepest grade, the crests (where a view opens), and what the
//! road passes through.

use crate::geo::{V2, point_along, polyline_length};
use crate::terrain::{Cover, Terrain};

/// Samples along a route this far apart (m).
pub const STEP: f64 = 20.0;
/// A crest stands at least this far above the road on both sides of it
/// before the road climbs higher (m).
pub const CREST_PROMINENCE: f64 = 6.0;
/// Grades are measured over this distance, so one noisy cell of the
/// terrain is not a wall (m).
pub const GRADE_RUN: f64 = 200.0;
/// What a road "passes through": the cover within this distance either
/// side (m).
pub const SIDE: f64 = 150.0;

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// Metres along the route.
    pub d: f64,
    pub at: V2,
    pub h: f64,
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub samples: Vec<Sample>,
    pub length: f64,
    pub climb: f64,
    pub descent: f64,
    pub h_min: f64,
    pub h_max: f64,
    /// The steepest grade over [`GRADE_RUN`], as a fraction (0.08 = 8 %).
    pub max_grade: f64,
    /// Indices into `samples` of the crests, in order.
    pub crests: Vec<usize>,
    /// The share of each cover beside the road, largest first.
    pub cover: Vec<(Cover, f64)>,
}

/// A sample this close to a tunnel or a bridge is on it (m).
const ON_STRUCTURE: f64 = 4.0;

impl Profile {
    /// Profiles a line over the ground. Through `structures` (the lines of
    /// the tunnels and bridges it may use) the road keeps its own level:
    /// the height runs straight from one end of the structure to the other
    /// instead of over the hill or down to the creek.
    pub fn of(line: &[V2], t: &Terrain, structures: &[&[V2]]) -> Profile {
        let length = polyline_length(line);
        let n = ((length / STEP).ceil() as usize).max(1);
        let mut samples: Vec<Sample> = (0..=n)
            .map(|i| {
                let d = (i as f64 * STEP).min(length);
                let at = point_along(line, d);
                Sample {
                    d,
                    at,
                    // A road never runs under the sea: the bridges and
                    // causeways the grid's 50 m cells miss.
                    h: t.height_at(at).max(0.0),
                }
            })
            .collect();
        let on: Vec<bool> = samples
            .iter()
            .map(|s| {
                structures
                    .iter()
                    .any(|l| crate::geo::nearest_on(s.at, l).0 < ON_STRUCTURE)
            })
            .collect();
        let mut i = 0;
        while i < samples.len() {
            if !on[i] {
                i += 1;
                continue;
            }
            let start = i;
            while i < samples.len() && on[i] {
                i += 1;
            }
            // From the last sample before to the first after (or level
            // where the structure starts or ends the route).
            let a = start.saturating_sub(1);
            let b = i.min(samples.len() - 1);
            let (ha, hb) = (samples[a].h, samples[b].h);
            let (da, db) = (samples[a].d, samples[b].d);
            for s in &mut samples[start..i] {
                let f = if db > da { (s.d - da) / (db - da) } else { 0.0 };
                s.h = ha + (hb - ha) * f;
            }
        }
        let (mut climb, mut descent) = (0.0, 0.0);
        for w in samples.windows(2) {
            let dh = w[1].h - w[0].h;
            if dh > 0.0 {
                climb += dh;
            } else {
                descent -= dh;
            }
        }
        let h_min = samples.iter().map(|s| s.h).fold(f64::INFINITY, f64::min);
        let h_max = samples
            .iter()
            .map(|s| s.h)
            .fold(f64::NEG_INFINITY, f64::max);
        let k = ((GRADE_RUN / STEP) as usize).max(1);
        let max_grade = samples
            .windows(k + 1)
            .map(|w| {
                let run = w[k].d - w[0].d;
                if run > 0.0 {
                    (w[k].h - w[0].h).abs() / run
                } else {
                    0.0
                }
            })
            .fold(0.0, f64::max);
        Profile {
            crests: crests(&samples),
            cover: cover_beside(line, length, t),
            samples,
            length,
            climb,
            descent,
            h_min,
            h_max,
            max_grade,
        }
    }

    /// The first crest at least `after` metres along.
    pub fn first_crest_after(&self, after: f64) -> Option<&Sample> {
        self.crests
            .iter()
            .map(|&i| &self.samples[i])
            .find(|s| s.d >= after)
    }
}

/// Local highs the road drops [`CREST_PROMINENCE`] from on both sides
/// before it climbs above them (an end of the route counts as a drop).
fn crests(s: &[Sample]) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 1..s.len().saturating_sub(1) {
        let h = s[i].h;
        if !(s[i - 1].h <= h && s[i + 1].h < h) {
            continue;
        }
        // Leftwards a higher point ends the search, rightwards an equal one
        // too, so a flat top counts once (at its last sample).
        let side = |range: &mut dyn Iterator<Item = usize>, right: bool| {
            let mut low = h;
            for j in range {
                if s[j].h > h || (right && s[j].h == h) {
                    return h - low >= CREST_PROMINENCE;
                }
                low = low.min(s[j].h);
            }
            h - low >= CREST_PROMINENCE
        };
        if side(&mut (0..i).rev(), false) && side(&mut (i + 1..s.len()), true) {
            out.push(i);
        }
    }
    out
}

fn cover_beside(line: &[V2], length: f64, t: &Terrain) -> Vec<(Cover, f64)> {
    let mut counts = [0usize; 10];
    let mut total = 0usize;
    let n = ((length / (2.0 * STEP)).ceil() as usize).max(1);
    for i in 0..=n {
        let d = (i as f64 * 2.0 * STEP).min(length);
        let p = point_along(line, d);
        let q = point_along(line, (d + 1.0).min(length));
        let r = point_along(line, (d - 1.0).max(0.0));
        let (dx, dz) = (q.x - r.x, q.z - r.z);
        let l = (dx * dx + dz * dz).sqrt();
        let (nx, nz) = if l > 0.0 {
            (-dz / l, dx / l)
        } else {
            (0.0, 0.0)
        };
        for k in [-3.0, -2.0, -1.0, 1.0, 2.0, 3.0] {
            let o = k / 3.0 * SIDE;
            let c = t.cover_at(V2::new(p.x + nx * o, p.z + nz * o));
            counts[c as usize] += 1;
            total += 1;
        }
    }
    let mut out: Vec<(Cover, f64)> = Cover::ALL
        .iter()
        .map(|&c| (c, counts[c as usize] as f64 / total.max(1) as f64))
        .filter(|&(_, f)| f > 0.0)
        .collect();
    // Largest first; ties in the classes' order (sort_by is stable).
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(hs: &[f64]) -> Vec<Sample> {
        hs.iter()
            .enumerate()
            .map(|(i, &h)| Sample {
                d: i as f64 * STEP,
                at: V2::default(),
                h,
            })
            .collect()
    }

    #[test]
    fn a_crest_needs_a_drop_on_both_sides() {
        // A bump of 3 m is not a crest; the 20 m hill is; the end is not.
        let v = s(&[0.0, 10.0, 20.0, 17.0, 20.0, 10.0, 0.0, 5.0, 8.0]);
        assert_eq!(crests(&v), vec![4]);
        let v = s(&[0.0, 10.0, 20.0, 12.0, 30.0, 0.0]);
        assert_eq!(crests(&v), vec![2, 4]);
    }
}
