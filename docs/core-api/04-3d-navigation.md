# `pdfcer-3d-nav` — navigating a 3D model viewer

**For a shell drawing an interactive 3D view** (pdfcer-gui's model viewer,
a future wasm32 shell). The crate is the viewer's camera logic with no
windowing, input or GPU dependency: orbit, first-person walk, the lens, the
operator's up axis, a file's saved view, and the model's size for fitting.
The shell maps input to these calls, draws the `pdfcer_3d::Camera` they
return, and owns the clock.

A shell depends on `pdfcer-3d-nav` and `pdfcer-3d` (for `Camera`, `Bounds`,
`TriangleMesh`, `Ray`, `Hit`, `NamedView`, `UpAxis`). It does **not** need
`pdfcer-core` here: a file's saved view arrives as the plain [`SavedAim`]
struct, filled from `pdfcer_core::threed::ThreeDSavedView` (see *Traps*).

**The guarantee every verb keeps:** for any finite model, every camera the
crate produces has a finite eye, target and projection, or the call returns
`RenderError::Camera`. Lengths are scaled norms (no squaring overflow), eyes
are placed by `Camera::looking_at` (distance clamped to `f64::MAX`, then
halved until the eye is finite), and nothing is indexed by a number.

## I want to… → call this

| I want to… | call | source |
|---|---|---|
| Size a model for fitting (drawn vertices, not the box) | `reach(&meshes, &bounds) -> f64`; falls back to `radius(&bounds)` when no vertex is drawn | `pdfcer-3d-nav/src/reach.rs` |
| The box's half-diagonal (1 for an empty or non-finite box) | `radius(&bounds) -> f64` | `reach.rs` |
| Open on a named view (Iso, Front, Back, Left, Right, Top, Bottom) | `Orbit::named(NamedView, perspective, Upright)` | `orbit.rs` |
| Open on the file's own saved view | `Orbit::saved(&SavedAim) -> Option<Orbit>`; `None` when direction and up are parallel | `orbit.rs` |
| The camera to draw this frame | `orbit.camera(&bounds, reach, aspect, frame: Option<ViewFrame>) -> Result<Camera, RenderError>` | `orbit.rs` |
| Turn by a drag | add to `orbit.yaw` / `orbit.pitch`, clamp pitch to `±PITCH_LIMIT` | `orbit.rs` |
| Turn about the point the pointer pressed | `orbit.anchor_at(&camera, point)` (ignored for a non-finite point); `orbit.refit()` forgets it | `orbit.rs` |
| Double-click a point to centre on it | `orbit.centre_on(&bounds, point)` | `orbit.rs` |
| Wheel-zoom keeping the point under the pointer still | `orbit.zoom_at(&bounds, reach, aspect, factor, offset, frame)`; `offset` in picture heights from the centre | `orbit.rs` |
| Pan | `orbit.set_pan([x, y])` / `orbit.pan_by([dx, dy])` (picture right/up, in model radii; each component held to `±PAN_LIMIT`, a non-finite pan ignored) | `orbit.rs` |
| Look along a picked direction, refitted | `orbit.look_along(direction)` | `orbit.rs` |
| The axes a turn is measured in | `orbit.axes() -> Axes`; `Axes::upright(Upright)`, `Axes::from_look(direction, up)`, `front()`, `up()`, `right()`, `look(yaw, pitch)`, `angles(direction)` | `orbit.rs` |
| Choose the up axis and the front | `Upright { up, front }`, `with_up(Axis)`, `with_front(Axis)`, `fronts()` (the four fronts square to `up`) | `axes.rs` |
| Name or persist an axis | `Axis::ALL`, `index()`, `vector()`, `token()` (`+x`…`-z`), `up_axis() -> UpAxis`, `negative()`, `perpendicular(other)` | `axes.rs` |
| Start walking from the orbit view | `Walk::from_camera(&camera, axes, radius)` | `walk.rs` |
| Start walking at a point, looking somewhere | `Walk::standing(eye, look, axes, radius)` | `walk.rs` |
| Advance a walk one frame | `walk.step(Intent, dt, Rules, &mut world)`; `world` is `FnMut(&Ray) -> Option<Hit>`, never asked about a ray that is not finite. A solid walker climbs a walkable ramp at any pace, stays on one going down, and is never stood up into what is above it or on its way there, keeping a quarter of its eye height clear; under a ceiling lower than its height it can still walk out, and stands up once clear. A solid step is made in parts of at most one eye height, up to 64, so a small set height in a large model still meets the floors along its stride | `walk.rs` |
| Move a set distance (keys, taps) | `walk.travel([forward, right], Rules, &mut world)` | `walk.rs` |
| Turn the body, or only the head | `walk.turn(yaw, pitch)` / `walk.look(yaw, pitch)`; `walk.centre_look()` re-aligns the head | `walk.rs` |
| Zoom the walker's lens | `walk.zoom_by(factor, &Lens)` (a non-positive or non-finite factor is ignored) | `walk.rs` |
| Face a direction / go to a picked point | `walk.face(direction)` / `walk.go_to(&hit, from)` | `walk.rs` |
| Level the view | `walk.level()` | `walk.rs` |
| The walker's camera | `walk.camera(aspect, &Lens) -> Camera` (always finite; never fails) | `walk.rs` |
| Body height, falling, look direction | `walk.body()`, `walk.falling()`, `walk.direction()`, `walk.axes()` | `walk.rs` |
| What the operator is pressing | `Intent { forward, right, rise, run }`, `intent.idle()` | `walk.rs` |
| How walking behaves | `Rules { solid, level, speed, stairs }`; `CLIMB` is the highest obstacle climbed with `stairs` on, as a share of eye height | `walk.rs` |
| Ray-cast the shown model, cached | `Solids::default()`, `solids.hit(&meshes, version, shown, &ray) -> Option<Hit>`; `forget()` drops the index | `solids.rs` |
| Shape a gamepad stick tilt | `shaped(tilt: f32) -> f64` (small dead zone, then squared; past ±1 is ±1, NaN is 0) | `stick.rs` |
| A flat, fisheye or automatic lens | `Lens { kind: LensKind, curve: FisheyeMapping, auto_at }`; `fisheye_at(fov)`, `widest()`, `projection(fov)`, `applied(camera)`; `is_fisheye(&camera)` | `lens.rs` |
| Lens limits | `FLAT_WIDEST` (100°), `FISHEYE_WIDEST` (180°), `AUTO_RANGE` (40°..=100°), `CURVES` (the offered fisheye mappings; not `Equirectangular`, which has no field to zoom — a `curve` of it makes `projection` always draw 360°), `LensKind::ALL` | `lens.rs` |
| Can I walk from the saved view's eye? | `aim.eye_inside(&bounds) -> Result<[f64; 3], NotInside>` | `saved.rs` |
| Keep a framed saved view's scale | pass `Some(ViewFrame { position, height })` to `camera` / `zoom_at`; applied only while orthographic and `orbit.framed` | `saved.rs` |
| Orbit limits | `PITCH_LIMIT` (just under 90°), `NAMED_PITCH` (89°: the named Top and Bottom views), `PAN_LIMIT` (1e3 model radii), `ZOOM_RANGE` (0.05..50), `FIT_MARGIN` (1.05) | `orbit.rs` |

## In `pdfcer-3d` (used by this crate, public for any shell)

| item | what |
|---|---|
| `DEFAULT_FOV_Y` | 30°: the vertical field of view of every fitted perspective camera (`Camera::fit`, `fit_meshes`, `fit_sphere`) |
| `Camera::fit_sphere(centre, reach, direction, up, perspective, aspect)` | frames a sphere exactly (no margin); errors on a non-positive aspect or reach, a zero direction, direction ∥ up, or no finite eye |
| `Camera::looking_at(target, direction, distance, up, projection)` | the eye `distance` back along `direction`, clamped and halved until finite |
| `Camera::visible_height(&self) -> f64` | the scene height shown at the target (ortho height, or `2·d·tan(fov/2)` with fov clamped to 179°); finite for a finite camera |
| `pdfcer_3d::vec3::{Vec3, add, sub, span, scale, dot, cross, length, normalize, unit_cross}` | `length` and `normalize` scale before squaring; `span` halves a difference that overflows; `unit_cross` rescales its inputs |

## The order of calls in a frame

1. On load: `let reach = reach(&meshes, &bounds);` (recompute when meshes change).
2. `let camera = orbit.camera(&bounds, reach, aspect, frame)?;` — or
   `walk.camera(aspect, &lens)` while walking, then `lens.applied(camera)`
   for an orbit camera drawn through the lens.
3. Input edits `orbit`'s fields or calls its verbs; the next frame's
   `camera` reflects them. Nothing is cached between frames except in
   `Solids`.

## Traps

- **Filling `SavedAim` from `pdfcer-core`.** Call
  `ThreeDSavedView::saved_aim()` and `ThreeDSavedView::view_frame(aspect)`
  (feature `3d`); they read the C2W z column as `direction`, its y column
  **as is** (not negated) as `up`, and its translation as `position`. Do not
  rebuild them from `aim(aspect)`. An `up` not square to `direction` is
  accepted: it becomes the turning axis and the view is pitched off it.
- **`Orbit::camera` can fail** for a look along the up axis or a model with
  no finite eye. Keep the last good camera and draw that; do not unwrap.
- **`frame` is ignored** unless the orbit is orthographic, `framed`, and has
  no anchor. Choosing perspective drops the frame's scale on purpose.
- **`Solids::hit` keys its cache on the mesh slice's address and length.**
  Pass the same slice every frame; bump `version` whenever part visibility
  changes, or hidden parts stay solid.
- **A walker's `step` with a non-finite `dt` or intent does nothing**; it
  does not reset. A walker at the edge of the floats moves only when the
  new eye is finite.
- **`NotInside` is `#[non_exhaustive]`**: match it with a wildcard arm.
  `SavedAim`, `ViewFrame`, `Intent`, `Rules`, `Upright`, `Lens` and `Anchor`
  are plain structs a shell builds with a literal.
