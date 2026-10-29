//! `/LS`, a 3D view's lighting scheme (ISO 32000-1 §13.6.4.5 Tables 309,
//! 310; PDF 1.7).

use crate::object::{Dict, Name, Object};

/// A lighting scheme's `/Subtype` (ISO 32000-1 Table 309).
///
/// [`Self::Artwork`] lights the model with the lights stored in it; every
/// other scheme is a fixed set of directional lights the reader supplies
/// (Table 310). A model with no lights of its own draws black under
/// `Artwork` in Acrobat, so a view over one should name another scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ThreeDLighting {
    /// `/Artwork`: the lights in the 3D artwork.
    Artwork,
    /// `/None`: no lights.
    None,
    /// `/White`: three white lights.
    White,
    /// `/Day`: three daylight-coloured lights.
    Day,
    /// `/Night`: three night-coloured lights.
    Night,
    /// `/Hard`: three hard white lights.
    Hard,
    /// `/Primary`: red, green and blue lights.
    Primary,
    /// `/Blue`: three blue lights.
    Blue,
    /// `/Red`: three red lights.
    Red,
    /// `/Cube`: six lights along the axes.
    Cube,
    /// `/CAD`: lights for CAD models.
    Cad,
    /// `/Headlamp`: one light from the camera.
    Headlamp,
}

const NAMES: [(ThreeDLighting, &[u8]); 12] = [
    (ThreeDLighting::Artwork, b"Artwork"),
    (ThreeDLighting::None, b"None"),
    (ThreeDLighting::White, b"White"),
    (ThreeDLighting::Day, b"Day"),
    (ThreeDLighting::Night, b"Night"),
    (ThreeDLighting::Hard, b"Hard"),
    (ThreeDLighting::Primary, b"Primary"),
    (ThreeDLighting::Blue, b"Blue"),
    (ThreeDLighting::Red, b"Red"),
    (ThreeDLighting::Cube, b"Cube"),
    (ThreeDLighting::Cad, b"CAD"),
    (ThreeDLighting::Headlamp, b"Headlamp"),
];

impl ThreeDLighting {
    /// The `/Subtype` name, as written.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::threed::ThreeDLighting;
    /// assert_eq!(ThreeDLighting::Cad.as_name(), b"CAD");
    /// assert_eq!(ThreeDLighting::from_name(b"Headlamp"), Some(ThreeDLighting::Headlamp));
    /// assert_eq!(ThreeDLighting::from_name(b"Disco"), None);
    /// ```
    #[must_use]
    pub fn as_name(self) -> &'static [u8] {
        NAMES
            .iter()
            .find(|(l, _)| *l == self)
            .map_or(b"Artwork", |(_, n)| n)
    }

    /// The scheme a `/Subtype` names, or `None` for a name Table 309 does
    /// not list (a reader then ignores the dictionary).
    #[must_use]
    pub fn from_name(name: &[u8]) -> Option<Self> {
        NAMES.iter().find(|(_, n)| *n == name).map(|(l, _)| *l)
    }

    /// The Table 309 dictionary.
    pub(crate) fn to_dict(self) -> Dict {
        let mut d = Dict::new();
        d.insert(
            Name::from(b"Type"),
            Object::Name(Name::from(b"3DLightingScheme")),
        );
        d.insert(
            Name::from(b"Subtype"),
            Object::Name(Name::from(self.as_name())),
        );
        d
    }
}
