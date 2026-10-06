//! Earcut and `ShapeUtils` (three.js r180 `src/extras/Earcut.js`, a copy of
//! mapbox/earcut v3.0.1 (ISC licence), and `src/extras/ShapeUtils.js`).
//!
//! The JS links nodes into circular lists by reference; here the nodes live
//! in an arena and link by index, and node identity (`p === a`) is index
//! equality. The arithmetic, the visiting order and the triangles are the
//! JS's.

use mp_math::js;

use super::math::Vector2;

const NIL: usize = usize::MAX;

#[derive(Clone, Copy, Debug)]
struct Node {
    /// vertex index in coordinates array
    i: usize,
    /// vertex coordinates
    x: f64,
    y: f64,
    /// previous and next vertex nodes in a polygon ring
    prev: usize,
    next: usize,
    /// z-order curve value
    z: i32,
    /// previous and next nodes in z-order
    prev_z: usize,
    next_z: usize,
    /// indicates whether this is a steiner point
    steiner: bool,
}

struct Earcut {
    nodes: Vec<Node>,
}

/// `Earcut.triangulate(data, holeIndices, dim)`: triangle vertex indices,
/// three per triangle.
pub fn earcut(data: &[f64], hole_indices: &[usize], dim: usize) -> Vec<usize> {
    let mut e = Earcut { nodes: Vec::new() };
    let has_holes = !hole_indices.is_empty();
    let outer_len = if has_holes {
        hole_indices[0] * dim
    } else {
        data.len()
    };
    let mut outer_node = e.linked_list(data, 0, outer_len, dim, true);
    let mut triangles = Vec::new();

    if outer_node == NIL || e.n(outer_node).next == e.n(outer_node).prev {
        return triangles;
    }

    let mut min_x = 0.0;
    let mut min_y = 0.0;
    // `invSize` is `undefined` for a simple shape; 0 stands for it (both are
    // falsy where it is tested).
    let mut inv_size = 0.0;

    if has_holes {
        outer_node = e.eliminate_holes(data, hole_indices, outer_node, dim);
    }

    // if the shape is not too simple, we'll use z-order curve hash later; calculate polygon bbox
    if data.len() > 80 * dim {
        min_x = f64::INFINITY;
        min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut i = dim;
        while i < outer_len {
            let x = data[i];
            let y = data[i + 1];
            if x < min_x {
                min_x = x;
            }
            if y < min_y {
                min_y = y;
            }
            if x > max_x {
                max_x = x;
            }
            if y > max_y {
                max_y = y;
            }
            i += dim;
        }
        // minX, minY and invSize are later used to transform coords into integers for z-order calculation
        inv_size = js::max(max_x - min_x, max_y - min_y);
        inv_size = if inv_size != 0.0 {
            32767.0 / inv_size
        } else {
            0.0
        };
    }

    e.earcut_linked(outer_node, &mut triangles, dim, min_x, min_y, inv_size, 0);
    triangles
}

/// JS truthiness of a number.
fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

impl Earcut {
    fn n(&self, k: usize) -> &Node {
        &self.nodes[k]
    }

    fn nm(&mut self, k: usize) -> &mut Node {
        &mut self.nodes[k]
    }

    // create a circular doubly linked list from polygon points in the specified winding order
    fn linked_list(
        &mut self,
        data: &[f64],
        start: usize,
        end: usize,
        dim: usize,
        clockwise: bool,
    ) -> usize {
        let mut last = NIL;
        if clockwise == (signed_area(data, start, end, dim) > 0.0) {
            let mut i = start;
            while i < end {
                last = self.insert_node(i / dim, data[i], data[i + 1], last);
                i += dim;
            }
        } else if end >= dim {
            let mut i = (end - dim) as i64;
            while i >= start as i64 {
                let iu = i as usize;
                last = self.insert_node(iu / dim, data[iu], data[iu + 1], last);
                i -= dim as i64;
            }
        }
        if last != NIL && self.equals(last, self.n(last).next) {
            self.remove_node(last);
            last = self.n(last).next;
        }
        last
    }

    // eliminate colinear or duplicate points
    fn filter_points(&mut self, start: usize, end: usize) -> usize {
        if start == NIL {
            return start;
        }
        let mut end = if end == NIL { start } else { end };
        let mut p = start;
        loop {
            let mut again = false;
            let pn = *self.n(p);
            if !pn.steiner
                && (self.equals(p, pn.next) || area(self.n(pn.prev), &pn, self.n(pn.next)) == 0.0)
            {
                self.remove_node(p);
                p = self.n(p).prev;
                end = p;
                if p == self.n(p).next {
                    break;
                }
                again = true;
            } else {
                p = pn.next;
            }
            if !(again || p != end) {
                break;
            }
        }
        end
    }

    // main ear slicing loop which triangulates a polygon (given as a linked list)
    #[allow(clippy::too_many_arguments)]
    fn earcut_linked(
        &mut self,
        ear: usize,
        triangles: &mut Vec<usize>,
        dim: usize,
        min_x: f64,
        min_y: f64,
        inv_size: f64,
        pass: u32,
    ) {
        if ear == NIL {
            return;
        }
        let mut ear = ear;

        // interlink polygon nodes in z-order
        if pass == 0 && truthy(inv_size) {
            self.index_curve(ear, min_x, min_y, inv_size);
        }

        let mut stop = ear;

        // iterate through ears, slicing them one by one
        while self.n(ear).prev != self.n(ear).next {
            let prev = self.n(ear).prev;
            let next = self.n(ear).next;

            let is_ear = if truthy(inv_size) {
                self.is_ear_hashed(ear, min_x, min_y, inv_size)
            } else {
                self.is_ear(ear)
            };
            if is_ear {
                triangles.push(self.n(prev).i);
                triangles.push(self.n(ear).i);
                triangles.push(self.n(next).i); // cut off the triangle
                self.remove_node(ear);
                // skipping the next vertex leads to less sliver triangles
                ear = self.n(next).next;
                stop = self.n(next).next;
                continue;
            }

            ear = next;

            // if we looped through the whole remaining polygon and can't find any more ears
            if ear == stop {
                // try filtering points and slicing again
                if pass == 0 {
                    let f = self.filter_points(ear, NIL);
                    self.earcut_linked(f, triangles, dim, min_x, min_y, inv_size, 1);
                // if this didn't work, try curing all small self-intersections locally
                } else if pass == 1 {
                    let f = self.filter_points(ear, NIL);
                    let ear = self.cure_local_intersections(f, triangles);
                    self.earcut_linked(ear, triangles, dim, min_x, min_y, inv_size, 2);
                // as a last resort, try splitting the remaining polygon into two
                } else if pass == 2 {
                    self.split_earcut(ear, triangles, dim, min_x, min_y, inv_size);
                }
                break;
            }
        }
    }

    // check whether a polygon node forms a valid ear with adjacent nodes
    fn is_ear(&self, ear: usize) -> bool {
        let b = *self.n(ear);
        let a = *self.n(b.prev);
        let c = *self.n(b.next);

        if area(&a, &b, &c) >= 0.0 {
            return false; // reflex, can't be an ear
        }

        // now make sure we don't have other points inside the potential ear
        let (ax, bx, cx, ay, by, cy) = (a.x, b.x, c.x, a.y, b.y, c.y);

        // triangle bbox
        let x0 = js::min_n(&[ax, bx, cx]);
        let y0 = js::min_n(&[ay, by, cy]);
        let x1 = js::max_n(&[ax, bx, cx]);
        let y1 = js::max_n(&[ay, by, cy]);

        let a_idx = b.prev;
        let mut p = c.next;
        while p != a_idx {
            let pn = self.n(p);
            if pn.x >= x0
                && pn.x <= x1
                && pn.y >= y0
                && pn.y <= y1
                && point_in_triangle_except_first(ax, ay, bx, by, cx, cy, pn.x, pn.y)
                && area(self.n(pn.prev), pn, self.n(pn.next)) >= 0.0
            {
                return false;
            }
            p = pn.next;
        }
        true
    }

    fn is_ear_hashed(&self, ear: usize, min_x: f64, min_y: f64, inv_size: f64) -> bool {
        let b = *self.n(ear);
        let a_idx = b.prev;
        let c_idx = b.next;
        let a = *self.n(a_idx);
        let c = *self.n(c_idx);

        if area(&a, &b, &c) >= 0.0 {
            return false; // reflex, can't be an ear
        }

        let (ax, bx, cx, ay, by, cy) = (a.x, b.x, c.x, a.y, b.y, c.y);

        // triangle bbox
        let x0 = js::min_n(&[ax, bx, cx]);
        let y0 = js::min_n(&[ay, by, cy]);
        let x1 = js::max_n(&[ax, bx, cx]);
        let y1 = js::max_n(&[ay, by, cy]);

        // z-order range for the current triangle bbox;
        let min_z = z_order(x0, y0, min_x, min_y, inv_size);
        let max_z = z_order(x1, y1, min_x, min_y, inv_size);

        let inside = |k: usize| -> bool {
            let q = self.n(k);
            q.x >= x0
                && q.x <= x1
                && q.y >= y0
                && q.y <= y1
                && k != a_idx
                && k != c_idx
                && point_in_triangle_except_first(ax, ay, bx, by, cx, cy, q.x, q.y)
                && area(self.n(q.prev), q, self.n(q.next)) >= 0.0
        };

        let mut p = b.prev_z;
        let mut n = b.next_z;

        // look for points inside the triangle in both directions
        while p != NIL && self.n(p).z >= min_z && n != NIL && self.n(n).z <= max_z {
            if inside(p) {
                return false;
            }
            p = self.n(p).prev_z;
            if inside(n) {
                return false;
            }
            n = self.n(n).next_z;
        }

        // look for remaining points in decreasing z-order
        while p != NIL && self.n(p).z >= min_z {
            if inside(p) {
                return false;
            }
            p = self.n(p).prev_z;
        }

        // look for remaining points in increasing z-order
        while n != NIL && self.n(n).z <= max_z {
            if inside(n) {
                return false;
            }
            n = self.n(n).next_z;
        }
        true
    }

    // go through all polygon nodes and cure small local self-intersections
    fn cure_local_intersections(&mut self, start: usize, triangles: &mut Vec<usize>) -> usize {
        let mut start = start;
        let mut p = start;
        loop {
            let a = self.n(p).prev;
            let b = self.n(self.n(p).next).next;
            if !self.equals(a, b)
                && intersects(self.n(a), self.n(p), self.n(self.n(p).next), self.n(b))
                && self.locally_inside(a, b)
                && self.locally_inside(b, a)
            {
                triangles.push(self.n(a).i);
                triangles.push(self.n(p).i);
                triangles.push(self.n(b).i);
                // remove two nodes involved
                self.remove_node(p);
                let pn = self.n(p).next;
                self.remove_node(pn);
                p = b;
                start = b;
            }
            p = self.n(p).next;
            if p == start {
                break;
            }
        }
        self.filter_points(p, NIL)
    }

    // try splitting polygon into two and triangulate them independently
    fn split_earcut(
        &mut self,
        start: usize,
        triangles: &mut Vec<usize>,
        dim: usize,
        min_x: f64,
        min_y: f64,
        inv_size: f64,
    ) {
        // look for a valid diagonal that divides the polygon into two
        let mut a = start;
        loop {
            let mut b = self.n(self.n(a).next).next;
            while b != self.n(a).prev {
                if self.n(a).i != self.n(b).i && self.is_valid_diagonal(a, b) {
                    // split the polygon in two by the diagonal
                    let mut c = self.split_polygon(a, b);
                    // filter colinear points around the cuts
                    let an = self.n(a).next;
                    a = self.filter_points(a, an);
                    let cn = self.n(c).next;
                    c = self.filter_points(c, cn);
                    // run earcut on each half
                    self.earcut_linked(a, triangles, dim, min_x, min_y, inv_size, 0);
                    self.earcut_linked(c, triangles, dim, min_x, min_y, inv_size, 0);
                    return;
                }
                b = self.n(b).next;
            }
            a = self.n(a).next;
            if a == start {
                break;
            }
        }
    }

    // link every hole into the outer loop, producing a single-ring polygon without holes
    fn eliminate_holes(
        &mut self,
        data: &[f64],
        hole_indices: &[usize],
        outer_node: usize,
        dim: usize,
    ) -> usize {
        let mut queue = Vec::new();
        let len = hole_indices.len();
        for i in 0..len {
            let start = hole_indices[i] * dim;
            let end = if i < len - 1 {
                hole_indices[i + 1] * dim
            } else {
                data.len()
            };
            let list = self.linked_list(data, start, end, dim, false);
            if list == self.n(list).next {
                self.nm(list).steiner = true;
            }
            queue.push(self.get_leftmost(list));
        }

        // Array.prototype.sort is stable; a NaN comparison counts as equal.
        queue.sort_by(|&a, &b| {
            let r = self.compare_xy_slope(a, b);
            if r < 0.0 {
                core::cmp::Ordering::Less
            } else if r > 0.0 {
                core::cmp::Ordering::Greater
            } else {
                core::cmp::Ordering::Equal
            }
        });

        // process holes from left to right
        let mut outer_node = outer_node;
        for &q in &queue {
            outer_node = self.eliminate_hole(q, outer_node);
        }
        outer_node
    }

    fn compare_xy_slope(&self, a: usize, b: usize) -> f64 {
        let (a, b) = (self.n(a), self.n(b));
        let mut result = a.x - b.x;
        // when the left-most point of 2 holes meet at a vertex, sort the holes counterclockwise so that when we find
        // the bridge to the outer shell is always the point that they meet at.
        if result == 0.0 {
            result = a.y - b.y;
            if result == 0.0 {
                let an = self.n(a.next);
                let bn = self.n(b.next);
                let a_slope = (an.y - a.y) / (an.x - a.x);
                let b_slope = (bn.y - b.y) / (bn.x - b.x);
                result = a_slope - b_slope;
            }
        }
        result
    }

    // find a bridge between vertices that connects hole with an outer ring and and link it
    fn eliminate_hole(&mut self, hole: usize, outer_node: usize) -> usize {
        let bridge = self.find_hole_bridge(hole, outer_node);
        if bridge == NIL {
            return outer_node;
        }
        let bridge_reverse = self.split_polygon(bridge, hole);
        // filter collinear points around the cuts
        let brn = self.n(bridge_reverse).next;
        self.filter_points(bridge_reverse, brn);
        let bn = self.n(bridge).next;
        self.filter_points(bridge, bn)
    }

    // David Eberly's algorithm for finding a bridge between hole and outer polygon
    fn find_hole_bridge(&self, hole: usize, outer_node: usize) -> usize {
        let mut p = outer_node;
        let hx = self.n(hole).x;
        let hy = self.n(hole).y;
        let mut qx = f64::NEG_INFINITY;
        let mut m = NIL;

        // find a segment intersected by a ray from the hole's leftmost point to the left;
        // segment's endpoint with lesser x will be potential connection point
        // unless they intersect at a vertex, then choose the vertex
        if self.equals(hole, p) {
            return p;
        }
        loop {
            let pn = self.n(p);
            let nx = self.n(pn.next);
            if self.equals(hole, pn.next) {
                return pn.next;
            } else if hy <= pn.y && hy >= nx.y && nx.y != pn.y {
                let x = pn.x + (hy - pn.y) * (nx.x - pn.x) / (nx.y - pn.y);
                if x <= hx && x > qx {
                    qx = x;
                    m = if pn.x < nx.x { p } else { pn.next };
                    if x == hx {
                        return m; // hole touches outer segment; pick leftmost endpoint
                    }
                }
            }
            p = pn.next;
            if p == outer_node {
                break;
            }
        }

        if m == NIL {
            return NIL;
        }

        // look for points inside the triangle of hole point, segment intersection and endpoint;
        // if there are no points found, we have a valid connection;
        // otherwise choose the point of the minimum angle with the ray as connection point

        let stop = m;
        let mx = self.n(m).x;
        let my = self.n(m).y;
        let mut tan_min = f64::INFINITY;

        p = m;

        loop {
            let pn = self.n(p);
            if hx >= pn.x
                && pn.x >= mx
                && hx != pn.x
                && point_in_triangle(
                    if hy < my { hx } else { qx },
                    hy,
                    mx,
                    my,
                    if hy < my { qx } else { hx },
                    hy,
                    pn.x,
                    pn.y,
                )
            {
                let tan = (hy - pn.y).abs() / (hx - pn.x); // tangential
                let mm = self.n(m);
                if self.locally_inside(p, hole)
                    && (tan < tan_min
                        || (tan == tan_min
                            && (pn.x > mm.x
                                || (pn.x == mm.x && self.sector_contains_sector(m, p)))))
                {
                    m = p;
                    tan_min = tan;
                }
            }
            p = pn.next;
            if p == stop {
                break;
            }
        }
        m
    }

    // whether sector in vertex m contains sector in vertex p in the same coordinates
    fn sector_contains_sector(&self, m: usize, p: usize) -> bool {
        let (mn, pn) = (self.n(m), self.n(p));
        area(self.n(mn.prev), mn, self.n(pn.prev)) < 0.0
            && area(self.n(pn.next), mn, self.n(mn.next)) < 0.0
    }

    // interlink polygon nodes in z-order
    fn index_curve(&mut self, start: usize, min_x: f64, min_y: f64, inv_size: f64) {
        let mut p = start;
        loop {
            let pn = *self.n(p);
            if pn.z == 0 {
                self.nm(p).z = z_order(pn.x, pn.y, min_x, min_y, inv_size);
            }
            self.nm(p).prev_z = pn.prev;
            self.nm(p).next_z = pn.next;
            p = pn.next;
            if p == start {
                break;
            }
        }
        let ppz = self.n(p).prev_z;
        self.nm(ppz).next_z = NIL;
        self.nm(p).prev_z = NIL;
        self.sort_linked(p);
    }

    // Simon Tatham's linked list merge sort algorithm
    // http://www.chiark.greenend.org.uk/~sgtatham/algorithms/listsort.html
    fn sort_linked(&mut self, list: usize) -> usize {
        let mut list = list;
        let mut in_size = 1;
        loop {
            let mut p = list;
            list = NIL;
            let mut tail = NIL;
            let mut num_merges = 0;

            while p != NIL {
                num_merges += 1;
                let mut q = p;
                let mut p_size = 0;
                for _ in 0..in_size {
                    p_size += 1;
                    q = self.n(q).next_z;
                    if q == NIL {
                        break;
                    }
                }
                let mut q_size = in_size;

                while p_size > 0 || (q_size > 0 && q != NIL) {
                    let e;
                    if p_size != 0 && (q_size == 0 || q == NIL || self.n(p).z <= self.n(q).z) {
                        e = p;
                        p = self.n(p).next_z;
                        p_size -= 1;
                    } else {
                        e = q;
                        q = self.n(q).next_z;
                        q_size -= 1;
                    }

                    if tail != NIL {
                        self.nm(tail).next_z = e;
                    } else {
                        list = e;
                    }

                    self.nm(e).prev_z = tail;
                    tail = e;
                }

                p = q;
            }

            self.nm(tail).next_z = NIL;
            in_size *= 2;
            if num_merges <= 1 {
                break;
            }
        }
        list
    }

    // find the leftmost node of a polygon ring
    fn get_leftmost(&self, start: usize) -> usize {
        let mut p = start;
        let mut leftmost = start;
        loop {
            let (pn, ln) = (self.n(p), self.n(leftmost));
            if pn.x < ln.x || (pn.x == ln.x && pn.y < ln.y) {
                leftmost = p;
            }
            p = pn.next;
            if p == start {
                break;
            }
        }
        leftmost
    }

    // check if a diagonal between two polygon nodes is valid (lies in polygon interior)
    fn is_valid_diagonal(&self, a: usize, b: usize) -> bool {
        let (an, bn) = (self.n(a), self.n(b));
        self.n(an.next).i != bn.i
            && self.n(an.prev).i != bn.i
            && !self.intersects_polygon(a, b) // doesn't intersect other edges
            && ((self.locally_inside(a, b) && self.locally_inside(b, a) && self.middle_inside(a, b) // locally visible
                && (truthy(area(self.n(an.prev), an, self.n(bn.prev))) || truthy(area(an, self.n(bn.prev), bn)))) // does not create opposite-facing sectors
                || (self.equals(a, b)
                    && area(self.n(an.prev), an, self.n(an.next)) > 0.0
                    && area(self.n(bn.prev), bn, self.n(bn.next)) > 0.0)) // special zero-length case
    }

    // check if two points are equal
    fn equals(&self, p1: usize, p2: usize) -> bool {
        let (a, b) = (self.n(p1), self.n(p2));
        a.x == b.x && a.y == b.y
    }

    // check if a polygon diagonal intersects any polygon segments
    fn intersects_polygon(&self, a: usize, b: usize) -> bool {
        let (ai, bi) = (self.n(a).i, self.n(b).i);
        let mut p = a;
        loop {
            let pn = self.n(p);
            let nn = self.n(pn.next);
            if pn.i != ai
                && nn.i != ai
                && pn.i != bi
                && nn.i != bi
                && intersects(pn, nn, self.n(a), self.n(b))
            {
                return true;
            }
            p = pn.next;
            if p == a {
                break;
            }
        }
        false
    }

    // check if a polygon diagonal is locally inside the polygon
    fn locally_inside(&self, a: usize, b: usize) -> bool {
        let (an, bn) = (self.n(a), self.n(b));
        let (ap, ax) = (self.n(an.prev), self.n(an.next));
        if area(ap, an, ax) < 0.0 {
            area(an, bn, ax) >= 0.0 && area(an, ap, bn) >= 0.0
        } else {
            area(an, bn, ap) < 0.0 || area(an, ax, bn) < 0.0
        }
    }

    // check if the middle point of a polygon diagonal is inside the polygon
    fn middle_inside(&self, a: usize, b: usize) -> bool {
        let (an, bn) = (self.n(a), self.n(b));
        let mut p = a;
        let mut inside = false;
        let px = (an.x + bn.x) / 2.0;
        let py = (an.y + bn.y) / 2.0;
        loop {
            let pn = self.n(p);
            let nn = self.n(pn.next);
            if ((pn.y > py) != (nn.y > py))
                && nn.y != pn.y
                && (px < (nn.x - pn.x) * (py - pn.y) / (nn.y - pn.y) + pn.x)
            {
                inside = !inside;
            }
            p = pn.next;
            if p == a {
                break;
            }
        }
        inside
    }

    // link two polygon vertices with a bridge; if the vertices belong to the same ring, it splits polygon into two;
    // if one belongs to the outer ring and another to a hole, it merges it into a single ring
    fn split_polygon(&mut self, a: usize, b: usize) -> usize {
        let (an_i, an_x, an_y) = (self.n(a).i, self.n(a).x, self.n(a).y);
        let (bn_i, bn_x, bn_y) = (self.n(b).i, self.n(b).x, self.n(b).y);
        let a2 = self.create_node(an_i, an_x, an_y);
        let b2 = self.create_node(bn_i, bn_x, bn_y);
        let an = self.n(a).next;
        let bp = self.n(b).prev;

        self.nm(a).next = b;
        self.nm(b).prev = a;

        self.nm(a2).next = an;
        self.nm(an).prev = a2;

        self.nm(b2).next = a2;
        self.nm(a2).prev = b2;

        self.nm(bp).next = b2;
        self.nm(b2).prev = bp;

        b2
    }

    // create a node and optionally link it with previous one (in a circular doubly linked list)
    fn insert_node(&mut self, i: usize, x: f64, y: f64, last: usize) -> usize {
        let p = self.create_node(i, x, y);
        if last == NIL {
            self.nm(p).prev = p;
            self.nm(p).next = p;
        } else {
            let ln = self.n(last).next;
            self.nm(p).next = ln;
            self.nm(p).prev = last;
            self.nm(ln).prev = p;
            self.nm(last).next = p;
        }
        p
    }

    fn remove_node(&mut self, p: usize) {
        let pn = *self.n(p);
        self.nm(pn.next).prev = pn.prev;
        self.nm(pn.prev).next = pn.next;
        if pn.prev_z != NIL {
            self.nm(pn.prev_z).next_z = pn.next_z;
        }
        if pn.next_z != NIL {
            self.nm(pn.next_z).prev_z = pn.prev_z;
        }
    }

    fn create_node(&mut self, i: usize, x: f64, y: f64) -> usize {
        self.nodes.push(Node {
            i,
            x,
            y,
            prev: NIL,
            next: NIL,
            z: 0,
            prev_z: NIL,
            next_z: NIL,
            steiner: false,
        });
        self.nodes.len() - 1
    }
}

// z-order of a point given coords and inverse of the longer side of data bbox
fn z_order(x: f64, y: f64, min_x: f64, min_y: f64, inv_size: f64) -> i32 {
    // coords are transformed into non-negative 15-bit integer range
    let mut x = js::to_int32((x - min_x) * inv_size);
    let mut y = js::to_int32((y - min_y) * inv_size);

    x = (x | (x << 8)) & 0x00FF00FF;
    x = (x | (x << 4)) & 0x0F0F0F0F;
    x = (x | (x << 2)) & 0x33333333;
    x = (x | (x << 1)) & 0x55555555;

    y = (y | (y << 8)) & 0x00FF00FF;
    y = (y | (y << 4)) & 0x0F0F0F0F;
    y = (y | (y << 2)) & 0x33333333;
    y = (y | (y << 1)) & 0x55555555;

    x | (y << 1)
}

// check if a point lies within a convex triangle
#[allow(clippy::too_many_arguments)]
fn point_in_triangle(
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    cx: f64,
    cy: f64,
    px: f64,
    py: f64,
) -> bool {
    (cx - px) * (ay - py) >= (ax - px) * (cy - py)
        && (ax - px) * (by - py) >= (bx - px) * (ay - py)
        && (bx - px) * (cy - py) >= (cx - px) * (by - py)
}

// check if a point lies within a convex triangle but false if its equal to the first point of the triangle
#[allow(clippy::too_many_arguments)]
fn point_in_triangle_except_first(
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    cx: f64,
    cy: f64,
    px: f64,
    py: f64,
) -> bool {
    !(ax == px && ay == py) && point_in_triangle(ax, ay, bx, by, cx, cy, px, py)
}

// signed area of a triangle
fn area(p: &Node, q: &Node, r: &Node) -> f64 {
    (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
}

// check if two segments intersect
fn intersects(p1: &Node, q1: &Node, p2: &Node, q2: &Node) -> bool {
    let o1 = sign(area(p1, q1, p2));
    let o2 = sign(area(p1, q1, q2));
    let o3 = sign(area(p2, q2, p1));
    let o4 = sign(area(p2, q2, q1));

    if o1 != o2 && o3 != o4 {
        return true; // general case
    }

    if o1 == 0 && on_segment(p1, p2, q1) {
        return true; // p1, q1 and p2 are collinear and p2 lies on p1q1
    }
    if o2 == 0 && on_segment(p1, q2, q1) {
        return true; // p1, q1 and q2 are collinear and q2 lies on p1q1
    }
    if o3 == 0 && on_segment(p2, p1, q2) {
        return true; // p2, q2 and p1 are collinear and p1 lies on p2q2
    }
    if o4 == 0 && on_segment(p2, q1, q2) {
        return true; // p2, q2 and q1 are collinear and q1 lies on p2q2
    }
    false
}

// for collinear points p, q, r, check if point q lies on segment pr
fn on_segment(p: &Node, q: &Node, r: &Node) -> bool {
    q.x <= js::max(p.x, r.x)
        && q.x >= js::min(p.x, r.x)
        && q.y <= js::max(p.y, r.y)
        && q.y >= js::min(p.y, r.y)
}

fn sign(num: f64) -> i32 {
    if num > 0.0 {
        1
    } else if num < 0.0 {
        -1
    } else {
        0
    }
}

fn signed_area(data: &[f64], start: usize, end: usize, dim: usize) -> f64 {
    let mut sum = 0.0;
    if end < dim {
        return sum;
    }
    let mut i = start;
    let mut j = end - dim;
    while i < end {
        sum += (data[j] - data[i]) * (data[i + 1] + data[j + 1]);
        j = i;
        i += dim;
    }
    sum
}

// ── ShapeUtils ──────────────────────────────────────────────────────────

/// `ShapeUtils.area(contour)`: the signed area (positive counter-clockwise).
pub fn shape_area(contour: &[Vector2]) -> f64 {
    let n = contour.len();
    let mut a = 0.0;
    if n == 0 {
        return a * 0.5;
    }
    let mut p = n - 1;
    for q in 0..n {
        a += contour[p].x * contour[q].y - contour[q].x * contour[p].y;
        p = q;
    }
    a * 0.5
}

/// `ShapeUtils.isClockWise(pts)`.
pub fn is_clock_wise(pts: &[Vector2]) -> bool {
    shape_area(pts) < 0.0
}

/// `ShapeUtils.triangulateShape(contour, holes)`: faces as index triples
/// into the contour followed by the holes. As in three, a last point equal
/// to the first is removed from the contour and from each hole **in
/// place**; callers that go on to use them see the change.
pub fn triangulate_shape(
    contour: &mut Vec<Vector2>,
    holes: &mut [Vec<Vector2>],
) -> Vec<[usize; 3]> {
    let mut vertices = Vec::new(); // flat array of vertices like [ x0,y0, x1,y1, x2,y2, ... ]
    let mut hole_indices = Vec::new(); // array of hole indices
    let mut faces = Vec::new(); // final array of vertex indices like [ [ a,b,d ], [ b,c,d ] ]

    remove_dup_end_pts(contour);
    add_contour(&mut vertices, contour);

    let mut hole_index = contour.len();
    for h in holes.iter_mut() {
        remove_dup_end_pts(h);
    }
    for h in holes.iter() {
        hole_indices.push(hole_index);
        hole_index += h.len();
        add_contour(&mut vertices, h);
    }

    let triangles = earcut(&vertices, &hole_indices, 2);
    for t in triangles.chunks(3) {
        faces.push([t[0], t[1], t[2]]);
    }
    faces
}

fn remove_dup_end_pts(points: &mut Vec<Vector2>) {
    let l = points.len();
    if l > 2 && points[l - 1].equals(points[0]) {
        points.pop();
    }
}

fn add_contour(vertices: &mut Vec<f64>, contour: &[Vector2]) {
    for p in contour {
        vertices.push(p.x);
        vertices.push(p.y);
    }
}
