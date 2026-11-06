//! Writing one saved 3D view's measurements
//! ([`EditSession::set_3d_view_measurements`]).

use super::threed_views::{
    COMMENT_DISCLOSURE, MEASURE_DISCLOSURE, REMOVED_COMMENT_DISCLOSURE, SHARED_DISCLOSURE,
    UNREAD_MEASURES_DISCLOSURE,
};
use super::{Command, CommandKind, EditError, EditSession, ObjectWrite};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::threed::{
    MeasureMerge, ThreeDEmbedError, ThreeDMeasurement, ThreeDViewMeasuresOutcome,
    check_new_measurements, merge_measurements, put_measurements, view_slots,
};

impl EditSession {
    /// Set the `/MA` of the saved view `view_index` of the `/3D` annotation
    /// `annot_id` on page `page_index` to `measurements`, leaving every other
    /// key of that view and every other view as they are (ISO 32000-2
    /// §13.6.4 Table 315, §13.6.7.3). One undo entry.
    ///
    /// `view_index` indexes [`crate::threed::saved_3d_views`]'s `views`, which
    /// skips a `/VA` entry that is not a dictionary. Exactly one object is
    /// written, and the annotation is not: the view when `/VA` refers to it
    /// (and to it only once), else an indirect `/VA` array, else the 3D
    /// stream, whose data does not change.
    ///
    /// A measurement equal to one read from the view keeps its original
    /// entry, so its comment link and any keys pdfcer does not read survive.
    /// An entry pdfcer cannot read is not in the caller's list and stays
    /// where it is. A list equal to what the view already holds writes
    /// nothing and makes no undo entry.
    ///
    /// [`ThreeDViewMeasuresOutcome::disclosures`] states that measurements
    /// are PDF 2.0 when any remain, a new measurement whose comment link is
    /// not written, a removed one that had a link, unreadable entries kept,
    /// and a stream shared through a `/3DRef`.
    ///
    /// # Errors
    ///
    /// As [`Self::set_3d_views`] for the annotation and the stream; then
    /// [`EditError::ThreeD`] with [`ThreeDEmbedError::ViewIndexOutOfRange`]
    /// or, for too many measurements or a new one missing a key its kind
    /// requires, [`ThreeDEmbedError::ViewInvalid`]. One the view already holds
    /// is not checked, because it is not rewritten.
    ///
    /// # Examples
    ///
    /// ```
    /// # use pdfcer_core::{edit::EditSession, object::ObjId};
    /// # use pdfcer_core::threed::ThreeDMeasurement;
    /// # fn demo(session: &mut EditSession, annot: ObjId) -> Result<(), Box<dyn std::error::Error>> {
    /// let m = ThreeDMeasurement::linear([0.0; 3], [3.0, 4.0, 0.0], [0.0, 0.0, 1.0], "mm")
    ///     .expect("a measurement");
    /// let outcome = session.set_3d_view_measurements(0, annot, 0, &[m])?;
    /// assert_eq!(outcome.measurements_after, 1);
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_3d_view_measurements(
        &mut self,
        page_index: usize,
        annot_id: ObjId,
        view_index: usize,
        measurements: &[ThreeDMeasurement],
    ) -> Result<ThreeDViewMeasuresOutcome, EditError> {
        let (_target, annot) = self.three_d_annot(page_index, annot_id)?;
        let (stream_id, shared_stream) = self.three_d_stream_id(&annot)?;
        let (write, merge) = self.view_measures_write(stream_id, view_index, measurements)?;
        let written = write.as_ref().map(|w| w.id);
        if let Some(write) = write {
            self.commit(Command {
                kind: CommandKind::SetThreeDViews,
                objects: vec![write],
                removals: Vec::new(),
                trailer: None,
            });
        }
        Ok(ThreeDViewMeasuresOutcome {
            annot_id,
            stream_id,
            view_index,
            written,
            measurements_before: merge.before,
            measurements_after: measurements.len(),
            unread_entries_kept: merge.unread,
            shared_stream,
            disclosures: disclosures(written.is_some(), &merge, shared_stream),
        })
    }

    /// The one write that gives view `view_index` these measurements, `None`
    /// when it already holds them, and how the `/MA` was merged.
    fn view_measures_write(
        &self,
        stream_id: ObjId,
        view_index: usize,
        measurements: &[ThreeDMeasurement],
    ) -> Result<(Option<ObjectWrite>, MeasureMerge), EditError> {
        let Some(Object::Stream(stream)) = self.value(stream_id) else {
            return Err(ThreeDEmbedError::NoThreeDStream.into());
        };
        let graph = self.graph();
        let va = stream.dict.get(b"VA");
        let list = va
            .map(|o| graph.resolve(o))
            .and_then(Object::as_array)
            .unwrap_or_default();
        let slots = view_slots(&graph, list);
        let found = slots.get(view_index).and_then(|slot| {
            let entry = list.get(*slot)?;
            Some((*slot, entry, graph.resolve(entry).as_dict()?))
        });
        let Some((slot, entry, view)) = found else {
            return Err(ThreeDEmbedError::ViewIndexOutOfRange {
                index: view_index,
                count: slots.len(),
            }
            .into());
        };
        let merge = merge_measurements(&graph, view, measurements);
        check_new_measurements(view_index, measurements, &merge.new)?;
        let current = view
            .get(b"MA")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_array)
            .unwrap_or_default();
        if merge.entries == current {
            return Ok((None, merge));
        }
        let mut new_view: Dict = view.clone();
        put_measurements(&mut new_view, merge.entries.clone());
        let only_once = list.iter().filter(|e| *e == entry).count() == 1;
        let (id, after) = match (entry, va) {
            (Object::Reference(id), _) if only_once => (*id, Object::Dict(new_view)),
            (_, Some(Object::Reference(array_id))) => {
                (*array_id, Object::Array(replaced(list, slot, new_view)))
            }
            _ => {
                let mut stream = stream.clone();
                let array = Object::Array(replaced(list, slot, new_view));
                stream.dict.insert(Name::from(b"VA"), array);
                (stream_id, Object::Stream(stream))
            }
        };
        let write = ObjectWrite {
            id,
            before: self.state.get(&id).cloned(),
            after: Some(after),
        };
        Ok((Some(write), merge))
    }
}

fn replaced(list: &[Object], slot: usize, view: Dict) -> Vec<Object> {
    let mut out = list.to_vec();
    if let Some(entry) = out.get_mut(slot) {
        *entry = Object::Dict(view);
    }
    out
}

fn disclosures(written: bool, merge: &MeasureMerge, shared: bool) -> Vec<String> {
    let says = [
        (!merge.entries.is_empty(), MEASURE_DISCLOSURE),
        (merge.lost_comment, COMMENT_DISCLOSURE),
        (merge.removed_comment, REMOVED_COMMENT_DISCLOSURE),
        (merge.unread > 0, UNREAD_MEASURES_DISCLOSURE),
        (shared, SHARED_DISCLOSURE),
    ];
    says.iter()
        .filter(|(applies, _)| written && *applies)
        .map(|(_, text)| (*text).to_owned())
        .collect()
}
