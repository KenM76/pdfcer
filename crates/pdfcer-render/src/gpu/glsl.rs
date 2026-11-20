//! The shaders that draw a [`super::scene::GpuScene`]: one full-screen
//! triangle, and per pixel [`super::reference`]'s algorithm, line for line.

/// GLSL ES 3.00 vertex shader: `gl_VertexID` 0..3 covers the framebuffer
/// with one triangle; no attributes or buffers.
pub const GLSL_ES_300_VERTEX: &str = "#version 300 es
void main() {
  vec2 p = vec2(float((gl_VertexID & 1) << 2), float((gl_VertexID & 2) << 1));
  gl_Position = vec4(p - 1.0, 0.0, 1.0);
}
";

/// GLSL ES 3.00 fragment shader: binds `u_scene` (an `isampler2DArray` over
/// [`super::scene::GpuScene::data`], `RGBA32I`, nearest) and writes the
/// pixel's RGBA8 to an `RGBA8` colour attachment. Framebuffer pixel
/// `(x, y)` is raster pixel `u_origin + (x, y)` (`ivec2`, default zero), so
/// a raster larger than the largest framebuffer is drawn as tiles, and
/// `readPixels` over a tile returns [`crate::DisplayList::replay_region`]'s
/// bytes for those rows and columns, in order. `u_zero` (`uint`) must be
/// left at its default zero: image sampling passes every product through
/// it, which keeps the compiler from fusing a multiply into the add after
/// it and so matches the CPU's separately rounded `f32` arithmetic.
pub const GLSL_ES_300_FRAGMENT: &str = r"#version 300 es
precision highp float;
precision highp int;
precision highp isampler2DArray;
uniform isampler2DArray u_scene;
uniform ivec2 u_origin;
uniform uint u_zero;
out vec4 o_colour;

ivec4 at(int i) { return texelFetch(u_scene, ivec3(i & 2047, (i >> 11) & 2047, i >> 22), 0); }
int lane(int e) { return at(e >> 2)[e & 3]; }
int round16(int x) { return (x + 0x8000) >> 16; }

struct Edge { int x; int dx; int first_y; int last_y; int winding; int index; };

Edge edge(int i) {
  ivec4 t = at(i);
  return Edge(t.x, t.y, t.z, t.w >> 1, (t.w & 1) != 0 ? 1 : -1, i);
}
int x_at(Edge e, int y) { return e.x + e.dx * (y - e.first_y); }
bool live(Edge e, int y) { return e.first_y <= y && y <= e.last_y; }
bool before(Edge a, Edge b, int y) {
  int xa = x_at(a, y), xb = x_at(b, y);
  if (xa != xb) return xa < xb;
  if (a.first_y != y && b.first_y != y && a.dx != b.dx) return xa - a.dx < xb - b.dx;
  if (a.first_y != b.first_y) return a.first_y < b.first_y;
  return a.index < b.index;
}
bool filled(int w, bool even_odd) { return even_odd ? (w & 1) != 0 : w != 0; }

bool split(int base, int count, int back, int c0, int y, bool eo) {
  for (int i = base; i < base + count; i++) {
    Edge e = edge(i);
    int x = round16(x_at(e, y));
    if (!live(e, y) || x <= c0 || x >= c0 + 4) continue;
    bool follower = false;
    int w = back;
    for (int j = base; j < base + count; j++) {
      Edge f = edge(j);
      if (!live(f, y)) continue;
      if (j == i || before(f, e, y)) w += f.winding;
      else if (round16(x_at(f, y)) == x) follower = true;
    }
    if (follower && !filled(w, eo)) return true;
  }
  return false;
}

int coverage(int h, int px, int py) {
  ivec4 o = at(h + 3);
  px -= o.x;
  py -= o.y;
  ivec4 area = at(h);
  if (px < area.x || px >= area.z || py < area.y || py >= area.w) return 0;
  ivec4 rows = at(h + 1);
  ivec4 tiles = at(h + 2);
  int c = (px >> 4) - tiles.x, r = (py >> 2) - tiles.y;
  if (c < 0 || r < 0 || c >= tiles.z || r >= tiles.w) return 0;
  ivec4 entry = at(h + rows.w + r * tiles.z + c);
  entry.x += h;
  if (entry.z >= 0) entry.z += h;
  bool eo = rows.z != 0;
  if (o.z == 0) {
    if (py < rows.x || py >= rows.y) return 0;
    int w = entry.z < 0 ? 0 : lane((entry.z << 2) + (py & 3));
    for (int i = entry.x; i < entry.x + entry.y; i++) {
      Edge e = edge(i);
      if (live(e, py) && round16(x_at(e, py)) <= px) w += e.winding;
    }
    return filled(w, eo) ? 255 : 0;
  }
  int sum = 0;
  for (int k = 0; k < 4; k++) {
    int y = (py << 2) + k;
    if (y < rows.x || y >= rows.y) continue;
    int b = entry.z < 0 ? 0 : lane((entry.z << 2) + (py & 3) * 4 + k);
    int c0 = px << 2;
    ivec4 w = ivec4(b);
    for (int i = entry.x; i < entry.x + entry.y; i++) {
      Edge e = edge(i);
      if (!live(e, y)) continue;
      int x = round16(x_at(e, y));
      for (int j = 0; j < 4; j++) if (x <= c0 + j) w[j] += e.winding;
    }
    int n = 0;
    for (int j = 0; j < 4; j++) if (filled(w[j], eo)) n++;
    if (n < 4) sum += 16 * n;
    else if (k == 3 && !split(entry.x, entry.y, b, c0, y, eo)) sum += 63;
    else sum += 64;
  }
  return min(sum, 255);
}

int mask(int clip_base, int clip, int k, int stride, int px, int py) {
  int chain[64];
  int n = 0;
  for (int at_ = clip; at_ >= 0;) {
    ivec4 t = at(clip_base + at_ * stride + k);
    if (t.x < 0 || n == 64) return 0;
    chain[n] = t.x;
    n++;
    at_ = t.y;
  }
  int m = 255;
  for (int k = n - 1; k >= 0; k--) {
    int c = coverage(chain[k], px, py);
    m = k == n - 1 ? c : c * m / 255;
  }
  return m;
}

int div255(int v) { return (v + 255) >> 8; }

float mul(float a, float b) { return uintBitsToFloat(floatBitsToUint(a * b) ^ u_zero); }
vec4 mul(vec4 a, float b) { return uintBitsToFloat(floatBitsToUint(a * b) ^ u_zero); }
float factor() { return uintBitsToFloat(0x3b808081u); }
vec4 load(int p) {
  return mul(vec4(p & 255, (p >> 8) & 255, (p >> 16) & 255, (p >> 24) & 255), factor());
}
float below(int n) { return intBitsToFloat(floatBitsToInt(float(n)) - 1); }
float clamp_to(float v, float l) { v = v > 0.0 ? v : 0.0; return v < l ? v : l; }
int gather(float x, float y, ivec2 size) {
  return int(clamp_to(y, below(size.y))) * size.x + int(clamp_to(x, below(size.x)));
}

vec4 sample_image(int base, ivec2 size, bool bilinear, float t[6], ivec2 l) {
  float r = float(l.x) + 0.5, g = float(l.y) + 0.5;
  float x = mul(r, t[0]) + (mul(g, t[2]) + t[4]);
  float y = mul(r, t[1]) + (mul(g, t[3]) + t[5]);
  if (!bilinear) return load(lane((base << 2) + gather(x, y, size)));
  float vx = x + 0.5, vy = y + 0.5;
  float fx = vx - floor(vx), fy = vy - floor(vy);
  vec4 acc = vec4(0.0);
  float yy = y + -0.5;
  for (int j = 0; j < 2; j++) {
    float wy = j == 0 ? 1.0 - fy : fy;
    float xx = x + -0.5;
    for (int i = 0; i < 2; i++) {
      float wx = i == 0 ? 1.0 - fx : fx;
      vec4 s = load(lane((base << 2) + gather(xx, yy, size)));
      acc = mul(s, mul(wx, wy)) + acc;
      xx += 1.0;
    }
    yy += 1.0;
  }
  return acc;
}

ivec4 image_pixel(int h, int rec, int px, int py, ivec4 d, float q, int c) {
  ivec4 o = at(h + 3), r0 = at(rec), r1 = at(rec + 1), r2 = at(rec + 2);
  float t[6] = float[6](intBitsToFloat(r1.x), intBitsToFloat(r1.y), intBitsToFloat(r1.z),
                        intBitsToFloat(r1.w), intBitsToFloat(r2.x), intBitsToFloat(r2.y));
  vec4 s = sample_image(r0.x, r0.yz, r0.w != 0, t, ivec2(px - o.x, py - o.y));
  if (q >= 0.0) s = mul(s, q);
  if (c < 255) s = mul(s, mul(float(c), factor()));
  vec4 v = mul(mul(vec4(d), factor()), 1.0 - s.a) + s;
  return ivec4(roundEven(mul(min(max(v, 0.0), 1.0), 255.0)));
}
int w16(int v) { return v & 0xFFFF; }
int d16(int v) { return w16(v + 255) >> 8; }
int light(int s, int sa, int d, int da, bool c) {
  int v = c ? w16(w16(2 * s) * d) : w16(w16(sa * da) - w16(w16(2 * w16(sa - s)) * w16(da - d)));
  return d16(w16(w16(w16(s * w16(255 - da)) + w16(d * w16(255 - sa))) + v));
}
int blend_channel(int code, int s, int sa, int d, int da) {
  int sda = w16(s * da), dsa = w16(d * sa), sum = w16(s + d);
  if (code == 1) return d16(w16(w16(w16(s * w16(255 - da)) + w16(d * w16(255 - sa))) + w16(s * d)));
  if (code == 2) return w16(sum - d16(w16(s * d)));
  if (code == 3) return light(s, sa, d, da, w16(d + d) <= da);
  if (code == 4) return w16(sum - d16(max(sda, dsa)));
  if (code == 5) return w16(sum - d16(min(sda, dsa)));
  if (code == 6) return light(s, sa, d, da, w16(s + s) <= sa);
  if (code == 7) return w16(sum - w16(2 * d16(min(sda, dsa))));
  return w16(sum - w16(2 * d16(w16(s * d))));
}
ivec4 blend_pixel(int code, ivec4 s, ivec4 d, int c) {
  ivec4 r;
  for (int k = 0; k < 3; k++) r[k] = blend_channel(code, s[k], s.a, d[k], d.a);
  r.a = code <= 2 ? blend_channel(code, s.a, s.a, d.a, d.a) : w16(s.a + d16(w16(d.a * w16(255 - s.a))));
  if (c < 255) for (int k = 0; k < 4; k++) r[k] = d16(w16(w16(d[k] * w16(255 - c)) + w16(r[k] * c)));
  return r & 255;
}
float hp_light(float s, float sa, float d, float da, bool c) {
  float v;
  if (c) { float p = mul(s, d); v = p + p; }
  else { float p = mul(da - d, sa - s); v = mul(sa, da) - (p + p); }
  return mul(s, 1.0 - da) + mul(d, 1.0 - sa) + v;
}
float hp_channel(int code, float s, float sa, float d, float da) {
  if (code == 1) return mul(s, 1.0 - da) + mul(d, 1.0 - sa) + mul(s, d);
  if (code == 2) return s + d - mul(s, d);
  if (code == 3) return hp_light(s, sa, d, da, d + d <= da);
  if (code == 4) return s + d - max(mul(s, da), mul(d, sa));
  if (code == 5) return s + d - min(mul(s, da), mul(d, sa));
  if (code == 6) return hp_light(s, sa, d, da, s + s <= sa);
  float m = code == 7 ? min(mul(s, da), mul(d, sa)) : mul(s, d);
  return s + d - (m + m);
}
ivec4 composite(ivec4 d, ivec4 inner, float opacity, int code) {
  vec4 s = mul(mul(vec4(inner), factor()), opacity);
  vec4 dv = mul(vec4(d), factor());
  vec4 r;
  if (code == 0) r = mul(dv, 1.0 - s.a) + s;
  else {
    for (int k = 0; k < 3; k++) r[k] = hp_channel(code, s[k], s.a, dv[k], dv.a);
    r.a = code <= 2 ? hp_channel(code, s.a, s.a, dv.a, dv.a) : mul(dv.a, 1.0 - s.a) + s.a;
  }
  return ivec4(roundEven(mul(min(max(r, 0.0), 1.0), 255.0)));
}
int byte(float v) {
  float f = floor(v);
  return int(clamp(v - f >= 0.5 ? f + 1.0 : f, 0.0, 255.0));
}
ivec4 shade_pixel(ivec4 s, ivec4 d) {
  float a = intBitsToFloat(s.w);
  if (!(a > 0.0)) return d;
  float inv = 1.0 - a;
  int oa = byte(mul(a, 255.0) + mul(float(d.a), inv));
  ivec4 o = ivec4(0, 0, 0, oa);
  for (int k = 0; k < 3; k++) o[k] = min(byte(intBitsToFloat(s[k]) + mul(float(d[k]), inv)), oa);
  return o;
}
ivec4 channels(int v) { return ivec4(v & 255, (v >> 8) & 255, (v >> 16) & 255, (v >> 24) & 255); }

void main() {
  int px = int(gl_FragCoord.x) + u_origin.x, py = int(gl_FragCoord.y) + u_origin.y;
  ivec4 head = at(0), sections = at(1);
  int dcols = (head.x + 8190) / 8191, stride = dcols * ((head.y + 8190) / 8191);
  int k = (py / 8191) * dcols + px / 8191;
  ivec4 tile = at(sections.x + (py >> 2) * head.z + (px >> 4));
  ivec4 d = ivec4(0);
  ivec4 saved[8];
  int depth = 0;
  for (int e = tile.x; e < tile.x + tile.y; e++) {
    ivec4 op = at(sections.y + lane(e));
    if (op.z == 0x00FFFFFE) {
      if (depth < 8) saved[depth] = d;
      depth++;
      d = ivec4(0);
      continue;
    }
    if (op.z == 0x00FFFFFD) {
      depth = max(depth - 1, 0);
      d = composite(depth < 8 ? saved[depth] : ivec4(0), d, intBitsToFloat(op.x), op.y);
      continue;
    }
    int c = coverage(op.x, px, py);
    if (c == 0) continue;
    if (op.z == 0x00FFFFFC) {
      ivec4 r = at(op.w);
      d = shade_pixel(at(r.x + (py - r.z) * r.w + (px - r.y)), d);
      continue;
    }
    if (op.z == 0x00FFFFFF) {
      float q = -1.0;
      if (op.y >= 0) {
        int m = mask(sections.z, op.y, k, stride, px, py);
        if (m == 0) continue;
        q = intBitsToFloat(lane((sections.w << 2) + m));
      }
      d = image_pixel(op.x, op.w, px, py, d, q, c);
      continue;
    }
    ivec4 u = channels(op.z);
    int code = at(op.x + 3).w;
    if (code != 0) {
      if (op.y >= 0) {
        int m = mask(sections.z, op.y, k, stride, px, py);
        if (m == 0) continue;
        u = (u * m + 255) >> 8;
      }
      d = blend_pixel(code, u, d, c);
      continue;
    }
    if (op.y < 0 && u.a == 255) {
      if (c == 255) d = channels(op.w);
      else d = (d * (255 - c) + u * c + 255) >> 8;
      continue;
    }
    ivec4 s = u;
    if (op.y >= 0) {
      int m = mask(sections.z, op.y, k, stride, px, py);
      if (m == 0) continue;
      s = (s * m + 255) >> 8;
    }
    if (c < 255) s = (s * c + 255) >> 8;
    d = s + ((d * (255 - s.a) + 255) >> 8);
  }
  ivec3 rgb = clamp(d.rgb + 255 - d.a, 0, 255);
  o_colour = vec4(vec3(rgb), 255.0) / 255.0;
}
";
