//! Which of a model's point and spot lights a frame draws: those whose
//! reach ([`crate::LampCut`]) meets the view, and of those, under
//! [`LampCost::NearestPerFrame`], the brightest at the camera target.
//!
//! A lamp whose reach sphere lies wholly outside the view frustum adds
//! exactly 0.0 to every pixel ([`crate::model_shade::Local::reach`] is 0
//! past it, and lamps light per pixel only), so leaving it out changes no
//! pixel on either path. Reach is fixed from the model's whole lamp count
//! before the cull, so the cull cannot move it.

use crate::lighting::Shader;
use crate::model_shade::Local;
use crate::raster::View;
use crate::render::{Camera, RenderError, RenderOptions};
use crate::vec3::{dot, length, sub};

/// How many of a model's point and spot lights a frame draws
/// ([`RenderOptions::lamp_cost`]).
///
/// Separate from [`crate::LampCut`], which bounds each lamp's reach so the
/// light left out stays under a level: this one chooses lamps, and its
/// cheaper setting can change the picture by any amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LampCost {
    /// Every lamp whose reach meets the view (the default). The picture is
    /// the one every lamp draws.
    #[default]
    Full,
    /// Only the given number of the lamps meeting the view, those adding
    /// most at the camera target (brightest channel times fade and cone
    /// there; ties to the earlier lamp). Never chosen automatically:
    /// it darkens whatever the lamps left out lit, so the picture
    /// changes. 0 draws no lamp.
    NearestPerFrame(u16),
}

/// How many of a model's point and spot lights one frame draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct LampStats {
    /// The model's point and spot lights.
    pub in_model: usize,
    /// Those whose reach meets the view frustum; every lamp under a
    /// fisheye, which has no single frustum.
    pub in_view: usize,
    /// Those drawn: `in_view`, or fewer under [`LampCost::NearestPerFrame`].
    pub lit_this_frame: usize,
}

impl LampStats {
    /// The counts [`crate::render_model`] draws `lights` with, from
    /// `camera` at `options`' size; the same as
    /// [`crate::FrameLights::lamps`].
    ///
    /// ```
    /// use pdfcer_3d::{Camera, LampStats, ModelLight, ModelLightKind, Projection, RenderOptions};
    /// let lamp = |x| ModelLight::new(
    ///     ModelLightKind::Point { position: [x, 0.0, 1.0], attenuation: [1.0, 0.0, 1.0] },
    ///     [1.0; 3],
    /// );
    /// let camera = Camera {
    ///     eye: [0.0, 0.0, 10.0],
    ///     target: [0.0; 3],
    ///     up: [0.0, 1.0, 0.0],
    ///     projection: Projection::Perspective { fov_y: 30.0 },
    /// };
    /// let stats = LampStats::for_model(&camera, &RenderOptions::default(), &[lamp(0.0), lamp(500.0)])?;
    /// assert_eq!((stats.in_model, stats.in_view, stats.lit_this_frame), (2, 1, 1));
    /// # Ok::<(), pdfcer_3d::RenderError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// As [`crate::FrameLights::for_model`].
    pub fn for_model(
        camera: &Camera,
        options: &RenderOptions,
        lights: &[crate::ModelLight],
    ) -> Result<LampStats, RenderError> {
        let options = crate::render_mode::frame_options(options);
        Ok(frame_shader(&options, lights, camera, None)?.1)
    }
}

/// `options.width / options.height`, the aspect a frame's view is culled at.
pub(crate) fn aspect(options: &RenderOptions) -> f64 {
    f64::from(options.width) / f64::from(options.height)
}

/// The shader a frame of `camera` (one face of `parent`'s fisheye, when
/// given) draws, holding only the lamps it lights, and the counts. Lights
/// are placed by the whole camera's basis, and the lamps are counted and
/// ranked for the whole camera, so every face of a fisheye draws the same
/// lamps; each face then leaves out the lamps its own frustum misses.
pub(crate) fn frame_shader(
    options: &RenderOptions,
    model: &[crate::ModelLight],
    camera: &Camera,
    parent: Option<&Camera>,
) -> Result<(Shader, LampStats), RenderError> {
    let whole = parent.unwrap_or(camera);
    let mut shader = Shader::for_frame(options, model, &whole.basis()?)?;
    let Shader::Set { local, .. } = &mut shader else {
        return Ok((shader, LampStats::default()));
    };
    let in_model = local.len();
    retain_in_view(local, whole, aspect(options));
    let in_view = local.len();
    if let LampCost::NearestPerFrame(n) = options.lamp_cost {
        keep_nearest(local, whole.target, usize::from(n));
    }
    let lit_this_frame = local.len();
    if parent.is_some() {
        retain_in_view(local, camera, aspect(options));
    }
    let stats = LampStats {
        in_model,
        in_view,
        lit_this_frame,
    };
    Ok((shader, stats))
}

/// The shadow slot each of a frame's lamps casts into, `None` for a lamp
/// casting none: its model index, among the first
/// [`crate::MAX_SHADOW_CASTERS`] of the model's lamps, so a lamp keeps its
/// slot as the view moves. The same on both paths.
pub(crate) fn shadow_slots(local: &[Local]) -> Vec<Option<usize>> {
    local
        .iter()
        .map(|l| (l.model < crate::MAX_SHADOW_CASTERS).then_some(l.model))
        .collect()
}

/// Leave out of `local` each lamp whose reach misses `camera`'s view at
/// `aspect`; none under a camera with no planar view.
fn retain_in_view(local: &mut Vec<Local>, camera: &Camera, aspect: f64) {
    let (Ok(view), Ok(basis)) = (View::new(camera, aspect), camera.basis()) else {
        return;
    };
    local.retain(|l| meets(l, camera.eye, &basis, &view));
}

/// Whether lamp `l`'s reach sphere may meet the view from `eye` along
/// `basis` (right, up, forward): false only when it lies wholly outside
/// one plane of the frustum (a perspective view's sides and near plane, an
/// orthographic view's sides), by more than a margin covering rounding.
pub(crate) fn meets(l: &Local, eye: [f64; 3], basis: &[[f64; 3]; 3], view: &View) -> bool {
    if !l.reach.is_finite() {
        return true;
    }
    let [right, up, forward] = basis;
    let d = sub(l.position, eye);
    let (x, y, z) = (dot(d, *right), dot(d, *up), dot(d, *forward));
    let r = l.reach + 1e-5 * (l.reach + length(d));
    match *view {
        View::Perspective {
            focal,
            aspect,
            near,
        } => {
            let outside = |a: f64, t: f64| (a.abs() - z * t) / t.mul_add(t, 1.0).sqrt() > r;
            !(z < near - r || outside(x, aspect / focal) || outside(y, 1.0 / focal))
        }
        View::Orthographic {
            half_height,
            aspect,
        } => !(x.abs() > half_height * aspect + r || y.abs() > half_height + r),
    }
}

/// Keep the `n` of `local` adding most at `target`, in their order.
fn keep_nearest(local: &mut Vec<Local>, target: [f64; 3], n: usize) {
    if local.len() <= n {
        return;
    }
    let mut ranked: Vec<(f64, usize)> = local
        .iter()
        .enumerate()
        .map(|(i, l)| (at_target(l, target), i))
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut keep = vec![false; local.len()];
    for &(_, i) in ranked.iter().take(n) {
        if let Some(k) = keep.get_mut(i) {
            *k = true;
        }
    }
    let mut it = keep.into_iter();
    local.retain(|_| it.next().unwrap_or(false));
}

/// Lamp `l`'s brightest channel times its fade and cone at `target`
/// (unfaded at the lamp itself); 0 for NaN.
fn at_target(l: &Local, target: [f64; 3]) -> f64 {
    let bright = l.colour.iter().fold(0.0f64, |a, &c| a.max(c));
    let k = bright * l.reach(target).map_or(1.0, |(_, k)| k);
    if k.is_nan() { 0.0 } else { k }
}
