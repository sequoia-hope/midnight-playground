//! A corridor: a route or event's real road compressed to the plan's scale
//! and written out for the game (docs/vision/peninsula.md, stage 1).
//!
//! `mp_levels` includes the file at compile time and builds a level's path
//! from it, so the game never reads the atlas. The file is text, one line a
//! sample, in game metres: `x z y cover`. It is small (about 60 kB for the
//! first race), diffable, and read with `split_whitespace`.

use crate::geo::{V2, point_along, polyline_length};
use crate::profile::STEP;
use crate::report::MeasuredRoute;
use crate::terrain::Terrain;

/// Samples this far apart along the real road (m): 5 game metres at the
/// plan's scale of 0.5.
pub const REAL_STEP: f64 = 10.0;
/// The real profile is smoothed with a triangular window this wide either
/// side (m) before it is compressed: the planning grid is 50 m cells, and
/// one noisy cell must not be a bump in the road.
pub const SMOOTH: f64 = 60.0;
/// The real line is smoothed the same way, this wide either side (m): the
/// map's road is a chain of straight segments between its nodes, and a
/// road is a curve. A kink of ten degrees moves under a metre; the samples
/// are then written to the centimetre, so the road has no wobble at the
/// sample spacing.
pub const SMOOTH_LINE: f64 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Game metres, the levels' frame (x east, z south, y up).
    pub x: f64,
    pub z: f64,
    pub y: f64,
    /// The land cover at the sample, as the terrain names it.
    pub cover: &'static str,
}

#[derive(Clone, Debug)]
pub struct Corridor {
    pub id: String,
    pub name: String,
    /// Game metres per real metre, horizontal and vertical alike.
    pub scale: f64,
    /// Real metres of road.
    pub real_length: f64,
    pub samples: Vec<Sample>,
}

/// The measured route or event `id`, compressed about the world's origin.
pub fn corridor(m: &MeasuredRoute, t: &Terrain) -> Corridor {
    let line = &m.line;
    let length = polyline_length(line);
    let scale = m.scale;
    // The profile's heights, smoothed.
    let prof = &m.profile.samples;
    let half = (SMOOTH / STEP).round() as i64;
    let smooth: Vec<f64> = (0..prof.len() as i64)
        .map(|i| {
            let (mut sum, mut wsum) = (0.0, 0.0);
            for k in -half..=half {
                let j = i + k;
                if j < 0 || j >= prof.len() as i64 {
                    continue;
                }
                let w = (half + 1 - k.abs()) as f64;
                sum += prof[j as usize].h * w;
                wsum += w;
            }
            sum / wsum
        })
        .collect();
    let height_at = |d: f64| -> f64 {
        // The profile is sampled every STEP metres, the last at `length`.
        let f = d / STEP;
        let i = (f.floor() as usize).min(prof.len() - 1);
        let j = (i + 1).min(prof.len() - 1);
        let t = if j > i {
            ((d - prof[i].d) / (prof[j].d - prof[i].d)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        smooth[i] + (smooth[j] - smooth[i]) * t
    };
    let n = ((length / REAL_STEP).ceil() as usize).max(1);
    let pts: Vec<V2> = (0..=n)
        .map(|i| point_along(line, (i as f64 * REAL_STEP).min(length)))
        .collect();
    // The line, smoothed along itself (the ends stay where they are: a
    // junction is a point of the parent road).
    let half = (SMOOTH_LINE / REAL_STEP).round() as i64;
    let smooth_pt = |i: usize| -> V2 {
        let (mut sx, mut sz, mut wsum) = (0.0, 0.0, 0.0);
        for k in -half..=half {
            let j = i as i64 + k;
            if j < 0 || j > n as i64 {
                continue;
            }
            let w = (half + 1 - k.abs()) as f64;
            sx += pts[j as usize].x * w;
            sz += pts[j as usize].z * w;
            wsum += w;
        }
        let p = V2 {
            x: sx / wsum,
            z: sz / wsum,
        };
        // Toward the ends the window is one-sided, which would pull the
        // road in: fade the smoothing out over the window there.
        let end = (i.min(n - i) as f64 / half as f64).min(1.0);
        pts[i].lerp(p, end)
    };
    let samples = (0..=n)
        .map(|i| {
            let d = (i as f64 * REAL_STEP).min(length);
            let p = smooth_pt(i);
            Sample {
                x: p.x * scale,
                z: p.z * scale,
                y: height_at(d) * scale,
                cover: t.cover_at(pts[i]).name(),
            }
        })
        .collect();
    Corridor {
        id: m.id.clone(),
        name: m.name.clone(),
        scale,
        real_length: length,
        samples,
    }
}

/// The corridor file.
pub fn text(c: &Corridor) -> String {
    let mut s = String::new();
    s.push_str(&format!("# {}: {}\n", c.id, c.name));
    s.push_str(
        "# Written by `cargo run -p mp_atlas -- corridor <id>` from the atlas; do not edit.\n",
    );
    s.push_str(&format!(
        "# scale {} (game metres per real metre), {:.0} real metres of road, a sample every {} game metres.\n",
        c.scale,
        c.real_length,
        REAL_STEP * c.scale
    ));
    s.push_str(
        "# x z y cover: game metres (x east, z south, y up), the land cover beside the road.\n",
    );
    for p in &c.samples {
        s.push_str(&format!("{:.2} {:.2} {:.2} {}\n", p.x, p.z, p.y, p.cover));
    }
    s
}
