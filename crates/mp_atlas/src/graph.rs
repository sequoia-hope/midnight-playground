//! Routes along the real roads: the shortest way between two places over
//! the roads a route may use (its route numbers, or its road classes).
//!
//! A place snaps to the nearest point on an allowed road, not to the
//! nearest junction: on the coast a road can run kilometres between
//! junctions. Roads are two-way here; one-way carriageways of a divided
//! highway are both in the graph and either does.

use crate::geo::{Geo, Road, V2, nearest_on, polyline_length, sub_line};
use crate::plan::Route;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Which roads a route may use.
pub struct Filter<'a> {
    pub refs: &'a [String],
    pub names: &'a [String],
    pub classes: &'a [String],
}

impl Filter<'_> {
    pub fn of(r: &Route) -> Filter<'_> {
        Filter {
            refs: &r.roads,
            names: &r.names,
            classes: &r.classes,
        }
    }

    pub fn allows(&self, road: &Road) -> bool {
        if !self.refs.is_empty() || !self.names.is_empty() {
            return self.refs.iter().any(|r| road.has_ref(r))
                || road
                    .name
                    .as_ref()
                    .is_some_and(|n| self.names.iter().any(|x| x == n));
        }
        self.classes.is_empty() || self.classes.contains(&road.class)
    }
}

/// Where a place meets the road network.
#[derive(Clone, Copy, Debug)]
pub struct Snap {
    pub road: usize,
    /// Metres along the road from its `a` end.
    pub along: f64,
    /// How far the place is from the road (m).
    pub off: f64,
    pub at: V2,
}

/// The nearest point on an allowed road.
pub fn snap(geo: &Geo, f: &Filter, p: V2) -> Option<Snap> {
    let mut best: Option<Snap> = None;
    for (i, r) in geo.roads.iter().enumerate() {
        if !f.allows(r) {
            continue;
        }
        let (d, along) = nearest_on(p, &r.pts);
        if best.is_none_or(|b| d < b.off) {
            best = Some(Snap {
                road: i,
                along,
                off: d,
                at: crate::geo::point_along(&r.pts, along),
            });
        }
    }
    best
}

#[derive(Clone, Copy, PartialEq)]
struct Item {
    cost: f64,
    node: usize,
}

impl Eq for Item {}

impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        // A min-heap on cost; ties broken by node, so the result never
        // depends on the heap's internals.
        o.cost
            .total_cmp(&self.cost)
            .then_with(|| o.node.cmp(&self.node))
    }
}

impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// The shortest way from one snap to another over the allowed roads, as a
/// polyline from `a` to `b`.
pub fn path(geo: &Geo, f: &Filter, a: Snap, b: Snap) -> Option<Vec<V2>> {
    let ra = &geo.roads[a.road];
    let rb = &geo.roads[b.road];
    // Along the same road and nothing shorter round the network: straight
    // along it.
    let direct = (a.road == b.road).then(|| (a.along - b.along).abs());
    let n = geo.nodes.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, r) in geo.roads.iter().enumerate() {
        if f.allows(r) {
            adj[r.a].push(i);
            adj[r.b].push(i);
        }
    }
    let mut dist = vec![f64::INFINITY; n];
    let mut prev: Vec<Option<usize>> = vec![None; n];
    let mut heap = BinaryHeap::new();
    for (node, cost) in [(ra.a, a.along), (ra.b, ra.length - a.along)] {
        if cost < dist[node] {
            dist[node] = cost;
            heap.push(Item { cost, node });
        }
    }
    while let Some(Item { cost, node }) = heap.pop() {
        if cost > dist[node] {
            continue;
        }
        for &e in &adj[node] {
            let r = &geo.roads[e];
            let other = if r.a == node { r.b } else { r.a };
            let c = cost + r.length;
            if c < dist[other] {
                dist[other] = c;
                prev[other] = Some(e);
                heap.push(Item {
                    cost: c,
                    node: other,
                });
            }
        }
    }
    let via_a = dist[rb.a] + b.along;
    let via_b = dist[rb.b] + (rb.length - b.along);
    let best = via_a.min(via_b);
    if let Some(d) = direct
        && d <= best
    {
        return Some(sub_line(&ra.pts, a.along, b.along));
    }
    if !best.is_finite() {
        return None;
    }
    // Back from the node b's road is entered by, to a's road.
    let (end_node, tail) = if via_a <= via_b {
        (rb.a, sub_line(&rb.pts, 0.0, b.along))
    } else {
        (rb.b, sub_line(&rb.pts, rb.length, b.along))
    };
    let mut legs: Vec<Vec<V2>> = vec![tail];
    let mut node = end_node;
    while let Some(e) = prev[node] {
        let r = &geo.roads[e];
        let (from, line) = if r.b == node {
            (r.a, r.pts.clone())
        } else {
            let mut p = r.pts.clone();
            p.reverse();
            (r.b, p)
        };
        legs.push(line);
        node = from;
    }
    // node is now ra.a or ra.b: the first leg is along a's road to it.
    let head = if node == ra.a {
        sub_line(&ra.pts, a.along, 0.0)
    } else {
        sub_line(&ra.pts, a.along, ra.length)
    };
    legs.push(head);
    legs.reverse();
    let mut out: Vec<V2> = Vec::new();
    for leg in legs {
        for p in leg {
            if out.last().is_none_or(|q| q.dist(p) > 1e-6) {
                out.push(p);
            }
        }
    }
    Some(out)
}

/// A route resolved over the roads: its line, and how far each of its
/// places sat from the road it snapped to.
pub struct Resolved {
    pub line: Vec<V2>,
    pub length: f64,
    /// (place id, metres off the road).
    pub snaps: Vec<(String, f64)>,
}

/// Resolves a route through its places, in order.
pub fn resolve(atlas: &crate::Atlas, f: &Filter, stops: &[&str]) -> Result<Resolved, String> {
    let mut snaps = Vec::new();
    for id in stops {
        let p = atlas.place(id)?;
        let s = snap(&atlas.geo, f, p)
            .ok_or_else(|| format!("no road the route may use near `{id}`"))?;
        snaps.push(((*id).to_owned(), s));
    }
    let mut line: Vec<V2> = Vec::new();
    for w in snaps.windows(2) {
        let leg = path(&atlas.geo, f, w[0].1, w[1].1).ok_or_else(|| {
            format!(
                "no way from `{}` to `{}` on the route's roads",
                w[0].0, w[1].0
            )
        })?;
        for p in leg {
            if line.last().is_none_or(|q| q.dist(p) > 1e-6) {
                line.push(p);
            }
        }
    }
    Ok(Resolved {
        length: polyline_length(&line),
        line,
        snaps: snaps.into_iter().map(|(id, s)| (id, s.off)).collect(),
    })
}
