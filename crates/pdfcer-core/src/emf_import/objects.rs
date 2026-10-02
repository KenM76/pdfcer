//! The object table: create, select and delete pens, brushes and fonts
//! ([MS-EMF] §2.3.7 object creation, §2.3.8 object manipulation, §2.1.31
//! stock objects).

use std::collections::HashMap;

use super::EmfImportNotes;
use super::dc::{Brush, Dc, Font, Pen, colorref};
use super::reader::Rec;

/// A created object. Selection copies it into the DC, so deleting an
/// object that is still selected leaves the DC drawing with it, as GDI
/// does (a selected object cannot be deleted).
#[derive(Debug, Clone)]
enum Obj {
    Pen(Pen),
    Brush(Brush),
    Font(Font),
}

#[derive(Debug, Default)]
pub(super) struct Objects {
    table: HashMap<u32, Obj>,
}

const STOCK: u32 = 0x8000_0000;

impl Objects {
    /// Play an object record; `false` when `rec` is not one.
    pub(super) fn play(&mut self, rec: &Rec<'_>, dc: &mut Dc, notes: &mut EmfImportNotes) -> bool {
        let ih = rec.u32(8).unwrap_or(0);
        match rec.kind {
            0x25 => self.select(ih, dc, notes),
            0x28 => {
                self.table.remove(&ih);
            }
            0x26 => self.insert(ih, create_pen(rec).map(Obj::Pen)),
            0x5F => {
                let pen = ext_create_pen(rec, notes);
                self.insert(ih, pen.map(Obj::Pen));
            }
            0x27 => {
                let brush = create_brush(rec, notes);
                self.insert(ih, brush.map(Obj::Brush));
            }
            0x52 => self.insert(ih, create_font(rec).map(Obj::Font)),
            // Pattern brushes: the slot exists, and fills with it draw
            // nothing (counted here, where the pattern is lost).
            0x5D | 0x5E => {
                notes.skip(&format!(
                    "{} (fills drawn empty)",
                    super::draw::name(rec.kind)
                ));
                self.insert(ih, Some(Obj::Brush(None)));
            }
            _ => return false,
        }
        true
    }

    fn insert(&mut self, ih: u32, obj: Option<Obj>) {
        if ih & STOCK != 0 || ih == 0 {
            return;
        }
        match obj {
            Some(o) => {
                self.table.insert(ih, o);
            }
            None => {
                self.table.remove(&ih);
            }
        }
    }

    fn select(&self, ih: u32, dc: &mut Dc, notes: &mut EmfImportNotes) {
        if ih & STOCK != 0 {
            select_stock(ih & !STOCK, dc, notes);
            return;
        }
        match self.table.get(&ih) {
            Some(Obj::Pen(p)) => dc.pen = p.clone(),
            Some(Obj::Brush(b)) => dc.brush = *b,
            Some(Obj::Font(f)) => dc.font = f.clone(),
            // Palettes, colour spaces, or an index never created.
            None => {}
        }
    }
}

fn select_stock(n: u32, dc: &mut Dc, notes: &mut EmfImportNotes) {
    let grey = |v: u8| Some([v, v, v]);
    match n {
        0 | 0x12 => dc.brush = grey(255),
        1 => dc.brush = grey(192),
        2 => dc.brush = grey(128),
        3 => dc.brush = grey(64),
        4 => dc.brush = grey(0),
        5 => dc.brush = None,
        6 => dc.pen = Pen::solid([255, 255, 255]),
        7 | 0x13 => dc.pen = Pen::solid([0, 0, 0]),
        8 => dc.pen = Pen::null(),
        10 | 11 | 16 => {
            dc.font = Font {
                pitch_family: 1,
                ..Font::default()
            };
        }
        12..=14 | 17 => dc.font = Font::default(),
        // DEFAULT_PALETTE: no paint.
        15 => {}
        _ => notes.skip("unknown stock object"),
    }
}

/// EMR_CREATEPEN (§2.3.7.7): style @12, width.x @16, colour @24. A width
/// above 1 is in logical units (Win32 CreatePen); 0 and 1 are one pixel.
fn create_pen(rec: &Rec<'_>) -> Option<Pen> {
    let style = rec.u32(12)?;
    let width = f64::from(rec.i32(16)?);
    Some(Pen {
        style,
        width,
        color: Some(colorref(rec, 24)?),
        geometric: style & 0x000F_0000 == 0x0001_0000 || width > 1.0,
        dashes: Vec::new(),
    })
}

/// EMR_EXTCREATEPEN (§2.3.7.9) with its LogPenEx @28 (§2.2.20).
fn ext_create_pen(rec: &Rec<'_>, notes: &mut EmfImportNotes) -> Option<Pen> {
    let style = rec.u32(28)?;
    let width = f64::from(rec.u32(32)?);
    let brush_style = rec.u32(36)?;
    let n = rec.u32(48)? as usize;
    let mut dashes = Vec::new();
    if style & 0x0F == 7 {
        for i in 0..n.min(64) {
            dashes.push(f64::from(rec.u32(52 + i * 4)?));
        }
    }
    let color = match brush_style {
        0 => Some(colorref(rec, 40)?),
        1 => None,
        2 => {
            notes.approximate("hatched pen (drawn solid)");
            Some(colorref(rec, 40)?)
        }
        _ => {
            notes.skip("pattern pen (drawn solid black)");
            Some([0, 0, 0])
        }
    };
    Some(Pen {
        style,
        width,
        color,
        geometric: style & 0x000F_0000 == 0x0001_0000,
        dashes,
    })
}

/// EMR_CREATEBRUSHINDIRECT (§2.3.7.1): LogBrushEx @12.
fn create_brush(rec: &Rec<'_>, notes: &mut EmfImportNotes) -> Option<Brush> {
    let style = rec.u32(12)?;
    let color = colorref(rec, 16)?;
    Some(match style {
        0 => Some(color),
        1 => None,
        2 => {
            notes.approximate("hatched brush (drawn solid)");
            Some(color)
        }
        _ => {
            notes.skip("pattern brush (fills drawn empty)");
            None
        }
    })
}

/// EMR_EXTCREATEFONTINDIRECTW (§2.3.7.8): LogFont @12 (§2.2.13).
fn create_font(rec: &Rec<'_>) -> Option<Font> {
    let face: Vec<u16> = (0..32)
        .map_while(|i| rec.u16(40 + i * 2))
        .take_while(|&u| u != 0)
        .collect();
    Some(Font {
        height: f64::from(rec.i32(12)?),
        escapement: f64::from(rec.i32(20)?),
        weight: rec.i32(28)?,
        italic: rec.u8(32)? != 0,
        pitch_family: rec.u8(39)?,
        face: String::from_utf16_lossy(&face),
        stock: false,
    })
}
