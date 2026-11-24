//! The PDF/X requirement inventory (`PXQ-<axis>-n`): one row per
//! requirement, graded per level, with the ISO 15930 clause each level's
//! grade rests on. Rows, grades and clauses are those of the spec RAG's
//! `pdfx__req__by_axis.md`; titles are paraphrases.
//!
//! Grades: `A` ISO preview text or the GTS registry, `B` a level-explicit
//! secondary source, `P` an ISO 32000-1 rule binding every PDF, `K` only an
//! all-parts summary (reported at warning), `N` NOT SOURCED. A level absent
//! from a row's grades means the requirement does not apply to it.

use super::PdfxLevel;

/// How pdfcer answers a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum How {
    /// Checked once per document.
    Document,
    /// Checked on every page.
    Page,
    /// Answered by another row or by the page-tree read; no rule of its own.
    CoveredBy(&'static str),
    /// Checkable, but not checked yet: reported as `Incomplete`.
    NotYet,
    /// A reader-side rule or a scope note: nothing to test in a file.
    NoTest,
}

/// One requirement.
#[derive(Debug, Clone, Copy)]
pub(super) struct Row {
    /// `PXQ-FT-1`.
    pub(super) id: &'static str,
    /// Advisory ("should"): warns, never fails.
    pub(super) advisory: bool,
    /// A machine cannot decide it; findings are `NeedsPerson`.
    pub(super) person: bool,
    /// `1a01:A 1a03:B ...`.
    grades: &'static str,
    /// `;`-separated `P:clause`, P the ISO 15930 part (1, 3, 4, 7) or 0 for
    /// ISO 32000-1. A part's clause belongs to its own level (1 = X-1a:2001,
    /// 3 = X-3:2002, 4 = X-1a:2003, 7 = X-4; 0 = every level) unless an
    /// `@levels` suffix names others. ISO 15930-6:2003 (X-3:2003) has no
    /// sourced clause numbers, so no clause is ever shown for it.
    clauses: &'static str,
    pub(super) how: How,
    /// What is required, in a few words.
    pub(super) title: &'static str,
}

/// Where a row stands for one level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
    /// Checked by pdfcer.
    Checked,
    /// NOT SOURCED for this level.
    NotSourced,
    /// Sourced but not checked yet.
    NotYet,
}

impl Row {
    fn parse(line: &'static str) -> Option<Self> {
        let mut f = line.split('|');
        let id = f.next()?;
        let advisory = f.next()? == "R";
        let person = f.next()? == "H";
        let grades = f.next()?;
        let clauses = f.next()?;
        let how = match f.next()? {
            "D" => How::Document,
            "P" => How::Page,
            "Y" => How::NotYet,
            other => other.strip_prefix("C:").map_or(How::NoTest, How::CoveredBy),
        };
        Some(Self {
            id,
            advisory,
            person,
            grades,
            clauses,
            how,
            title: f.next()?,
        })
    }

    /// This level's grade letter, or `None` when the row does not apply.
    pub(super) fn grade(&self, level: PdfxLevel) -> Option<char> {
        self.grades.split(' ').find_map(|g| {
            let (code, grade) = g.split_once(':')?;
            if code == level.code() {
                grade.chars().next()
            } else {
                None
            }
        })
    }

    /// How the row stands for `level`; `None` when it yields no rule.
    pub(super) fn state(&self, level: PdfxLevel) -> Option<State> {
        let grade = self.grade(level)?;
        match self.how {
            How::CoveredBy(_) | How::NoTest => None,
            _ if grade == 'N' => Some(State::NotSourced),
            How::NotYet => Some(State::NotYet),
            How::Document | How::Page => Some(State::Checked),
        }
    }

    /// The clauses `level`'s grade rests on, as text. A level with no
    /// ISO 15930 clause says so rather than borrowing another part's.
    pub(super) fn clause(&self, level: PdfxLevel) -> String {
        let mut own = Vec::new();
        let mut base = Vec::new();
        for entry in self.clauses.split(';').filter(|e| !e.is_empty()) {
            let (head, levels) = entry.split_once('@').unwrap_or((entry, ""));
            let Some((part, clause)) = head.split_once(':') else {
                continue;
            };
            let applies = if levels.is_empty() {
                part == "0" || part == level.part_digit()
            } else {
                levels.split(' ').any(|l| l == level.code())
            };
            if !applies {
                continue;
            }
            if part == "0" {
                base.push(format!("ISO 32000-1 {clause}"));
            } else {
                own.push(format!("{} {clause}", part_name(part)));
            }
        }
        let mut out = if own.is_empty() {
            format!("ISO 15930 clause NOT SOURCED for {}", level.claim())
        } else {
            own.join(", ")
        };
        for c in base {
            out.push_str("; ");
            out.push_str(&c);
        }
        out
    }
}

fn part_name(digit: &str) -> &'static str {
    match digit {
        "1" => "ISO 15930-1:2001",
        "3" => "ISO 15930-3:2002",
        "4" => "ISO 15930-4:2003",
        _ => "ISO 15930-7:2010",
    }
}

/// Every row, in inventory order.
pub(super) fn all() -> impl Iterator<Item = Row> {
    TABLE.lines().filter_map(Row::parse)
}

/// The row with `id`.
pub(super) fn find(id: &str) -> Option<Row> {
    all().find(|r| r.id == id)
}

/// `id|F or R|M or H|grades|clauses|how|title`; how is `D` document, `P`
/// page, `Y` not yet, `-` no test, `C:x` covered by x.
const TABLE: &str = "\
PXQ-ID-1|F|M|1a01:A 1a03:A 3-02:A 3-03:A|4:§5@1a01;4:§6.7@1a03|D|the Info GTS_PDFXVersion names the level\n\
PXQ-ID-2|R|M|1a03:A 3-02:A 3-03:B|3:§5;4:§5|-|the header version does not decide conformance\n\
PXQ-ID-3|F|M|4:B|7:§6.11|D|XMP metadata with pdfxid:GTS_PDFXVersion PDF/X-4\n\
PXQ-ID-4|F|M|4:B|7:§6.10|D|Info values match the XMP\n\
PXQ-ID-5|F|M|4:B|7:§6.10|Y|the required XMP properties are present\n\
PXQ-ID-6|F|M|1a01:B 1a03:B 3-02:B 3-03:B||D|Info Title, CreationDate and ModDate are present\n\
PXQ-ID-7|F|M|1a01:B 1a03:B 3-02:B 3-03:B||-|XMP metadata is optional before PDF/X-4\n\
PXQ-ID-8|R|M|1a03:B 3-03:B||D|private Info values are text strings\n\
PXQ-ID-9|R|M|1a01:A|1:§5|D|CGATS.12/1-1999 is the predecessor, not PDF/X-1a:2001\n\
PXQ-OI-1|F|M|1a01:B 1a03:B 3-02:A 3-03:B 4:B|3:§6.2.2;7:§6.4|D|a GTS_PDFX output intent is present\n\
PXQ-OI-2|F|M|1a01:N 1a03:N 3-02:A 3-03:N 4:N|3:§6.2.2|D|exactly one GTS_PDFX output intent\n\
PXQ-OI-3|F|M|1a01:P 1a03:P 3-02:A 3-03:P 4:P|3:§6.2.2;0:Table 365|D|the output intent has an OutputConditionIdentifier\n\
PXQ-OI-4|F|M|1a01:N 1a03:B 3-02:A 3-03:B 4:N|3:§6.2.2|Y|a registered condition matches the ICC registry and has a RegistryName\n\
PXQ-OI-5|F|M|1a01:N 1a03:B 3-02:A 3-03:B 4:N|3:§6.2.2|Y|an unregistered condition embeds a DestOutputProfile\n\
PXQ-OI-6|F|M|3-02:A 3-03:B|3:§6.2.2|D|CIE-based colour requires a DestOutputProfile\n\
PXQ-OI-7|F|M|4:B|7:§6.4|D|the output intent embeds its DestOutputProfile\n\
PXQ-OI-8|F|M|1a01:N 1a03:N 3-02:A 3-03:N 4:N|3:§6.2.2|D|the DestOutputProfile is an output-device (prtr) profile\n\
PXQ-OI-9|F|M|1a01:N 1a03:B 3-02:N 3-03:N 4:N||Y|an unregistered X-1a condition's profile has an AtoB1 tag\n\
PXQ-OI-10|R|M|3-02:A|3:§6.2.2|-|readers ignore the profile's Alternate\n\
PXQ-CS-1|F|M|1a01:A 1a03:B|1:§6.3.1|P|only CMYK, gray and spot colour\n\
PXQ-CS-2|F|H|1a01:A 1a03:N 3-02:A 3-03:N 4:N|1:§6.3.1;3:§6.2.1|D|tone values are prepared for the one printing condition\n\
PXQ-CS-3|F|M|3-02:A 3-03:B 4:B|3:§6.2.1|-|colorimetric spaces are permitted\n\
PXQ-CS-4|F|M|3-02:A 3-03:B 4:B|3:§6.2.3|Y|DeviceCMYK with a non-CMYK intent has a DefaultCMYK\n\
PXQ-CS-5|F|M|3-02:A 3-03:B 4:B|3:§6.2.4|Y|DeviceGray with a non-CMYK, non-gray intent has a DefaultGray\n\
PXQ-CS-6|F|M|3-02:N 3-03:B 4:B||Y|DeviceRGB with a non-RGB intent has a DefaultRGB\n\
PXQ-CS-7|F|M|1a01:N 1a03:N 3-02:N 3-03:B 4:N||Y|the colour rules apply to alternate and base spaces\n\
PXQ-CS-8|F|M|1a01:K 1a03:K 3-02:K 3-03:K 4:K||D|a spot ink has the same alternate everywhere\n\
PXQ-CS-9|F|M|1a01:K 1a03:K 3-02:K 3-03:K 4:K|4:§6.9;7:§6.13|D|no transfer functions\n\
PXQ-CS-10|F|M|1a01:A 1a03:A 3-02:A 3-03:N 4:N|1:§6.2;3:§6.1;4:§6.1|P|no pre-separated pages\n\
PXQ-CS-11|F|M|1a03:B 3-03:B||C:PXQ-CS-1|printer's marks follow the colour rules\n\
PXQ-TR-1|F|M|1a03:A 3-03:B|4:§6.16|P|no transparency\n\
PXQ-TR-2|F|M|1a01:A 3-02:A|3:Annex D|P|transparency post-dates the PDF 1.3 base\n\
PXQ-TR-3|F|M|4:A|7:Introduction|-|PDF/X-4 permits transparency\n\
PXQ-TR-4|F|M|4:N|7:§6.20|Y|constraints on transparency in PDF/X-4\n\
PXQ-FT-1|F|M|1a01:A 1a03:B 3-02:A 3-03:B 4:B|1:§6.2;3:§6.1;4:§6.3;7:§6.5|D|every font is embedded\n\
PXQ-FT-2|R|M|1a03:B 3-03:B||-|readers use the embedded font\n\
PXQ-FT-3|F|M|1a03:N 4:N|4:§6.3;7:§6.5|Y|further font constraints\n\
PXQ-FL-1|F|M|1a01:K 1a03:B 3-02:K 3-03:B 4:K|4:§6.5;7:§6.8|D|no LZW compression\n\
PXQ-FL-2|F|M|1a03:B 3-03:B|4:§6.5|D|no JBIG2 compression\n\
PXQ-FL-3|F|M|1a03:B 3-03:B|4:§6.5|D|the only lossy compression is JPEG\n\
PXQ-FL-4|F|H|1a03:B 3-03:B||D|JPEG only on continuous-tone images\n\
PXQ-FL-5|R|M|1a03:B 3-03:B||D|no JPEG 2000 and no 16-bit images\n\
PXQ-FL-6|F|M|4:N|7:§6.27|Y|constraints on JPEG 2000 images\n\
PXQ-BX-1|F|M|1a01:P 1a03:B 3-02:P 3-03:B 4:P|4:§6.8;7:§6.12;0:Table 30|C:the page-tree read|every page has a MediaBox\n\
PXQ-BX-2|F|M|1a01:B 1a03:B 3-02:B 3-03:B 4:B|4:§6.8;7:§6.12|P|every page has a TrimBox or an ArtBox\n\
PXQ-BX-3|F|M|1a01:K 1a03:B 3-02:K 3-03:B 4:K|4:§6.8;7:§6.12|P|the trim or art box lies inside the bleed box\n\
PXQ-BX-4|F|M|1a03:B 3-03:B|4:§6.8|P|the trim or art box lies inside the crop box\n\
PXQ-BX-5|R|M|1a03:B 3-03:B|4:§6.8|P|a TrimBox is preferred to an ArtBox\n\
PXQ-EN-1|F|M|1a01:B 1a03:B 3-02:B 3-03:B 4:B|4:§6.11;7:§6.15|D|the file is not encrypted\n\
PXQ-AN-1|F|M|1a01:B 1a03:B 3-02:N 3-03:B 4:K|4:§6.13;7:§6.17|P|annotations lie outside the printed area\n\
PXQ-AN-2|F|M|1a03:B 3-03:B|4:§6.13|P|printer's marks lie outside the trim box\n\
PXQ-AN-3|F|M|1a01:K 1a03:K 3-02:K 3-03:K 4:K|7:§6.17|P|no sound, movie, screen, rich-media or 3D annotations\n\
PXQ-AN-4|R|M|1a01:A 1a03:A 3-02:A|1:§5;3:§5;4:§5|-|readers may ignore the Print flag\n\
PXQ-AJ-1|F|M|1a01:B 1a03:B 3-02:B 3-03:B 4:B|4:§6.14;7:§6.18|D|no JavaScript\n\
PXQ-AJ-2|F|M|1a01:N 1a03:N 3-02:N 3-03:N 4:N|4:§6.14;7:§6.18|Y|which other actions are prohibited\n\
PXQ-AJ-3|F|M|1a01:K 1a03:K 3-02:K 3-03:K 4:K|7:§6.26|D|no form fields\n\
PXQ-TP-1|F|M|1a01:B 1a03:B 3-02:B 3-03:B 4:B|4:§6.6;7:§6.9|D|Trapped is the name True or False\n\
PXQ-TP-2|R|M|1a03:A 3-03:B|4:§5 EXAMPLE 1|-|readers honour the trapping state\n\
PXQ-ST-1|F|M|1a01:A 1a03:K 3-02:A 3-03:K 4:B|1:§6.2;3:§6.1;4:§6.4;7:§6.7|D|nothing is outside the file\n\
PXQ-ST-2|F|M|1a01:B 1a03:B 3-02:B 3-03:B|4:§6.4|D|no OPI\n\
PXQ-ST-3|F|M|1a03:B 3-03:B||D|no reference XObjects\n\
PXQ-ST-4|F|M|1a03:B 3-03:B 4:N|4:§6.10;7:§6.14|D|no PostScript XObjects\n\
PXQ-ST-5|F|M|1a01:K 1a03:K 3-02:K 3-03:K 4:K|7:§6.7|D|no embedded files\n\
PXQ-ST-6|R|H|1a03:B 3-03:B|4:§6.12|D|alternate images are kept in step with their base image\n\
PXQ-ST-7|F|H|1a01:A 1a03:B 3-02:A 3-03:B|1:§5;3:§5|D|other features do not affect the print\n\
PXQ-ST-8|R|M|1a01:A 1a03:A 3-02:A 3-03:A 4:A|7:Introduction Table 1@1a01 1a03 3-02 3-03 4|D|no feature newer than the level's PDF version\n\
PXQ-ST-9|R|M|1a01:N 1a03:B 3-02:A 3-03:B 4:N|3:§5|C:PXQ-ST-8|newer features should not be used\n\
PXQ-ST-10|F|M|1a01:N 1a03:N 3-02:N 3-03:N||Y|no optional content before PDF/X-4\n\
PXQ-ST-11|F|M|4:N|7:§6.24|Y|constraints on optional content in PDF/X-4\n\
PXQ-ST-12|F|M|1a01:N|1:§6.5, §6.13, §6.17|Y|the further PDF/X-1a:2001 restrictions\n\
PXQ-ST-13|F|M|3-02:N|3:§6.3–§6.16|Y|the remaining PDF/X-3:2002 clauses\n\
PXQ-ST-14|F|M|1a03:N|4:§6.15, §6.17|Y|BX/EX operators and viewer preferences\n\
PXQ-ST-15|F|M|4:N|7:§6.2, §6.3, §6.6, §6.9, §6.13, §6.16, §6.19, §6.21, §6.22, §6.23, §6.25, §6.26|Y|the remaining PDF/X-4 clauses\n\
PXQ-ST-16|F|M|3-03:N||-|PDF/X-3:2003 has no sourced clause numbers";
