//! [`CommandKind`] by variant NAME for [`EditSession::export_history`], so a
//! history written by one build reads in another as long as every kind it
//! names still exists; a renamed or removed kind is refused by name.
//!
//! The variant list below is the single source for both directions. The
//! encoder's `match` is exhaustive, so a new `CommandKind` variant does not
//! compile until it is listed here.
//!
//! [`EditSession::export_history`]: super::EditSession::export_history

use super::history_codec::{Reader, Res, Writer};
use super::{AnnotKind, CommandKind, InfoField, InkEditKind, VertexEditKind};
use crate::dimension::DimensionEnd;
use crate::form_script::Trigger;
use crate::object::ObjId;

/// A value carried inside a [`CommandKind`] variant.
trait Field: Sized {
    fn put(&self, w: &mut Writer);
    fn get(r: &mut Reader<'_>) -> Res<Self>;
}

impl Field for usize {
    fn put(&self, w: &mut Writer) {
        w.usize(*self);
    }
    fn get(r: &mut Reader<'_>) -> Res<Self> {
        r.usize()
    }
}

impl Field for bool {
    fn put(&self, w: &mut Writer) {
        w.bool(*self);
    }
    fn get(r: &mut Reader<'_>) -> Res<Self> {
        r.bool()
    }
}

impl Field for i64 {
    fn put(&self, w: &mut Writer) {
        w.int(*self);
    }
    fn get(r: &mut Reader<'_>) -> Res<Self> {
        r.int()
    }
}

impl Field for i32 {
    fn put(&self, w: &mut Writer) {
        w.int(i64::from(*self));
    }
    fn get(r: &mut Reader<'_>) -> Res<Self> {
        let v = r.int()?;
        i32::try_from(v).or_else(|_| r.fail())
    }
}

impl Field for u16 {
    fn put(&self, w: &mut Writer) {
        w.uint(u64::from(*self));
    }
    fn get(r: &mut Reader<'_>) -> Res<Self> {
        let v = r.uint()?;
        u16::try_from(v).or_else(|_| r.fail())
    }
}

impl Field for ObjId {
    fn put(&self, w: &mut Writer) {
        w.id(*self);
    }
    fn get(r: &mut Reader<'_>) -> Res<Self> {
        r.id()
    }
}

/// `SetFieldScript::trigger`: one of [`Trigger::token`]'s spellings.
impl Field for &'static str {
    fn put(&self, w: &mut Writer) {
        w.bytes(self.as_bytes());
    }
    fn get(r: &mut Reader<'_>) -> Res<Self> {
        let raw = r.bytes()?;
        let all = [
            Trigger::Calculate,
            Trigger::Format,
            Trigger::Validate,
            Trigger::Keystroke,
        ];
        match all.iter().find(|t| t.token().as_bytes() == raw) {
            Some(t) => Ok(t.token()),
            None => r.fail(),
        }
    }
}

/// A fieldless enum carried by variant name.
macro_rules! named_enum {
    ($ty:ty: $($v:ident),* $(,)?) => {
        impl Field for $ty {
            fn put(&self, w: &mut Writer) {
                let name: &str = match self {
                    $(<$ty>::$v => stringify!($v),)*
                };
                w.bytes(name.as_bytes());
            }
            fn get(r: &mut Reader<'_>) -> Res<Self> {
                let raw = r.bytes()?;
                $(if raw == stringify!($v).as_bytes() {
                    return Ok(<$ty>::$v);
                })*
                r.fail()
            }
        }
    };
}

named_enum!(InfoField: Title, Author, Subject, Keywords);
named_enum!(VertexEditKind: Moved, Inserted, Removed);
named_enum!(
    InkEditKind: PointMoved,
    PointInserted,
    PointRemoved,
    StrokeReplaced,
    StrokeMoved,
    StrokeRemoved
);
named_enum!(
    AnnotKind: Square,
    Circle,
    Line,
    Ink,
    Polygon,
    PolyLine,
    Highlight,
    Underline,
    StrikeOut,
    Squiggly,
    FreeText,
    Text,
    Stamp,
    Redact,
    FileAttachment,
    Caret,
    ReplaceText,
    Sound,
    Screen,
    ThreeD,
    Link
);
named_enum!(DimensionEnd: A, B);

/// What [`read_kind`] found: a known kind, or the name of one this build
/// does not have.
pub(super) enum ReadKind {
    Known(CommandKind),
    Unknown(String),
}

macro_rules! kind_codec {
    ($($name:ident $({ $($f:ident : $t:ty),* })? $(($b:ident : $bt:ty))?,)*) => {
        /// Append `kind` as its variant name followed by its fields.
        pub(super) fn write_kind(w: &mut Writer, kind: CommandKind) {
            match kind {
                $(kind_codec!(@pat $name $({ $($f),* })? $(($b))?) => {
                    w.bytes(stringify!($name).as_bytes());
                    $($($f.put(w);)*)?
                    $($b.put(w);)?
                })*
            }
        }

        /// Read a kind written by [`write_kind`].
        pub(super) fn read_kind(r: &mut Reader<'_>) -> Res<ReadKind> {
            let raw = r.bytes()?;
            $(if raw == stringify!($name).as_bytes() {
                return Ok(ReadKind::Known(
                    kind_codec!(@build r $name $({ $($f : $t),* })? $(($bt))?),
                ));
            })*
            Ok(ReadKind::Unknown(String::from_utf8_lossy(&raw).into_owned()))
        }
    };
    (@pat $name:ident { $($f:ident),* }) => { CommandKind::$name { $($f),* } };
    (@pat $name:ident ($b:ident)) => { CommandKind::$name($b) };
    (@pat $name:ident) => { CommandKind::$name };
    (@build $r:ident $name:ident { $($f:ident : $t:ty),* }) => {
        CommandKind::$name { $($f: <$t as Field>::get($r)?),* }
    };
    (@build $r:ident $name:ident ($bt:ty)) => { CommandKind::$name(<$bt as Field>::get($r)?) };
    (@build $r:ident $name:ident) => { CommandKind::$name };
}

kind_codec! {
    AddOutlineItem,
    EditOutlineItem,
    AddNamedDestination,
    AdoptWidget,
    MergeDocument { count: usize },
    SetNamedPages,
    SetInfoField(field: InfoField),
    ClearInfoField(field: InfoField),
    SetPageRotation { page_index: usize, degrees: u16 },
    SetMediaBox { page_index: usize },
    SetMarkupStyle,
    ReshapeAnnotation { edit: VertexEditKind },
    RespanTextMarkup,
    SetLinkTarget,
    SetLinkBorder,
    ReplaceImage,
    DeskewImage,
    UpscaleImage,
    ReshapeInk { edit: InkEditKind, stroke: usize },
    AddSignatureField,
    SetMediaBoxes { count: usize },
    SetCropBoxes { count: usize },
    ResizePages { count: usize },
    ScalePages { count: usize },
    DeletePages { count: usize },
    InsertPages { count: usize },
    ReorderPages { count: usize },
    SetPageLabels { ranges: usize },
    ClearPageLabels,
    ReorderAnnotations { count: usize },
    SetPageTabs,
    RotatePages { count: usize, delta: i32 },
    AddAnnotation { kind: AnnotKind },
    AttachFile,
    DetachFile,
    AddFormField,
    PasteFormField,
    CutSelection,
    PasteOutlineItem,
    DeleteFormField,
    RenameFormField,
    MoveWidget,
    MoveWidgetToField,
    MoveAnnotation,
    ResizeAnnotation,
    SetMarkupNote,
    SetAnnotationOpen,
    SetAnnotationFlags,
    SetThreeDPoster,
    SetThreeDViews,
    SetTextAnnotStyle,
    SetAnnotOpacity,
    SetMarkerStyle,
    AddReviewState,
    RotateAnnotation,
    RotateDimension,
    EditFormField,
    EditWidget,
    FillTextField,
    ResetForm,
    RemoveMetadata,
    PurgePasswordValues,
    PromoteInlineDrFonts,
    AddValidationMaterial,
    SetButtonAction { removed: bool },
    SetFieldScript { trigger: &'static str, removed: bool },
    SetButtonState,
    SetChoiceValue,
    RegenerateAppearances { count: usize },
    BakeAppearances { count: usize },
    FlattenFields { count: usize },
    FlattenAnnotations { count: usize },
    DeleteRedactionMark,
    DeleteAnnotation,
    EditText,
    FormatText,
    ReflowBlock { lines_before: usize, lines_after: usize },
    EditBlockText { lines_before: usize, lines_after: usize },
    EditRecognisedText { words_changed: usize },
    AddText,
    StampBates,
    RemoveBates,
    StampRunningText,
    PlaceText { pages: usize },
    AddOcrLayer,
    RemoveOcrLayer,
    UnshareForm,
    AddDimension,
    MoveDimension,
    EditDimensionVertex { edit: VertexEditKind, index: usize },
    PlaceDimension,
    SetDimensionExtensionGap { end: DimensionEnd, cleared: bool },
    SetDimensionDisplay { show_diameter: bool },
    SetDimensionArea { area: bool },
    SetGroupStandard { members: usize },
    SetGroupStyle { members: usize },
    SetDimensionStyle { overrides: usize },
    RotateWidget { degrees: i64 },
    TurnWidget,
    SetDimensionLabel { overridden: bool },
    DeleteDimension,
    RenameDimensionGroup,
    DeleteDimensionGroup { members: usize },
    SetDimensionGroup,
    SetGroupScale { members: usize },
    ToggleDimensionLayer { visible: bool },
    SetLayerProperties { layer: ObjId },
    AddLayer { layer: ObjId },
    DeleteLayer { layer: ObjId },
    MergeLayers { target: ObjId },
    FlattenLayers,
    EditLayerOrder,
    SetAnnotationLayer { annot: ObjId },
    SetObjectsLayer { page_index: usize },
    MoveObject,
    PasteObjects,
    TransformObjects,
    DeleteObject,
    SetObjectPaint,
    SetObjectStrokeStyle,
    RestackObjects,
    MoveNode,
    MoveHandle,
    DeleteSubpath,
    DeleteTextRun,
    MoveSubpath,
    MoveTextRun,
    SplitTextObject,
    MergeTextRuns,
    DeleteNode,
    InsertNode,
    ConvertNode,
    ConvertSegment,
    AddImage,
    AddSvg,
    AddEmf,
    PlacePageContent,
    UnembedFonts { count: usize },
    EmbedFonts { count: usize },
    ImportEditable,
    Optimize { count: usize },
    MarkComparison,
}
