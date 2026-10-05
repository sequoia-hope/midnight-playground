// The HUD's drawn parts (HUD.js's canvases and #speedlines), one UI
// material for all three so the pipeline is built once (play/hud/material.rs):
//   kind 1: the rev counter or power meter (`drawTach`, `drawPower`) on its
//           260 px canvas: backplate, track, redline or regen zone, fill,
//           ticks. The labels and the needle are UI nodes above it.
//   kind 2: the minimap (`drawMinimap`, `drawPolice`) on its 220 px canvas:
//           the road stroked twice, dots and bars, clipped to the circle,
//           and the player's arrow.
//   kind 3: the speed lines: `repeating-conic-gradient(from 0deg at 50% 55%,
//           transparent 0 3deg, rgba(255,255,255,.07) 3deg 3.4deg)` masked
//           by `radial-gradient(circle at 50% 55%, transparent 32%, #000 75%)`.
// Shapes are signed distances, antialiased over a pixel as the canvas
// does; colours are composited in sRGB as the canvas composites them, and
// the result is handed to the UI pass in linear.

#import bevy_ui::ui_vertex_output::UiVertexOutput

struct HudParams {
    head: vec4<f32>,
    v: array<vec4<f32>, 8>,
    road: array<vec4<f32>, 128>,
    shapes: array<vec4<f32>, 256>,
};

@group(1) @binding(0)
var<uniform> p: HudParams;

const TAU: f32 = 6.283185307179586;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// Coverage of a shape at signed distance d (negative inside), aa canvas
// units to the pixel.
fn cov(d: f32, aa: f32) -> f32 {
    return clamp(0.5 - d / aa, 0.0, 1.0);
}

// Source over, premultiplied.
fn over(acc: vec4<f32>, rgb: vec3<f32>, a: f32) -> vec4<f32> {
    return vec4<f32>(rgb * a + acc.rgb * (1.0 - a), a + acc.a * (1.0 - a));
}

// The page composites in sRGB, the UI pass in linear: the alpha that lands
// a translucent colour where the sRGB blend would over a typical
// background (play/hud.rs `lin_alpha`, D823).
fn lin1(v: f32) -> f32 {
    return select(pow((v + 0.055) / 1.055, 2.4), v / 12.92, v <= 0.04045);
}

fn lin_alpha(l: f32, a: f32) -> f32 {
    if (a <= 0.0 || a >= 1.0) {
        return a;
    }
    let b = 0.25;
    let t = lin1((1.0 - a) * b + a * l);
    let bl = lin1(b);
    let cl = lin1(l);
    if (abs(cl - bl) < 0.0001) {
        return a;
    }
    return clamp((t - bl) / (cl - bl), 0.0, 1.0);
}

fn done(acc: vec4<f32>) -> vec4<f32> {
    if (acc.a <= 0.0001) {
        return vec4<f32>(0.0);
    }
    let c = acc.rgb / acc.a;
    let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec4<f32>(to_linear(c), lin_alpha(l, acc.a));
}

// A box from `a` to `b` of half width `hw` (butt ends).
fn sd_bar(q: vec2<f32>, a: vec2<f32>, b: vec2<f32>, hw: f32) -> f32 {
    let e = b - a;
    let len = max(length(e), 0.0001);
    let dir = e / len;
    let w = q - (a + b) * 0.5;
    let local = abs(vec2<f32>(dot(w, dir), dot(w, vec2<f32>(-dir.y, dir.x)))) - vec2<f32>(len * 0.5, hw);
    return length(max(local, vec2<f32>(0.0))) + min(max(local.x, local.y), 0.0);
}

fn sd_segment(q: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let e = b - a;
    let w = q - a;
    let h = clamp(dot(w, e) / max(dot(e, e), 0.000001), 0.0, 1.0);
    return length(w - e * h);
}

// The signed distance along an arc's ends: negative inside the angles
// a..b (clockwise), in canvas units at radius r.
fn arc_ends(phi: f32, r: f32, a: f32, b: f32) -> f32 {
    let span = b - a;
    if (span <= 0.00001) {
        return 1000.0;
    }
    let u = (phi - a) - floor((phi - a) / TAU) * TAU;
    if (u <= span) {
        return -min(u, span - u) * r;
    }
    return min(u - span, TAU - u) * r;
}

// Coverage of a 10 px stroke of radius 112 from a to b, butt caps.
fn arc(phi: f32, r: f32, a: f32, b: f32, aa: f32) -> f32 {
    return cov(abs(r - 112.0) - 5.0, aa) * cov(arc_ends(phi, r, a, b), aa);
}

fn dial(q: vec2<f32>, aa: f32) -> vec4<f32> {
    let d = q - vec2<f32>(130.0, 130.0);
    let r = length(d);
    let phi = atan2(d.y, d.x);
    let a0 = 0.75 * 3.141592653589793;
    let a1 = 2.25 * 3.141592653589793;
    var acc = vec4<f32>(0.0);
    // Backplate.
    acc = over(acc, vec3<f32>(8.0, 10.0, 18.0) / 255.0, 0.5 * cov(r - 122.0, aa));
    // Track.
    acc = over(acc, vec3<f32>(1.0), 0.12 * arc(phi, r, a0, a1, aa));
    // Redline or regen zone: v[1] = (from, to, _, alpha), v[2] = colour.
    let zone = p.v[1];
    acc = over(acc, p.v[2].rgb, zone.w * arc(phi, r, zone.x, zone.y, aa));
    // Fill: v[0] = (from, to, solid, mid stop); v[3..5] the stops.
    let fill = p.v[0];
    var col = p.v[3].rgb;
    if (fill.z < 0.5) {
        let t = clamp((q.x - q.y + 260.0) / 520.0, 0.0, 1.0);
        if (t <= fill.w) {
            col = mix(p.v[3].rgb, p.v[4].rgb, t / fill.w);
        } else {
            col = mix(p.v[4].rgb, p.v[5].rgb, (t - fill.w) / (1.0 - fill.w));
        }
    }
    acc = over(acc, col, arc(phi, r, fill.x, fill.y, aa));
    // Ticks: v[6] = (first, step, count): from R - 9 to R - 14, 2 px.
    let ticks = p.v[6];
    for (var k = 0; k < i32(ticks.z); k = k + 1) {
        let a = ticks.x + ticks.y * f32(k);
        let dir = vec2<f32>(cos(a), sin(a));
        let sd = sd_bar(d, dir * 98.0, dir * 103.0, 1.0);
        acc = over(acc, vec3<f32>(1.0), 0.5 * cov(sd, aa));
    }
    return acc;
}

fn road_pt(i: i32) -> vec2<f32> {
    let v = p.road[i / 2];
    if ((i & 1) == 0) {
        return v.xy;
    }
    return v.zw;
}

// One edge of iq's polygon distance: returns the squared distance to the
// edge and whether the crossing test flips the sign.
fn poly_edge(q: vec2<f32>, vi: vec2<f32>, vj: vec2<f32>, d: ptr<function, f32>, s: ptr<function, f32>) {
    let e = vj - vi;
    let w = q - vi;
    let b = w - e * clamp(dot(w, e) / dot(e, e), 0.0, 1.0);
    *d = min(*d, dot(b, b));
    let c1 = q.y >= vi.y;
    let c2 = q.y < vj.y;
    let c3 = e.x * w.y > e.y * w.x;
    if ((c1 && c2 && c3) || (!c1 && !c2 && !c3)) {
        *s = -*s;
    }
}

fn minimap(q: vec2<f32>, aa: f32) -> vec4<f32> {
    let clip = cov(length(q - vec2<f32>(110.0)) - 108.0, aa);
    var acc = vec4<f32>(0.0);
    // The road, s - 500 to s + 900, 26 and 14 m wide (0.28 px a metre).
    let n = i32(p.head.y);
    if (n > 1 && clip > 0.0) {
        var dmin = 1e9;
        var a = road_pt(0);
        for (var i = 1; i < n; i = i + 1) {
            let b = road_pt(i);
            dmin = min(dmin, sd_segment(q, a, b));
            a = b;
        }
        acc = over(acc, vec3<f32>(0.0), 0.5 * clip * cov(dmin - 3.64, aa));
        acc = over(acc, vec3<f32>(230.0, 236.0, 255.0) / 255.0, 0.85 * clip * cov(dmin - 1.96, aa));
    }
    // Dots and bars, in the canvas's order.
    let m = i32(p.head.z);
    for (var i = 0; i < m; i = i + 1) {
        let g = p.shapes[i * 4];
        let s = p.shapes[i * 4 + 1];
        let f = p.shapes[i * 4 + 2];
        let k = p.shapes[i * 4 + 3];
        if (s.x < 0.5) {
            let dist = length(q - g.xy);
            acc = over(acc, f.rgb, f.a * clip * cov(dist - g.z, aa));
            if (s.y > 0.0) {
                acc = over(acc, k.rgb, k.a * clip * cov(abs(dist - g.z) - s.y * 0.5, aa));
            }
        } else {
            acc = over(acc, f.rgb, f.a * clip * cov(sd_bar(q, g.xy, g.zw, s.y * 0.5), aa));
        }
    }
    // The player's arrow, screen space, always pointing up.
    let o = q - vec2<f32>(110.0, 140.0);
    var d2 = dot(o - vec2<f32>(0.0, -9.0), o - vec2<f32>(0.0, -9.0));
    var sg = 1.0;
    poly_edge(o, vec2<f32>(0.0, -9.0), vec2<f32>(-6.5, 7.0), &d2, &sg);
    poly_edge(o, vec2<f32>(6.5, 7.0), vec2<f32>(0.0, -9.0), &d2, &sg);
    poly_edge(o, vec2<f32>(0.0, 3.5), vec2<f32>(6.5, 7.0), &d2, &sg);
    poly_edge(o, vec2<f32>(-6.5, 7.0), vec2<f32>(0.0, 3.5), &d2, &sg);
    let sd = sg * sqrt(d2);
    acc = over(acc, vec3<f32>(1.0), cov(sd, aa));
    acc = over(acc, vec3<f32>(0.0), cov(abs(sd) - 1.0, aa));
    return acc;
}

fn speedlines(uv: vec2<f32>, size: vec2<f32>) -> vec4<f32> {
    let c = size * vec2<f32>(0.5, 0.55);
    let d = uv * size - c;
    // Degrees clockwise from up.
    var ang = degrees(atan2(d.x, -d.y));
    ang = ang - floor(ang / 360.0) * 360.0;
    let m = ang - floor(ang / 3.4) * 3.4;
    let fw = max(fwidth(ang), 0.0001);
    let band = clamp(min(m - 3.0, 3.4 - m) / fw + 0.5, 0.0, 1.0);
    let far = length(max(c, size - c));
    let mask = clamp((length(d) / far - 0.32) / (0.75 - 0.32), 0.0, 1.0);
    let a = 0.07 * band * mask * p.head.y;
    return vec4<f32>(1.0, 1.0, 1.0, lin_alpha(1.0, a));
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let kind = i32(p.head.x + 0.5);
    if (kind == 3) {
        return speedlines(in.uv, in.size);
    }
    let canvas = select(220.0, 260.0, kind == 1);
    let q = in.uv * canvas;
    let aa = max(fwidth(q.x), 0.0001);
    if (kind == 1) {
        return done(dial(q, aa));
    }
    return done(minimap(q, aa));
}
