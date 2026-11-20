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
/// bytes for those rows and columns, in order.
pub const GLSL_ES_300_FRAGMENT: &str = r"#version 300 es
precision highp float;
precision highp int;
precision highp isampler2DArray;
uniform isampler2DArray u_scene;
uniform ivec2 u_origin;
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
ivec4 channels(int v) { return ivec4(v & 255, (v >> 8) & 255, (v >> 16) & 255, (v >> 24) & 255); }

void main() {
  int px = int(gl_FragCoord.x) + u_origin.x, py = int(gl_FragCoord.y) + u_origin.y;
  ivec4 head = at(0), sections = at(1);
  int dcols = (head.x + 8190) / 8191, stride = dcols * ((head.y + 8190) / 8191);
  int k = (py / 8191) * dcols + px / 8191;
  ivec4 tile = at(sections.x + (py >> 2) * head.z + (px >> 4));
  ivec4 d = ivec4(0);
  for (int e = tile.x; e < tile.x + tile.y; e++) {
    ivec4 op = at(sections.y + lane(e));
    int c = coverage(op.x, px, py);
    if (c == 0) continue;
    ivec4 u = channels(op.z);
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
