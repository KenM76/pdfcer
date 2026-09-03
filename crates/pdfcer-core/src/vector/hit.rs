//! # Hit-test geometry (ISO 32000-1 §8.5.3 fill rules)
//!
//! Point→object and marquee→objects hit-testing over a page's
//! [`super::PageObjects`], in **page space**, per decision 011 §2.1's
//! "hit-test geometry … is what the snapping engine (12.M1) and the GUI
//! selection consume." All math is `pdfcer-core`-local (GUI-free) so the
//! GUI target provider (`pdfce-gui`) stays a thin adapter that only
//! converts coordinate spaces.
//!
//! ## What "hits" an object
//!
//! - **Filled path** (`f`/`B`/…): the point is *inside* the fill, tested
//!   with the object's own winding rule (§8.5.3.3) — nonzero winding
//!   number ≠ 0, or even-odd crossing parity odd — with every subpath
//!   treated as closed (a fill "implicitly closes all open subpaths",
//!   §8.5.3.1). A near-miss just outside the edge also hits within
//!   `tolerance`.
//! - **Stroked path** (`S`/`s`/`B`/…): the point is within
//!   `stroke_half_width + tolerance` of the path outline, where the
//!   stroke half-width is the user-space line width scaled into page space
//!   by the object's CTM (§8.4.3.2 — line width is a user-space quantity).
//! - **Clip/no-op path** (`n`): the point is within `tolerance` of the
//!   outline (invisible geometry is still selectable, but only precisely).
//! - **Text / image / form**: the point is inside the object's page bbox
//!   (inflated by `tolerance`). These carry no editable node geometry, so
//!   a bbox test is the whole of it.
//!
//! ## Topmost wins — and the ones underneath
//!
//! [`super::PageObjects::objects`] is in paint order, so the scan runs
//! back-to-front and the **last-painted** (topmost) object at the point
//! wins — the selection convention every editor uses.
//!
//! Two queries share that one scan:
//!
//! - [`hit_test_point`] — the topmost hit, or `None`.
//! - [`hit_test_point_all`] — **every** hit, topmost first.
//!
//! They are not two implementations that happen to agree: both are thin
//! wrappers over the single private [`hits_front_to_back`] iterator, so
//! `hit_test_point(..) == hit_test_point_all(..).first().copied()` holds by
//! construction rather than by discipline. That matters because a GUI that
//! cycles through overlapping objects (Alt+click) resolves the FIRST click
//! with one query and every subsequent click with the other: if the two
//! disagreed about what counts as a hit, the first Alt+click would select
//! one object and the second would start cycling a list that does not
//! contain it. Decision 011 §Z2 names exactly this "two implementations of
//! one idea quietly diverge" shape as the recurring failure of this
//! subsystem; sharing the iterator is the structural answer to it, and
//! `hit_test_point_agrees_with_the_head_of_hit_test_point_all` pins it.
//!
//! ### Why an all-hits query has to exist at all
//!
//! With only a topmost query, an object **behind** another is unreachable:
//! there is no click that can ever select it, because every click at every
//! point inside the overlap resolves to the same winner. `hit_test_rect`
//! does not substitute — it tests bbox enclosure/intersection, applies no
//! tolerance, and returns paint order rather than front-to-back — so it
//! answers a different question with a different geometry. ui-spec
//! `pass-17-dock-and-layer-tree.md` §C.3 names the sibling query as a
//! binding ask for that reason.
//!
//! ## Bézier handling
//!
//! Curves are flattened to [`FLATTEN_STEPS`] line segments for the
//! inside/proximity tests — a fixed subdivision (bounded work for the
//! fuzz target) that is well within a screen pixel at any realistic zoom
//! for the tolerances selection uses.

use super::decompose::{
    FillRule, ImageSource, PageObjects, PathObject, Segment, Subpath, TextObject, VectorObject,
};
use super::geometry::{Bounds, Matrix, Point};

/// Fixed cubic-flattening subdivision (module docs). 16 chords is
/// sub-pixel for selection tolerances and bounds the per-object work a
/// hostile node count can force.
pub const FLATTEN_STEPS: usize = 16;

/// How a marquee rectangle decides which objects it selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarqueeMode {
    /// An object is selected only if its page bbox is **fully enclosed** by
    /// the marquee — decision 011's default, grounded in Inkscape's default
    /// rubber-band behavior (R61).
    Enclosed,
    /// An object is selected if its page bbox **touches** the marquee (any
    /// overlap) — the alternate Inkscape "touch" convention.
    Touched,
}

/// Every object hit by `point`, **topmost (front-most) first** — the ONE
/// scan both public point queries are built from (module docs).
///
/// Private, and deliberately an iterator rather than a `Vec`: it lets
/// [`hit_test_point`] answer with `.next()` and allocate nothing on the
/// click path, while [`hit_test_point_all`] collects the same sequence. A
/// non-finite query point yields an empty iterator (a `NaN` pointer is a
/// miss, never a panic — ARCHITECTURE.md §10).
fn hits_front_to_back<'a>(
    model: &'a PageObjects,
    point: Point,
    tolerance: f64,
) -> impl Iterator<Item = usize> + 'a {
    let finite = point.is_finite();
    model
        .objects
        .iter()
        .enumerate()
        .rev()
        .filter(move |(_, obj)| finite && object_hit(obj, point, tolerance))
        .map(|(i, _)| i)
}

/// Test a page-space `point` against a page's objects and return the
/// **topmost** (last-painted) object's index, or `None` for a miss.
///
/// `tolerance` is a page-space slack (the GUI converts a few screen pixels
/// into page units and passes it here), widening every object's hittable
/// region so a click near — not dead-on — an edge still selects.
///
/// Exactly `hit_test_point_all(model, point, tolerance).first().copied()`,
/// by construction — see the module docs' "Topmost wins" section for why
/// that identity is load-bearing rather than incidental.
#[must_use]
pub fn hit_test_point(model: &PageObjects, point: Point, tolerance: f64) -> Option<usize> {
    hits_front_to_back(model, point, tolerance).next()
}

/// **Every** object a page-space `point` hits within `tolerance`,
/// **topmost/front-most first** (reverse paint order).
///
/// The all-hits sibling of [`hit_test_point`], whose result is exactly this
/// list's head. It exists so a GUI can offer *click-through cycling*:
/// repeated clicks at one point step down the returned list, which is the
/// only way an object completely covered by another can ever be selected
/// (ui-spec `pass-17-dock-and-layer-tree.md` §C.3).
///
/// The hit predicate is the same per-kind rule [`hit_test_point`] uses —
/// fill-interior under the object's winding rule, stroke proximity within
/// **half the CTM-scaled line width PLUS `tolerance`**, bbox inflated by
/// `tolerance` for text/image/form — because both
/// functions filter the one [`hits_front_to_back`] scan. **Empty** for a
/// miss and for a non-finite point; never `None`-vs-empty ambiguity.
///
/// ## Cost
///
/// One linear pass over the page's objects with the same per-object work
/// [`hit_test_point`] does, plus a `Vec` whose length is the number of
/// objects genuinely under the pointer (typically 1–3; bounded above by
/// [`super::MAX_OBJECTS`] in the pathological case of a page whose objects
/// all cover the same point). Callers on a hot path that only need the
/// winner should keep calling [`hit_test_point`], which allocates nothing.
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{Matrix, NoXObjects, Point, decompose, hit_test_point,
///                          hit_test_point_all};
///
/// // Two overlapping filled rectangles; the second painted is on top.
/// let cs = ContentStream::parse(b"0 0 60 60 re f 20 20 60 60 re f".to_vec())?;
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
///
/// let at = Point::new(40.0, 40.0); // inside both
/// let all = hit_test_point_all(&model, at, 0.5);
/// assert_eq!(all, vec![1, 0]); // topmost first
/// // …and the topmost query is exactly the head of that list.
/// assert_eq!(hit_test_point(&model, at, 0.5), all.first().copied());
/// # Ok::<(), pdfcer_core::content::ContentError>(())
/// ```
#[must_use]
pub fn hit_test_point_all(model: &PageObjects, point: Point, tolerance: f64) -> Vec<usize> {
    hits_front_to_back(model, point, tolerance).collect()
}

/// What a deep hit test found.
///
/// Two lists, one paint order — see [`hit_test_point_deep`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    /// An index into [`PageObjects::objects`] — an object drawn by the page's
    /// own content stream, and therefore editable by the paint-order verbs.
    Object(usize),
    /// An index into [`PageObjects::leaves`] — an object drawn from **inside**
    /// a form XObject.
    ///
    /// Read [`FormLeaf::stream`] before doing anything but selecting it: its
    /// token range indexes the form's buffer, not the page's.
    Leaf(usize),
}

/// Every target a page-space `point` hits, **topmost first**, descending into
/// form XObjects and **excluding the forms themselves**.
///
/// # ★★★ WHY A FORM IS NOT A CANDIDATE
///
/// [`hit_test_point`] treats an image/form object as its bounding box inflated
/// by the tolerance. That is right for a **raster image**, whose quad genuinely
/// is its ink. It is wrong for a **form**, whose `/BBox` is a declaration about
/// extent (§8.10.1 — a clipping boundary) and says nothing about coverage: a
/// form declaring the whole `MediaBox` and drawing one small line is legal and
/// common.
///
/// The consequence, which the operator met: a page-sized form is a page-sized
/// hit target sitting in paint order above everything drawn before it, and it
/// wins every click at every point. *"When I click on one of the objects all I
/// get is the page selected."* He was selecting a real object; it was a form.
///
/// So this function answers with what is **inside** the form. The form itself
/// is still reachable — [`FormLeaf::containment`] names every enclosing form,
/// so a shell can offer "select the container" as a deliberate second act,
/// which is a different thing from having it win by default.
///
/// # ★★ The ordering, which is the part that is easy to get wrong
///
/// Leaves and page objects are **two lists and one paint order**: a form's
/// contents are painted exactly where its `Do` sits among the page's other
/// objects. Something drawn on the page *after* a form sits on top of
/// everything inside it. So this does not concatenate the lists — it
/// interleaves them on [`FormLeaf::paint_order`], which is why that field
/// exists. Returning "all leaves first" or "all leaves last" would be wrong on
/// any page that draws anything outside its forms.
///
/// Within one form, leaves keep their own paint order; ties therefore resolve
/// to the later-painted leaf, exactly as they do between page objects.
///
/// # Returns
///
/// Topmost first, so `.first()` is the object a single click should select.
/// **Empty** for a miss and for a non-finite point — never a `None`-vs-empty
/// ambiguity. A caller offering click-through cycling steps down the list, and
/// gets objects inside forms as first-class stops on that walk.
///
/// # Examples
///
/// ```
/// # use pdfcer_core::document::Document;
/// # use pdfcer_core::page_tree;
/// # use pdfcer_core::vector::{decompose_page, hit_test_point_deep, HitTarget, Matrix, Point};
/// # fn demo(doc: &Document) -> Result<(), Box<dyn std::error::Error>> {
/// let page = &page_tree::pages(doc)?[0];
/// let model = decompose_page(&doc.view(), page, Matrix::IDENTITY)?;
///
/// // A click inside a page-sized form now finds what is drawn in it.
/// if let Some(HitTarget::Leaf(i)) = hit_test_point_deep(&model, Point::new(30.0, 30.0), 1.0).first() {
///     let leaf = &model.leaves[*i];
///     println!("inside {} nested form(s)", leaf.containment.len());
/// }
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn hit_test_point_deep(model: &PageObjects, point: Point, tolerance: f64) -> Vec<HitTarget> {
    if !point.is_finite() {
        return Vec::new();
    }

    // (paint position, tie-breaker within that position, target). The
    // tie-breaker keeps leaves of one form in their own paint order and puts a
    // page object at its own index unambiguously.
    let mut hits: Vec<(usize, usize, HitTarget)> = Vec::new();

    for (i, obj) in model.objects.iter().enumerate() {
        // ★ The exclusion. A form's bbox is an extent declaration, not ink.
        if matches!(obj, VectorObject::Image(img) if img.source == ImageSource::Form) {
            continue;
        }
        if object_hit(obj, point, tolerance) {
            hits.push((i, 0, HitTarget::Object(i)));
        }
    }
    for (i, leaf) in model.leaves.iter().enumerate() {
        if object_hit(&leaf.object, point, tolerance) {
            // `i + 1` so a leaf of the form at index `n` sorts after a page
            // object at index `n` could ever be -- there is no page object at
            // `n` in that case, because `n` IS the form and forms are skipped.
            hits.push((leaf.paint_order, i + 1, HitTarget::Leaf(i)));
        }
    }

    // Topmost first: reverse paint order.
    hits.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    hits.into_iter().map(|(_, _, t)| t).collect()
}

/// The indices of every object selected by a page-space marquee `rect`,
/// per `mode`, in paint order.
#[must_use]
pub fn hit_test_rect(model: &PageObjects, rect: Bounds, mode: MarqueeMode) -> Vec<usize> {
    if rect.is_empty() {
        return Vec::new();
    }
    model
        .objects
        .iter()
        .enumerate()
        .filter(|(_, obj)| {
            let bb = obj.page_bbox();
            match mode {
                MarqueeMode::Enclosed => bb.contained_by(rect),
                MarqueeMode::Touched => bb.intersects(rect),
            }
        })
        .map(|(i, _)| i)
        .collect()
}

/// Whether a deep marquee may select a **form XObject itself**.
///
/// # ★★★ WHY THIS IS A CHOICE FOR A RECT WHEN IT IS NOT ONE FOR A POINT
///
/// [`hit_test_point_deep`] excludes forms outright and needs no policy,
/// because the argument against them is airtight: a `/BBox` is a *clipping
/// extent* (§8.10.1), not ink, so a point inside it is not evidence the
/// operator aimed at the form. There is nothing to weigh.
///
/// A marquee is genuinely different, and the consuming shell said so rather
/// than assuming: *"a marquee that fully encloses a form's box has arguably
/// named the form on purpose, and a form is a legitimate operand… We think
/// that is right and we are not sure."* Enclosing a rectangle **is** a
/// deliberate statement about that rectangle in a way that touching a point
/// inside it is not.
///
/// # Both are shipped, and the default is [`Self::Exclude`]
///
/// Standing rule `R206`: two defensible answers means ship both and pick a
/// default, not ask. The default is `Exclude` for one reason that outweighs
/// the argument above — **two gestures that both mean "select this" must
/// agree about what is selectable.** A click can never yield a form. If a
/// marquee can, the operator acquires, by one gesture and not the other, a
/// selection that every edit verb then refuses. That is a trap laid by the
/// UI rather than a capability.
///
/// [`Self::Include`] exists for the caller who wants the form as an operand —
/// a bounding-box report, a "what is in this region" census, a delete-the-
/// container gesture — and it is a deliberate act at the call site rather
/// than a surprise.
///
/// # ★ `Include` is not the same as the old shallow behaviour
///
/// Under `Include` the form **and** its leaves are both candidates, so a
/// marquee enclosing a form returns the container and everything in it.
/// [`hit_test_rect`]'s shallow answer returns the container **only**. A
/// caller migrating from one to the other is changing two things, and the
/// leaf half is the one that will surprise it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormMarquee {
    /// A form is never itself selected; only what is drawn inside it.
    /// Matches [`hit_test_point_deep`], and is the default.
    #[default]
    Exclude,
    /// A form is selected on its own terms, alongside its leaves.
    Include,
}

/// Every target a page-space marquee `rect` selects, in **paint order**,
/// descending into form XObjects.
///
/// The rect twin of [`hit_test_point_deep`], and it exists because the two
/// gestures disagreeing is a defect an operator meets in the first minute.
///
/// # ★★ Why this needed to exist rather than be composed at the call site
///
/// It was composed at a call site, once, and that is why it is here. The
/// consuming shell shipped `hit_test_rect(…)` plus its own loop over
/// `model.leaves` filtered by `Bounds::contained_by`, and reported the
/// workaround under decision 058 rather than keeping it: *"It is still a
/// second statement of the enclosure rule, in another crate… it will drift
/// the day `MarqueeMode` grows a third mode, or the day `Enclosed` stops
/// meaning `contained_by` — and it will drift **silently**, because our copy
/// will keep compiling and keep returning something plausible."*
///
/// That is the whole argument. A duplicated predicate does not fail when it
/// falls out of date; it keeps answering.
///
/// # Ordering
///
/// Interleaved on [`FormLeaf::paint_order`] exactly as
/// [`hit_test_point_deep`] interleaves, so a marquee's result and a click's
/// result order the same objects the same way. **Front-most LAST here**,
/// which is the opposite of the point query and is deliberate: a point query
/// answers "which one?" and wants the winner first, while a marquee answers
/// "which ones?" and a caller iterating them to draw handles, group them or
/// re-emit them wants paint order. Reversing at the call site is one line;
/// guessing which order a `Vec` is in is a bug.
///
/// # Returns
///
/// Empty for an empty rect. Never `None`-vs-empty ambiguous.
///
/// # Examples
///
/// ```
/// # use pdfcer_core::document::Document;
/// # use pdfcer_core::page_tree;
/// # use pdfcer_core::vector::{decompose_page, hit_test_rect_deep, Bounds, FormMarquee, Matrix, MarqueeMode, Point};
/// # fn demo(doc: &Document) -> Result<(), Box<dyn std::error::Error>> {
/// let page = &page_tree::pages(doc)?[0];
/// let model = decompose_page(&doc.view(), page, Matrix::IDENTITY)?;
/// let region = Bounds { min: Point::new(0.0, 0.0), max: Point::new(200.0, 200.0) };
///
/// // Objects drawn inside a form are selected; the form itself is not.
/// let picked = hit_test_rect_deep(&model, region, MarqueeMode::Enclosed, FormMarquee::Exclude);
/// println!("{} target(s)", picked.len());
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn hit_test_rect_deep(
    model: &PageObjects,
    rect: Bounds,
    mode: MarqueeMode,
    forms: FormMarquee,
) -> Vec<HitTarget> {
    if rect.is_empty() {
        return Vec::new();
    }
    let selects = |bb: Bounds| match mode {
        MarqueeMode::Enclosed => bb.contained_by(rect),
        MarqueeMode::Touched => bb.intersects(rect),
    };

    // (paint position, tie-breaker, target) — the same triple, sorted the
    // same way, as `hit_test_point_deep`. Written out rather than shared with
    // it because the two differ in their per-candidate predicate and in
    // nothing else, and a shared helper taking a closure would make the ONE
    // line that differs the hardest one to find.
    let mut hits: Vec<(usize, usize, HitTarget)> = Vec::new();

    for (i, obj) in model.objects.iter().enumerate() {
        let is_form = matches!(obj, VectorObject::Image(img) if img.source == ImageSource::Form);
        if is_form && forms == FormMarquee::Exclude {
            continue;
        }
        if selects(obj.page_bbox()) {
            hits.push((i, 0, HitTarget::Object(i)));
        }
    }
    for (i, leaf) in model.leaves.iter().enumerate() {
        if selects(leaf.object.page_bbox()) {
            // `i + 1` for the same reason `hit_test_point_deep` uses it: a
            // leaf of the form at index `n` must sort after anything at `n`.
            hits.push((leaf.paint_order, i + 1, HitTarget::Leaf(i)));
        }
    }

    hits.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, _, t)| t).collect()
}

/// Whether `point` hits `obj` within `tolerance` (module docs' per-kind
/// rules).
/// Whether `point` hits a text object.
///
/// Tests the **per-run boxes**, not the object's enclosing rectangle.
///
/// # Why the enclosing rectangle is the wrong shape
///
/// A `BT`…`ET` may hold any number of show operators anywhere on the page,
/// and nothing obliges a producer to keep them together. `page_bbox` is
/// their union, so for a producer that puts every label on a drawing in one
/// text object, that rectangle spans the whole sheet while the ink covers
/// almost none of it — and every point in the gaps "hits" the object.
///
/// This was measured, not hypothesised. On a SolidWorks export, one text
/// object holding every dimension label had `page_bbox` = 23,14 → 1564,1216
/// (the entire drawing) and was painted near the front, so it was the
/// front-most hit for every click on the page; at one point over a real
/// line it beat 57 genuine objects beneath it. Selection was effectively
/// dead on that document, and empty space appeared to select "something"
/// that could not be seen.
///
/// The gaps between runs are not part of the object, and now they do not
/// behave as if they were.
///
/// # The fallback, and why it is the old behaviour rather than a miss
///
/// `runs` is empty when no show operator could be laid out (no resolvable
/// font, an unusable `Tf`) or when the object exceeded [`MAX_TEXT_RUNS`].
/// Then this falls back to `page_bbox` — imprecise, but it is what pdfcer
/// did before and it keeps the object reachable. Treating an empty `runs`
/// as "hits nothing" would make text that pdfcer cannot measure completely
/// unselectable, which trades a wrong hit for a missing one.
fn text_hit(t: &TextObject, point: Point, tolerance: f64) -> bool {
    if t.runs.is_empty() {
        return t.page_bbox.inflate(tolerance).contains(point);
    }
    t.runs
        .iter()
        .any(|r| r.bounds.inflate(tolerance).contains(point))
}

/// Which **runs** (show operators) of the text object at `object_index` a
/// point hits, nearest first — the text-side twin of
/// [`hit_test_subpaths`] (`Pass 32.0`).
///
/// # Why an object is not the unit an operator means, again
///
/// A producer may put every label on a sheet inside one `BT`…`ET`.
/// Measured on a real SolidWorks export: **one text object holding all 237
/// dimension labels**. `hit_test_point` already tests per-run boxes so
/// such an object is not a page-wide hit ([`text_hit`], Pass 18.5) — but
/// it answers *whether*, not *which*, and a shell that wants to delete
/// "this label" needs the index.
///
/// # Ordering
///
/// **Nearest first, by distance to the run's box**, and by the same
/// argument [`hit_test_subpaths`] makes: runs inside one text object have
/// no z-order among themselves to inherit, so "the label I clicked on" is
/// the nearest one and any other order would be arbitrary dressed up as
/// meaningful. A point *inside* a run's box counts at distance zero.
///
/// # Contract
///
/// - Empty for a non-text object, an out-of-range index, or no hit.
/// - Empty when the object has no laid-out runs — no resolvable font, or
///   past `MAX_TEXT_RUNS`. **Deliberately not a fallback to `page_bbox`**,
///   unlike [`text_hit`]: that fallback keeps an unmeasurable object
///   *selectable*, which is the honest answer for "did I hit this object".
///   Here the question is "which run", and inventing run 0 for an object
///   whose runs were never laid out would name a target the caller could
///   then delete — the wrong one, silently.
/// - `tolerance` is in page units and is applied exactly as
///   [`text_hit`] applies it, so a click that selects the text object can
///   always then select one of its runs.
#[must_use]
pub fn hit_test_text_runs(
    model: &PageObjects,
    object_index: usize,
    point: Point,
    tolerance: f64,
) -> Vec<usize> {
    let Some(VectorObject::Text(text)) = model.objects.get(object_index) else {
        return Vec::new();
    };
    let mut hits: Vec<(f64, usize)> = Vec::new();
    for (i, run) in text.runs.iter().enumerate() {
        let b = run.bounds;
        if !b.inflate(tolerance).contains(point) {
            continue;
        }
        // Distance to the box, zero inside it. A run box is an axis-aligned
        // rectangle, so this is the per-axis outside-distance hypotenuse —
        // there is no outline to walk as there is for a subpath.
        let dx = (b.min.x - point.x).max(point.x - b.max.x).max(0.0);
        let dy = (b.min.y - point.y).max(point.y - b.max.y).max(0.0);
        hits.push((dx.hypot(dy), i));
    }
    hits.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, i)| i).collect()
}

/// Which **subpaths** of the path object at `object_index` a point hits,
/// nearest first.
///
/// # Why an object is not always the unit an operator means
///
/// A PDF path object may hold any number of subpaths, and a producer is free to
/// put an entire drawing in one. Measured on a real SolidWorks export: object
/// 5870 of page 1 is a single stroked path with **1194 subpaths and 6681
/// anchors** covering a 550×500 pt isometric view — every visible line of that
/// view is one object as far as [`hit_test_point`] is concerned. Clicking any
/// line selects all of them, and the operator's report was the direct
/// consequence: *"how do I click on individual lines and nodes to move or
/// delete them?"*
///
/// So selection needs a second level. This is the query for it: given the
/// object the operator has already entered, which of its subpaths is under the
/// pointer. It is deliberately a separate function rather than a mode of
/// [`hit_test_point`] — descending into an object is a decision the shell makes
/// from a gesture (a double-click), and the geometry layer should not have to
/// know about selection depth.
///
/// # Ordering
///
/// **Nearest first, by distance to the subpath's outline** — not paint order.
/// Subpaths within one object are painted by a single operator, so there is no
/// z-order among them to inherit; "the line I clicked on" is the nearest one,
/// and any other order would be arbitrary dressed up as meaningful.
///
/// # Contract
///
/// - Returns empty for a non-path object, an out-of-range index, or no hit.
/// - `tolerance` is in page units and is added to the stroke half-width, the
///   same way [`hit_test_point`] treats it, so a click that selects the object
///   can always then select one of its subpaths.
/// - A filled path also hits on interior containment, per subpath, so clicking
///   inside one closed shape of a many-shape path picks that shape.
#[must_use]
pub fn hit_test_subpaths(
    model: &PageObjects,
    object_index: usize,
    point: Point,
    tolerance: f64,
) -> Vec<usize> {
    let Some(VectorObject::Path(path)) = model.objects.get(object_index) else {
        return Vec::new();
    };
    hit_test_subpaths_of(path, point, tolerance)
}

/// [`hit_test_subpaths`] against a path this caller already has in hand.
///
/// # ★ Why this exists, rather than every caller taking an index
///
/// An index into [`PageObjects::objects`] cannot name an object drawn inside
/// a form XObject — those live in [`PageObjects::leaves`], a second list — so
/// an index-only API is structurally incapable of answering a question about
/// form contents. That is not a hypothetical limit: it is why the two-line
/// measure tool was **inert**, not merely degraded, on a CAD drawing whose
/// 10,256 pickable objects were all inside one form.
///
/// Splitting the lookup from the geometry is the whole fix. The geometry
/// never needed the index; only the lookup did.
#[must_use]
pub fn hit_test_subpaths_of(path: &PathObject, point: Point, tolerance: f64) -> Vec<usize> {
    let half = stroke_half_width(path);
    if !path.page_bbox.inflate(half + tolerance).contains(point) {
        return Vec::new();
    }
    let threshold = if path.style.stroke {
        half + tolerance
    } else {
        tolerance
    };

    let mut hits: Vec<(f64, usize)> = Vec::new();
    for (i, sp) in path.page_subpaths().iter().enumerate() {
        let one = std::slice::from_ref(sp);
        let d = outline_distance(one, point);
        // An interior hit on a filled subpath counts at distance zero: the
        // operator is pointing squarely at that shape, which should outrank a
        // neighbouring outline they merely came close to.
        let inside = path
            .style
            .fill
            .is_some_and(|rule| point_inside(one, point, rule));
        if inside {
            hits.push((0.0, i));
        } else if d <= threshold {
            hits.push((d, i));
        }
    }
    // `total_cmp` rather than `partial_cmp().unwrap()`: `outline_distance`
    // returns infinity for an empty subpath, and a NaN coordinate that survived
    // flattening must not panic the sort (R- untrusted input reaches here).
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    hits.into_iter().map(|(_, i)| i).collect()
}

/// The page-space bounding box of one subpath of one object.
///
/// The companion to [`hit_test_subpaths`]: a shell that can select a subpath
/// must be able to outline it, and the object's own `page_bbox` describes the
/// whole 1194-subpath view rather than the one line the operator picked.
///
/// Returns `None` for a non-path object, an out-of-range index, or a subpath
/// with no finite points.
#[must_use]
pub fn subpath_bounds(model: &PageObjects, object_index: usize, subpath: usize) -> Option<Bounds> {
    let VectorObject::Path(path) = model.objects.get(object_index)? else {
        return None;
    };
    let sp = path.page_subpaths().into_iter().nth(subpath)?;
    // `EMPTY` is the grow-from-nothing seed, and `union_point` drops
    // non-finite points — so a subpath whose every vertex is hostile yields an
    // empty box, which the `is_empty` guard turns into `None` rather than a
    // ±∞ rectangle a caller would try to draw.
    let b = flatten(&sp)
        .into_iter()
        .fold(Bounds::EMPTY, Bounds::union_point);
    (!b.is_empty()).then_some(b)
}

/// Shortest distance from `point` to any segment of these subpaths, or
/// infinity if they contain no segment.
///
/// Factored out of [`outline_within`]'s shape because [`hit_test_subpaths`]
/// needs the magnitude to ORDER hits, not merely a yes/no against a threshold.
/// `outline_within` keeps its early return — it is on the per-object hit path,
/// where the answer is a bool and stopping at the first segment within range
/// matters across thousands of objects per click.
fn outline_distance(subpaths: &[Subpath], point: Point) -> f64 {
    let mut best = f64::INFINITY;
    for sp in subpaths {
        let poly = flatten(sp);
        for w in poly.windows(2) {
            let [a, b] = w else { continue };
            best = best.min(dist_sq_point_segment(point, *a, *b));
        }
        if sp.closed
            && poly.len() >= 2
            && let (Some(&last), Some(&firstp)) = (poly.last(), poly.first())
        {
            best = best.min(dist_sq_point_segment(point, last, firstp));
        }
    }
    best.sqrt()
}

fn object_hit(obj: &VectorObject, point: Point, tolerance: f64) -> bool {
    match obj {
        VectorObject::Path(p) => path_hit(p, point, tolerance),
        VectorObject::Text(t) => text_hit(t, point, tolerance),
        VectorObject::Image(i) => i.page_bbox.inflate(tolerance).contains(point),
    }
}

/// Whether `point` hits a path object: inside its fill (if filled) or
/// within the stroke/clip proximity threshold of its outline.
fn path_hit(path: &PathObject, point: Point, tolerance: f64) -> bool {
    // A cheap bbox reject first (the object's page bbox widened by the
    // stroke half-width and tolerance).
    let half = stroke_half_width(path);
    if !path.page_bbox.inflate(half + tolerance).contains(point) {
        return false;
    }

    let subpaths = path.page_subpaths();

    if let Some(rule) = path.style.fill
        && point_inside(&subpaths, point, rule)
    {
        return true;
    }

    let threshold = if path.style.stroke {
        half + tolerance
    } else {
        // A filled-only or `n` path: no stroke, but a near-edge click
        // should still land, so use the tolerance alone as the proximity
        // band.
        tolerance
    };
    outline_within(&subpaths, point, threshold)
}

/// The user-space line width scaled into page space by the object's CTM,
/// halved — the distance the stroke extends either side of the path
/// centerline (§8.4.3.2). A width-0 hairline gets a tiny nominal value so
/// it is still selectable.
fn stroke_half_width(path: &PathObject) -> f64 {
    if !path.style.stroke {
        return 0.0;
    }
    let scale = ctm_scale(path.ctm);
    let w = if path.line_width <= 0.0 {
        0.1
    } else {
        path.line_width
    };
    (w * scale) / 2.0
}

/// A scalar page-space scale estimate for a CTM — the square root of the
/// absolute determinant (the geometric-mean linear scale). Used to map a
/// user-space line width into page space for stroke proximity. A
/// degenerate/non-finite CTM yields a harmless 1.0.
fn ctm_scale(ctm: Matrix) -> f64 {
    let d = ctm.determinant().abs();
    if d.is_finite() && d > 0.0 {
        d.sqrt()
    } else {
        1.0
    }
}

/// Whether `point` is inside the region the subpaths fill, under `rule`
/// (every subpath treated as closed — a fill implicitly closes, §8.5.3.1).
fn point_inside(subpaths: &[Subpath], point: Point, rule: FillRule) -> bool {
    let mut winding = 0i32;
    let mut crossings = 0u32;
    for sp in subpaths {
        let poly = flatten(sp);
        accumulate_crossings(&poly, point, &mut winding, &mut crossings);
    }
    match rule {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => crossings % 2 == 1,
    }
}

/// Whether `point` is within `threshold` of any outline segment (stroke /
/// clip proximity). Closed subpaths include their closing edge.
fn outline_within(subpaths: &[Subpath], point: Point, threshold: f64) -> bool {
    let t2 = threshold * threshold;
    for sp in subpaths {
        let poly = flatten(sp);
        let n = poly.len();
        if n == 0 {
            continue;
        }
        for w in poly.windows(2) {
            let [a, b] = w else { continue };
            if dist_sq_point_segment(point, *a, *b) <= t2 {
                return true;
            }
        }
        // Closing edge, for a closed subpath (a stroked `h`/`re`/`s`).
        if sp.closed
            && n >= 2
            && let (Some(&last), Some(&firstp)) = (poly.last(), poly.first())
            && dist_sq_point_segment(point, last, firstp) <= t2
        {
            return true;
        }
    }
    false
}

/// Flatten one subpath (page space) to a polyline of on-curve vertices,
/// cubics subdivided into [`FLATTEN_STEPS`] chords. Non-finite vertices
/// are dropped (a hostile operand cannot poison the ray cast).
fn flatten(sp: &Subpath) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    let push = |p: Point, out: &mut Vec<Point>| {
        if p.is_finite() {
            out.push(p);
        }
    };
    push(sp.start, &mut out);
    let mut from = sp.start;
    for seg in &sp.segments {
        match *seg {
            Segment::Line { to } => {
                push(to, &mut out);
                from = to;
            }
            Segment::Cubic { c1, c2, to } => {
                for step in 1..=FLATTEN_STEPS {
                    let t = step as f64 / FLATTEN_STEPS as f64;
                    push(cubic_at(from, c1, c2, to, t), &mut out);
                }
                from = to;
            }
        }
    }
    out
}

/// A cubic Bézier point at parameter `t` (de Casteljau, closed form).
fn cubic_at(p0: Point, c1: Point, c2: Point, p3: Point, t: f64) -> Point {
    let u = 1.0 - t;
    let w0 = u * u * u;
    let w1 = 3.0 * u * u * t;
    let w2 = 3.0 * u * t * t;
    let w3 = t * t * t;
    Point::new(
        w0 * p0.x + w1 * c1.x + w2 * c2.x + w3 * p3.x,
        w0 * p0.y + w1 * c1.y + w2 * c2.y + w3 * p3.y,
    )
}

/// Fold one closed polygon's edge crossings of the ray `y = point.y,
/// x ≥ point.x` into the running winding number (signed, for nonzero) and
/// crossing count (unsigned, for even-odd). Standard robust half-open
/// (`[y0, y1)`) crossing test.
fn accumulate_crossings(poly: &[Point], point: Point, winding: &mut i32, crossings: &mut u32) {
    if poly.len() < 2 {
        return;
    }
    // Every consecutive pair, plus the closing edge (last → first) so the
    // polygon is treated as closed (a fill implicitly closes).
    let closing = match (poly.first(), poly.last()) {
        (Some(&f), Some(&l)) => Some((l, f)),
        _ => None,
    };
    let pairs = poly.windows(2).filter_map(|w| match w {
        [a, b] => Some((*a, *b)),
        _ => None,
    });
    for (a, b) in pairs.chain(closing) {
        // Half-open interval avoids double-counting a vertex on the ray.
        let a_below = a.y <= point.y;
        let b_below = b.y <= point.y;
        if a_below == b_below {
            continue;
        }
        // The edge crosses the horizontal line through `point`; find the x
        // of the intersection.
        let dy = b.y - a.y;
        if dy == 0.0 {
            continue;
        }
        let t = (point.y - a.y) / dy;
        let x = a.x + t * (b.x - a.x);
        if x >= point.x {
            *crossings += 1;
            if b.y > a.y {
                *winding += 1; // upward edge
            } else {
                *winding -= 1; // downward edge
            }
        }
    }
}

/// Squared distance from `p` to the segment `a`–`b` (avoids a `sqrt` in
/// the proximity loop). A degenerate segment (`a == b`) reduces to the
/// point distance.
fn dist_sq_point_segment(p: Point, a: Point, b: Point) -> f64 {
    let vx = b.x - a.x;
    let vy = b.y - a.y;
    let wx = p.x - a.x;
    let wy = p.y - a.y;
    let len2 = vx * vx + vy * vy;
    if len2 <= 0.0 {
        return wx * wx + wy * wy;
    }
    let t = ((wx * vx + wy * vy) / len2).clamp(0.0, 1.0);
    let dx = wx - t * vx;
    let dy = wy - t * vy;
    dx * dx + dy * dy
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::content::ContentStream;
    use crate::vector::decompose::{NoXObjects, decompose};

    fn model(src: &[u8]) -> PageObjects {
        let cs = ContentStream::parse(src.to_vec()).unwrap();
        decompose(&cs, Matrix::IDENTITY, &NoXObjects)
    }

    #[test]
    fn a_click_inside_a_filled_rectangle_hits_it() {
        let m = model(b"10 10 80 80 re f");
        assert_eq!(hit_test_point(&m, Point::new(50.0, 50.0), 1.0), Some(0));
        // outside, beyond tolerance
        assert_eq!(hit_test_point(&m, Point::new(200.0, 200.0), 1.0), None);
    }

    #[test]
    fn a_filled_rectangle_with_a_hole_is_even_odd_empty_in_the_hole() {
        // Outer 0..100 square, inner 40..60 square, even-odd fill => the
        // inner square is a hole.
        let m = model(b"0 0 100 100 re 40 40 20 20 re f*");
        // inside the outer ring
        assert_eq!(hit_test_point(&m, Point::new(10.0, 10.0), 0.5), Some(0));
        // inside the hole -> miss (even-odd)
        assert_eq!(hit_test_point(&m, Point::new(50.0, 50.0), 0.5), None);
    }

    #[test]
    fn a_click_near_a_stroked_line_hits_within_the_stroke_and_tolerance() {
        // A 1 pt horizontal line from (0,50) to (100,50).
        let m = model(b"0 50 m 100 50 l S");
        assert_eq!(hit_test_point(&m, Point::new(50.0, 50.4), 0.5), Some(0));
        // Well away from the line -> miss.
        assert_eq!(hit_test_point(&m, Point::new(50.0, 70.0), 0.5), None);
    }

    #[test]
    fn topmost_object_wins_at_an_overlap() {
        // Two overlapping filled rectangles; the second painted is on top.
        let m = model(b"0 0 60 60 re f 20 20 60 60 re f");
        // In the overlap region, the later (index 1) object wins.
        assert_eq!(hit_test_point(&m, Point::new(40.0, 40.0), 0.5), Some(1));
        // In the first-only region, the first wins.
        assert_eq!(hit_test_point(&m, Point::new(5.0, 5.0), 0.5), Some(0));
    }

    #[test]
    fn marquee_enclosed_selects_only_fully_contained_objects() {
        let m = model(b"10 10 20 20 re f 200 200 20 20 re f");
        let rect = Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(100.0, 100.0),
        };
        assert_eq!(hit_test_rect(&m, rect, MarqueeMode::Enclosed), vec![0]);
        // A marquee that only clips the first still selects it under Touched.
        let clip = Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(15.0, 15.0),
        };
        assert_eq!(
            hit_test_rect(&m, clip, MarqueeMode::Enclosed),
            Vec::<usize>::new()
        );
        assert_eq!(hit_test_rect(&m, clip, MarqueeMode::Touched), vec![0]);
    }

    #[test]
    fn a_curve_is_hittable_after_flattening() {
        // A cubic bump from (0,0) to (100,0), control points up high.
        let m = model(b"0 0 m 30 100 70 100 100 0 c S");
        // Near the apex of the flattened curve.
        assert!(hit_test_point(&m, Point::new(50.0, 75.0), 3.0).is_some());
    }

    #[test]
    fn non_finite_query_point_is_a_miss_not_a_panic() {
        let m = model(b"0 0 100 100 re f");
        assert_eq!(hit_test_point(&m, Point::new(f64::NAN, 0.0), 1.0), None);
        assert!(hit_test_point_all(&m, Point::new(f64::NAN, 0.0), 1.0).is_empty());
        assert!(hit_test_point_all(&m, Point::new(0.0, f64::INFINITY), 1.0).is_empty());
    }

    /// The all-hits query returns EVERY object under the point, front-most
    /// first — the ordering a GUI's click-through cycling steps down.
    ///
    /// Three stacked rectangles rather than two, because two cannot tell a
    /// correct front-to-back ordering apart from a merely reversed one.
    #[test]
    fn all_hits_are_returned_front_most_first() {
        // Three concentric-ish filled rectangles, painted 0 then 1 then 2.
        let m = model(b"0 0 100 100 re f 10 10 80 80 re f 20 20 60 60 re f");
        // Dead centre: inside all three. Topmost (last painted) leads.
        assert_eq!(
            hit_test_point_all(&m, Point::new(50.0, 50.0), 0.5),
            vec![2, 1, 0]
        );
        // Inside the outer two only.
        assert_eq!(
            hit_test_point_all(&m, Point::new(15.0, 15.0), 0.5),
            vec![1, 0]
        );
        // Inside the outermost only.
        assert_eq!(hit_test_point_all(&m, Point::new(5.0, 5.0), 0.5), vec![0]);
        // Outside everything: empty, not a one-element "nearest" fallback.
        assert!(hit_test_point_all(&m, Point::new(500.0, 500.0), 0.5).is_empty());
    }

    /// **The invariant that makes cycling safe.** The topmost query must be
    /// exactly the head of the all-hits list, at every point and every
    /// tolerance — otherwise a first click and a cycling click would
    /// disagree about which objects are even candidates (module docs).
    ///
    /// Swept over a mixed page (paths of both fill rules, a stroke-only
    /// path, an `n` path, text) and a grid of points and tolerances, rather
    /// than asserted at one hand-picked point, because the divergence this
    /// guards against would most likely appear in exactly one kind's
    /// predicate.
    #[test]
    fn hit_test_point_agrees_with_the_head_of_hit_test_point_all() {
        let m = model(
            b"0 0 100 100 re f 40 40 20 20 re f* 10 90 m 90 90 l S \
              20 20 m 80 20 l n BT /F1 12 Tf 30 50 Td (Hi) Tj ET",
        );
        for tolerance in [0.0_f64, 0.5, 3.0, 25.0] {
            for x in (-10..=110).step_by(7) {
                for y in (-10..=110).step_by(7) {
                    let p = Point::new(f64::from(x), f64::from(y));
                    let all = hit_test_point_all(&m, p, tolerance);
                    assert_eq!(
                        hit_test_point(&m, p, tolerance),
                        all.first().copied(),
                        "point {p:?} tolerance {tolerance} disagreed: all = {all:?}"
                    );
                    // And the list is strictly descending (front-to-back),
                    // never merely "contains the right set".
                    assert!(
                        all.windows(2).all(|w| w[0] > w[1]),
                        "{all:?} is not front-most first"
                    );
                }
            }
        }
    }

    /// Cycling must be able to reach an object *completely* covered by
    /// another — the case a topmost-only query makes permanently
    /// unselectable, which is the whole reason this query was added.
    #[test]
    fn a_fully_covered_object_is_reachable_only_through_the_all_hits_query() {
        // A small rectangle entirely inside a larger one painted after it.
        let m = model(b"40 40 20 20 re f 0 0 100 100 re f");
        let inside = Point::new(50.0, 50.0);
        // Every click resolves to the cover; the covered object is
        // unreachable through the topmost query at ANY tolerance.
        for tolerance in [0.0_f64, 1.0, 10.0] {
            assert_eq!(hit_test_point(&m, inside, tolerance), Some(1));
        }
        // The all-hits query reaches it as the second step of the cycle.
        assert_eq!(hit_test_point_all(&m, inside, 0.5), vec![1, 0]);
    }
}
