// The sky (`src/world/Sky.js`, skyFrag): gradient with a sun-side glow and
// horizon haze matched to the fog, domain-warped cumulus lit toward the sun,
// cirrus streaks, the sun disc, the moon with maria, twinkling stars and a
// faint Milky Way. Ported line for line; shared by the dome (SkyDome kind)
// and the environment map's sky pass (`render::pmrem`), as main.js builds
// its environment from the dome itself.
//
// The one change of form: WGSL wants texture samples with implicit
// derivatives in uniform control flow, so the five noise lookups the GLSL
// makes inside its `if` blocks are taken first, unconditionally, and used
// where the GLSL used them.

#define_import_path mr::sky

#import mr::three_std::smooth_step
#import mr::three_globals as g

struct SkyUniforms {
    zenith: vec3<f32>,
    horizon: vec3<f32>,
    ground: vec3<f32>,
    sun_color: vec3<f32>,
    sun_dir: vec3<f32>,
    moon_dir: vec3<f32>,
    night: f32,
    time: f32,
    cloud: f32,
    haze: f32,
};

fn sky_uniforms(globals: texture_2d<f32>) -> SkyUniforms {
    var u: SkyUniforms;
    u.zenith = textureLoad(globals, vec2<i32>(g::G_SKY_ZENITH, 0), 0).rgb;
    u.horizon = textureLoad(globals, vec2<i32>(g::G_SKY_HORIZON, 0), 0).rgb;
    u.ground = textureLoad(globals, vec2<i32>(g::G_SKY_GROUND, 0), 0).rgb;
    u.sun_color = textureLoad(globals, vec2<i32>(g::G_SKY_SUN, 0), 0).rgb;
    u.sun_dir = textureLoad(globals, vec2<i32>(g::G_SKY_SUN_DIR, 0), 0).xyz;
    u.moon_dir = textureLoad(globals, vec2<i32>(g::G_SKY_MOON_DIR, 0), 0).xyz;
    let p = textureLoad(globals, vec2<i32>(g::G_SKY_PARAMS, 0), 0);
    u.night = p.x;
    u.time = p.y;
    u.cloud = p.z;
    u.haze = p.w;
    return u;
}

fn hash(p_in: vec3<f32>) -> f32 {
    var p = fract(p_in * 0.3183099 + 0.1);
    p *= 17.0;
    return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}

fn hash2d(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2d(i), hash2d(i + vec2<f32>(1.0, 0.0)), u.x), mix(hash2d(i + vec2<f32>(0.0, 1.0)), hash2d(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

// cloudN's two fetches, given the samples.
fn cloud_n(a_tex: vec4<f32>, b_tex: vec4<f32>) -> f32 {
    let a = a_tex.a;
    let b = (b_tex.r - 0.59) * 2.4;
    return a * 0.62 + b * 0.38;
}

// skyFrag's main(), for the direction `v_dir` (vDir, normalised again as
// the GLSL does).
fn sky_color(v_dir: vec3<f32>, u: SkyUniforms, noise: texture_2d<f32>, noise_sampler: sampler) -> vec3<f32> {
    let d = normalize(v_dir);
    let h = d.y;

    // The noise lookups (see the header).
    let cp = d.xz / (h + 0.09);
    let wind = vec2<f32>(u.time * 0.005, u.time * 0.0018);
    var q = cp * 1.35 + wind;
    q += (textureSample(noise, noise_sampler, q * 0.021 + 0.7).a - 0.5) * 1.2;
    let n = cloud_n(textureSample(noise, noise_sampler, q * 0.05), textureSample(noise, noise_sampler, q * 0.23 + 0.31));
    let to_sun = normalize(u.sun_dir.xz + vec2<f32>(1e-4)) * 0.3;
    let n2 = cloud_n(textureSample(noise, noise_sampler, (q + to_sun) * 0.05), textureSample(noise, noise_sampler, (q + to_sun) * 0.23 + 0.31));
    let cq = d.xz / (h + 0.3);
    let cs = vec2<f32>(cq.x * 1.2 + cq.y * 0.4, cq.y * 5.0 - cq.x * 1.5) + wind * 2.5;
    let ci_a = textureSample(noise, noise_sampler, cs * 0.06 + 0.13).a;
    let ci_b = textureSample(noise, noise_sampler, cs * 0.25).r;
    let mw_r = textureSample(noise, noise_sampler, d.xz / (h + 0.4) * 0.4 + d.y).r;

    let sd = dot(d, u.sun_dir);
    let sun_amt = max(sd, 0.0);
    let t = pow(clamp(h, 0.0, 1.0), 0.45);
    var col = mix(u.horizon, u.zenith, t);
    // Sun-side glow: a broad warm wash along the horizon plus a tighter halo.
    let sun2 = sun_amt * sun_amt;
    col += u.sun_color * ((sun2 * sun2 * sun2) * 0.35 * (1.0 - t) + sun2 * 0.08 * (1.0 - t) * (1.0 - t));
    // Twilight arch: while the sun is just below the horizon, a warm band
    // hugs the horizon on its side of the sky (and a faint pink one opposite,
    // the Belt of Venus), so blue hour isn't a flat gradient.
    let twi = smooth_step(-0.22, -0.02, u.sun_dir.y) * (1.0 - smooth_step(0.03, 0.12, u.sun_dir.y));
    if (twi > 0.0) {
        let az = dot(normalize(d.xz + vec2<f32>(1e-4)), normalize(u.sun_dir.xz + vec2<f32>(1e-4)));
        let low = 1.0 - smooth_step(0.0, 0.28, max(h, 0.0));
        col += vec3<f32>(1.0, 0.5, 0.3) * twi * low * low * pow(max(az, 0.0), 3.0) * 0.16;
        col += vec3<f32>(0.5, 0.3, 0.42) * twi * (1.0 - smooth_step(0.02, 0.2, abs(h - 0.08))) * max(-az, 0.0) * 0.05;
    }
    // Horizon haze: the band just above the horizon fades into the fog colour
    // (plus the same sun tint the fog chunk adds), so fogged far terrain meets
    // the sky without a seam.
    let haze_col = u.ground + u.sun_color * (sun2 * sun2 * 0.06 + pow(sun_amt, 24.0) * 0.1);
    let haze = exp(-max(h, 0.0) * 16.0) * u.haze;
    col = mix(col, haze_col, haze);
    // Below the horizon fade to the fog/ground tone.
    col = mix(col, haze_col, smooth_step(0.0, -0.06, h));

    // Clouds.
    var cloud_cover = 0.0;
    if (h > 0.0 && u.cloud > 0.01) {
        let fade = smooth_step(0.0, 0.14, h);
        // Cumulus: domain-warped fbm, shaded by comparing density one step
        // toward the sun (thinner toward the sun = lit side).
        let cov = mix(0.6, 0.42, u.cloud);
        let dens = smooth_step(cov, cov + 0.22, n);
        let lit = clamp(0.55 + (n - n2) * 5.0, 0.0, 1.0);
        // Base (shadowed) and lit tones: sky-tinted greys by day, sun-coloured
        // at dawn/dusk, deep blue at night with moonlit edges.
        let shadow_c = mix(u.zenith, u.horizon, 0.55) * 0.72 + 0.03;
        let lit_c = u.sun_color * 0.95 + u.horizon * 0.35 + 0.04;
        let day = 1.0 - u.night;
        var cc = mix(shadow_c, lit_c, lit * mix(0.35, 1.0, day));
        // Silver lining: thin edges near the sun glow.
        cc += u.sun_color * pow(sun_amt, 10.0) * (1.0 - dens) * 1.6;
        cc = mix(cc, u.horizon * 0.35 + vec3<f32>(0.02, 0.025, 0.05) + vec3<f32>(0.1, 0.11, 0.14) * lit * pow(max(dot(d, u.moon_dir), 0.0), 6.0), u.night * 0.85);
        // Distant clouds sink into the haze.
        cc = mix(cc, haze_col, (1.0 - smooth_step(0.02, 0.35, h)) * 0.6);
        // Cirrus: high thin streaks, stretched along the wind.
        var ci = ci_a * 0.7 + (ci_b - 0.59) * 0.8;
        ci = smooth_step(0.55, 0.85, ci) * 0.4 * (1.0 - dens);
        var ci_c = mix(u.horizon * 1.05 + 0.05, u.sun_color * 0.8 + u.horizon * 0.4, pow(sun_amt, 2.0));
        ci_c = mix(ci_c, u.horizon * 0.5, u.night * 0.8);
        col = mix(col, ci_c, ci * u.cloud * fade);
        cloud_cover = clamp((dens * min(u.cloud * 1.25, 1.0) + ci * u.cloud) * fade, 0.0, 1.0);
        col = mix(col, cc, dens * min(u.cloud * 1.25, 1.0) * fade);
    }

    // Sun disc and glow.
    col += u.sun_color * (smooth_step(0.9993, 0.9997, sd) * 6.0 + pow(sun_amt, 180.0) * 0.8 + pow(sun_amt, 32.0) * 0.18) * step(-0.02, h) * (1.0 - cloud_cover * 0.8);
    // Moon: disc with dusky maria, plus a soft halo.
    let md = dot(d, u.moon_dir);
    let disc = smooth_step(0.99955, 0.99975, md);
    var maria = 0.0;
    if (md > 0.9995) {
        maria = vnoise(d.xz * 1400.0 + d.y * 900.0) * 0.35 + vnoise(d.xz * 3100.0) * 0.15;
    }
    col += vec3<f32>(0.85, 0.9, 1.0) * u.night * (1.0 - cloud_cover * 0.7) * (disc * (2.4 - maria * 1.6) + pow(max(md, 0.0), 400.0) * 0.25 + pow(max(md, 0.0), 30.0) * 0.05);
    // Stars (twinkling, slightly tinted) and the Milky Way band.
    if (u.night > 0.5 && h > 0.0) {
        let night_sky = smooth_step(0.5, 0.95, u.night) * smooth_step(0.0, 0.25, h);
        let sp = d * 420.0;
        let cell = floor(sp);
        let r = hash(cell);
        let star = step(0.9965, r) * smooth_step(0.55, 0.0, length(fract(sp) - 0.5));
        let tw = 0.7 + 0.3 * sin(u.time * 3.0 + r * 60.0);
        let tint = mix(vec3<f32>(1.0, 0.85, 0.7), vec3<f32>(0.75, 0.85, 1.0), fract(r * 91.0));
        // A band across the sky, tilted: faint glow plus a denser star field.
        let bq = dot(d, normalize(vec3<f32>(0.35, 0.45, -0.82))) * 5.0;
        let band = exp(-bq * bq);
        let mw = band * (0.5 + mw_r) * 0.5;
        let faint = step(0.985 - band * 0.01, hash(floor(d * 900.0))) * 0.35;
        col += (tint * star * tw * 1.4 + vec3<f32>(0.8, 0.85, 1.0) * faint * band + vec3<f32>(0.07, 0.075, 0.1) * mw) * night_sky * (1.0 - cloud_cover);
    }
    return col;
}
