//! The GLSL ES 3.00 passes of a GPU bounce build: a ray tracer over the
//! flattened scene, mirroring [`crate::indirect`]'s trace in f32.

/// The scene a build traces through, its uniforms and its queries:
/// `scene_first_hit` (closest hit past wholly clear surfaces),
/// `scene_transmittance` (the any-hit product) and `scene_direct` (the
/// lamps' shadowed light on a point).
macro_rules! scene_glsl {
    () => {
        r"precision highp float;
precision highp int;
precision highp sampler2D;
precision highp usampler2DArray;
uniform highp sampler2D u_nodes;
uniform highp sampler2D u_tris;
uniform highp sampler2D u_uvs;
uniform highp sampler2D u_materials;
uniform highp usampler2DArray u_pictures;
uniform highp sampler2D u_lamps;
uniform int u_lamp_count;
uniform float u_far;
uniform float u_eps;

vec4 flat_fetch(highp sampler2D s, int i) {
    return texelFetch(s, ivec2(i % 2048, i / 2048), 0);
}

// Whether the ray o + t d crosses the box lo..hi for some t in [t0, t1],
// the box widened so it never rejects what the triangle test accepts.
bool scene_crosses(vec3 lo, vec3 hi, vec3 o, vec3 d, float t0, float t1) {
    for (int k = 0; k < 3; k++) {
        if (d[k] == 0.0) {
            if (o[k] < lo[k] || o[k] > hi[k]) return false;
            continue;
        }
        float a = (lo[k] - o[k]) / d[k];
        float c = (hi[k] - o[k]) / d[k];
        if (a > c) { float s = a; a = c; c = s; }
        float slack = 1e-5 * max(max(abs(a), abs(c)), u_far);
        t0 = max(t0, a - slack);
        t1 = min(t1, c + slack);
        if (t0 > t1) return false;
    }
    return true;
}

// Integer texel coordinate i folded into 0..n by wrap (0 repeat, 1 clamp,
// 2 mirrored repeat), as texture.rs's fold.
float scene_fold(float i, float n, float wrap) {
    float f;
    if (wrap < 0.5) {
        f = i - n * floor(i / n);
    } else if (wrap < 1.5) {
        f = clamp(i, 0.0, n - 1.0);
    } else {
        float m = i - 2.0 * n * floor(i / (2.0 * n));
        f = m < n ? m : 2.0 * n - 1.0 - m;
    }
    return clamp(floor(f), 0.0, n - 1.0);
}

vec4 scene_texel(int base, vec4 t0, float x, float y, float wu, float wv) {
    int k = base + int(scene_fold(y, t0.z, wv)) * int(t0.y) + int(scene_fold(x, t0.y, wu));
    int rows = textureSize(u_pictures, 0).y;
    int row = k / 2048;
    return vec4(texelFetch(u_pictures, ivec3(k % 2048, row % rows, row / rows), 0));
}

// Texture m's texel at stored coordinates uv, 0..255, as Texture::sample.
vec4 scene_sample(int m, vec2 uv) {
    vec4 t0 = flat_fetch(u_materials, 5 * m);
    vec4 t1 = flat_fetch(u_materials, 5 * m + 1);
    vec4 t2 = flat_fetch(u_materials, 5 * m + 2);
    int base = int(flat_fetch(u_materials, 5 * m + 3).y) * 16777216 + int(t0.x);
    float u = t1.x * uv.x + t1.y * uv.y + t1.z;
    float v = t2.x * uv.x + t2.y * uv.y + t2.z;
    if (t0.w < 0.5) v = 1.0 - v;
    if (t0.y < 1.0 || t0.z < 1.0 || isnan(u) || isinf(u) || isnan(v) || isinf(v)) return vec4(0.0);
    float x = u * t0.y - 0.5;
    float y = v * t0.z - 0.5;
    float x0 = floor(x), y0 = floor(y);
    float fx = x - x0, fy = y - y0;
    vec4 a = scene_texel(base, t0, x0, y0, t1.w, t2.w);
    vec4 b = scene_texel(base, t0, x0 + 1.0, y0, t1.w, t2.w);
    vec4 c = scene_texel(base, t0, x0, y0 + 1.0, t1.w, t2.w);
    vec4 e = scene_texel(base, t0, x0 + 1.0, y0 + 1.0, t1.w, t2.w);
    vec4 top = a + (b - a) * fx;
    vec4 bottom = c + (e - c) * fx;
    return clamp(floor(top + (bottom - top) * fy + 0.5), 0.0, 255.0);
}

// Triangle p's straight RGBA colour (0..1) at barycentric weights w, as
// Painted::colour_at.
vec4 scene_colour(int p, vec3 w) {
    vec4 base = flat_fetch(u_tris, 4 * p + 3);
    float slot = flat_fetch(u_tris, 4 * p + 2).w;
    if (slot < 0.0) return base;
    vec4 q0 = flat_fetch(u_uvs, 2 * p);
    vec4 q1 = flat_fetch(u_uvs, 2 * p + 1);
    int m = int(slot);
    vec4 t = scene_sample(m, w.x * q0.xy + w.y * q0.zw + w.z * q1.xy) / 255.0;
    float f = flat_fetch(u_materials, 5 * m + 3).x;
    vec4 out_c;
    if (f < 0.5) {
        out_c = vec4(base.rgb * t.rgb, base.a * t.a);
    } else if (f < 1.5) {
        out_c = vec4(t.rgb, base.a * t.a);
    } else if (f < 2.5) {
        vec4 blend = flat_fetch(u_materials, 5 * m + 4);
        out_c = vec4(base.rgb * (1.0 - t.rgb) + blend.rgb * t.rgb, base.a * t.a);
    } else {
        out_c = vec4(base.rgb * (1.0 - t.a) + t.rgb * t.a, base.a);
    }
    return floor(clamp(out_c, 0.0, 1.0) * 255.0 + 0.5) / 255.0;
}

// Triangle p met by the ray at t >= tmin: t, the unit face normal and the
// hit's barycentric weights on the corners.
bool scene_intersect(int p, vec3 o, vec3 d, float tmin, out float t, out vec3 n, out vec3 w) {
    vec3 c0 = flat_fetch(u_tris, 4 * p).xyz;
    vec3 e1 = flat_fetch(u_tris, 4 * p + 1).xyz - c0;
    vec3 e2 = flat_fetch(u_tris, 4 * p + 2).xyz - c0;
    vec3 nn = cross(e1, e2);
    float nl = length(nn);
    t = 0.0;
    n = vec3(0.0, 0.0, 1.0);
    w = vec3(1.0 / 3.0);
    if (!(nl > 0.0) || nl > 3.0e38) return false;
    vec3 pv = cross(d, e2);
    float det = dot(e1, pv);
    if (abs(det) < 1.1920929e-7 * max(dot(e1, e1), dot(e2, e2))) return false;
    vec3 s = o - c0;
    float u = dot(s, pv) / det;
    vec3 q = cross(s, e1);
    float v = dot(d, q) / det;
    t = dot(e2, q) / det;
    if (u < 0.0 || u > 1.0 || v < 0.0 || u + v > 1.0 || !(t >= tmin) || t > 3.0e38) return false;
    n = nn / nl;
    w = vec3(1.0 - u - v, u, v);
    return true;
}

// The nearest triangle in [tmin, tmax] that is not wholly clear, ties to
// the lower (mesh, triangle); -1 for none. t, n and w as scene_intersect.
int scene_first_hit(vec3 o, vec3 d, float tmin, float tmax, out float t, out vec3 n, out vec3 w) {
    int stack[64];
    int sp = 1;
    stack[0] = 0;
    int best = -1;
    float limit = tmax;
    vec2 key = vec2(0.0);
    n = vec3(0.0, 0.0, 1.0);
    w = vec3(1.0 / 3.0);
    while (sp > 0) {
        sp--;
        int node = stack[sp];
        vec4 a = flat_fetch(u_nodes, 2 * node);
        vec4 b = flat_fetch(u_nodes, 2 * node + 1);
        if (!scene_crosses(a.xyz, b.xyz, o, d, tmin, limit)) continue;
        if (a.w >= 0.0) {
            if (sp <= 62) { stack[sp] = int(b.w); stack[sp + 1] = int(a.w); sp += 2; }
            continue;
        }
        int end = int(b.w);
        for (int p = int(-a.w) - 1; p < end; p++) {
            float h; vec3 hn, hw;
            if (!scene_intersect(p, o, d, tmin, h, hn, hw) || h > limit) continue;
            if (scene_colour(p, hw).a <= 0.0) continue;
            vec2 k = vec2(flat_fetch(u_tris, 4 * p).w, flat_fetch(u_tris, 4 * p + 1).w);
            bool tie_lower = h == limit && (k.x < key.x || (k.x == key.x && k.y < key.y));
            if (best >= 0 && !(h < limit || tie_lower)) continue;
            best = p; limit = h; key = k; n = hn; w = hw;
        }
    }
    t = limit;
    return best;
}

// The share of light that gets along the ray from tmin to tmax.
float scene_transmittance(vec3 o, vec3 d, float tmin, float tmax) {
    int stack[64];
    int sp = 1;
    stack[0] = 0;
    float through = 1.0;
    while (sp > 0) {
        sp--;
        int node = stack[sp];
        vec4 a = flat_fetch(u_nodes, 2 * node);
        vec4 b = flat_fetch(u_nodes, 2 * node + 1);
        if (!scene_crosses(a.xyz, b.xyz, o, d, tmin, tmax)) continue;
        if (a.w >= 0.0) {
            if (sp <= 62) { stack[sp] = int(b.w); stack[sp + 1] = int(a.w); sp += 2; }
            continue;
        }
        int end = int(b.w);
        for (int p = int(-a.w) - 1; p < end; p++) {
            float h; vec3 hn, hw;
            if (!scene_intersect(p, o, d, tmin, h, hn, hw) || h > tmax) continue;
            through *= clamp(1.0 - scene_colour(p, hw).a, 0.0, 1.0);
            if (through == 0.0) return 0.0;
        }
    }
    return through;
}

// Lamp l's light straight onto p on the side its unit normal n faces.
vec3 scene_lamp(int l, vec3 p, vec3 n) {
    vec4 t0 = flat_fetch(u_lamps, 4 * l);
    vec4 t1 = flat_fetch(u_lamps, 4 * l + 1);
    vec3 to_light = t0.xyz;
    float dist = u_far;
    vec3 colour = t1.rgb;
    if (t0.w > 0.5) {
        vec4 t2 = flat_fetch(u_lamps, 4 * l + 2);
        vec4 t3 = flat_fetch(u_lamps, 4 * l + 3);
        vec3 v = t0.xyz - p;
        float d = length(v);
        if (!(d > 0.0)) return vec3(0.0);
        to_light = v / d;
        float share = 0.0;
        if (d <= t1.w) {
            float den = t2.x + t2.y * d + t2.z * d * d;
            float fade = den > 0.0 && den <= 3.4e38 ? 1.0 / den : 1.0;
            float cone = 1.0;
            if (t0.w > 1.5) {
                float c = -dot(to_light, t3.xyz);
                cone = c < t2.w ? 0.0 : (t3.w > 0.0 ? pow(max(c, 0.0), t3.w) : 1.0);
            }
            share = fade * cone;
        }
        dist = d - u_eps;
        colour *= share;
    }
    float c = dot(n, to_light);
    if (c <= 0.0 || all(equal(colour, vec3(0.0)))) return vec3(0.0);
    return colour * (c * scene_transmittance(p + n * u_eps, to_light, u_eps, dist));
}

// Lamps first .. first + count's light straight onto p on the side n faces.
vec3 scene_direct_range(int first, int count, vec3 p, vec3 n) {
    vec3 sum = vec3(0.0);
    for (int l = first; l < first + count; l++) sum += scene_lamp(l, p, n);
    return sum;
}

// Every lamp's light straight onto p on the side n faces.
vec3 scene_direct(vec3 p, vec3 n) {
    return scene_direct_range(0, u_lamp_count, p, n);
}

"
    };
}

/// The vertex shader of every build pass: one triangle covering the
/// target, drawn with `drawArrays(TRIANGLES, 0, 3)` and no attributes.
pub const GLSL_ES_300_BOUNCE_BUILD_VERTEX: &str = r"#version 300 es
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
";

/// The trace pass: texel (`x`, `y`) of the target traces ray `x mod 64`
/// of probe `8 y + x div 64`, writing the radiance it brings back (RGB)
/// and how far it went (alpha). Reads the previous bounce through
/// `u_bounce_atlas` unless `u_first`.
pub const GLSL_ES_300_BOUNCE_TRACE_FRAGMENT: &str = concat!(
    "#version 300 es\n",
    scene_glsl!(),
    bounce_glsl!(),
    r"uniform highp sampler2D u_probes;
uniform int u_probe_count;
uniform vec3 u_rays[64];
uniform bool u_first;
uniform vec3 u_sky;
out vec4 o_ray;

void main() {
    ivec2 f = ivec2(gl_FragCoord.xy);
    int probe = f.y * 8 + f.x / 64;
    if (probe >= u_probe_count) { o_ray = vec4(0.0); return; }
    vec4 at = flat_fetch(u_probes, probe);
    if (at.w < 0.5) { o_ray = vec4(0.0, 0.0, 0.0, u_far); return; }
    vec3 d = u_rays[f.x % 64];
    float t;
    vec3 n, w;
    int p = scene_first_hit(at.xyz, d, u_eps, u_far, t, n, w);
    if (p < 0) { o_ray = vec4(u_sky, u_far); return; }
    vec3 hit = at.xyz + d * t;
    if (dot(n, d) > 0.0) n = -n;
    vec4 albedo = scene_colour(p, w);
    vec3 light = scene_direct(hit, n);
    if (!u_first) light += bounce(hit, n);
    o_ray = vec4(albedo.rgb * albedo.a * light, t);
}
"
);

/// The reduce pass: writes the bounce's atlas, in [`crate::FrameBounce`]'s
/// layout, from the trace pass's rays.
pub const GLSL_ES_300_BOUNCE_REDUCE_FRAGMENT: &str = r"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2D;
uniform highp sampler2D u_trace;
uniform highp sampler2D u_probes;
uniform int u_probe_count;
uniform vec3 u_rays[64];
uniform vec3 u_texels[64];
uniform float u_far;
uniform float u_cap;
out vec4 o_texel;

void main() {
    ivec2 f = ivec2(gl_FragCoord.xy);
    int probe = (f.y / 8) * 64 + f.x / 16;
    int u = f.x % 16;
    if (probe >= u_probe_count) { o_texel = vec4(0.0); return; }
    bool used = texelFetch(u_probes, ivec2(probe % 2048, probe / 2048), 0).w >= 0.5;
    if (!used) {
        o_texel = u < 8 ? vec4(0.0) : vec4(u_far, 1e-12 * (1.0 + u_far * u_far), 0.0, 0.0);
        return;
    }
    ivec2 row = ivec2((probe % 8) * 64, probe / 8);
    vec3 td = u_texels[(f.y % 8) * 8 + u % 8];
    if (u < 8) {
        vec3 e = vec3(0.0);
        float ew = 0.0;
        for (int r = 0; r < 64; r++) {
            float c = dot(td, u_rays[r]);
            if (c <= 0.0) continue;
            e += texelFetch(u_trace, row + ivec2(r, 0), 0).rgb * c;
            ew += c;
        }
        o_texel = vec4(ew > 0.0 ? e / ew : vec3(0.0), 1.0);
        return;
    }
    float best = -2.0;
    float nearest = u_cap;
    float m = 0.0;
    float mw = 0.0;
    for (int r = 0; r < 64; r++) {
        float c = dot(td, u_rays[r]);
        float d = min(texelFetch(u_trace, row + ivec2(r, 0), 0).a, u_cap);
        if (c > best) { best = c; nearest = d; }
        if (c <= 0.0) continue;
        float w = c * c; w *= w; w *= w; w *= w; w *= w; w *= w;
        m += w * d;
        mw += w;
    }
    if (!(mw > 0.0)) {
        o_texel = vec4(nearest, 1e-12 * (1.0 + nearest * nearest), 0.0, 0.0);
        return;
    }
    float mean = m / mw;
    float s = 0.0;
    for (int r = 0; r < 64; r++) {
        float c = dot(td, u_rays[r]);
        if (c <= 0.0) continue;
        float w = c * c; w *= w; w *= w; w *= w; w *= w; w *= w;
        float g = min(texelFetch(u_trace, row + ivec2(r, 0), 0).a, u_cap) - mean;
        s += w * g * g;
    }
    float variance = s / mw;
    o_texel = vec4(mean, max(variance, 1e-12 * (1.0 + variance + mean * mean)), 0.0, 0.0);
}
";

/// The light-meter map pass ([`crate::GpuLightMap`]): texel (`x`, `y`)
/// measures point `2048 y + x`, writing the luminance of the light
/// arriving straight from the lamps (R), bounced (G, strength applied)
/// and straight from glowing surfaces (B).
pub const GLSL_ES_300_LIGHT_METER_MAP_FRAGMENT: &str = concat!(
    "#version 300 es\n",
    scene_glsl!(),
    bounce_glsl!(),
    r"uniform highp sampler2D u_points;
uniform highp sampler2D u_normals;
uniform highp sampler2D u_directions;
uniform highp sampler2D u_glow;
uniform int u_point_count;
uniform int u_direction_count;
uniform int u_glow_count;
uniform int u_bounce_first;
uniform int u_bounce_count;
uniform float u_gather_far;
uniform vec3 u_sky;
out vec4 o_figure;

float meter_luminance(vec3 c) {
    return dot(c, vec3(0.2126, 0.7152, 0.0722));
}

void main() {
    ivec2 f = ivec2(gl_FragCoord.xy);
    int i = f.y * 2048 + f.x;
    if (i >= u_point_count) { o_figure = vec4(0.0); return; }
    vec3 p = flat_fetch(u_points, i).xyz;
    vec3 n = flat_fetch(u_normals, i).xyz;
    vec3 direct = scene_direct(p, n);
    vec3 reflected = vec3(0.0);
    vec3 emitted = vec3(0.0);
    float weight = 0.0;
    if (u_bounce || u_glow_count > 0) {
        float far = u_bounce ? u_gather_far : u_far;
        vec3 o = p + n * u_eps;
        for (int k = 0; k < u_direction_count; k++) {
            vec3 d = flat_fetch(u_directions, k).xyz;
            float c = dot(n, d);
            if (c <= 0.0) continue;
            weight += c;
            float t;
            vec3 hn, hw;
            int h = scene_first_hit(o, d, u_eps, far, t, hn, hw);
            if (h < 0) {
                if (u_bounce) reflected += u_sky * c;
                continue;
            }
            vec4 a = scene_colour(h, hw);
            float kept = clamp(a.a, 0.0, 1.0);
            int mesh = int(flat_fetch(u_tris, 4 * h).w);
            if (mesh < u_glow_count) emitted += flat_fetch(u_glow, mesh).rgb * (kept * c);
            if (!u_bounce) continue;
            vec3 hit = o + d * t;
            if (dot(hn, d) > 0.0) hn = -hn;
            vec3 light = scene_direct_range(u_bounce_first, u_bounce_count, hit, hn) + bounce(hit, hn);
            reflected += a.rgb * kept * light * c;
        }
    }
    float k = weight > 0.0 ? 1.0 / weight : 0.0;
    o_figure = vec4(meter_luminance(direct),
        u_bounce_strength * meter_luminance(reflected * k),
        meter_luminance(emitted * k), 1.0);
}
"
);
